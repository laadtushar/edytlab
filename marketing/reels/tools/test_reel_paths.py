"""Regression checks for untrusted cached paths and markup token boundaries."""
import importlib.util
import itertools
import json
import subprocess
import os
from pathlib import Path
import re
import shutil
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
        import build


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


class BuildAndDocsPathsTests(unittest.TestCase):
    """Guard derived build and documentation paths without rendering media."""

    setUp = ReelPathsTests.setUp

    def load_docs(self):
        """Import the documentation writer with synthetic local usage records."""
        (self.root / "work").mkdir(exist_ok=True)
        (self.root / "out").mkdir(exist_ok=True)
        (self.root / "work/ledger.json").write_text('{"entries": [], "spent": 0}')
        (self.root / "out/usage.log").write_text("")
        module_spec = importlib.util.spec_from_file_location("reel_docs_test", TOOLS / "docs.py")
        module = importlib.util.module_from_spec(module_spec)
        with mock.patch.object(spec, "ROOT", str(self.root)), \
                mock.patch.object(sys, "path", [str(TOOLS), *sys.path]):
            module_spec.loader.exec_module(module)
        return module

    def assert_operation_refused(self, call, path):
        """Require a path-specific refusal before external I/O can occur."""
        with self.assertRaises(SystemExit) as error:
            call()
        message = str(error.exception)
        self.assertIn("Refused path", message)
        self.assertIn(str(PROJECT), message)
        self.assertIn(str(path), message)

    def test_build_writers_refuse_directory_and_leaf_symlink_escapes(self):
        """Every cue/SRT writer guards both symlinked parents and existing files."""
        tl = {"file": "synthetic", "duration": 2, "fps": 30, "cues": []}
        with tempfile.TemporaryDirectory() as external:
            target = Path(external) / "untouched"
            target.write_bytes(b"original bytes")
            directory = self.root / "out"
            directory.symlink_to(external, target_is_directory=True)
            leaf = self.root / "leaf"
            leaf.symlink_to(target)
            with mock.patch.object(build.C, "build_demo", return_value=tl), \
                    mock.patch.object(build.C, "check", return_value=[]), \
                    mock.patch.object(build.subprocess, "run") as run:
                for path in (directory / "new", leaf):
                    for writer in (
                        lambda path=path: build.write_srt(tl, {}, str(path)),
                        lambda path=path: build.write_plain_cues(tl, str(path)),
                        lambda path=path: build.write_cues(tl, {}, str(path)),
                        lambda path=path: build.write_demo_cues("BT", str(path)),
                    ):
                        with self.subTest(path=path, writer=writer):
                            self.assert_operation_refused(writer, path)
                with mock.patch.object(build, "OUT", str(directory)), \
                        mock.patch.object(build, "get_tl") as get_tl:
                    self.assert_operation_refused(
                        lambda: build.build_plain("highlights"), directory
                    )
                    get_tl.assert_not_called()
                for call in (
                    lambda: build.mean_volume(str(leaf)),
                    lambda: build.run_add("external-demo.mp4", str(leaf), str(self.root),
                                          str(self.root / "result.mp4")),
                    lambda: build.run_add("external-demo.mp4", str(self.root / "cues.json"),
                                          str(directory), str(self.root / "result.mp4")),
                    lambda: build.run_add("external-demo.mp4", str(self.root / "cues.json"),
                                          str(self.root), str(leaf)),
                ):
                    with self.assertRaises(SystemExit):
                        call()
                run.assert_not_called()
            self.assertEqual(target.read_bytes(), b"original bytes")
            self.assertEqual(sorted(path.name for path in Path(external).iterdir()), ["untouched"])

    def test_successful_json_and_srt_bytes(self):
        """Keep JSON order, Unicode, numeric formatting and final newlines exact."""
        tl = {"file": "synthetic", "duration": 2, "fps": 30, "cues": []}
        plain = self.root / "plain.json"
        build.write_plain_cues(tl, str(plain))
        self.assertEqual(plain.read_bytes(), (
            '{\n  "video": "synthetic.mp4",\n  "duration": 2,\n  "fps": 30,\n'
            '  "size": "1080x1920",\n'
            '  "note": "Plain reel: app audio only, no narration. Cut lengths are the edit list\'s '
            '(spec.py). The narration lines for the earlier takes are in transcripts.md.",\n'
            '  "cues": []\n}\n'
        ).encode())
        narrated = self.root / "narrated.json"
        build.write_cues(tl, {}, str(narrated))
        self.assertEqual(narrated.read_bytes(), (
            '{\n  "video": "synthetic.mp4",\n  "duration": 2,\n'
            '  "audio_dir": "audio/synthetic",\n'
            '  "narration_voice": "Roger - Laid-Back, Casual, Resonant (CwhRBWXzGAHq8TQ4Fs17)",\n'
            '  "narration_model": "eleven_v4",\n  "cues": [],\n  "fps": 30\n}'
        ).encode())
        demo = self.root / "demo.json"
        with mock.patch.object(build.C, "build_demo", return_value=tl), \
                mock.patch.object(build.C, "check", return_value=[]) as check:
            self.assertEqual(build.write_demo_cues("BT", str(demo)), [])
            check.assert_called_once_with(tl)
        self.assertEqual(demo.read_bytes(), (
            '{\n  "video": "dj-beatmatched-transition.mp4",\n  "duration": 2,\n'
            '  "narration_status": "The narration lines were written for the earlier (amber-UI) '
            'takes of the demos. Check each against this footage before re-narrating; '
            'see transcripts.md.",\n  "cues": []\n}\n'
        ).encode())
        srt = self.root / "captions.srt"
        tl["cues"] = [{"id": "A", "start": 0.25, "end": 2}]
        timing = {"A": {"words": [{"start": 0.1, "end": 0.5, "text": "Olá"}]}}
        self.assertEqual(build.write_srt(tl, timing, str(srt)), 1)
        self.assertEqual(srt.read_bytes(), "1\n00:00:00,350 --> 00:00:01,000\nOlá\n\n".encode())

    def test_docs_refuse_symlinked_inputs_and_output_paths(self):
        """Documentation checks timing, usage, ledger and each transcript destination."""
        docs = self.load_docs()
        tl = {"file": "synthetic", "cues": [{
            "id": "A", "narration": "Olá", "narration_spoken": "Olá"}]}
        with tempfile.TemporaryDirectory() as external:
            target = Path(external) / "untouched"
            target.write_bytes(b"original bytes")
            paths = [
                (self.root / "out/synthetic-narration-timing.json",
                 lambda: docs.timing("synthetic")),
                (self.root / "work/ledger.json", docs.usage_text),
                (self.root / "out/usage.log", docs.usage_text),
            ]
            for path, call in paths:
                if path.exists():
                    original = path.read_bytes()
                    path.unlink()
                else:
                    original = None
                path.symlink_to(target)
                self.assert_operation_refused(call, path)
                path.unlink()
                if original is not None:
                    path.write_bytes(original)
            transcripts = self.root / "out/transcripts"
            transcripts.symlink_to(external, target_is_directory=True)
            self.assert_operation_refused(docs.write_txt, transcripts / "es")
            transcripts.unlink()
            (transcripts / "es").mkdir(parents=True)
            leaf = transcripts / "A.txt"
            leaf.symlink_to(target)
            with mock.patch.object(docs, "KEYS", [("highlights", "Synthetic")]), \
                    mock.patch.object(docs, "DEMOS", {}), \
                    mock.patch.object(docs, "get_tl", return_value=tl):
                self.assert_operation_refused(docs.write_txt, leaf)
                markdown = self.root / "out/transcripts.md"
                markdown.symlink_to(target)
                with mock.patch.object(docs, "KEYS", []):
                    self.assert_operation_refused(docs.main, markdown)
                leaf.unlink()
                self.assertEqual(docs.write_txt(), 1)
                self.assertEqual(leaf.read_bytes(), "Olá\n".encode())
            timing = self.root / "out/synthetic-narration-timing.json"
            timing.write_bytes(b'{"A": {"tempo": 1.0}}')
            self.assertEqual(docs.timing("synthetic"), {"A": {"tempo": 1.0}})
            self.assertEqual(target.read_bytes(), b"original bytes")
            self.assertEqual(sorted(path.name for path in Path(external).iterdir()), ["untouched"])


