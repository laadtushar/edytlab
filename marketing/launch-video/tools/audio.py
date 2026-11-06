#!/usr/bin/env python3
"""Assemble a cut's audio from its cue file and mux it onto the silent video.
CLI data inputs and outputs stay in marketing/launch-video, except the three
read-only recordings in website/public/demos; copy other external data there first.
  audio.py mix  cues.json video.mp4 out.mp4 [--narration narr.json] [--sfx-dir DIR] [--no-sfx]

Sources, all placed on the cut's own clock:
  * real app audio from footage (only the Listen moments are audible)
  * synthesised sounds: whoosh on scene changes, tick on callouts, chime under the wordmark
  * optional narration: [{"t": seconds, "file": "x.mp3", "gain_db": 0, "tempo": 1.0}, ...]

Sources decode to 48 kHz stereo PCM with fades and gain, then audioop sums their exact offsets.
This avoids the earlier amix/adelay timing shift when streams ended. Gain and a look-ahead
limiter iterate until -14 LUFS with a true peak below -1 dBTP. AAC uses 48 kHz stereo;
the video stream is copied. Local file: URLs retain FFmpeg's one-prefix filename semantics;
other protocols and virtual streams are refused.
"""
import audioop  # skipcq: PYL-W0402 -- the mixer is built on audioop (docstring above)
import json
import os
import re
import subprocess
import sys
import wave
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import launch_paths  # noqa: E402
from launch_paths import checked_path  # noqa: E402

HERE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
# The three repository recordings or files inside the launch project may be used.
DEMO_DIR = os.environ.get("EDYTLAB_DEMO_DIR") or os.path.join(os.path.dirname(os.path.dirname(HERE)), "website", "public", "demos")
SR, FRAME = 48000, 4          # 16-bit stereo
FMT = "aformat=sample_rates=48000:channel_layouts=stereo"


def run(cmd, **kw):
    """Run one ffmpeg command and surface its bounded diagnostic tail."""
    p = subprocess.run(["nice", "-n", "10"] + cmd, capture_output=True, text=True, **kw)
    if p.returncode:
        raise SystemExit("ffmpeg failed:\n" + p.stderr[-3000:])
    return p


def decode(in_args, filt):
    """Decode the selected source and filter to stereo 48 kHz signed PCM."""
    cmd = ["nice", "-n", "10", "ffmpeg", "-nostdin", "-v", "error"] + in_args + ["-vn", "-af", filt, "-f", "s16le", "-ar", str(SR), "-ac", "2", "-"]
    p = subprocess.run(cmd, capture_output=True)
    if p.returncode:
        raise SystemExit("decode failed: " + p.stderr.decode()[-2000:])
    return p.stdout


