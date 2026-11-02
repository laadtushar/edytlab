---
name: acx-audiobook
description: ACX audiobook: a narration measured, fixed and exported to ACX's rules
trigger: regex
pattern: \b(acx|audible|audio ?books?|narrat(or|ion)s?)\b
enabled: true
---

## ACX audiobook

Track 0 is the narration. `measure` track=0 check="acx" grades ACX's four rules: RMS, peak, noise floor and digital silence. Tell the user the numbers after each measurement.

1. **Measure.** `measure` track=0 check="acx". Done when you have told the user each rule's measured value and pass or fail, any `clipping` events, and `sample_rate`, and have asked whether the book is mono or stereo (every file in it must match).
   - `digital_silence` fails: `silence_finder` track=0 threshold_db=-120 min_silence_ms=10 lists where. Tell the user ACX wants room tone there and ask for room tone or a re-record to fill it; carry on with the other rules meanwhile.
2. **Clean** (noise floor fails). `noise_reduction` track=0 noise_from=<`noise_floor.at_sec`> noise_to=<that + 0.5> strength=1.0: it learns the noise from the quietest half second the measurement found. Done when it returns a node.
3. **Measure again**, and repeat step 2 with the new `noise_floor.at_sec`. Done when the noise floor plus the RMS rule's `gain_to_centre_db` is at or under -60, because levelling raises the noise by the same gain. Four passes at most; when a pass lowers the floor by less than 2 dB, stop and tell the user the recording is too noisy to fix here.
4. **Level.** `gain` track=0 db=<the RMS rule's `gain_to_centre_db`>. Done when it returns a node.
5. **Hold the peaks.** `add_effect` track=0 kind=limiter params={"ceiling_db": -3.5}, or `set_effect_params` on the limiter the session block already lists there. Done when the session block lists one limiter, last in track 0's chain.
6. **Measure again.** `measure` track=0 check="acx". Done when `acx.pass` is true. The limiter lowers the RMS slightly: if RMS left its band, `gain` track=0 db=<the new `gain_to_centre_db`> and measure once more. After two rounds, report the numbers and stop.
7. **Export.** ACX takes MP3 at 192 kbps or more, 44.1 kHz. When `sample_rate` is not 44100, first `resample_track` track=0 target_sample_rate=44100 (it sets the session's rate too, and resamples by linear interpolation: tell the user, and offer to leave the conversion to their own tool when quality matters), then repeat step 6. Then `render_final` node_id=<the session block's head node> format="mp3" bitrate_kbps=192 out_path=<the user's path>. Done when the result's `sample_rate` is 44100.

Report the final value of each rule, then the measurement's `not_checked` list: those are the user's to check.

## Reference

- **Level by RMS.** ACX measures RMS, so level with `gain` and the measurement's `gain_to_centre_db`. `normalize_loudness` sets LUFS, which is a different number.
- **The limiter effect, not the limiter tool.** The effect runs after the track's gain; the tool bakes into the file and never sees the gain from step 4.
- **Pauses keep their room tone.** Lower the floor with `noise_reduction` only. Leave `noise_gate` out: a gated floor can pass the number and still fail ACX's listening check.
- Source of the rules: https://help.acx.com/s/article/acx-audio-submission-requirements
