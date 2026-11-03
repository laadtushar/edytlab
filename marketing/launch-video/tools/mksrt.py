#!/usr/bin/env python3
"""mksrt.py cues.json out.srt -> SRT of the on-screen text.

Inputs and outputs must stay inside marketing/launch-video, including through
symlinks. Copy external inputs (such as /tmp files) into that folder first.
"""
import json
import sys
from pathlib import Path

# Keep the project helper importable when Python's isolated mode omits script paths.
sys.path.insert(0, str(Path(__file__).resolve().parent))
from launch_paths import checked_path


def ts(x):
    """Format seconds as an SRT timestamp."""
    h, r = divmod(x, 3600)
    m, s = divmod(r, 60)
    return f"{int(h):02d}:{int(m):02d}:{int(s):02d},{int(round((s - int(s)) * 1000)):03d}"


def main():
    """Convert the caption cues to SRT in the project directory."""
    if len(sys.argv) == 2 and sys.argv[1] in ('-h', '--help'):
        print(__doc__)
        return
    if len(sys.argv) != 3:
        raise SystemExit(__doc__)
    source, destination = map(checked_path, sys.argv[1:])
    with open(source) as cues_file:
        cues = json.load(cues_file)["captions"]
    cues.sort()
    cues = [list(c) for c in cues]
    for i in range(len(cues) - 1):
        if cues[i][1] > cues[i + 1][0] - 0.03:
            cues[i][1] = max(cues[i][0] + 0.4, cues[i + 1][0] - 0.03)
    with open(destination, "w") as f:
        for i, (a, b, t) in enumerate(cues, 1):
            f.write(f"{i}\n{ts(a)} --> {ts(max(a + 0.8, b))}\n{t}\n\n")
    print(sys.argv[2], len(cues), "cues")


if __name__ == '__main__':
    main()
