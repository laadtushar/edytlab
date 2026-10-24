# Narration script, v0.7.0 promo

One speaker, plain and calm. Every sentence below sits inside one shot of its video, starts after the shot has faded in and
ends before it fades out, and comes from a row of [`../claims.md`](../claims.md) (the last column). Times are seconds on the
video's own clock, as laid in the Daniel mix; the River mix lays the same lines at the same start times. Every line was also
checked against `main` on 2026-10-24 (v0.8.0, which ships transcription): none says or implies that something is unbuilt.

Spoken forms that differ from the screen: "ONNX Runtime" is spoken "Onyx Runtime"; "v0.7.0" is spoken "version zero point
seven point zero"; "list_session" is spoken "list session". Nothing is said about stem separation, which is still not built,
or about the speech model: the S5 line says only that the runtime ships in every installer (an earlier take went on to say
"Groundwork only, no model yet", which stopped being true when v0.8.0 made the model downloadable; the line was cut at the
pause between the two sentences, with a 30 ms fade, so no audio was regenerated).

The 1:1 video uses the 16:9 script and track unchanged (the two have one timeline).


## 16:9 (`edytlab-v0.7.0-16x9.mp4`, 76.5 s)

| # | Shot | Narration (as spoken) | Starts | Ends | Traced to `claims.md` |
|---|---|---|---|---|---|
| 1 | open (S8a b-roll) (0.0 to 3.8 s) | edytlab, version zero point seven point zero. | 0.40 | 3.45 | Edit table: title "edytlab v0.7.0"; end card "v0.7.0" |
| 2 | describe (S1) (3.5 to 21.5 s) | Describe the edit in plain English. | 3.90 | 6.20 | S1 row 1 (plain-English request typed in the real composer); edit table "Describe the edit in plain English" |
| 3 | describe (S1) (3.5 to 21.5 s) | The assistant calls fade, then gain, and the timeline updates. | 8.20 | 12.47 | S1 rows 2-4 (Fade then Gain chips; backing fades in over 4 s; vocal lane reads -3.0 dB); edit table "The assistant calls the tools, the timeline updates" |
| 4 | describe (S1) (3.5 to 21.5 s) | Press play to hear it. The audio is a demo track, made in code. | 13.50 | 17.92 | S1 row "Play"; edit table "Press play and hear it" / "demo audio, made in code"; All audio row (synthetic) |
| 5 | faders (S2) (21.2 to 26.2 s) | Faders stay where you put them: four key taps, four steps. | 21.60 | 25.73 | S2 row (#596/#626: readout steps +0.5 to +2.0 dB and never goes back); edit table "Faders that stay where you put them" |
| 6 | graph intro (S8b) (25.9 to 29.9 s) | Every edit is a node in the history graph. | 26.25 | 29.01 | Edit table "Every edit is a node in the history graph" (README "saves each change as a node"); S8 row (History graph) |
| 7 | graph (S3) (29.6 to 36.8 s) | Name a version: it shows at once. | 29.95 | 32.13 | S3 row 2 (name_node; #602/#625; bubble reads "chorus"); edit table "Name a version: it shows at once" |
| 8 | graph (S3) (29.6 to 36.8 s) | Compact the history: twenty nodes become eight. | 33.15 | 36.49 | S3 row 3 (compact_session; 20 bubbles became 8); edit table "Compact the history: nodes leave at once", "20 nodes became 8" |
| 9 | big sessions (S4) (36.5 to 48.1 s) | Two hundred tracks? Just ask. | 36.95 | 39.02 | S4 row 1 (200 TRACKS in the toolbar); edit table "200 tracks? Just ask." |
| 10 | big sessions (S4) (36.5 to 48.1 s) | It reads what the summary left out with list session, then lowers twelve strings by six decibels. | 39.60 | 45.95 | S4 row 2 (list_session, #618/#634, then twelve gain calls, db -6 chosen by Claude); S4 row 4 (lanes read -6.0 dB); edit table "list_session reads what the summary left out", "Twelve strings, 6 dB quieter" |
| 11 | runtime (S5) (47.8 to 53.8 s) | The Onyx Runtime ships in every installer. | 48.15 | 51.15 | S5 row 1 (ONNX Runtime 1.23.2 (bundled); every release installer after v0.6.1, #383/#615) |
| 12 | skills (S6) (53.5 to 58.9 s) | Bundled skills update when a release changes them, unless edited. | 53.85 | 58.08 | S6 row 1 (SkillsEditor note, #574/#631); edit table "Bundled skills update, unless you edited them" / "Updated when a release changes them" |
| 13 | mix (S7) (58.6 to 71.4 s) | Switch tabs, press play: it still plays. | 59.10 | 62.08 | S7 row 1 (#607/#635: Play, Graph tab and back, Play again with no error); edit table "Switch tabs, press play: it still plays" |
| 14 | mix (S7) (58.6 to 71.4 s) | Even when the mix is gone from the cache, the app renders it again. | 66.05 | 70.20 | S7 row 2 (the story deletes the mix file from the preview cache; the app renders it again once); edit table "Even when the mix is gone from the cache" |
| 15 | end card (71.1 to 76.5 s) | Free and open source. Installers are unsigned. | 71.65 | 75.31 | Edit table, end card row (LICENSE MIT; README macOS/Windows/Linux builds; installers unsigned); "Not shown" section |

## What's new reel (`edytlab-v0.7.0-whats-new-9x16.mp4`, 39.5 s)

| # | Shot | Narration (as spoken) | Starts | Ends | Traced to `claims.md` |
|---|---|---|---|---|---|
| 1 | hook (S8a) (0.0 to 2.4 s) | Five changes you can see. | 0.35 | 1.83 | Edit table "What's new in v0.7.0 / Five changes you can see" (R1 shows five changes) |
| 2 | faders (S2) (2.1 to 6.9 s) | Faders stay where you put them: four key taps, four steps. | 2.50 | 6.57 | S2 row (#596/#626); edit table "Four key taps, four steps" |
| 3 | graph (S3) (6.6 to 13.8 s) | The history graph keeps up: names and clean-ups show at once. Twenty nodes became eight. | 7.00 | 13.47 | S3 rows 2-3 (#602/#625; 20 bubbles became 8); edit table "The history graph keeps up / Names and clean-ups show at once" |
| 4 | big sessions (S4) (13.6 to 23.4 s) | Two hundred tracks? Just ask. List session reads what the summary left out, then twelve strings drop six decibels. | 13.95 | 22.94 | S4 rows 1,2,4 (200 TRACKS; list_session #618/#634; twelve gain calls at -6 dB); edit table "200 tracks? Just ask." / "list_session pages past the summary" |
| 5 | mix (S7) (23.1 to 30.7 s) | Switch tabs and it still plays, even when the mix is gone from the cache. | 23.45 | 28.31 | S7 rows 1-2 (#607/#635); edit table "Switch tabs. It still plays." / "Even when the mix is gone from the cache" |
| 6 | skills (S6) (30.4 to 35.0 s) | Bundled skills stay current, unless you edited them. | 30.75 | 34.33 | S6 row 1 (#574/#631); edit table "Bundled skills stay current" / "unless you edited them" |
| 7 | end card (34.7 to 39.5 s) | Free and open source. Installers are unsigned. | 35.10 | 38.76 | Edit table end card (MIT; unsigned installers); "Not shown" section |

## Describe the edit reel (`edytlab-v0.7.0-describe-the-edit-9x16.mp4`, 26.1 s)

| # | Shot | Narration (as spoken) | Starts | Ends | Traced to `claims.md` |
|---|---|---|---|---|---|
| 1 | hook (S1) (0.0 to 1.8 s) | Describe the edit. | 0.35 | 1.43 | Edit table "Describe the edit." (S1) |
| 2 | type (S1) (1.8 to 5.4 s) | Say it in plain English. | 2.20 | 3.50 | S1 row 1; edit table "Say it in plain English / Typed into the chat box" |
| 3 | tools (S1) (5.4 to 11.4 s) | The assistant calls fade, then gain, and the timeline changes. | 6.20 | 10.33 | S1 rows 2-4 (Fade and Gain chips; fade in over 4 s; vocal lane -3.0 dB); edit table "The assistant calls the tools" / "The timeline changes" |
| 4 | play (S1) (11.4 to 22.6 s) | Press play to hear it. The audio is a demo track, made in code. | 12.00 | 16.91 | S1 row "Play"; edit table "Then hear it / A synthetic demo track / demo audio, made in code"; All audio row |
| 5 | end card (22.3 to 26.1 s) | edytlab is free and open source. | 22.70 | 25.09 | Edit table end card ("Free and open source (MIT)") |

## 200 tracks reel (`edytlab-v0.7.0-200-tracks-9x16.mp4`, 27.4 s)

| # | Shot | Narration (as spoken) | Starts | Ends | Traced to `claims.md` |
|---|---|---|---|---|---|
| 1 | hook (S4) (0.0 to 3.5 s) | Two hundred tracks? Just ask. | 0.40 | 2.45 | S4 row 1 (200 TRACKS); edit table "200 tracks?" / "Ask." |
| 2 | ask (S4) (3.2 to 6.7 s) | Make all the strings quieter. | 3.60 | 5.31 | S4 row 2 (the request typed in the footage: "make all the strings quieter") |
| 3 | pages (S4) (6.7 to 16.2 s) | It reads what the summary left out with list session, then makes twelve gain calls, each one a node on the graph. | 7.05 | 14.68 | S4 row 2 (list_session once, offset 140; twelve gain calls); edit table "It reads what the summary left out" / "Each change is a node on the graph" |
| 4 | result (S4) (15.9 to 23.5 s) | All twelve strings are now six decibels quieter: violins, viola, cello and contrabass. | 16.25 | 23.07 | S4 rows 3-4 (twelve strings, tracks 147-158, read -6.0 dB; Claude's reply lists Cello, Contrabass, Viola, Violin I and II); edit table "Violins, viola, cello, contrabass" |
| 5 | end card (23.2 to 27.4 s) | edytlab is free and open source. | 23.60 | 25.53 | Edit table end card ("Free and open source (MIT)") |


# Checks

Run by `tools/validate.py` on the finished mp4s; the numbers are in `checks/validation.json`.

## Picture and sound format, against the cut in `../out/`

| Video | Voice | Size, rate, frames, length | Codecs | Loudness (cut to mix) | True peak (cut to mix) |
|---|---|---|---|---|---|
| `16x9` | daniel | 1920x1080, 30/1 fps, 2294 frames, 76.467 s (identical to the cut: yes) | h264/High + aac 48000 Hz, 2 ch | -14.1 to -14.1 LUFS | -1.2 to -1.1 dBTP |
| `1x1` | daniel | 1080x1080, 30/1 fps, 2294 frames, 76.467 s (identical to the cut: yes) | h264/High + aac 48000 Hz, 2 ch | -14.1 to -14.1 LUFS | -1.2 to -1.1 dBTP |
| `whats-new-9x16` | daniel | 1080x1920, 30/1 fps, 1184 frames, 39.467 s (identical to the cut: yes) | h264/High + aac 48000 Hz, 2 ch | -13.9 to -13.9 LUFS | -1.1 to -1.2 dBTP |
| `describe-the-edit-9x16` | daniel | 1080x1920, 30/1 fps, 782 frames, 26.067 s (identical to the cut: yes) | h264/High + aac 48000 Hz, 2 ch | -14.0 to -13.9 LUFS | -1.1 to -1.4 dBTP |
| `200-tracks-9x16` | daniel | 1080x1920, 30/1 fps, 821 frames, 27.367 s (identical to the cut: yes) | h264/High + aac 48000 Hz, 2 ch | -14.0 to -14.0 LUFS | -2.0 to -1.2 dBTP |
| `16x9` | river | 1920x1080, 30/1 fps, 2294 frames, 76.467 s (identical to the cut: yes) | h264/High + aac 48000 Hz, 2 ch | -14.1 to -14.2 LUFS | -1.2 to -1.5 dBTP |
| `1x1` | river | 1080x1080, 30/1 fps, 2294 frames, 76.467 s (identical to the cut: yes) | h264/High + aac 48000 Hz, 2 ch | -14.1 to -14.2 LUFS | -1.2 to -1.5 dBTP |
| `whats-new-9x16` | river | 1080x1920, 30/1 fps, 1184 frames, 39.467 s (identical to the cut: yes) | h264/High + aac 48000 Hz, 2 ch | -13.9 to -14.1 LUFS | -1.1 to -2.2 dBTP |
| `describe-the-edit-9x16` | river | 1080x1920, 30/1 fps, 782 frames, 26.067 s (identical to the cut: yes) | h264/High + aac 48000 Hz, 2 ch | -14.0 to -14.1 LUFS | -1.1 to -1.3 dBTP |
| `200-tracks-9x16` | river | 1080x1920, 30/1 fps, 821 frames, 27.367 s (identical to the cut: yes) | h264/High + aac 48000 Hz, 2 ch | -14.0 to -14.2 LUFS | -2.0 to -2.0 dBTP |

Every mix is within 0.2 LU of its cut's integrated loudness and at or under -1.1 dBTP (the limit set was -1 dBTP).
The picture of each is the cut's own stream, copied.

## The cut's own sound under the voice

Level (dBFS RMS) over each line that overlaps a stretch where the cut plays the app's own audio, measured on the finished file:
the cut before the voice, what is left of it under the voice, and the voice.

| Video | Voice | Line | Cut alone | Cut under the voice | Voice |
|---|---|---|---|---|---|
| `16x9` | daniel | v1-describe-c | -14.8 | -22.5 | -19.5 |
| `16x9` | daniel | v1-mix-a | -13.5 | -22.4 | -20.1 |
| `16x9` | daniel | v1-mix-b | -14.0 | -22.0 | -19.6 |
| `1x1` | daniel | v1-describe-c | -14.8 | -22.5 | -19.5 |
| `1x1` | daniel | v1-mix-a | -13.5 | -22.4 | -20.1 |
| `1x1` | daniel | v1-mix-b | -14.0 | -22.0 | -19.6 |
| `whats-new-9x16` | daniel | r1-mix | -17.9 | -24.5 | -19.7 |
| `describe-the-edit-9x16` | daniel | r2-play | -18.6 | -24.8 | -20.0 |
| `16x9` | river | v1-describe-c | -15.8 | -24.3 | -20.6 |
| `16x9` | river | v1-mix-a | -13.4 | -22.6 | -20.7 |
| `16x9` | river | v1-mix-b | -14.0 | -22.8 | -20.4 |
| `1x1` | river | v1-describe-c | -15.8 | -24.3 | -20.6 |
| `1x1` | river | v1-mix-a | -13.4 | -22.6 | -20.7 |
| `1x1` | river | v1-mix-b | -14.0 | -22.8 | -20.4 |
| `whats-new-9x16` | river | r1-mix | -17.4 | -25.2 | -20.7 |
| `describe-the-edit-9x16` | river | r2-play | -23.0 | -27.6 | -21.0 |

The app's sound stays 4 to 9 dB below where it was and 2 to 7 dB below the voice, above the -38 dBFS floor the check uses, so it
is heard under the voice and comes back to full level between lines. The 9:16 `200-tracks` reel has no app audio (its sound is
UI sounds only).
