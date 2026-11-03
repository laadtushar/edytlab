"""Path confinement shared by standalone launch-video command-line tools."""
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def checked_path(path):
    """Resolve an input or output and refuse escapes, including through symlinks."""
    resolved = Path(path).resolve()
    if resolved != ROOT and ROOT not in resolved.parents:
        raise SystemExit(f"Refused path {path!r}: must resolve inside {ROOT}")
    return resolved
