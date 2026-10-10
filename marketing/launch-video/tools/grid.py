#!/usr/bin/env python3
"""grid.py out.png demo:t demo:t ...  -> 2x2 sheets of native frames with a 50 px coordinate grid (source pixels)."""
import io, os, subprocess, sys
from PIL import Image, ImageDraw, ImageFont
# <repo>/website/public/demos (this file is <repo>/marketing/launch-video/tools/grid.py); EDYTLAB_DEMO_DIR overrides it.
D = os.environ.get("EDYTLAB_DEMO_DIR") or os.path.join(os.path.dirname(os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))), "website", "public", "demos")
def frame(demo, t):
    p = subprocess.run(["nice","-n","10","ffmpeg","-nostdin","-v","error","-ss",f"{t:.3f}","-i",f"{D}/{demo}.mp4","-frames:v","1","-f","image2pipe","-vcodec","png","-"],capture_output=True)
    return Image.open(io.BytesIO(p.stdout)).convert("RGB")
out = sys.argv[1]; items = sys.argv[2:]
S = 1.0
W, H = 1280, 776
cols = 2
rows = (len(items)+1)//2
sheet = Image.new("RGB",(W*cols, H*rows))
f = ImageFont.truetype("/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf", 13)
for i,it in enumerate(items):
    demo,t = it.split(":"); t=float(t)
    demo = {"BT":"dj-beatmatched-transition","EI":"dj-extended-club-intro","MM":"dj-mini-mix"}[demo]
    im = frame(demo,t)
    d = ImageDraw.Draw(im)
    for x in range(0,W,50):
        d.line([(x,0),(x,H)], fill=(255,0,255) if x%100==0 else (90,0,90), width=1)
        if x%100==0: d.text((x+2,2), str(x), fill=(255,255,0), font=f)
    for y in range(0,H,50):
        d.line([(0,y),(W,y)], fill=(0,255,255) if y%100==0 else (0,90,90), width=1)
        if y%100==0: d.text((2,y+2), str(y), fill=(255,255,0), font=f)
    d.rectangle([W-150,H-40,W,H], fill=(0,0,0)); d.text((W-140,H-32), f"{it}", fill=(255,255,0), font=f)
    sheet.paste(im,((i%cols)*W,(i//cols)*H))
sheet.save(out)
