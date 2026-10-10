"""Orchestrator.

  build.py reel <key> [plain|narrated|both]   key: highlights beatmatched-transition extended-club-intro mini-mix teaser highlights-es
  build.py demo <BT|EI|MM>                    narrated version of a full website demo (+ srt)
  build.py measure <file>                     ebur128 loudness / true peak

Outputs land in out/. Intermediate renders live in work/ and are deleted by cleanup.sh when done.
"""
import json, os, subprocess, sys
from render import *
import cues as C
import narrate as N

OUT = f"{ROOT}/out"
ADD = f"{ROOT}/add-narration.sh"
SFX_RMS = {"whoosh": -31.0, "hit": -30.0, "ding": -33.0}      # target mean level of each effect before the final normalisation


def get_tl(key):
    if key.endswith("-es"):
        tl = C.build_reel(key[:-3], lines=ES_LINES)
        tl["file"] = f"{REELS[key[:-3]]['file']}-es"
        tl["lang"] = "es"
        for c in tl["cues"]:
            if c["kind"] == "hook":
                c["title"], c["sub"] = ES_TEXT["hook_title"], ES_TEXT["hook_sub"]
                c["onscreen"] = " ".join(c["title"])
            elif c["kind"] == "listen":
                c["onscreen"] = ES_TEXT["listen"][c["source"]["caption"]]
            elif c["kind"] == "end":
                c["onscreen"] = f"edytlab · {ES_TEXT['free']} · {SITE_URL} · macOS · Windows · Linux"
        return tl
    return C.build_reel(key)


def mean_volume(path):
    p = subprocess.run(["ffmpeg", "-nostdin", "-hide_banner", "-i", path, "-af", "volumedetect", "-f", "null", "-"], capture_output=True, text=True)
    return float(re.search(r"mean_volume:\s+(-?[\d.]+) dB", p.stderr).group(1))


def sfx_plan(tl):
    """Whoosh on cuts into a step, a soft hit under the hook, a ding at the end card. Never on a Listen cut."""
    plan = []
    gains = {n: round(SFX_RMS[n] - mean_volume(f"{OUT}/sfx/{n}.mp3"), 1) for n in SFX_RMS}
    prev = None
    for c in tl["cues"]:
        if c["kind"] == "hook":
            plan.append(dict(file="sfx/hit.mp3", at=round(c["start"], 3), gain_db=gains["hit"]))
        elif c["kind"] == "step" and prev is not None:
            plan.append(dict(file="sfx/whoosh.mp3", at=round(max(0.0, c["start"] - 0.1), 3), gain_db=gains["whoosh"]))
        elif c["kind"] == "end":
            plan.append(dict(file="sfx/ding.mp3", at=round(c["start"], 3), gain_db=gains["ding"]))
        prev = c
    return plan


def fmt_ts(t):
    t = max(0.0, t)
    h, rem = divmod(t, 3600)
    m, s = divmod(rem, 60)
    return f"{int(h):02d}:{int(m):02d}:{int(s):02d},{int(round((s - int(s)) * 1000)):03d}"


def wrap2(text, width=42):
    if len(text) <= width:
        return text
    words = text.split()
    best = None
    for k in range(1, len(words)):
        a, b = " ".join(words[:k]), " ".join(words[k:])
        score = max(len(a), len(b))
        if best is None or score < best[0]:
            best = (score, a, b)
    return f"{best[1]}\n{best[2]}"


def write_srt(tl, timing, path):
    """One entry per narrated cue, timed from the character alignment (first word in -> last word out)."""
    entries = []
    cs = [c for c in tl["cues"] if c["id"] in timing]
    for i, c in enumerate(cs):
        w = timing[c["id"]]["words"]
        if not w:
            continue
        a = c["start"] + w[0]["start"]
        b = c["start"] + w[-1]["end"] + 0.25
        nxt = cs[i + 1]["start"] if i + 1 < len(cs) else tl["duration"]
        b = min(b, nxt - 0.02, c["end"])
        text = wrap2(" ".join(x["text"] for x in w))
        entries.append((a, b, text))
    with open(path, "w") as f:
        for n, (a, b, text) in enumerate(entries, 1):
            f.write(f"{n}\n{fmt_ts(a)} --> {fmt_ts(b)}\n{text}\n\n")
    return len(entries)


