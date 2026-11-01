# Narration script: the three DJ demos

One speaker, plain and calm, for `website/public/demos/dj-*.mp4`. Every sentence comes from that demo's caption in
[`website/lib/demos.ts`](../../../website/lib/demos.ts) (the last column quotes the sentence it takes), so each number is one that
caption states (6.7 %, 15.5 s, 47.5 s, 8.77 s, 79 s and so on); the cue names and on-screen words are those of
[`marketing/reels/demo-*-cues.json`](../../reels/). The demos have no `.vtt` or `.srt` (they are 4:3-ish screen recordings
with captions burned in); the cue files are their caption record. Nothing is said that the caption does not say.

Each line sits inside its caption's span, starts at least 0.3 s after the caption appears and, where the app plays its own
sound (the "Listen" captions), ends 0.3 s before that sound starts, so the voice never overlaps the app's audio.

Spoken forms that differ from the screen: "WAV" is spoken "wave file"; "LUFS" is spoken "loofs" (the old narration lines in
`marketing/reels/` used the same spelling); numbers are written out where a reading could vary ("one twenty against one
twenty-eight").

## Beatmatch and blend two tracks (2:45): `dj-beatmatched-transition.mp4`

The narrated mixes of this demo are not in this folder. The BT04 line said both tracks were in F major, which is what the
demo's recording shows on screen and the key detector no longer reads (A major and B major, [#710](https://github.com/laadtushar/edytlab/issues/710)),
so the two mixes, Daniel's voice track, the two BT04 clips and the two metrics files are in
[`archive/demos-narration/2026-10-24-f-major-line/`](../../archive/README.md). The BT04 row below is the new line, not yet
spoken. After the demo is re-recorded, BT's cue times change anyway: re-measure them, make every BT line again with
`tools/gen.py` and build the mixes with `tools/mix.py`.

| Caption (cue span, s) | Narration (as spoken) | Starts | Ends | Source |
|---|---|---|---|---|
| BT01 title (0.0 to 2.5) | Blending two tracks. | 0.40 | 1.60 | website/lib/demos.ts caption of dj-beatmatched-transition (title "Beatmatch and blend two tracks") |
| BT02 open the tracks (2.5 to 10.1) | First, open both tracks: Midnight Drive and Neon Rush. | 2.80 | 6.91 | website/lib/demos.ts caption of dj-beatmatched-transition (names both tracks); on-screen "Open the tracks" |
| BT03 before (120 vs 128 BPM) (10.1 to 14.8) | One twenty against one twenty-eight. | 10.40 | 12.70 | website/lib/demos.ts caption of dj-beatmatched-transition (Neon Rush is about 6.7% faster; Midnight Drive is 120 BPM); on-screen "both tracks from the top, 120 against 128 BPM" |
| BT04 what am I working with (21.9 to 34.9) | Ask what you are working with. Neon Rush is about six point seven percent faster. | 22.50 | (not made yet) | website/lib/demos.ts caption of dj-beatmatched-transition (sentence 1: "Neon Rush is about 6.7% faster") |
| BT05 beatmatch (34.9 to 47.3) | Neon Rush is time-stretched to Midnight Drive's one twenty BPM, keeping its pitch. | 35.20 | 40.63 | website/lib/demos.ts caption of dj-beatmatched-transition (sentence 2: "time-stretched to Midnight Drive's 120 BPM, keeping its pitch") |
| BT06 line up the overlap (47.3 to 59.6) | It starts at sixteen seconds, under the last eight bars. Claude notes its bars then run a beat behind, and offers fifteen and a half seconds instead. | 47.65 | 58.24 | website/lib/demos.ts caption of dj-beatmatched-transition (sentence 2: "started at 16 s, under Midnight Drive's last 8 bars; Claude points out ... a beat behind and offers ... 15.5 s") |
| BT07 blend (59.6 to 82.1) | Midnight Drive fades out under a one kilohertz low-pass, while Neon Rush fades in. | 60.00 | 65.93 | website/lib/demos.ts caption of dj-beatmatched-transition (sentence 3: "fades out under a 1 kHz low-pass while Neon Rush fades in") |
| BT08 listen (transition) (82.1 to 86.3) | Have a listen. | 82.45 | 83.17 | website/lib/demos.ts caption of dj-beatmatched-transition ("every playback is in real time, with sound"); on-screen "Listen: the transition" |
| BT09 master (104.4 to 132.1) | For the master, Claude mixes the two to a new track, compresses it, limits at minus one dB, and raises it to about minus fourteen loofs. | 104.70 | 115.59 | website/lib/demos.ts caption of dj-beatmatched-transition (sentence 4: "mixes the two to a new track, compresses it, limits at -1 dB and raises it to about -14 LUFS") |
| BT10 export (132.1 to 144.9) | Then it exports a fifty-second wave file. | 132.40 | 134.56 | website/lib/demos.ts caption of dj-beatmatched-transition (sentence 4: "exports a 50-second WAV") |
| BT11 listen (mastered mix) (144.9 to 149.0) | The mastered mix. | 145.25 | 146.34 | website/lib/demos.ts caption of dj-beatmatched-transition (sentence 4); on-screen "The mastered mix, from 0:20" |
| BT12 done (161.2 to 165.3) | Minus thirteen point nine loofs. | 161.50 | 163.41 | website/lib/demos.ts caption of dj-beatmatched-transition (sentence 4: "at -13.9 LUFS") |

## Extend an intro for mixing (1:55): `dj-extended-club-intro.mp4`

| Caption (cue span, s) | Narration (as spoken) | Starts | Ends | Source |
|---|---|---|---|---|
| EI01 title (0.0 to 2.4) | A mixable intro. | 0.40 | 1.59 | website/lib/demos.ts caption of dj-extended-club-intro (title "Extend an intro for mixing") |
| EI02 open the track (2.4 to 9.7) | First, open the track: Solar Flare. | 2.80 | 5.55 | website/lib/demos.ts caption of dj-extended-club-intro (names Solar Flare); on-screen "Open the tracks" |
| EI03 before (original intro) (9.7 to 14.5) | First, the original intro. | 10.10 | 11.68 | on-screen "Before: the original intro"; website/lib/demos.ts caption of dj-extended-club-intro ("every playback is in real time, with sound") |
| EI04 tempo and bar length (22.7 to 35.4) | Ask for its tempo and bar length without changing anything: 124 BPM, a 1.935-second bar, and the first downbeat at 0.476 seconds. | 23.05 | 34.92 | website/lib/demos.ts caption of dj-extended-club-intro (sentence 1: "124 BPM, a 1.935-second bar, the first downbeat at 0.476 s") |
| EI05 extend the intro (35.4 to 51.6) | The drums-only first eight bars are copied from that downbeat and pasted after themselves: a sixteen-bar intro, and the track grows from 32 to 47.5 seconds. | 38.50 | 50.99 | website/lib/demos.ts caption of dj-extended-club-intro (sentence 2: "the drums-only first 8 bars are copied ... pasted after themselves, so the intro runs 16 bars and the track grows from 32 to 47.5 seconds") |
| EI06 filter into the drop (51.6 to 66.3) | A 200 hertz high-pass holds the low end back until the drop at 31.45 seconds. Claude suggests 120 to 150 hertz if the intro sounds too thin. | 52.20 | 65.79 | website/lib/demos.ts caption of dj-extended-club-intro (sentence 3: "A 200 Hz high-pass holds the low end back until the drop at 31.45 s, and Claude suggests 120-150 Hz if the intro sounds too thin") |
| EI07 fade in and export (66.3 to 80.5) | The first four bars fade in, and the track is exported as a wave file. Claude flags that it peaks right at full scale. | 66.90 | 75.23 | website/lib/demos.ts caption of dj-extended-club-intro (sentence 4: "The first 4 bars fade in and the track is exported as a WAV; Claude flags that it peaks at -0.0003 dBFS, right at full scale") |
| EI08 listen (intro fades in) (80.5 to 85.1) | Listen to the new intro. | 80.90 | 82.18 | website/lib/demos.ts caption of dj-extended-club-intro (sentence 5: "You hear the new intro"); on-screen "Listen: the new intro fades in" |
| EI09 listen (the drop, 0:31) (94.2 to 98.5) | Wait for the drop. | 94.60 | 95.63 | website/lib/demos.ts caption of dj-extended-club-intro (sentence 5: "then the drop at 0:31"); on-screen "Listen: the low end arrives at the drop (0:31)" |
| EI10 done (110.7 to 114.9) | A sixteen-bar intro, exported. | 111.10 | 113.23 | website/lib/demos.ts caption of dj-extended-club-intro (sentence 2 and 4); on-screen "a 16-bar intro" |

## A three-track mini-mix (1:58): `dj-mini-mix.mp4`

| Caption (cue span, s) | Narration (as spoken) | Starts | Ends | Source |
|---|---|---|---|---|
| MM01 title (0.0 to 2.5) | A three-track mini-mix. | 0.40 | 1.86 | website/lib/demos.ts caption of dj-mini-mix (title "A three-track mini-mix") |
| MM02 open the tracks (2.5 to 10.3) | First, open the three tracks. | 2.80 | 4.69 | website/lib/demos.ts caption of dj-mini-mix (three tracks: Midnight Drive, Neon Rush, Solar Flare); on-screen "Open the tracks" |
| MM03 match to 124 BPM (10.3 to 24.6) | Midnight Drive and Neon Rush are time-stretched to 124 BPM, keeping their pitch. Claude checks that their beats now line up with Solar Flare's. | 11.00 | 21.66 | website/lib/demos.ts caption of dj-mini-mix (sentence 1) |
| MM04 sequence with 4-bar overlaps (24.6 to 46.7) | Each track comes in about four bars before the one before it ends, with a straight-line crossfade over each overlap. Claude brings Neon Rush in on Solar Flare's bar line, which makes that overlap 8.77 seconds. | 25.20 | 41.89 | website/lib/demos.ts caption of dj-mini-mix (sentence 2: "about 4 bars before the one before it ends ... straight-line crossfade ... Neon Rush on Solar Flare's bar line ... overlap 8.77 s") |
| MM05 loudness and limiter (46.7 to 65.5) | Each track is brought to about minus 14 loofs, with a limiter at minus one dB on the master. Claude notes that the fades were counted in the measurement. | 47.20 | 58.69 | website/lib/demos.ts caption of dj-mini-mix (sentence 3: "about -14 LUFS, with a limiter at -1 dB on the master, and Claude notes that the fades were counted in the measurement") |
| MM06 listen (first blend) (65.5 to 70.2) | The first blend. | 65.90 | 66.92 | website/lib/demos.ts caption of dj-mini-mix (sentence 4: "You hear both transitions"); on-screen "Listen: Midnight Drive into Solar Flare (0:23)" |
| MM07 listen (second blend) (83.4 to 87.7) | And the second. | 83.80 | 84.60 | website/lib/demos.ts caption of dj-mini-mix (sentence 4); on-screen "Listen: Solar Flare into Neon Rush (0:46)" |
| MM08 export (100.8 to 114.0) | Then the mix is exported as a 79-second wave file. | 101.20 | 104.85 | website/lib/demos.ts caption of dj-mini-mix (sentence 4: "exported as a 79-second WAV") |

# Checks

Run by `tools/validate.py` on the finished mp4s (`checks/validation.json`).

| Video | Voice | ffprobe (identical to the demo) | LUFS demo to mix | dBTP demo to mix | The app's sound in its Listen windows (RMS dBFS, demo to mix) | Voice over the app's sound |
|---|---|---|---|---|---|---|
| dj-beatmatched-transition | daniel | 1280x776 15/1 fps 2480 fr 165.333 s, h264/High + AAC 48000 Hz | -15.2 to -15.3 | 0.9 to -1.3 | BT03 -12.7 to -13.3; BT08 -15.9 to -15.5; BT11 -15.8 to -15.9 | none |
| dj-extended-club-intro | daniel | 1280x776 15/1 fps 1723 fr 114.867 s, h264/High + AAC 48000 Hz | -18.2 to -18.2 | 1.2 to -1.5 | EI03 -15.4 to -17.7; EI08 -30.8 to -33.1; EI09 -17.4 to -19.7 | none |
| dj-mini-mix | daniel | 1280x776 15/1 fps 1770 fr 118.000 s, h264/High + AAC 48000 Hz | -14.4 to -14.5 | 0.2 to -1.3 | MM06 -14.5 to -15.1; MM07 -13.9 to -14.3 | none |
| dj-beatmatched-transition | river | 1280x776 15/1 fps 2480 fr 165.333 s, h264/High + AAC 48000 Hz | -15.2 to -15.3 | 0.9 to -1.4 | BT03 -12.7 to -13.2; BT08 -15.9 to -15.5; BT11 -15.8 to -15.7 | none |
| dj-extended-club-intro | river | 1280x776 15/1 fps 1723 fr 114.867 s, h264/High + AAC 48000 Hz | -18.2 to -18.2 | 1.2 to -2.2 | EI03 -15.4 to -18.2; EI08 -30.8 to -33.6; EI09 -17.4 to -20.2 | none |
| dj-mini-mix | river | 1280x776 15/1 fps 1770 fr 118.000 s, h264/High + AAC 48000 Hz | -14.4 to -14.6 | 0.2 to -1.4 | MM06 -14.5 to -14.6; MM07 -13.9 to -13.7 | none |

The demo masters peak above full scale (+0.9, +1.2 and +0.2 dBTP, measured on the files), so a mix cannot match their peak:
each mix is held to -1 dBTP instead and keeps the demo's integrated loudness, which in `dj-extended-club-intro` takes the
app's own sound down 2.3 dB (Daniel) or 2.8 dB (River). The picture of each mix is the demo's own stream, copied.
