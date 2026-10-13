"""Writes out/README.md: an index of every file with duration, size and measured loudness.
Run it after `build.py all`: the tables read the finished files in out/."""
import json, os, subprocess
from spec import *
import cues as C
from build import get_tl
from render import measure_loudness

OUT = f"{ROOT}/out"


def dur(p):
    r = subprocess.run(["ffprobe", "-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", p], capture_output=True, text=True)
    return float(r.stdout.strip())


def size(p):
    n = os.path.getsize(p)
    return f"{n / 1e6:.1f} MB" if n > 1e6 else f"{n / 1e3:.0f} KB"


def row(f, what, loud=True):
    p = f"{OUT}/{f}"
    d = dur(p)
    ls = ""
    if loud:
        m = measure_loudness(p)
        ls = f"{m['integrated_lufs']:.1f} LUFS / {m['true_peak_dbtp']:.1f} dBTP"
    return f"| `{f}` | {int(d // 60)}:{d % 60:04.1f} ({d:.1f} s) | {size(p)} | {ls} | {what} |"


def main():
    L = []
    L.append("# edytlab social videos: index\n")
    L.append("> **Licence.** The app footage and the plain reels (`reel-*.mp4`) are covered by the repository's MIT licence. The narration clips are not: "
             "everything under `audio/` and the sound effects in `sfx/` were generated with ElevenLabs ([elevenlabs.io](https://elevenlabs.io)) on its free plan. "
             "ElevenLabs' terms apply to them: \"The free plan does not include a commercial license and cannot be used for any commercial purpose\", and a published piece "
             "must credit \"elevenlabs.io\" or \"11.ai\" in its title ([source](https://elevenlabs.io/docs/help-center/legal/can-i-publish-the-content-i-generate-on-the-platform)). "
             "The narrated videos made from those clips are gone (below); when they are regenerated, do it on a paid plan before any commercial use: "
             "`tools/` and `add-narration.sh` rebuild every narrated file from the lines in `transcripts/` and the plain reels.\n")
    L.append("Vertical 1080x1920, 30 fps H.264 (High, yuv420p, CRF 20) with AAC 48 kHz stereo, cut from the three real DJ demo recordings in "
             "`website/public/demos/`, as re-filmed in the app's violet look (#581). App audio is untouched, normalised to -14 LUFS / -1 dBTP. "
             "The reels carry no narration. Their own text (the caption above the app frame, the hook and end cards) is the website's: Inter SemiBold/Bold on its dark "
             "tokens (`website/app/globals.css`), the caption on a card with a violet edge like the captions burned into the demos, 4.5:1 contrast or better. "
             "Every added element stays 120 px clear of both sides and above the bottom 20 % of the frame, where the platforms put their buttons "
             "(the recording itself runs edge to edge). `tools/brand.py` reads the colours from the site and checks the contrast; "
             "the font is Debian/Ubuntu's `fonts-inter` (SIL OFL), not copied into the repo.\n")
    L.append("**Narrated versions.** The narrated reels and full demos (`*-narrated.mp4`, their `.srt` and `*-narration-timing.json`) were cut and voiced "
             "for the earlier, amber-UI recordings, so they were deleted when the demos were re-filmed. They will be regenerated once narration credits are "
             "available. Until then post the plain reels below. The lines (`transcripts.md`, `transcripts/`) and the generated clips (`audio/`) "
             "are kept as the source for that, but **they were written for the old takes: check every line against the new footage before re-narrating** "
             "(the timings changed, and so did a few facts, for example the mini-mix export is now 1:19 and its second Listen caption says 0:46).\n")
    L.append("## Reels (app audio only)\n")
    L.append("| File | Length | Size | Loudness | What it is |\n|---|---|---|---|---|")
    rows = [("reel-highlights.mp4", "The combined highlights reel: hook card, three demos (request typed, plan/result on the timeline, a Listen moment each), end card"),
            ("reel-beatmatched-transition.mp4", "One-demo reel: beatmatch and blend two tracks (Midnight Drive into Neon Rush), with both Listen moments"),
            ("reel-extended-club-intro.mp4", "One-demo reel: extend Solar Flare's intro, the filter that opens into the drop, a Listen moment"),
            ("reel-mini-mix.mp4", "One-demo reel: a three-track mini-mix at 124 BPM, both transitions heard"),
            ("reel-teaser.mp4", "Teaser: hook, two fast cuts, the best seconds of the transition, end card")]
    for f, w in rows:
        L.append(row(f, w))
    L.append("\n## Cue files, transcripts, audio\n")
    L.append("- `reel-*-cues.json`: every moment of a reel with `start`, `end` and `onscreen` text; for a cut from a demo its `source` (demo, caption id, start and end on the demo video), "
             "and for a Listen cut `sound`, where the app's sound plays on the reel's timeline (a Listen cut starts at most 0.3 s before the sound and ends inside it). No narration.")
    L.append("- `demo-*-cues.json`: the caption moments of each website demo video, measured on the re-filmed videos: `start`, `end`, `onscreen`, `audio_window` for a Listen, "
             "the room each leaves for a voice (`max_seconds` = window minus 0.3 s, `max_words` = floor(`max_seconds` x 2.4), `voice_end` where the app's sound starts) "
             "and `narration`, the line written for the old take (see `narration_status`).")
    L.append("- `transcripts.md`: ElevenLabs settings with what is confirmed from the docs and what is not, the free-plan licence terms, and every line per reel and per demo, "
             "for the old takes (their windows, not the new ones). `transcripts/<cue-id>.txt` is the same text one file per cue, ready to paste (`transcripts/es/` for Spanish).")
    L.append("- `audio/<video>/<cue-id>.mp3`: the generated clips of the old takes, one per cue (a clip sped up to fit its window is `.wav`). `sfx/`: whoosh, hit, ding.")
    L.append("- `add-narration.sh <video.mp4> <cues.json> <audio-dir> <out.mp4>`: lays clips at cue starts, ducks, mixes, normalises, copies the video stream. "
             "Use it to re-narrate with the owner's own recordings.")
    L.append("- `reel-*-contact.png`: 12-frame contact sheets of the reels.")
    L.append("- `tools/`: the build scripts (see its README; narration needs `ELEVENLABS_API_KEY` in the environment, a plain build needs nothing).\n")
    L.append("## Highlights reel, structure\n")
    L.append("| Time | Cue | Segment | Source (final demo video) |\n|---|---|---|---|")
    for c in get_tl("highlights")["cues"]:
        src = c.get("source")
        s = f"`{src['demo']}.mp4` {src['start']:.1f} to {src['end']:.1f} s" if src else ""
        kind = {"hook": "hook card", "end": "end card"}.get(c["kind"], c["onscreen"])
        L.append(f"| {c['start']:.1f} to {c['end']:.1f} s | {c['id']} | {kind} | {s} |")
    open(f"{OUT}/README.md", "w").write("\n".join(L) + "\n")


if __name__ == "__main__":
    main()
