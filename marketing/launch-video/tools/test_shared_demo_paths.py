"""The launch mixer and picture decoder accept only authorized demo inputs."""
import contextlib
import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock
from PIL import Image, ImageFont

TOOLS = Path(__file__).resolve().parent
sys.path.insert(0, str(TOOLS))
import audio  # noqa: E402
import engine  # noqa: E402
import launch  # noqa: E402
import launch_paths  # noqa: E402

NAMES = ('dj-beatmatched-transition', 'dj-extended-club-intro', 'dj-mini-mix')
PCM = b'\0\0\1\0' * 8
# Exercise the real CLI with a synthetic approved file and Pillow's embedded
# font, so neither the fixture nor CI needs a host-installed system font.
FONT_CLI = r'''
import runpy
import sys
from pathlib import Path
from PIL import ImageFont
script, font = sys.argv[1:3]
sys.path.insert(0, str(Path(script).parent))
import launch_paths
launch_paths.SYSTEM_FONT = Path(font)
embedded = ImageFont.load_default()
ImageFont.truetype = lambda *args, **kwargs: embedded
sys.argv = [script, *sys.argv[3:]]
runpy.run_path(script, run_name='__main__')
'''


class SharedDemoPaths(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix='launch-demo-fixture-')
        self.addCleanup(self.tmp.cleanup)
        # Resolved: a symlinked TMPDIR (macOS /var) must not look like an escape.
        self.repo = Path(self.tmp.name).resolve() / 'repo'
        self.root = self.repo / 'marketing' / 'launch-video'
        self.root.mkdir(parents=True)
        self.demos = self.repo / 'website' / 'public' / 'demos'
        self.demos.mkdir(parents=True)
        for name in NAMES:
            (self.demos / f'{name}.mp4').write_bytes(b'synthetic recording')
        self.outside = Path(self.tmp.name) / 'outside'
        self.outside.mkdir()
        self.stack = contextlib.ExitStack()
        self.addCleanup(self.stack.close)
        self.stack.enter_context(mock.patch.object(launch_paths, 'ROOT', self.root))
        self.stack.enter_context(mock.patch.object(launch_paths, 'REPO_ROOT', self.repo, create=True))
        self.stack.enter_context(mock.patch.object(audio, 'DEMO_DIR', str(self.demos)))
        self.stack.enter_context(mock.patch.object(engine, 'DEMO_DIR', str(self.demos)))
        self.stack.enter_context(mock.patch.object(audio, 'HERE', str(self.root)))

    @staticmethod
    def cues(names):
        return {'total': 0.001, 'clips': [
            {'demo': name, 'src0': 1.25, 'dur': 0.001, 't0': 0} for name in names
        ], 'sfx': []}

    def test_allowed_inputs_reach_both_ffmpeg_commands_with_original_arguments(self):
        for name in NAMES:
            with self.subTest(name=name):
                source = f'{self.demos}/{name}.mp4'
                with mock.patch.object(audio.subprocess, 'run', return_value=subprocess.CompletedProcess([], 0, PCM, b'')) as run:
                    self.assertEqual(audio.mix_pcm(self.cues([name]), [], str(self.root), False, -3, -2), PCM + bytes(160))
                self.assertEqual(run.call_args.args[0], [
                    'nice', '-n', '10', 'ffmpeg', '-nostdin', '-v', 'error',
                    '-ss', '1.250', '-t', '0.001', '-i', source, '-vn', '-af',
                    ('aformat=sample_rates=48000:channel_layouts=stereo,afade=t=in:d=0.03,'
                     'afade=t=out:st=0.000:d=0.300,volume=-3dB'),
                    '-f', 's16le', '-ar', '48000', '-ac', '2', '-',
                ])
                with mock.patch.object(engine.subprocess, 'Popen') as popen:
                    engine.Decoder(source, 1.25, 0.001)
                self.assertEqual(popen.call_args.args[0], [
                    'nice', '-n', '10', 'ffmpeg', '-nostdin', '-v', 'error',
                    '-ss', '1.250', '-t', '0.501', '-i', source,
                    '-vf', 'fps=30', '-f', 'rawvideo', '-pix_fmt', 'rgb24', '-',
                ])

    def test_in_project_override_is_allowed(self):
        local = self.root / 'my clips'
        local.mkdir()
        with mock.patch.object(audio, 'DEMO_DIR', str(local)), \
                mock.patch.object(audio.subprocess, 'run', return_value=subprocess.CompletedProcess([], 0, PCM, b'')) as run:
            audio.mix_pcm(self.cues(['custom']), [], str(self.root), False, -3, -2)
        self.assertIn(f'{local}/custom.mp4', run.call_args.args[0])

    def test_escaped_or_non_allowlisted_inputs_never_reach_either_decoder(self):
        other = self.demos / 'other.mp4'
        other.write_bytes(b'not allowed')
        (self.outside / 'dj-mini-mix.mp4').write_bytes(b'outside')
        link = self.demos / 'alias.mp4'
        link.symlink_to(self.outside / 'dj-mini-mix.mp4')
        for directory, name in ((self.demos, 'other'), (self.demos, 'alias'),
                                (self.demos, '../../../outside/dj-mini-mix'),
                                (self.outside, 'dj-mini-mix'),
                                ('https://example.invalid', 'dj-mini-mix')):
            with self.subTest(directory=directory, name=name), \
                    mock.patch.object(audio, 'DEMO_DIR', str(directory)), \
                    mock.patch.object(audio.subprocess, 'run') as run, \
                    mock.patch.object(engine.subprocess, 'Popen') as popen:
                with self.assertRaises(SystemExit):
                    audio.mix_pcm(self.cues([name]), [], str(self.root), False, -3, -2)
                with self.assertRaises(SystemExit):
                    engine.Decoder(f'{directory}/{name}.mp4', 0, 1)
                run.assert_not_called()
                popen.assert_not_called()

    def test_allowlisted_leaf_and_demo_root_symlinks_cannot_escape_repository(self):
        leaf = self.demos / f'{NAMES[0]}.mp4'
        leaf.unlink()
        leaf.symlink_to(self.outside / 'stolen.mp4')
        (self.outside / 'stolen.mp4').write_bytes(b'outside')
        with mock.patch.object(engine.subprocess, 'Popen') as popen, self.assertRaises(SystemExit):
            engine.Decoder(str(leaf), 0, 1)
        popen.assert_not_called()
        leaf.unlink()
        for name in NAMES[1:]:
            (self.demos / f'{name}.mp4').unlink()
        self.demos.rmdir()
        self.demos.symlink_to(self.outside, target_is_directory=True)
        (self.outside / f'{NAMES[0]}.mp4').write_bytes(b'outside')
        with mock.patch.object(audio.subprocess, 'run') as run, self.assertRaises(SystemExit):
            audio.mix_pcm(self.cues([NAMES[0]]), [], str(self.root), False, -3, -2)
        run.assert_not_called()

    def test_cli_checks_all_demo_inputs_before_mkdir_or_decode(self):
        cue_file = self.root / 'cues.json'
        cue_file.write_text(json.dumps(self.cues([NAMES[0], '../bad'])))
        video = self.root / 'video.mp4'
        video.write_bytes(b'video')
        argv = ['audio.py', 'mix', str(cue_file), str(video), str(self.root / 'out.mp4'), '--no-sfx']
        with mock.patch.object(sys, 'argv', argv), \
                mock.patch.object(audio.os, 'makedirs') as mkdir, \
                mock.patch.object(audio.subprocess, 'run') as run, \
                self.assertRaises(SystemExit):
            audio.main()
        mkdir.assert_not_called()
        run.assert_not_called()

    def test_external_cli_refuses_override_before_invoking_ffmpeg(self):
        tools = self.root / 'tools'
        tools.mkdir()
        for name in ('audio.py', 'launch_paths.py'):
            shutil.copyfile(TOOLS / name, tools / name)
        binaries = self.root / 'bin'
        binaries.mkdir()
        marker = self.root / 'unexpected-ffmpeg'
        nice = binaries / 'nice'
        nice.write_text('#!/bin/sh\n: > "$DEMO_TEST_MARKER"\nexit 91\n')
        nice.chmod(0o700)
        cue_file = self.root / 'cues.json'
        cue_file.write_text(json.dumps(self.cues([NAMES[0]])))
        video = self.root / 'video.mp4'
        video.write_bytes(b'video')
        result = subprocess.run(
            [sys.executable, '-I', str(tools / 'audio.py'), 'mix', str(cue_file),
             str(video), str(self.root / 'out.mp4'), '--no-sfx'],
            env={**os.environ, 'EDYTLAB_DEMO_DIR': str(self.outside),
                 'PATH': str(binaries), 'DEMO_TEST_MARKER': str(marker)},
            capture_output=True, text=True, check=False,
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('Refused demo path', result.stderr)
        self.assertFalse(marker.exists())
        self.assertFalse((self.root / 'work').exists())

    def test_render_preflights_footage_before_starting_encoder(self):
        piece = engine.Piece('sq', 1)
        clip = engine.FootageClip(piece.ctx, 'other', 0, 1, 0,
                                  [{'crop': (0, 0, 100, 100)}], {})
        piece.add(clip)
        with mock.patch.object(engine.subprocess, 'Popen') as popen, self.assertRaises(SystemExit):
            piece.render(str(self.root / 'output.mp4'))
        popen.assert_not_called()

    def test_render_refuses_external_output_before_starting_encoder(self):
        piece = engine.Piece('sq', 1)
        with mock.patch.object(engine, 'make_bg') as background, \
                mock.patch.object(engine.subprocess, 'Popen') as popen, \
                self.assertRaises(SystemExit):
            piece.render(str(self.outside / 'output.mp4'))
        background.assert_not_called()
        popen.assert_not_called()

    def test_still_cli_refuses_bad_demo_before_creating_output_directory(self):
        piece = engine.Piece('sq', 1)
        piece.add(engine.FootageClip(piece.ctx, 'other', 0, 1, 0,
                                     [{'crop': (0, 0, 100, 100)}], {}))
        plan = mock.Mock(piece=piece)
        with mock.patch.object(sys, 'argv', ['launch.py', 'li', 'still', '0.5']), \
                mock.patch.object(launch, 'HERE', str(self.root)), \
                mock.patch.dict(launch.BUILDERS, {'li': lambda: plan}), \
                mock.patch.object(launch.os, 'makedirs') as mkdir, \
                self.assertRaises(SystemExit):
            launch.main()
        mkdir.assert_not_called()

    def test_still_skips_missing_footage_until_it_is_visible(self):
        piece = engine.Piece('sq', 10)
        piece.L.W = piece.L.H = 8
        piece.add(engine.FootageClip(piece.ctx, 'missing', 0, 1, 3,
                                     [{'crop': (0, 0, 2, 2)}], {}))
        with mock.patch.object(engine, 'make_bg', side_effect=lambda w, h: Image.new('RGB', (w, h))), \
                mock.patch.object(engine.subprocess, 'Popen') as popen:
            self.assertEqual(piece.still(0).size, (8, 8))
            self.assertEqual(piece.still(3).size, (8, 8))  # The fade starts at zero opacity.
            with self.assertRaises(SystemExit):
                piece.still(3.5)
        popen.assert_not_called()

    def test_partial_render_checks_only_recordings_used_by_its_frames(self):
        piece = engine.Piece('sq', 10)
        piece.L.W = piece.L.H = 8
        piece.add(engine.FootageClip(piece.ctx, 'missing', 0, 1, 3,
                                     [{'crop': (0, 0, 2, 2)}], {}))
        process = mock.Mock()
        with mock.patch.object(engine, 'make_bg', side_effect=lambda w, h: Image.new('RGB', (w, h))), \
                mock.patch.object(engine.subprocess, 'Popen', return_value=process) as popen:
            piece.render(str(self.root / 'out.mp4'), 0, 0.1, quiet=True)
            self.assertEqual(process.stdin.write.call_count, 3)
            piece.render(str(self.root / 'out.mp4'), 3, 3 + 1 / 30, quiet=True)
            self.assertEqual(process.stdin.write.call_count, 4)  # Invisible first frame.
            self.assertEqual(popen.call_count, 2)
            with self.assertRaises(SystemExit):
                piece.render(str(self.root / 'out.mp4'), 3, 3.1, quiet=True)
            self.assertEqual(popen.call_count, 2)

    def test_multi_still_preflights_every_selected_frame_before_mkdir(self):
        piece = engine.Piece('sq', 10)
        piece.add(engine.FootageClip(piece.ctx, 'missing', 0, 1, 3,
                                     [{'crop': (0, 0, 2, 2)}], {}))
        with mock.patch.object(sys, 'argv', ['launch.py', 'li', 'still', '0', '3.5']), \
                mock.patch.object(launch, 'HERE', str(self.root)), \
                mock.patch.dict(launch.BUILDERS, {'li': lambda: mock.Mock(piece=piece)}), \
                mock.patch.object(launch.os, 'makedirs') as mkdir, \
                self.assertRaises(SystemExit):
            launch.main()
        mkdir.assert_not_called()

    def test_direct_contact_preflights_later_frames_before_rendering_first(self):
        piece = engine.Piece('sq', 10)
        piece.add(engine.FootageClip(piece.ctx, 'missing', 0, 1, 3,
                                     [{'crop': (0, 0, 2, 2)}], {}))
        plan = mock.Mock(piece=piece, L=piece.L)
        font = self.root / 'font.ttf'
        font.write_bytes(b'synthetic font')
        with mock.patch.object(launch_paths, 'SYSTEM_FONT', font), \
                mock.patch.object(launch.ImageFont, 'truetype', return_value=ImageFont.load_default()), \
                mock.patch.object(piece, 'still', return_value=Image.new('RGB', (8, 8))) as still, \
                self.assertRaises(SystemExit):
            launch.contact(plan, [0, 3.5], self.root / 'contact.png', w=8)
        still.assert_not_called()

    @staticmethod
    def test_launch_font_requests_launch_project_scope():
        with mock.patch.dict(engine._fonts, {}, clear=True), \
                mock.patch.object(engine.brand, 'font', return_value=object()) as font:
            engine.font('Bold', 24)
        font.assert_called_once_with('Bold', 24, project='launch-video')

    def test_demo_file_urls_strip_one_prefix_and_reject_virtual_streams(self):
        local = self.root / 'local.mp4'
        local.write_bytes(b'local')
        for raw, expected in ((str(local), str(local)), ('file:' + str(local), str(local)),
                              ('file:file:literal.mp4', 'file:literal.mp4'),
                              ('-ordinary.mp4', '-ordinary.mp4')):
            with self.subTest(raw=raw), mock.patch.object(launch_paths, 'checked_path') as check:
                self.assertEqual(launch_paths.checked_demo_path(raw), raw)
                check.assert_called_once_with(expected)
        for raw in ('-', 'pipe:0', 'fd:0', 'concat:one|two', 'https://example.invalid/demo.mp4',
                    'subfile,,start,0,end,1,,:outside.mp4', 'file:' + str(self.outside / 'x.mp4')):
            with self.subTest(raw=raw), mock.patch.object(engine.subprocess, 'Popen') as popen:
                with self.assertRaises(SystemExit):
                    engine.Decoder(raw, 0, 1)
                popen.assert_not_called()

    def test_approved_demo_is_allowed_through_a_linked_checkout_parent(self):
        """The default folder is built with abspath, so a linked checkout spelling must pass."""
        alias = Path(self.tmp.name) / 'checkout-alias'
        alias.symlink_to(self.repo, target_is_directory=True)
        for name in NAMES:
            with self.subTest(name=name):
                raw = str(alias / 'website' / 'public' / 'demos' / f'{name}.mp4')
                self.assertEqual(launch_paths.checked_demo_path(raw), raw)
        leaf = self.demos / f'{NAMES[0]}.mp4'
        leaf.unlink()
        leaf.symlink_to(self.outside / 'stolen.mp4')
        (self.outside / 'stolen.mp4').write_bytes(b'outside')
        with self.assertRaises(SystemExit):
            launch_paths.checked_demo_path(str(alias / 'website' / 'public' / 'demos' / leaf.name))

    def test_demo_root_redirected_inside_the_repository_is_refused(self):
        """A linked demo folder cannot swap in other repository footage with approved names."""
        archive = self.repo / 'marketing' / 'archive' / 'demos'
        archive.parent.mkdir(parents=True)
        self.demos.rename(archive)
        self.demos.symlink_to(archive, target_is_directory=True)
        for name in NAMES:
            with self.subTest(name=name), self.assertRaises(SystemExit):
                launch_paths.checked_demo_path(str(self.demos / f'{name}.mp4'))

    def test_demo_root_cannot_resolve_outside_even_if_leaf_points_back_inside(self):
        for name in NAMES:
            (self.demos / f'{name}.mp4').unlink()
        self.demos.rmdir()
        self.demos.symlink_to(self.outside, target_is_directory=True)
        local = self.root / 'local.mp4'
        local.write_bytes(b'local')
        (self.outside / f'{NAMES[0]}.mp4').symlink_to(local)
        with mock.patch.object(engine.subprocess, 'Popen') as popen, self.assertRaises(SystemExit):
            engine.Decoder(str(self.demos / f'{NAMES[0]}.mp4'), 0, 1)
        popen.assert_not_called()

    def test_contact_sheet_checks_system_font_before_reading_it(self):
        font = self.root / 'system' / 'DejaVuSans-Bold.ttf'
        font.parent.mkdir()
        external = self.outside / 'font.ttf'
        external.write_bytes(b'external font')
        font.symlink_to(external)
        plan = mock.Mock()
        plan.L.W = 10
        plan.L.H = 10
        plan.piece.still.return_value = Image.new('RGB', (10, 10), 'white')
        output = self.root / 'contact.png'
        with mock.patch.object(launch_paths, 'SYSTEM_FONT', font, create=True), \
                mock.patch.object(launch.ImageFont, 'truetype', return_value=ImageFont.load_default()) as reader, \
                self.assertRaises(SystemExit):
            launch.contact(plan, [0], output, w=20)
        reader.assert_not_called()
        self.assertFalse(output.exists())
        font.unlink()
        font.write_bytes(b'synthetic font')
        with mock.patch.object(launch_paths, 'SYSTEM_FONT', font), \
                mock.patch.object(launch.ImageFont, 'truetype', return_value=ImageFont.load_default()) as reader:
            launch.contact(plan, [0], output, w=20)
        reader.assert_called_once_with(str(font), 16)
        self.assertTrue(output.is_file())

    def test_system_font_parent_alias_and_missing_file_are_refused(self):
        fonts = self.root / 'fonts'
        fonts.symlink_to(self.outside, target_is_directory=True)
        font = fonts / 'DejaVuSans-Bold.ttf'
        (self.outside / font.name).write_bytes(b'external font')
        with mock.patch.object(launch_paths, 'SYSTEM_FONT', font), self.assertRaises(SystemExit):
            launch_paths.checked_system_font()
        with mock.patch.object(launch_paths, 'SYSTEM_FONT', self.root / 'missing.ttf'), \
                self.assertRaises(SystemExit):
            launch_paths.checked_system_font()

    def test_grid_cli_preflights_paths_and_preserves_frame_command_and_image(self):
        tools = self.root / 'tools'
        tools.mkdir()
        for name in ('grid.py', 'launch_paths.py'):
            shutil.copyfile(TOOLS / name, tools / name)
        binaries = self.root / 'bin'
        binaries.mkdir()
        png = self.root / 'frame.png'
        Image.new('RGB', (1280, 776), (10, 20, 30)).save(png)
        arguments = self.root / 'arguments.txt'
        nice = binaries / 'nice'
        nice.write_text('#!/bin/sh\nprintf "%s\\n" "$@" > "$GRID_TEST_ARGS"\n/bin/cat "$GRID_TEST_PNG"\n')
        nice.chmod(0o700)
        output = self.root / 'grid.png'
        env = {**os.environ, 'PATH': str(binaries), 'GRID_TEST_ARGS': str(arguments),
               'GRID_TEST_PNG': str(png)}
        font = self.root / 'font.ttf'
        font.write_bytes(b'synthetic font')
        command = [sys.executable, '-I', '-c', FONT_CLI, str(tools / 'grid.py'), str(font)]
        alias = self.root / 'escaped-alias.png'
        alias.symlink_to(self.outside / 'escaped-alias.png')
        for destination, source in ((self.outside / 'escaped.png', self.demos),
                                    (alias, self.demos),
                                    (output, self.outside)):
            with self.subTest(destination=destination, source=source):
                result = subprocess.run(command + [str(destination), 'BT:1.25'],
                                        env={**env, 'EDYTLAB_DEMO_DIR': str(source)},
                                        capture_output=True, text=True, check=False)
                self.assertNotEqual(result.returncode, 0)
                self.assertFalse(arguments.exists())
                self.assertFalse(destination.exists())
        second = self.demos / f'{NAMES[1]}.mp4'
        second.unlink()
        second.symlink_to(self.outside / 'second.mp4')
        result = subprocess.run(command + [str(output), 'BT:1.25', 'EI:2'],
                                env={**env, 'EDYTLAB_DEMO_DIR': str(self.demos)},
                                capture_output=True, text=True, check=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(arguments.exists())
        self.assertFalse(output.exists())
        font.unlink()
        font.symlink_to(self.outside / 'font.ttf')
        (self.outside / 'font.ttf').write_bytes(b'external font')
        result = subprocess.run(command + [str(output), 'BT:1.25'],
                                env={**env, 'EDYTLAB_DEMO_DIR': str(self.demos)},
                                capture_output=True, text=True, check=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('Refused system font', result.stderr)
        self.assertFalse(arguments.exists())
        self.assertFalse(output.exists())
        font.unlink()
        font.write_bytes(b'synthetic font')
        result = subprocess.run(command + [str(output), 'BT:1.25'],
                                env={**env, 'EDYTLAB_DEMO_DIR': str(self.demos)},
                                capture_output=True, text=True, check=False)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(arguments.read_text().splitlines(), [
            '-n', '10', 'ffmpeg', '-nostdin', '-v', 'error', '-ss', '1.250', '-i',
            f'{self.demos}/{NAMES[0]}.mp4', '-frames:v', '1', '-f', 'image2pipe',
            '-vcodec', 'png', '-',
        ])
        with Image.open(output) as sheet:
            self.assertEqual(sheet.size, (2560, 776))
            self.assertEqual(sheet.getpixel((3, 3)), (10, 20, 30))
            self.assertEqual(sheet.getpixel((50, 51)), (90, 0, 90))
            self.assertEqual(sheet.getpixel((1300, 20)), (0, 0, 0))

    def test_sheet_cli_checks_fixed_font_before_media(self):
        tools = self.root / 'tools'
        tools.mkdir()
        for name in ('sheet.py', 'launch_paths.py'):
            shutil.copyfile(TOOLS / name, tools / name)
        binaries = self.root / 'bin'
        binaries.mkdir()
        marker = self.root / 'media-ran'
        png = self.root / 'frame.png'
        Image.new('RGB', (1280, 776), (10, 20, 30)).save(png)
        probe = binaries / 'ffprobe'
        probe.write_text('#!/bin/sh\n: > "$FONT_TEST_MARKER"\nprintf "1280,776,10\\n"\n')
        probe.chmod(0o700)
        nice = binaries / 'nice'
        nice.write_text('#!/bin/sh\n/bin/cat "$FONT_TEST_PNG"\n')
        nice.chmod(0o700)
        font = self.root / 'font.ttf'
        font.symlink_to(self.outside / 'font.ttf')
        (self.outside / 'font.ttf').write_bytes(b'external font')
        video = self.root / 'video.mp4'
        video.write_bytes(b'video')
        output = self.root / 'sheet.png'
        command = [sys.executable, '-I', '-c', FONT_CLI, str(tools / 'sheet.py'),
                   str(font), str(video), str(output), '2', '128', '1']
        env = {**os.environ, 'PATH': str(binaries), 'FONT_TEST_MARKER': str(marker),
               'FONT_TEST_PNG': str(png)}
        result = subprocess.run(command, env=env, capture_output=True, text=True, check=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('Refused system font', result.stderr)
        self.assertFalse(marker.exists())
        self.assertFalse(output.exists())
        font.unlink()
        font.write_bytes(b'synthetic font')
        result = subprocess.run(command, env=env, capture_output=True, text=True, check=False)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(marker.exists())
        with Image.open(output) as sheet:
            self.assertEqual(sheet.size, (256, 78))
            self.assertEqual(sheet.getpixel((80, 40)), (10, 20, 30))


if __name__ == '__main__':
    unittest.main()
