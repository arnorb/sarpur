"""Differential check for S3: the Rust port against the Python worker.

Runs each ported function in Python (worker/ruvsarpur.py) and in Rust (the
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


def rust(*args: str) -> list[str]:
    cmd = ["cargo", "run", "-q", "-p", "sarpur-lib", "--example", "compat", "--", *args]
    out = subprocess.run(cmd, cwd=ROOT, check=True, capture_output=True, text=True).stdout
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


CHECKS = {
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
