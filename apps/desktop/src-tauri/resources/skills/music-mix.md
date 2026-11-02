---
name: music-mix
description: Music mix and master: tracks balanced under one master limiter
trigger: regex
pattern: \b(mixing|mix ?down|mastering|music|songs?|stems?|instrumentals?|beats?|drums?|bass)\b
enabled: true
---

## Music mix

Steps 1 to 4 run per track, on the tracks that need them; steps 5 to 7 run on the whole mix.

1. **EQ.** Give each instrument its own range and cut what competes with it there. Compare tracks with `plot_spectrum` track=N start_sec=<a busy passage> end_sec=<its end>, then cut with `eq` track=N bands=[{"freq_hz": <hz>, "gain_db": <negative>}]. Done when `plot_spectrum` on each EQ'd track reads lower in the `bands_dbfs` band you cut.
2. **Compress.** `compressor` track=N threshold_db=<a few dB under the track's level> ratio=2.0 attack_ms=20 release_ms=200, gentle glue. Done when `measure` track=N reads a lower `loudness.range_lu` than before.
3. **Space.** `reverb` track=N room_size=0.4 wet=0.2; keep wet lower for up-front sounds. Done when it returns a node.
4. **Width.** `stereo_widener` track=N width=1.3 on stereo pads and synths; leave kick and bass at width 1. Done when it returns a node.
5. **Balance.** Set each track's level with `set_track_gain` track=N db=<level>. Done when `measure` with no track reads `peak.sample_dbfs` between -7 and -5, leaving headroom for the limiter.
6. **Limiter, once, on the master.** `add_effect` scope=master kind=limiter params={"ceiling_db": -1.0}. When the session block already lists a limiter on the master, change it with `set_effect_params` scope=master instead. Done when the master chain lists exactly one limiter, last.
7. **Check.** `measure` with no track. Done when `clipping.events` is 0 and `peak.true_dbtp` is at or under -1.0. Otherwise lower the limiter's `ceiling_db` by the overshoot and measure again, three rounds at most, then report the numbers. For a platform loudness target (Spotify, YouTube), follow loudness-master next: its master limiter is this one.

## Reference

- Steps 1 to 4 rewrite each track's own audio, which plays before its track gain, so the compressor's threshold is against that audio's level: take it from `measure` track=N's `rms_dbfs` less the track's gain in the session block. That is also why the balance comes after them.
- A whole-mix effect belongs on the master bus (`add_effect` scope=master: limiter, gain or a filter), where it keeps working when a track changes. A `mix_to_new_track` copy goes stale when a source track changes, so use it only to commit a stem mix.
- Hold the peaks with the limiter effect on the master, not the limiter tool: the tool bakes into one track's file, and the master bus never sees it.
