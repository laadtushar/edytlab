"""Tests for prepare.py's output-folder guards (run: python3 -m unittest discover -s scripts/word-timing-eval)."""
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import prepare  # noqa: E402


class ChildTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.out = Path(self.tmp.name).resolve()

    def test_a_bare_name_is_in_the_folder(self):
        self.assertEqual(prepare.child(self.out, "a.wav"), self.out / "a.wav")

    def test_a_name_that_leaves_the_folder_is_refused(self):
        for name in ("../a.wav", "sub/a.wav", "", ".", ".."):
            with self.assertRaises(SystemExit, msg=name):
                prepare.child(self.out, name)

    def test_a_symlink_at_the_name_is_refused(self):
        target = self.out / "elsewhere.txt"
        target.write_text("keep")
        link = self.out / "a.wav"
        try:
            link.symlink_to(target)
        except (OSError, NotImplementedError):
            self.skipTest("symlinks are not available here")
        with self.assertRaises(SystemExit):
            prepare.child(self.out, "a.wav")


if __name__ == "__main__":
    unittest.main()
