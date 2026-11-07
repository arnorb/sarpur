"""Generate fixtures/ruv-golden/series.jsonl offline from fixtures/ruv/.

Runs getVodSeriesSchedule(sid, None, {}, None) for every recorded
program/{sid}/all under replay and the worker's frozen clock (the only
date-dependent branch is `date.today()` for sport_season).

    python tools/record/golden_series.py [output_path]
"""
import json
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import replay  # noqa: E402

OUT = replay.REPO / "fixtures" / "ruv-golden" / "series.jsonl"
FIELDS = ["sid", "pid", "is_movie", "is_docu", "is_sport", "ep_num", "ep_total",
          "season_num", "english_subtitled", "series_title", "title"]


def recorded_sids():
    sids = set()
    for (_, url) in replay.load_index():
        m = re.search(r"/programs/program/(\d+)/all$", url)
        if m:
            sids.add(m.group(1))
    return sorted(sids, key=int)


def generate():
    ruvsarpur, worker = replay.import_worker()
    lines = []
    with replay.replaying(), replay.no_imdb(ruvsarpur), worker._frozen_clock(replay.FROZEN_NOW):
        for sid in recorded_sids():
            for item in ruvsarpur.getVodSeriesSchedule(sid, None, {}, None).values():
                lines.append(json.dumps({k: item[k] for k in FIELDS}, ensure_ascii=False, sort_keys=True))
    return "\n".join(lines) + "\n"


if __name__ == "__main__":
    target = Path(sys.argv[1]) if len(sys.argv) > 1 else OUT
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(generate(), encoding="utf-8")
    print("wrote", target)
