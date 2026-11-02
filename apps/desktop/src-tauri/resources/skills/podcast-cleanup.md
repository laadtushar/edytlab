---
name: podcast-cleanup
description: Podcast and voice recording cleanup, delivered at -16 LUFS
trigger: regex
pattern: \b(podcasts?|voice[ -]?overs?|voice (memo|recording)s?|spoken word)\b
enabled: true
---

## Podcast cleanup

Run steps 1 to 7 on each voice track N, in order, then step 8 on the mix.

1. **Measure.** `measure` track=N. Done when you have `noise_floor.dbfs`, `noise_floor.at_sec`, `rms_dbfs` and `loudness.range_lu`.
2. **Denoise.** `noise_reduction` track=N noise_from=<`noise_floor.at_sec`> noise_to=<that + 0.5> learns the hiss and hum from the quietest half second, which must be a pause, not speech. Done when `measure` track=N reads a lower `noise_floor.dbfs` than step 1. Skip when step 1 found no noise floor.
3. **Gate.** `noise_gate` track=N threshold_db=-50 attack_ms=10 release_ms=100 silences the gaps between speech in a typical room. For an audiobook, skip it: ACX wants room tone in the pauses. Done when it returns a node.
4. **Rumble.** `high_pass_filter` track=N cutoff_hz=80. Done when it returns a node.
5. **EQ.** `eq` track=N bands=[{"freq_hz": 300, "gain_db": -3}, {"freq_hz": 3000, "gain_db": 3}] clears the 200 to 400 Hz mud and lifts 2 to 4 kHz presence. Done when it returns a node.
6. **De-ess.** `de_esser` track=N frequency_hz=7000 threshold_db=-20. Done when it returns a node.
7. **Compress.** `compressor` track=N threshold_db=-20 ratio=3.0 attack_ms=10 release_ms=60. Done when `measure` track=N reads a lower `loudness.range_lu` than step 1; if it does not, the threshold sat above the speech: say so rather than compressing twice.
8. **Level.** `normalize_loudness` track=N preset="apple_podcasts" for each voice track (-16 LUFS). Then `measure` with no track, for the mix. Done when `loudness.integrated_lufs` is within ±1 LU of -16, `peak.true_dbtp` is at or under -1.0 and `clipping.events` is 0. Otherwise correct on the master bus and measure again, three rounds at most, then report the numbers:
   - Peaks over: `add_effect` scope=master kind=limiter params={"ceiling_db": -1.0}, or lower the ceiling of the master limiter the session block lists with `set_effect_params` scope=master.
   - Level off: `add_effect` scope=master kind=gain params={"db": <-16 − measured>} position=0, ahead of the limiter.

## Reference

Steps 2 to 7 rewrite the track's own audio, which plays before its track gain. Their thresholds suit speech recorded near -20 dBFS RMS: when step 1's `rms_dbfs` (less the track's gain in the session block) sits further under that, lower each threshold_db by the difference.
