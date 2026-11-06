"""Regression tests for preparing clips over existing output symlinks."""
import json
import os
from pathlib import Path
import shutil
import sys
import tempfile
import unittest
from unittest import mock
import wave


TOOLS = Path(__file__).resolve().parent
PROJECT = TOOLS.parent
with mock.patch.dict(os.environ, {"EDYTLAB_REEL_ROOT": str(PROJECT)}), \
        mock.patch.object(sys, "path", [str(TOOLS), *sys.path]):
    import narrate


class PrepareSymlinkTests(unittest.TestCase):
    def setUp(self):
        self.fixture = tempfile.TemporaryDirectory(dir=PROJECT)
        self.addCleanup(self.fixture.cleanup)
        self.root = Path(self.fixture.name)
        self.audio = self.root / "audio" / "reel"
        self.audio.mkdir(parents=True)
        self.source = self.root / "source.mp3"
        # FFmpeg detects the WAV header even though the cache name ends in .mp3.
        with wave.open(str(self.source), "wb") as audio:
            audio.setnchannels(1)
            audio.setsampwidth(2)
            audio.setframerate(44100)
            audio.writeframes(b"\0\0" * 44100)
        self.meta = {
            "mp3": str(self.source), "text": "hi", "request_id": "test",
            "alignment": {"characters": ["h", "i"],
                          "character_start_times_seconds": [0, 0.5],
                          "character_end_times_seconds": [0.5, 1.0]},
        }

    def prepare(self, end):
        timeline = {"file": "reel", "cues": [{
            "id": "R1", "narration": "hi", "narration_spoken": "hi", "start": 0, "end": end,
        }]}
        with mock.patch.object(narrate, "OUT", str(self.root)), \
                mock.patch.object(narrate, "cached", return_value=self.meta):
            return narrate.prepare(timeline)

    def check_local_link(self, linked_ext, clip_ext, end):
        target = self.root / f"keep-{linked_ext}.dat"
        target.write_bytes(b"keep the target")
        link = self.audio / f"R1.{linked_ext}"
        link.symlink_to(target)
        timing, _ = self.prepare(end)
        self.assertEqual(target.read_bytes(), b"keep the target")
        self.assertFalse(link.is_symlink())
        clip = self.audio / f"R1.{clip_ext}"
        self.assertTrue(clip.is_file())
        self.assertEqual(timing["R1"]["clip"], f"R1.{clip_ext}")
        saved = json.loads((self.root / "reel-narration-timing.json").read_text())
        self.assertEqual(saved["R1"]["clip"], f"R1.{clip_ext}")

    def test_mp3_output_replaces_each_local_symlink_without_touching_target(self):
        for ext in ("mp3", "wav"):
            with self.subTest(ext=ext), tempfile.TemporaryDirectory(dir=self.root) as case:
                original_audio = self.audio
                self.audio = Path(case) / "audio" / "reel"
                self.audio.mkdir(parents=True)
                original_root = self.root
                self.root = Path(case)
                try:
                    self.check_local_link(ext, "mp3", 3)
                finally:
                    self.root, self.audio = original_root, original_audio

    @unittest.skipUnless(shutil.which("ffmpeg"), "FFmpeg is required to test tempo fitting")
    def test_wav_output_replaces_each_local_symlink_without_touching_target(self):
        for ext in ("mp3", "wav"):
            with self.subTest(ext=ext), tempfile.TemporaryDirectory(dir=self.root) as case:
                original_audio = self.audio
                self.audio = Path(case) / "audio" / "reel"
                self.audio.mkdir(parents=True)
                original_root = self.root
                self.root = Path(case)
                try:
                    self.check_local_link(ext, "wav", 1.2)
                finally:
                    self.root, self.audio = original_root, original_audio

    def test_clip_symlink_to_cached_source_preserves_source(self):
        clip = self.audio / "R1.mp3"
        source_bytes = self.source.read_bytes()
        clip.symlink_to(self.source)
        timing, _ = self.prepare(3)
        self.assertEqual(self.source.read_bytes(), source_bytes)
        self.assertFalse(clip.is_symlink())
        self.assertEqual(clip.read_bytes(), source_bytes)
        self.assertEqual(timing["R1"]["clip"], "R1.mp3")

    def test_external_symlink_is_refused_before_either_output_is_removed(self):
        with tempfile.TemporaryDirectory() as external:
            target = Path(external) / "outside"
            target.write_bytes(b"untouched")
            existing = self.audio / "R1.mp3"
            existing.write_bytes(b"existing clip")
            link = self.audio / "R1.wav"
            link.symlink_to(target)
            with self.assertRaises(SystemExit):
                self.prepare(3)
            self.assertEqual(existing.read_bytes(), b"existing clip")
            self.assertTrue(link.is_symlink())
            self.assertEqual(target.read_bytes(), b"untouched")


if __name__ == "__main__":
    unittest.main()
