"""Path-boundary and output compatibility tests for mksrt.py."""
import contextlib
import importlib.util
import io
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parent.parent
SCRIPT = Path(__file__).with_name('mksrt.py')
NARRATE = Path(__file__).with_name('narrate.py')
SHEET = Path(__file__).with_name('sheet.py')
LAUNCH = Path(__file__).with_name('launch.py')


class MksrtPaths(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory(dir=ROOT)
        self.addCleanup(self.scratch.cleanup)
        self.folder = Path(self.scratch.name)
        self.source = self.folder / 'cues.json'
        self.source.write_text(json.dumps({'captions': [[0, 1, 'Hello']]}))
        self.output = self.folder / 'new.srt'
        self.external = tempfile.TemporaryDirectory()
        self.addCleanup(self.external.cleanup)
        self.external_folder = Path(self.external.name)

    @staticmethod
    def run_script(source, output):
        return subprocess.run([sys.executable, '-I', str(SCRIPT), str(source), str(output)],
                              capture_output=True, text=True, cwd=ROOT, check=False)

    def test_in_root_input_and_new_output_keep_srt_bytes(self):
        result = self.run_script(self.source, self.output)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.output.read_bytes(), b'1\n00:00:00,000 --> 00:00:01,000\nHello\n\n')

    def test_external_input_and_output_are_refused(self):
        for source, output, refused in [(self.external_folder / 'cues.json', self.output, str(self.external_folder / 'cues.json')),
                                        (self.source, self.external_folder / 'new.srt', str(self.external_folder / 'new.srt'))]:
            with self.subTest(refused=refused):
                result = self.run_script(source, output)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(str(ROOT), result.stderr)
                self.assertIn(refused, result.stderr)
        self.assertFalse(self.output.exists())

    def test_parent_traversal_and_symlink_escape_are_refused(self):
        link = self.folder / 'outside'
        link.symlink_to(self.external_folder, target_is_directory=True)
        for path in [self.folder / '..' / '..' / '..' / 'outside.srt', link / 'new.srt']:
            with self.subTest(path=path):
                result = self.run_script(self.source, path)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(str(ROOT), result.stderr)
                self.assertIn(str(path), result.stderr)


