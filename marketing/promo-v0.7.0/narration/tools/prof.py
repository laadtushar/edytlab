"""Measure audio levels in windows.

Usage: prof.py AUDIO WINDOW_SECONDS
Inputs must stay in marketing/promo-v0.7.0. Copy external inputs, including
/tmp files, into that project before running the tool.
"""
import subprocess
import sys
import numpy as np
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def checked_path(path):
    """Resolve a path and refuse escapes, including through symlinks."""
    resolved = Path(path).resolve()
    if resolved != ROOT and ROOT not in resolved.parents:
        raise SystemExit(f"Refused path {str(path)!r}: must resolve inside {ROOT}")
    return resolved


def load(path, sr=48000):
    """Decode a project-local media file into stereo float samples."""
    path = str(checked_path(path))
    raw = subprocess.run(
        ['ffmpeg', '-v', 'error', '-i', path, '-vn', '-ac', '2', '-ar', str(sr),
         '-f', 'f32le', '-'],
        capture_output=True, check=False).stdout
    return np.frombuffer(raw, dtype=np.float32).reshape(-1, 2)


if __name__ == '__main__':
    if sys.argv[1:] in (['-h'], ['--help']):
        print(__doc__)
        raise SystemExit(0)
    x = load(sys.argv[1])
    win = float(sys.argv[2])
    n = int(48000*win)
    out = []
    for i in range(0, len(x), n):
        c = x[i:i+n]
        r = np.sqrt((c**2).mean()+1e-12)
        out.append('%d:%d' % (i/48000, round(20*np.log10(r))))
    print(' '.join(out))