def place(buf, frag, t):
    """Sum one PCM fragment at its exact offset within the cut buffer."""
    off = int(round(t * SR)) * FRAME
    if off >= len(buf):
        return
    frag = frag[: len(buf) - off]
    frag = frag[: len(frag) // FRAME * FRAME]
    buf[off:off + len(frag)] = audioop.add(bytes(buf[off:off + len(frag)]), frag, 2)


def _checked_media_path(path):
    """Check FFmpeg's actual local filename while retaining its argument spelling."""
    raw = os.fspath(path)
    native_drive = os.name == "nt" and bool(os.path.splitdrive(raw)[0])
    protocol = re.match(r"^[a-zA-Z0-9+.-]+:", raw)
    subfile_options = raw.startswith("subfile,") and ":" in raw[8:]
    if raw == "-" or (not native_drive and not raw.startswith("file:")
                      and (protocol or subfile_options)):
        raise SystemExit(
            f"Refused path {raw!r}: must name a local file inside {launch_paths.ROOT}"
        )
    # FFmpeg's file_open strips exactly one file: prefix, then opens that filename.
    actual = raw[5:] if raw.startswith("file:") else raw
    try:
        checked_path(actual)
    except SystemExit as error:
        raise SystemExit(f"Refused media path {raw!r}: {error}") from None
    return raw


def _sfx_file(directory, name):
    """Validate a sound's actual file while retaining its original path spelling."""
    path = f"{directory}/{name}.wav"
    _checked_media_path(path)
    return path


def mix_pcm(cues, narration, sfx_dir, use_sfx, music_db, sfx_db_shift):
    """Mix app audio, synthetic sounds and narration on the cut's sample clock."""
    _checked_media_path(sfx_dir)
    for c in cues['clips']:
        launch_paths.checked_demo_path(f"{DEMO_DIR}/{c['demo']}.mp4")
    total = cues["total"]
    buf = bytearray(int(round(total * SR)) * FRAME)
    for c in cues["clips"]:
        fo = min(0.3, c.get("fade_out", 0.3))
        filt = f"{FMT},afade=t=in:d=0.03,afade=t=out:st={max(0.0, c['dur'] - fo):.3f}:d={fo:.3f},volume={music_db}dB"
        source = launch_paths.checked_demo_path(f"{DEMO_DIR}/{c['demo']}.mp4")
        place(buf, decode(["-ss", f"{c['src0']:.3f}", "-t", f"{c['dur']:.3f}", "-i", source], filt), c["t0"])
    if use_sfx:
        cache = {}
        for t, name, db in cues["sfx"]:
            key = (name, db)
            if key not in cache:
                cache[key] = decode(
                    ["-i", _sfx_file(sfx_dir, name)], f"{FMT},volume={db + sfx_db_shift}dB",
                )
            place(buf, cache[key], t)
    for item in narration:
        _checked_media_path(item["file"])
        tempo = f"atempo={item['tempo']}," if item.get("tempo", 1.0) != 1.0 else ""
        place(buf, decode(
            ["-i", item["file"]], f"{tempo}{FMT},volume={item.get('gain_db', 0) + 3}dB",
        ), item["t"])
    return bytes(buf)


def measure(path):
    """Read one local media file's integrated loudness and true peak."""
    _checked_media_path(path)
    q = run(["ffmpeg", "-nostdin", "-hide_banner", "-i", path, "-af", "ebur128=peak=true", "-f", "null", "-"]).stderr
    i = float(re.findall(r"\bI:\s+(-?[\d.]+) LUFS", q)[-1])
    tp = float(re.findall(r"Peak:\s+(-?[\d.]+) dBFS", q)[-1])
    return i, tp


def _mix_inputs(a):
    """Read manifests and preflight ordinary media before creating mix outputs."""
    checked_path(a[1])
    with open(a[1]) as cues_file:  # skipcq: PTC-W6004
        cues = json.load(cues_file)
    video, out = a[2], a[3]
    _checked_media_path(video)
    _checked_media_path(out)
    narration = []
    if "--narration" in a:
        manifest = a[a.index("--narration") + 1]
        checked_path(manifest)
        with open(manifest) as narration_file:
            narration = json.load(narration_file)
    sfx_dir = a[a.index("--sfx-dir") + 1] if "--sfx-dir" in a else f"{HERE}/work/sfx"
    use_sfx = "--no-sfx" not in a
    _checked_media_path(sfx_dir)
    for c in cues['clips']:
        launch_paths.checked_demo_path(f"{DEMO_DIR}/{c['demo']}.mp4")
    if use_sfx:
        for _, name, _ in cues["sfx"]:
            _sfx_file(sfx_dir, name)
    for item in narration:
        _checked_media_path(item["file"])
    return cues, video, out, narration, sfx_dir, use_sfx


def main():
    """Build, normalise and mux one cut after validating its local data paths."""
    a = sys.argv[1:]
    if a[0] != "mix":
        raise AssertionError
    cues, video, out, narration, sfx_dir, use_sfx = _mix_inputs(a)
    work = f"{HERE}/work/audio"
    tag = os.path.splitext(os.path.basename(out))[0]
    mixed = f"{work}/{tag}_mix.wav"
    normed = f"{work}/{tag}_norm.wav"
    # Validate every writer before mkdir, decoding or the first scratch write.
    for destination in (work, mixed, normed):
        checked_path(destination)
    os.makedirs(work, exist_ok=True)
    # UI sounds sit under the music. With a voice, playback is 3 dB lower and the voice
    # is 3 dB higher so they balance.
    pcm = mix_pcm(cues, narration, sfx_dir, use_sfx,
                  music_db=-6 if narration else -3, sfx_db_shift=-4 if narration else -2)
    with wave.open(mixed, "wb") as w:
        w.setnchannels(2)
        w.setsampwidth(2)
        w.setframerate(SR)
        w.writeframes(pcm)
    # Iterate gain and a look-ahead limiter to -14 LUFS and a peak below -1 dBTP.
    # Then measure AAC: encoder overshoot once raised a -1.7 dBTP source to -0.4 dBTP.
    # Lower the limiter and re-encode until the file itself stays below -1 dBTP.
    i_in, _ = measure(mixed)
    g, limit = -14.0 - i_in + 2.0, 0.78
    for _attempt in range(6):
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
        f_i, f_tp = measure(out)
        print(f"  the file: {f_i:.2f} LUFS, true peak {f_tp:.2f} dBTP")
        if f_tp <= -1.05 and abs(f_i + 14.0) <= 0.3:
            break
        limit *= 0.93
    print(out, "mix loudness before normalising", f"{i_in:.2f} LUFS")


if __name__ == "__main__":
    main()
