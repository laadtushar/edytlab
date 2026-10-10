#!/usr/bin/env python3
"""verify.py  -> prints the README index rows (duration, resolution, size, loudness) and checks that no narration line overlaps the real app audio."""
import json, os, re, subprocess, sys
HERE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = f"{HERE}/out"
AUDIBLE = {"dj-beatmatched-transition": [(92.30, 108.90), (171.31, 181.95)], "dj-extended-club-intro": [(14.15, 20.75), (86.03, 93.65), (99.51, 110.10)],
           "dj-mini-mix": [(91.75, 103.39), (109.29, 120.88)]}

def probe(p):
    j = json.loads(subprocess.run(["ffprobe", "-v", "error", "-show_entries", "stream=codec_name,width,height,r_frame_rate,pix_fmt,sample_rate,channels,profile:format=duration,size", "-of", "json", p], capture_output=True, text=True).stdout)
    v = [s for s in j["streams"] if s.get("width")][0]
    a = [s for s in j["streams"] if s.get("sample_rate")][0]
    q = subprocess.run(["nice", "-n", "10", "ffmpeg", "-nostdin", "-hide_banner", "-i", p, "-af", "ebur128=peak=true", "-f", "null", "-"], capture_output=True, text=True).stderr
    lufs = float(re.findall(r"\bI:\s+(-?[\d.]+) LUFS", q)[-1]); tp = float(re.findall(r"Peak:\s+(-?[\d.]+) dBFS", q)[-1])
    return dict(dur=float(j["format"]["duration"]), size=int(j["format"]["size"]), w=v["width"], h=v["height"], fps=v["r_frame_rate"], pix=v["pix_fmt"], vc=v["codec_name"], prof=v.get("profile"),
                ac=a["codec_name"], sr=a["sample_rate"], ch=a["channels"], lufs=lufs, tp=tp)

rows = []
for key, name in (("li", "linkedin"), ("rd", "reddit"), ("ig", "instagram")):
    cues = json.load(open(f"{HERE}/work/cues_{key}.json"))
    for suffix in ("", "-narrated"):
        f = f"{OUT}/launch-{name}{suffix}.mp4"
        if not os.path.exists(f):
            continue
        r = probe(f)
        print(f"| `launch-{name}{suffix}.mp4` | {r['w']}x{r['h']} {r['fps']} fps {r['vc']} {r['prof']} {r['pix']}; {r['ac']} {r['sr']} Hz {r['ch']} ch | {r['dur']:.1f} s | {r['size']/1e6:.1f} MB | {r['lufs']:.1f} LUFS / {r['tp']:.1f} dBTP |")
    nj = f"{HERE}/out/narration/{key}/narr_{key}.json"
    if os.path.exists(nj):
        windows = []
        for c in cues["clips"]:
            for a, b in AUDIBLE.get(c["demo"], []):
                lo, hi = max(a, c["src0"]), min(b, c["src0"] + c["dur"])
                if hi > lo:
                    windows.append((c["t0"] + lo - c["src0"], c["t0"] + hi - c["src0"]))
        bad = []
        for n in json.load(open(nj)):
            s0, s1 = n["t"], n["t"] + n["speech_end"] + 0.05
            for a, b in windows:
                if s0 < b and s1 > a:
                    bad.append((n["id"], round(s0, 2), round(s1, 2), (round(a, 2), round(b, 2))))
        # the real app audio must sit inside its window: loud there, quiet just before it (un-narrated files)
        for a, b in windows:
            f0 = f"{OUT}/launch-{name}.mp4"
            def mv(ss, d):
                q = subprocess.run(["nice", "-n", "10", "ffmpeg", "-nostdin", "-hide_banner", "-ss", f"{ss:.2f}", "-t", f"{d:.2f}", "-i", f0, "-af", "volumedetect", "-f", "null", "-"], capture_output=True, text=True).stderr
                return float(re.findall(r"mean_volume: (-?[\d.]+) dB", q)[-1])
            inside, before = mv(a + 0.3, min(b - a - 0.6, 4.0)), mv(max(0, a - 3.5), 3.0)
            print(f"<!-- {key} real audio window {a:.1f}-{b:.1f}s: mean {inside:.1f} dB inside, {before:.1f} dB in the 3 s before: {'OK' if inside > -25 and before < inside - 15 else 'CHECK'} -->")
        print(f"<!-- {key}: app-audio windows {[(round(a,1), round(b,1)) for a,b in windows]}; narration overlapping them: {bad or 'none'} -->")
