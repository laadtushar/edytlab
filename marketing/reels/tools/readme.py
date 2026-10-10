"""Writes out/README.md: an index of every file with duration, size and measured loudness."""
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
    if not os.path.exists(p):
        return None
    d = dur(p)
    ls = ""
    if loud:
        m = measure_loudness(p)
        ls = f"{m['integrated_lufs']:.1f} LUFS / {m['true_peak_dbtp']:.1f} dBTP"
    return f"| `{f}` | {int(d // 60)}:{d % 60:04.1f} ({d:.1f} s) | {size(p)} | {ls} | {what} |"


def main():
    L = []
    L.append("# edytlab social videos: index\n")
    L.append("Vertical 1080x1920, 30 fps H.264 (High, yuv420p, CRF 20) with AAC 48 kHz stereo, built from the three real DJ demo recordings in "
             "`website/public/demos/`. App audio is untouched; every `*-narrated.mp4` adds Roger's narration (ElevenLabs, Eleven v4) with the app audio "
             "ducked about 12 dB under the voice and the mix normalised to -14 LUFS / -1 dBTP. The big caption above the app frame is reel-native text "
             "(Liberation Sans Bold, up to 64 px, two lines at most); in the narrated versions its words light up as they are spoken.\n")
    L.append("**Read first:** the narration was generated on ElevenLabs' free plan, whose terms say it \"does not include a commercial license and cannot be "
             "used for any commercial purpose\" and must be credited with \"elevenlabs.io\" or \"11.ai\" in the title. Full wording, the URL and the options are at the top "
             "of [`transcripts.md`](transcripts.md). The un-narrated reels carry no ElevenLabs audio.\n")
    L.append("## Reels, ready for narration (app audio only)\n")
    L.append("| File | Length | Size | Loudness | What it is |\n|---|---|---|---|---|")
    rows = [("reel-highlights.mp4", "The combined highlights reel: hook card, three demos (request typed, plan/result on the timeline, a Listen moment each), end card"),
            ("reel-beatmatched-transition.mp4", "One-demo reel: beatmatch and blend two tracks (Midnight Drive into Neon Rush), with both Listen moments"),
            ("reel-extended-club-intro.mp4", "One-demo reel: extend Solar Flare's intro, the filter that opens into the drop, a Listen moment"),
            ("reel-mini-mix.mp4", "One-demo reel: a three-track mini-mix at 124 BPM, both transitions heard"),
            ("reel-teaser.mp4", "Teaser: hook, two fast cuts, the best seconds of the transition, end card")]
    for f, w in rows:
        L.append(row(f, w))
    L.append("\n## Narrated versions (word-synced captions, SFX)\n")
    L.append("| File | Length | Size | Loudness | What it is |\n|---|---|---|---|---|")
    rows = [("reel-highlights-narrated.mp4", "Highlights reel with narration"),
            ("reel-beatmatched-transition-narrated.mp4", "Beatmatch reel with narration"),
            ("reel-extended-club-intro-narrated.mp4", "Extended-intro reel with narration"),
            ("reel-mini-mix-narrated.mp4", "Mini-mix reel with narration"),
            ("reel-teaser-narrated.mp4", "Teaser with narration"),
            ("reel-highlights-es-narrated.mp4", "Highlights reel, Spanish narration (the app's own captions stay in English)"),
            ("demo-dj-beatmatched-transition-narrated.mp4", "Full website demo, narrated (picture byte-identical to the website video)"),
            ("demo-dj-extended-club-intro-narrated.mp4", "Full website demo, narrated"),
            ("demo-dj-mini-mix-narrated.mp4", "Full website demo, narrated")]
    for f, w in rows:
        r = row(f, w)
        if r:
            L.append(r)
    L.append("\n## Captions for upload (.srt, one entry per spoken line)\n")
    srts = sorted(f for f in os.listdir(OUT) if f.endswith(".srt"))
    L.append(", ".join(f"`{f}`" for f in srts) + "\n")
    L.append("## Cue files, transcripts, audio\n")
    L.append("- `reel-*-cues.json`, `demo-*-cues.json`: every caption moment with `start`, `end`, `onscreen`, `max_seconds` (window minus 0.3 s), `max_words` "
             "(floor(max_seconds x 2.4)), the narration line and, for a Listen moment, `voice_end` (the voice stops there, the music follows).")
    L.append("- `transcripts.md`: ElevenLabs settings with what is confirmed from the docs and what is not, the free-plan licence terms, and every line per reel and per demo. "
             "`transcripts/<cue-id>.txt` is the same text one file per cue, ready to paste (`transcripts/es/` for Spanish).")
    L.append("- `audio/<video>/<cue-id>.mp3`: the generated clips, one per cue (a clip sped up to fit its window is `.wav`). `sfx/`: whoosh, hit, ding.")
    L.append("- `*-narration-timing.json`: per-word start/end seconds from ElevenLabs' character alignment.")
    L.append("- `add-narration.sh <video.mp4> <cues.json> <audio-dir> <out.mp4>`: lays clips at cue starts, ducks, mixes, normalises, copies the video stream. "
             "Use it to re-narrate with the owner's own recordings.")
    L.append("- `reel-*-contact.png`, `reel-*-narrated-contact.png`: 12-frame contact sheets.")
    L.append("- `usage.log`: ElevenLabs characters used, per call. `tools/`: the build scripts (they read `ELEVENLABS_API_KEY` from the environment, nothing else).\n")
    tl = C.build_reel("highlights")
    L.append("## Highlights reel, structure\n")
    L.append("| Time | Cue | Segment | Source (final demo video) |\n|---|---|---|---|")
    for c in get_tl("highlights")["cues"]:
        src = c.get("source")
        s = f"`{src['demo']}.mp4` {src['start']:.1f} to {src['end']:.1f} s" if src else ""
        kind = {"hook": "hook card", "end": "end card"}.get(c["kind"], c["onscreen"])
        L.append(f"| {c['start']:.1f} to {c['end']:.1f} s | {c['id']} | {kind} | {s} |")
    open(f"{OUT}/README.md", "w").write("\n".join(x for x in L if x) + "\n")


if __name__ == "__main__":
    main()
