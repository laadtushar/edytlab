"""Shell assembly, default still paths and help must respect the launch contracts."""
import contextlib
import importlib.util
import io
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

TOOLS = Path(__file__).resolve().parent
PROJECT = TOOLS.parent
SOUNDS = ('whoosh.wav', 'tick.wav', 'chime.wav')


class AssemblySoundEffects(unittest.TestCase):
    """The shell delegates to the full SFX generator whenever any sound is missing."""

    def test_fixed_directories_refuse_symlink_escapes_before_mkdir_or_media(self):
        """Assembly cannot create scratch/output directories through external links."""
        for target, dangling in (('out', False), ('work', False), ('work/sfx', False),
                                 ('out', True)):
            with self.subTest(target=target, dangling=dangling), \
                    tempfile.TemporaryDirectory(prefix='launch paths [fixture] ') as folder, \
                    tempfile.TemporaryDirectory() as external:
                root = Path(folder)
                tools, binaries = root / 'tools', root / 'bin'
                tools.mkdir()
                binaries.mkdir()
                for name in ('assemble.sh', 'launch_paths.py'):
                    shutil.copyfile(TOOLS / name, tools / name)
                link = root / target
                link.parent.mkdir(parents=True, exist_ok=True)
                destination = Path(external) / 'new' if dangling else Path(external)
                link.symlink_to(destination, target_is_directory=True)
                (binaries / 'python3').symlink_to(sys.executable)
                (binaries / 'dirname').symlink_to(shutil.which('dirname'))
                marker = root / 'unexpected-command'
                for name in ('mkdir', 'nice'):
                    stub = binaries / name
                    stub.write_text('#!/bin/sh\n: > "$ASSEMBLY_TEST_MARKER"\nexit 91\n')
                    stub.chmod(0o700)
                result = subprocess.run(
                    [shutil.which('bash'), str(tools / 'assemble.sh')],
                    env={**os.environ, 'PATH': str(binaries),
                         'ASSEMBLY_TEST_MARKER': str(marker)},
                    capture_output=True, text=True, check=False,
                )
                self.assertNotEqual(result.returncode, 0)
                self.assertIn('Refused path', result.stderr)
                self.assertIn(str(root), result.stderr)
                self.assertIn('../out' if target == 'out' else '../work/sfx', result.stderr)
                self.assertFalse(marker.exists(), result.stderr)
                self.assertEqual(list(Path(external).iterdir()), [])

    def test_each_missing_sound_requests_regeneration_and_complete_sets_are_kept(self):
        """Exercise every existence branch with recorded commands and no media tools."""
        for missing in (None, *SOUNDS):
            with self.subTest(missing=missing), tempfile.TemporaryDirectory() as folder:
                root = Path(folder)
                tools, binaries = root / 'tools', root / 'bin'
                sounds = root / 'work' / 'sfx'
                tools.mkdir()
                binaries.mkdir()
                sounds.mkdir(parents=True)
                shutil.copyfile(TOOLS / 'assemble.sh', tools / 'assemble.sh')
                for name in SOUNDS:
                    if name != missing:
                        (sounds / name).write_bytes(b'fixture')
                log = root / 'commands.log'
                python = binaries / 'python3'
                python.write_text(
                    '#!/bin/sh\n'
                    'printf "%s\\n" "$*" >> "$EDYTLAB_TEST_COMMAND_LOG"\n'
                )
                python.chmod(0o700)
                nice = binaries / 'nice'
                nice.write_text('#!/bin/sh\nshift 2\nexec "$@"\n')
                nice.chmod(0o700)
                result = subprocess.run(
                    ['bash', str(tools / 'assemble.sh')], cwd=root,
                    env={**os.environ, 'PATH': str(binaries) + os.pathsep + os.environ['PATH'],
                         'EDYTLAB_TEST_COMMAND_LOG': str(log)},
                    capture_output=True, text=True, check=False,
                )  # skipcq: BAN-B607
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(result.stdout, 'assembled\n')
                commands = [line.split() for line in log.read_text().splitlines()]
                sfx = [command for command in commands if command[1] == 'sfx.py']
                self.assertEqual(sfx, [['-I', 'sfx.py', '../work/sfx']] if missing else [])
                # All Python/media commands are stubs; no actual outputs can be produced.
                self.assertEqual(list((root / 'out').iterdir()), [])


