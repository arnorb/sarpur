"""Differential check for S3: the Rust port against ruvsarpur's Python.

Runs each ported function in Python (tools/reference/ruvsarpur.py) and in Rust (the
`compat` example of sarpur-lib) on the same input and compares the JSON
lines. Offline by default: RÚV is replayed from fixtures/ruv/. With --live,
both sides fetch from RÚV (a live snapshot; run it by hand, not in CI).

    python tools/diff/diff.py [--live] [check ...]

Exit status 1 when any line differs; the first differences are printed.
"""
from __future__ import annotations

import argparse
import contextlib
import io
import json
import os
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tools" / "record"))
import replay  # noqa: E402

ruvsarpur, worker = replay.import_worker()

SCHEDULE = ROOT / "fixtures" / "tvschedule.json"
FIXTURES = ROOT / "fixtures" / "ruv"


def dump(obj) -> str:
    return json.dumps(obj, ensure_ascii=False, sort_keys=True, default=worker._json_default)


def rust(*args: str, stdin: str | None = None) -> list[str]:
    cmd = ["cargo", "run", "-q", "-p", "sarpur-lib", "--example", "compat", "--", *args]
    out = subprocess.run(cmd, cwd=ROOT, check=True, capture_output=True, text=True, input=stdin).stdout
    return [dump(json.loads(line)) for line in out.splitlines() if line.strip()]


@contextlib.contextmanager
def network(live: bool):
    """Live, or replayed with a 404 for anything not recorded: the Rust side
    reads an unrecorded request as "not found" too, so a fallback to another
    quality is compared instead of stopping at the first gap."""
    if live:
        yield
        return
    index = replay.load_index()

    def fake_request(self, method, url, params=None, **kwargs):
        key = replay.request_key(method, url, params)
        entry = index.get(key)
        if entry is None:
            return replay.make_response({"status": 404}, b"", key[1])
        return replay.make_response(entry, (FIXTURES / entry["file"]).read_bytes(), key[1])

    with replay.mock.patch.object(replay.requests.Session, "request", fake_request):
        yield


def schedule_items() -> list[dict]:
    schedule = json.loads(SCHEDULE.read_text(encoding="utf-8"))
    items = [v for k, v in schedule.items() if k != "date" and isinstance(v, dict)]
    return sorted(items, key=lambda i: str(i.get("pid")))


def check_metadata(live: bool) -> tuple[list[str], list[str]]:
    lines = []
    for item in schedule_items():
        try:
            lines.append(dump({"pid": item["pid"], "metadata": ruvsarpur.buildFfmpegMetadata(item)}))
        except Exception:  # noqa: BLE001 - the Rust side reports an error too
            lines.append(dump({"pid": item["pid"], "error": True}))
    return lines, rust("metadata", str(SCHEDULE))


def master_urls() -> list[str]:
    return sorted(u for (_, u) in replay.load_index()
                  if u.startswith(ruvsarpur.RUV_URL) and u.rsplit("/", 1)[-1].split(".")[0] in u.rsplit("/", 2)[-2])


def check_resolve(live: bool) -> tuple[list[str], list[str]]:
    urls = master_urls()
    py, rs = [], []
    for quality in ("HD1080", "HD720", "Normal"):
        with network(live):
            for url in urls:
                item = {"pid": "p", "title": "t", "file": url}
                try:
                    p = worker._resolve_playlist(item, quality)
                    py.append(dump({"file": url, "url": p["url"], "audio_url": p.get("audio_url"),
                                    "fragments": p["fragments"], "quality": p["quality"]}))
                except worker.WorkerError:
                    py.append(dump({"file": url, "error": True}))
        rs += rust("resolve", *(["--live"] if live else ["--replay", str(FIXTURES)]), quality, *urls)
    return py, rs


def fuzz_pairs() -> list[tuple[str, str]]:
    """Queries a person might type against the catalogue's titles: words and
    pieces of titles, with and without typos, in lower case as search uses."""
    import random
    rng = random.Random(7)
    titles = sorted({t.lower() for item in schedule_items()
                     for t in (item.get("title"), item.get("series_title"), item.get("original-title")) if t})
    queries = set()
    for title in rng.sample(titles, 300):
        words = title.split()
        queries.add(rng.choice(words))
        start = rng.randrange(len(title))
        queries.add(title[start:start + rng.randint(3, 12)])
        typo = list(title[: rng.randint(4, 16)])
        if len(typo) > 2:
            typo[rng.randrange(len(typo))] = rng.choice("aeiouáéíóúþæöð ")
        queries.add("".join(typo))
    queries = sorted(q for q in queries if q.strip())
    return [(q, t) for q in queries for t in rng.sample(titles, 25)]


def check_fuzz(live: bool) -> tuple[list[str], list[str]]:
    from fuzzywuzzy import fuzz
    pairs = fuzz_pairs()
    py = [dump({"ratio": fuzz.ratio(a, b), "partial": fuzz.partial_ratio(a, b)}) for a, b in pairs]
    stdin = "".join(json.dumps([a, b], ensure_ascii=False) + "\n" for a, b in pairs)
    return py, rust("fuzz", stdin=stdin)


EPISODE_KEYS = ("description", "duration", "file_expires", "files", "firstrun", "id", "number", "title")


def normal(item: dict) -> str:
    """An item as compared: `episode` cut to the keys sarpur keeps, and the
    subtitle tracks sorted (their order comes from a JSON object, which
    serde_json doesn't keep)."""
    item = dict(item)
    if isinstance(item.get("episode"), dict):
        item["episode"] = {k: v for k, v in item["episode"].items() if k in EPISODE_KEYS}
    if isinstance(item.get("subtitles"), list):
        item["subtitles"] = sorted(item["subtitles"], key=lambda s: str(s.get("name")))
    return dump(item)


