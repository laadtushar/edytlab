# edytlab social videos: index

> **Licence.** The app footage and the plain reels (`reel-*.mp4` without `-narrated`) are covered by the repository's MIT licence. The narration is not: every `*-narrated.mp4`, everything under `audio/`, and the sound effects in `sfx/` were generated with ElevenLabs ([elevenlabs.io](https://elevenlabs.io)) on its free plan. ElevenLabs' terms apply to them: "The free plan does not include a commercial license and cannot be used for any commercial purpose", and a published piece must credit "elevenlabs.io" or "11.ai" in its title ([source](https://elevenlabs.io/docs/help-center/legal/can-i-publish-the-content-i-generate-on-the-platform)). Regenerate the narration on a paid plan before any commercial use: `tools/` and `add-narration.sh` rebuild every narrated file from `transcripts/` and the plain reels.

Vertical 1080x1920, 30 fps H.264 (High, yuv420p, CRF 20) with AAC 48 kHz stereo, built from the three real DJ demo recordings in `website/public/demos/`. App audio is untouched; every `*-narrated.mp4` adds Roger's narration (ElevenLabs, Eleven v4) with the app audio ducked about 12 dB under the voice and the mix normalised to -14 LUFS / -1 dBTP. The big caption above the app frame is reel-native text (Liberation Sans Bold, up to 64 px, two lines at most); in the narrated versions its words light up as they are spoken.

**Read first:** the narration was generated on ElevenLabs' free plan, whose terms say it "does not include a commercial license and cannot be used for any commercial purpose" and must be credited with "elevenlabs.io" or "11.ai" in the title. Full wording, the URL and the options are at the top of [`transcripts.md`](transcripts.md). The un-narrated reels carry no ElevenLabs audio.

## Reels, ready for narration (app audio only)

| File | Length | Size | Loudness | What it is |
|---|---|---|---|---|
| `reel-highlights.mp4` | 1:02.1 (62.1 s) | 5.6 MB | -14.2 LUFS / -1.5 dBTP | The combined highlights reel: hook card, three demos (request typed, plan/result on the timeline, a Listen moment each), end card |
| `reel-beatmatched-transition.mp4` | 0:55.0 (55.0 s) | 4.8 MB | -14.2 LUFS / -1.6 dBTP | One-demo reel: beatmatch and blend two tracks (Midnight Drive into Neon Rush), with both Listen moments |
| `reel-extended-club-intro.mp4` | 0:40.3 (40.3 s) | 3.4 MB | -14.2 LUFS / -1.1 dBTP | One-demo reel: extend Solar Flare's intro, the filter that opens into the drop, a Listen moment |
| `reel-mini-mix.mp4` | 0:44.2 (44.2 s) | 3.9 MB | -14.2 LUFS / -1.1 dBTP | One-demo reel: a three-track mini-mix at 124 BPM, both transitions heard |
| `reel-teaser.mp4` | 0:14.0 (14.0 s) | 1.6 MB | -14.2 LUFS / -1.4 dBTP | Teaser: hook, two fast cuts, the best seconds of the transition, end card |

## Narrated versions (word-synced captions, SFX)

| File | Length | Size | Loudness | What it is |
|---|---|---|---|---|
| `reel-highlights-narrated.mp4` | 1:02.1 (62.1 s) | 6.6 MB | -14.1 LUFS / -1.4 dBTP | Highlights reel with narration |
| `reel-beatmatched-transition-narrated.mp4` | 0:55.0 (55.0 s) | 5.7 MB | -14.2 LUFS / -1.8 dBTP | Beatmatch reel with narration |
| `reel-extended-club-intro-narrated.mp4` | 0:40.3 (40.3 s) | 4.1 MB | -14.2 LUFS / -1.6 dBTP | Extended-intro reel with narration |
| `reel-mini-mix-narrated.mp4` | 0:44.2 (44.2 s) | 4.6 MB | -14.2 LUFS / -1.1 dBTP | Mini-mix reel with narration |
| `reel-teaser-narrated.mp4` | 0:14.0 (14.0 s) | 1.8 MB | -14.1 LUFS / -1.3 dBTP | Teaser with narration |
| `reel-highlights-es-narrated.mp4` | 1:02.3 (62.3 s) | 6.5 MB | -14.1 LUFS / -1.4 dBTP | Highlights reel, Spanish narration (the app's own captions stay in English) |
| `demo-dj-beatmatched-transition-narrated.mp4` | 3:07.7 (187.7 s) | 3.6 MB | -14.1 LUFS / -1.7 dBTP | Full website demo, narrated (picture byte-identical to the website video) |
| `demo-dj-extended-club-intro-narrated.mp4` | 1:55.9 (115.9 s) | 2.3 MB | -14.2 LUFS / -1.6 dBTP | Full website demo, narrated |
| `demo-dj-mini-mix-narrated.mp4` | 2:17.8 (137.8 s) | 2.6 MB | -14.2 LUFS / -1.3 dBTP | Full website demo, narrated |

## Captions for upload (.srt, one entry per spoken line)

`demo-dj-beatmatched-transition-narrated.srt`, `demo-dj-extended-club-intro-narrated.srt`, `demo-dj-mini-mix-narrated.srt`, `reel-beatmatched-transition-narrated.srt`, `reel-extended-club-intro-narrated.srt`, `reel-highlights-es-narrated.srt`, `reel-highlights-narrated.srt`, `reel-mini-mix-narrated.srt`, `reel-teaser-narrated.srt`

## Cue files, transcripts, audio

- `reel-*-cues.json`, `demo-*-cues.json`: every caption moment with `start`, `end`, `onscreen`, `max_seconds` (window minus 0.3 s), `max_words` (floor(max_seconds x 2.4)), the narration line and, for a Listen moment, `voice_end` (the voice stops there, the music follows).
- `transcripts.md`: ElevenLabs settings with what is confirmed from the docs and what is not, the free-plan licence terms, and every line per reel and per demo. `transcripts/<cue-id>.txt` is the same text one file per cue, ready to paste (`transcripts/es/` for Spanish).
- `audio/<video>/<cue-id>.mp3`: the generated clips, one per cue (a clip sped up to fit its window is `.wav`). `sfx/`: whoosh, hit, ding.
- `*-narration-timing.json`: per-word start/end seconds from ElevenLabs' character alignment.
- `add-narration.sh <video.mp4> <cues.json> <audio-dir> <out.mp4>`: lays clips at cue starts, ducks, mixes, normalises, copies the video stream. Use it to re-narrate with the owner's own recordings.
- `reel-*-contact.png`, `reel-*-narrated-contact.png`: 12-frame contact sheets.
- `usage.log`: ElevenLabs characters used, per call. `tools/`: the build scripts (they read `ELEVENLABS_API_KEY` from the environment, nothing else).

## Highlights reel, structure

| Time | Cue | Segment | Source (final demo video) |
|---|---|---|---|
| 0.0 to 3.0 s | R01 | hook card |  |
| 3.0 to 8.4 s | R02 | 2 · Beatmatch | `dj-beatmatched-transition.mp4` 37.9 to 43.3 s |
| 8.4 to 13.4 s | R03 | 4 · Blend: crossfade, low-pass on the outgoing track | `dj-beatmatched-transition.mp4` 82.6 to 87.6 s |
| 13.4 to 22.1 s | R04 | Listen: the transition, from 0:12 | `dj-beatmatched-transition.mp4` 90.3 to 99.0 s |
| 22.1 to 27.1 s | R05 | 2 · Extend the intro | `dj-extended-club-intro.mp4` 40.0 to 45.0 s |
| 27.1 to 31.3 s | R06 | 3 · A filter that opens into the drop | `dj-extended-club-intro.mp4` 61.5 to 65.7 s |
| 31.3 to 41.8 s | R07 | Listen: the low end arrives at the drop (0:31) | `dj-extended-club-intro.mp4` 97.5 to 108.0 s |
| 41.8 to 46.4 s | R08 | 1 · Match every track to 124 BPM | `dj-mini-mix.mp4` 13.6 to 18.2 s |
| 46.4 to 50.6 s | R09 | 2 · Sequence with 4-bar overlaps | `dj-mini-mix.mp4` 54.0 to 58.2 s |
| 50.6 to 59.1 s | R10 | Listen: Midnight Drive into Solar Flare (0:23) | `dj-mini-mix.mp4` 89.8 to 98.2 s |
| 59.1 to 62.1 s | R11 | end card |  |
