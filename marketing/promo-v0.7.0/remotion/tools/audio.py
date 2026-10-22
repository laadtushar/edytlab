#!/usr/bin/env python3
"""Put the sound on a silent render, and master it.

  audio.py <cues.json> <silent.mp4> <out.mp4> [--work DIR] [--crf N]

Sound is only (a) the app's own audio: the S1 and S7 recordings' .wav files (../../footage), laid at the moments of the
footage they belong to, and (b) UI sounds synthesised in code: marketing/launch-video/tools/sfx.py (a whoosh on a scene
change, a tick when a callout lands, a chime under the wordmark). No music from anywhere else, no voice.

Every source is decoded to 48 kHz stereo 16-bit by ffmpeg and summed at its exact sample offset (as marketing/launch-video/
tools/audio.py does: ffmpeg's amix + adelay put sound early when a stream ended). Then ffmpeg's `loudnorm` runs in two
passes (measure, then apply the measured values in linear mode) to -14 LUFS integrated with a true peak under -1 dBTP,
the picture (Remotion's near-lossless render) is encoded once as H.264 High, yuv420p, 30 fps and muxed with AAC 48 kHz
stereo, and the FINISHED AAC file is measured with ebur128: the encoder adds
overshoot to the peaks, so if the file is over -1.0 dBTP the target is lowered and the file made again.

Needs Python 3.12 or older (audioop) and ffmpeg.
"""
import audioop
import json
import os
import re
import subprocess
import sys
import wave

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.abspath(os.path.join(HERE, "..", "..", "..", ".."))
FOOTAGE = os.path.join(REPO, "marketing", "promo-v0.7.0", "footage")
SFX_PY = os.path.join(REPO, "marketing", "launch-video", "tools", "sfx.py")
SR, FRAME = 48000, 4  # 16-bit stereo
FMT = "aformat=sample_rates=48000:channel_layouts=stereo"
TARGET_I = -14.0


def run(cmd, **kw):
    p = subprocess.run(["nice", "-n", "10"] + cmd, capture_output=True, text=True, **kw)
    if p.returncode:
        raise SystemExit("command failed: " + " ".join(cmd[:6]) + "\n" + p.stderr[-3000:])
    return p


def decode(in_args, filt):
    cmd = ["nice", "-n", "10", "ffmpeg", "-nostdin", "-v", "error"] + in_args + ["-vn", "-af", filt, "-f", "s16le", "-ar", str(SR), "-ac", "2", "-"]
    p = subprocess.run(cmd, capture_output=True)
    if p.returncode:
        raise SystemExit("decode failed: " + p.stderr.decode()[-2000:])
    return p.stdout


