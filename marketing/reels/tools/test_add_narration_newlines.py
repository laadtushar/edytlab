"""Exercise the public narration script with symlinked line-break paths."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

from stub_commands import write_python_command


PROJECT = Path(__file__).resolve().parent.parent
SCRIPT = PROJECT / "add-narration.sh"


class NarrationLineBreakTests(unittest.TestCase):
    """Paths written to line-delimited ffmpeg arguments must be single-line."""

    def setUp(self):
        self.fixture = tempfile.TemporaryDirectory(dir=PROJECT)
        self.addCleanup(self.fixture.cleanup)
        self.root = Path(self.fixture.name)
        self.bin = self.root / "bin"
        self.bin.mkdir()
        probe = self.bin / "ffprobe"
        probe.write_text("#!/bin/sh\necho 0.5\n")
        probe.chmod(0o755)
        ffmpeg = self.bin / "ffmpeg"
        ffmpeg.write_text(
            "#!/bin/sh\n"
            "case \" $* \" in *' silencedetect='*) exit 0 ;; esac\n"
            "echo mix-reached >&2\nexit 73\n"
        )
        ffmpeg.chmod(0o755)
        self.video = self.root / "video.mp4"
        self.video.write_bytes(b"video")
        self.audio = self.root / "audio"
        self.audio.mkdir()
        (self.audio / "R1.mp3").write_bytes(b"audio")
        self.cues = self.root / "cues.json"
        self.doc = {"cues": [{"id": "R1", "start": 0, "end": 1}]}
        self.write_cues()
        self.out = self.root / "out.mp4"

    def write_cues(self):
        self.cues.write_text(json.dumps(self.doc))

    def invoke(self, cues=None, audio=None, out=None, tmpdir=None):
        env = {**os.environ, "PATH": str(self.bin) + os.pathsep + os.environ["PATH"],
               "TMPDIR": str(tmpdir or self.root / "scratch")}
        return subprocess.run(
            ["bash", str(SCRIPT), str(self.video), str(cues or self.cues),
             str(audio or self.audio), str(out or self.out)],
            env=env, capture_output=True, text=True, check=False,
        )

    def assert_refused(self, result):
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Refused path", result.stderr)
        self.assertIn("line breaks", result.stderr)
        self.assertNotIn("mix-reached", result.stderr)

    def test_resolved_argument_paths_with_line_breaks_are_refused(self):
        for char in ("\n", "\r"):
            for kind in ("cues", "audio", "out"):
                with self.subTest(char=repr(char), kind=kind):
                    target = self.root / (kind + char + "target")
                    link = self.root / (kind + "-link")
                    if kind == "cues":
                        target.write_text(json.dumps(self.doc))
                    elif kind == "audio":
                        target.mkdir()
                    link.symlink_to(target, target_is_directory=kind == "audio")
                    result = self.invoke(**{kind: link})
                    link.unlink()
                    if target.is_dir():
                        target.rmdir()
                    elif target.exists():
                        target.unlink()
                    self.assert_refused(result)

    def test_resolved_clip_and_effect_paths_with_line_breaks_are_refused(self):
        for char in ("\n", "\r"):
            for kind in ("clip", "effect"):
                with self.subTest(char=repr(char), kind=kind):
                    target = self.root / (kind + char + "target.mp3")
                    target.write_bytes(b"audio")
                    link = self.audio / "R1.mp3" if kind == "clip" else self.root / "effect.mp3"
                    if kind == "clip":
                        link.unlink()
                    link.symlink_to(target)
                    if kind == "effect":
                        self.doc["sfx"] = [{"file": "effect.mp3", "at": 0}]
                        self.write_cues()
                    result = self.invoke()
                    link.unlink()
                    target.unlink()
                    if kind == "clip":
                        link.write_bytes(b"audio")
                    else:
                        del self.doc["sfx"]
                        self.write_cues()
                    self.assert_refused(result)

    def test_safe_local_symlinks_reach_mixing(self):
        link = self.root / "safe-audio"
        link.symlink_to(self.audio, target_is_directory=True)
        result = self.invoke(audio=link)
        self.assertEqual(result.returncode, 73, result.stderr)
        self.assertIn("mix-reached", result.stderr)

    def test_line_break_scratch_symlink_uses_fallback(self):
        write_python_command(
            self.bin / "mkdir", "import sys\nprint(sys.argv[2], file=sys.stderr)\nsys.exit(73)\n"
        )
        for char in ("\n", "\r"):
            with self.subTest(char=repr(char)):
                target = self.root / ("scratch" + char + "target")
                target.mkdir()
                link = self.root / "scratch-link"
                link.symlink_to(target, target_is_directory=True)
                result = self.invoke(tmpdir=link)
                link.unlink()
                target.rmdir()
                self.assertEqual(result.returncode, 73, result.stderr)
                self.assertEqual(result.stderr.strip(), str(PROJECT / "work"))


if __name__ == "__main__":
    unittest.main()
