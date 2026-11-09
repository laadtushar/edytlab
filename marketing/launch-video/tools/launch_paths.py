"""Path confinement shared by standalone launch-video command-line tools."""
import os
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
REPO_ROOT = ROOT.parent.parent
SYSTEM_FONT = Path('/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf')
DEMO_NAMES = frozenset({
    'dj-beatmatched-transition.mp4', 'dj-extended-club-intro.mp4', 'dj-mini-mix.mp4',
})


def checked_path(path):
    """Resolve an input or output and refuse escapes, including through symlinks."""
    resolved = Path(path).resolve()
    if resolved != ROOT and ROOT not in resolved.parents:
        raise SystemExit(f"Refused path {path!r}: must resolve inside {ROOT}")
    return resolved


def checked_system_font():
    """Permit only the fixed DejaVu font, with no symlink redirection."""
    if SYSTEM_FONT.resolve() != SYSTEM_FONT or not SYSTEM_FONT.is_file():
        raise SystemExit(f'Refused system font {SYSTEM_FONT}: expected the fixed font file')
    return str(SYSTEM_FONT)


def checked_demo_path(path):
    """Validate an FFmpeg demo input, keeping its original argument spelling."""
    raw = os.fspath(path)
    native_drive = os.name == 'nt' and bool(os.path.splitdrive(raw)[0])
    protocol = re.match(r'^[a-zA-Z0-9+.-]+:', raw)
    if raw == '-' or (not native_drive and not raw.startswith('file:') and
                      (protocol or (raw.startswith('subfile,') and ':' in raw[8:]))):
        raise SystemExit(f'Refused demo path {raw!r}: must name a local file inside {ROOT}')
    actual = raw.removeprefix('file:')
    demo_dir = REPO_ROOT / 'website' / 'public' / 'demos'
    candidate = Path(actual).absolute()
    resolved_dir = demo_dir.resolve()
    repo = REPO_ROOT.resolve()
    if demo_dir in candidate.parents and repo not in resolved_dir.parents:
        raise SystemExit(f'Refused demo path {raw!r}: repository demo directory escapes {repo}')
    try:
        checked_path(actual)
        return raw
    except SystemExit:
        pass

    resolved = candidate.resolve()
    if (candidate.parent == demo_dir and candidate.name in DEMO_NAMES
            and repo in resolved_dir.parents
            and resolved == resolved_dir / candidate.name
            and resolved.is_file()):
        return raw
    raise SystemExit(f'Refused demo path {raw!r}: must resolve inside {ROOT} or be an approved repository demo')
