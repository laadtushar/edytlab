#!/usr/bin/env python3
"""verify.py <outDir>: checks every finished file against the spec and prints the README's table.

Per video: H.264 High, yuv420p, 30 fps, AAC 48 kHz stereo; the frame size its platform wants; integrated loudness within
0.5 LU of -14 LUFS and a true peak under -1 dBTP, both measured on the finished AAC file with ebur128; an .srt and a
contact sheet beside it. The folder's videos together are under 80 MB. Exits non-zero if anything fails.
"""
import glob
import json
import os
import re
import subprocess
import sys

OUT = sys.argv[1]
FRAMES = {"9x16": (1080, 1920), "16x9": (1920, 1080), "1x1": (1080, 1080)}
problems = []


def probe(path):
    q = subprocess.run(["ffprobe", "-v", "error", "-show_entries", "stream=codec_name,profile,width,height,pix_fmt,r_frame_rate,sample_rate,channels,duration", "-of", "json", path],
                       capture_output=True, text=True, check=True).stdout
    return json.loads(q)["streams"]


def loudness(path):
    q = subprocess.run(["ffmpeg", "-nostdin", "-hide_banner", "-i", path, "-vn", "-af", "ebur128=peak=true", "-f", "null", "-"], capture_output=True, text=True).stderr
    return float(re.findall(r"\bI:\s+(-?[\d.]+) LUFS", q)[-1]), float(re.findall(r"Peak:\s+(-?[\d.]+) dBFS", q)[-1])


rows, total = [], 0
for path in sorted(glob.glob(f"{OUT}/edytlab-v0.7.0-*.mp4")):
    name = os.path.basename(path)
    size = os.path.getsize(path)
    total += size
    v, a = None, None
    for s in probe(path):
        if "width" in s:
            v = s
        else:
            a = s
    shape = name.rsplit("-", 1)[-1][:-4]
    checks = {
        "H.264": v["codec_name"] == "h264",
        "High": v["profile"] == "High",
        "yuv420p": v["pix_fmt"] == "yuv420p",
        "30 fps": v["r_frame_rate"] == "30/1",
        "AAC": a is not None and a["codec_name"] == "aac",
        "48 kHz": a is not None and a["sample_rate"] == "48000",
        "stereo": a is not None and a["channels"] == 2,
        "frame": (v["width"], v["height"]) == FRAMES.get(shape),
    }
    lufs, tp = loudness(path)
    checks["-14 LUFS +-0.5"] = abs(lufs + 14) <= 0.5
    checks["TP < -1 dBTP"] = tp < -1.0
    stem = path[:-4]
    checks[".srt"] = os.path.exists(stem + ".srt") and os.path.getsize(stem + ".srt") > 0
    checks["contact sheet"] = os.path.exists(stem + "-contact.png")
    bad = [k for k, ok in checks.items() if not ok]
    if bad:
        problems.append(f"{name}: failed {bad}")
    rows.append((name, f"{v['width']}x{v['height']}", float(v["duration"]), size / 1e6, lufs, tp))

print("| File | Frame | Length | Size | Loudness (final AAC) |\n|---|---|---|---|---|")
for n, fr, d, mb, lufs, tp in rows:
    print(f"| `{n}` | {fr} | {d:.1f} s | {mb:.1f} MB | {lufs:.1f} LUFS / {tp:.1f} dBTP |")
print(f"\n{len(rows)} videos, {total / 1e6:.1f} MB together")
if total >= 80e6:
    problems.append(f"the videos total {total / 1e6:.1f} MB, over 80 MB")
if problems:
    print("\nPROBLEMS:\n  " + "\n  ".join(problems))
    sys.exit(1)
print("all checks passed")
