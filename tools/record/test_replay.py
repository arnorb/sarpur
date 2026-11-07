"""Offline tests for the RÚV recordings.

    /path/to/python -m unittest discover -s tools/record
"""
import sys
import unittest
from pathlib import Path

import requests

sys.path.insert(0, str(Path(__file__).resolve().parent))
import replay  # noqa: E402

ruvsarpur, worker = replay.import_worker()


class ReplayTests(unittest.TestCase):
    def series(self, sid):
        with replay.replaying(), replay.no_imdb(ruvsarpur), worker._frozen_clock(replay.FROZEN_NOW):
            return ruvsarpur.getVodSeriesSchedule(sid, None, {}, None)

    def test_series_replays_offline(self):
        eps = self.series("35732")
        self.assertEqual(len(eps), 4)
        first = eps["akohaa"]
        self.assertEqual(first["series_title"], "Séra Brown")
        self.assertEqual(first["season_num"], "1")
        self.assertFalse(first["is_movie"])

    def test_english_subtitled_flag(self):
        eps = self.series("32719")
        self.assertTrue(all(e["english_subtitled"] for e in eps.values()))

    def test_unrecorded_url_raises(self):
        with replay.replaying():
            with self.assertRaises(replay.UnrecordedRequest):
                requests.get("https://api.ruv.is/api/programs/program/1/all")

    def test_unrecorded_not_swallowed_by_code_under_test(self):
        # getVodSeriesSchedule for an unrecorded sid must fail loudly, not return {}
        with replay.replaying(), replay.no_imdb(ruvsarpur):
            with self.assertRaises(replay.UnrecordedRequest):
                ruvsarpur.getVodSeriesSchedule("1", None, {}, None)

    def test_playlist_resolves_from_recording(self):
        index = replay.load_index()
        masters = [u for (_, u) in index if u.endswith("5499461-MRDLS2.m3u8")]
        item = {"pid": "akohaa", "vod_url_full": masters[0],
                "vod_url": masters[0].rsplit("/", 1)[0], "title": "t"}
        with replay.replaying():
            result = ruvsarpur.find_m3u8_playlist_url(item, "t", "HD1080")
        self.assertTrue(result["url"].endswith("1080p/stream.m3u8"))
        self.assertTrue(result["audio_url"].endswith("ISL/stream.m3u8"))


if __name__ == "__main__":
    unittest.main()
