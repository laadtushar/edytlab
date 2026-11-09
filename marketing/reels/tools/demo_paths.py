"""Local output and read-only demo input boundaries for reel tools."""
import os
from pathlib import Path
import re

REELS_DIR = Path(__file__).resolve().parents[1]
REPO_ROOT = Path(__file__).resolve().parents[3]


def checked_reel_path(path):
    """Resolve a local input/output inside this project's root, including symlinks."""
    resolved = Path(path).resolve()
    if resolved != REELS_DIR and REELS_DIR not in resolved.parents:
        raise SystemExit(f"Refused path {path!r}: must resolve inside {REELS_DIR}. "
                         "Copy external inputs (including /tmp files) into the project first.")
    return str(resolved)


def checked_demo_input(path):
    """Allow reel inputs and only the three repository demo recordings."""
    path = os.fspath(path)
    windows_drive = os.name == "nt" and re.match(r"^[A-Za-z]:[\\/]", path)
    if path.startswith("-") or (not windows_drive and ":" in path.split("/", 1)[0]):
        raise SystemExit(f"Refused path {path!r}: expected a local input filename.")
    resolved = Path(path).resolve()
    shared = REPO_ROOT / "website" / "public" / "demos"
    allowed = {shared / f"{slug}.mp4" for slug in (
        "dj-beatmatched-transition", "dj-extended-club-intro", "dj-mini-mix",
    )}
    if (resolved != REELS_DIR and REELS_DIR not in resolved.parents
            and (resolved not in allowed or not resolved.is_file())):
        raise SystemExit(f"Refused path {path!r}: input must resolve inside {REELS_DIR} "
                         "or to an allowed repository demo recording.")
    return path
