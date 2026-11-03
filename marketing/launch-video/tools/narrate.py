#!/usr/bin/env python3
"""ElevenLabs narration for the launch cuts: one clip per line, a few takes each, pick the \
take that fits its window.

  narrate.py status                         -> prints credits used / limit (never the key)
  narrate.py gen  lines.json OUTDIR [N]     -> N takes (default 2) per line, with word \
timings; logs characters to OUTDIR/usage.log
  narrate.py pick lines.json OUTDIR OUT.json -> best take per line (fits its window, \
fewest odd pauses) as audio.py's narration json + .srt

Voice Roger (CwhRBWXzGAHq8TQ4Fs17), model eleven_v4, stability 0.5, similarity 0.75, \
mp3_44100_128. The key is read from the
environment variable ELEVENLABS_API_KEY (load it with `set -a; . <env file>; set +a` in \
the same command) and is handed to curl on
stdin, so it never appears in an argument list, a log or a file.

Interrupted paid requests stop automatic retries when their billing outcome is
unknown; reconcile the named request marker with the provider before retrying.
Publication is recoverable for these tools. After an interruption, rerun the
command to recover before reading its outputs with other tools. Independent raw
readers do not get an atomic view of two separate output files.

Inputs and outputs must resolve inside marketing/launch-video. Copy external
inputs, including /tmp files, into that folder first.
"""
import base64
import io
import json
import os
import subprocess
import sys
import time
from pathlib import Path

# Keep the project helper importable when Python's isolated mode omits script paths.
sys.path.insert(0, str(Path(__file__).resolve().parent))
from launch_paths import ROOT, checked_path as _checked_path
import narration_store as store

VOICE = "CwhRBWXzGAHq8TQ4Fs17"
MODEL = "eleven_v4"
API = "https://api.elevenlabs.io/v1"


def checked_path(path):
    """Resolve paths through the shared policy, preserving string refusal labels."""
    return _checked_path(str(path))


def curl(method, path, body=None, out=None):
    """Call the narration service with the API key supplied on stdin.

    Keep the legacy out keyword accepted; responses are always returned as text.
    """
    _ = out
    key = os.environ["ELEVENLABS_API_KEY"]
    cfg = f'header = "xi-api-key: {key}"\nheader = "Content-Type: application/json"\n'
    cmd = ["curl", "-sS", "-m", "120", "-K", "-", "-X", method, API + path, "-w", "\n%{http_code}"]
    if body is not None:
        cmd += ["--data-binary", json.dumps(body)]
    p = subprocess.run(cmd, input=cfg, capture_output=True, text=True, check=False)
    txt, _, code = p.stdout.rpartition("\n")
    return int(code or 0), txt


def status():
    """Read current narration credits and subscription tier."""
    _code, txt = curl("GET", "/user/subscription")
    d = json.loads(txt)
    return d.get("character_count"), d.get("character_limit"), d.get("tier")


def words_from_alignment(al, text):
    """Group aligned characters into words with their start and end times.

    The text argument remains accepted for callers of the original helper.
    """
    _ = text
    chars = al["characters"]
    st = al["character_start_times_seconds"]
    en = al["character_end_times_seconds"]
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


def _take_paths(base, outdir):
    """Identify the take pair and its serialized usage ledger."""
    return [checked_path(base + ".mp3"), checked_path(base + ".json"),
            checked_path(f"{outdir}/usage.log")]


def _save_take(paths, receipt):
    """Publish a saved provider response and one matching ledger entry."""
    data = receipt["response"]
    alignment = dict(text=receipt["text"],
                     words=words_from_alignment(data["alignment"], receipt["text"]))
    log = paths[2].read_bytes() if paths[2].exists() else b""
    entry = (f"{receipt['timestamp']}  {receipt['id']} take {receipt['take']}  "
             f"{len(receipt['text'])} chars\n")
    store.publish(paths, [base64.b64decode(data["audio_base64"]),
                          store.text_bytes(json.dumps(alignment)),
                          log + store.text_bytes(entry)])


def _recover_take(base, outdir):
    """Finish saved paid responses before deciding whether to request a take.

    An abandoned in-flight request has an unknown billing outcome. Refuse an
    automatic new charge until the operator reconciles that request with the
    provider and removes its named request marker. No API key is stored.
    """
    paths = _take_paths(base, outdir)
    request = store.state_file(paths, "request")
    response = store.state_file(paths, "response")
    store.recover(paths)
    if paths[0].exists() and paths[1].exists():
        request.unlink(missing_ok=True)
        response.unlink(missing_ok=True)
        return
    if response.exists():
        _save_take(paths, json.loads(response.read_text()))
        request.unlink(missing_ok=True)
        response.unlink()
    elif request.exists():
        raise SystemExit(f"Uncertain paid narration request: {request}. Reconcile its billing "
                         "and response with the provider before removing this marker to retry.")
    elif paths[0].exists():
        raise SystemExit(f"Incomplete narration take: {paths[0]} has no alignment {paths[1]}. "
                         "Restore its original alignment before retrying; no new charge was sent.")


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
    with store.locked([checked_path(outdir)]):
        _generate_lines(lines, outdir, n_takes)


