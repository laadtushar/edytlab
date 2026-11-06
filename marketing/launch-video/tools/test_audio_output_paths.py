"""Audio writers refuse escaped outputs while keeping synthetic bytes and argv."""
import ast
import contextlib
import importlib.util
import io
import json
import os
import subprocess
import sys
import tempfile
import unittest
import wave
from pathlib import Path
from unittest import mock

TOOLS = Path(__file__).resolve().parent
PCM = b'\0\0\1\0' * 8
SOUNDS = ('whoosh', 'tick', 'chime')


def load_audio(path=TOOLS / 'audio.py'):
    """Import the actual mixer; its media functions are replaced by each fixture."""
    spec = importlib.util.spec_from_file_location('launch_audio_output_fixture', path)
    module = importlib.util.module_from_spec(spec)
    with mock.patch.object(sys, 'path', list(sys.path)):
        spec.loader.exec_module(module)
    return module


def run_sfx(directory, path=TOOLS / 'sfx.py'):
    """Run the real SFX entrypoint using tiny synthetic arrays instead of sounds."""
    module = ast.parse(path.read_text())
    for node in module.body:
        if isinstance(node, ast.FunctionDef) and node.name in SOUNDS:
            node.body = ast.parse('return [0.0, 0.125, -0.25]').body
    ast.fix_missing_locations(module)
    namespace = {'__file__': str(path), '__name__': 'launch_sfx_output_fixture'}
    with mock.patch.object(sys, 'argv', ['sfx.py', str(directory)]), \
            mock.patch.object(sys, 'path', list(sys.path)):
        exec(compile(module, str(path), 'exec'), namespace)  # skipcq: PYL-W0122
    return namespace


