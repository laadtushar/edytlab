---
name: vocal-chain
description: Vocal chain for sung vocals, high-pass to limiter
trigger: regex
pattern: \b(vocals?|vocalist|singers?|singing)\b
enabled: true
---

## Vocal chain

Track N is the vocal. The session block was written at the start of this turn: for anything changed since, trust the tool results.

1. **Measure.** `measure` track=N. Done when you have `rms_dbfs`, `loudness.range_lu` and `peak.true_dbtp`.
2. **High-pass.** `high_pass_filter` track=N cutoff_hz=100 takes out rumble and plosives. Done when it returns a node.
3. **De-ess** before compressing, so the compressor does not lift the sibilance. `de_esser` track=N frequency_hz=7500 threshold_db=-18. Done when it returns a node.
4. **Compress.** `compressor` track=N threshold_db=-18 ratio=4.0 attack_ms=5 release_ms=50. A slightly longer attack lets the first consonants through for more punch. Done when `measure` track=N reads a lower `loudness.range_lu` than step 1; if it does not, the threshold sat above the vocal: say so rather than compressing twice.
5. **EQ.** `eq` track=N bands=[{"freq_hz": 400, "gain_db": -3}, {"freq_hz": 4000, "gain_db": 2}, {"freq_hz": 12000, "gain_db": 2, "type": "high_shelf"}] cuts the 300 to 500 Hz mud and lifts 3 to 5 kHz presence and the air above 12 kHz. Done when it returns a node.
6. **Space.** `reverb` track=N room_size=0.3 wet=0.2 places the vocal in the mix. Done when it returns a node.
7. **Limit, once in the mix.** A limiter on the master bus (listed in the session block, or added this turn) holds the vocal too: skip this step. If track N has one, change it with `set_effect_params` track=N effect_index=<its index>. Otherwise `add_effect` track=N kind=limiter params={"ceiling_db": -1.0}, and later edits use its result's `position` as effect_index. Done when the mix has exactly one limiter: on the master, or last in track N's chain (the `add_effect` result's `position` is `effect_count` − 1).
8. **Check.** `measure` track=N, or with no track (the mix) when the limiter is on the master bus. Done when `clipping.events` is 0 and `peak.true_dbtp` is at or under -1.0. Otherwise lower the limiter's `ceiling_db` by the overshoot with `set_effect_params` and measure again, three rounds at most, then report the numbers.

## Reference

- Steps 2 to 6 rewrite the track's own audio, which plays before its track gain. The thresholds suit a vocal recorded near -18 dBFS RMS: when step 1's `rms_dbfs` (less the track's gain in the session block) sits further under that, lower each threshold_db by the difference.
- Hold the peaks with the limiter effect, not the limiter tool: a later track gain lifts the peaks past the tool's.
