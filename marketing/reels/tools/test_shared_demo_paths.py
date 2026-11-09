"""Exercise demo input boundaries at the renderer and narration invocation."""
import os
from pathlib import Path
import tempfile
import unittest
from unittest import mock

TOOLS = Path(__file__).resolve().parent
PROJECT = TOOLS.parent
with mock.patch.dict(os.environ, {"EDYTLAB_REEL_ROOT": str(PROJECT)}):
    import spec
    import demo_paths
    import render
    import build


class SharedDemoPathsTests(unittest.TestCase):
    def setUp(self):
        self.fixture = tempfile.TemporaryDirectory(dir=PROJECT)
        self.addCleanup(self.fixture.cleanup)
        self.root = Path(self.fixture.name)
        self.repo_fixture = tempfile.TemporaryDirectory()
        self.addCleanup(self.repo_fixture.cleanup)
        # Resolved: a symlinked TMPDIR (macOS /var) must not look like an escape.
        self.repo = Path(self.repo_fixture.name).resolve()
        self.demos = self.repo / "website/public/demos"
        self.demos.mkdir(parents=True)
        self.work = self.root / "work"
        self.slug = "dj-mini-mix"
        self.video = self.demos / f"{self.slug}.mp4"
        self.video.write_bytes(b"demo")
        self.repo_patch = mock.patch.object(demo_paths, "REPO_ROOT", self.repo)
        self.repo_patch.start()
        self.addCleanup(self.repo_patch.stop)

    def decode(self, directory, slug=None):
        with mock.patch.object(render, "DEMO_DIR", str(directory)), \
                mock.patch.object(render, "WORK", str(self.work)), \
                mock.patch.object(render, "run") as run:
            result = render.demo_wav(slug or self.slug)
            return result, run.call_args.args[0]

    def test_decode_preserves_allowed_input_argv(self):
        for slug in ("dj-beatmatched-transition", "dj-extended-club-intro", "dj-mini-mix"):
            (self.demos / f"{slug}.mp4").write_bytes(b"demo")
            for directory in (self.demos, self.root):
                with self.subTest(directory=directory, slug=slug):
                    result, argv = self.decode(directory, slug)
                    self.assertEqual(argv[5:7], ["-i", f"{directory}/{slug}.mp4"])
                    self.assertEqual(result, f"{self.work}/audio/{slug}.wav")

    def test_decode_rejects_external_override_and_leaf_escape(self):
        with tempfile.TemporaryDirectory() as external:
            external = Path(external)
            (external / f"{self.slug}.mp4").write_bytes(b"outside")
            for directory in (external, self.root):
                with self.subTest(directory=directory):
                    if directory == self.root:
                        (directory / f"{self.slug}.mp4").symlink_to(external / f"{self.slug}.mp4")
                    with self.assertRaises(SystemExit):
                        self.decode(directory)

    def test_shared_directory_symlink_escape_is_refused(self):
        with tempfile.TemporaryDirectory() as external:
            self.video.unlink()
            self.demos.rmdir()
            self.demos.symlink_to(external, target_is_directory=True)
            with self.assertRaises(SystemExit):
                self.decode(self.demos)

    def test_shared_file_symlink_escape_is_refused(self):
        with tempfile.TemporaryDirectory() as external:
            self.video.unlink()
            self.video.symlink_to(Path(external) / self.video.name)
            with self.assertRaises(SystemExit):
                self.decode(self.demos)

    def test_other_file_in_shared_directory_is_refused(self):
        with self.assertRaises(SystemExit):
            self.decode(self.demos, "unrelated")

    def test_render_segment_is_checked_before_media(self):
        tl = {"name": "highlights", "file": "test", "duration": 1,
              "cues": [{"kind": "step", "start": 0, "end": 1,
                        "source": {"demo": self.slug, "start": 0}}]}
        with tempfile.TemporaryDirectory() as external, \
                mock.patch.object(render, "WORK", str(self.work)), \
                mock.patch.object(render, "DEMO_DIR", external), \
                mock.patch.object(render, "background", side_effect=AssertionError("input was not checked")), \
                mock.patch.object(render, "run") as run:
            with self.assertRaises(SystemExit):
                render.render_video(tl)
            run.assert_not_called()

    def test_render_preserves_shared_demo_input_argv(self):
        tl = {"name": "highlights", "file": "test", "duration": 1, "pills": False,
              "cues": [{"kind": "step", "start": 0, "end": 1,
                        "source": {"demo": self.slug, "start": 0, "code": "MM"}}]}
        frame = self.root / "frame.png"
        with mock.patch.object(render, "WORK", str(self.work)), \
                mock.patch.object(render, "DEMO_DIR", str(self.demos)), \
                mock.patch.object(render, "background") as background, \
                mock.patch.object(render, "bar_image"), \
                mock.patch.object(render, "group_layer"), \
                mock.patch.object(render, "caption_timeline", return_value=[(str(frame), 30)]), \
                mock.patch.object(render, "run") as run:
            render.render_video(tl)
            argv = run.call_args.args[0]
            self.assertEqual(argv[5:11], ["-ss", "0.000", "-t", "1.500", "-i", str(self.video)])
            background.return_value.save.assert_called_once()

    def test_render_rechecks_input_at_media_invocation(self):
        tl = {"name": "highlights", "file": "test", "duration": 1, "pills": False,
              "cues": [{"kind": "step", "start": 0, "end": 1,
                        "source": {"demo": self.slug, "start": 0, "code": "MM"}}]}
        frame = self.root / "frame.png"
        with tempfile.TemporaryDirectory() as external, \
                mock.patch.object(render, "WORK", str(self.work)), \
                mock.patch.object(render, "DEMO_DIR", str(self.demos)), \
                mock.patch.object(render, "background") as background, \
                mock.patch.object(render, "bar_image"), \
                mock.patch.object(render, "group_layer"), \
                mock.patch.object(render, "caption_timeline", return_value=[(str(frame), 30)]), \
                mock.patch.object(render, "run") as run:
            def swap_demo():
                self.video.unlink()
                self.video.symlink_to(Path(external) / "outside.mp4")
                return mock.Mock()
            background.side_effect = swap_demo
            with self.assertRaises(SystemExit):
                render.render_video(tl)
            run.assert_not_called()

    def test_ffmpeg_writes_the_destination_that_was_checked(self):
        """ffmpeg reads each file it is given as a URL, so every writer hands it
        the resolved path the check approved. `pipe:1` resolves to a file in the
        project, but given to ffmpeg as it is, the media would go to stdout; a
        relative work folder is checked as an absolute path too. Each writer
        must call ffmpeg once, so no earlier call escapes the check.
        """
        tl = {"name": "highlights", "file": "test", "duration": 1, "pills": False,
              "cues": [{"kind": "step", "start": 0, "end": 1,
                        "source": {"demo": self.slug, "start": 0, "code": "MM"}}]}
        audio_tl = {"file": "test", "cues": [{"kind": "hook", "start": 0, "end": 1}]}
        frame = self.root / "frame.png"
        cwd = os.getcwd()
        os.chdir(self.root)
        self.addCleanup(os.chdir, cwd)
        with mock.patch.object(render, "WORK", "work"), \
                mock.patch.object(render, "DEMO_DIR", str(self.demos)), \
                mock.patch.object(render, "background"), \
                mock.patch.object(render, "bar_image"), \
                mock.patch.object(render, "group_layer"), \
                mock.patch.object(render, "caption_timeline", return_value=[(str(frame), 30)]), \
                mock.patch.object(render, "run") as run:
            def only_call(write):
                run.reset_mock()
                result = write()
                self.assertEqual(run.call_count, 1)
                return result, run.call_args.args[0]
            video, video_argv = only_call(lambda: render.render_video(tl, out_path="pipe:1"))
            audio, audio_argv = only_call(lambda: render.render_app_audio(audio_tl, "pipe:2"))
            wav, wav_argv = only_call(lambda: render.demo_wav(self.slug))
        written = {"video": (video, video_argv, self.root / "pipe:1", "mp4"),
                   "app audio": (audio, audio_argv, self.root / "pipe:2", "wav"),
                   "demo audio": (wav, wav_argv, self.work / "audio" / f"{self.slug}.wav", "wav")}
        for name, (result, argv, expected, muxer) in written.items():
            with self.subTest(writer=name):
                self.assertEqual(argv[-1], str(expected))
                self.assertEqual(argv[-3:-1], ["-f", muxer])
                self.assertEqual(result, str(expected))
        for name, argv, graph in (("video", video_argv, self.work / "test-plain" / "graph.txt"),
                                  ("app audio", audio_argv, self.work / "test-audio-graph.txt")):
            with self.subTest(graph=name):
                self.assertEqual(argv[argv.index("-filter_complex_script") + 1], str(graph))

    def test_mux_and_master_name_their_format(self):
        """The resolved destination may have no extension (`pipe:3` here), and
        ffmpeg then cannot pick a muxer, so both writers name it with `-f`."""
        (self.root / "in.mp4").write_bytes(b"v")
        (self.root / "in.wav").write_bytes(b"a")
        measured = mock.Mock(stderr="Summary:\n I: -20.0 LUFS\n Peak: -3.0 dBFS\n")
        with mock.patch.object(render, "run", return_value=measured) as run, \
                mock.patch.object(render, "measure_loudness",
                                  return_value={"integrated_lufs": -14.0, "true_peak_dbtp": -1.5}):
            render.mux(str(self.root / "in.mp4"), str(self.root / "in.wav"), str(self.root / "pipe:3"))
            mux_argv = run.call_args.args[0]
            run.reset_mock()
            render.loudnorm_two_pass(str(self.root / "in.wav"), str(self.root / "pipe:4"))
            encode_argv = run.call_args_list[-1].args[0]
        self.assertEqual(mux_argv[-3:], ["-f", "mp4", str(self.root / "pipe:3")])
        self.assertEqual(encode_argv[-3:], ["-f", "ipod", str(self.root / "pipe:4")])

    def test_narration_invocation_checks_video(self):
        with tempfile.TemporaryDirectory() as external, \
                mock.patch.object(build.subprocess, "run") as run:
            for video in (self.video, self.root / "local.mp4"):
                run.return_value = mock.Mock(returncode=0, stdout="ok", stderr="")
                self.assertEqual(build.run_add(str(video), str(self.root / "cues"),
                                               str(self.root), str(self.root / "out")), "ok")
                self.assertEqual(run.call_args.args[0][1], str(video))
            run.reset_mock()
            with self.assertRaises(SystemExit):
                build.run_add(f"{external}/external.mp4", str(self.root / "cues"),
                              str(self.root), str(self.root / "out"))
            run.assert_not_called()

    def test_narration_rejects_ffmpeg_protocols_and_option_paths(self):
        with mock.patch.object(build.subprocess, "run") as run:
            run.return_value = mock.Mock(returncode=0, stdout="", stderr="")
            for video in ("http:remote.mp4", "file:local.mp4", "-video.mp4",
                          "subfile,,start,0,end,1,,:local.mp4"):
                with self.subTest(video=video), self.assertRaises(SystemExit):
                    build.run_add(video, str(self.root / "cues"), str(self.root),
                                  str(self.root / "out"))
            run.assert_not_called()

    def test_render_checks_all_demos_before_creating_work_or_cards(self):
        tl = {"name": "highlights", "file": "test", "duration": 2,
              "cues": [{"kind": "hook", "start": 0, "end": 1, "title": ["title"]},
                       {"kind": "step", "start": 1, "end": 2,
                        "source": {"demo": self.slug, "start": 0}}]}
        with tempfile.TemporaryDirectory() as external, \
                mock.patch.object(render, "WORK", str(self.work)), \
                mock.patch.object(render, "DEMO_DIR", external), \
                mock.patch.object(render, "hook_card") as hook:
            with self.assertRaises(SystemExit):
                render.render_video(tl)
            self.assertFalse(self.work.exists())
            hook.assert_not_called()

    def test_decode_refuses_before_creating_work(self):
        with tempfile.TemporaryDirectory() as external:
            with self.assertRaises(SystemExit):
                self.decode(external)
            self.assertFalse(self.work.exists())

    def test_missing_shared_demo_is_refused_before_creating_work(self):
        self.video.unlink()
        with self.assertRaises(SystemExit):
            self.decode(self.demos)
        self.assertFalse(self.work.exists())

    def test_plain_build_checks_demos_before_writing_cues(self):
        tl = {"file": "test", "duration": 1, "cues": [
            {"kind": "step", "source": {"demo": self.slug}}]}
        with tempfile.TemporaryDirectory() as external, \
                mock.patch.object(build, "DEMO_DIR", external), \
                mock.patch.object(build, "OUT", str(self.root / "out")), \
                mock.patch.object(build, "get_tl", return_value=tl), \
                mock.patch.object(build, "render_app_audio", side_effect=AssertionError("input was not checked")), \
                mock.patch.object(build, "write_plain_cues") as write:
            with self.assertRaises(SystemExit):
                build.build_plain("highlights")
            self.assertFalse((self.root / "out").exists())
            write.assert_not_called()


if __name__ == "__main__":
    unittest.main()
