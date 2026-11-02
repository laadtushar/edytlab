---
name: dialog-enhancer
description: Dialog clarity for interviews and conversations
trigger: regex
pattern: \b(dialog(ue)?s?|interviews?|conversations?|clarity|intelligibility)\b
enabled: true
---

## Dialog enhancement

Run steps 1 to 6 on each speaker's track N in turn. Keep each speaker on their own track; `render_final` exports their mix.

1. **Measure.** `measure` track=N. Done when you have `noise_floor.dbfs`, `noise_floor.at_sec`, `rms_dbfs`, `loudness.range_lu` and `contrast.difference_db`.
2. **High-pass.** `high_pass_filter` track=N cutoff_hz=120 takes out handling noise and rumble. Done when it returns a node.
3. **Denoise.** `noise_reduction` track=N noise_from=<`noise_floor.at_sec`> noise_to=<that + 0.5>. Done when `measure` track=N reads a lower `noise_floor.dbfs` than step 1. Skip when step 1 found no noise floor.
4. **Presence.** `eq` track=N bands=[{"freq_hz": 2500, "gain_db": 3, "q": 0.7}] lifts the 1 to 4 kHz range where consonants sit. Done when it returns a node.
5. **Compress.** `compressor` track=N threshold_db=-20 ratio=3.0 attack_ms=15 release_ms=100 evens out the speakers' levels. Done when `measure` track=N reads a lower `loudness.range_lu` than step 1; if it does not, the threshold sat above the speech: say so rather than compressing twice.
6. **Gate.** `noise_gate` track=N threshold_db=-45 closes between sentences, where the other speakers bleed into this microphone. For an audiobook, skip it: ACX wants room tone in the pauses. Done when it returns a node.
7. **Check.** `measure` track=N for each track. Done when every track's `contrast.wcag_20db_pass` is true (speech at least 20 dB over the background). A track that fails gets step 3 once more; then report the numbers, two denoise passes at most.

## Reference

- Steps 2 to 6 rewrite the track's own audio, which plays before its track gain. Thresholds are against that audio: for a recording much quieter than speech usually is, lower threshold_db by how far step 1's `rms_dbfs` (less the track's gain in the session block) sits under -20 dBFS.
- `noise_reduction` learns the noise from noise_from to noise_to, so that stretch must hold no speech; when the quietest stretch is speech, ask the user for a pause.
