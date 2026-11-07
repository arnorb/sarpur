"""Spike 6: which series ids share a follow title key (plan §3.5).

title_key = NFC(lower(trimSeasonNumberSuffix(normalizeSeriesLabel(series_title))))

Prints a summary and writes every multi-sid group to out/groups.jsonl.
Usage: python spikes/follow/identity.py [fixtures/tvschedule.json]
"""
from __future__ import annotations

import json
import re
import sys
import unicodedata
from collections import defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "worker"))

import ruvsarpur  # noqa: E402

TRAILING_PARENTHESIS = re.compile(r"\s*\([^)]*\)\s*$")  # same as App.tsx and gui_api.py


def normalize_series_label(raw: str) -> str:
    title = (raw or "").strip()
    while title:
        updated = TRAILING_PARENTHESIS.sub("", title).strip()
        if updated == title:
            break
        title = updated
    return title


def title_key(series_title: str) -> str:
    trimmed = ruvsarpur.trimSeasonNumberSuffix(normalize_series_label(series_title))
    return unicodedata.normalize("NFC", trimmed.lower())


def main() -> int:
    schedule_path = Path(sys.argv[1]) if len(sys.argv) > 1 else ROOT / "fixtures" / "tvschedule.json"
    schedule = json.loads(schedule_path.read_text(encoding="utf-8"))
    sids: dict[str, dict] = {}
    for key, item in schedule.items():
        if key == "date" or not isinstance(item, dict) or not item.get("sid"):
            continue
        sid = str(item["sid"])
        entry = sids.setdefault(sid, {
            "sid": sid,
            "series_title": item.get("series_title") or item.get("title") or "",
            "original_title": item.get("original-title"),
            "english": bool(item.get("english_subtitled")),
            "episodes": 0,
        })
        entry["episodes"] += 1

    groups: dict[str, list[dict]] = defaultdict(list)
    for entry in sids.values():
        groups[title_key(entry["series_title"])].append(entry)

    multi = {k: v for k, v in groups.items() if len(v) > 1}
    same_flag = {k: v for k, v in multi.items() if len({e["english"] for e in v}) == 1}
    mixed_flag = {k: v for k, v in multi.items() if len({e["english"] for e in v}) > 1}
    # Groups where the raw series titles differ: these only merge because of
    # the season-suffix trim or the parenthesis strip.
    trimmed_only = {k: v for k, v in multi.items() if len({e["series_title"] for e in v}) > 1}

    out_dir = Path(__file__).resolve().parent / "out"
    out_dir.mkdir(exist_ok=True)
    with (out_dir / "groups.jsonl").open("w", encoding="utf-8") as out:
        for key in sorted(multi):
            out.write(json.dumps({"title_key": key, "sids": multi[key]}, ensure_ascii=False) + "\n")

    print(f"series ids: {len(sids)}")
    print(f"title keys: {len(groups)}")
    print(f"keys shared by >1 sid: {len(multi)} (covering {sum(len(v) for v in multi.values())} sids)")
    print(f"  same English flag (would auto-add): {len(same_flag)}")
    print(f"  mixed English flag (flag rule separates): {len(mixed_flag)}")
    print(f"  raw titles differ (merged by trim/strip): {len(trimmed_only)}")
    for label, selection in (("mixed flag", mixed_flag), ("titles differ", trimmed_only), ("same flag", same_flag)):
        print(f"\nexamples, {label}:")
        for key in sorted(selection)[:12]:
            members = ", ".join(
                f"{e['sid']} '{e['series_title']}'{' [EN]' if e['english'] else ''} ({e['episodes']} ep)"
                for e in selection[key]
            )
            print(f"  {key!r}: {members}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
