"""Brand inputs stay within the fixed project and system boundaries."""
import importlib.util
import os
from pathlib import Path
import shutil
import tempfile
import unittest
from unittest import mock

TOOLS = Path(__file__).resolve().parent
REELS = TOOLS.parent
REPO = TOOLS.parents[2]
SYSTEM_FONT = Path("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf")


class BrandInputTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(dir=REELS)
        self.addCleanup(self.temp.cleanup)
        self.repo = Path(self.temp.name)
        self.reels = self.repo / "marketing" / "reels"
        self.tools = self.reels / "tools"
        self.tools.mkdir(parents=True)
        shutil.copyfile(TOOLS / "brand.py", self.tools / "brand.py")
        for name in ("website/app/globals.css", "website/public/logo.svg"):
            dest = self.repo / name
            dest.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(REPO / name, dest)

    def load_brand(self, font_dir=None):
        env = {} if font_dir is None else {"EDYTLAB_REEL_FONT_DIR": str(font_dir)}
        with mock.patch.dict(os.environ, env, clear=True):
            spec = importlib.util.spec_from_file_location("test_brand_fixture", self.tools / "brand.py")
            module = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(module)
        return module

    @unittest.skipUnless(SYSTEM_FONT.is_file(), "DejaVu Sans fixture is not installed")
    def test_local_font_copy_loads_and_preserves_rendering(self):
        fonts = self.reels / "fonts"
        fonts.mkdir()
        shutil.copyfile(SYSTEM_FONT, fonts / "Inter-Medium.ttf")
        brand = self.load_brand(fonts)
        loaded = brand.font("Medium", 18)
        self.assertGreater(loaded.getlength("edytlab"), 0)
        self.assertEqual(bytes(loaded.getmask("edytlab")),
                         bytes(brand.ImageFont.truetype(str(SYSTEM_FONT), 18).getmask("edytlab")))

    @unittest.skipUnless(SYSTEM_FONT.is_file(), "DejaVu Sans fixture is not installed")
    def test_launch_video_font_copy_loads(self):
        fonts = self.repo / "marketing" / "launch-video" / "fonts"
        fonts.mkdir(parents=True)
        shutil.copyfile(SYSTEM_FONT, fonts / "Inter-Bold.otf")
        self.assertGreater(self.load_brand(fonts).font("Bold", 18, project="launch-video").getlength("edytlab"), 0)

    def test_projects_cannot_use_each_others_font_copies(self):
        reels_fonts = self.reels / "fonts"
        reels_fonts.mkdir()
        launch_fonts = self.repo / "marketing" / "launch-video" / "fonts"
        launch_fonts.mkdir(parents=True)
        for directory, project in ((launch_fonts, "reels"), (reels_fonts, "launch-video")):
            with self.subTest(project=project), self.assertRaises(SystemExit):
                self.load_brand(directory).font("Medium", 18, project=project)
        with self.assertRaises(SystemExit):
            self.load_brand(launch_fonts).font("Medium", 18)

    def test_project_name_must_be_recognized(self):
        with self.assertRaises(ValueError):
            self.load_brand().font("Medium", 18, project="unknown")

    def test_font_symlinks_cannot_cross_between_projects(self):
        reels_fonts = self.reels / "fonts"
        reels_fonts.mkdir()
        launch_fonts = self.repo / "marketing" / "launch-video" / "fonts"
        launch_fonts.mkdir(parents=True)
        for source, target, project in ((reels_fonts, launch_fonts, "reels"),
                                        (launch_fonts, reels_fonts, "launch-video")):
            with self.subTest(project=project):
                other_font = target / "outside.ttf"
                other_font.write_bytes(b"other project")
                link = source / "Inter-Medium.ttf"
                link.symlink_to(other_font)
                try:
                    with self.assertRaises(SystemExit):
                        self.load_brand(source).font("Medium", 18, project=project)
                    self.assertTrue(link.is_symlink())
                    self.assertEqual(other_font.read_bytes(), b"other project")
                finally:
                    link.unlink()
                    other_font.unlink()

    def test_external_font_directory_is_rejected_without_changing_it(self):
        with tempfile.TemporaryDirectory() as external:
            font = Path(external) / "Inter-Medium.ttf"
            font.write_bytes(b"outside font")
            original = font.read_bytes()
            with self.assertRaises(SystemExit):
                self.load_brand(external).font("Medium", 18)
            self.assertEqual(font.read_bytes(), original)
            self.assertEqual(list(Path(external).iterdir()), [font])

    def test_local_font_symlink_cannot_escape(self):
        fonts = self.reels / "fonts"
        fonts.mkdir()
        with tempfile.TemporaryDirectory() as external:
            target = Path(external) / "Inter-Medium.ttf"
            target.write_bytes(b"outside font")
            (fonts / target.name).symlink_to(target)
            with self.assertRaises(SystemExit):
                self.load_brand(fonts).font("Medium", 18)
            self.assertTrue((fonts / target.name).is_symlink())
            self.assertEqual(target.read_bytes(), b"outside font")

    def test_font_directory_symlink_cannot_escape(self):
        with tempfile.TemporaryDirectory() as external:
            target = Path(external) / "Inter-Medium.ttf"
            target.write_bytes(b"outside font")
            link = self.reels / "fonts"
            link.symlink_to(external, target_is_directory=True)
            with self.assertRaises(SystemExit):
                self.load_brand(link).font("Medium", 18)
            self.assertTrue(link.is_symlink())
            self.assertEqual(target.read_bytes(), b"outside font")

    @unittest.skipUnless(SYSTEM_FONT.is_file(), "DejaVu Sans fixture is not installed")
    def test_system_font_location_rejects_symlinked_font_and_directory(self):
        with tempfile.TemporaryDirectory() as temp:
            base = Path(temp)
            fonts = base / "inter"
            fonts.mkdir()
            font = fonts / "Inter-Medium.otf"
            shutil.copyfile(SYSTEM_FONT, font)
            brand = self.load_brand()
            with mock.patch.object(brand, "_SYSTEM_FONT_DIR", fonts), \
                    mock.patch.object(brand, "FONT_DIR", str(fonts)):
                self.assertGreater(brand.font("Medium", 18).getlength("edytlab"), 0)
                self.assertGreater(brand.font("Medium", 18, project="launch-video").getlength("edytlab"), 0)
                font.unlink()
                font.symlink_to(SYSTEM_FONT)
                with self.assertRaises(SystemExit):
                    brand.font("Medium", 18)
            link = base / "linked-inter"
            link.symlink_to(fonts, target_is_directory=True)
            with mock.patch.object(brand, "_SYSTEM_FONT_DIR", link), \
                    mock.patch.object(brand, "FONT_DIR", str(link)):
                with self.assertRaises(SystemExit):
                    brand.font("Medium", 18)

    def test_font_weight_cannot_supply_a_path(self):
        fonts = self.reels / "fonts"
        fonts.mkdir()
        with self.assertRaises((ValueError, SystemExit)):
            self.load_brand(fonts).font("../outside", 18)

    def test_shared_site_inputs_cannot_be_symlinks_to_external_files(self):
        for name in ("website/app/globals.css", "website/public/logo.svg"):
            with self.subTest(name=name), tempfile.TemporaryDirectory() as external:
                path = self.repo / name
                contents = path.read_bytes()
                target = Path(external) / path.name
                target.write_bytes(contents)
                path.unlink()
                path.symlink_to(target)
                try:
                    with self.assertRaises(SystemExit):
                        self.load_brand()
                    self.assertTrue(path.is_symlink())
                    self.assertEqual(target.read_bytes(), contents)
                finally:
                    path.unlink()
                    path.write_bytes(contents)


if __name__ == "__main__":
    unittest.main()
