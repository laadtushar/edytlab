"""Build cue timelines (reels and full demos) from spec.py. Pure data, no rendering.

A reel's timeline is its edit list laid end to end (`build_reel`). The plain reels carry no
narration: every cut length is the edit list's own `seconds`, so a plain reel does not depend
on a voice. `build_reel(name, narrated=True)` is the earlier way of building it, for
re-narrating: the cut lengths follow the real speech length of each line (the lines in
spec.LINES, written for the earlier takes) and a Listen cut leads in long enough for its line.
"""
import json, math, os
from spec import *

FPS = 30
SRC_FPS = 15        # the demo videos' frame rate: a cut starts on one of their frames
NARRATED_STEP_FLOOR = 4.2
NARRATED_LISTEN_LEAD = 2.0


def snap(t):
    return round(t * FPS) / FPS


def snap_src(t):
    return round(t * SRC_FPS) / SRC_FPS


def _cue_common(cid, start, end, onscreen, line, voice_end=None, kind="step", source=None):
    """Fields every narrated cue carries. voice_end < end for Listen cues (voice only before the music)."""
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


def _source(code, capid, start, end):
    return dict(demo=DEMOS[code]["slug"], code=code, caption=capid, start=round(start, 3), end=round(end, 3))


def build_reel(name, lines=None, speech=None, narrated=False):
    """Edit list -> cue timeline.

    Plain (the default): lengths are the edit list's seconds. Every cut is checked against its
    caption's window on the demo video, and a Listen cut against the app's sound: it starts at most
    0.3 s before the sound does and ends inside it.
    Narrated: lengths follow the speech (see the module docstring)."""
    r = REELS[name]
    if narrated:
        return _build_narrated(r, name, lines, speech)
    t = 0.0
    segs = []
    for n, it in enumerate(r["items"], 1):
        kind = it["kind"]
        cid = f"{r['prefix']}{n:02d}"
        if kind in ("hook", "end"):
            dur = snap(it.get("seconds", HOOK_SECONDS if kind == "hook" else END_SECONDS))
            if kind == "hook":
                cue = dict(id=cid, kind=kind, start=round(t, 3), end=round(t + dur, 3), onscreen=" ".join(it["title"]),
                           title=it["title"], sub=it.get("sub", ""))
            else:
                cue = dict(id=cid, kind=kind, start=round(t, 3), end=round(t + dur, 3),
                           onscreen=f"edytlab · Free, open source · {SITE_URL} · macOS · Windows · Linux")
            segs.append(cue); t += dur
            continue
        code, (capid, cs, ce, text, aud) = caption_by_id(it["cap"])
        if kind == "step":
            src = snap_src(it["src"])
            dur = snap(it.get("seconds", STEP_SECONDS))
            sound = None
        else:  # listen
            onset, offset = aud
            if "src" in it:
                src, dur = snap_src(it["src"]), snap(it["seconds"])
            else:
                src = math.floor((onset - it.get("lead", LISTEN_LEAD)) * SRC_FPS + 1e-6) / SRC_FPS
                dur = snap((onset - src) + it["music"])
            assert src >= onset - 0.3, (name, capid, "starts well before the sound", src, onset)
            sound = (round(t + max(0.0, onset - src), 3), round(t + min(offset, src + dur) - src, 3))
            assert src + dur <= offset + 0.02, (name, capid, "runs past the end of the sound", src + dur, offset)
        src_end = src + dur
        assert src >= cs - 1e-6 and src_end <= ce + 0.05, (name, capid, "outside its caption's window", src, src_end, cs, ce)
        cue = dict(id=cid, kind=kind, start=round(t, 3), end=round(t + dur, 3), onscreen=text,
                   source=_source(code, capid, src, src_end))
        if sound:
            cue["sound"] = list(sound)
        segs.append(cue); t += dur
    return dict(name=name, file=r["file"], duration=round(t, 3), fps=FPS, cues=segs, pills=r.get("pills", False),
                label=r["label"])


def _build_narrated(r, name, lines, speech):
    """The earlier build: cut lengths follow the real speech length when the clip exists
    (words / 2.4 is only the fallback), so every line ends 0.3 s or more before its cue does."""
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
            src = snap_src(it["src"])
            if "seconds" in it and line:
                dur = snap(clamp(math.ceil((sp + 0.5) * 10) / 10, 2.2, 3.2)) if sp else snap(it["seconds"])
            else:
                dur = snap(math.ceil(max(it.get("seconds", it.get("floor", NARRATED_STEP_FLOOR)), est) * 10) / 10)
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
                lead = max(NARRATED_LISTEN_LEAD, (sp + 0.5) if sp else words / WORDS_PER_SECOND + 0.6) if line else 1.0
                lead = math.ceil(lead * 10) / 10
                src = onset - lead
                dur = snap(lead + it["music"]); src_end = src + dur
                voice_end = lead if line else None
        assert src >= cs - 1e-6 and src_end <= ce + 0.05, (name, capid, src, src_end, cs, ce)
        cue = _cue_common(cid, t, t + dur, text, line, voice_end=(t + voice_end) if voice_end is not None else None, kind=kind,
                          source=_source(code, capid, src, src_end))
        if sp:
            cue["speech_seconds"] = sp
        segs.append(cue); t += dur
    return dict(name=name, file=r["file"], duration=round(t, 3), fps=FPS, cues=segs, pills=r.get("pills", False),
                label=r["label"])


def build_demo(code):
    """The caption moments of a full demo video, each with the narration window it leaves. The lines are
    spec.LINES, written for the earlier takes: `check` flags the ones that no longer fit."""
    d = DEMOS[code]
    cues = []
    for cid, start, end, text, aud in caption_table(code):
        line = LINES.get(cid, "")
        ve = aud[0] if aud else None
        kind = "listen" if aud else ("title" if cid.endswith("01") else "step")
        cue = _cue_common(cid, start, end, text, line, voice_end=ve, kind=kind,
                          source=_source(code, cid, start, end))
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
    for name in REEL_ORDER:
        tl = build_reel(name)
        print(f"== {name}: {tl['duration']:.1f}s, {len(tl['cues'])} cues")
        for c in tl["cues"]:
            src = c.get("source")
            where = f"{src['demo']} {src['start']:.2f}-{src['end']:.2f}" if src else ""
            sound = f" sound {c['sound'][0]:.2f}-{c['sound'][1]:.2f}" if "sound" in c else ""
            print(f"  {c['id']} {c['start']:6.2f}-{c['end']:6.2f} {c['kind']:6s} {where}{sound}  {c['onscreen'][:44]}")
    print("\nThe demo narration lines (written for the earlier takes) that do not fit their window now:")
    for code in DEMOS:
        tl = build_demo(code)
        for b in check(tl):
            print(f"  {code} {b[0]}: {b[1]} words, room for {b[2]}: {b[3]}")
