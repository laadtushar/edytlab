"""Exercise process exclusion, interrupted publication and paid-request recovery."""
import contextlib
import errno
import io
import json
import multiprocessing
import os
import subprocess
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


@contextlib.contextmanager
def observe_contention(gates):
    """Record real OS lock contention while preserving blocking acquisition."""
    if gates is None:
        yield
        return
    if os.name == 'nt':
        import msvcrt
        lock = msvcrt.locking

        def observed(stream, operation, count):
            """Observe the production nonblocking Windows lock attempt."""
            try:
                return lock(stream, operation, count)
            except OSError as error:
                if operation == msvcrt.LK_NBLCK and error.errno in (errno.EACCES, errno.EDEADLK):
                    gates['contended'].set()
                raise

        probe = mock.patch.object(msvcrt, 'locking', side_effect=observed)
    else:
        import fcntl
        lock = fcntl.flock

        def observed(stream, operation):
            """Probe POSIX exclusion before completing the original blocking call."""
            if operation != fcntl.LOCK_EX:
                return lock(stream, operation)
            try:
                return lock(stream, operation | fcntl.LOCK_NB)
            except BlockingIOError:
                gates['contended'].set()
                return lock(stream, operation)

        probe = mock.patch.object(fcntl, 'flock', side_effect=observed)
    with probe:
        yield


