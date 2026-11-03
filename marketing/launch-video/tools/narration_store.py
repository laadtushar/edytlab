"""Cooperative narration locks and recoverable publication of related files.

Readers must hold the same lock and recover a transaction before using its
outputs. Independent files cannot be switched atomically for external readers
that bypass this protocol. Recovery covers process interruption; filesystem
and hardware durability guarantees still depend on the operating system.
"""
import contextlib
import errno
import hashlib
import io
import json
import os
import shutil
import time

from launch_paths import ROOT, checked_path


def _identity(paths):
    """Name a transaction from its resolved output paths."""
    canonical = [os.path.normcase(str(checked_path(path))) for path in paths]
    return hashlib.sha256('\0'.join(canonical).encode('utf-8')).hexdigest()


def state_file(paths, suffix):
    """Return an in-project state path tied to these exact destinations."""
    first = checked_path(paths[0])
    path = first.parent / f'.narrate-{_identity(paths)}.{suffix}'
    if path.is_symlink():
        raise SystemExit(f'Refused transaction symlink {path}: project root {ROOT}')
    return checked_path(path)


@contextlib.contextmanager
def locked(paths):
    """Hold an OS lock released automatically if the process exits.

    fcntl provides POSIX locks; msvcrt provides the matching Windows byte lock.
    Lock files stay in the ignored work folder so waiting processes always
    refer to the same inode. Removing a live lock file would break exclusion.
    """
    folder = checked_path(ROOT / 'work' / '.narrate-locks')
    folder.mkdir(parents=True, exist_ok=True)
    path = checked_path(folder / (_identity(paths) + '.lock'))
    with path.open('a+b') as stream:
        if os.name == 'nt':
            import msvcrt
            stream.seek(0, os.SEEK_END)
            if stream.tell() == 0:
                stream.write(b'\0')
                stream.flush()
            while True:
                stream.seek(0)
                try:
                    msvcrt.locking(stream.fileno(), msvcrt.LK_NBLCK, 1)
                    break
                except OSError as error:
                    if error.errno not in (errno.EACCES, errno.EDEADLK):
                        raise
                    time.sleep(0.05)
            try:
                yield
            finally:
                stream.seek(0)
                msvcrt.locking(stream.fileno(), msvcrt.LK_UNLCK, 1)
        else:
            import fcntl
            fcntl.flock(stream.fileno(), fcntl.LOCK_EX)
            try:
                yield
            finally:
                fcntl.flock(stream.fileno(), fcntl.LOCK_UN)


def text_bytes(text):
    """Encode exactly like the original default-encoding text file writes."""
    buffer = io.BytesIO()
    with io.TextIOWrapper(buffer, encoding=None, newline=None) as stream:
        stream.write(text)
        stream.flush()
        return buffer.getvalue()


def write_state(path, data):
    """Persist a complete state file before replacing its previous value."""
    path = checked_path(path)
    temporary = checked_path(str(path) + '.tmp')
    with temporary.open('wb') as stream:
        stream.write(data)
        stream.flush()
        os.fsync(stream.fileno())
    os.replace(temporary, path)


def _journal(paths):
    """Locate private staging while refusing symlink aliases."""
    path = state_file(paths, 'pending')
    if path.is_symlink():
        raise SystemExit(f'Refused transaction symlink {path}: project root {ROOT}')
    return path


def recover(paths):
    """Complete a prepared transaction, or discard staging never published.

    The caller must hold the lock that guards these outputs. Staged payloads
    remain until every destination has been replaced, making replay idempotent.
    """
    paths = [checked_path(path) for path in paths]
    journal = _journal(paths)
    if not journal.exists():
        return False
    ready = checked_path(journal / 'ready')
    prepared = ready.exists()
    if prepared:
        if ready.read_text() != str(len(paths)):
            raise RuntimeError(f'Invalid narration transaction: {journal}')
        payloads = [checked_path(journal / str(index)) for index in range(len(paths))]
        if not all(path.is_file() for path in payloads):
            raise RuntimeError(f'Incomplete prepared narration transaction: {journal}')
        for source, target in zip(payloads, paths):
            temporary = checked_path(target.parent / f'.narrate-{_identity(paths)}.publish')
            with source.open('rb') as src, temporary.open('wb') as dst:
                shutil.copyfileobj(src, dst)
                dst.flush()
                os.fsync(dst.fileno())
            os.replace(temporary, target)
        # Removing readiness first makes interruption during cleanup harmless:
        # all destinations are complete and remaining staging can be discarded.
        ready.unlink()
    shutil.rmtree(journal)
    return prepared


def publish(paths, payloads):
    """Stage all payloads before replacing any destination; recover on retry."""
    paths = [checked_path(path) for path in paths]
    recover(paths)
    if len(paths) != len(payloads):
        raise ValueError('Every narration output needs one staged payload')
    journal = _journal(paths)
    journal.mkdir()
    for index, payload in enumerate(payloads):
        with checked_path(journal / str(index)).open('xb') as stream:
            stream.write(payload)
            stream.flush()
            os.fsync(stream.fileno())
    write_state(journal / 'ready', text_bytes(str(len(paths))))
    recover(paths)


def _ledger_index(ledger):
    """Locate the private coordinator for a canonical usage ledger."""
    folder = checked_path(ROOT / 'work' / '.narrate-ledgers')
    folder.mkdir(parents=True, exist_ok=True)
    path = folder / (_identity([ledger]) + '.json')
    if path.is_symlink():
        raise SystemExit(f'Refused transaction symlink {path}: project root {ROOT}')
    return checked_path(path)


