#!/usr/bin/env python3
"""Assemble a cut's audio from its cue file and mux it onto the silent video.

  audio.py mix  cues.json video.mp4 out.mp4 [--narration narr.json] [--sfx-dir DIR] [--no-sfx]

Sources, all placed on the cut's own clock:
  * the real app audio of every footage clip (the demo recordings' own sound; only the Listen moments are audible)
  * synthesised UI sounds (tools/sfx.py): whoosh on scene changes, tick when a callout lands, chime under the wordmark
  * optionally narration clips: narr.json = [{"t": seconds, "file": "x.mp3", "gain_db": 0, "tempo": 1.0}, ...]

Each source is decoded to 48 kHz stereo PCM by ffmpeg (with its own fades and gain), then summed at its exact sample offset with the
standard library's audioop (an earlier version used ffmpeg's amix + adelay and the mix landed early when streams ended; this cannot).
Then gain + a look-ahead limiter are iterated until the file measures -14 LUFS with a true peak under -1 dBTP; AAC 48 kHz stereo;
the video stream is copied.
"""
import audioop
import json
import os
import re
import subprocess
import sys
import wave

HERE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
# <repo>/website/public/demos (this file is <repo>/marketing/launch-video/tools/audio.py); EDYTLAB_DEMO_DIR overrides it.
DEMO_DIR = os.environ.get("EDYTLAB_DEMO_DIR") or os.path.join(os.path.dirname(os.path.dirname(HERE)), "website", "public", "demos")
SR, FRAME = 48000, 4          # 16-bit stereo
FMT = "aformat=sample_rates=48000:channel_layouts=stereo"


def run(cmd, **kw):
    p = subprocess.run(["nice", "-n", "10"] + cmd, capture_output=True, text=True, **kw)
    if p.returncode:
        raise SystemExit("ffmpeg failed:\n" + p.stderr[-3000:])
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


def mix_pcm(cues, narration, sfx_dir, use_sfx, music_db, sfx_db_shift):
    total = cues["total"]
    buf = bytearray(int(round(total * SR)) * FRAME)
    for c in cues["clips"]:
        fo = min(0.3, c.get("fade_out", 0.3))
        filt = f"{FMT},afade=t=in:d=0.03,afade=t=out:st={max(0.0, c['dur'] - fo):.3f}:d={fo:.3f},volume={music_db}dB"
        place(buf, decode(["-ss", f"{c['src0']:.3f}", "-t", f"{c['dur']:.3f}", "-i", f"{DEMO_DIR}/{c['demo']}.mp4"], filt), c["t0"])
    if use_sfx:
        cache = {}
        for t, name, db in cues["sfx"]:
            key = (name, db)
            if key not in cache:
                cache[key] = decode(["-i", f"{sfx_dir}/{name}.wav"], f"{FMT},volume={db + sfx_db_shift}dB")
            place(buf, cache[key], t)
    for item in narration:
        tempo = f"atempo={item['tempo']}," if item.get("tempo", 1.0) != 1.0 else ""
        place(buf, decode(["-i", item["file"]], f"{tempo}{FMT},volume={item.get('gain_db', 0) + 3}dB"), item["t"])
    return bytes(buf)


def measure(path):
    q = run(["ffmpeg", "-nostdin", "-hide_banner", "-i", path, "-af", "ebur128=peak=true", "-f", "null", "-"]).stderr
    i = float(re.findall(r"\bI:\s+(-?[\d.]+) LUFS", q)[-1])
    tp = float(re.findall(r"Peak:\s+(-?[\d.]+) dBFS", q)[-1])
    return i, tp


def main():
    a = sys.argv[1:]
    assert a[0] == "mix"
    cues = json.load(open(a[1]))
    video, out = a[2], a[3]
    narration = json.load(open(a[a.index("--narration") + 1])) if "--narration" in a else []
    sfx_dir = a[a.index("--sfx-dir") + 1] if "--sfx-dir" in a else f"{HERE}/work/sfx"
    use_sfx = "--no-sfx" not in a
    work = f"{HERE}/work/audio"
    os.makedirs(work, exist_ok=True)
    tag = os.path.splitext(os.path.basename(out))[0]
    # the UI sounds sit under the music; with a voice the real playback is 3 dB lower and the voice 3 dB higher so they balance
    pcm = mix_pcm(cues, narration, sfx_dir, use_sfx, music_db=-6 if narration else -3, sfx_db_shift=-4 if narration else -2)
    mixed = f"{work}/{tag}_mix.wav"
    with wave.open(mixed, "wb") as w:
        w.setnchannels(2); w.setsampwidth(2); w.setframerate(SR); w.writeframes(pcm)
    # Gain + a look-ahead limiter, iterated until -14 LUFS with a true peak under -1 dBTP.
    normed = f"{work}/{tag}_norm.wav"
    i_in, _ = measure(mixed)
    g, limit = -14.0 - i_in + 2.0, 0.78
    for _ in range(8):
        af = f"volume={g:.2f}dB,alimiter=limit={limit:.3f}:attack=3:release=60:level=false"
        run(["ffmpeg", "-nostdin", "-y", "-v", "error", "-i", mixed, "-af", af, "-ar", "48000", "-c:a", "pcm_s16le", normed])
        got, tp = measure(normed)
        if abs(got + 14.0) <= 0.15 and tp <= -1.25:
            break
        if tp > -1.25:
            limit *= 0.97
        g += (-14.0 - got) * 0.9
    print(f"  loudness: {got:.2f} LUFS, true peak {tp:.2f} dBTP (gain {g:.1f} dB, limiter {limit:.3f})")
    run(["ffmpeg", "-nostdin", "-y", "-v", "error", "-i", video, "-i", normed, "-map", "0:v:0", "-map", "1:a:0", "-c:v", "copy", "-c:a", "aac",
         "-b:a", "192k", "-ar", "48000", "-ac", "2", "-shortest", "-movflags", "+faststart", out])
    print(out, "mix loudness before normalising", f"{i_in:.2f} LUFS")


if __name__ == "__main__":
    main()
