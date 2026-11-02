---
name: lofi-master
description: Lo-fi master: a dark, warm, narrow sound with optional hiss, at a streaming level
trigger: regex
pattern: \b(lo[- ]?fi|low[- ]?fi|tape (saturation|warmth)|vintage (sound|feel|vibe|tone))\b
enabled: true
---

## Lo-fi master

Track N is the finished mix to give the lo-fi sound. The level and the limiter go on the master bus, because the optional hiss in step 5 is a track of its own and the delivery is the mix. The session block was written at the start of this turn: for anything changed since, trust the tool results.

1. **Target.** -14 LUFS integrated and a -1.0 dBTP ceiling (Spotify, the default), or the platform the user named from loudness-master's table. Done when you have one integrated target and one ceiling.
2. **Saturate.** `distortion` track=N drive=1.5 tone=0.7 rounds the transients like tape; it also darkens the top, above about 6 kHz. Raise drive towards 3 only when the user asks for it dirtier. Done when it returns a node.
3. **Squash.** `compressor` track=N ratio=4.0 attack_ms=10 release_ms=100 threshold_db=<the rms_dbfs of `measure` track=N, less the track's gain in the session block>, for the flat, pumping level of the style. Done when `measure` track=N reads a lower `loudness.range_lu` than before; if it does not, the threshold sat above the music: say so rather than compressing twice.
4. **Band-limit and narrow**, as live effects the user can change later with `set_effect_params` track=N effect_index=<its index>:
   - `add_effect` track=N kind=high_pass_filter params={"cutoff_hz": 100}, then `add_effect` track=N kind=low_pass_filter params={"cutoff_hz": 6000}: the thin, dull band of an old cassette or a sampled record. Keep each result's `position`. Done when both are in the chain, in that order.
   - On a stereo track, `stereo_widener` track=N width=0.6 pulls the image in; skip it for mono. Done when it returns a node.
5. **Hiss** (only when the user wants it). `generate_noise` noise_type="pink" amplitude=0.5 duration_sec=<the length of track N> adds a new track; turn it down under the music with `set_track_gain` track=<the new track> db=-30. Done when the noise is under the music: ask the user, and change the db by what they ask.
6. **Level.** `measure` with no track. If the session block lists a gain at the front of the master chain, or you added one this turn, set it with `set_effect_params` scope=master effect_index=<its index> params={"db": <its db + target − measured>}. Otherwise `add_effect` scope=master kind=gain params={"db": <target − measured>} position=0. Done when the master's first effect is that gain.
7. **Limiter, last and once, on the master.** If the session block lists a limiter on the master, or you added one this turn, set its ceiling with `set_effect_params` scope=master effect_index=<its index> params={"ceiling_db": <ceiling>}. Otherwise `add_effect` scope=master kind=limiter params={"ceiling_db": <ceiling>}. Done when the master has one limiter, last in its chain: the `add_effect` result's `position` is `effect_count` − 1, or the session block lists the existing one last.
8. **Measure again.** `measure` with no track. Done when `loudness.integrated_lufs` is within ±0.5 LU of the target, `peak.true_dbtp` is at or under the ceiling and `clipping.events` is 0. Otherwise change the master gain's db by target − measured, or lower the limiter's `ceiling_db` by the true-peak overshoot, with `set_effect_params` scope=master, and measure again, three rounds at most, then report the numbers.

Report the chain you built, the final loudness and true peak, and the two filter cutoffs, so the user knows what to change for more or less of the effect.

## Reference

- **Order.** Steps 2 and 3 rewrite the track's own audio; step 4's filters run in the track's chain after it. The master gain and limiter run on the sum, the hiss included, and a gain inserted at the front of the master chain after the limiter was added moves the limiter up one place, so its index is then one more than `add_effect` reported.
- **The limiter effect, not the limiter tool.** The tool bakes into one track's file and never sees the hiss or the master gain. The ceiling is a sample-peak ceiling, and true peak can sit just above it; that is why step 8 lowers the ceiling by the overshoot.
- **The target is the platform's.** A lo-fi master is delivered at the same integrated loudness as any other: Spotify plays every master at -14 LUFS and asks for a true peak under -1 dBTP (https://support.spotify.com/us/artists/article/loudness-normalization/). The style is in the tone and the dynamics, not in a lower level.
- **More or less of it.** Lower the low-pass cutoff (4000 is a telephone-like band) or raise drive for more; raise the cutoff towards 10000 and set width back to 1.0 for less.
