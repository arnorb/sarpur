"""Record real RÚV responses made by ruvsarpur's own functions.

Patches requests.Session.request (the single layer under requests.get() and
the retry session), forwards to the real network, and stores each response in
fixtures/ruv/ plus index.json. Sequential, with a pause between requests.

    python tools/record/record.py            # needs network, ~60 requests

Trimming (recorded in the index as "trimmed"): the featured/tv listing is cut
to a few panels and programs, and a program/{sid}/all body over 200 KB keeps
only its first 40 episodes. The code under test sees the trimmed body, so the
fixtures are exactly what it processed.
"""
import hashlib
import json
import re
import sys
import time
import warnings
from pathlib import Path
from unittest import mock

import requests

sys.path.insert(0, str(Path(__file__).resolve().parent))
import replay  # noqa: E402

warnings.filterwarnings("ignore")

OUT = replay.FIXTURE_DIR
PAUSE = 0.4

# sid -> role. Chosen from fixtures/tvschedule.json, verified live on 2026-11-07.
SERIES = {
    "30766": "long episodic (Veður)",
    "35732": "Father Brown (Séra Brown, pid akoha1)",
    "34968": "multi-season (Ofurhetjuskólinn)",
    "33071": "movie (Jólastund snjóbarnanna)",
    "31136": "documentary (Grænir fingur)",
    "37857": "sport (Systraslagur 2025)",
    "35785": "kids (Undraveröld villtu dýranna)",
    "32719": "English subtitled (Verbúðin)",
    "38169": "newest playlist format (1080p/stream.m3u8 + audio group)",
}
SEARCHES = ["Veður", "Brown"]
# getProgramType is asked for the first live episode of these sids (akoha1 itself has expired on RÚV)
GQL_SIDS = ["35732", "30766", "38169"]
GQL_TYPE_HASH = "9d18a07f82fcd469ad52c0656f47fb8e711dc2436983b53754e0c09bad61ca29"
FEATURED_PANELS, FEATURED_PROGRAMS = 2, 3
MAX_BODY, KEEP_EPISODES = 200_000, 40


def slug_for(url):
    m = re.search(r"/programs/program/(\d+)/all", url)
    if m:
        return "program-{0}-all".format(m.group(1))
    if "featured/tv" in url:
        return "featured-tv"
    m = re.search(r"operationName=(\w+)", url)
    if m:
        return "gql-" + m.group(1)
    m = re.search(r"/([^/]+)\.m3u8", url)
    if m:
        return "m3u8-" + m.group(1)
    return "other"


class Recorder:
    def __init__(self):
        self.entries = []
        self.seen = set()
        self.real = requests.Session.request

    def trim(self, url, resp):
        if resp.status_code != 200:
            return None
        if "featured/tv" in url:
            data = resp.json()
            panels = [p for p in data["panels"] if p.get("programs")][:FEATURED_PANELS]
            for p in panels:
                p["programs"] = p["programs"][:FEATURED_PROGRAMS]
            data["panels"] = panels
            resp._content = json.dumps(data, ensure_ascii=False, indent=1).encode("utf-8")
            return "featured/tv cut to {0} panels x {1} programs".format(FEATURED_PANELS, FEATURED_PROGRAMS)
        if re.search(r"/program/\d+/all", url) and len(resp.content) > MAX_BODY:
            data = resp.json()
            total = len(data["episodes"])
            data["episodes"] = data["episodes"][:KEEP_EPISODES]
            resp._content = json.dumps(data, ensure_ascii=False, indent=1).encode("utf-8")
            return "episodes cut from {0} to {1}".format(total, KEEP_EPISODES)
        return None

    def __call__(self, session, method, url, params=None, **kwargs):
        key = replay.request_key(method, url, params)
        time.sleep(PAUSE)
        resp = self.real(session, method, url, params=params, **kwargs)
        note = self.trim(key[1], resp)
        if key not in self.seen:
            self.seen.add(key)
            body = resp.content
            ctype = resp.headers.get("Content-Type", "")
            ext = "json" if "json" in ctype else "m3u8" if ("mpegurl" in ctype or ".m3u8" in key[1]) else "bin"
            name = "{0}-{1}.{2}".format(hashlib.sha1(key[1].encode()).hexdigest()[:8], slug_for(key[1]), ext)
            OUT.joinpath(name).write_bytes(body)
            entry = {"method": key[0], "url": key[1], "file": name, "status": resp.status_code,
                     "content_type": ctype, "bytes": len(body)}
            if note:
                entry["trimmed"] = note
            self.entries.append(entry)
            print(resp.status_code, len(body), key[1][:110], flush=True)
        return resp

    def write_index(self):
        entries = sorted(self.entries, key=lambda e: (e["file"].split("-", 1)[1], e["url"]))
        index = {"recorded": "2026-11-07", "entries": entries}
        OUT.joinpath("index.json").write_text(json.dumps(index, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")


def main():
    ruvsarpur, worker = replay.import_worker()
    OUT.mkdir(parents=True, exist_ok=True)
    for old in OUT.iterdir():
        if old.suffix in (".json", ".m3u8", ".bin"):
            old.unlink()
    rec = Recorder()
    items = {}
    def patched(session, method, url, params=None, **kwargs):
        return rec(session, method, url, params=params, **kwargs)

    with mock.patch.object(requests.Session, "request", patched), replay.no_imdb(ruvsarpur), \
            worker._frozen_clock(replay.FROZEN_NOW):
        # 1. catalogue entry point: capped featured/tv walk (trimmed listing -> a few series calls)
        ruvsarpur.getVodSchedule({}, False, {}, None)
        # 2. program/{sid}/all for the chosen series
        for sid in SERIES:
            episodes = ruvsarpur.getVodSeriesSchedule(sid, None, {}, None)
            print("sid", sid, SERIES[sid], "->", len(episodes), "episodes", flush=True)
            items.update(episodes)
        # 3. GraphQL search + getProgramType
        for q in SEARCHES:
            ruvsarpur.getVodSearchResults(q)
        gql_files = {}
        for sid in GQL_SIDS:
            pid = next(i["pid"] for i in items.values() if i["sid"] == sid)
            graph = ('?operationName=getProgramType&variables={"id":' + sid + ',"episodeId":["' + pid
                     + '"]}&extensions={"persistedQuery":{"version":1,"sha256Hash":"' + GQL_TYPE_HASH + '"}}')
            data = ruvsarpur.requestsVodDataRetrieveWithRetries(graph)
            # On 2026-11-07 RÚV answers PersistedQueryNotFound (data is None); the error is the fixture.
            # The playlist then uses the `file` URL that program/{sid}/all already gave for the episode.
            gql_files[pid] = items[pid]["file"]
        # 4. master playlists: the GQL pids plus the first episode of each series
        targets = dict(gql_files)
        for sid in SERIES:
            first = next((i for i in items.values() if i["sid"] == sid and i.get("file")), None)
            if first:
                targets.setdefault(first["pid"], first["file"])
        for pid, file_url in targets.items():
            item = dict(items[pid])
            item["vod_url_full"] = file_url
            item["vod_url"] = ruvsarpur.getGroup(ruvsarpur.RE_VOD_BASE_URL, "vodbase", file_url)
            r = ruvsarpur.find_m3u8_playlist_url(item, pid, "HD1080")
            print("playlist", pid, file_url, "->", r and r.get("url"), flush=True)
    rec.write_index()
    print(len(rec.entries), "responses recorded")


if __name__ == "__main__":
    main()
