---
name: live-master
description: Live recording master: room and crowd kept, one level for the whole set
trigger: regex
pattern: \b(live (recordings?|sets?|shows?|albums?|performances?|versions?)|concerts?|gigs?|bootlegs?)\b
enabled: true
---

## Live master

Track N is the live recording: one board or room mix of the whole set. For a multitrack concert, balance it with music-mix first and run steps 6 to 8 on the master bus, as loudness-master's Reference describes. The session block was written at the start of this turn: for anything changed since, trust the tool results.

1. **Target.** Take the row for where the recording goes from the table below; ask when the user has not said. Done when you have one integrated target and one true-peak ceiling. Master the set as one piece and split it into songs afterwards, so every song keeps the level it had in the room.
2. **Measure.** `measure` track=N. Done when you have told the user `loudness.integrated_lufs`, `loudness.range_lu`, `peak.true_dbtp`, `rms_dbfs` and `clipping.events`. Clipping events were in the recording: no later step brings them back, so say so.
3. **Rumble.** `high_pass_filter` track=N cutoff_hz=40 takes out stage rumble, handling and wind under the music; use 30 when the set leans on kick and bass. Done when it returns a node.
4. **Mud.** Only when the user hears the room in the low mids: `plot_spectrum` track=N start_sec=<a moment where the band plays fully> end_sec=<start_sec + 1>, then `eq` track=N bands=[{"freq_hz": 300, "gain_db": -2}]. Done when `plot_spectrum` at the same start_sec reads a lower low_mid in `bands_dbfs`.
5. **Compress gently.** The dynamics are the performance: `compressor` track=N ratio=1.5 attack_ms=30 release_ms=250 threshold_db=<step 2's rms_dbfs, less the track's gain in the session block>, so only the passages above the average level are held. Done when `measure` track=N reads a lower `loudness.range_lu` than step 2; if it does not, the threshold sat above the music: say so rather than compressing twice.
6. **Level.** `normalize_loudness` track=N target_lufs=<target> true_peak_ceiling_db=<ceiling>. Done when its result is in hand. When `capped_by_ceiling` is true:
   - `requested_gain_db` − `applied_gain_db` up to about 6 dB: `set_track_gain` track=N db=<`requested_gain_db`>, and step 7 holds the peaks.
   - More than that: the limiter would flatten the drums and the applause. Tell the user and ask whether to accept `achieved_lufs` (a quieter master that keeps its peaks) or compress more first.
7. **Limiter, last and once.** If the session block lists a limiter on the track, or you added one this turn, set its ceiling with `set_effect_params` track=N effect_index=<its index> params={"ceiling_db": <ceiling>}. Otherwise `add_effect` track=N kind=limiter params={"ceiling_db": <ceiling>}, and later edits use its result's `position` as effect_index. Done when the track has one limiter, last in its chain: the `add_effect` result's `position` is `effect_count` − 1, or the session block lists the existing one last.
8. **Measure again.** `measure` track=N. Done when `loudness.integrated_lufs` is within ±1 LU of the target (or of `achieved_lufs`, when the user accepted it) and `peak.true_dbtp` is at or under the ceiling. Otherwise correct and measure again, three rounds at most, then report the numbers:
   - Too quiet: `gain` track=N db=<target − measured>.
   - True peak over: `set_effect_params` track=N effect_index=<the limiter's index> params={"ceiling_db": <ceiling − overshoot>}.

Report the final loudness, loudness range and true peak, and the step 2 numbers beside them.

## Reference

| Destination | Integrated | True-peak ceiling | Source |
|---|---|---|---|
| Streaming (Spotify and the default) | -14 LUFS | -1.0 dBTP | https://support.spotify.com/us/artists/article/loudness-normalization/ |
| Broadcast (EBU R128) | -23 LUFS, ±1 LU allowed for live programmes | -1.0 dBTP | https://tech.ebu.ch/docs/r/r128.pdf |

- **One level for the set.** Spotify normalizes an album with one gain, so the quiet songs stay as quiet as they were played; AES TD1008 recommends the same album normalization (https://aes.org/wp-content/uploads/2026/10/20210924_TD1008_v3.13.pdf). Songs mastered one by one to the same LUFS would lift the ballads and the talk between songs to the level of the loudest number.
- **Order.** Steps 3 to 5 rewrite the track's own audio, which plays before the track gain; the gain (steps 6 and 8) runs before the track's effect chain, so the limiter, last in the chain, holds every gain change. A gain effect placed after the limiter lifts the peaks past it.
- **The limiter effect, not the limiter tool.** The tool bakes into the track's file, which plays before the track gain. The ceiling is a sample-peak ceiling, and true peak can sit just above it; that is why step 8 lowers the ceiling by the overshoot.
- Leave the crowd and the room in: `noise_reduction` and `noise_gate` take the applause and the air between songs with them. Clean a noisy recording only when the user asks.
