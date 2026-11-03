"""Exercise process exclusion, interrupted publication and paid-request recovery."""
import contextlib
import io
import json
import multiprocessing
from pathlib import Path
import tempfile
import time
import unittest
from unittest import mock

import narrate
import narration_store as store

RESPONSE = json.dumps({'audio_base64': 'QUJD', 'alignment': {
    'characters': ['h', 'i'], 'character_start_times_seconds': [0, 0.1],
    'character_end_times_seconds': [0.1, 0.2]}})


def generate_worker(folder, start):
    """Model independent clients contending for one paid take."""
    folder = Path(folder)
    store.ROOT = folder

    def service(*_args):
        """Count accepted synthetic paid requests at the shared provider fixture."""
        with (folder / 'charges').open('a') as log:
            log.write('request\n')
        time.sleep(0.1)
        return 200, RESPONSE

    with mock.patch.object(narrate, 'status', return_value=(0, 100, 'test')):
        with mock.patch.object(narrate, 'curl', side_effect=service):
            with contextlib.redirect_stdout(io.StringIO()):
                start.wait(5)
                narrate.gen(str(folder / 'lines.json'), str(folder / 'takes'), 1)


def hold_lock_worker(folder, acquired):
    """Hold a lock until the test terminates this process."""
    store.ROOT = Path(folder)
    with store.locked([Path(folder) / 'takes']):
        acquired.set()
        time.sleep(30)


