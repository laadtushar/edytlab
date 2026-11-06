"""Run standalone demo utilities without external media or a reel-root setting."""
import os
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from PIL import Image

TOOLS = Path(__file__).resolve().parent
PROJECT = TOOLS.parent


class StandaloneDemoPathsTests(unittest.TestCase):
    def setUp(self):
        self.fixture = tempfile.TemporaryDirectory(dir=PROJECT)
        self.addCleanup(self.fixture.cleanup)
        self.root = Path(self.fixture.name)
        self.demos = self.root / "demos"
        self.demos.mkdir()
        for name in ("dj-beatmatched-transition", "dj-extended-club-intro", "dj-mini-mix"):
            (self.demos / f"{name}.mp4").write_bytes(b"demo")
        self.bin = self.root / "bin"
        self.bin.mkdir()
        nice = self.bin / "nice"
        nice.write_text('#!/usr/bin/python3\n'
                        'import json, os, pathlib, sys\n'
                        'with open(os.environ["MEDIA_MARKER"], "a") as f: f.write(json.dumps(sys.argv[1:]) + "\\n")\n'
                        'if "image2pipe" in sys.argv: sys.stdout.buffer.write(pathlib.Path(os.environ["TEST_PNG"]).read_bytes())\n')
        nice.chmod(0o755)
        self.png = self.root / "frame.png"
        Image.new("RGB", (2, 2)).save(self.png)
        self.marker = self.root / "media-called"

    def run_tool(self, script, args, demos=None):
        env = os.environ.copy()
        env.pop("EDYTLAB_REEL_ROOT", None)
        env.update(EDYTLAB_DEMO_DIR=str(demos or self.demos),
                   PATH=f"{self.bin}:{env['PATH']}", MEDIA_MARKER=str(self.marker),
                   TEST_PNG=str(self.png))
        return subprocess.run([sys.executable, str(TOOLS / script), *map(str, args)],
                              cwd=PROJECT, env=env, capture_output=True, text=True)

    def test_utilities_reject_external_override_before_media(self):
        with tempfile.TemporaryDirectory() as external:
            for script, args in (("detect_captions.py", ["dj-mini-mix"]),
                                 ("rms_listen_regions.py", []),
                                 ("sheet.py", [self.root / "out.png", "1", "32", "dj-mini-mix:0"])):
                with self.subTest(script=script):
                    result = self.run_tool(script, args, external)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn("Refused path", result.stderr)
                    self.assertFalse(self.marker.exists())

    def test_utility_rejects_traversal_and_leaf_symlink(self):
        with tempfile.TemporaryDirectory() as external:
            video = self.demos / "dj-mini-mix.mp4"
            video.unlink()
            video.symlink_to(Path(external) / "outside.mp4")
            for slug in ("dj-mini-mix", "../../../../../../tmp/outside"):
                with self.subTest(slug=slug):
                    result = self.run_tool("detect_captions.py", [slug])
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn("Refused path", result.stderr)
                    self.assertFalse(self.marker.exists())

    def test_sheet_refuses_external_output_before_media(self):
        with tempfile.TemporaryDirectory() as external:
            output = Path(external) / "out.png"
            alias = self.root / "output-link.png"
            alias.symlink_to(output)
            for target in (output, alias):
                with self.subTest(target=target):
                    result = self.run_tool("sheet.py", [target, "1", "32", "dj-mini-mix:0"])
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn("Refused path", result.stderr)
                    self.assertFalse(self.marker.exists())
                    self.assertFalse(output.exists())

    def test_utilities_preflight_later_inputs_before_media(self):
        with tempfile.TemporaryDirectory() as external:
            video = self.demos / "dj-mini-mix.mp4"
            video.unlink()
            video.symlink_to(Path(external) / "outside.mp4")
            for script, args in (("detect_captions.py", ["dj-beatmatched-transition", "dj-mini-mix"]),
                                 ("rms_listen_regions.py", []),
                                 ("sheet.py", [self.root / "out.png", "1", "32",
                                               "dj-beatmatched-transition:0", "dj-mini-mix:0"])):
                with self.subTest(script=script):
                    self.marker.unlink(missing_ok=True)
                    result = self.run_tool(script, args)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn("Refused path", result.stderr)
                    self.assertFalse(self.marker.exists())

    def test_all_utilities_accept_local_inputs_without_reel_root(self):
        output = self.root / "out.png"
        for script, args in (("detect_captions.py", ["dj-mini-mix"]),
                             ("rms_listen_regions.py", []),
                             ("sheet.py", [output, "1", "32", "dj-mini-mix:0"])):
            with self.subTest(script=script):
                self.marker.unlink(missing_ok=True)
                result = self.run_tool(script, args)
                self.assertEqual(result.returncode, 0, result.stderr)
                argv = json.loads(self.marker.read_text().splitlines()[0])
                expected = "dj-beatmatched-transition" if script == "rms_listen_regions.py" else "dj-mini-mix"
                self.assertEqual(argv[argv.index("-i") + 1], str(self.demos / f"{expected}.mp4"))
        self.assertTrue(output.is_file())
        self.assertTrue(self.marker.is_file())


if __name__ == "__main__":
    unittest.main()
