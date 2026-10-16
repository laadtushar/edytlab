"""Where the burned-in captions change on the demo videos.

make-demo-videos.mjs (apps/desktop/e2e-native) burns each caption into the recording as a card at the bottom of the
frame. This differences the strip that card sits in, frame by frame, and lists every frame where it changes by more
than a threshold: the caption starts that spec.py's DEMOS tables hold. Run it after the demos are re-filmed, then check
each time on an extracted frame (sheet.py) before it goes into spec.py.

  python3 detect_captions.py [slug ...]        every demo by default (EDYTLAB_DEMO_DIR to read them from elsewhere)

The GTK file chooser that opens the tracks covers the strip twice, about 3.6 s and 6.7 s in: those two are not captions.
The recordings are 15 fps, so every start is a multiple of 1/15 s.
"""
import subprocess, sys, os
from pathlib import Path
# <repo>/website/public/demos (this file is <repo>/marketing/reels/tools/detect_captions.py); EDYTLAB_DEMO_DIR overrides it.
DEMOS = os.environ.get("EDYTLAB_DEMO_DIR") or str(Path(__file__).resolve().parents[3] / "website" / "public" / "demos")
SLUGS = ["dj-beatmatched-transition", "dj-extended-club-intro", "dj-mini-mix"]
FPS = 15
W, H = 160, 16                    # the strip, shrunk
STRIP = "crop=1000:70:140:684"    # the card's place in the 1280x776 frame: 1000 x 70 px, 684 px down, 140 in
THRESHOLD = 8000                  # summed absolute difference of two frames' strips; a caption change is 16000 and up, the app moving behind it about 3000


def changes(slug):
    p = subprocess.run(["nice", "-n", "10", "ffmpeg", "-nostdin", "-v", "error", "-i", f"{DEMOS}/{slug}.mp4", "-vf",
                        f"{STRIP},scale={W}:{H}:flags=area,format=gray,fps={FPS}", "-f", "rawvideo", "-"], capture_output=True)
    data, fs = p.stdout, W * H
    n = len(data) // fs
    diffs = [0] + [sum(abs(x - y) for x, y in zip(data[(i - 1) * fs:i * fs], data[i * fs:(i + 1) * fs])) for i in range(1, n)]
    found = []   # (frame, difference), one per change: the biggest within a second of the first
    for i, d in enumerate(diffs):
        if d > THRESHOLD:
            if found and i - found[-1][0] < FPS:
                if d > found[-1][1]:
                    found[-1] = (i, d)
            else:
                found.append((i, d))
    return n / FPS, found


if __name__ == "__main__":
    for slug in sys.argv[1:] or SLUGS:
        length, found = changes(slug)
        print(f"{slug}: {length:.3f} s, {len(found)} changes")
        for frame, d in found:
            print(f"  {frame / FPS:8.3f} s  (frame {frame}, difference {d})")
