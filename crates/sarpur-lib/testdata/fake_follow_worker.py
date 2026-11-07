"""A stand-in for `ruvsarpur.py --json` in the follow tests. The catalogue lives
in `fake_catalogue.json` in the working folder (the test's data folder):
{"items": [...], "series": {"<sid>": [...]}, "refresh_fails": false}."""
import json
import sys

args = [a for a in sys.argv[1:] if a not in ("--json", "--portable")]
command = args[0] if args else ""


def emit(obj):
    print(json.dumps(obj, ensure_ascii=False), flush=True)


def load():
    with open("fake_catalogue.json", encoding="utf-8") as f:
        return json.load(f)


def save(data):
    with open("fake_catalogue.json", "w", encoding="utf-8") as f:
        json.dump(data, f, ensure_ascii=False)


data = load()
if command == "catalogue":
    if "--refresh" in args and data.get("refresh_fails"):
        emit({"error": "RÚV is down"})
        sys.exit(1)
    for item in data["items"]:
        emit(item)
elif command == "series":
    items = data.get("series", {}).get(args[1], [])
    if "--save" in args and items:
        data["items"].extend(items)
        data["series"].pop(args[1])
        save(data)
    for item in items:
        emit(item)
elif command == "names":
    by_pid = {i["pid"]: i for i in data["items"]}
    for pid in [a for a in args[1:] if a not in ("--mode", "plex", "web")]:
        item = by_pid.get(pid)
        if item is None:
            emit({"pid": pid, "not_found": True})
            continue
        title = item.get("series_title") or item["title"]
        emit({"pid": pid, "relative_path": f"{title}/Season 01/{title} - {pid}.mp4",
              "content_type": "show"})
elif command == "plan":
    item = next((i for i in data["items"] if i["pid"] == args[1]), None)
    if item is None:
        emit({"error": f"pid {args[1]} not found in the schedule"})
        sys.exit(1)
    title = item.get("series_title") or item["title"]
    emit({"pid": item["pid"], "sid": item["sid"], "title": item["title"],
          "relative_path": f"{title}/Season 01/{title} - {item['pid']}.mp4",
          "content_type": "show", "playlist_url": "fake://ok", "audio_url": None,
          "quality": "HD1080", "duration_seconds": 300,
          "metadata": [["comment", f"ruvinfo:{item['pid']}:{item['sid']}"]], "subtitles": None})
else:
    emit({"error": f"unknown command {command}"})
    sys.exit(2)
