"""Build cue timelines (reels and full demos) from spec.py. Pure data, no rendering."""
import json, math, os
from spec import *

FPS = 30


def snap(t):
    return round(t * FPS) / FPS


def _cue_common(cid, start, end, onscreen, line, voice_end=None, kind="step", source=None):
    """Fields every cue carries. voice_end < end for Listen cues (voice only before the music)."""
    ve = end if voice_end is None else voice_end
    silent = not line
    ms = 0.0 if silent else max(0.0, round(ve - start - BREATHING_ROOM, 3))
    cue = dict(id=cid, kind=kind, start=round(start, 3), end=round(end, 3), onscreen=onscreen,
               max_seconds=ms, max_words=0 if silent else max_words(ms))
    if line:
        cue["narration"] = line
        cue["narration_spoken"] = spoken_text(line)
        cue["narration_display"] = display_text(line)
        cue["words"] = spoken_word_count(line)
    else:
        cue["silent"] = True
    if voice_end is not None and voice_end < end - 1e-6:
        cue["voice_end"] = round(voice_end, 3)
        cue["note"] = "app audio plays after the voice window: the line is only a lead-in, narration stays silent over the music"
    if source:
        cue["source"] = source
    return cue


SPEECH_FILE = f"{ROOT}/work/speech.json"


def load_speech():
    """spoken text -> seconds from the clip start to the end of the last word (written by narrate.py)."""
    return json.load(open(SPEECH_FILE)) if os.path.exists(SPEECH_FILE) else {}


def clamp(x, lo, hi):
    return max(lo, min(hi, x))


def build_reel(name, lines=None, speech=None):
    """Edit list -> cue timeline. Cut lengths follow the real speech length when the clip exists
    (words / 2.4 is only the fallback), so every line ends 0.3 s or more before its cue does."""
    r = REELS[name]
    speech = load_speech() if speech is None else speech
    lines = lines or {}
    t = 0.0
    segs = []
    n = 0
    for it in r["items"]:
        kind = it["kind"]
        n += 1
        cid = f"{r['prefix']}{n:02d}"
        if kind in ("hook", "end"):
            line = lines.get(cid, it.get("line", END_LINE) if kind == "end" else it["line"])
            sp = speech.get(spoken_text(line))
            default = it.get("seconds", HOOK_SECONDS if kind == "hook" else END_SECONDS)
            dur = snap(clamp(math.ceil((sp + 0.45) * 10) / 10, 2.0, 3.0)) if sp else snap(default)
            if kind == "hook":
                onscreen = " ".join(it["title"])
                cue = _cue_common(cid, t, t + dur, onscreen, line, kind="hook")
                cue.update(title=it["title"], sub=it.get("sub", ""))
            else:
                cue = _cue_common(cid, t, t + dur, f"edytlab · Free, open source · {SITE_URL} · macOS · Windows · Linux", line, kind="end")
            if sp:
                cue["speech_seconds"] = sp
            segs.append(cue); t += dur
            continue
        code, (capid, cs, ce, text, aud) = caption_by_id(it["cap"])
        line = lines.get(cid, it["line"] if "line" in it else LINES.get(capid, ""))
        if it.get("silent"):
            line = ""
        sp = speech.get(spoken_text(line)) if line else None
        words = spoken_word_count(line) if line else 0
        est = (sp + 0.6) if sp else words / WORDS_PER_SECOND + 0.7
        if kind == "step":
            src = it["src"]
            if "seconds" in it:
                dur = snap(clamp(math.ceil((sp + 0.5) * 10) / 10, 2.2, 3.2)) if sp else snap(it["seconds"])
            else:
                dur = snap(math.ceil(max(it.get("floor", STEP_FLOOR), est) * 10) / 10)
            avail = math.floor((ce - src) * FPS) / FPS     # never run past the caption's own window
            dur = min(dur, avail)
            src_end = src + dur
            voice_end = None
        else:  # listen
            onset, offset = aud
            if "src" in it:
                src = it["src"]; dur = snap(it["seconds"]); src_end = src + dur
                voice_end = max(0.0, onset - src) if line else None
            else:
                lead = max(LISTEN_LEAD, (sp + 0.5) if sp else words / WORDS_PER_SECOND + 0.6) if line else 1.0
                lead = math.ceil(lead * 10) / 10
                src = onset - lead
                dur = snap(lead + it["music"]); src_end = src + dur
                voice_end = lead if line else None
        assert src >= cs - 1e-6 and src_end <= ce + 0.05, (name, capid, src, src_end, cs, ce)
        cue = _cue_common(cid, t, t + dur, text, line, voice_end=(t + voice_end) if voice_end is not None else None, kind=kind,
                          source=dict(demo=DEMOS[code]["slug"], code=code, caption=capid, start=round(src, 3), end=round(src_end, 3)))
        if sp:
            cue["speech_seconds"] = sp
        segs.append(cue); t += dur
    return dict(name=name, file=r["file"], duration=round(t, 3), fps=FPS, cues=segs, pills=r.get("pills", False),
                label=r["label"])


def build_demo(code):
    d = DEMOS[code]
    cues = []
    for cid, start, end, text, aud in caption_table(code):
        line = LINES.get(cid, "")
        ve = aud[0] if aud else None
        kind = "listen" if aud else ("title" if cid.endswith("01") else "step")
        cue = _cue_common(cid, start, end, text, line, voice_end=ve, kind=kind,
                          source=dict(demo=d["slug"], code=code, caption=cid, start=start, end=end))
        if aud:
            cue["audio_window"] = [aud[0], aud[1]]
        cues.append(cue)
    return dict(name=f"demo-{d['slug']}", file=f"demo-{d['slug']}", duration=d["duration"], cues=cues, title=d["title"])


def check(timeline):
    bad = []
    for c in timeline["cues"]:
        if c.get("silent"):
            continue
        if c["words"] > c["max_words"]:
            bad.append((c["id"], c["words"], c["max_words"], c["narration"]))
    return bad


if __name__ == "__main__":
    total_chars = set()
    for name in REEL_ORDER:
        tl = build_reel(name)
        print(f"== {name}: {tl['duration']:.1f}s, {len(tl['cues'])} cues")
        for c in tl["cues"]:
            ve = c.get("voice_end", c["end"])
            print(f"  {c['id']} {c['start']:6.2f}-{c['end']:6.2f} {c['kind']:6s} words={c.get('words',0):2d}/max {c['max_words']:2d}  {c.get('narration_spoken','(silent)')[:60]}")
            if c.get("narration_spoken"): total_chars.add(c["narration_spoken"])
        for b in check(tl): print("  OVER:", b)
    for code in DEMOS:
        tl = build_demo(code)
        print(f"== demo {code}: {tl['duration']}s, {len(tl['cues'])} cues")
        for c in tl["cues"]:
            print(f"  {c['id']} {c['start']:6.2f}-{c['end']:6.2f} {c['kind']:6s} words={c.get('words',0):2d}/max {c['max_words']:2d}  {c.get('narration_spoken','(silent)')[:60]}")
            if c.get("narration_spoken"): total_chars.add(c["narration_spoken"])
        for b in check(tl): print("  OVER:", b)
    print("unique lines", len(total_chars), "chars", sum(len(x) for x in total_chars))
