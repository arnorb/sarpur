"""Offline tests for the worker's `plan` command.

    /path/to/python -m unittest discover -s worker -p "test_*.py"
"""
import io
import json
import os
import shutil
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

WORKER_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(WORKER_DIR))
import ruvsarpur  # noqa: E402
import sarpur_worker  # noqa: E402

FIXTURE = WORKER_DIR.parent / "fixtures" / "tvschedule.json"
FAKE_PLAYLIST = {
    "url": "https://ruv-vod.akamaized.net/opid/X/1080p/stream.m3u8",
    "audio_url": "https://ruv-vod.akamaized.net/opid/X/audio/stream.m3u8",
    "fragments": 42,
    "quality": "HD1080",
}

# One of each kind in the pinned fixture: episode, movie in parts, sport,
# movie that is also sport, and a single (non-episodic) programme.
PIDS = ["akoha1", "7e4r61", "8vqq32"]


def _ffmpeg_metadata_pairs(item):
    """Run download_m3u8_playlist_using_ffmpeg with Popen mocked; return its -metadata pairs."""
    process = mock.MagicMock()
    process.stdout.readline.return_value = ""
    process.poll.return_value = 1
    process.returncode = 1
    with mock.patch.object(ruvsarpur.subprocess, "Popen", return_value=process) as popen, \
            mock.patch.object(ruvsarpur, "removePartialFile"), \
            mock.patch("sys.stdout", io.StringIO()):
        ruvsarpur.download_m3u8_playlist_using_ffmpeg(
            "ffmpeg", FAKE_PLAYLIST["url"], 42, "out", "t", False, "HD1080", False,
            dict(item), FAKE_PLAYLIST["audio_url"])
    args = popen.call_args[0][0]
    return [args[i + 1].split("=", 1) for i, arg in enumerate(args) if arg == "-metadata"]


class PlanTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.schedule = json.loads(FIXTURE.read_text(encoding="utf-8"))
        items = [v for k, v in cls.schedule.items() if k != "date"]
        cls.pids = list(PIDS)
        cls.pids.append(next(i["pid"] for i in items if i.get("is_movie") and i.get("is_sport")))
        cls.pids.append(next(i["pid"] for i in items if "ep_num" in i and int(i.get("ep_total") or 0) <= 1
                             and not (i.get("is_movie") or i.get("is_docu") or i.get("is_sport"))))

    def setUp(self):
        self.tmp = tempfile.mkdtemp()
        shutil.copy(FIXTURE, os.path.join(self.tmp, "tvschedule.json"))
        self.cwd = os.getcwd()
        os.chdir(self.tmp)

    def tearDown(self):
        os.chdir(self.cwd)
        shutil.rmtree(self.tmp, ignore_errors=True)

    def run_worker(self, *argv):
        out = io.StringIO()
        with mock.patch("sys.stdout", out), \
                mock.patch.object(ruvsarpur, "find_m3u8_playlist_url", return_value=dict(FAKE_PLAYLIST)) as find:
            code = sarpur_worker.main(list(argv))
        lines = [json.loads(line) for line in out.getvalue().splitlines()]
        return code, lines, find

    def test_plan_plex_episode(self):
        code, lines, find = self.run_worker("plan", "akoha1", "--mode", "plex", "--portable")
        self.assertEqual(code, 0)
        self.assertEqual(len(lines), 1)
        plan = lines[0]
        item = self.schedule["akoha1"]
        self.assertEqual(plan["pid"], "akoha1")
        self.assertEqual(plan["sid"], str(item["sid"]))
        self.assertEqual(plan["relative_path"], ruvsarpur.createLocalFileName(item, False, True) + ".mp4")
        self.assertIn("Season 01", plan["relative_path"])
        self.assertEqual(plan["content_type"], "show")
        self.assertEqual(plan["playlist_url"], FAKE_PLAYLIST["url"])
        self.assertEqual(plan["audio_url"], FAKE_PLAYLIST["audio_url"])
        self.assertEqual(plan["quality"], "HD1080")
        self.assertEqual(plan["duration_seconds"], int(item["duration"]))
        self.assertIn(["comment", f"ruvinfo:akoha1:{item['sid']}"], plan["metadata"])
        # The resolved item carries the full and base VOD urls.
        resolved = find.call_args[0][0]
        self.assertEqual(resolved["vod_url_full"], item["file"])

    def test_plan_web_mode_name(self):
        code, lines, _ = self.run_worker("plan", "akoha1", "--mode", "web", "--portable")
        self.assertEqual(code, 0)
        self.assertEqual(lines[0]["relative_path"],
                         ruvsarpur.createLocalFileName(self.schedule["akoha1"], False, False))
        self.assertTrue(lines[0]["relative_path"].endswith(".mp4"))

    def test_content_types(self):
        expected = {"7e4r61": "movie_or_docu", "8vqq32": "sport"}
        for pid, content_type in expected.items():
            _, lines, _ = self.run_worker("plan", pid, "--mode", "plex", "--portable")
            self.assertEqual(lines[0]["content_type"], content_type, pid)

    def test_metadata_matches_ffmpeg_args(self):
        for pid in self.pids:
            with self.subTest(pid=pid):
                code, lines, _ = self.run_worker("plan", pid, "--mode", "plex", "--portable")
                self.assertEqual(code, 0, lines)
                self.assertEqual(lines[0]["metadata"], _ffmpeg_metadata_pairs(self.schedule[pid]))

    def test_item_without_full_vod_url_fails_clearly(self):
        code, lines, find = self.run_worker("plan", "bhf3r1", "--mode", "plex", "--portable")
        self.assertEqual(code, 1)
        self.assertIn("can't be downloaded", lines[-1]["error"])
        find.assert_not_called()

    def test_unknown_pid(self):
        code, lines, _ = self.run_worker("plan", "nosuch", "--mode", "web", "--portable")
        self.assertEqual(code, 1)
        self.assertIn("not found", lines[-1]["error"])


if __name__ == "__main__":
    unittest.main()