class LaunchCommandContracts(unittest.TestCase):
    """Launch commands must validate default paths and provide side-effect-free help."""

    def test_malformed_render_ranges_fail_before_path_checks_or_plan_construction(self):
        """Missing, option-like and nonnumeric times cannot start a media build."""
        spec = importlib.util.spec_from_file_location('launch_range_test', TOOLS / 'launch.py')
        launch = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(launch)
        output = str(PROJECT / 'work' / 'range.mp4')
        for options in (['--from', '--to', '2'], ['--to', '--from', '1'],
                        ['--from', 'not-a-time'], ['--to', 'not-a-time'],
                        ['--from', ''], ['--to', '-h'], ['--from'], ['--to']):
            with self.subTest(options=options), \
                    mock.patch.object(sys, 'argv', ['launch.py', 'li', 'render',
                                                    *options, output]), \
                    mock.patch.object(launch, 'checked_output') as check_output, \
                    mock.patch.dict(launch.BUILDERS, {'li': mock.Mock()}) as builders:
                with self.assertRaisesRegex(SystemExit, 'requires a numeric value after'):
                    launch.main()
                check_output.assert_not_called()
                builders['li'].assert_not_called()
        for options in (['--from'], ['--to'], ['--from', '1', output, '--to']):
            with self.subTest(options=options), \
                    mock.patch.object(sys, 'argv', ['launch.py', 'li', 'render', *options]), \
                    mock.patch.object(launch, 'checked_output') as check_output, \
                    mock.patch.dict(launch.BUILDERS, {'li': mock.Mock()}) as builders:
                with self.assertRaisesRegex(SystemExit, 'requires a numeric value after'):
                    launch.main()
                check_output.assert_not_called()
                builders['li'].assert_not_called()

    def test_nonfinite_render_ranges_fail_before_path_checks_or_plan_construction(self):
        """Times that cannot become frame indices are rejected before a media build."""
        spec = importlib.util.spec_from_file_location('launch_range_finite', TOOLS / 'launch.py')
        launch = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(launch)
        output = str(PROJECT / 'work' / 'range.mp4')
        for option in ('--from', '--to'):
            for value in ('nan', 'NaN', 'inf', '+Infinity', '-inf', '1e309', '-1e309',
                          '1e308', '-1e308'):
                with self.subTest(option=option, value=value), \
                        mock.patch.object(sys, 'argv', ['launch.py', 'li', 'render',
                                                        option, value, output]), \
                        mock.patch.object(launch, 'checked_output') as check_output, \
                        mock.patch.dict(launch.BUILDERS, {'li': mock.Mock()}) as builders:
                    with self.assertRaisesRegex(SystemExit, 'requires a finite frame value after'):
                        launch.main()
                    check_output.assert_not_called()
                    builders['li'].assert_not_called()

    def test_numeric_render_ranges_keep_original_values_and_destination(self):
        """Accepted float syntax and repeated options retain the original render call."""
        spec = importlib.util.spec_from_file_location('launch_range_valid', TOOLS / 'launch.py')
        launch = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(launch)
        with tempfile.TemporaryDirectory(dir=PROJECT) as folder:
            output = Path(folder) / 'range.mp4'
            for options, start, end in (([], 0.0, None), (['--to', '2'], 0.0, 2.0),
                                        (['--from', '-1.25', '--to', '2e1'], -1.25, 20.0),
                                        (['--to', '3', '--from', ' 0.5 '], 0.5, 3.0),
                                        (['--from', '1', '--from', '2'], 1.0, None),
                                        (['--from', '1', '--from', 'nan'], 1.0, None),
                                        (['--from', '1', '--from', 'not-a-time'], 1.0, None),
                                        (['--to', '2', '--to', 'inf'], 0.0, 2.0)):
                plan = mock.Mock()
                with self.subTest(options=options), \
                        mock.patch.object(sys, 'argv', ['launch.py', 'li', 'render',
                                                        *options, str(output)]), \
                        mock.patch.dict(launch.BUILDERS, {'li': lambda plan=plan: plan}), \
                        contextlib.redirect_stdout(io.StringIO()) as stdout:
                    launch.main()
                plan.piece.render.assert_called_once_with(str(output.resolve()), start, end)
                self.assertEqual(stdout.getvalue(), str(output) + '\n')
                self.assertFalse(output.exists())

    def test_default_still_symlink_is_refused_before_building_the_plan(self):
        """The internal work path cannot redirect a still outside the project."""
        spec = importlib.util.spec_from_file_location('launch_contract_test', TOOLS / 'launch.py')
        launch = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(launch)
        with tempfile.TemporaryDirectory(dir=PROJECT) as inside, \
                tempfile.TemporaryDirectory() as outside:
            local, external = Path(inside), Path(outside)
            (local / 'work').symlink_to(external, target_is_directory=True)
            builder = mock.Mock()
            with mock.patch.object(launch, 'HERE', str(local)), \
                    mock.patch.dict(launch.BUILDERS, {'li': builder}), \
                    mock.patch.object(sys, 'argv', ['launch.py', 'li', 'still', '1']), \
                    self.assertRaises(SystemExit) as refusal:
                launch.main()
            self.assertIn('Refused path', str(refusal.exception))
            self.assertIn(str(PROJECT), str(refusal.exception))
            self.assertIn(str(local / 'work' / 'qc_li.png'), str(refusal.exception))
            builder.assert_not_called()
            self.assertEqual(list(external.iterdir()), [])

    def test_isolated_help_succeeds_without_credentials_or_outputs(self):
        """All four tools explain their boundary from an external working directory."""
        environment = os.environ.copy()
        environment.pop('ELEVENLABS_API_KEY', None)
        with tempfile.TemporaryDirectory() as outside:
            for name in ('launch.py', 'narrate.py', 'sheet.py', 'mksrt.py'):
                with self.subTest(tool=name):
                    result = subprocess.run(
                        [sys.executable, '-I', '-B', str(TOOLS / name), '--help'],
                        cwd=outside, env=environment,
                        capture_output=True, text=True, check=False,
                    )
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertEqual(result.stderr, '')
                    self.assertIn(name, result.stdout)
                    self.assertIn('marketing/launch-video', result.stdout)
                    self.assertIn('Copy external', result.stdout)
                    self.assertEqual(list(Path(outside).iterdir()), [])


if __name__ == '__main__':
    unittest.main()
