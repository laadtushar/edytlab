#!/usr/bin/env python3
"""sheet.py video.mp4 out.png [frames]  ->  a labelled contact sheet of exact frames (default 20, evenly spaced).

Tile width follows the frame: 9:16 gets 5 columns of 216 px, 16:9 gets 4 of 480 px, 1:1 gets 5 of 300 px. Pillow is needed.
"""
import io
import subprocess
import sys

from PIL import Image, ImageDraw, ImageFont

video, out = sys.argv[1], sys.argv[2]
n = int(sys.argv[3]) if len(sys.argv) > 3 else 20
p = subprocess.run(["ffprobe", "-v", "error", "-show_entries", "stream=width,height,duration", "-select_streams", "v:0", "-of", "csv=p=0", video],
                   capture_output=True, text=True, check=True)
W, H, dur = p.stdout.strip().split(",")[:3]
W, H, dur = int(W), int(H), float(dur)
if W < H:
    cols, tw = 5, 216
elif W > H:
    cols, tw = 4, 480
else:
    cols, tw = 5, 300
th = round(tw * H / W)
ts = [dur * (i + 0.5) / n for i in range(n)]
rows = (n + cols - 1) // cols
sheet = Image.new("RGB", (cols * tw, rows * th), (9, 9, 11))
d = ImageDraw.Draw(sheet)
f = ImageFont.truetype("/usr/share/fonts/opentype/inter/Inter-SemiBold.otf", max(11, tw // 22))
for i, t in enumerate(ts):
    r = subprocess.run(["ffmpeg", "-nostdin", "-v", "error", "-ss", f"{t:.3f}", "-i", video, "-frames:v", "1", "-f", "image2pipe", "-vcodec", "png", "-"],
                       capture_output=True, check=True)
    im = Image.open(io.BytesIO(r.stdout)).convert("RGB").resize((tw, th), Image.LANCZOS)
    x, y = (i % cols) * tw, (i // cols) * th
    sheet.paste(im, (x, y))
    label = f"{t:.1f}s"
    box = d.textbbox((0, 0), label, font=f)
    # the time sits in the bottom-right corner, where the 9:16 frames have no words (and the others have the progress line)
    x1, y1 = x + tw, y + th
    d.rectangle([x1 - box[2] - 12, y1 - box[3] - 10, x1, y1], fill=(9, 9, 11))
    d.text((x1 - box[2] - 6, y1 - box[3] - 6), label, fill=(191, 131, 252), font=f)
sheet.save(out)
print(out, sheet.size, f"{n} frames")
