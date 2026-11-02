"""Path-boundary and output compatibility tests for mksrt.py."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent.parent
SCRIPT = Path(__file__).with_name('mksrt.py')
NARRATE = Path(__file__).with_name('narrate.py')


class MksrtPaths(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory(dir=ROOT)
        self.addCleanup(self.scratch.cleanup)
        self.folder = Path(self.scratch.name)
        self.source = self.folder / 'cues.json'
        self.source.write_text(json.dumps({'captions': [[0, 1, 'Hello']]}))
        self.output = self.folder / 'new.srt'

    def run_script(self, source, output):
        return subprocess.run([sys.executable, '-I', str(SCRIPT), str(source), str(output)],
                              capture_output=True, text=True, cwd=ROOT)

    def test_in_root_input_and_new_output_keep_srt_bytes(self):
        result = self.run_script(self.source, self.output)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.output.read_bytes(), b'1\n00:00:00,000 --> 00:00:01,000\nHello\n\n')

    def test_external_input_and_output_are_refused(self):
        for source, output, refused in [(Path('/tmp/cues.json'), self.output, '/tmp/cues.json'),
                                        (self.source, Path('/tmp/new.srt'), '/tmp/new.srt')]:
            with self.subTest(refused=refused):
                result = self.run_script(source, output)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(str(ROOT), result.stderr)
                self.assertIn(refused, result.stderr)
        self.assertFalse(self.output.exists())

    def test_parent_traversal_and_symlink_escape_are_refused(self):
        link = self.folder / 'outside'
        link.symlink_to('/tmp', target_is_directory=True)
        for path in [self.folder / '..' / '..' / '..' / 'outside.srt', link / 'new.srt']:
            with self.subTest(path=path):
                result = self.run_script(self.source, path)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(str(ROOT), result.stderr)
                self.assertIn(str(path), result.stderr)


class NarrationPaths(unittest.TestCase):
    setUp = MksrtPaths.setUp
    def run_pick(self, lines, outdir, output):
        return subprocess.run([sys.executable, '-I', str(NARRATE), 'pick', str(lines),
                               str(outdir), str(output)], capture_output=True, text=True, cwd=ROOT)

    def test_pick_in_root_writes_empty_results(self):
        self.source.write_text('[]')
        result = self.run_pick(self.source, self.folder, self.folder / 'narr.json')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual((self.folder / 'narr.json').read_text(), '[]')
        self.assertEqual((self.folder / 'narr.srt').read_text(), '')

    def test_pick_rejects_external_paths_and_ids(self):
        self.source.write_text(json.dumps([{'id': '../../outside'}]))
        for lines, outdir, output, refused in [
            ('/tmp/lines.json', self.folder, self.folder / 'narr.json', '/tmp/lines.json'),
            (self.source, '/tmp', self.folder / 'narr.json', '/tmp'),
            (self.source, self.folder, '/tmp/narr.json', '/tmp/narr.json'),
            (self.source, self.folder, self.folder / 'narr.json', '../../outside'),
        ]:
            with self.subTest(refused=refused):
                result = self.run_pick(lines, outdir, output)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(str(ROOT), result.stderr)
                self.assertIn(refused, result.stderr)


if __name__ == '__main__':
    unittest.main()
