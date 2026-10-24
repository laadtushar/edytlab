# Narration for the three DJ demos

A calm, single-speaker voice-over for the demo videos on the website, `website/public/demos/dj-beatmatched-transition.mp4`,
`dj-extended-club-intro.mp4` and `dj-mini-mix.mp4` (real Claude driving the real app, recorded by
`apps/desktop/e2e-native/stories/demos.mjs`). The words come only from each demo's caption in
[`website/lib/demos.ts`](../../../website/lib/demos.ts); the cue spans are
[`marketing/reels/demo-*-cues.json`](../../reels/). Each sentence is traced to its caption sentence in
[`narration-script.md`](narration-script.md). The voice never overlaps the app's own sound (the "Listen" captions): every
line ends 0.3 s before it starts. The files in `website/public/demos/` are not touched.

**Voice: synthetic, made with ElevenLabs (`eleven_multilingual_v2`, voices Daniel and River). The owner confirmed with ElevenLabs that this open-source (MIT) project may use this output.** Made on 2026-10-24. Say that the voice is synthetic when you post one of these files.

## Made with

| | |
|---|---|
| Service, model | ElevenLabs text to speech, REST API, `eleven_multilingual_v2` (1 credit per character; the model ElevenLabs recommends, with `eleven_v4`, for "video narration", <https://elevenlabs.io/docs/overview/models>) |
| Voices | **Daniel** (`onwK4e9ZLuTAKqWW03F9`, British, "Steady Broadcaster") is the default; **River** (`SAz9YHcvj6GT2YYXdXww`, "Relaxed, Neutral, Informative") is an alternative. Premade voices only. The promo folder (`marketing/promo-v0.7.0/narration/voice-samples/`) has both, and Sarah, saying the same two sentences |
| Settings | stability 0.6, similarity 0.75, style 0, speaker boost on, speed 1.0, language `en`; one request per line |
| Credits | 2,106 characters per voice, 4,212 in all. Daniel's first six lines (319 characters) came from the first key, which that left at 156 credits (the floor was 150); the rest of Daniel's lines (1,787) and all of River's (2,106) came from the second key, which the API reported at 9,912 before the first request and 5,931 after the last. |

## What is in here

| Path | What |
|---|---|
| `daniel/<demo>-narrated-daniel.mp4` | The three demos with Daniel under them. The picture is the demo's own stream, copied; the sound is new |
| `daniel/narration-<demo>-daniel.wav` | Daniel's voice alone (mono, 48 kHz, 16-bit), as long as the demo, silent between lines |
| `river/<demo>-narrated-river.mp4` | The same with River |
| `narration-script.md` | The script by caption, with start and end times, the caption sentence each line comes from, and the checks |
| `lines/<voice>/*.mp3` | Each line as ElevenLabs returned it, so a mix can be rebuilt with no credits |
| `checks/` | `validation.json` and one `metrics-*.json` per mix |
| `tools/` | `gen.py` (requests; a spend ledger; stops at 150 credits left), `fit.py`, `mix.py` (lay, master, mux), `validate.py`, `prof.py`, `script.py` (the lines and when each starts), `el.py` (reads the key from the file `ELEVENLABS_KEY_FILE` names) |

`<demo>` is `dj-beatmatched-transition`, `dj-extended-club-intro` or `dj-mini-mix`. The mp4s are plain files, as every video
in `marketing/` is (no Git LFS). River's voice tracks are not kept (about 38 MB of mostly silence); `tools/mix.py` rebuilds them
from `lines/river/`.

## How the mix is made

The demos are silent except where the app plays its own sound, so the voice sits in the silences. Each line is trimmed of its
silence, sped up by at most 1.08x where it would not fit its caption (two Daniel lines in `dj-extended-club-intro`), levelled
and laid at its start time. The sum is mastered with ffmpeg's two-pass `loudnorm` to **the demo's own integrated loudness**,
and the AAC (160 kb/s, 48 kHz stereo) is muxed with the demo's video stream, unchanged (1280x776, 15 fps, same frame count and
length). Results are in `narration-script.md` and `checks/`.

The demo masters peak above full scale (+0.9, +1.2 and +0.2 dBTP, as the files stand), so a mix cannot match their peak: each
is held to -1 dBTP and keeps the loudness, which takes the app's sound in `dj-extended-club-intro` down 2.3 dB (Daniel) or
2.8 dB (River). The Listen windows stay within 3 dB of the demo's own level in every mix.

## Voice, licence and attribution

Voice: synthetic, made with ElevenLabs (`eleven_multilingual_v2`, voices Daniel and River). The owner confirmed with ElevenLabs that this open-source (MIT) project may use this output. The footage is MIT as the rest of the repository.
