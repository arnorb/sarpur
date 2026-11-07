"""JSON Lines worker for ruvsarpur (`python ruvsarpur.py --json <command> ...`).

Writes one JSON object per line to stdout and nothing else. Everything that
ruvsarpur functions print goes to stderr. The worker never downloads video
files and never writes prevrecorded.log.
"""

import argparse
import contextlib
import datetime as real_datetime
import json
import sys
import types
from typing import Any, Callable, Dict, Iterator, List, Optional, TextIO
from unittest import mock

import ruvsarpur


class WorkerError(Exception):
    pass


class UsageError(WorkerError):
    pass


class _WorkerParser(argparse.ArgumentParser):
    def __init__(self, *args: Any, **kwargs: Any) -> None:
        kwargs.setdefault("add_help", False)  # help text would pollute stdout
        super().__init__(*args, **kwargs)

    def error(self, message: str) -> None:  # type: ignore[override]
        raise UsageError(message)


def _json_default(value: Any) -> Any:
    if isinstance(value, (real_datetime.datetime, real_datetime.date)):
        return value.isoformat()
    raise TypeError(f"Object of type {type(value).__name__} is not JSON serializable")


def _emit(out: TextIO, obj: Any) -> None:
    out.write(json.dumps(obj, ensure_ascii=False, default=_json_default) + "\n")
    out.flush()


def _frozen_datetime_module(now: real_datetime.datetime) -> types.SimpleNamespace:
    """A stand-in for the `datetime` module whose today()/now() return `now`."""

    class FrozenDate(real_datetime.date):
        @classmethod
        def today(cls) -> "FrozenDate":
            return cls(now.year, now.month, now.day)

    class FrozenDateTime(real_datetime.datetime):
        @classmethod
        def now(cls, tz: Optional[real_datetime.tzinfo] = None) -> "FrozenDateTime":
            value = now if tz is None else now.astimezone(tz)
            return cls(value.year, value.month, value.day, value.hour, value.minute,
                       value.second, value.microsecond, value.tzinfo)

        @classmethod
        def today(cls) -> "FrozenDateTime":
            return cls.now()

    namespace = types.SimpleNamespace(**{k: getattr(real_datetime, k) for k in dir(real_datetime) if not k.startswith("__")})
    namespace.date = FrozenDate
    namespace.datetime = FrozenDateTime
    return namespace


@contextlib.contextmanager
def _frozen_clock(now_text: Optional[str]) -> Iterator[None]:
    if not now_text:
        yield
        return
    try:
        now = real_datetime.datetime.fromisoformat(now_text)
    except ValueError as ex:
        raise UsageError(f"invalid --now value '{now_text}': {ex}")
    fake = _frozen_datetime_module(now)
    # ruvsarpur.getVodSeriesSchedule uses a bare `date.today()` that is not
    # imported there; create it under the patch so it follows the frozen clock.
    with mock.patch.object(ruvsarpur, "datetime", fake), \
            mock.patch.object(ruvsarpur, "date", fake.date, create=True):
        yield


def _schedule_path(portable: bool) -> str:
    return ruvsarpur.createFullConfigFileName(portable, ruvsarpur.TV_SCHEDULE_LOG_FILE)


def _load_schedule(portable: bool) -> Optional[Dict[str, Any]]:
    return ruvsarpur.getExistingTvSchedule(_schedule_path(portable))


def _items(schedule: Dict[str, Any]) -> Iterator[Dict[str, Any]]:
    for key, value in schedule.items():
        if key == "date" or not isinstance(value, dict):
            continue
        yield value


def _require_item(portable: bool, pid: str) -> Dict[str, Any]:
    schedule = _load_schedule(portable)
    item = schedule.get(pid) if schedule else None
    if not isinstance(item, dict):
        raise WorkerError(f"pid {pid} not found in the schedule")
    return item


def _load_imdb_cache(portable: bool) -> Dict[str, Any]:
    cache = ruvsarpur.getExistingJsonFile(
        ruvsarpur.createFullConfigFileName(portable, ruvsarpur.IMDB_CACHE_FILE))
    return cache if isinstance(cache, dict) else {}