class NarrationPaths(unittest.TestCase):
    setUp = MksrtPaths.setUp
    @staticmethod
    def run_pick(lines, outdir, output):
        return subprocess.run([sys.executable, '-I', str(NARRATE), 'pick', str(lines),
                               str(outdir), str(output)], capture_output=True, text=True, cwd=ROOT, check=False)

    def test_narration_symlink_refused_before_generation(self):
        self.source.write_text('[]')
        link = self.folder / 'outside'
        link.symlink_to(self.external_folder, target_is_directory=True)
        for command in ['pick', 'gen']:
            args = [sys.executable, '-I', str(NARRATE), command, str(self.source), str(link)]
            if command == 'pick':
                args.append(str(self.folder / 'narr.json'))
            result = subprocess.run(args, capture_output=True, text=True, check=False)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn(str(ROOT), result.stderr)
            self.assertIn(str(link), result.stderr)
            self.assertEqual(list(self.external_folder.iterdir()), [])

    def test_pick_rejects_missing_json_suffix_without_writing(self):
        self.source.write_text('[]')
        output = self.folder / 'narration'
        result = self.run_pick(self.source, self.folder, output)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('Output must end in .json', result.stderr)
        self.assertFalse(output.exists())

    def test_pick_in_root_writes_empty_results(self):
        self.source.write_text('[]')
        result = self.run_pick(self.source, self.folder, self.folder / 'narr.json')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual((self.folder / 'narr.json').read_text(), '[]')
        self.assertEqual((self.folder / 'narr.srt').read_text(), '')

    def test_pick_keeps_the_best_fitting_take_and_output(self):
        spec = importlib.util.spec_from_file_location('test_narration', NARRATE)
        narrate = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(narrate)
        lines = [{'id': 'line', 't': 1, 'win': 1.5, 'text': 'Hello'},
                 {'id': 'missing', 't': 4, 'win': 1, 'text': 'Missing'}]
        self.source.write_text(json.dumps(lines))
        for take, end in [(1, 2.0), (2, 1.2), (3, 1.4)]:
            (self.folder / f'line_t{take}.mp3').touch()
            (self.folder / f'line_t{take}.json').write_text(json.dumps({'words': [['Hello', 0, end]]}))
        output = self.folder / 'narr.json'
        stdout = io.StringIO()
        with mock.patch.object(narrate, 'dur', return_value=2.0), contextlib.redirect_stdout(stdout):
            narrate.pick(str(self.source), str(self.folder), str(output))
        expected = [{'id': 'line', 't': 1, 'file': str(self.folder / 'line_t2.mp3'),
                     'gain_db': 0, 'tempo': 1.0, 'speech_end': 1.2, 'win': 1.5, 'text': 'Hello'}]
        self.assertEqual(output.read_text(), json.dumps(expected, indent=1))
        self.assertEqual((self.folder / 'narr.srt').read_text(),
                         '1\n00:00:01,000 --> 00:00:02,350\nHello\n\n')
        self.assertIn('missing: NO TAKE', stdout.getvalue())

    def test_pick_rejects_external_paths_and_ids(self):
        self.source.write_text(json.dumps([{'id': '../../outside'}]))
        for lines, outdir, output, refused in [
            (self.external_folder / 'lines.json', self.folder, self.folder / 'narr.json', str(self.external_folder / 'lines.json')),
            (self.source, self.external_folder, self.folder / 'narr.json', str(self.external_folder)),
            (self.source, self.folder, self.external_folder / 'narr.json', str(self.external_folder / 'narr.json')),
            (self.source, self.folder, self.folder / 'narr.json', '../../outside'),
        ]:
            with self.subTest(refused=refused):
                result = self.run_pick(lines, outdir, output)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(str(ROOT), result.stderr)
                self.assertIn(refused, result.stderr)


class SheetPaths(unittest.TestCase):
    """Contact sheets reject paths escaping the launch-video project."""

    setUp = MksrtPaths.setUp

    def test_external_and_symlink_paths_are_rejected_before_running_tools(self):
        link = self.folder / 'outside'
        link.symlink_to(self.external_folder, target_is_directory=True)
        video = self.folder / 'video.mp4'
        output = self.folder / 'sheet.png'
        for source, destination, refused in [
            (self.external_folder / 'video.mp4', output, self.external_folder / 'video.mp4'),
            (video, self.external_folder / 'sheet.png', self.external_folder / 'sheet.png'),
            (video, link / 'sheet.png', link / 'sheet.png'),
            (link / 'video.mp4', output, link / 'video.mp4'),
            (video, self.folder / '..' / '..' / '..' / 'sheet.png',
             self.folder / '..' / '..' / '..' / 'sheet.png'),
        ]:
            with self.subTest(refused=refused):
                result = subprocess.run(
                    [sys.executable, '-I', str(SHEET), str(source), str(destination), '2', '100'],
                    capture_output=True, text=True, cwd=ROOT, check=False,
                )
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(str(ROOT), result.stderr)
                self.assertIn(str(refused), result.stderr)
        self.assertFalse(output.exists())


class LaunchPaths(unittest.TestCase):
    """The launch builder rejects external output paths before rendering."""

    setUp = MksrtPaths.setUp

    def test_cues_and_render_refuse_external_or_symlinked_outputs(self):
        link = self.folder / 'outside'
        link.symlink_to(self.external_folder, target_is_directory=True)
        for command in ('cues', 'render'):
            for output in (self.external_folder / 'output', link / 'output'):
                with self.subTest(command=command, output=output):
                    result = subprocess.run(
                        [sys.executable, '-I', str(LAUNCH), 'li', command, str(output)],
                        capture_output=True, text=True, cwd=ROOT, check=False,
                    )
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn(str(ROOT), result.stderr)
                    self.assertIn(str(output), result.stderr)
        self.assertEqual(list(self.external_folder.iterdir()), [])


if __name__ == '__main__':
    unittest.main()
