"""Reject escaping media and metrics before invoking external processes."""
import json
import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import mix
import prof
import validate


class NarrationPaths(unittest.TestCase):
    """Exercise input, output and file-supplied paths at the project boundary."""

    def setUp(self):
        """Create private in-project and external directories and restore the caller cwd."""
        inside = tempfile.TemporaryDirectory(dir=prof.ROOT)
        outside = tempfile.TemporaryDirectory()
        self.addCleanup(inside.cleanup)
        self.addCleanup(outside.cleanup)
        self.cwd = Path(inside.name)
        self.external = Path(outside.name)
        previous = Path.cwd()
        os.chdir(self.cwd)
        self.addCleanup(os.chdir, previous)

    def assert_refused(self, operation, refused):
        """Require the refusal to identify the project root and rejected path."""
        with self.assertRaises(SystemExit) as error:
            operation()
        self.assertIn(str(prof.ROOT), str(error.exception))
        self.assertIn(str(refused), str(error.exception))

    def test_new_outputs_and_in_root_absolute_paths_are_permitted(self):
        """Allow new in-project files and normalized relative or absolute paths."""
        target = self.cwd / 'out' / 'new.wav'
        self.assertEqual(prof.checked_path(target), target)
        self.assertEqual(prof.checked_path('out/new.wav'), target)
        self.assertEqual(prof.checked_path('out/../new.wav'), self.cwd / 'new.wav')

    def test_absolute_traversal_and_symlink_escapes_are_refused(self):
        """Refuse absolute external paths, parent traversal and symlink escapes."""
        (self.cwd / 'linked').symlink_to(self.external, target_is_directory=True)
        for target in (
                self.external / 'new.wav', os.path.relpath(self.external / 'new.wav'),
                'linked/new.wav'):
            with self.subTest(target=target):
                self.assert_refused(lambda: prof.checked_path(target), target)

    def test_decoder_probe_and_measure_refuse_before_starting_process(self):
        """Validate each reader path before invoking its external process."""
        target = self.external / 'media.mp4'
        for operation in (prof.load, mix.dur, mix.read_wav_f32, mix.measure, validate.probe):
            with self.subTest(operation=operation.__name__), patch('subprocess.run') as run:
                self.assert_refused(lambda: operation(target), target)
                run.assert_not_called()

    def test_build_preflights_output_symlinks_before_writing(self):
        """Refuse escaped output directories before processing or creating work."""
        (self.cwd / 'out').symlink_to(self.external, target_is_directory=True)
        with patch('subprocess.run') as run:
            self.assert_refused(lambda: mix.build('R1', 'voice', 'out', {}, 'voice'), 'out')
            run.assert_not_called()
        self.assertEqual(list(self.external.iterdir()), [])
        self.assertFalse((self.cwd / 'work').exists())

    def test_build_preflights_input_symlinks_before_writing(self):
        """Refuse escaped narration inputs before creating output directories."""
        (self.cwd / 'lines').symlink_to(self.external, target_is_directory=True)
        with patch('subprocess.run') as run:
            self.assert_refused(lambda: mix.build(
                'R1', 'voice', 'out', {}, 'voice'), 'lines/voice/')
            run.assert_not_called()
        self.assertFalse((self.cwd / 'out').exists())

    def test_trimmed_line_refuses_escaped_output(self):
        """Refuse a line destination outside the project before trimming audio."""
        target = self.external / 'line.wav'
        with patch('subprocess.run') as run:
            self.assert_refused(lambda: mix.trimmed_line(
                'voice', 'line', 1.0, str(self.external)), target)
            run.assert_not_called()

    def test_metrics_file_and_embedded_media_are_validated(self):
        """Refuse escaped metrics files and media paths supplied inside their JSON."""
        self.assert_refused(lambda: validate.check(
            self.external / 'metrics.json'), self.external / 'metrics.json')
        metrics = self.cwd / 'metrics.json'
        metrics.write_text(json.dumps(dict(cut='R1', voice='voice',
                           tag='voice', out=str(self.external / 'out.mp4'))))
        with patch('subprocess.run') as run:
            self.assert_refused(lambda: validate.check(metrics), self.external / 'out.mp4')
            run.assert_not_called()

    def test_metrics_work_path_symlink_is_validated(self):
        """Refuse metrics whose derived work directory escapes through a symlink."""
        (self.cwd / 'work').symlink_to(self.external, target_is_directory=True)
        metrics = self.cwd / 'metrics.json'
        metrics.write_text(json.dumps(dict(cut='R1', voice='voice', tag='voice', out='out.mp4')))
        with patch('subprocess.run') as run:
            self.assert_refused(lambda: validate.check(metrics), 'work/voice-R1/pre.wav')
            run.assert_not_called()


if __name__ == '__main__':
    unittest.main()