def generate_worker(folder, start, gates=None):
    """Generate only after the parent releases ready, contending clients."""
    folder = Path(folder)
    store.ROOT = folder

    def service(*_args):
        """Hold the first synthetic request until the second client contends."""
        with (folder / 'charges').open('a') as log:
            log.write('request\n')
        if gates is not None:
            gates['service_started'].set()
            if not gates['release_service'].wait(15):
                raise RuntimeError('parent did not release the held narration response')
        return 200, RESPONSE

    with mock.patch.object(narrate, 'status', return_value=(0, 100, 'test')), \
            mock.patch.object(narrate, 'curl', side_effect=service), \
            observe_contention(gates), contextlib.redirect_stdout(io.StringIO()):
        if gates is not None:
            gates['ready'].set()
        if not start.wait(15):
            raise RuntimeError('parent did not release the narration start gate')
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
        with mock.patch.object(narrate, 'status', return_value=(0, 100, 'test')), \
                contextlib.redirect_stdout(io.StringIO()):
            narrate.gen(str(self.lines), str(self.takes), 1)

    def test_concurrent_generators_make_one_paid_request(self):
        """Two independent generators must publish one matching take and ledger entry."""
        context = multiprocessing.get_context('spawn')
        start = context.Event()
        service_started = context.Event()
        release_service = context.Event()
        contended = context.Event()
        gates = [dict(ready=context.Event(), contended=contended,
                      service_started=service_started, release_service=release_service)
                 for _ in range(2)]
        processes = [context.Process(target=generate_worker,
                                     args=(str(self.folder), start, gate)) for gate in gates]
        for process in processes:
            process.start()
        try:
            for gate in gates:
                self.assertTrue(gate['ready'].wait(15), 'generator did not reach the start gate')
            start.set()
            self.assertTrue(service_started.wait(15), 'no generator reached the service')
            self.assertTrue(contended.wait(15), 'second generator did not contend for the lock')
            self.assertTrue(all(process.is_alive() for process in processes))
            self.assertEqual((self.folder / 'charges').read_text(), 'request\n')
            release_service.set()
            for process in processes:
                process.join(15)
                self.assertFalse(process.is_alive(), 'generator did not finish')
                self.assertEqual(process.exitcode, 0)
        finally:
            release_service.set()
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
        for code in (400, 500):
            with self.subTest(code=code):
                with mock.patch.object(narrate, 'curl', return_value=(code, 'error')) as service:
                    self.generate()
                    self.generate()
                    self.assertEqual(service.call_count, 4)
                paths = narrate._take_paths(f"{self.takes}/line_t1", self.takes)
                self.assertFalse(store.state_file(paths, 'request').exists())

    def test_transport_loss_preserves_marker_without_context_or_later_retry(self):
        """A lost first response cannot trigger a second potentially paid request."""
        for context in ({}, {'prev': 'before', 'next': 'after'}):
            with self.subTest(context=context):
                self.lines.write_text(json.dumps([dict(id='line', text='hi', **context)]))
                paths = narrate._take_paths(f"{self.takes}/line_t1", self.takes)
                request = store.state_file(paths, 'request')
                with mock.patch.object(narrate, 'curl', return_value=(0, '')) as service:
                    message = 'No HTTP response was received'
                    with self.assertRaisesRegex(SystemExit, message) as error:
                        self.generate()
                    self.assertIn(str(request), str(error.exception))
                    marker = request.read_bytes()
                    with self.assertRaisesRegex(SystemExit, 'Uncertain paid narration request'):
                        self.generate()
                    self.assertEqual(service.call_count, 1)
                    self.assertEqual(request.read_bytes(), marker)
                self.assertEqual(json.loads(marker)['text'], 'hi')
                self.assertFalse(any(path.exists() for path in paths))
                self.assertFalse(store.state_file(paths, 'response').exists())
                # The synthetic provider reconciles this subcase before the next one.
                request.unlink()

    def test_curl_timeout_without_http_response_keeps_reconciliation_marker(self):
        """Exercise curl's HTTP000 parser after a timeout, without calling a provider."""
        self.lines.write_text(json.dumps([dict(id='line', text='hi', prev='before')]))
        paths = narrate._take_paths(f"{self.takes}/line_t1", self.takes)
        request = store.state_file(paths, 'request')
        timeout = subprocess.CompletedProcess([], 28, stdout="\n000", stderr="request timed out")
        with mock.patch.dict(narrate.os.environ, {'ELEVENLABS_API_KEY': 'synthetic-fixture-key'}), \
                mock.patch.object(narrate.subprocess, 'run', return_value=timeout) as process:
            with self.assertRaisesRegex(SystemExit, 'No HTTP response was received'):
                self.generate()
            marker = request.read_bytes()
            with self.assertRaisesRegex(SystemExit, 'Uncertain paid narration request'):
                self.generate()
            self.assertEqual(process.call_count, 1)
            self.assertEqual(request.read_bytes(), marker)
        self.assertFalse(any(path.exists() for path in paths))

    def test_transport_loss_during_context_fallback_stops_later_retries(self):
        """An explicit error permits fallback, but a lost fallback response does not."""
        self.lines.write_text(json.dumps([dict(id='line', text='hi', prev='before')]))
        for code in (400, 500):
            with self.subTest(code=code):
                paths = narrate._take_paths(f"{self.takes}/line_t1", self.takes)
                request = store.state_file(paths, 'request')
                replies = [(code, 'error'), (0, '')]
                with mock.patch.object(narrate, 'curl', side_effect=replies) as service:
                    with self.assertRaisesRegex(SystemExit, 'No HTTP response was received'):
                        self.generate()
                    marker = request.read_bytes()
                    with self.assertRaisesRegex(SystemExit, 'Uncertain paid narration request'):
                        self.generate()
                    self.assertEqual(service.call_count, 2)
                    self.assertEqual(request.read_bytes(), marker)
                self.assertFalse(any(path.exists() for path in paths))
                request.unlink()

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
            with mock.patch.object(store.os, 'replace', side_effect=stop_at_alignment), \
                    self.assertRaises(OSError):
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
            failure = OSError('disk unavailable')
            with mock.patch.object(narrate, '_save_take', side_effect=failure), \
                    self.assertRaises(OSError):
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
        with mock.patch.object(store, 'write_state', side_effect=OSError('interrupted staging')), \
                self.assertRaises(OSError):
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

        with mock.patch.object(store.os, 'replace', side_effect=stop_at_subtitles), \
                self.assertRaises(OSError):
            store.publish(paths, [b'new json', b'new srt'])
        with mock.patch.object(narrate, 'best_take', side_effect=RuntimeError('bad media')), \
                self.assertRaises(RuntimeError):
            narrate.pick(str(self.lines), str(self.takes), str(paths[0]))
        self.assertEqual([path.read_bytes() for path in paths], [b'new json', b'new srt'])

    def test_interrupted_cleanup_keeps_published_outputs_and_discards_staging(self):
        """Cleanup may be retried after readiness has been removed."""
        paths = [self.folder / 'narr.json', self.folder / 'narr.srt']
        failure = OSError('interrupted cleanup')
        with mock.patch.object(store.shutil, 'rmtree', side_effect=failure), \
                self.assertRaises(OSError):
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
        with mock.patch.object(narrate, 'dur', return_value=0.3), \
                contextlib.redirect_stdout(stdout):
            narrate.pick(str(self.lines), str(self.takes), str(output))
        chosen = json.loads(output.read_text())
        self.assertEqual([line['text'] for line in chosen], ['first', 'second'])
        # The original JSON preserves the supplied directory spelling and
        # appends "/<id>_t1.mp3", including mixed separators on Windows.
        expected_file = f'{self.takes}/line_t1.mp3'
        self.assertEqual([line['file'] for line in chosen], [expected_file] * 2)
        self.assertIn('missing: NO TAKE', stdout.getvalue())


if __name__ == '__main__':
    unittest.main()
