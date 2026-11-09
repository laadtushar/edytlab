"""Project boundaries must be checked before local media tools run."""
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from paths import ROOT, checked_path

TOOLS = Path(__file__).resolve().parent


class PromoPaths(unittest.TestCase):
    """Media tools confine new files and reject traversal and symlink escapes."""
    def test_new_outputs_and_symlinks(self):
        """Permit new in-root files and refuse traversal or symlink escapes."""
        with tempfile.TemporaryDirectory(dir=ROOT) as inside, \
                tempfile.TemporaryDirectory() as outside:
            local = Path(inside)
            self.assertEqual(checked_path(local / 'new' / 'out.png'), local / 'new' / 'out.png')
            for path in (Path(outside) / 'missing.png', ROOT / '..' / 'escape.png'):
                with self.assertRaises(SystemExit) as refusal:
                    checked_path(path)
                self.assertIn(str(ROOT), str(refusal.exception))
                self.assertIn(str(path), str(refusal.exception))
            (local / 'link').symlink_to(outside, target_is_directory=True)
            (local / 'dangling').symlink_to(Path(outside) / 'not-created')
            for name in ('link', 'dangling'):
                with self.assertRaises(SystemExit):
                    checked_path(local / name / 'out.png')

    def test_cli_rejects_before_probing(self):
        """Reject external CLI paths before any media process starts."""
        with tempfile.TemporaryDirectory(dir=ROOT) as inside, \
                tempfile.TemporaryDirectory() as outside:
            for tool, args in (
                ('sheet.py', [str(Path(outside) / 'input.mp4'), str(Path(inside) / 'out.png')]),
                ('sheet.py', [str(Path(inside) / 'input.mp4'), str(Path(outside) / 'out.png')]),
                ('verify.py', [outside]),
            ):
                result = subprocess.run([sys.executable, str(TOOLS / tool), *args],
                                        capture_output=True, text=True, check=False)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn('Refused path', result.stderr)
                self.assertIn(str(ROOT), result.stderr)
                self.assertIn(outside, result.stderr)


if __name__ == '__main__':
    unittest.main()
