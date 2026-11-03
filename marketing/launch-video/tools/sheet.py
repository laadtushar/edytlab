#!/usr/bin/env python3
"""sheet.py video.mp4 out.png cols tile_w [t ...] -> labelled contact sheet.

Inputs and outputs must stay inside marketing/launch-video, including through
symlinks. Copy external inputs such as /tmp files into that folder first.
"""
import io, subprocess, sys
from pathlib import Path

if len(sys.argv) == 2 and sys.argv[1] in ("-h", "--help"):
    print(__doc__)
    raise SystemExit(0)

from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parent.parent


def checked_path(path):
    """Resolve a path and reject escapes from the launch-video project."""
    resolved = Path(path).resolve()
    if resolved != ROOT and ROOT not in resolved.parents:
        raise SystemExit(f"Refused path {path!r}: must resolve inside {ROOT}")
    return resolved


v, out, cols, tw = str(checked_path(sys.argv[1])), sys.argv[2], int(sys.argv[3]), int(sys.argv[4])
checked_out = checked_path(out)
ts = [float(x) for x in sys.argv[5:]]
p = subprocess.run(["ffprobe","-v","error","-show_entries","stream=width,height,duration","-select_streams","v:0","-of","csv=p=0",v],capture_output=True,text=True)
W, H, dur = p.stdout.strip().split(",")[:3]; W, H, dur = int(W), int(H), float(dur)
if not ts:
    n = 16
    ts = [dur * (i + 0.5) / n for i in range(n)]
th = round(tw * H / W)
rows = (len(ts) + cols - 1) // cols
sheet = Image.new("RGB", (cols * tw, rows * th), (0, 0, 0))
d = ImageDraw.Draw(sheet); f = ImageFont.truetype("/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf", max(14, tw // 28))
for i, t in enumerate(ts):
    r = subprocess.run(["nice","-n","10","ffmpeg","-nostdin","-v","error","-ss",f"{t:.3f}","-i",v,"-frames:v","1","-f","image2pipe","-vcodec","png","-"],capture_output=True)
    im = Image.open(io.BytesIO(r.stdout)).convert("RGB").resize((tw, th), Image.LANCZOS)
    x, y = (i % cols) * tw, (i // cols) * th
    sheet.paste(im, (x, y)); d.rectangle([x, y, x + tw // 6, y + max(18, tw // 22)], fill=(0, 0, 0)); d.text((x + 3, y + 2), f"{t:.1f}s", fill=(255, 255, 0), font=f)
sheet.save(checked_out)
print(out, sheet.size)
