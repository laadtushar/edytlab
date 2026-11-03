"""Containment checks for the promo narration fit tool."""
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = Path(__file__).with_name('fit.py')


class FitPaths(unittest.TestCase):
    """The tool only reads and writes inside its own promo project."""

    def setUp(self):
        self.inside = tempfile.TemporaryDirectory(dir=ROOT)
        self.outside = tempfile.TemporaryDirectory()
        self.addCleanup(self.inside.cleanup)
        self.addCleanup(self.outside.cleanup)
        self.cwd = Path(self.inside.name)
        self.external = Path(self.outside.name)

    def run_fit(self, voice):
        return subprocess.run([sys.executable, str(SCRIPT), voice], cwd=self.cwd,
                              capture_output=True, text=True, check=False)

    def test_in_root_voice_without_takes_succeeds(self):
        result = self.run_fit('voice')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, '')
        self.assertTrue((self.cwd / 'trim' / 'voice').is_dir())

    def test_traversal_and_symlinked_output_are_refused(self):
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
        (self.cwd / 'lines').mkdir()
        (self.cwd / 'lines' / 'voice').symlink_to(self.external, target_is_directory=True)
        result = self.run_fit('voice')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(str(ROOT), result.stderr)
        self.assertIn('lines/voice/', result.stderr)
        self.assertEqual(list(self.external.iterdir()), [])


if __name__ == '__main__':
    unittest.main()
