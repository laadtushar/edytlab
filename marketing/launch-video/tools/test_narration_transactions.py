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


def generate_worker(folder, start, gates=None, lines=None, outdir=None):
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
        narrate.gen(str(lines or folder / 'lines.json'), str(outdir or folder / 'takes'), 1)


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
        scratch = tempfile.TemporaryDirectory(dir=store.ROOT)
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

    def test_interrupted_other_take_recovers_before_new_ledger_snapshot(self):
        """A-only recovery cannot erase B after a B-only invocation."""
        self._other_take_recovery(False)

    def test_interrupted_take_recovers_through_canonical_ledger_alias(self):
        """Distinct directories sharing a ledger recover its owner first."""
        self._other_take_recovery(True)

    def _other_take_recovery(self, alias, same_id=False):
        """Exercise actual gen interleaving with a held prepared journal."""
        self.takes.mkdir()
        other = self.takes
        if alias:
            other = self.folder / 'other'
            other.mkdir()
            ledger = self.folder / 'shared.log'
            ledger.write_bytes(b'')
            try:
                (self.takes / 'usage.log').symlink_to(ledger)
                (other / 'usage.log').symlink_to(ledger)
            except OSError as error:
                self.skipTest(f'filesystem symlinks unavailable: {error}')
        first, second = ('same', 'same') if same_id else ('A', 'B')
        self.lines.write_text(json.dumps([dict(id=first, text='hi')]))
        replace = store.os.replace

        def stop_at_alignment(source, destination):
            """Interrupt A after audio, leaving its old ledger snapshot prepared."""
            if Path(destination) == self.takes / f'{first}_t1.json':
                raise OSError('interrupted A publication')
            return replace(source, destination)

        with mock.patch.object(narrate, 'curl', return_value=(200, RESPONSE)) as service, \
                mock.patch.object(narrate.time, 'strftime', return_value='2026-01-01 00:00:00'):
            with mock.patch.object(store.os, 'replace', side_effect=stop_at_alignment), \
                    self.assertRaises(OSError):
                self.generate()
            self.lines.write_text(json.dumps([dict(id=second, text='hi')]))
            with mock.patch.object(narrate, 'status', return_value=(0, 100, 'test')), \
                    contextlib.redirect_stdout(io.StringIO()):
                narrate.gen(str(self.lines), str(other), 1)
            self.lines.write_text(json.dumps([dict(id=first, text='hi')]))
            self.generate()
            self.assertEqual(service.call_count, 2)
        rows = (self.takes / 'usage.log').read_text().splitlines()
        self.assertEqual(len(rows), 2)
        if same_id:
            self.assertEqual(rows[0], rows[1])
        self.assertIn(f'  {first} take 1  2 chars', rows[0])
        self.assertIn(f'  {second} take 1  2 chars', rows[1])
        self.assertEqual((other / f'{second}_t1.mp3').read_bytes(), b'ABC')
        self.assertEqual((self.takes / f'{first}_t1.mp3').read_bytes(), b'ABC')

    def test_identical_visible_entries_from_aliased_takes_are_both_retained(self):
        """Two distinct paid takes may have identical legacy ledger bytes."""
        self._other_take_recovery(True, same_id=True)

    def test_pick_recovers_unselected_ledger_owner(self):
        """Selecting B must complete interrupted A even though A is not selected."""
        self.lines.write_text(json.dumps([dict(id='A', text='hi', t=0, win=1)]))
        replace = store.os.replace

        def interrupt(source, destination):
            """Leave A prepared before its metadata is installed."""
            if Path(destination) == self.takes / 'A_t1.json':
                raise OSError('interrupted A')
            return replace(source, destination)

        with mock.patch.object(narrate, 'curl', return_value=(200, RESPONSE)) as service:
            with mock.patch.object(store.os, 'replace', side_effect=interrupt), \
                    self.assertRaises(OSError):
                self.generate()
            self.lines.write_text(json.dumps([dict(id='missing', text='hi', t=0, win=1)]))
            with contextlib.redirect_stdout(io.StringIO()):
                narrate.pick(str(self.lines), str(self.takes), str(self.folder / 'narr.json'))
            self.assertEqual(service.call_count, 1)
        self.assertTrue((self.takes / 'A_t1.json').exists())
        self.assertIn('  A take 1  ', (self.takes / 'usage.log').read_text())
        self.assertEqual((self.folder / 'narr.json').read_bytes(), b'[]')

    def test_fresh_independent_ledger_proceeds_while_another_owner_is_active(self):
        """An indexed A transaction is not mistaken for unrelated legacy state."""
        context = multiprocessing.get_context('spawn')
        start = context.Event()
        gates = dict(ready=context.Event(), contended=context.Event(),
                     service_started=context.Event(), release_service=context.Event())
        process = context.Process(target=generate_worker, args=(str(self.folder), start, gates))
        process.start()
        try:
            self.assertTrue(gates['ready'].wait(15))
            start.set()
            self.assertTrue(gates['service_started'].wait(15))
            other = self.folder / 'independent'
            with mock.patch.object(narrate, 'status', return_value=(0, 100, 'test')), \
                    mock.patch.object(narrate, 'curl', return_value=(200, RESPONSE)) as service, \
                    contextlib.redirect_stdout(io.StringIO()):
                narrate.gen(str(self.lines), str(other), 1)
                self.assertEqual(service.call_count, 1)
            self.assertEqual((other / 'line_t1.mp3').read_bytes(), b'ABC')
            self.assertTrue(process.is_alive(), 'A must remain held while B finishes')
        finally:
            gates['release_service'].set()
            process.join(15)
            if process.is_alive():
                process.terminate()
                process.join()
        self.assertEqual(process.exitcode, 0)

    def test_changed_owner_destination_keeps_unknown_request_and_prevents_recharge(self):
        """An in-root leaf retarget cannot change the saved request's identity."""
        paths = narrate._take_paths(f'{self.takes}/line_t1', self.takes)
        request = store.state_file(paths, 'request')
        with mock.patch.object(narrate, 'curl', return_value=(0, '')) as service:
            with self.assertRaisesRegex(SystemExit, 'No HTTP response'):
                self.generate()
            marker = request.read_bytes()
            index = store._ledger_index(paths[-1])
            owner = index.read_bytes()
            redirected = self.folder / 'redirected.mp3'
            try:
                paths[0].symlink_to(redirected)
            except OSError as error:
                self.skipTest(f'filesystem symlinks unavailable: {error}')
            with self.assertRaises(SystemExit) as error:
                self.generate()
            self.assertIn(str(index), str(error.exception))
            with self.assertRaisesRegex(SystemExit, 'owner destination changed'):
                narrate.pick(str(self.lines), str(self.takes), str(self.folder / 'narr.json'))
            self.assertEqual(service.call_count, 1)
        self.assertEqual(request.read_bytes(), marker)
        self.assertEqual(index.read_bytes(), owner)
        self.assertFalse(redirected.exists())
        self.assertFalse((self.takes / 'usage.log').exists())

    def test_other_index_is_validated_from_one_atomic_snapshot(self):
        """Retiring another ledger between reads must not turn paths into None."""
        self.takes.mkdir()
        paths = narrate._take_paths(f'{self.takes}/line_t1', self.takes)
        index = store._ledger_index(paths[-1])
        store._write_owner(index, paths, paths[-1])
        response = store.state_file(paths, 'response')
        response.write_text('{}')
        read, reads = Path.read_text, []

        def retire_after_read(path, *args, **kwargs):
            """Return its active snapshot while the real index becomes idle."""
            text = read(path, *args, **kwargs)
            if path == index:
                reads.append(path)
                response.unlink(missing_ok=True)
                store._write_owner(index, None, paths[-1])
            return text

        with mock.patch.object(Path, 'read_text', new=retire_after_read):
            self.assertTrue(store._indexed_state(response, index.parent))
        self.assertEqual(len(reads), 1)
        self.assertFalse(response.exists())

    def test_independent_ledger_proceeds_during_indexed_pick_publication(self):
        """An active two-output pick journal has an exact coordinator owner."""
        output = self.folder / 'narr.json'
        other = self.folder / 'independent'
        replace = store.os.replace
        observed = []

        def generate_during_pick(source, destination):
            """Invoke B while A's prepared selection journal still exists."""
            if Path(destination) == output:
                observed.append(True)
                self.assertTrue(store.state_file(
                    [output, output.with_suffix('.srt')], 'pending').exists())
                narrate.gen(str(self.lines), str(other), 1)
            return replace(source, destination)

        with mock.patch.object(narrate, 'status', return_value=(0, 100, 'test')), \
                mock.patch.object(narrate, 'curl', return_value=(200, RESPONSE)) as service, \
                mock.patch.object(store.os, 'replace', side_effect=generate_during_pick), \
                contextlib.redirect_stdout(io.StringIO()):
            narrate.pick(str(self.lines), str(self.takes), str(output))
            self.assertEqual(service.call_count, 1)
        self.assertEqual(observed, [True])
        self.assertEqual(output.read_bytes(), b'[]')
        self.assertEqual((other / 'line_t1.mp3').read_bytes(), b'ABC')

    def test_interrupted_indexed_pick_remains_discoverable_to_another_ledger(self):
        """Pair ownership survives failure and is recovered with its pair lock."""
        output = self.folder / 'narr.json'
        replace = store.os.replace

        def interrupt(source, destination):
            """Leave A's prepared pair and its durable owner behind."""
            if Path(destination) == output:
                raise OSError('interrupted indexed pick')
            return replace(source, destination)

        with mock.patch.object(store.os, 'replace', side_effect=interrupt), \
                contextlib.redirect_stdout(io.StringIO()), self.assertRaises(OSError):
            narrate.pick(str(self.lines), str(self.takes), str(output))
        other = self.folder / 'independent'
        with mock.patch.object(narrate, 'status', return_value=(0, 100, 'test')), \
                mock.patch.object(narrate, 'curl', return_value=(200, RESPONSE)), \
                contextlib.redirect_stdout(io.StringIO()):
            narrate.gen(str(self.lines), str(other), 1)
        with mock.patch.object(narrate, '_pick_lines', side_effect=RuntimeError('next input')), \
                self.assertRaisesRegex(RuntimeError, 'next input'):
            narrate.pick(str(self.lines), str(self.takes), str(output))
        self.assertEqual(output.read_bytes(), b'[]')
        self.assertEqual(output.with_suffix('.srt').read_bytes(), b'')
        self.assertEqual((other / 'line_t1.mp3').read_bytes(), b'ABC')

    def test_directory_ledger_alias_is_rejected_before_any_lock(self):
        """Crossed directory aliases cannot invert directory and ledger locks."""
        self.takes.mkdir()
        other = self.folder / 'other'
        other.mkdir()
        try:
            (self.takes / 'usage.log').symlink_to(other, target_is_directory=True)
            (other / 'usage.log').symlink_to(self.takes, target_is_directory=True)
        except OSError as error:
            self.skipTest(f'filesystem symlinks unavailable: {error}')
        with mock.patch.object(store, 'locked') as locks, \
                mock.patch.object(narrate, 'curl') as service:
            for outdir in (self.takes, other):
                with self.subTest(outdir=outdir):
                    with self.assertRaisesRegex(SystemExit, 'regular file'):
                        narrate.gen(str(self.lines), str(outdir), 1)
                    with self.assertRaisesRegex(SystemExit, 'regular file'):
                        narrate.pick(str(self.lines), str(outdir), str(self.folder / 'narr.json'))
            locks.assert_not_called()
            service.assert_not_called()

    def test_alias_generators_contend_for_the_canonical_ledger(self):
        """Different directory locks cannot admit simultaneous ledger writers."""
        self.takes.mkdir()
        other = self.folder / 'other'
        other.mkdir()
        ledger = self.folder / 'shared.log'
        ledger.write_bytes(b'')
        try:
            (self.takes / 'usage.log').symlink_to(ledger)
            (other / 'usage.log').symlink_to(ledger)
        except OSError as error:
            self.skipTest(f'filesystem symlinks unavailable: {error}')
        alternate = self.folder / 'other-lines.json'
        alternate.write_text(json.dumps([dict(id='other', text='hi')]))
        context = multiprocessing.get_context('spawn')
        start, contended = context.Event(), context.Event()
        service_started, release_service = context.Event(), context.Event()
        gates = [dict(ready=context.Event(), contended=contended,
                      service_started=service_started, release_service=release_service)
                 for _ in range(2)]
        processes = [context.Process(target=generate_worker, args=(
            str(self.folder), start, gates[index], str(lines), str(outdir)))
            for index, (lines, outdir) in enumerate(((self.lines, self.takes), (alternate, other)))]
        for process in processes:
            process.start()
        try:
            for gate in gates:
                self.assertTrue(gate['ready'].wait(15))
            start.set()
            self.assertTrue(service_started.wait(15))
            self.assertTrue(contended.wait(15), 'aliased ledger was not locked')
            self.assertEqual((self.folder / 'charges').read_text(), 'request\n')
            release_service.set()
            for process in processes:
                process.join(15)
                self.assertFalse(process.is_alive())
                self.assertEqual(process.exitcode, 0)
        finally:
            release_service.set()
            for process in processes:
                if process.is_alive():
                    process.terminate()
                    process.join()
        self.assertEqual((self.folder / 'charges').read_text(), 'request\nrequest\n')
        rows = ledger.read_text().splitlines()
        self.assertEqual(len(rows), 2)
        self.assertTrue(any('  line take 1  ' in row for row in rows))
        self.assertTrue(any('  other take 1  ' in row for row in rows))

    def test_unindexed_unknown_state_refuses_before_paid_work(self):
        """Legacy hashes cannot be reverse-mapped by guessing receipt IDs."""
        unknown = self.folder / '.narrate-unknown.response'
        unknown.write_text('{}')
        with mock.patch.object(narrate, 'curl') as service:
            with self.assertRaises(SystemExit) as error:
                self.generate()
            self.assertIn(str(unknown), str(error.exception))
            service.assert_not_called()
        self.assertEqual(unknown.read_text(), '{}')

    def test_owning_command_migrates_known_legacy_journal(self):
        """Exact known legacy paths may recover when no later ledger conflicts."""
        self.takes.mkdir()
        paths = narrate._take_paths(f'{self.takes}/line_t1', self.takes)
        receipt = dict(id='line', take=1, text='hi', timestamp='2026-01-01 00:00:00',
                       response=json.loads(RESPONSE))
        store.write_state(store.state_file(paths, 'response'),
                          store.text_bytes(json.dumps(receipt)))
        replace = store.os.replace

        def stop_at_alignment(source, destination):
            """Prepare the old unindexed protocol before its second replacement."""
            if Path(destination) == paths[1]:
                raise OSError('legacy interruption')
            return replace(source, destination)

        with mock.patch.object(store.os, 'replace', side_effect=stop_at_alignment), \
                self.assertRaises(OSError):
            narrate._save_take(paths, receipt)
        with mock.patch.object(narrate, 'curl') as service:
            self.generate()
            service.assert_not_called()
        self.assertEqual(paths[0].read_bytes(), b'ABC')
        expected = b'2026-01-01 00:00:00  line take 1  2 chars' + os.linesep.encode()
        self.assertEqual(paths[2].read_bytes(), expected)

    def stage_legacy_take(self, folder, stop_before):
        """Run the old unindexed publication protocol until a chosen replacement."""
        takes = folder / 'takes'
        takes.mkdir()
        paths = narrate._take_paths(str(takes / 'line_t1'), takes)
        receipt = dict(id='line', take=1, text='hi', timestamp='2026-01-01 00:00:00',
                       response=json.loads(RESPONSE))
        for suffix in ('request', 'response'):
            store.write_state(store.state_file(paths, suffix),
                              store.text_bytes(json.dumps(receipt)))
        replace = store.os.replace

        def interrupt(source, destination):
            """Preserve the prepared legacy journal at the selected crash boundary."""
            if Path(destination) == paths[stop_before]:
                raise OSError('legacy interruption')
            return replace(source, destination)

        with mock.patch.object(store.os, 'replace', side_effect=interrupt), \
                self.assertRaises(OSError):
            narrate._save_take(paths, receipt)
        return paths, receipt

    def test_equal_legacy_ledger_refuses_even_when_media_already_match(self):
        """A second paid take can create the same visible row before migration."""
        for stop_before in (1, 2):
            for command in ('gen', 'pick'):
                with self.subTest(stop_before=stop_before, command=command), \
                        tempfile.TemporaryDirectory(dir=self.folder) as temporary:
                    folder = Path(temporary)
                    with mock.patch.object(store, 'ROOT', folder):
                        paths, receipt = self.stage_legacy_take(folder, stop_before)
                        other = folder / 'other'
                        other.mkdir()
                        other_response = dict(receipt['response'], audio_base64='REVG')
                        other_receipt = dict(receipt, response=other_response)
                        # Both old writers address the same canonical aliased ledger.
                        narrate._save_take([other / 'line_t1.mp3', other / 'line_t1.json',
                                            paths[2]], other_receipt)
                        journal = store.state_file(paths, 'pending')
                        self.assertEqual((journal / '2').read_bytes(), paths[2].read_bytes())
                        index = store._ledger_index(paths[2])
                        self.assertFalse(index.exists())
                        markers = [store.state_file(paths, suffix)
                                   for suffix in ('request', 'response')]
                        observed = [*paths, *markers, *journal.iterdir()]
                        before = {path: path.read_bytes() for path in observed if path.is_file()}
                        alignment_existed = paths[1].exists()
                        with mock.patch.object(narrate, 'curl') as service, \
                                mock.patch.object(narrate, 'status',
                                                  return_value=(0, 100, 'test')), \
                                mock.patch.object(narrate, 'dur', return_value=0.2), \
                                contextlib.redirect_stdout(io.StringIO()), \
                                self.assertRaisesRegex(
                                    SystemExit, 'Conflicting legacy narration ledger'):
                            if command == 'gen':
                                narrate.gen(str(self.lines), str(folder / 'takes'), 1)
                            else:
                                narrate.pick(str(self.lines), str(folder / 'takes'),
                                             str(folder / 'chosen.json'))
                        service.assert_not_called()
                        self.assertEqual({path: path.read_bytes() for path in before}, before)
                        self.assertEqual(paths[1].exists(), alignment_existed)
                        self.assertFalse(index.exists())
                        self.assertFalse((folder / 'chosen.json').exists())
                        self.assertFalse((folder / 'chosen.srt').exists())

    def test_legacy_media_can_finish_its_unwritten_ledger(self):
        """Matching media remain recoverable when the ledger still needs this row."""
        paths, _receipt = self.stage_legacy_take(self.folder, 2)
        self.assertTrue(paths[0].exists() and paths[1].exists())
        self.assertFalse(paths[2].exists())
        with mock.patch.object(narrate, 'curl') as service:
            self.generate()
            service.assert_not_called()
        expected = b'2026-01-01 00:00:00  line take 1  2 chars' + os.linesep.encode()
        self.assertEqual(paths[2].read_bytes(), expected)

    def test_legacy_staging_symlinks_are_refused_before_external_reads(self):
        """Migration applies the same confinement as normal journal recovery."""
        self.takes.mkdir()
        paths = narrate._take_paths(f'{self.takes}/line_t1', self.takes)
        journal = store.state_file(paths, 'pending')
        journal.mkdir()
        with tempfile.TemporaryDirectory() as external:
            outside = Path(external) / 'sentinel'
            outside.write_bytes(b'external sentinel')
            for child in ('ready', '2'):
                with self.subTest(child=child):
                    for old in journal.iterdir():
                        old.unlink()
                    if child == '2':
                        (journal / 'ready').write_text('3')
                    try:
                        (journal / child).symlink_to(outside)
                    except OSError as error:
                        self.skipTest(f'filesystem symlinks unavailable: {error}')
                    read = Path.read_bytes

                    def guarded_read(path, read=read):
                        """Fail if the external payload reaches the filesystem reader."""
                        if path.resolve() == outside:
                            raise AssertionError('external staging payload was read')
                        return read(path)

                    with mock.patch.object(Path, 'read_bytes', new=guarded_read), \
                            self.assertRaisesRegex(SystemExit, 'Refused path'):
                        narrate._recover_paths(paths, legacy=True)
            self.assertEqual(outside.read_bytes(), b'external sentinel')

    def test_legacy_stale_snapshot_cannot_erase_newer_ledger(self):
        """An already conflicting old journal needs explicit reconciliation."""
        self.takes.mkdir()
        paths = narrate._take_paths(f'{self.takes}/line_t1', self.takes)
        receipt = dict(id='line', take=1, text='hi', timestamp='2026-01-01 00:00:00',
                       response=json.loads(RESPONSE))
        store.write_state(store.state_file(paths, 'response'),
                          store.text_bytes(json.dumps(receipt)))
        replace = store.os.replace

        def stop_at_alignment(source, destination):
            """Preserve a staged legacy ledger and its original response."""
            if Path(destination) == paths[1]:
                raise OSError('legacy interruption')
            return replace(source, destination)

        with mock.patch.object(store.os, 'replace', side_effect=stop_at_alignment), \
                self.assertRaises(OSError):
            narrate._save_take(paths, receipt)
        paths[2].write_bytes(b'newer independent B entry\n')
        with mock.patch.object(narrate, 'curl') as service:
            with self.assertRaisesRegex(SystemExit, 'Conflicting legacy narration ledger'):
                self.generate()
            service.assert_not_called()
        self.assertEqual(paths[2].read_bytes(), b'newer independent B entry\n')
        self.assertTrue(store.state_file(paths, 'response').exists())
        self.assertTrue((store.state_file(paths, 'pending') / 'ready').exists())

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
