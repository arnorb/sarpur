"""Generate golden naming and IMDB outputs from ruvsarpur's Python (tools/reference).

Every output line carries its input item, so a failing comparison can be
reproduced from the golden file alone. The clock is frozen, so outputs don't
depend on the day this runs.

Usage (from the repo root):
    python tools/golden/generate.py [--schedule fixtures/tvschedule.json] [--out golden]
"""
from __future__ import annotations

import argparse
import contextlib
import hashlib
import io
import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tools" / "reference"))

import ruvsarpur  # noqa: E402
import sarpur_worker  # noqa: E402

# Noon on the schedule's own date: the date the catalogue was fetched.
FROZEN_NOW = "2026-03-21T12:00:00"

# The modes the app uses: browser downloads (web), library downloads (plex),
# and the CLI's --originaltitle variant, which the naming code branches on.
MODES = {
    "web": {"original": False, "plex": False},
    "plex": {"original": False, "plex": True},
    "web_original": {"original": True, "plex": False},
}


# The item fields createShowTitle/createLocalFileName read (R:960-1066).
# Only these are stored as the input, and main() checks that they reproduce
# every output, so nothing the naming code depends on is left out.
NAMING_FIELDS = (
    "pid", "title", "series_title", "original-title", "showtime",
    "ep_num", "ep_total", "season_num", "multiple_episodes",
    "is_movie", "is_docu", "is_sport",
)


def _naming_input(item: dict) -> dict:
    trimmed = {key: item[key] for key in NAMING_FIELDS if key in item}
    imdb = item.get("imdb")
    if isinstance(imdb, dict):
        trimmed["imdb"] = {key: imdb.get(key) for key in ("id", "year")}
    elif "imdb" in item:
        trimmed["imdb"] = imdb
    return trimmed


def _names(item: dict) -> dict:
    return {
        mode: {
            "path": ruvsarpur.createLocalFileName(item, options["original"], options["plex"], ""),
            "title": ruvsarpur.createShowTitle(item, options["original"], options["plex"]),
        }
        for mode, options in MODES.items()
    }


def _dump(obj: object) -> str:
    return json.dumps(obj, ensure_ascii=False, sort_keys=True, default=sarpur_worker._json_default)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--schedule", default=str(ROOT / "fixtures" / "tvschedule.json"))
    parser.add_argument("--out", default=str(ROOT / "golden"))
    args = parser.parse_args()

    schedule_path = Path(args.schedule)
    out_dir = Path(args.out)
    out_dir.mkdir(parents=True, exist_ok=True)

    schedule = json.loads(schedule_path.read_text(encoding="utf-8"))
    items = [v for k, v in schedule.items() if k != "date" and isinstance(v, dict)]
    items.sort(key=lambda item: str(item.get("pid")))

    name_lines = []
    imdb_by_sid: dict[str, object] = {}
    with contextlib.redirect_stdout(io.StringIO()), sarpur_worker._frozen_clock(FROZEN_NOW):
        for item in items:
            outputs = _names(item)
            naming_input = _naming_input(item)
            if _names(naming_input) != outputs:
                raise SystemExit(f"NAMING_FIELDS is missing a field the naming code reads (pid {item.get('pid')})")
            name_lines.append(_dump({"pid": item.get("pid"), "input": naming_input, "output": outputs}))
            sid = str(item.get("sid"))
            if item.get("imdb") and sid not in imdb_by_sid:
                imdb_by_sid[sid] = item["imdb"]

    (out_dir / "names.jsonl").write_text("\n".join(name_lines) + "\n", encoding="utf-8")
    imdb_lines = [_dump({"sid": sid, "imdb": imdb_by_sid[sid]}) for sid in sorted(imdb_by_sid)]
    (out_dir / "imdb.jsonl").write_text("\n".join(imdb_lines) + "\n", encoding="utf-8")

    manifest = {
        "schedule": schedule_path.name,
        "schedule_sha256": hashlib.sha256(schedule_path.read_bytes()).hexdigest(),
        "schedule_date": schedule.get("date"),
        "items": len(items),
        "frozen_now": FROZEN_NOW,
        "modes": list(MODES),
        "python": sys.version.split()[0],
        "files": {
            name: hashlib.sha256((out_dir / name).read_bytes()).hexdigest()
            for name in ("names.jsonl", "imdb.jsonl")
        },
    }
    (out_dir / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(manifest, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
