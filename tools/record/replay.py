"""Replay recorded RÚV responses by patching requests.Session.request.

Every HTTP call in ruvsarpur.py goes through requests.Session.request
(requests.get() and the retry session both end up there), so that one method
is the only layer patched. A request that was not recorded raises
UnrecordedRequest, which derives from BaseException so the broad
`except Exception` blocks in ruvsarpur cannot swallow it.

Usage as a module:
    with replaying():
        ruvsarpur.getVodSeriesSchedule(...)
"""
import contextlib
import json
import sys
from pathlib import Path
from unittest import mock

import requests

REPO = Path(__file__).resolve().parents[2]
FIXTURE_DIR = REPO / "fixtures" / "ruv"
WORKER_DIR = REPO / "tools" / "reference"
FROZEN_NOW = "2026-11-07T12:00:00"


class UnrecordedRequest(BaseException):
    pass


def request_key(method, url, params=None):
    """(METHOD, fully prepared URL); identical for recording and replay."""
    prepared = requests.Request(method.upper(), url, params=params).prepare()
    return method.upper(), prepared.url


def load_index(fixture_dir=FIXTURE_DIR):
    data = json.loads((Path(fixture_dir) / "index.json").read_text(encoding="utf-8"))
    return {(e["method"], e["url"]): e for e in data["entries"]}


def make_response(entry, body, url):
    resp = requests.Response()
    resp.status_code = entry["status"]
    resp._content = body
    resp.url = url
    resp.encoding = "utf-8"
    if entry.get("content_type"):
        resp.headers["Content-Type"] = entry["content_type"]
    return resp


@contextlib.contextmanager
def replaying(fixture_dir=FIXTURE_DIR):
    fixture_dir = Path(fixture_dir)
    index = load_index(fixture_dir)

    def fake_request(self, method, url, params=None, **kwargs):
        key = request_key(method, url, params)
        entry = index.get(key)
        if entry is None:
            raise UnrecordedRequest("no recording for {0} {1}".format(*key))
        body = (fixture_dir / entry["file"]).read_bytes()
        return make_response(entry, body, key[1])

    with mock.patch.object(requests.Session, "request", fake_request):
        yield


def import_worker():
    """Import ruvsarpur and sarpur_worker from tools/reference/ without touching them."""
    if str(WORKER_DIR) not in sys.path:
        sys.path.insert(0, str(WORKER_DIR))
    import ruvsarpur
    import sarpur_worker
    return ruvsarpur, sarpur_worker


@contextlib.contextmanager
def no_imdb(ruvsarpur):
    """IMDB lookups are out of scope for the RÚV recordings (see golden/imdb.jsonl)."""
    with mock.patch.object(ruvsarpur, "lookupItemInIMDB", lambda *a, **k: None):
        yield