def cmd_catalogue(args: argparse.Namespace, out: TextIO) -> None:
    schedule = _load_schedule(args.portable)
    if args.refresh or schedule is None:
        if not args.refresh:
            raise WorkerError("no existing schedule, run catalogue --refresh")
        today = ruvsarpur.datetime.date.today()
        imdb_titles = ruvsarpur.loadImdbOriginalTitles(args.imdbfolder)
        imdb_cache_file = ruvsarpur.createFullConfigFileName(args.portable, ruvsarpur.IMDB_CACHE_FILE)
        imdb_cache = _load_imdb_cache(args.portable)
        if schedule is None or not args.incremental or schedule["date"].date() < today:
            schedule = {}
        schedule = ruvsarpur.getVodSchedule(schedule, len(schedule) > 0, imdb_cache, imdb_titles)
        if len(schedule) > 1:
            ruvsarpur.saveCurrentTvSchedule(schedule, _schedule_path(args.portable))
        if len(imdb_cache) > 0:
            ruvsarpur.saveImdbCache(imdb_cache, imdb_cache_file)
    for item in _items(schedule):
        _emit(out, item)


def cmd_series(args: argparse.Namespace, out: TextIO) -> None:
    imdb_cache = _load_imdb_cache(args.portable)
    episodes = ruvsarpur.getVodSeriesSchedule(args.sid, None, imdb_cache, None)
    for item in _items(episodes or {}):
        _emit(out, item)


def _resolve_playlist(item: Dict[str, Any], quality: str) -> Dict[str, Any]:
    """Find the playlist for one schedule item, or raise WorkerError.

    Items without a full ruv-vod URL used to be looked up with RÚV's
    getProgramType GraphQL query, which RÚV no longer answers (spike 2), so
    they fail here with a clear message instead.
    """
    file_url = item.get("file")
    if not file_url or not str(file_url).startswith(ruvsarpur.RUV_URL):
        raise WorkerError(
            f"pid {item.get('pid')} has no {ruvsarpur.RUV_URL} video address "
            f"(file={file_url!r}); it can't be downloaded")
    item["vod_url_full"] = file_url
    item["vod_url"] = ruvsarpur.getGroup(ruvsarpur.RE_VOD_BASE_URL, "vodbase", file_url)
    display_title = ruvsarpur.createShowTitle(item, False)
    playlist = ruvsarpur.find_m3u8_playlist_url(item, display_title, quality)
    if not playlist:
        raise WorkerError(f"could not resolve a playlist for pid {item.get('pid')}")
    return playlist


def cmd_resolve(args: argparse.Namespace, out: TextIO) -> None:
    item = dict(_require_item(args.portable, args.pid))
    playlist = _resolve_playlist(item, args.quality)
    result: Dict[str, Any] = {
        "pid": args.pid,
        "playlist_url": playlist["url"],
        "audio_url": playlist.get("audio_url"),
        "quality": playlist.get("quality"),
    }
    if "fragments" in playlist:
        result["fragments"] = playlist["fragments"]
    result["subtitles"] = item.get("subtitles")
    _emit(out, result)


def _content_type(item: Dict[str, Any]) -> str:
    # Same rule as gui_api._content_type_from_item.
    if item.get("is_movie") or item.get("is_docu"):
        return "movie_or_docu"
    if item.get("is_sport"):
        return "sport"
    return "show"


def _duration_seconds(item: Dict[str, Any]) -> Optional[int]:
    try:
        return int(float(item["duration"]))
    except (KeyError, TypeError, ValueError):
        return None


def cmd_plan(args: argparse.Namespace, out: TextIO) -> None:
    """Everything the server needs to download one episode, in one call."""
    item = dict(_require_item(args.portable, args.pid))
    plex = args.mode == "plex"
    relative_path = ruvsarpur.createLocalFileName(item, False, plex)
    # download_m3u8_playlist_using_ffmpeg appends .mp4 the same way.
    if not relative_path.endswith(".mp4"):
        relative_path += ".mp4"
    metadata = ruvsarpur.buildFfmpegMetadata(item)
    playlist = _resolve_playlist(item, args.quality)
    result: Dict[str, Any] = {
        "pid": args.pid,
        "sid": str(item.get("sid")),
        "title": ruvsarpur.createShowTitle(item, False),
        "relative_path": relative_path,
        "content_type": _content_type(item),
        "playlist_url": playlist["url"],
        "audio_url": playlist.get("audio_url"),
        "quality": playlist.get("quality"),
        "duration_seconds": _duration_seconds(item),
        "metadata": metadata,
        "subtitles": item.get("subtitles"),
    }
    if "fragments" in playlist:
        result["fragments"] = playlist["fragments"]
    _emit(out, result)


