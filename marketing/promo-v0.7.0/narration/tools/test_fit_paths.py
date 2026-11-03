"""Containment checks for the promo narration fit tool."""
import os
import runpy
import subprocess
import sys
import tempfile
import unittest
from unittest import mock
from pathlib import Path

import script

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = Path(__file__).with_name('fit.py')


class FitPaths(unittest.TestCase):
    """The tool only reads and writes inside its own promo project."""

    def setUp(self):
        """Create private project-local and external fixtures."""
        self.inside = tempfile.TemporaryDirectory(dir=ROOT)
        self.outside = tempfile.TemporaryDirectory()
        self.addCleanup(self.inside.cleanup)
        self.addCleanup(self.outside.cleanup)
        self.cwd = Path(self.inside.name)
        self.external = Path(self.outside.name)

    def run_fit(self, voice):
        """Run the tool against a voice without contacting any provider."""
        return subprocess.run([sys.executable, str(SCRIPT), voice], cwd=self.cwd,
                              capture_output=True, text=True, check=False)

    def test_in_root_voice_without_takes_succeeds(self):
        """A new in-project trim directory is allowed."""
        result = self.run_fit('voice')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, '')
        self.assertTrue((self.cwd / 'trim' / 'voice').is_dir())

    def test_traversal_and_symlinked_output_are_refused(self):
        """Neither traversal nor symlinks may redirect generated WAV files."""
        (self.cwd / 'trim').mkdir()
        (self.cwd / 'trim' / 'linked').symlink_to(self.external, target_is_directory=True)
        for voice in ('../../../escape', 'linked'):
            with self.subTest(voice=voice):
                result = self.run_fit(voice)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(str(ROOT), result.stderr)
                self.assertIn(voice, result.stderr)
        self.assertEqual(list(self.external.iterdir()), [])

    def test_symlinked_input_is_refused(self):
        """Refuse external takes before invoking the media decoder."""
        (self.cwd / 'lines').mkdir()
        (self.cwd / 'lines' / 'voice').symlink_to(self.external, target_is_directory=True)
        result = self.run_fit('voice')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(str(ROOT), result.stderr)
        self.assertIn('lines/voice/', result.stderr)
        self.assertEqual(list(self.external.iterdir()), [])

    def test_failed_probe_reports_subprocess_error(self):
        """Report a failed probe before attempting to parse its empty duration."""
        takes = self.cwd / 'lines' / 'voice'
        takes.mkdir(parents=True)
        (takes / f"{script.L[0]['id']}.mp3").write_bytes(b'fixture')
        failure = "synthetic ffprobe failure"

        def run_media(command, **kwargs):
            """Model native subprocess check semantics without platform executables."""
            if command[0] == 'ffmpeg':
                return subprocess.CompletedProcess(command, 0)
            self.assertEqual(command[0], 'ffprobe')
            if kwargs.get('check'):
                raise subprocess.CalledProcessError(1, command, output='', stderr=failure)
            return subprocess.CompletedProcess(command, 1, stdout='', stderr=failure)

        original_cwd = Path.cwd()
        try:
            os.chdir(self.cwd)
            with mock.patch.object(sys, 'argv', [str(SCRIPT), 'voice']), \
                    mock.patch.object(subprocess, 'run', side_effect=run_media) as run, \
                    self.assertRaises(subprocess.CalledProcessError) as error:
                runpy.run_path(str(SCRIPT), run_name='__main__')
        finally:
            os.chdir(original_cwd)
        self.assertEqual(error.exception.returncode, 1)
        self.assertEqual(error.exception.stderr, failure)
        self.assertEqual(error.exception.cmd[0], 'ffprobe')
        self.assertEqual(len(run.call_args_list), 2)
        self.assertEqual(run.call_args_list[0].args[0][0], 'ffmpeg')
        probe = run.call_args_list[1]
        self.assertEqual(probe.args[0][-1],
                         str((self.cwd / 'trim/voice' / f"{script.L[0]['id']}.wav").resolve()))
        self.assertEqual(probe.kwargs, {'capture_output': True, 'text': True, 'check': True})


if __name__ == '__main__':
    unittest.main()