def write_cues(tl, timing, path, sfx=None, audio_rel=None, video_name=None):
    cues = []
    for c in tl["cues"]:
        c = dict(c)
        t = timing.get(c["id"])
        if t:
            c["clip"] = t["clip"]
            c["clip_speech_seconds"] = t["speech_end"]
            if t["tempo"] != 1.0:
                c["clip_tempo"] = t["tempo"]
        cues.append(c)
    doc = dict(video=video_name or f"{tl['file']}.mp4", duration=tl["duration"], audio_dir=audio_rel or f"audio/{tl['file']}",
               narration_voice="Roger - Laid-Back, Casual, Resonant (CwhRBWXzGAHq8TQ4Fs17)", narration_model=N.MODEL_ID, cues=cues)
    if tl.get("fps"):
        doc["fps"] = tl["fps"]
    if tl.get("lang"):
        doc["language"] = tl["lang"]
    if sfx:
        doc["sfx"] = sfx
    json.dump(doc, open(path, "w"), indent=2, ensure_ascii=False)


def run_add(video, cues_path, audio_dir, out):
    p = subprocess.run([ADD, video, cues_path, audio_dir, out], capture_output=True, text=True)
    sys.stderr.write(p.stderr)
    if p.returncode != 0:
        raise RuntimeError("add-narration failed:\n" + p.stdout + p.stderr)
    return p.stdout


def build_reel_key(key, what="both"):
    os.makedirs(OUT, exist_ok=True)
    tl = get_tl(key)
    timing, report = N.prepare(tl)
    bad = [r for r in report if r[1] in ("MISSING", "OVER")]
    if bad:
        raise RuntimeError(f"{key}: narration does not fit: {bad}")
    words = {cid: t["words"] for cid, t in timing.items()}
    sfx = sfx_plan(tl)
    cues_path = f"{OUT}/{tl['file']}-cues.json"
    write_cues(tl, timing, cues_path, sfx)
    res = dict(key=key, file=tl["file"], duration=tl["duration"])
    # app audio, normalised once; both versions start from it
    wav = f"{WORK}/{tl['file']}-app.wav"
    render_app_audio(tl, wav)
    m4a = f"{WORK}/{tl['file']}-app.m4a"
    meas = loudnorm_two_pass(wav, m4a)
    res["app_loudness"] = meas["result"]
    if what in ("plain", "both"):
        vid = render_video(tl, "plain")
        out = f"{OUT}/{tl['file']}.mp4"
        mux(vid, m4a, out)
        contact_sheet(out, tl, f"{OUT}/{tl['file']}-contact.png")
        res["plain"] = out
    if what in ("narrated", "both"):
        vid = render_video(tl, "words", words)
        base = f"{WORK}/{tl['file']}-wordsync-base.mp4"
        mux(vid, m4a, base)
        out = f"{OUT}/{tl['file']}-narrated.mp4"
        res["add_narration_output"] = run_add(base, cues_path, f"{OUT}/audio/{tl['file']}", out)
        contact_sheet(out, tl, f"{OUT}/{tl['file']}-narrated-contact.png")
        res["narrated"] = out
        res["srt_entries"] = write_srt(tl, timing, f"{OUT}/{tl['file']}-narrated.srt")
    res["fit"] = report
    return res


def build_demo(code):
    tl = C.build_demo(code)
    d = DEMOS[code]
    timing, report = N.prepare(tl)
    bad = [r for r in report if r[1] in ("MISSING", "OVER")]
    if bad:
        raise RuntimeError(f"{code}: narration does not fit: {bad}")
    cues_path = f"{OUT}/demo-{d['slug']}-cues.json"
    write_cues(tl, timing, cues_path, None, audio_rel=f"audio/{tl['file']}", video_name=f"{d['slug']}.mp4")
    # N.prepare wrote the clips to audio/demo-<slug>/ (tl['file'] is the slug); align the dir name
    out = f"{OUT}/demo-{d['slug']}-narrated.mp4"
    adir = f"{OUT}/audio/{tl['file']}"
    log = run_add(f"{DEMO_DIR}/{d['slug']}.mp4", cues_path, adir, out)
    tl2 = dict(tl)
    n = write_srt(tl2, timing, f"{OUT}/demo-{d['slug']}-narrated.srt")
    return dict(key=code, file=out, add_narration_output=log, srt_entries=n, fit=report)


if __name__ == "__main__":
    cmd = sys.argv[1]
    if cmd == "reel":
        res = build_reel_key(sys.argv[2], sys.argv[3] if len(sys.argv) > 3 else "both")
        print(json.dumps(res, indent=1, default=str))
    elif cmd == "demo":
        res = build_demo(sys.argv[2])
        print(json.dumps(res, indent=1, default=str))
    elif cmd == "measure":
        print(measure_loudness(sys.argv[2]))
