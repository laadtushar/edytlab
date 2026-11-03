#!/usr/bin/env python3
"""ElevenLabs narration for the launch cuts: one clip per line, a few takes each, pick the take that fits its window.

  narrate.py status                         -> prints credits used / limit (never the key)
  narrate.py gen  lines.json OUTDIR [N]     -> N takes (default 2) per line, with word timings; logs characters to OUTDIR/usage.log
  narrate.py pick lines.json OUTDIR OUT.json -> best take per line (fits its window, fewest odd pauses) as audio.py's narration json + .srt

Voice Roger (CwhRBWXzGAHq8TQ4Fs17), model eleven_v4, stability 0.5, similarity 0.75, mp3_44100_128. The key is read from the
environment variable ELEVENLABS_API_KEY (load it with `set -a; . <env file>; set +a` in the same command) and is handed to curl on
stdin, so it never appears in an argument list, a log or a file.

Inputs and outputs must resolve inside marketing/launch-video. Copy external
inputs, including /tmp files, into that folder first.
"""
import base64
import json
import os
import subprocess
import sys
import time
from pathlib import Path

VOICE = "CwhRBWXzGAHq8TQ4Fs17"
MODEL = "eleven_v4"
API = "https://api.elevenlabs.io/v1"
ROOT = Path(__file__).resolve().parent.parent


def checked_path(path):
    """Resolve an input or output path and reject escapes from the project."""
    resolved = Path(path).resolve()
    if resolved != ROOT and ROOT not in resolved.parents:
        raise SystemExit(f"Refused path {str(path)!r}: must resolve inside {ROOT}")
    return resolved


def curl(method, path, body=None, out=None):
    """Call the narration service with the API key supplied on stdin."""
    key = os.environ["ELEVENLABS_API_KEY"]
    cfg = f'header = "xi-api-key: {key}"\nheader = "Content-Type: application/json"\n'
    cmd = ["curl", "-sS", "-m", "120", "-K", "-", "-X", method, API + path, "-w", "\n%{http_code}"]
    if body is not None:
        cmd += ["--data-binary", json.dumps(body)]
    p = subprocess.run(cmd, input=cfg, capture_output=True, text=True)
    txt, _, code = p.stdout.rpartition("\n")
    return int(code or 0), txt


def status():
    code, txt = curl("GET", "/user/subscription")
    d = json.loads(txt)
    return d.get("character_count"), d.get("character_limit"), d.get("tier")


def words_from_alignment(al, text):
    chars, st, en = al["characters"], al["character_start_times_seconds"], al["character_end_times_seconds"]
    words, cur, s0, e0 = [], "", None, None
    for ch, a, b in zip(chars, st, en):
        if ch.isspace():
            if cur:
                words.append((cur, s0, e0))
                cur, s0 = "", None
            continue
        if s0 is None:
            s0 = a
        cur += ch
        e0 = b
    if cur:
        words.append((cur, s0, e0))
    return words


def gen(lines_path, outdir, n_takes):
    """Generate takes from in-project input and write them within the project."""
    with open(checked_path(lines_path)) as lines_file:
        lines = json.load(lines_file)
    checked_path(outdir)
    checked_path(f"{outdir}/usage.log")
    # Validate IDs before making directories or sending requests.
    for ln in lines:
        for k in range(1, n_takes + 1):
            checked_path(f"{outdir}/{ln['id']}_t{k}.mp3")
            checked_path(f"{outdir}/{ln['id']}_t{k}.json")
    os.makedirs(checked_path(outdir), exist_ok=True)
    used0, limit, tier = status()
    print(f"credits before: {used0}/{limit} ({tier})")
    total = 0
    for ln in lines:
        for k in range(1, n_takes + 1):
            base = f"{outdir}/{ln['id']}_t{k}"
            if os.path.exists(checked_path(base + ".mp3")):
                continue
            body = {"text": ln["text"], "model_id": MODEL, "voice_settings": {"stability": 0.5, "similarity_boost": 0.75}}
            if ln.get("prev"):
                body["previous_text"] = ln["prev"]
            if ln.get("next"):
                body["next_text"] = ln["next"]
            code, txt = curl("POST", f"/text-to-speech/{VOICE}/with-timestamps?output_format=mp3_44100_128", body)
            if code != 200 and ("previous_text" in body or "next_text" in body):
                body.pop("previous_text", None)
                body.pop("next_text", None)
                code, txt = curl("POST", f"/text-to-speech/{VOICE}/with-timestamps?output_format=mp3_44100_128", body)
            if code != 200:
                print(f"{ln['id']} take {k}: HTTP {code}: {txt[:200]}")
                continue
            d = json.loads(txt)
            with open(checked_path(base + ".mp3"), "wb") as audio_file:
                audio_file.write(base64.b64decode(d["audio_base64"]))
            with open(checked_path(base + ".json"), "w") as alignment_file:
                json.dump(dict(text=ln["text"], words=words_from_alignment(d["alignment"], ln["text"])), alignment_file)
            total += len(ln["text"])
            with open(checked_path(f"{outdir}/usage.log"), "a") as f:
                f.write(f"{time.strftime('%Y-%m-%d %H:%M:%S')}  {ln['id']} take {k}  {len(ln['text'])} chars\n")
            print(f"{ln['id']} take {k}: ok ({len(ln['text'])} chars)")
    used1, limit, _ = status()
    print(f"characters sent this run: {total}; credits after: {used1}/{limit}; remaining {limit - used1}")


