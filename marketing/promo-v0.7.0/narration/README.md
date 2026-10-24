# Narration for the v0.7.0 promo

A calm, single-speaker voice-over for the five finished videos in [`../out/`](../out/): the 16:9 cut, its 1:1 twin (same
timeline, so the same voice track) and the three 9:16 reels. The words come only from [`../claims.md`](../claims.md); each
sentence is traced to its row in [`narration-script.md`](narration-script.md). The narration says nothing about stem
separation (still not built) and nothing the footage does not show; every line was checked against `main` on 2026-10-24 (v0.8.0),
and none says or implies that something is unbuilt.

**Voice: synthetic, made with ElevenLabs (`eleven_multilingual_v2`, voices Daniel and River). The owner confirmed with ElevenLabs that this open-source (MIT) project may use this output.** Made on 2026-10-24 (see "Made with" below). Say that the voice is synthetic when you post one of these files.

## Made with

| | |
|---|---|
| Service | ElevenLabs text to speech, REST API (`POST /v1/text-to-speech/{voice_id}`, `mp3_44100_128`) |
| Model | `eleven_multilingual_v2`, 1 credit per character. ElevenLabs recommends it, with `eleven_v4`, for "content creation: professional content, audiobooks & video narration" ([models](https://elevenlabs.io/docs/overview/models)). Flash v2.5 costs half (0.5 credit per character through the API, <https://elevenlabs.io/pricing>, <https://elevenlabs.io/pricing/api>: $0.04 against $0.08 per 1,000 characters) but is the real-time model; the credit allowed the better one |
| Voices | **Daniel** (`onwK4e9ZLuTAKqWW03F9`, "Steady Broadcaster", British): the default, in all five videos. **River** (`SAz9YHcvj6GT2YYXdXww`, "Relaxed, Neutral, Informative"): an alternative, all five tracks, and the 16:9 mix. Both are premade voices of the account; no voice was cloned or designed. `voice-samples/` has the opening two sentences in Daniel, River and Sarah (`EXAVITQu4vr4xnSDxMaL`, "Mature, Reassuring, Confident") so the voice can be chosen by ear |
| Settings | stability 0.6, similarity 0.75, style 0, speaker boost on, speed 1.0; language `en`. Each line is one request |
| Credits | The key held 4,347 characters of credit before the first request. This folder used 3,872: 345 for the three voice samples, 1,827 for Daniel (1,781 for the script and 46 for one line made again) and 1,700 for River, both counted by the characters sent |

## What is in here

| Path | What |
|---|---|
| `daniel/edytlab-v0.7.0-<cut>-narrated-daniel.mp4` | The five videos with Daniel under them. The video stream is copied from `../out/` untouched (same frames, resolution, rate and length); only the sound is new |
| `daniel/narration-<cut>-daniel.wav` | The voice track alone (mono, 48 kHz, 16-bit), as long as its video, silent between lines. The 1:1 video uses the 16:9 track |
| `river/` | The same for River: the 16:9 mix and the four tracks |
| `voice-samples/` | Three voices, the same two sentences |
| `narration-script.md` | The script, shot by shot, with start and end times and the `claims.md` row each sentence comes from |
| `lines/<voice>/*.mp3` | Each line as ElevenLabs returned it, so any mix can be rebuilt with no credits. One exception: `v1-runtime.mp3` is the first sentence only, cut at the pause with a 30 ms fade (the second sentence went stale in v0.8.0); `v1-runtime.full.mp3` is what ElevenLabs returned |
| `checks/` | The measured results (`validation.json`) and one `metrics-*.json` per mix |
| `tools/` | The scripts: `gen.py` (requests, with a spend ledger and a stop at 150 credits), `fit.py` (does each line fit its shot), `mix.py` (lay, duck, master, mux), `validate.py` (the checks), `prof.py`, `script.py` (the lines and their windows), `el.py` |

`<cut>` is `16x9`, `1x1`, `whats-new-9x16`, `describe-the-edit-9x16` or `200-tracks-9x16`. The mp4s are stored as plain files,
as the videos in `../out/` and the rest of `marketing/` are (no Git LFS).

## How the mix is made

`tools/mix.py` runs on the finished videos in `../out/`; nothing is re-rendered. Each line is trimmed of its silence, sped up
by at most 1.11x if it would not fit its shot (two lines in the 16:9, one in two of the reels; the rest at 1.0), levelled to
the same loudness and laid at its start time. The cut's own sound is ducked under each line (-12 dB for the UI sounds, -11 dB
for the clips that play the app's own audio, with an 80 ms attack and a 350 ms release), the sum is
mastered with ffmpeg's two-pass `loudnorm` to the **same integrated loudness the cut had** (a limiter at -2 dBFS first only
where the voice made the mix quieter than that), and the AAC (160 kb/s, 48 kHz stereo) is muxed with the original video. The
file is measured after encoding, and the true peak is brought under -1 dBTP.

The two stretches where the cut plays the app's own sound (S1's playback and S7's, in the 16:9; S7's in the first reel; S1's in
the second) get only one or two short lines, ducked by 11 dB, so the music stays audible beneath them. A word the voice
says differently from how it is written on screen is spelled as it is said in `script.py`: the "ONNX Runtime" is spoken "Onyx
Runtime", and the version number as "zero point seven point zero".

## Voice, licence and attribution

Voice: synthetic, made with ElevenLabs (`eleven_multilingual_v2`, voices Daniel and River). The owner confirmed with ElevenLabs that this open-source (MIT) project may use this output. The footage, the Remotion edit and the scripts are MIT as the rest of the repository.
