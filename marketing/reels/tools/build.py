"""Orchestrator.

  build.py reel <key>            the plain reel: out/<file>.mp4, its cue file and contact sheet
  build.py all                   the five plain reels and the three demo cue files
  build.py demo-cues             out/demo-<slug>-cues.json for the three demo videos
  build.py measure <file>        ebur128 loudness / true peak

  key: highlights beatmatched-transition extended-club-intro mini-mix teaser

Outputs land in out/. Intermediate renders live in work/ and are not part of the repo.

Narration, for when it is regenerated (needs the ElevenLabs clips and `ELEVENLABS_API_KEY`; the
lines in spec.LINES were written for the earlier takes, see the README):

  build.py reel <key> narrated|both     (highlights-es for the Spanish reel)
  build.py demo <BT|EI|MM>              narrated version of a full website demo (+ srt)
"""
import json, os, subprocess, sys
from render import *
import cues as C

OUT = f"{ROOT}/out"
ADD = f"{REELS_DIR}/add-narration.sh"
SFX_RMS = {"whoosh": -31.0, "hit": -30.0, "ding": -33.0}      # target mean level of each effect before the final normalisation

STATUS = ("The narration lines were written for the earlier (amber-UI) takes of the demos. Check each against this "
          "footage before re-narrating; see transcripts.md.")


def get_tl(key, narrated=False):
    if key.endswith("-es"):
        tl = C.build_reel(key[:-3], lines=ES_LINES, narrated=True)
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
    return C.build_reel(key, narrated=narrated)


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


def write_plain_cues(tl, path):
    """The plain reel's cue file: every moment with its start, end, on-screen text and, for a cut from a demo, where
    it comes from (demo, caption id, start and end on the demo video) and, for a Listen cut, where the app's sound
    plays on the reel's timeline."""
    doc = dict(video=f"{tl['file']}.mp4", duration=tl["duration"], fps=tl["fps"], size="1080x1920",
               note="Plain reel: app audio only, no narration. Cut lengths are the edit list's (spec.py). "
                    "The narration lines for the earlier takes are in transcripts.md.",
               cues=tl["cues"])
    json.dump(doc, open(path, "w"), indent=2, ensure_ascii=False)
    open(path, "a").write("\n")


def write_cues(tl, timing, path, sfx=None, audio_rel=None, video_name=None):
    """The narrated build's cue file (cut lengths follow the speech, clips and timing per cue)."""
    import narrate as N
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


def write_demo_cues(code, path):
    """A full demo video's caption moments, each with the narration window it leaves (voice_end where the app's sound
    starts). The lines are spec.LINES, written for the earlier takes, so the file says so."""
    tl = C.build_demo(code)
    doc = dict(video=f"{DEMOS[code]['slug']}.mp4", duration=tl["duration"], narration_status=STATUS, cues=tl["cues"])
    json.dump(doc, open(path, "w"), indent=2, ensure_ascii=False)
    open(path, "a").write("\n")
    return C.check(tl)


def run_add(video, cues_path, audio_dir, out):
    p = subprocess.run([ADD, video, cues_path, audio_dir, out], capture_output=True, text=True)
    sys.stderr.write(p.stderr)
    if p.returncode != 0:
        raise RuntimeError("add-narration failed:\n" + p.stdout + p.stderr)
    return p.stdout


def build_plain(key):
    os.makedirs(OUT, exist_ok=True)
    tl = get_tl(key)
    res = dict(key=key, file=tl["file"], duration=tl["duration"])
    write_plain_cues(tl, f"{OUT}/{tl['file']}-cues.json")
    # app audio, normalised once
    wav = f"{WORK}/{tl['file']}-app.wav"
    render_app_audio(tl, wav)
    m4a = f"{WORK}/{tl['file']}-app.m4a"
    res["app_loudness"] = loudnorm_two_pass(wav, m4a)["result"]
    vid = render_video(tl, "plain")
    out = f"{OUT}/{tl['file']}.mp4"
    mux(vid, m4a, out)
    contact_sheet(out, tl, f"{OUT}/{tl['file']}-contact.png")
    res["plain"] = out
    return res


def build_reel_key(key, what="plain"):
    """plain: the reel as published. narrated / both: the narrated build, for when the narration is regenerated."""
    res = build_plain(key) if what in ("plain", "both") else dict(key=key)
    if what in ("narrated", "both"):
        import narrate as N
        tl = get_tl(key, narrated=True)
        timing, report = N.prepare(tl)
        bad = [r for r in report if r[1] in ("MISSING", "OVER")]
        if bad:
            raise RuntimeError(f"{key}: narration does not fit: {bad}")
        words = {cid: t["words"] for cid, t in timing.items()}
        cues_path = f"{OUT}/{tl['file']}-narrated-cues.json"
        write_cues(tl, timing, cues_path, sfx_plan(tl))
        wav = f"{WORK}/{tl['file']}-narrated-app.wav"
        render_app_audio(tl, wav)
        m4a = f"{WORK}/{tl['file']}-narrated-app.m4a"
        loudnorm_two_pass(wav, m4a)
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
    import narrate as N
    tl = C.build_demo(code)
    d = DEMOS[code]
    timing, report = N.prepare(tl)
    bad = [r for r in report if r[1] in ("MISSING", "OVER")]
    if bad:
        raise RuntimeError(f"{code}: narration does not fit: {bad}")
    cues_path = f"{OUT}/demo-{d['slug']}-cues.json"
    write_cues(tl, timing, cues_path, None, audio_rel=f"audio/{tl['file']}", video_name=f"{d['slug']}.mp4")
    out = f"{OUT}/demo-{d['slug']}-narrated.mp4"
    adir = f"{OUT}/audio/{tl['file']}"
    log = run_add(f"{DEMO_DIR}/{d['slug']}.mp4", cues_path, adir, out)
    n = write_srt(tl, timing, f"{OUT}/demo-{d['slug']}-narrated.srt")
    return dict(key=code, file=out, add_narration_output=log, srt_entries=n, fit=report)


if __name__ == "__main__":
    cmd = sys.argv[1]
    if cmd == "reel":
        res = build_reel_key(sys.argv[2], sys.argv[3] if len(sys.argv) > 3 else "plain")
        print(json.dumps(res, indent=1, default=str))
    elif cmd == "all":
        for key in REEL_ORDER:
            print(json.dumps(build_plain(key), indent=1, default=str), flush=True)
        for code in DEMOS:
            write_demo_cues(code, f"{OUT}/demo-{DEMOS[code]['slug']}-cues.json")
    elif cmd == "demo-cues":
        for code in DEMOS:
            print(code, "lines over their window:", write_demo_cues(code, f"{OUT}/demo-{DEMOS[code]['slug']}-cues.json"))
    elif cmd == "demo":
        print(json.dumps(build_demo(sys.argv[2]), indent=1, default=str))
    elif cmd == "measure":
        print(measure_loudness(sys.argv[2]))
