"""Regression checks for untrusted cached paths and markup token boundaries."""
import itertools
import json
import subprocess
import os
from pathlib import Path
import re
import sys
import tempfile
import unittest
from unittest import mock

TOOLS = Path(__file__).resolve().parent
PROJECT = TOOLS.parent
with mock.patch.dict(os.environ, {"EDYTLAB_REEL_ROOT": str(PROJECT)}):
    with mock.patch.object(sys, "path", [str(TOOLS), *sys.path]):
        import spec
        import narrate
        import render


class ReelPathsTests(unittest.TestCase):
    """Check permitted and refused narration paths."""

    def setUp(self):
        """Create an isolated project-local fixture directory."""
        self.fixture = tempfile.TemporaryDirectory(dir=PROJECT)
        self.addCleanup(self.fixture.cleanup)
        self.root = Path(self.fixture.name)

    def assert_refused(self, path):
        """Require the refusal to identify both path and project root."""
        with self.assertRaises(SystemExit) as error:
            spec.checked_reel_path(path)
        self.assertIn(str(path), str(error.exception))
        self.assertIn(str(PROJECT), str(error.exception))

    def test_existing_and_new_paths(self):
        """Existing and new paths."""
        source = self.root / "input.mp3"
        source.write_bytes(b"synthetic input")
        for path in (source, self.root / "work" / "new.mp3"):
            self.assertEqual(spec.checked_reel_path(path), str(path.resolve()))

    def test_external_parent_and_symlink_escapes(self):
        """External parent and symlink escapes."""
        with tempfile.TemporaryDirectory() as external:
            self.assert_refused(Path(external) / "input.mp3")
            self.assert_refused(PROJECT / ".." / "outside.mp3")
            link = self.root / "link"
            link.symlink_to(external, target_is_directory=True)
            self.assert_refused(link / "new.mp3")

    def test_prepare_rejects_cached_source_before_subprocess(self):
        """Prepare rejects cached source before subprocess."""
        tl = {"file": "reel", "cues": [{"id": "R1", "narration": "hi", "narration_spoken": "hi"}]}
        with mock.patch.object(narrate, "OUT", str(self.root)), \
                mock.patch.object(narrate, "cached", return_value={"mp3": "/tmp/external.mp3"}), \
                mock.patch.object(narrate.subprocess, "run") as run:
            with self.assertRaises(SystemExit):
                narrate.prepare(tl)
            run.assert_not_called()

    def test_prepare_rejects_traversal_and_output_symlink(self):
        """Prepare rejects traversal and output symlink."""
        source = self.root / "source.mp3"
        source.write_bytes(b"synthetic input")
        for filename in ("../../../../outside", "reel"):
            tl = {"file": filename, "cues": []}
            with tempfile.TemporaryDirectory() as external:
                if filename == "reel":
                    (self.root / "reel-narration-timing.json").symlink_to(
                        Path(external) / "timing.json"
                    )
                with mock.patch.object(narrate, "OUT", str(self.root)), \
                        mock.patch.object(narrate.subprocess, "run") as run:
                    with self.assertRaises(SystemExit):
                        narrate.prepare(tl)
                    run.assert_not_called()

    def test_prepare_rejects_cue_output_symlink_before_unlink(self):
        """Prepare rejects cue output symlink before unlink."""
        audio = self.root / "audio" / "reel"
        audio.mkdir(parents=True)
        with tempfile.TemporaryDirectory() as external:
            target = Path(external) / "outside.mp3"
            target.write_bytes(b"untouched")
            (audio / "R1.mp3").symlink_to(target)
            tl = {"file": "reel", "cues": [
                {"id": "R1", "narration": "hi", "narration_spoken": "hi"}]}
            with mock.patch.object(narrate, "OUT", str(self.root)), \
                    mock.patch.object(narrate, "cached", return_value={
                        "mp3": str(self.root / "source.mp3")}), \
                    mock.patch.object(narrate.subprocess, "run") as run:
                with self.assertRaises(SystemExit):
                    narrate.prepare(tl)
                run.assert_not_called()
            self.assertEqual(target.read_bytes(), b"untouched")
            self.assertTrue((audio / "R1.mp3").is_symlink())


