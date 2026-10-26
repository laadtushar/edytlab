---
name: acx-audiobook
description: Measure, clean, level and re-measure an audiobook narration against ACX's rules
trigger: keywords
keywords: [acx, audiobook, audible, narration, narrator, noise floor, rms, peak, qc, quality check]
enabled: true
---

## ACX audiobook check

ACX (Audible, Amazon, Apple Books) rejects a narration file that breaks one of these. These are its published limits (https://help.acx.com/s/article/acx-audio-submission-requirements):

- **RMS** between -23 and -18 dBFS
- **Peak** at or below -3 dBFS
- **Noise floor** at or below -60 dBFS RMS
- **No digital silence**: pauses and both ends carry room tone, never exact zeros

`measure` reports all four, with the measured value and a pass or fail for each, when it is called with check="acx". It also reports integrated loudness, true peak, clipping (the count and the first times) and the contrast between speech and background. It only reads the session, so it is safe to run as often as needed.

Workflow, in order. Tell the user the numbers after each measurement.

1. **Measure**: `measure` track=0 check="acx". Say which rules fail, by how much, and where `clipping` found anything. `sample_rate` in the result is the session's: if it is not 44100, see the export step. ACX takes mono or stereo as long as every file in the book matches; the tool does not check that, so ask which the book uses.
   If the `digital_silence` rule fails (a gap, a clip that starts late, a gated pause), say where it is and that ACX wants room tone there, not silence. Fill it with the recording's own room tone, or ask the user for some; do not leave it. The noise floor then reads the room tone, and `gain_to_centre_db` is worked out from the audio outside the silence, because that is what the file will read once the gaps are filled. If the noise floor reads `found: false`, there is no room tone anywhere to measure.
2. **Clean**: if the noise floor fails, `noise_reduction` track=0 noise_duration_sec=1.0 strength=1.0. It learns the noise from the first seconds of the track, so that stretch must be room tone with no speech in it (`noise_floor` in the measurement says where the quietest half second is). If the track does not begin with room tone, ask the user before going on.
3. **Measure again** and repeat step 2 until the noise floor is at least as far under -60 as the level step will add. Raising the level raises the noise by the same amount: a floor of -65 that needs +8 dB of gain ends at -57 and fails. Allow up to four passes; stop early if a pass gains less than 2 dB, and say that the recording is too noisy to fix here.
4. **Level**: the RMS rule carries `gain_to_centre_db` when it fails. Apply it with `gain` track=0 and the value as db. Do not use `normalize_loudness`: ACX measures RMS, not LUFS, and the two differ.
5. **Hold the peaks**: `add_effect` track=0 kind="limiter" with ceiling_db -3.5 in its params. Use the effect, not the `limiter` tool: the effect runs after the track's gain and the tool only sees the file, so it would not see the gain from step 4. The limiter lowers the RMS slightly.
6. **Measure again**: `measure` track=0 check="acx". If the RMS fell out of its band, adjust with `gain` and measure once more. If any rule still fails after two rounds, report the numbers and stop.

7. **Export**: ACX takes a 192 kbps or higher CBR MP3 at 44.1 kHz, and `render_final` has no sample-rate argument: it writes the session's rate. If `measure` showed a `sample_rate` other than 44100, run `resample_track` track=0 target_sample_rate=44100 first (it sets the session's rate too; it resamples by linear interpolation, so tell the user, and offer to leave the conversion to their own tool if quality matters), then measure once more, since resampling can move the numbers. Then `render_final` with format="mp3" and bitrate_kbps=192 (or higher). Whether the MP3 is constant bitrate is not something edytlab checks.

Report the final numbers as pass or fail, and what ACX does not get checked here: the MP3 format above, every file in the book in the same channel format (mono or stereo, as long as they all match), 0.5 to 1 second of room tone at the start and 1 to 5 seconds at the end, and no clicks, pops or outtakes.

Do not gate or silence the pauses to lower the noise floor. ACX wants natural room tone, and a floor made by gating can pass the number and still be rejected by ear.