def recorded_sids() -> list[str]:
    import re
    return sorted({m.group(1) for (_, u) in replay.load_index()
                   if (m := re.search(r"/programs/program/(\d+)/all$", u))}, key=int)


def check_series(live: bool) -> tuple[list[str], list[str]]:
    sids = recorded_sids()
    py = []
    with network(live), replay.no_imdb(ruvsarpur):
        for sid in sids:
            try:
                py += [normal(i) for i in ruvsarpur.getVodSeriesSchedule(sid, None, {}, None).values()]
            except Exception:  # noqa: BLE001
                py.append(dump({"sid": sid, "error": True}))
    rs = [normal(json.loads(line)) for line in rust("series", *net_args(live), *sids)]
    return py, rs


def check_catalogue(live: bool) -> tuple[list[str], list[str]]:
    """Items compared by pid: the saved schedule is sorted by pid, so the
    order they were fetched in doesn't matter. Live, RÚV can change while
    the two fetch (Python takes about 15 minutes), so a few differences
    there need a look rather than mean a bug."""
    with network(live), replay.no_imdb(ruvsarpur):
        py = sorted(normal(i) for i in ruvsarpur.getVodSchedule({}, False, {}, None).values())
    rs = sorted(normal(json.loads(line)) for line in rust("catalogue", *net_args(live)))
    if live:
        out = Path(os.environ.get("DIFF_OUT", "."))
        (out / "catalogue-python.jsonl").write_text("\n".join(py) + "\n", encoding="utf-8")
        (out / "catalogue-rust.jsonl").write_text("\n".join(rs) + "\n", encoding="utf-8")
    return py, rs


SEARCHES = ["Brown", "séra", "veður", "fréttir", "krakka", "kiljan", "landinn", "Silfrið", "the", "a",
            "Hvolpasveitin", "Father Brown", "english", "with English subtitles", "Kastljós", "spaug",
            "Stundin okkar", "Ófærð", "trapped", "Bíó", "heimild", "íþrótt", "Gettu betur", "Útsvar",
            "xyz", "Ævar", "Sögur", "2024", "Hraðfréttir", "Tónatal", "jól", "Lag dagsins", "Áramótaskaup"]


def check_search(live: bool) -> tuple[list[str], list[str]]:
    schedule = json.loads(SCHEDULE.read_text(encoding="utf-8"))
    schedule["date"] = None
    py = []
    for query in SEARCHES:
        args = argparse.Namespace(sid=None, pid=None, find=query, originaltitle=False,
                                  new=False, includeenglishsubs=True)
        pids = [i["pid"] for i in ruvsarpur.searchForItemsInTvSchedule(args, schedule)]
        py.append(dump({"query": query, "pids": pids}))
    return py, rust("search", str(SCHEDULE), *SEARCHES)


def imdb_cases() -> list[list]:
    """The lookups the catalogue made: each series with a foreign title, as
    getVodSeriesSchedule would ask (live only: IMDB isn't recorded)."""
    seen, cases = set(), []
    for item in schedule_items():
        title = item.get("original-title") or (item.get("series_title") if item.get("is_movie") else None)
        if not title or item["sid"] in seen:
            continue
        seen.add(item["sid"])
        kind = "documentary" if item.get("is_docu") else "movie" if item.get("is_movie") else "tvshow"
        year = ruvsarpur.getGroup(ruvsarpur.RE_CAPTURE_YEAR_FROM_DESCRIPTION, "year", item.get("series_sdesc") or "")
        duration = float(item.get("duration") or 0)
        cases.append([title, year, kind, duration, int(item.get("ep_total") or 1)])
    return cases[:120]


def check_imdb(live: bool) -> tuple[list[str], list[str]]:
    if not live:
        return [], []
    cases = imdb_cases()
    py = []
    for title, year, kind, duration, episodes in cases:
        try:
            found = ruvsarpur.lookupItemInIMDB(title, year, kind, duration, episodes, False, {})
            py.append(dump({"title": title, "imdb": found}))
        except Exception:  # noqa: BLE001
            py.append(dump({"title": title, "error": True}))
    stdin = "".join(json.dumps(c, ensure_ascii=False) + "\n" for c in cases)
    return py, rust("imdb", "--live", stdin=stdin)


def net_args(live: bool) -> list[str]:
    return ["--live"] if live else ["--replay", str(FIXTURES)]


CHECKS = {
    "imdb": check_imdb,
    "search": check_search,
    "series": check_series,
    "catalogue": check_catalogue,
    "fuzz": check_fuzz,
    "metadata": check_metadata,
    "resolve": check_resolve,
}


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--live", action="store_true")
    parser.add_argument("checks", nargs="*", default=list(CHECKS))
    args = parser.parse_args()
    failed = False
    for name in args.checks:
        with contextlib.redirect_stdout(io.StringIO()), worker._frozen_clock(replay.FROZEN_NOW):
            py, rs = CHECKS[name](args.live)
        diffs = [(a, b) for a, b in zip(py, rs) if a != b]
        if len(py) != len(rs):
            diffs.append((f"{len(py)} lines", f"{len(rs)} lines"))
        print(f"{name}: {len(py)} lines, {len(diffs)} differences")
        for a, b in diffs[:5]:
            print(f"  python: {a}\n  rust:   {b}")
        failed |= bool(diffs)
    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