def cmd_name(args: argparse.Namespace, out: TextIO) -> None:
    item = _require_item(args.portable, args.pid)
    plex = args.mode == "plex"
    _emit(out, {
        "pid": args.pid,
        "path": ruvsarpur.createLocalFileName(item, args.originaltitle, plex, args.suffix),
        "title": ruvsarpur.createShowTitle(item, args.originaltitle, plex),
    })


def cmd_imdb(args: argparse.Namespace, out: TextIO) -> None:
    schedule = _load_schedule(args.portable)
    imdb = None
    for item in _items(schedule or {}):
        if str(item.get("sid")) == str(args.sid) and item.get("imdb"):
            imdb = item["imdb"]
            break
    if imdb is None:
        entry = _load_imdb_cache(args.portable).get(str(args.sid))
        if isinstance(entry, dict):
            imdb = entry.get("imdb")
    _emit(out, {"sid": str(args.sid), "imdb": imdb})


COMMANDS: Dict[str, Callable[[argparse.Namespace, TextIO], None]] = {
    "catalogue": cmd_catalogue,
    "series": cmd_series,
    "resolve": cmd_resolve,
    "plan": cmd_plan,
    "name": cmd_name,
    "imdb": cmd_imdb,
}


def build_parser() -> argparse.ArgumentParser:
    parser = _WorkerParser(prog="ruvsarpur.py --json")
    common = argparse.ArgumentParser(add_help=False)
    common.add_argument("--portable", action="store_true", default=argparse.SUPPRESS)
    common.add_argument("--now", default=argparse.SUPPRESS, help="ISO datetime that freezes today/now")
    parser.add_argument("--portable", action="store_true")
    parser.add_argument("--now", default=None)
    sub = parser.add_subparsers(dest="command", required=True, parser_class=_WorkerParser)

    p = sub.add_parser("catalogue", parents=[common])
    p.add_argument("--incremental", action="store_true")
    p.add_argument("--refresh", action="store_true")
    p.add_argument("--imdbfolder", default=None)

    p = sub.add_parser("series", parents=[common])
    p.add_argument("sid")

    p = sub.add_parser("resolve", parents=[common])
    p.add_argument("pid")
    p.add_argument("--quality", "-q", default="HD1080", choices=list(ruvsarpur.QUALITY_BITRATE.keys()))

    p = sub.add_parser("plan", parents=[common])
    p.add_argument("pid")
    p.add_argument("--mode", choices=["web", "plex"], required=True)
    p.add_argument("--quality", "-q", default="HD1080", choices=list(ruvsarpur.QUALITY_BITRATE.keys()))

    p = sub.add_parser("name", parents=[common])
    p.add_argument("pid")
    p.add_argument("--mode", choices=["web", "plex"], required=True)
    p.add_argument("--originaltitle", action="store_true")
    p.add_argument("--suffix", default="")

    p = sub.add_parser("imdb", parents=[common])
    p.add_argument("sid")
    return parser


def main(argv: Optional[List[str]] = None) -> int:
    out = sys.stdout
    argv = list(sys.argv[1:] if argv is None else argv)
    try:
        args = build_parser().parse_args(argv)
        # Anything ruvsarpur prints must not reach the JSON stream.
        with contextlib.redirect_stdout(sys.stderr), _frozen_clock(args.now):
            COMMANDS[args.command](args, out)
        return 0
    except WorkerError as ex:
        _emit(out, {"error": str(ex)})
        return 2 if isinstance(ex, UsageError) else 1
    except SystemExit as ex:
        code = ex.code if isinstance(ex.code, int) else 1
        if code:
            _emit(out, {"error": "exited with status {0}".format(code)})
        return code
    except Exception as ex:  # noqa: BLE001 - worker contract: error line + non-zero exit
        _emit(out, {"error": f"{type(ex).__name__}: {ex}"})
        return 1
