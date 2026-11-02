---
name: dialog-enhancer
description: Dialog clarity for interviews and conversations
trigger: regex
pattern: \b(dialog(ue)?s?|interviews?|conversations?|clarity|intelligibility)\b
enabled: true
---

## Dialog enhancement

Run steps 1 to 7 on each speaker's track N in turn, then step 8. Keep each speaker on their own track; `render_final` exports their mix.

1. **High-pass.** `high_pass_filter` track=N cutoff_hz=120 takes out handling noise and rumble. Done when it returns a node.
2. **Measure.** `measure` track=N. Done when you have `noise_floor.dbfs`, `noise_floor.at_sec`, `rms_dbfs`, `loudness.range_lu` and `contrast.difference_db`. When `noise_floor.found` is false, skip steps 3 and 4.
3. **Noise print.** Done when the half second from `noise_floor.at_sec` is known to be a pause with no voice in it: either `contrast.difference_db` is 20 dB or more, or the user has confirmed it or named another stretch. In a multi-microphone interview, the quietest stretch of one track is often the other speaker's bleed: then ask.
4. **Denoise.** `noise_reduction` track=N noise_from=<the stretch's start> noise_to=<its end>. Then `measure` track=N. Done when `contrast.wcag_20db_pass` is true (speech at least 20 dB over the background). Otherwise one more pass on the same stretch, two in all, then report the numbers.
5. **Presence.** `eq` track=N bands=[{"freq_hz": 2500, "gain_db": 3, "q": 0.7}] lifts the 1 to 4 kHz range where consonants sit. Done when it returns a node.
6. **Compress.** `compressor` track=N threshold_db=-20 ratio=3.0 attack_ms=15 release_ms=100 evens out the loud and quiet words of one speaker. Done when `measure` track=N reads a lower `loudness.range_lu` than step 2; if it does not, the threshold sat above the speech: say so rather than compressing twice.
7. **Gate.** `noise_gate` track=N threshold_db=-45 closes between sentences, where the other speakers bleed into this microphone. It is the last edit to this track's audio: denoise only before it, since a gated pause is exact silence and no longer a noise print. For an audiobook, skip it: ACX wants room tone in the pauses. Done when it returns a node.
8. **Match the speakers.** `normalize_loudness` track=N target_lufs=<one value, -16 unless the user named another> on every speaker track, so each speaks at the same loudness. Done when each result's `achieved_lufs` is within 1 LU of that value. A result with `capped_by_ceiling` true fell short for its peaks: `set_track_gain` track=N db=<its `requested_gain_db`>, and hold the peaks with one limiter on the master bus (`add_effect` scope=master kind=limiter params={"ceiling_db": -1.0}, once).

Report the contrast and loudness of each track.

## Reference

Steps 1, 4, 5, 6 and 7 rewrite the track's own audio, which plays before its track gain. The thresholds suit speech recorded near -20 dBFS RMS: when step 2's `rms_dbfs` (less the track's gain in the session block) sits further under that, lower threshold_db by the difference.
