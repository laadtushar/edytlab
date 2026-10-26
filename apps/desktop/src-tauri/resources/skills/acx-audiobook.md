---
name: acx-audiobook
description: Measure, clean, level and re-measure an audiobook narration against ACX's rules
trigger: keywords
keywords: [acx, audiobook, audible, narration, narrator, noise floor, rms, peak, qc, quality check]
enabled: true
---

## ACX audiobook check

ACX (Audible, Amazon, Apple Books) rejects a narration file that breaks one of three numbers. These are its published limits (https://help.acx.com/s/article/acx-audio-submission-requirements):

- **RMS** between -23 and -18 dBFS
- **Peak** at or below -3 dBFS
- **Noise floor** at or below -60 dBFS RMS

`measure` reports all three, with the measured value and a pass or fail for each, when it is called with check="acx". It also reports integrated loudness, true peak, clipping (the count and the first times) and the contrast between speech and background. It only reads the session, so it is safe to run as often as needed.

Workflow, in order. Tell the user the numbers after each measurement.

1. **Measure**: `measure` track=0 check="acx". Say which rules fail, by how much, and where `clipping` found anything. If the track is stereo and ACX wants mono, say so; the tool does not check that.
2. **Clean**: if the noise floor fails, `noise_reduction` track=0 noise_duration_sec=1.0 strength=1.0. It learns the noise from the first seconds of the track, so that stretch must be room tone with no speech in it (`noise_floor` in the measurement says where the quietest half second is). If the track does not begin with room tone, ask the user before going on.
3. **Measure again** and repeat step 2 until the noise floor is at least as far under -60 as the level step will add. Raising the level raises the noise by the same amount: a floor of -65 that needs +8 dB of gain ends at -57 and fails. Allow up to four passes; stop early if a pass gains less than 2 dB, and say that the recording is too noisy to fix here.
4. **Level**: the RMS rule carries `gain_to_centre_db` when it fails. Apply it with `gain` track=0 and the value as db. Do not use `normalize_loudness`: ACX measures RMS, not LUFS, and the two differ.
5. **Hold the peaks**: `add_effect` track=0 kind="limiter" with ceiling_db -3.5 in its params. Use the effect, not the `limiter` tool: the effect runs after the track's gain and the tool only sees the file, so it would not see the gain from step 4. The limiter lowers the RMS slightly.
6. **Measure again**: `measure` track=0 check="acx". If the RMS fell out of its band, adjust with `gain` and measure once more. If any rule still fails after two rounds, report the numbers and stop.

Report the final three numbers as pass or fail, and what ACX does not get checked here: the file must be a 192 kbps or higher CBR 44.1 kHz MP3 (export with `render_final`), every file in the book in the same channel format, 0.5 to 5 seconds of room tone at each end, and no clicks, pops or outtakes.

Do not gate or silence the pauses to lower the noise floor. ACX wants natural room tone, and a floor made by gating can pass the number and still be rejected by ear.
