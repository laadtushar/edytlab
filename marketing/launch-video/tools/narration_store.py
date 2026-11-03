"""Cooperative narration locks and recoverable publication of related files.

Readers must hold the same lock and recover a transaction before using its
outputs. Independent files cannot be switched atomically for external readers
that bypass this protocol. Recovery covers process interruption; filesystem
and hardware durability guarantees still depend on the operating system.
"""
import contextlib
import hashlib
import errno
import io
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
