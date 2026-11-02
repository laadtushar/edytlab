---
name: music-mix
description: Music mix and master: tracks balanced under one master limiter
trigger: regex
pattern: \b(mixing|mix ?down|mastering|music|songs?|stems?|instrumentals?|beats?|drums?|bass)\b
enabled: true
---

## Music mix

Steps 1 to 5 run per track, on the tracks that need them; steps 6 to 8 run on the whole mix. The session block was written at the start of this turn: for anything changed since, trust the tool results.

1. **Measure.** `measure` track=N. Done when you have its `rms_dbfs` and `loudness.range_lu`.
2. **EQ.** Give each instrument its own range and cut what competes with it there. Compare tracks with `plot_spectrum` track=N start_sec=<a moment where the track plays fully> end_sec=<start_sec + 1>, then cut with `eq` track=N bands=[{"freq_hz": <hz>, "gain_db": <negative>}]. Done when `plot_spectrum` at the same start_sec reads lower in the `bands_dbfs` band you cut.
3. **Compress.** `compressor` track=N threshold_db=<a few dB under step 1's level> ratio=2.0 attack_ms=20 release_ms=200, gentle glue. Done when `measure` track=N reads a lower `loudness.range_lu` than step 1; if it does not, the threshold sat above the track: say so rather than compressing twice.
4. **Space.** `reverb` track=N room_size=0.4 wet=0.2; keep wet lower for up-front sounds. Done when it returns a node.
5. **Width.** `stereo_widener` track=N width=1.3 on stereo pads and synths; leave kick and bass at width 1. Done when it returns a node.
6. **Balance.** Set each track's level with `set_track_gain` track=N db=<level>. Done when `measure` with no track reads `peak.sample_dbfs` between -7 and -5, leaving headroom for the limiter.
7. **Limiter, once, on the master.** If the session block lists a limiter on the master, or you added one this turn, change it with `set_effect_params` scope=master effect_index=<its index>. Otherwise `add_effect` scope=master kind=limiter params={"ceiling_db": -1.0}, and later edits use its result's `position` as effect_index. Done when the master has one limiter, last in its chain: the `add_effect` result's `position` is `effect_count` − 1, or the session block lists the existing one last.
8. **Check.** `measure` with no track. Done when `clipping.events` is 0 and `peak.true_dbtp` is at or under -1.0. Otherwise lower the limiter's `ceiling_db` by the overshoot and measure again, three rounds at most, then report the numbers. For a platform loudness target (Spotify, YouTube), follow loudness-master next: its master limiter is this one.

## Reference

- Steps 2 to 5 rewrite each track's own audio, which plays before its track gain, so the compressor's threshold is against that audio's level: step 1's `rms_dbfs` less the track's gain in the session block. That is also why the balance comes after them.
- `plot_spectrum` analyses the first 4096 samples from start_sec (about 0.09 s at 44.1 kHz), so start_sec has to land on a moment typical of the track.
- Whole-mix processing goes on the master bus. A `mix_to_new_track` copy is for committing a stem mix.
- Hold the peaks with the limiter effect on the master, not the limiter tool: the tool limits one track, not the sum of them all.
