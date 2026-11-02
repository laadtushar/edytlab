---
name: podcast-cleanup
description: Podcast and voice recording cleanup, delivered at -16 LUFS
trigger: regex
pattern: \b(podcasts?|voice[ -]?overs?|voice (memo|recording)s?|spoken word)\b
enabled: true
---

## Podcast cleanup

Run steps 1 to 8 on each voice track N, in order, then step 9 on the mix. The session block was written at the start of this turn: for anything changed since, trust the tool results.

1. **Rumble.** `high_pass_filter` track=N cutoff_hz=80, first, so rumble does not hold the gate open later. Done when it returns a node.
2. **Measure.** `measure` track=N. Done when you have `noise_floor.dbfs`, `noise_floor.at_sec`, `rms_dbfs`, `loudness.range_lu` and `contrast.difference_db`. When `noise_floor.found` is false, skip steps 3 and 4.
3. **Noise print.** Done when the half second from `noise_floor.at_sec` is known to be a pause with no voice in it: either `contrast.difference_db` is 20 dB or more, or the user has confirmed it or named another stretch. With a microphone per speaker, the quietest stretch of one track is often another speaker's bleed: then ask.
4. **Denoise.** `noise_reduction` track=N noise_from=<the stretch's start> noise_to=<its end> takes out hiss and hum. Done when `measure` track=N reads a lower `noise_floor.dbfs` than step 2.
5. **EQ.** `eq` track=N bands=[{"freq_hz": 300, "gain_db": -3}, {"freq_hz": 3000, "gain_db": 3}] clears the 200 to 400 Hz mud and lifts 2 to 4 kHz presence. Done when it returns a node.
6. **De-ess.** `de_esser` track=N frequency_hz=7000 threshold_db=-20. Done when it returns a node.
7. **Compress.** `compressor` track=N threshold_db=-20 ratio=3.0 attack_ms=10 release_ms=60. Done when `measure` track=N reads a lower `loudness.range_lu` than step 2; if it does not, the threshold sat above the speech: say so rather than compressing twice.
8. **Gate.** `noise_gate` track=N threshold_db=-50 attack_ms=10 release_ms=100 silences the gaps between speech in a typical room. It is the last edit to this track's audio: a gated pause is exact silence and no longer a noise print. For an audiobook, skip it: ACX wants room tone in the pauses. Done when it returns a node.
9. **Level.** `normalize_loudness` track=N preset="apple_podcasts" for each voice track (-16 LUFS). Then `measure` with no track, for the mix. Done when `loudness.integrated_lufs` is within ±1 LU of -16, `peak.true_dbtp` is at or under -1.0 and `clipping.events` is 0. Otherwise correct on the master bus and measure again, three rounds at most, then report the numbers. Add each master effect once, keeping the `position` its `add_effect` returns; in later rounds change it with `set_effect_params` scope=master effect_index=<that position>:
   - Peaks over: a limiter, `add_effect` scope=master kind=limiter params={"ceiling_db": -1.0} (or the master limiter the session block lists), its ceiling lowered by the overshoot in later rounds.
   - Level off: a gain ahead of the limiter, `add_effect` scope=master kind=gain params={"db": <-16 − measured>} position=0 (which moves the limiter up one place), its db adjusted by the remaining difference in later rounds.

## Reference

Steps 1 and 4 to 8 rewrite the track's own audio, which plays before its track gain. Their thresholds suit speech recorded near -20 dBFS RMS: when step 2's `rms_dbfs` (less the track's gain in the session block) sits further under that, lower each threshold_db by the difference.