def _owner_paths(index, owner):
    """Validate the owner destination list and its pinned canonical paths."""
    if not isinstance(owner, list) or len(owner) not in (2, 3):
        raise RuntimeError(f'Invalid narration ledger owner: {index}')
    paths = [checked_path(path) for path in owner]
    if any(os.path.normcase(str(path)) != os.path.normcase(recorded)
           for path, recorded in zip(paths, owner)):
        raise SystemExit(f'Narration owner destination changed: {index}. '
                         'Restore or reconcile its recorded paths before retrying; '
                         'no new request was sent.')
    return paths


def _ledger_owner(index, ledger, record=None):
    """Validate the recorded destinations before attempting recovery."""
    if record is None:
        record = json.loads(index.read_text())
    if not isinstance(record, dict) or record.get('version') != 1 or 'owner' not in record:
        raise RuntimeError(f'Invalid narration ledger coordinator: {index}')
    owner = record['owner']
    if owner is None:
        return None
    paths = _owner_paths(index, owner)
    recorded_ledger = record.get('ledger', paths[-1] if len(paths) == 3 else None)
    if (recorded_ledger is None or checked_path(recorded_ledger) != checked_path(ledger)
            or (len(paths) == 3 and paths[-1] != checked_path(ledger))):
        raise RuntimeError(f'Wrong narration ledger owner: {index}')
    return paths


def _write_owner(index, paths, ledger):
    """Persist ownership before any paid request or take publication."""
    write_state(index, text_bytes(json.dumps(dict(
        version=1, ledger=str(ledger),
        owner=None if paths is None else [str(path) for path in paths]))))


def _indexed_state(candidate, folder):
    """Recognize other ledgers' active state only from exact owner manifests."""
    for index in folder.glob('*.json'):
        if index.is_symlink():
            raise SystemExit(f'Refused transaction symlink {index}: project root {ROOT}')
        record = json.loads(index.read_text())
        owner = record.get('owner') if isinstance(record, dict) else None
        if not isinstance(owner, list) or len(owner) not in (2, 3):
            continue
        ledger = record.get('ledger', owner[-1] if len(owner) == 3 else None)
        paths = _ledger_owner(index, ledger, record)
        if index.name != _identity([ledger]) + '.json':
            raise RuntimeError(f'Invalid narration ledger coordinator name: {index}')
        suffixes = ('pending', 'request', 'response') if len(paths) == 3 else ('pending',)
        if candidate in {state_file(paths, suffix) for suffix in suffixes}:
            return True
    return False


def _bootstrap_ledger(index, ledger, known, reconcile, outputs):
    """Reconcile known legacy state before creating a ledger coordinator."""
    if not index.exists():
        known = [[checked_path(path) for path in paths] for paths in known]
        names = {state_file(paths, suffix) for paths in known
                 for suffix in ('pending', 'request', 'response')}
        if outputs:
            names.add(state_file(outputs, 'pending'))
        for candidate in ROOT.rglob('.narrate-*'):
            # Another ledger may finish while its index is inspected;
            # recheck existence before refusing a stale directory entry.
            if (candidate.suffix in ('.pending', '.request', '.response')
                    and candidate not in names and candidate.exists()
                    and not _indexed_state(candidate, index.parent) and candidate.exists()):
                raise SystemExit(f'Unindexed narration transaction: {candidate}. '
                                 'Reconcile it with its original owning command '
                                 'before retrying; '
                                 'no new request was sent.')
        for paths in known:
            if any(state_file(paths, suffix).exists()
                   for suffix in ('pending', 'request', 'response')):
                reconcile(paths, legacy=True)
        _write_owner(index, None, ledger)


@contextlib.contextmanager
def ledger_locked(ledger, known, reconcile, outputs=()):
    """Recover a ledger's previous owner before admitting another writer.

    Directory locks are acquired first, then this canonical-ledger lock. The
    persistent idle coordinator makes legacy discovery a once-only migration.
    Legacy state names contain hashes, not destination manifests: never infer
    destinations from receipt IDs or silently discard unknown private state.
    """
    ledger = checked_path(ledger)
    with locked([ledger]):
        index = _ledger_index(ledger)
        _bootstrap_ledger(index, ledger, known, reconcile, outputs)
        owner = _ledger_owner(index, ledger)
        if owner is not None:
            if len(owner) == 3:
                reconcile(owner)
            else:
                with locked(owner):
                    recover(owner)
            _write_owner(index, None, ledger)
        yield


def claim_ledger(paths, ledger=None):
    """Record the active take while its canonical-ledger lock is held."""
    paths = [checked_path(path) for path in paths]
    ledger = checked_path(ledger if ledger is not None else paths[-1])
    index = _ledger_index(ledger)
    if _ledger_owner(index, ledger) is not None:
        raise RuntimeError(f'Unrecovered narration ledger owner: {index}')
    _write_owner(index, paths, ledger)


def release_ledger(paths, ledger=None):
    """Retire ownership only after the take and its markers are complete."""
    paths = [checked_path(path) for path in paths]
    ledger = checked_path(ledger if ledger is not None else paths[-1])
    index = _ledger_index(ledger)
    if _ledger_owner(index, ledger) != paths:
        raise RuntimeError(f'Unexpected narration ledger owner: {index}')
    _write_owner(index, None, ledger)
