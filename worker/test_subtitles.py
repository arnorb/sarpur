"""ruvsarpur's VTT to SRT conversion (R:382) on fixtures/subtitles/.

The `.srt` files there are this function's output; the Rust port
(`sarpur-lib::subtitles`) is tested against the same files. Run with
`--regenerate` to rewrite them after a deliberate change.
"""
import io
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

FIXTURES = WORKER_DIR.parent / "fixtures" / "subtitles"


def convert(vtt: Path):
    """The SRT text ruvsarpur writes for `vtt`, or None if it writes none."""
    with tempfile.TemporaryDirectory() as tmp:
        copy = Path(tmp) / vtt.name
        shutil.copy(vtt, copy)
        with mock.patch("sys.stdout", io.StringIO()):
            srt = ruvsarpur.convertVttSubtitleFileToSrt(str(copy))
        return None if srt is None else Path(srt).read_bytes()


class SubtitleTests(unittest.TestCase):
    def test_fixtures_match_ruvsarpur(self):
        vtts = sorted(FIXTURES.glob("*.vtt"))
        self.assertGreaterEqual(len(vtts), 4)
        for vtt in vtts:
            with self.subTest(vtt=vtt.name):
                expected = vtt.with_suffix(".srt")
                got = convert(vtt)
                if expected.exists():
                    self.assertEqual(got, expected.read_bytes())
                else:
                    self.assertIsNone(got)


if __name__ == "__main__":
    if "--regenerate" in sys.argv:
        for vtt in sorted(FIXTURES.glob("*.vtt")):
            srt = convert(vtt)
            target = vtt.with_suffix(".srt")
            if srt is None:
                target.unlink(missing_ok=True)
            else:
                target.write_bytes(srt)
            print(vtt.name, "->", "no cues" if srt is None else target.name)
        sys.exit(0)
    unittest.main()
