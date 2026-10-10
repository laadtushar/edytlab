#!/usr/bin/env python3
"""sheet.py out.png cols width demo:t demo:t ...   -> labelled contact sheet of exact frames (accurate seek)."""
import subprocess, sys, io
from PIL import Image, ImageDraw, ImageFont
DEMOS = "/home/user/edytlab/website/public/demos"
FONT = "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf"
def frame(src, t):
    p = subprocess.run(["nice","-n","10","ffmpeg","-nostdin","-v","error","-ss",f"{t:.3f}","-i",src,"-frames:v","1","-f","image2pipe","-vcodec","png","-"],capture_output=True)
    return Image.open(io.BytesIO(p.stdout)).convert("RGB")
def main():
    out, cols, w = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
    items = sys.argv[4:]
    h = round(w * 776 / 1280)
    rows = (len(items) + cols - 1) // cols
    sheet = Image.new("RGB", (cols * w, rows * h), (0, 0, 0))
    d = ImageDraw.Draw(sheet); f = ImageFont.truetype(FONT, 18)
    for i, it in enumerate(items):
        demo, t = it.split(":"); t = float(t)
        im = frame(f"{DEMOS}/{demo}.mp4", t).resize((w, h), Image.LANCZOS)
        x, y = (i % cols) * w, (i // cols) * h
        sheet.paste(im, (x, y))
        d.rectangle([x, y, x + 150, y + 24], fill=(0, 0, 0)); d.text((x + 4, y + 2), f"{demo[3:6]} {t:.1f}", fill=(255, 255, 0), font=f)
    sheet.save(out)
main()
