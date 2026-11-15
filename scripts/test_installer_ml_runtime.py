"""Tests for installer-ml-runtime.py's path guards (run: python3 -m unittest discover -s scripts -p "test_installer*.py")."""
import importlib.util
import os
import tempfile
import unittest
from pathlib import Path

SPEC = importlib.util.spec_from_file_location(
    "installer_ml_runtime", Path(__file__).resolve().parent / "installer-ml-runtime.py"
)
mod = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(mod)


class InsideTests(unittest.TestCase):
    """inside(): a path must resolve to the root or below it."""
    def setUp(self):
        """A temporary ml-runtime folder to stage into."""
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name).resolve() / "ml-runtime"
        self.root.mkdir()

    def test_the_workflows_argument_is_accepted(self):
        """The not-yet-existing output folder the workflows pass is accepted."""
        # A path that does not exist yet, as vc-runtime-x64 does not before the first run.
        wanted = self.root / "vc-runtime-x64"
        self.assertEqual(mod.inside(self.root, wanted, "out"), wanted)

    def test_the_root_itself_is_accepted(self):
        """The staging root itself is inside."""
        self.assertEqual(mod.inside(self.root, self.root, "out"), self.root)

    def test_a_relative_path_resolves_against_the_working_directory(self):
        """A relative argument is resolved from the working directory, as in CI."""
        before = os.getcwd()
        os.chdir(self.root.parent)
        self.addCleanup(os.chdir, before)
        self.assertEqual(mod.inside(self.root, "ml-runtime/vc-runtime-x64", "out"), self.root / "vc-runtime-x64")

    def test_dotdot_and_siblings_are_refused(self):
        """`..`, a sibling with the same prefix, and the parent are refused."""
        sibling = self.root.parent / "ml-runtime-other"
        for bad in (self.root / ".." / "x", sibling, self.root.parent):
            with self.assertRaises(SystemExit, msg=str(bad)):
                mod.inside(self.root, bad, "out")

    def test_a_symlink_out_of_the_root_is_refused(self):
        """A symlink inside the root that points outside it is refused."""
        outside = self.root.parent / "outside"
        outside.mkdir()
        link = self.root / "vc-runtime-x64"
        try:
            link.symlink_to(outside, target_is_directory=True)
        except (OSError, NotImplementedError):
            self.skipTest("symlinks are not available here")
        with self.assertRaises(SystemExit):
            mod.inside(self.root, link, "out")


class FileNameTests(unittest.TestCase):
    """file_name(): manifest entries must be bare file names."""
    def test_the_manifest_names_pass(self):
        """The four DLL names the runtime needs are accepted."""
        for name in ("msvcp140.dll", "msvcp140_1.dll", "vcruntime140.dll", "vcruntime140_1.dll"):
            self.assertEqual(mod.file_name(name, "f"), name)

    def test_paths_and_non_strings_are_refused(self):
        """Paths, drive prefixes, dot names and non-strings are refused."""
        for bad in ("", ".", "..", "../x.dll", "a/b.dll", "a\\b.dll", "C:x.dll", None, 3):
            with self.assertRaises(SystemExit, msg=repr(bad)):
                mod.file_name(bad, "f")

    @staticmethod
    def test_the_committed_manifest_passes():
        """Every name in the committed ml-runtime.json is a bare file name."""
        vc = mod.load_json(mod.MANIFEST)["platforms"]["win-x64"]["vc_runtime"]
        for name in vc["files"]:
            mod.file_name(name, "f")

    def test_the_workflows_output_folder_is_inside_the_staging_root(self):
        """The real workflow argument resolves inside the real staging root."""
        out = mod.REPO / "apps" / "desktop" / "src-tauri" / "ml-runtime" / "vc-runtime-x64"
        self.assertEqual(mod.inside(mod.STAGING_ROOT, out, "out"), out.resolve())


if __name__ == "__main__":
    unittest.main()
