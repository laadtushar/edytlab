"""Measure how well each narration line fits its shot.

Usage: fit.py [voice]
Inputs and outputs must stay in marketing/promo-v0.7.0. Copy external inputs,
including /tmp files, into that project before running the tool.
"""
import script, subprocess, os, sys
from pathlib import Path

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


voice = sys.argv[1] if len(sys.argv)>1 else 'daniel'
def trimmed(p, out):
    p, out = str(checked_path(p)), str(checked_path(out))
    subprocess.run(['ffmpeg','-y','-v','error','-i',p,'-af','silenceremove=start_periods=1:start_threshold=-45dB:start_silence=0.05,areverse,silenceremove=start_periods=1:start_threshold=-45dB:start_silence=0.05,areverse','-ar','48000','-ac','1',out],check=True)
    return float(subprocess.run(['ffprobe','-v','error','-show_entries','format=duration','-of','csv=p=0',out],capture_output=True,text=True).stdout)
os.makedirs(checked_path(f'trim/{voice}'),exist_ok=True)
for l in script.L:
    p=f'lines/{voice}/{l["id"]}.mp3'
    if not checked_path(p).exists(): continue
    d=trimmed(p,f'trim/{voice}/{l["id"]}.wav')
    lo=l['lo']+0.3; hi=l['hi']-0.3
    room=hi-max(l['at'],lo)
    need=d/room
    print(f"{l['id']:15s} speech {d:5.2f}  at {l['at']:6.2f} room {room:5.2f}  tempo needed {need:4.2f}  chars/s {len(l['text'])/d:4.1f}" + ('  <-- RETAKE/SHORTEN' if need>1.10 else ('  (atempo)' if need>1.0 else '')))
