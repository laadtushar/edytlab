"""detect_captions.changes() reports a failed FFmpeg decode and keeps its results for a good one."""
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

TOOLS = Path(__file__).resolve().parent
PROJECT = TOOLS.parent
sys.path.insert(0, str(TOOLS))

SLUG = "dj-beatmatched-transition"


class ChangesDecodeTests(unittest.TestCase):
    def setUp(self):
        self.fixture = tempfile.TemporaryDirectory(dir=PROJECT)
        self.addCleanup(self.fixture.cleanup)
        demos = Path(self.fixture.name)
        (demos / f"{SLUG}.mp4").write_bytes(b"demo")
        with mock.patch.dict(os.environ, {"EDYTLAB_DEMO_DIR": str(demos)}):
            sys.modules.pop("detect_captions", None)
            import detect_captions
        self.addCleanup(sys.modules.pop, "detect_captions", None)
        self.mod = detect_captions

    def stub(self, returncode, stdout, stderr=b""):
        return mock.patch.object(self.mod.subprocess, "run", return_value=subprocess.CompletedProcess([], returncode, stdout, stderr))

    def test_failed_decode_raises_with_ffmpeg_stderr(self):
        with self.stub(1, b"", b"Invalid data found when processing input\n"):
            with self.assertRaises(SystemExit) as caught:
                self.mod.changes(SLUG)
        message = str(caught.exception.code)
        self.assertIn(f"ffmpeg failed on {SLUG}", message)
        self.assertIn("Invalid data found when processing input", message)

    def test_successful_decode_lists_the_changes(self):
        fs = self.mod.W * self.mod.H
        frames = [bytes([0]) * fs] * 3 + [bytes([255]) * fs] * 3   # one caption change at frame 3
        with self.stub(0, b"".join(frames)):
            length, found = self.mod.changes(SLUG)
        self.assertEqual(length, 6 / self.mod.FPS)
        self.assertEqual(found, [(3, 255 * fs)])


if __name__ == "__main__":
    unittest.main()