def dur(path):
    """Return the duration reported by ffprobe for an in-project take."""
    p = subprocess.run(["ffprobe", "-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", str(checked_path(path))], capture_output=True, text=True, check=False)
    return float(p.stdout.strip())


def best_take(ln, outdir):
    """Choose the fitting take with the fewest pauses, then shortest speech."""
    best = None
    for k in range(1, 6):
        base = f"{outdir}/{ln['id']}_t{k}"
        if not os.path.exists(checked_path(base + ".mp3")):
            continue
        d = dur(base + ".mp3")
        with open(checked_path(base + ".json")) as take_file:
            w = json.load(take_file)["words"]
        gaps = [w[i + 1][1] - w[i][2] for i in range(len(w) - 1)] or [0]
        speech_end = w[-1][2] / ln.get("tempo", 1.0)
        fit = speech_end <= ln["win"]
        score = (0 if fit else 1, max(gaps), speech_end)
        if best is None or score < best[0]:
            best = (score, base, d, speech_end, max(gaps), fit)
    return best


def pick(lines_path, outdir, out_json):
    """Write the chosen takes and subtitles using the original path strings."""
    if not out_json.endswith(".json"):
        raise SystemExit(f"Output must end in .json: {out_json!r}")
    with open(checked_path(lines_path)) as lines_file:
        lines = json.load(lines_file)
    checked_path(outdir)
    checked_path(out_json)
    checked_path(out_json.replace(".json", ".srt"))
    chosen, report, srt = [], [], []
    for ln in lines:
        best = best_take(ln, outdir)
        if not best:
            report.append(f"{ln['id']}: NO TAKE")
            continue
        _, base, _duration, se, mg, fit = best
        chosen.append(dict(id=ln["id"], t=ln["t"], file=base + ".mp3", gain_db=ln.get("gain_db", 0), tempo=ln.get("tempo", 1.0), speech_end=se, win=ln["win"], text=ln["text"]))
        report.append(f"{ln['id']}: {os.path.basename(base)} speech {se:.2f}s of {ln['win']:.1f}s window, longest pause {mg:.2f}s {'OK' if fit else 'TOO LONG'}")
        srt.append((ln["t"], ln["t"] + se + 0.15, ln["text"]))
    with open(checked_path(out_json), "w") as output_file:
        json.dump(chosen, output_file, indent=1)
    srt = [list(x) for x in srt]
    for i in range(len(srt) - 1):
        if srt[i][1] > srt[i + 1][0] - 0.03:
            srt[i][1] = max(srt[i][0] + 0.4, srt[i + 1][0] - 0.03)
    with open(checked_path(out_json.replace(".json", ".srt")), "w") as f:
        for i, (a, b, t) in enumerate(srt, 1):
            f.write(f"{i}\n{ts(a)} --> {ts(b)}\n{t}\n\n")
    print("\n".join(report))


def ts(x):
    h, r = divmod(x, 3600)
    m, s = divmod(r, 60)
    return f"{int(h):02d}:{int(m):02d}:{int(s):02d},{int(round((s - int(s)) * 1000)):03d}"


if __name__ == "__main__":
    if len(sys.argv) < 2 or sys.argv[1] in ("-h", "--help"):
        print(__doc__)
        raise SystemExit(0 if len(sys.argv) > 1 else 1)
    c = sys.argv[1]
    if c == "status":
        print("credits used/limit/tier:", status())
    elif c == "gen":
        gen(sys.argv[2], sys.argv[3], int(sys.argv[4]) if len(sys.argv) > 4 else 2)
    elif c == "pick":
        pick(sys.argv[2], sys.argv[3], sys.argv[4])
