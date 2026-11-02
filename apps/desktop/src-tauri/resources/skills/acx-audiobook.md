---
name: acx-audiobook
description: ACX audiobook: a narration measured, fixed and exported to ACX's rules
trigger: regex
pattern: \b(acx|audible|audio ?books?|narrat(or|ion)s?)\b
enabled: true
---

## ACX audiobook

Track 0 is the narration. Tell the user the numbers after each measurement. The session block was written at the start of this turn: for anything changed since, trust the tool results.

1. **Measure.** `measure` track=0 check="acx". Done when you have told the user each rule's measured value and pass or fail, any `clipping` events, and `sample_rate`, and have asked whether the book is mono or stereo (every file in it must match).
   - `digital_silence` fails: `silence_finder` track=0 threshold_db=-120 min_silence_ms=10 lists where on track 0. Zeros after track 0 ends, when another track runs longer, count too but are not listed: say so. Tell the user ACX wants room tone there and ask for room tone or a re-record to fill it; carry on with the other rules meanwhile.
   - `noise_floor.found` is false: there is no room tone anywhere to learn from. Ask the user for room tone, and skip steps 2 and 3.
2. **Noise print** (noise floor fails). Done when the half second from `noise_floor.at_sec` is known to be a pause, not quiet speech: either `contrast.difference_db` is 20 dB or more, or the user has confirmed it or named another room-tone stretch. Keep this stretch for every pass of step 3: once the noise is lower, the quietest half second can move onto quiet speech.
3. **Clean.** `noise_reduction` track=0 noise_from=<the stretch's start> noise_to=<its end> strength=1.0, then `measure` track=0 check="acx"; repeat on the same stretch. Done when the noise floor plus the RMS rule's `gain_to_centre_db` (0 when RMS passes and the field is absent) is at or under -60, because levelling raises the noise by the same gain. Four passes at most; when a pass lowers the floor by less than 2 dB, stop and tell the user the recording is too noisy to fix here.
4. **Level** (RMS fails). `gain` track=0 db=<the RMS rule's `gain_to_centre_db`>. Done when it returns a node.
5. **Hold the peaks.** If the session block lists a limiter on track 0, or you added one this turn, set its ceiling with `set_effect_params` track=0 effect_index=<its index> params={"ceiling_db": -3.5}. Otherwise `add_effect` track=0 kind=limiter params={"ceiling_db": -3.5}, and later edits use its result's `position` as effect_index. Done when track 0 has one limiter, last in its chain: the `add_effect` result's `position` is `effect_count` − 1, or the session block lists the existing one last.
6. **Measure again.** `measure` track=0 check="acx". Done when `acx.pass` is true. The limiter lowers the RMS slightly: if RMS left its band, `gain` track=0 db=<the new `gain_to_centre_db`> and measure once more. After two rounds, report the numbers and stop.
7. **Export.** ACX takes MP3 at 192 kbps or more, 44.1 kHz.
   - When `sample_rate` is not 44100: `resample_track` track=0 target_sample_rate=44100 (linear interpolation: tell the user, and offer to leave the conversion to their own tool when quality matters), then repeat step 6. If `sample_rate` still is not 44100, track 0 was already at 44.1 kHz and the session's rate comes from elsewhere: stop and tell the user.
   - `render_final` node_id=<the node id of this turn's last edit, or the session block's head node if nothing changed this turn> format="mp3" bitrate_kbps=192 out_path=<the user's path>. Done when the result's `sample_rate` is 44100.

Report the final value of each rule, then the measurement's `not_checked` list: those are the user's to check.

## Reference

- **Level by RMS.** ACX measures RMS, so level with `gain` and `gain_to_centre_db`. `normalize_loudness` sets LUFS, which is a different number.
- **The limiter effect, not the limiter tool.** The effect runs after the track's gain; the tool bakes into the file and never sees the gain from step 4.
- **Pauses keep their room tone.** Lower the floor with `noise_reduction` only. Leave `noise_gate` out: a gated floor can pass the number and still fail ACX's listening check.
- Source of the rules: https://help.acx.com/s/article/what-are-the-acx-audio-submission-requirements
