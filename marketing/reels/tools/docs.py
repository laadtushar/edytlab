"""Writes out/transcripts.md, out/transcripts/<cue-id>.txt (plus es/) and the per-cue tables.
The intro (ElevenLabs settings, licence terms) is a template in INTRO below."""
import json, os, re, sys
from spec import *
import cues as C
import narrate as N
from build import get_tl

OUT = f"{ROOT}/out"
KEYS = [("highlights", "Highlights reel (reel-highlights)"),
        ("beatmatched-transition", "Reel: beatmatch and blend two tracks (reel-beatmatched-transition)"),
        ("extended-club-intro", "Reel: extend an intro for mixing (reel-extended-club-intro)"),
        ("mini-mix", "Reel: a three-track mini-mix (reel-mini-mix)"),
        ("teaser", "Teaser (reel-teaser)"),
        ("highlights-es", "Spanish highlights reel (reel-highlights-es)")]


def timing(file):
    p = f"{OUT}/{file}-narration-timing.json"
    return json.load(open(p)) if os.path.exists(p) else {}


def cell(s):
    return s.replace("|", "\\|").replace("\n", " ")


def table(tl):
    t = timing(tl["file"])
    rows = ["| Cue | Window (s) | On screen | Narration (text sent to ElevenLabs) | Words / max | Spoken | Note |",
            "|---|---|---|---|---|---|---|"]
    for c in tl["cues"]:
        win = f"{c['start']:.2f} to {c['end']:.2f}"
        if "voice_end" in c:
            win += f" (voice to {c['voice_end']:.2f})"
        if c.get("narration") and not c.get("silent"):
            tm = t.get(c["id"], {})
            sp = f"{tm['speech_end']:.2f} s" if tm else "-"
            note = {"hook": "title card", "end": "end card", "listen": "lead-in only, silent over the music"}.get(c["kind"], "")
            if tm.get("tempo", 1.0) != 1.0:
                note = (note + "; " if note else "") + f"clip sped up {tm['tempo']:.2f}x to keep 0.3 s of room"
            rows.append(f"| {c['id']} | {win} | {cell(c['onscreen'])} | {cell(c['narration_spoken'])} | {c['words']} / {c['max_words']} | {sp} | {note} |")
        else:
            why = "silent: the app's music plays here, narration would talk over it" if c["kind"] == "listen" else "silent"
            rows.append(f"| {c['id']} | {win} | {cell(c['onscreen'])} | (silent) | - | - | {why} |")
    return "\n".join(rows)


def write_txt():
    os.makedirs(f"{OUT}/transcripts/es", exist_ok=True)
    n = 0
    all_tl = [(k, get_tl(k)) for k, _ in KEYS] + [(code, C.build_demo(code)) for code in DEMOS]
    for key, tl in all_tl:
        sub = "es/" if key.endswith("-es") else ""
        for c in tl["cues"]:
            if c.get("narration") and not c.get("silent"):
                open(f"{OUT}/transcripts/{sub}{c['id']}.txt", "w").write(c["narration_spoken"] + "\n")
                n += 1
    return n


def usage_text():
    d = json.load(open(f"{ROOT}/work/ledger.json"))
    tts = sum(n for t, w, n in d["entries"] if w.startswith("tts") and "speed" not in w)
    spd = sum(n for t, w, n in d["entries"] if "speed" in w)
    sfx = sum(n for t, w, n in d["entries"] if w.startswith("sfx"))
    last = ""
    for line in open(f"{OUT}/usage.log"):
        if "status: character_count=" in line:
            last = line.strip()
    lines = [
        f"- text-to-speech: {tts} characters in {sum(1 for t, w, n in d['entries'] if w.startswith('tts') and 'speed' not in w)} calls (every distinct line of the reels, the three demos and the Spanish reel; "
        f"a line that appears in several files was generated once). Included: one line regenerated because its first take had unparsed markup, and one line replaced by a shorter wording to fit its window",
        f"- one speed experiment: {spd} characters (the Speed setting test described above)",
        f"- three sound effects: {sfx} credits counted in my ledger (40 credits per second of requested duration, plus a margin)",
        f"- total in my local ledger: **{d['spent']} of the 8,000-character budget** (the account's free allowance is 10,000 a month)",
        f"- the account's own counter (`GET /v1/user/subscription`, which lags the calls by a few seconds): `{last.split('  ', 1)[-1] if last else 'n/a'}`",
        "- full per-call log: `usage.log`"]
    return "\n".join(lines), d["spent"]


_u, _spent = usage_text()
INTRO = open(f"{ROOT}/tools/intro.md").read().replace("@@USAGE@@", _u).replace("@@CHARS@@", f"{_spent:,}")


def main():
    parts = [INTRO]
    parts.append("\n## Reels\n")
    for key, title in KEYS:
        tl = get_tl(key)
        parts.append(f"\n### {title}\n")
        lang = " Spanish narration, the same shots with cut lengths set by the Spanish speech." if key.endswith("-es") else ""
        vids = f"`{tl['file']}-narrated.mp4` (no un-narrated Spanish video: its cuts are timed to the Spanish speech)" if key.endswith("-es") else f"`{tl['file']}.mp4` / `{tl['file']}-narrated.mp4`"
        parts.append(f"Video: {vids}, {tl['duration']:.1f} s.{lang} Cue file: `{tl['file']}-cues.json`. Audio: `audio/{tl['file']}/<cue-id>.mp3`.\n")
        parts.append(table(tl))
    parts.append("\n## Full website demo videos\n")
    for code in DEMOS:
        tl = C.build_demo(code)
        d = DEMOS[code]
        parts.append(f"\n### {d['title']} (demo-{d['slug']})\n")
        parts.append(f"Video: `{d['slug']}.mp4` (the website video, untouched) / `demo-{d['slug']}-narrated.mp4`, {d['duration']:.1f} s. Cue file: `demo-{d['slug']}-cues.json`. Audio: `audio/demo-{d['slug']}/<cue-id>.mp3`.\n")
        parts.append(table(tl))
    open(f"{OUT}/transcripts.md", "w").write("\n".join(parts) + "\n")
    print("txt files:", write_txt())


if __name__ == "__main__":
    main()
