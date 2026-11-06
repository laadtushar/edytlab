"""Exercise the narration script's video boundary without processing media."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


PROJECT = Path(__file__).resolve().parents[1]
SCRIPT = PROJECT / "add-narration.sh"
DEMOS = ("dj-beatmatched-transition.mp4", "dj-extended-club-intro.mp4", "dj-mini-mix.mp4")


class VideoBoundaryTests(unittest.TestCase):
    def setUp(self):
        self.fixture = tempfile.TemporaryDirectory(dir=PROJECT)
        self.addCleanup(self.fixture.cleanup)
        self.repo = Path(self.fixture.name)
        self.project = self.repo / "marketing/reels"
        self.project.mkdir(parents=True)
        self.script = self.project / "add-narration.sh"
        self.script.write_bytes(SCRIPT.read_bytes())
        self.demo_dir = self.repo / "website/public/demos"
        self.demo_dir.mkdir(parents=True)
        self.bin = self.project / "bin"
        self.bin.mkdir()
        self.log = self.project / "probe.json"
        probe = self.bin / "ffprobe"
        probe.write_text(
            f"#!{sys.executable}\n"
            "import json, os, sys\n"
            "with open(os.environ['PROBE_LOG'], 'w') as f: json.dump(sys.argv[1:], f)\n"
            "sys.exit(73)\n"
        )
        probe.chmod(0o755)
        ffmpeg = self.bin / "ffmpeg"
        ffmpeg.write_text("#!/bin/sh\nexit 74\n")
        ffmpeg.chmod(0o755)
        self.cues = self.project / "cues.json"
        self.cues.write_text('{"cues": []}')
        self.audio = self.project / "audio"
        self.audio.mkdir()
        self.output = self.project / "output.mp4"

    def invoke(self, video, output=None):
        env = {**os.environ, "PATH": str(self.bin) + os.pathsep + os.environ["PATH"],
               "TMPDIR": str(self.project / "scratch"), "PROBE_LOG": str(self.log)}
        return subprocess.run(
            ["bash", str(self.script), str(video), str(self.cues), str(self.audio),
             str(output or self.output)], cwd=self.project, env=env,
            capture_output=True, text=True, check=False,
        )

    def assert_accepted(self, video):
        self.log.unlink(missing_ok=True)
        result = self.invoke(video)
        self.assertEqual(result.returncode, 73, result.stderr)
        self.assertEqual(json.loads(self.log.read_text())[-1], str(video))

    def assert_refused(self, video):
        self.log.unlink(missing_ok=True)
        result = self.invoke(video)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Refused path", result.stderr)
        self.assertFalse(self.log.exists(), "refused video reached ffprobe")

    def test_local_video_and_named_demos_keep_supplied_spelling(self):
        local = self.project / "video with spaces.mp4"
        local.write_bytes(b"synthetic")
        self.assert_accepted("./video with spaces.mp4")
        link = self.project / "local-link.mp4"
        link.symlink_to(local)
        self.assert_accepted("local-link.mp4")
        for name in DEMOS:
            demo = self.demo_dir / name
            demo.write_bytes(b"synthetic")
            with self.subTest(name=name):
                self.assert_accepted(demo)

    def test_external_files_and_symlink_escapes_are_refused(self):
        with tempfile.TemporaryDirectory() as external:
            outside = Path(external) / "outside.mp4"
            outside.write_bytes(b"synthetic")
            self.assert_refused(outside)
            external_link = Path(external) / "alias.mp4"
            external_link.symlink_to(outside)
            self.assert_refused(external_link)
            local_link = self.project / "escape.mp4"
            local_link.symlink_to(outside)
            self.assert_refused(local_link)
            demo_link = self.demo_dir / DEMOS[0]
            demo_link.symlink_to(outside)
            self.assert_refused(demo_link)
        other_demo = self.demo_dir / "other.mp4"
        other_demo.write_bytes(b"synthetic")
        self.assert_refused(other_demo)

    def test_external_aliases_to_allowed_videos_are_accepted(self):
        local = self.project / "local.mp4"
        local.write_bytes(b"synthetic")
        demo = self.demo_dir / DEMOS[0]
        demo.write_bytes(b"synthetic")
        with tempfile.TemporaryDirectory() as external:
            for name, target in (("local.mp4", local), ("demo.mp4", demo)):
                alias = Path(external) / name
                alias.symlink_to(target)
                with self.subTest(target=target):
                    self.assert_accepted(alias)

    def test_option_and_protocol_names_are_refused_but_safe_prefix_is_accepted(self):
        for name in ("-input.mp4", "file:video.mp4", "concat:video.mp4",
                     "subfile,,start,0,end,1,,:video.mp4"):
            (self.project / name).write_bytes(b"synthetic")
            with self.subTest(name=name):
                self.assert_refused(name)
                self.assert_accepted("./" + name)

    def test_video_line_breaks_and_external_output_are_refused(self):
        video = self.project / "line\nbreak.mp4"
        video.write_bytes(b"synthetic")
        self.assert_refused(video)
        good = self.project / "good.mp4"
        good.write_bytes(b"synthetic")
        with tempfile.TemporaryDirectory() as external:
            result = self.invoke(good, Path(external) / "output.mp4")
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("Refused path", result.stderr)
            self.assertFalse(self.log.exists())


if __name__ == "__main__":
    unittest.main()
