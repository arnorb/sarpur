"""A stand-in for `ruvsarpur.py --json` in the Rust tests. Behaviour is keyed by pid."""
import json
import sys
import time

args = [a for a in sys.argv[1:] if a not in ("--json", "--portable")]
command = args[0] if args else ""


def emit(obj):
    print(json.dumps(obj, ensure_ascii=False), flush=True)


def plan(pid, relative_path, playlist="fake://ok", content_type="show", duration=300):
    return {
        "pid": pid, "sid": "100", "title": "Þáttur s01e01", "relative_path": relative_path,
        "content_type": content_type, "playlist_url": playlist, "audio_url": "fake://audio",
        "quality": "HD1080", "duration_seconds": duration,
        "metadata": [["title", "Þáttur"], ["comment", f"ruvinfo:{pid}:100"]], "subtitles": None,
    }


PLANS = {
    "web1": plan("web1", "Þáttur s01e01.mp4"),
    "lib1": plan("lib1", "Þáttur [x]/Season 01/Þáttur [x] - s01e01.mp4"),
    "fail1": plan("fail1", "Bilun.mp4", playlist="fake://fail"),
    "slow1": plan("slow1", "Hægt.mp4", playlist="fake://slow"),
    "slow2": plan("slow2", "Hægt 2.mp4", playlist="fake://slow"),
}

if command == "catalogue":
    emit({"pid": "web1", "sid": "100", "title": "Þáttur (1 af 2)", "series_title": "Þáttur",
          "showtime": "2026-11-07T19:35:00", "duration": "300", "english_subtitled": False,
          "portrait_image": "https://myndir.ruv.is/p.jpg", "series_image": "https://evil.example/s.jpg",
          "subtitles": [{"name": "IS", "value": "https://x/is.vtt"}, {"name": "en", "value": "https://x/en.vtt"}]})
    emit({"pid": "mov1", "sid": "200", "title": "Bíómynd", "series_title": "Bíómynd",
          "showtime": "2026-11-06T21:00:00", "duration": 5400, "is_movie": True})
    if "--refresh" in args:
        emit({"pid": "new1", "sid": "300", "title": "Nýtt", "series_title": "Nýtt",
              "showtime": "2026-11-08T20:00:00", "duration": 600})
elif command == "plan":
    pid = args[1]
    if pid in PLANS:
        emit(PLANS[pid])
    else:
        emit({"error": f"pid {pid} not found in the schedule"})
        sys.exit(1)
elif command == "crash":
    print("boom", file=sys.stderr)
    sys.exit(3)
elif command == "sleep":
    time.sleep(30)
else:
    emit({"error": f"unknown command {command}"})
    sys.exit(2)
