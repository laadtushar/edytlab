"""Shell assembly, default still paths and help must respect the launch contracts."""
import importlib.util
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

TOOLS = Path(__file__).resolve().parent
PROJECT = TOOLS.parent
SOUNDS = ('whoosh.wav', 'tick.wav', 'chime.wav')


class AssemblySoundEffects(unittest.TestCase):
    """The shell delegates to the full SFX generator whenever any sound is missing."""

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
                )
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(result.stdout, 'assembled\n')
                commands = [line.split() for line in log.read_text().splitlines()]
                sfx = [command for command in commands if command[1] == 'sfx.py']
                self.assertEqual(sfx, [['-I', 'sfx.py', '../work/sfx']] if missing else [])
                # All Python/media commands are stubs; no actual outputs can be produced.
                self.assertEqual(list((root / 'out').iterdir()), [])


class LaunchCommandContracts(unittest.TestCase):
    """Launch commands must validate default paths and provide side-effect-free help."""

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