def _generate_lines(lines, outdir, n_takes):
    """Generate under the directory lock, recovering each take before skipping."""
    used0, limit, tier = status()
    print(f"credits before: {used0}/{limit} ({tier})")
    total = 0
    for ln in lines:
        for k in range(1, n_takes + 1):
            base = f"{outdir}/{ln['id']}_t{k}"
            _recover_take(base, outdir)
            if os.path.exists(checked_path(base + ".mp3")):
                continue
            paths = _take_paths(base, outdir)
            request = store.state_file(paths, "request")
            response = store.state_file(paths, "response")
            store.write_state(request, store.text_bytes(json.dumps(dict(
                id=ln["id"], take=k, text=ln["text"], created=time.time()))))
            body = {
                "text": ln["text"], "model_id": MODEL,
                "voice_settings": {"stability": 0.5, "similarity_boost": 0.75},
            }
            if ln.get("prev"):
                body["previous_text"] = ln["prev"]
            if ln.get("next"):
                body["next_text"] = ln["next"]
            code, txt = curl(
                "POST",
                f"/text-to-speech/{VOICE}/with-timestamps?output_format=mp3_44100_128", body,
            )
            if code != 200 and ("previous_text" in body or "next_text" in body):
                body.pop("previous_text", None)
                body.pop("next_text", None)
                code, txt = curl(
                    "POST",
                    f"/text-to-speech/{VOICE}/with-timestamps?output_format=mp3_44100_128", body,
                )
            if code != 200:
                # Preserve the original explicit-error retry behavior. An HTTP
                # code does not establish whether the provider billed a request.
                request.unlink()
                print(f"{ln['id']} take {k}: HTTP {code}: {txt[:200]}")
                continue
            receipt = dict(id=ln["id"], take=k, text=ln["text"],
                           timestamp=time.strftime('%Y-%m-%d %H:%M:%S'), response=json.loads(txt))
            store.write_state(response, store.text_bytes(json.dumps(receipt)))
            _save_take(paths, receipt)
            request.unlink()
            response.unlink()
            total += len(ln["text"])
            print(f"{ln['id']} take {k}: ok ({len(ln['text'])} chars)")
    used1, limit, _ = status()
    print(
        f"characters sent this run: {total}; credits after: {used1}/{limit}; "
        f"remaining {limit - used1}"
    )


def dur(path):
    """Return the duration reported by ffprobe for an in-project take."""
    p = subprocess.run(
        ["ffprobe", "-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0",
         str(checked_path(path))],
        capture_output=True, text=True, check=True,
    )
    return float(p.stdout.strip())


def best_take(ln, outdir):
    """Choose the fitting take with the fewest pauses, then shortest speech."""
    best = None
    for k in range(1, 6):
        base = f"{outdir}/{ln['id']}_t{k}"
        _recover_take(base, outdir)
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
    outputs = [checked_path(out_json), checked_path(out_json.replace(".json", ".srt"))]
    with store.locked([checked_path(outdir)]), store.locked(outputs):
        store.recover(outputs)
        _pick_lines(lines, outdir, outputs)


def _pick_lines(lines, outdir, outputs):
    """Select under both locks and publish complete JSON/subtitle payloads."""
    chosen, report, srt = [], [], []
    for ln in lines:
        best = best_take(ln, outdir)
        if not best:
            report.append(f"{ln['id']}: NO TAKE")
            continue
        _, base, _duration, se, mg, fit = best
        chosen.append(dict(
            id=ln["id"], t=ln["t"], file=base + ".mp3", gain_db=ln.get("gain_db", 0),
            tempo=ln.get("tempo", 1.0), speech_end=se, win=ln["win"], text=ln["text"],
        ))
        report.append(
            f"{ln['id']}: {os.path.basename(base)} speech {se:.2f}s of {ln['win']:.1f}s window, "
            f"longest pause {mg:.2f}s {'OK' if fit else 'TOO LONG'}"
        )
        srt.append((ln["t"], ln["t"] + se + 0.15, ln["text"]))
    srt = [list(x) for x in srt]
    for i in range(len(srt) - 1):
        if srt[i][1] > srt[i + 1][0] - 0.03:
            srt[i][1] = max(srt[i][0] + 0.4, srt[i + 1][0] - 0.03)
    subtitles = io.StringIO()
    for i, (a, b, t) in enumerate(srt, 1):
        subtitles.write(f"{i}\n{ts(a)} --> {ts(b)}\n{t}\n\n")
    store.publish(outputs, [store.text_bytes(json.dumps(chosen, indent=1)),
                            store.text_bytes(subtitles.getvalue())])
    print("\n".join(report))


def ts(x):
    """Format seconds as an SRT timestamp."""
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