class RendererPathsTests(unittest.TestCase):
    """Reject renderer paths before filesystem or media operations."""
    setUp = ReelPathsTests.setUp

    def test_renderer_destinations_refused_before_work(self):
        """Renderer destinations refused before work."""
        inside = str(self.root / "source.wav")
        with tempfile.TemporaryDirectory() as external:
            outside = str(Path(external) / "output.mp4")
            calls = [
                lambda: render.hook_card(outside, ["title"], ""),
                lambda: render.end_card(outside),
                lambda: render.group_layer(outside, None, ""),
                lambda: render.bar_image(outside),
                lambda: render.blank_caption(outside),
                lambda: render.write_concat(outside, [(inside, 1)]),
                lambda: render.write_concat(inside, [(outside, 1)]),
                lambda: render.render_video(
                    {"name": "highlights", "file": "synthetic"}, out_path=outside),
                lambda: render.render_app_audio({"file": "synthetic"}, outside),
                lambda: render.loudnorm_two_pass(inside, outside),
                lambda: render.mux(inside, inside, outside),
                lambda: render.contact_sheet(inside, {}, outside),
            ]
            with mock.patch.object(render, "run") as run:
                for call in calls:
                    with self.assertRaises(SystemExit):
                        call()
                run.assert_not_called()

    def test_renderer_work_and_new_destination_symlinks_refused(self):
        """Renderer work and new destination symlinks refused."""
        with tempfile.TemporaryDirectory() as external:
            alias = self.root / "work"
            alias.symlink_to(external, target_is_directory=True)
            with mock.patch.object(render, "WORK", str(alias)), \
                    mock.patch.object(render, "run") as run:
                with self.assertRaises(SystemExit):
                    render.demo_wav("synthetic")
                with self.assertRaises(SystemExit):
                    render.render_video({"name": "highlights", "file": "synthetic"})
                run.assert_not_called()


class ShellPathsTests(unittest.TestCase):
    """Check shell argv, scratch and metadata boundaries."""
    setUp = ReelPathsTests.setUp

    def invoke(self, args, **extra_env):
        """Run the narration shell with an isolated scratch directory."""
        return subprocess.run(
            ["bash", str(PROJECT / "add-narration.sh"), *map(str, args)],
            env={**os.environ, "TMPDIR": str(self.root / "scratch"), **extra_env},
            capture_output=True, text=True, check=False,
        )

    def test_shell_argv_and_scratch_escapes_refused(self):
        """Shell argv and scratch escapes refused."""
        video, cues = self.root / "video.mp4", self.root / "cues.json"
        video.write_bytes(b"synthetic")
        cues.write_text('{"cues": []}')
        args = [video, cues, self.root, self.root / "output.mp4"]
        with tempfile.TemporaryDirectory() as external:
            for index in (1, 2, 3):
                escaped = args.copy()
                escaped[index] = Path(external) / "outside"
                result = self.invoke(escaped)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("Refused path", result.stderr)
                self.assertIn(str(PROJECT), result.stderr)
                self.assertIn(str(escaped[index]), result.stderr)
            result = self.invoke(args, TMPDIR=external)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("Refused path", result.stderr)
            self.assertIn(external, result.stderr)
            self.assertIn(str(PROJECT), result.stderr)

    def test_shell_cue_and_sfx_escapes_before_audio_processing(self):
        # Invoke the shell's cue-reader block with synthetic duration, so this
        # regression proves metadata confinement without depending on FFmpeg.
        """Shell cue and sfx escapes before audio processing."""
        source = (PROJECT / "add-narration.sh").read_text()
        block = source.split('"$ADIR_DISPLAY" <<\'PY\'\n', 1)[1].split("\nPY\n", 1)[0]
        cues = self.root / "cues.json"
        with tempfile.TemporaryDirectory() as external:
            docs = [
                {"cues": [{"id": "../../../../outside", "start": 0, "end": 1}]},
                {"cues": [{"id": "R1", "start": 0, "end": 1}],
                    "sfx": [{"file": external, "at": 0}]},
            ]
            # A local clip lets the second fixture reach sfx validation. Mock
            # the probe calls by executing this block in-process.
            (self.root / "R1.mp3").write_bytes(b"synthetic")
            for doc in docs:
                cues.write_text(json.dumps(doc))
                argv = ["-", str(self.root / "video.mp4"), str(cues), str(self.root),
                        str(self.root), "2", str(PROJECT), str(self.root)]
                fake = mock.Mock(stdout="0.5", stderr="")
                with mock.patch.object(sys, "argv", argv), \
                        mock.patch.object(subprocess, "run", return_value=fake):
                    with self.assertRaises(SystemExit) as error:
                        exec(compile(block, "<shell cue-reader>", "exec"), {})
                    self.assertIn("Refused path", str(error.exception))
                    self.assertIn(str(PROJECT), str(error.exception))


class MarkupTests(unittest.TestCase):
    """Preserve markup token precedence without backtracking."""

    def test_original_token_precedence(self):
        """Original token precedence."""
        original = re.compile(r"\[[^\]]+\]|\S*\{[^}]*\}\S*|\S+")
        for size in range(7):
            for chars in itertools.product("x{}[] \n", repeat=size):
                text = "".join(chars)
                self.assertEqual(spec.markup_tokens(text), original.findall(text), repr(text))

    def test_unclosed_markup_stays_literal(self):
        """Unclosed markup stays literal."""
        for text in ("{" * 100_000, "[ " * 50_000):
            self.assertEqual(spec.markup_tokens(text), text.split())


if __name__ == "__main__":
    unittest.main()