class ShellPathsTests(unittest.TestCase):
    """Check shell argv, scratch and metadata boundaries."""

    def setUp(self):
        """Provide all checked commands while forbidding real media processing."""
        ReelPathsTests.setUp(self)
        self.bash = shutil.which("bash")
        self.assertIsNotNone(self.bash)
        self.bin = self.root / "bin"
        self.bin.mkdir()
        self.tool_log = self.root / "unexpected-tools.log"
        (self.bin / "python3").symlink_to(sys.executable)
        for command in ("ffmpeg", "ffprobe", "awk"):
            stub = self.bin / command
            stub.write_text('#!/bin/sh\necho "$0" >> "$REEL_TEST_TOOL_LOG"\nexit 99\n')
            stub.chmod(0o755)

    def invoke(self, args, **extra_env):
        """Run the narration shell with an isolated scratch directory."""
        return subprocess.run(
            [self.bash, str(PROJECT / "add-narration.sh"), *map(str, args)],
            env={**os.environ, "TMPDIR": str(self.root / "scratch"),
                 "PATH": str(self.bin), "REEL_TEST_TOOL_LOG": str(self.tool_log), **extra_env},
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
                self.assertFalse(self.tool_log.exists(), "media/awk stub was executed")
            result = self.invoke(args, TMPDIR=external)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("Refused path", result.stderr)
            self.assertIn(external, result.stderr)
            self.assertIn(str(PROJECT), result.stderr)
            self.assertFalse(self.tool_log.exists(), "media/awk stub was executed")

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
