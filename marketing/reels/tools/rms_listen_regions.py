import subprocess, sys, re, struct, os
from pathlib import Path
from demo_paths import checked_demo_input
# <repo>/website/public/demos (this file is <repo>/marketing/reels/tools/rms_listen_regions.py); EDYTLAB_DEMO_DIR overrides it.
DEMOS = os.environ.get("EDYTLAB_DEMO_DIR") or str(Path(__file__).resolve().parents[3] / "website" / "public" / "demos")
def rms(demo, a, b, step=0.5):
    p = subprocess.run(
        ["nice", "-n", "10", "ffmpeg", "-nostdin", "-v", "error", "-ss", str(a), "-t", str(b - a),
         "-i", checked_demo_input(f"{DEMOS}/{demo}.mp4"), "-vn", "-ac", "1", "-ar", "16000", "-f", "s16le", "-"],
        capture_output=True, check=False)
    if p.returncode:
        raise SystemExit(f"ffmpeg failed on {demo}:\n" + p.stderr.decode(errors="replace")[-2000:])
    d = p.stdout; n = len(d)//2
    s = struct.unpack(f"<{n}h", d)
    w = int(16000*step); out = []
    import math
    for i in range(0, n-w+1, w):
        seg = s[i:i+w]; e = (sum(x*x for x in seg)/w) ** 0.5
        out.append(20*math.log10(max(e,1)/32768))
    return out
regions = [("dj-beatmatched-transition",85,104),("dj-beatmatched-transition",147,161),("dj-extended-club-intro",84,94),("dj-extended-club-intro",97,110.5),("dj-mini-mix",69,83),("dj-mini-mix",86.5,100.5)]
for demo, _, _ in regions:
    checked_demo_input(f"{DEMOS}/{demo}.mp4")
for demo,a,b in regions:
    r = rms(demo,a,b)
    print(demo, a, b, "RMS dB per 0.5s:")
    print("  " + " ".join(f"{v:.0f}" for v in r))
