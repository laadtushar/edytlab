# edytlab social videos: index

> **Licence.** The app footage and the plain reels (`reel-*.mp4`) are covered by the repository's MIT licence. The narration clips are not: everything under `audio/` and the sound effects in `sfx/` were generated with ElevenLabs ([elevenlabs.io](https://elevenlabs.io)) on its free plan. ElevenLabs' terms apply to them: "The free plan does not include a commercial license and cannot be used for any commercial purpose", and a published piece must credit "elevenlabs.io" or "11.ai" in its title ([source](https://elevenlabs.io/docs/help-center/legal/can-i-publish-the-content-i-generate-on-the-platform)). The narrated videos made from those clips are gone (below); when they are regenerated, do it on a paid plan before any commercial use: `tools/` and `add-narration.sh` rebuild every narrated file from the lines in `transcripts/` and the plain reels.

Vertical 1080x1920, 30 fps H.264 (High, yuv420p, CRF 20) with AAC 48 kHz stereo, cut from the three real DJ demo recordings in `website/public/demos/`, as re-filmed in the app's violet look (#581). App audio is untouched, normalised to -14 LUFS / -1 dBTP. The reels carry no narration. Their own text (the caption above the app frame, the hook and end cards) is the website's: Inter SemiBold/Bold on its dark tokens (`website/app/globals.css`), the caption on a card with a violet edge like the captions burned into the demos, 4.5:1 contrast or better. Every added element stays 120 px clear of both sides and above the bottom 20 % of the frame, where the platforms put their buttons (the recording itself runs edge to edge). `tools/brand.py` reads the colours from the site and checks the contrast; the font is Debian/Ubuntu's `fonts-inter` (SIL OFL), not copied into the repo.

**Narrated versions.** The narrated reels and full demos (`*-narrated.mp4`, their `.srt` and `*-narration-timing.json`) were cut and voiced for the earlier, amber-UI recordings, so they were deleted when the demos were re-filmed. They will be regenerated once narration credits are available. Until then post the plain reels below. The lines (`transcripts.md`, `transcripts/`) and the generated clips (`audio/`) are kept as the source for that, but **they were written for the old takes: check every line against the new footage before re-narrating** (the timings changed, and so did a few facts, for example the mini-mix export is now 1:19 and its second Listen caption says 0:46).

## Reels (app audio only)

| File | Length | Size | Loudness | What it is |
|---|---|---|---|---|
| `reel-highlights.mp4` | 1:01.5 (61.5 s) | 6.6 MB | -14.2 LUFS / -1.3 dBTP | The combined highlights reel: hook card, three demos (request typed, plan/result on the timeline, a Listen moment each), end card |
| `reel-beatmatched-transition.mp4` | 0:50.6 (50.6 s) | 5.3 MB | -14.1 LUFS / -1.4 dBTP | One-demo reel: beatmatch and blend two tracks (Midnight Drive into Neon Rush), with both Listen moments |
| `reel-extended-club-intro.mp4` | 0:39.4 (39.4 s) | 4.2 MB | -14.2 LUFS / -1.3 dBTP | One-demo reel: extend Solar Flare's intro, the filter that opens into the drop, a Listen moment |
| `reel-mini-mix.mp4` | 0:42.1 (42.1 s) | 4.4 MB | -14.2 LUFS / -1.3 dBTP | One-demo reel: a three-track mini-mix at 124 BPM, both transitions heard |
| `reel-teaser.mp4` | 0:14.9 (14.9 s) | 2.0 MB | -14.1 LUFS / -1.6 dBTP | Teaser: hook, two fast cuts, the best seconds of the transition, end card |

## Cue files, transcripts, audio

- `reel-*-cues.json`: every moment of a reel with `start`, `end` and `onscreen` text; for a cut from a demo its `source` (demo, caption id, start and end on the demo video), and for a Listen cut `sound`, where the app's sound plays on the reel's timeline (a Listen cut starts at most 0.3 s before the sound and ends inside it). No narration.
- `demo-*-cues.json`: the caption moments of each website demo video, measured on the re-filmed videos: `start`, `end`, `onscreen`, `audio_window` for a Listen, the room each leaves for a voice (`max_seconds` = window minus 0.3 s, `max_words` = floor(`max_seconds` x 2.4), `voice_end` where the app's sound starts) and `narration`, the line written for the old take (see `narration_status`).
- `transcripts.md`: ElevenLabs settings with what is confirmed from the docs and what is not, the free-plan licence terms, and every line per reel and per demo, for the old takes (their windows, not the new ones). `transcripts/<cue-id>.txt` is the same text one file per cue, ready to paste (`transcripts/es/` for Spanish).
- `audio/<video>/<cue-id>.mp3`: the generated clips of the old takes, one per cue (a clip sped up to fit its window is `.wav`). `sfx/`: whoosh, hit, ding.
- `add-narration.sh <video.mp4> <cues.json> <audio-dir> <out.mp4>`: lays clips at cue starts, ducks, mixes, normalises, copies the video stream. Use it to re-narrate with the owner's own recordings.
- `reel-*-contact.png`: 12-frame contact sheets of the reels.
- `tools/`: the build scripts (see its README; narration needs `ELEVENLABS_API_KEY` in the environment, a plain build needs nothing).

## Highlights reel, structure

| Time | Cue | Segment | Source (final demo video) |
|---|---|---|---|
| 0.0 to 2.5 s | R01 | hook card |  |
| 2.5 to 7.3 s | R02 | 2 · Beatmatch | `dj-beatmatched-transition.mp4` 37.0 to 41.8 s |
| 7.3 to 12.1 s | R03 | 4 · Blend: crossfade, low-pass on the outgoing track | `dj-beatmatched-transition.mp4` 74.4 to 79.2 s |
| 12.1 to 20.9 s | R04 | Listen: the transition, from 0:12 | `dj-beatmatched-transition.mp4` 86.1 to 94.8 s |
| 20.9 to 25.7 s | R05 | 2 · Extend the intro | `dj-extended-club-intro.mp4` 43.6 to 48.4 s |
| 25.7 to 30.5 s | R06 | 3 · A filter that opens into the drop | `dj-extended-club-intro.mp4` 58.2 to 63.0 s |
| 30.5 to 39.7 s | R07 | Listen: the low end arrives at the drop (0:31) | `dj-extended-club-intro.mp4` 98.3 to 107.5 s |
| 39.7 to 44.5 s | R08 | 1 · Match every track to 124 BPM | `dj-mini-mix.mp4` 14.3 to 19.1 s |
| 44.5 to 49.3 s | R09 | 2 · Sequence with 4-bar overlaps | `dj-mini-mix.mp4` 40.4 to 45.2 s |
| 49.3 to 58.5 s | R10 | Listen: Midnight Drive into Solar Flare (0:23) | `dj-mini-mix.mp4` 70.0 to 79.3 s |
| 58.5 to 61.5 s | R11 | end card |  |