class AudioOutputPaths(unittest.TestCase):
    """No scratch, ffmpeg or WAV write may cross the launch project boundary."""

    def setUp(self):
        """Create only isolated synthetic inputs and use the real path validator."""
        self.temporary = tempfile.TemporaryDirectory(prefix='launch writer [fixture] ')
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.external = tempfile.TemporaryDirectory()
        self.addCleanup(self.external.cleanup)
        self.outside = Path(self.external.name)
        self.audio = load_audio()
        self.stack = contextlib.ExitStack()
        self.addCleanup(self.stack.close)
        self.stack.enter_context(mock.patch.object(sys.modules['launch_paths'], 'ROOT', self.root))
        self.stack.enter_context(mock.patch.object(self.audio, 'HERE', str(self.root)))
        self.work = self.root / 'work' / 'audio'
        self.work.mkdir(parents=True)
        (self.root / 'out').mkdir()
        self.out = self.root / 'out' / 'final track.mp4'
        self.cues = self.root / 'cues.json'
        self.cues.write_text(json.dumps({'total': 1, 'clips': [], 'sfx': []}))
        self.video = self.root / 'video.mp4'
        self.video.write_bytes(b'synthetic input')
        self.mixed = self.work / 'final track_mix.wav'
        self.normed = self.work / 'final track_norm.wav'

    def audio_argv(self):
        """Retain caller path spelling, including spaces, in the actual CLI."""
        return ['audio.py', 'mix', str(self.cues), str(self.video), str(self.out), '--no-sfx']

    def run_audio(self):
        """Exercise the real mixer control flow with synthetic PCM and ffmpeg bytes."""
        calls = []

        def ffmpeg(command, **_kwargs):
            """Record the exact encoder arguments and write only synthetic fixture bytes."""
            calls.append(command)
            Path(command[-1]).write_bytes(b'synthetic ffmpeg output')

        with mock.patch.object(sys, 'argv', self.audio_argv()), \
                mock.patch.object(self.audio, 'mix_pcm', return_value=PCM), \
                mock.patch.object(self.audio, 'measure', return_value=(-14.0, -2.0)), \
                mock.patch.object(self.audio, 'run', side_effect=ffmpeg), \
                contextlib.redirect_stdout(io.StringIO()) as stdout:
            self.audio.main()
        return calls, stdout.getvalue()

    def test_mixer_refuses_directory_and_every_leaf_before_any_output_or_decode(self):
        """Existing and dangling aliases cannot redirect any of the three writers."""
        for destination in (self.work, self.mixed, self.normed, self.out):
            for dangling in (False, True):
                with self.subTest(destination=destination, dangling=dangling):
                    target = self.outside / ('missing' if dangling else 'target')
                    if not dangling:
                        if destination == self.work:
                            target.mkdir()
                        else:
                            target.write_bytes(b'outside sentinel')
                    if destination == self.work:
                        destination.rmdir()
                    destination.symlink_to(target, target_is_directory=destination == self.work)
                    with mock.patch.object(sys, 'argv', self.audio_argv()), \
                            mock.patch.object(self.audio.os, 'makedirs') as mkdir, \
                            mock.patch.object(self.audio, 'mix_pcm') as decode, \
                            mock.patch.object(self.audio, 'run') as ffmpeg, \
                            self.assertRaises(SystemExit) as error:
                        self.audio.main()
                    self.assertIn('Refused path', str(error.exception))
                    self.assertIn(str(destination), str(error.exception))
                    self.assertIn(str(self.root), str(error.exception))
                    mkdir.assert_not_called()
                    decode.assert_not_called()
                    ffmpeg.assert_not_called()
                    if dangling:
                        self.assertFalse(target.exists())
                    if not dangling and target.is_file():
                        self.assertEqual(target.read_bytes(), b'outside sentinel')
                    destination.unlink()
                    if destination == self.work:
                        destination.mkdir()
                    if target.is_dir():
                        target.rmdir()
                    elif target.exists():
                        target.unlink()

    def test_mixer_preserves_internal_aliases_pcm_and_lexical_ffmpeg_arguments(self):
        """Valid leaves keep their aliases, original names, headers and PCM bytes."""
        for dangling in (False, True):
            with self.subTest(dangling=dangling):
                for index, destination in enumerate((self.mixed, self.normed, self.out)):
                    target = self.root / f'local-{index}'
                    if not dangling:
                        target.write_bytes(b'old local data')
                    destination.symlink_to(target)
                calls, stdout = self.run_audio()
                self.assertEqual(len(calls), 2)
                self.assertEqual(calls[0][-1], str(self.normed))
                self.assertEqual(calls[1][-1], str(self.out))
                self.assertIn(str(self.mixed), calls[0])
                self.assertIn(str(self.normed), calls[1])
                self.assertTrue(stdout.endswith(
                    f'{self.out} mix loudness before normalising -14.00 LUFS\n'))
                with wave.open(str(self.mixed), 'rb') as mixed:
                    self.assertEqual((mixed.getnchannels(), mixed.getsampwidth()), (2, 2))
                    self.assertEqual(mixed.readframes(8), PCM)
                for destination in (self.mixed, self.normed, self.out):
                    self.assertTrue(destination.is_symlink())
                    destination.resolve().unlink()
                    destination.unlink()

    def test_cli_refuses_external_and_aliased_inputs_before_media_or_output(self):  # skipcq: PY-R1000
        """Ordinary manifests and media references obey the same project root."""
        directory = self.root / 'work' / 'sfx'
        directory.mkdir()
        for kind in ('cues', 'video', 'manifest', 'narration', 'sfx_dir', 'sfx_leaf'):
            for variant in ('absolute', 'existing-alias', 'dangling-alias'):
                with self.subTest(kind=kind, variant=variant):
                    target = self.outside / f'{kind}-{variant}.wav'
                    if variant != 'dangling-alias':
                        if kind == 'sfx_dir':
                            target.mkdir()
                        else:
                            payload = self.cues.read_bytes() if kind == 'cues' else b'[]'
                            target.write_bytes(payload)
                    parent = directory if kind == 'sfx_leaf' else self.root
                    alias = parent / f'{kind}-{variant}.wav'
                    candidate = str(target)
                    if variant != 'absolute':
                        alias.symlink_to(target, target_is_directory=kind == 'sfx_dir')
                        candidate = str(alias)
                    argv = self.audio_argv()
                    self.cues.write_text(json.dumps({'total': 1, 'clips': [], 'sfx': []}))
                    if kind == 'cues':
                        argv[2] = candidate
                    elif kind == 'video':
                        argv[3] = candidate
                    elif kind == 'manifest':
                        argv.extend(['--narration', candidate])
                    elif kind == 'narration':
                        manifest = self.root / 'narration.json'
                        manifest.write_text(json.dumps([{'t': 0, 'file': candidate}]))
                        argv.extend(['--narration', str(manifest)])
                    elif kind == 'sfx_dir':
                        argv.extend(['--sfx-dir', candidate])
                    else:
                        name = os.path.relpath(candidate, directory)[:-4]
                        candidate = f'{directory}/{name}.wav'
                        self.cues.write_text(json.dumps({
                            'total': 1, 'clips': [], 'sfx': [[0, name, -2]],
                        }))
                        argv.remove('--no-sfx')
                    with mock.patch.object(sys, 'argv', argv), \
                            mock.patch('builtins.open', wraps=open) as reader, \
                            mock.patch.object(self.audio.os, 'makedirs') as mkdir, \
                            mock.patch.object(self.audio, 'mix_pcm', return_value=PCM) as decode, \
                            mock.patch.object(
                                self.audio, 'measure', return_value=(-14.0, -2.0),
                            ) as meter, \
                            mock.patch.object(self.audio, 'run') as encoder, \
                            self.assertRaises(SystemExit) as error:
                        self.audio.main()
                    self.assertIn('Refused path', str(error.exception))
                    self.assertIn(candidate, str(error.exception))
                    self.assertIn(str(self.root), str(error.exception))
                    mkdir.assert_not_called()
                    decode.assert_not_called()
                    meter.assert_not_called()
                    encoder.assert_not_called()
                    if kind in ('cues', 'manifest'):
                        opened = [str(call.args[0]) for call in reader.call_args_list]
                        self.assertNotIn(candidate, opened)
                    if variant != 'absolute':
                        alias.unlink()

    def test_direct_pcm_sources_refuse_escaped_files_before_their_decoder(self):
        """The reusable mixer also guards its SFX and narration decode boundaries."""
        directory = self.root / 'work' / 'sfx'
        directory.mkdir()
        target = self.outside / 'source.wav'
        target.write_bytes(b'outside sentinel')
        (directory / 'tick.wav').symlink_to(target)
        cues = {'total': 0.001, 'clips': [], 'sfx': [[0, 'tick', -2]]}
        cases = (([], str(directory), True),
                 ([{'t': 0, 'file': str(target)}], str(directory), False),
                 ([], str(self.outside), False))
        for narration, sfx_dir, use_sfx in cases:
            with self.subTest(narration=narration, sfx_dir=sfx_dir, use_sfx=use_sfx), \
                    mock.patch.object(self.audio, 'decode', return_value=PCM) as decoder:
                with self.assertRaises(SystemExit):
                    self.audio.mix_pcm(cues, narration, sfx_dir, use_sfx, -3, -2)
                decoder.assert_not_called()

    def test_sfx_preflights_every_leaf_before_synthesis_writes(self):
        """A later escaped sound refuses before even the first synthetic WAV write."""
        directory = self.root / 'work' / 'sfx'
        directory.mkdir()
        for name in (None, *SOUNDS):
            for dangling in (False, True):
                with self.subTest(name=name, dangling=dangling):
                    destination = directory if name is None else directory / f'{name}.wav'
                    target = self.outside / ('missing' if dangling else 'sound.wav')
                    if not dangling:
                        if name is None:
                            target.mkdir()
                        else:
                            target.write_bytes(b'outside sentinel')
                    if name is None:
                        directory.rmdir()
                    destination.symlink_to(target, target_is_directory=name is None)
                    with mock.patch.object(wave, 'open') as writer, \
                            self.assertRaises(SystemExit) as error:
                        run_sfx(directory)
                    self.assertIn('Refused path', str(error.exception))
                    self.assertIn(str(destination), str(error.exception))
                    self.assertIn(str(self.root), str(error.exception))
                    writer.assert_not_called()
                    destination.unlink()
                    if name is None:
                        directory.mkdir()
                    if target.is_dir():
                        target.rmdir()
                    elif target.exists():
                        self.assertEqual(target.read_bytes(), b'outside sentinel')
                        target.unlink()

    def test_sfx_save_preserves_internal_aliases_and_guards_direct_calls(self):
        """The reusable writer validates its concrete file on every call."""
        directory = self.root / 'work' / 'sfx'
        directory.mkdir()
        target = self.root / 'local-sound.wav'
        (directory / 'whoosh.wav').symlink_to(target)
        with contextlib.redirect_stdout(io.StringIO()) as stdout:
            namespace = run_sfx(directory)
        self.assertEqual(stdout.getvalue(), 'sfx ok\n')
        self.assertTrue((directory / 'whoosh.wav').is_symlink())
        with wave.open(str(target), 'rb') as sound:
            self.assertEqual((sound.getnchannels(), sound.getsampwidth()), (1, 2))
            self.assertEqual(sound.getnframes(), 3)
        external = self.outside / 'new.wav'
        (directory / 'extra.wav').symlink_to(external)
        with mock.patch.object(wave, 'open') as writer, self.assertRaises(SystemExit):
            namespace['save']('extra', [0.0, 0.125, -0.25])
        writer.assert_not_called()
        self.assertFalse(external.exists())

    def test_media_paths_preserve_one_file_prefix_and_native_drive_semantics(self):
        """Match FFmpeg's literal file opener without rewriting permitted arguments."""
        local = str(self.root / 'local.wav')
        once = 'file:' + local
        literal = 'file:local.wav'
        with mock.patch.object(self.audio, 'checked_path') as check:
            for raw, target in ((local, local), (once, local),
                                ('file:' + literal, literal), ('file:-', '-'),
                                ('./actual:name.wav', './actual:name.wav')):
                with self.subTest(raw=raw):
                    self.assertEqual(self.audio._checked_media_path(raw), raw)  # skipcq: PYL-W0212
                    check.assert_called_with(target)
            with mock.patch.object(self.audio.os, 'name', 'nt'), \
                    mock.patch.object(self.audio.os.path, 'splitdrive', return_value=('C:', '\\x')):
                self.assertEqual(self.audio._checked_media_path('C:\\x'), 'C:\\x')  # skipcq: PYL-W0212
                check.assert_called_with('C:\\x')
        if os.name != 'nt':
            with self.assertRaises(SystemExit):
                self.audio._checked_media_path('C:\\x')  # skipcq: PYL-W0212

    def test_cli_refuses_protocols_streams_and_external_file_urls_before_media(self):
        """Every ordinary FFmpeg operand preflights before decoder or writer calls."""
        manifest = self.root / 'protocol-narration.json'
        candidates = ('https://example.invalid/never-requested.wav', 'http:/never-requested',
                      'concat:one|two', 'crypto+file:outside', 'pipe:0', 'fd:0', '-',
                      'subfile,,start,0,end,1,,:outside.wav', 'file:' + str(self.outside / 'x.wav'))
        for candidate in candidates:
            manifest.write_text(json.dumps([{'t': 0, 'file': candidate}]))
            video = self.audio_argv()
            video[3] = candidate
            output = self.audio_argv()
            output[4] = candidate
            variants = {
                'video': video,
                'output': output,
                'narration': self.audio_argv() + ['--narration', str(manifest)],
                'sfx_dir': self.audio_argv() + ['--sfx-dir', candidate],
            }
            for kind, argv in variants.items():
                with self.subTest(kind=kind, candidate=candidate), \
                        mock.patch.object(sys, 'argv', argv), \
                        mock.patch.object(self.audio.os, 'makedirs') as mkdir, \
                        mock.patch.object(self.audio, 'mix_pcm') as decoder, \
                        mock.patch.object(self.audio, 'run') as encoder, \
                        mock.patch.object(self.audio, 'measure') as meter, \
                        self.assertRaises(SystemExit) as error:
                    self.audio.main()
                self.assertIn(candidate, str(error.exception))
                self.assertIn(str(self.root), str(error.exception))
                mkdir.assert_not_called()
                decoder.assert_not_called()
                encoder.assert_not_called()
                meter.assert_not_called()

    def test_file_urls_resolve_actual_existing_and_dangling_symlink_targets(self):
        """A file URL validates the real target rather than a fictitious file: folder."""
        for dangling in (False, True):
            with self.subTest(dangling=dangling):
                target = self.outside / ('missing.wav' if dangling else 'existing.wav')
                if not dangling:
                    target.write_bytes(b'outside sentinel')
                alias = self.root / f'url-alias-{dangling}.wav'
                alias.symlink_to(target)
                raw = 'file:' + str(alias)
                with self.assertRaises(SystemExit) as error:
                    self.audio._checked_media_path(raw)  # skipcq: PYL-W0212
                self.assertIn(raw, str(error.exception))
                self.assertIn(str(self.root), str(error.exception))

    def test_public_media_readers_refuse_protocols_before_their_subprocess(self):
        """Reusable loudness and PCM helpers enforce their ordinary media boundary."""
        cues = {'total': 0.001, 'clips': [], 'sfx': []}
        for raw in ('file:' + str(self.outside / 'outside.wav'), 'pipe:0', '-'):
            with self.subTest(raw=raw), \
                    mock.patch.object(self.audio, 'run') as runner, \
                    mock.patch.object(self.audio, 'decode') as decoder:
                with self.assertRaises(SystemExit):
                    self.audio.measure(raw)
                with self.assertRaises(SystemExit):
                    self.audio.mix_pcm(cues, [{'t': 0, 'file': raw}],
                                       str(self.root / 'sfx'), False, -3, -2)
                runner.assert_not_called()
                decoder.assert_not_called()

    def test_command_assertion_survives_optimized_execution_with_same_diagnostic(self):
        """An unsupported command keeps the original empty AssertionError under -O."""
        code = r"""
import importlib.util
import sys
spec = importlib.util.spec_from_file_location('audio_optimized_fixture', sys.argv[1])
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
sys.argv = ['audio.py', 'not-mix']
try:
    module.main()
except AssertionError as error:
    print(type(error).__name__, repr(error.args))
else:
    raise RuntimeError('command guard disappeared')
"""
        for optimization in ([], ['-O']):
            with self.subTest(optimization=optimization):
                result = subprocess.run(
                    [sys.executable, '-I', *optimization, '-c', code, str(TOOLS / 'audio.py')],
                    capture_output=True, text=True, check=True,
                )
                self.assertEqual(result.stdout, 'AssertionError ()\n')


if __name__ == '__main__':
    unittest.main()
