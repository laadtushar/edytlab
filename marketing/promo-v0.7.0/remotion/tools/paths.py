"""Project boundary for the promo's local media tools."""
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def checked_path(path):
    """Resolve existing ancestors and symlinks before reading or writing."""
    resolved = Path(path).resolve()
    if resolved != ROOT and ROOT not in resolved.parents:
        raise SystemExit(f"Refused path {str(path)!r}: must resolve inside {ROOT}")
    return resolved