class NarrationTransactions(unittest.TestCase):
    """Related outputs recover together without repeating paid requests."""

    def setUp(self):
        """Create private synthetic data inside the project boundary."""
        scratch = tempfile.TemporaryDirectory(dir=narrate.ROOT)
        self.addCleanup(scratch.cleanup)
        self.folder = Path(scratch.name)
        root = mock.patch.object(store, 'ROOT', self.folder)
        root.start()
        self.addCleanup(root.stop)
        self.lines = self.folder / 'lines.json'
        self.lines.write_text(json.dumps([dict(id='line', text='hi', t=1, win=1)]))
        self.takes = self.folder / 'takes'

    def generate(self):
        """Run with a service mock supplied by each individual test."""
        with mock.patch.object(narrate, 'status', return_value=(0, 100, 'test')):
            with contextlib.redirect_stdout(io.StringIO()):
                narrate.gen(str(self.lines), str(self.takes), 1)

    def test_concurrent_generators_make_one_paid_request(self):
        """Two independent generators must publish one matching take and ledger entry."""
        context = multiprocessing.get_context('spawn')
        start = context.Event()
        processes = [context.Process(target=generate_worker, args=(str(self.folder), start))
                     for _ in range(2)]
        for process in processes:
            process.start()
        try:
            start.set()
            for process in processes:
                process.join(10)
                self.assertFalse(process.is_alive(), 'generator did not finish')
                self.assertEqual(process.exitcode, 0)
        finally:
            for process in processes:
                if process.is_alive():
                    process.terminate()
                    process.join()
        self.assertEqual((self.folder / 'charges').read_text(), 'request\n')
        self.assertEqual((self.takes / 'line_t1.mp3').read_bytes(), b'ABC')
        self.assertEqual(len((self.takes / 'usage.log').read_text().splitlines()), 1)
        self.assertEqual(json.loads((self.takes / 'line_t1.json').read_text())['text'], 'hi')

    def test_process_exit_releases_the_directory_lock(self):
        """A killed generator cannot leave another process blocked forever."""
        context = multiprocessing.get_context('spawn')
        acquired = context.Event()
        holder = context.Process(target=hold_lock_worker, args=(str(self.folder), acquired))
        holder.start()
        try:
            self.assertTrue(acquired.wait(5))
        finally:
            if holder.is_alive():
                holder.terminate()
            holder.join(5)
        start = context.Event()
        waiter = context.Process(target=generate_worker, args=(str(self.folder), start))
        waiter.start()
        try:
            start.set()
            waiter.join(10)
            self.assertFalse(waiter.is_alive(), 'terminated process retained its lock')
            self.assertEqual(waiter.exitcode, 0)
        finally:
            if waiter.is_alive():
                waiter.terminate()
                waiter.join()

    def test_explicit_error_retry_compatibility_does_not_assume_free_billing(self):
        """Keep the original context fallback and later explicit-error retries."""
        self.lines.write_text(json.dumps([dict(id='line', text='hi', prev='before')]))
        for code in (0, 400, 500):
            with self.subTest(code=code):
                with mock.patch.object(narrate, 'curl', return_value=(code, 'error')) as service:
                    self.generate()
                    self.generate()
                    self.assertEqual(service.call_count, 4)
                self.assertFalse(list(self.takes.glob('*.request')))

    def test_interrupted_take_publication_recovers_without_recharging(self):
        """A prepared response replays every output after an interrupted replacement."""
        replace = store.os.replace
        alignment = self.takes / 'line_t1.json'

        def stop_at_alignment(source, destination):
            """Leave a prepared journal between its first and second publication."""
            if Path(destination) == alignment:
                raise OSError('simulated interruption between related replacements')
            return replace(source, destination)

        with mock.patch.object(narrate, 'curl', return_value=(200, RESPONSE)) as service:
            with mock.patch.object(store.os, 'replace', side_effect=stop_at_alignment):
                with self.assertRaises(OSError):
                    self.generate()
            self.generate()
            self.assertEqual(service.call_count, 1)
        self.assertEqual((self.takes / 'line_t1.mp3').read_bytes(), b'ABC')
        self.assertTrue(alignment.exists())
        self.assertEqual(len((self.takes / 'usage.log').read_text().splitlines()), 1)
        self.assertFalse(list(self.takes.glob('.narrate-*')))

    def test_saved_response_recovers_before_any_publication(self):
        """A receipt survives failure before staging starts."""
        with mock.patch.object(narrate, 'curl', return_value=(200, RESPONSE)) as service:
            with mock.patch.object(narrate, '_save_take', side_effect=OSError('disk unavailable')):
                with self.assertRaises(OSError):
                    self.generate()
            self.generate()
            self.assertEqual(service.call_count, 1)
        self.assertEqual((self.takes / 'line_t1.mp3').read_bytes(), b'ABC')
        self.assertEqual(len((self.takes / 'usage.log').read_text().splitlines()), 1)

    def test_uncertain_request_cannot_silently_charge_again(self):
        """An abandoned attempt needs reconciliation before another paid attempt."""
        failure = RuntimeError('connection lost')
        with mock.patch.object(narrate, 'curl', side_effect=failure) as service:
            with self.assertRaises(RuntimeError):
                self.generate()
            with self.assertRaisesRegex(SystemExit, 'Uncertain paid narration request'):
                self.generate()
            self.assertEqual(service.call_count, 1)

    def test_staging_failure_leaves_original_outputs_intact(self):
        """Uncommitted staging never changes either existing destination."""
        paths = [self.folder / 'narr.json', self.folder / 'narr.srt']
        for path in paths:
            path.write_bytes(b'old')
        with mock.patch.object(store, 'write_state', side_effect=OSError('interrupted staging')):
            with self.assertRaises(OSError):
                store.publish(paths, [b'new json', b'new srt'])
        self.assertFalse(store.recover(paths))
        self.assertEqual([path.read_bytes() for path in paths], [b'old', b'old'])

    def test_interrupted_pick_pair_is_recovered_before_a_failing_reader(self):
        """Selection recovers an earlier pair even if the next input is unusable."""
        paths = [self.folder / 'narr.json', self.folder / 'narr.srt']
        for path in paths:
            path.write_bytes(b'old')
        replace = store.os.replace

        def stop_at_subtitles(source, destination):
            """Stop after JSON was replaced, with subtitles still stale."""
            if Path(destination) == paths[1]:
                raise OSError('simulated interruption')
            return replace(source, destination)

        with mock.patch.object(store.os, 'replace', side_effect=stop_at_subtitles):
            with self.assertRaises(OSError):
                store.publish(paths, [b'new json', b'new srt'])
        with mock.patch.object(narrate, 'best_take', side_effect=RuntimeError('bad media')):
            with self.assertRaises(RuntimeError):
                narrate.pick(str(self.lines), str(self.takes), str(paths[0]))
        self.assertEqual([path.read_bytes() for path in paths], [b'new json', b'new srt'])

    def test_interrupted_cleanup_keeps_published_outputs_and_discards_staging(self):
        """Cleanup may be retried after readiness has been removed."""
        paths = [self.folder / 'narr.json', self.folder / 'narr.srt']
        with mock.patch.object(store.shutil, 'rmtree', side_effect=OSError('interrupted cleanup')):
            with self.assertRaises(OSError):
                store.publish(paths, [b'json', b'srt'])
        self.assertFalse(store.recover(paths))
        self.assertEqual([path.read_bytes() for path in paths], [b'json', b'srt'])

    def test_duplicate_ids_and_no_take_keep_the_legacy_selection(self):
        """Duplicate IDs share the same take and missing lines remain a partial result."""
        self.takes.mkdir()
        (self.takes / 'line_t1.mp3').write_bytes(b'ABC')
        (self.takes / 'line_t1.json').write_text(json.dumps({'words': [['hi', 0, 0.2]]}))
        self.lines.write_text(json.dumps([
            dict(id='line', text='first', t=1, win=1),
            dict(id='line', text='second', t=2, win=1),
            dict(id='missing', text='missing', t=3, win=1)]))
        output = self.folder / 'narr.json'
        stdout = io.StringIO()
        with mock.patch.object(narrate, 'dur', return_value=0.3):
            with contextlib.redirect_stdout(stdout):
                narrate.pick(str(self.lines), str(self.takes), str(output))
        chosen = json.loads(output.read_text())
        self.assertEqual([line['text'] for line in chosen], ['first', 'second'])
        self.assertEqual([line['file'] for line in chosen], [str(self.takes / 'line_t1.mp3')] * 2)
        self.assertIn('missing: NO TAKE', stdout.getvalue())


if __name__ == '__main__':
    unittest.main()
