"""Measure how well each narration line fits its shot.

Usage: fit.py [voice]
Inputs and outputs must stay in marketing/promo-v0.7.0. Copy external inputs,
including /tmp files, into that project before running the tool.
"""
import os
from pathlib import Path
import subprocess
import sys

import script

if len(sys.argv) == 2 and sys.argv[1] in ('-h', '--help'):
    print(__doc__)
    raise SystemExit(0)

ROOT = Path(__file__).resolve().parents[2]


def checked_path(path):
    """Resolve a path and refuse escapes from this promo project."""
    resolved = Path(path).resolve()
    if resolved != ROOT and ROOT not in resolved.parents:
        raise SystemExit(f"Refused path {str(path)!r}: must resolve inside {ROOT}")
    return resolved


def trimmed(source, out):
    """Trim silence into an in-project WAV and return its probed duration."""
    source, out = str(checked_path(source)), str(checked_path(out))
    silence = ('silenceremove=start_periods=1:start_threshold=-45dB:start_silence=0.05,'
               'areverse,silenceremove=start_periods=1:start_threshold=-45dB:'
               'start_silence=0.05,areverse')
    subprocess.run(['ffmpeg', '-y', '-v', 'error', '-i', source, '-af', silence,
                    '-ar', '48000', '-ac', '1', out], check=True)
    result = subprocess.run(['ffprobe', '-v', 'error', '-show_entries', 'format=duration',
                             '-of', 'csv=p=0', out], capture_output=True, text=True, check=False)
    return float(result.stdout)


voice = sys.argv[1] if len(sys.argv) > 1 else 'daniel'
os.makedirs(checked_path(f'trim/{voice}'), exist_ok=True)
for line in script.L:
    source_path = f'lines/{voice}/{line["id"]}.mp3'
    if not checked_path(source_path).exists():
        continue
    duration = trimmed(source_path, f'trim/{voice}/{line["id"]}.wav')
    lo = line['lo'] + 0.3
    hi = line['hi'] - 0.3
    room = hi - max(line['at'], lo)
    need = duration / room
    print(f"{line['id']:15s} speech {duration:5.2f}  at {line['at']:6.2f} room {room:5.2f}  "
          f"tempo needed {need:4.2f}  chars/s {len(line['text'])/duration:4.1f}" +
          ('  <-- RETAKE/SHORTEN' if need > 1.10 else ('  (atempo)' if need > 1.0 else '')))