def place(buf, frag, t):
    off = int(round(t * SR)) * FRAME
    if off >= len(buf):
        return
    frag = frag[: len(buf) - off]
    frag = frag[: len(frag) // FRAME * FRAME]
    buf[off:off + len(frag)] = audioop.add(bytes(buf[off:off + len(frag)]), frag, 2)


def mix(cues, sfx_dir):
    buf = bytearray(int(round(cues["total"] * SR)) * FRAME)
    for c in cues["clips"]:
        # the app's own sound, 3 dB down, with 20 ms fades so a cut never clicks
        filt = f"{FMT},afade=t=in:d=0.02,afade=t=out:st={max(0.0, c['dur'] - 0.02):.3f}:d=0.02,volume=-3dB"
        place(buf, decode(["-ss", f"{c['src0']:.3f}", "-t", f"{c['dur']:.3f}", "-i", os.path.join(FOOTAGE, c["wav"])], filt), c["t0"])
    cache = {}
    for t, name, db, *rest in cues["sfx"]:
        pitch = rest[0] if rest and rest[0] else 1
        key = (name, db, pitch)
        if key not in cache:
            # a different pitch is the same sound played faster or slower (the ticks are 0.12 s long, so the change in length is nothing)
            shift = f"asetrate={round(SR * pitch)},aresample={SR}," if pitch != 1 else ""
            cache[key] = decode(["-i", f"{sfx_dir}/{name}.wav"], f"{shift}{FMT},volume={db}dB")
        place(buf, cache[key], t)
    return bytes(buf)


def measure(path):
    """Integrated loudness (LUFS) and true peak (dBTP) of a file, with ebur128."""
    q = run(["ffmpeg", "-nostdin", "-hide_banner", "-i", path, "-vn", "-af", "ebur128=peak=true", "-f", "null", "-"]).stderr
    i = float(re.findall(r"\bI:\s+(-?[\d.]+) LUFS", q)[-1])
    tp = float(re.findall(r"Peak:\s+(-?[\d.]+) dBFS", q)[-1])
    return i, tp


def loudnorm(src, dst, tp):
    """Two passes: measure with loudnorm, then apply the measured values (linear gain where the peak allows it)."""
    f = f"loudnorm=I={TARGET_I}:TP={tp}:LRA=11"
    q = run(["ffmpeg", "-nostdin", "-hide_banner", "-i", src, "-af", f"{f}:print_format=json", "-f", "null", "-"]).stderr
    m = json.loads(q[q.rindex("{"):q.rindex("}") + 1])
    second = (f"{f}:measured_I={m['input_i']}:measured_TP={m['input_tp']}:measured_LRA={m['input_lra']}:measured_thresh={m['input_thresh']}"
              f":offset={m['target_offset']}:linear=true:print_format=summary")
    run(["ffmpeg", "-nostdin", "-y", "-v", "error", "-i", src, "-af", f"{second},aresample=48000", "-c:a", "pcm_s16le", dst])
    return m


def main():
    a = sys.argv[1:]
    cues = json.load(open(a[0]))
    silent, out = a[1], a[2]
    work = a[a.index("--work") + 1] if "--work" in a else os.path.join(HERE, "..", "work")
    os.makedirs(work, exist_ok=True)
    sfx_dir = os.path.join(work, "sfx")
    if not os.path.exists(f"{sfx_dir}/chime.wav"):
        os.makedirs(sfx_dir, exist_ok=True)
        run([sys.executable, SFX_PY, sfx_dir])
    crf = a[a.index("--crf") + 1] if "--crf" in a else "20"
    tag = cues["id"]
    video = f"{work}/{tag}_video.mp4"
    run(["ffmpeg", "-nostdin", "-y", "-v", "error", "-i", silent, "-an", "-vf", "format=yuv420p", "-r", "30", "-c:v", "libx264", "-profile:v", "high",
         "-preset", "slow", "-crf", crf, "-g", "60", "-colorspace", "bt709", "-color_primaries", "bt709", "-color_trc", "bt709", video])
    mixed = f"{work}/{tag}_mix.wav"
    with wave.open(mixed, "wb") as w:
        w.setnchannels(2); w.setsampwidth(2); w.setframerate(SR); w.writeframes(mix(cues, sfx_dir))
    i_in, tp_in = measure(mixed)
    print(f"  mix before mastering: {i_in:.2f} LUFS, true peak {tp_in:.2f} dBTP")
    tp = -1.5
    for attempt in range(5):
        normed = f"{work}/{tag}_norm.wav"
        loudnorm(mixed, normed, tp)
        got, got_tp = measure(normed)
        run(["ffmpeg", "-nostdin", "-y", "-v", "error", "-i", video, "-i", normed, "-map", "0:v:0", "-map", "1:a:0", "-c:v", "copy",
             "-c:a", "aac", "-b:a", "192k", "-ar", "48000", "-ac", "2", "-shortest", "-movflags", "+faststart", out])
        f_i, f_tp = measure(out)
        print(f"  attempt {attempt + 1}: target TP {tp}: wav {got:.2f} LUFS / {got_tp:.2f} dBTP; final AAC {f_i:.2f} LUFS / {f_tp:.2f} dBTP")
        if f_tp <= -1.05 and abs(f_i - TARGET_I) <= 0.6:
            break
        if f_tp > -1.05:
            tp -= 0.5
    else:
        raise SystemExit(f"{out}: could not reach -14 LUFS / -1 dBTP (last: {f_i:.2f} LUFS, {f_tp:.2f} dBTP)")
    json.dump({"id": tag, "lufs": round(f_i, 2), "true_peak_dbtp": round(f_tp, 2)}, open(f"{work}/{tag}_loudness.json", "w"))
    print(f"  {out}: {f_i:.2f} LUFS integrated, true peak {f_tp:.2f} dBTP")


if __name__ == "__main__":
    main()
