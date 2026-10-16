#!/usr/bin/env python3
"""verify.py  -> prints the README index rows (resolution, codecs, duration, size, loudness) of the three plain cuts, and checks
that the app's own sound sits where the Listen shots put it: loud inside its window on the cut's clock, quiet in the 3 s before.

Needs work/cues_<key>.json (assemble.sh writes them). The audible windows are the reels' (marketing/reels/tools/spec.py,
measured on the re-filmed demos with silencedetect)."""
import json, os, re, subprocess, sys
HERE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(os.path.dirname(os.path.dirname(HERE)), "marketing", "reels", "tools"))
os.environ.setdefault("EDYTLAB_REEL_ROOT", os.path.join(HERE, "work"))
import spec  # noqa: E402

OUT = f"{HERE}/out"
CODE = {"dj-beatmatched-transition": "BT", "dj-extended-club-intro": "EI", "dj-mini-mix": "MM"}


def probe(p):
    j = json.loads(subprocess.run(["ffprobe", "-v", "error", "-show_entries", "stream=codec_name,width,height,r_frame_rate,pix_fmt,sample_rate,channels,profile:format=duration,size", "-of", "json", p], capture_output=True, text=True).stdout)
    v = [s for s in j["streams"] if s.get("width")][0]
    a = [s for s in j["streams"] if s.get("sample_rate")][0]
    q = subprocess.run(["nice", "-n", "10", "ffmpeg", "-nostdin", "-hide_banner", "-i", p, "-af", "ebur128=peak=true", "-f", "null", "-"], capture_output=True, text=True).stderr
    lufs = float(re.findall(r"\bI:\s+(-?[\d.]+) LUFS", q)[-1]); tp = float(re.findall(r"Peak:\s+(-?[\d.]+) dBFS", q)[-1])
    return dict(dur=float(j["format"]["duration"]), size=int(j["format"]["size"]), w=v["width"], h=v["height"], fps=v["r_frame_rate"], pix=v["pix_fmt"], vc=v["codec_name"], prof=v.get("profile"),
                ac=a["codec_name"], sr=a["sample_rate"], ch=a["channels"], lufs=lufs, tp=tp)


def mean_volume(path, ss, d):
    q = subprocess.run(["nice", "-n", "10", "ffmpeg", "-nostdin", "-hide_banner", "-ss", f"{ss:.2f}", "-t", f"{d:.2f}", "-i", path, "-af", "volumedetect", "-f", "null", "-"], capture_output=True, text=True).stderr
    return float(re.findall(r"mean_volume: (-?[\d.]+) dB", q)[-1])


for key, name in (("li", "linkedin"), ("rd", "reddit"), ("ig", "instagram")):
    cues = json.load(open(f"{HERE}/work/cues_{key}.json"))
    f = f"{OUT}/launch-{name}.mp4"
    r = probe(f)
    print(f"| `launch-{name}.mp4` | {r['w']}x{r['h']} {r['fps']} fps {r['vc']} {r['prof']} {r['pix']}; {r['ac']} {r['sr']} Hz {r['ch']} ch | {r['dur']:.1f} s | {r['size']/1e6:.1f} MB | {r['lufs']:.1f} LUFS / {r['tp']:.1f} dBTP |")
    # where the app's sound is on this cut's clock
    windows = []
    for c in cues["clips"]:
        for a, b in spec.DEMOS[CODE[c["demo"]]]["audio"].values():
            lo, hi = max(a, c["src0"]), min(b, c["src0"] + c["dur"])
            if hi > lo:
                windows.append((c["t0"] + lo - c["src0"], c["t0"] + hi - c["src0"]))
    for a, b in windows:
        inside, before = mean_volume(f, a + 0.3, min(b - a - 0.6, 4.0)), mean_volume(f, max(0, a - 3.5), 3.0)
        print(f"<!-- {key} real audio window {a:.1f}-{b:.1f}s: mean {inside:.1f} dB inside, {before:.1f} dB in the 3 s before: {'OK' if inside > -25 and before < inside - 15 else 'CHECK'} -->")
    print(f"<!-- {key}: app-audio windows {[(round(a,1), round(b,1)) for a,b in windows]} -->")
