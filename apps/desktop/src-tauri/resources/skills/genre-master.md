---
name: genre-master
description: Genre master: a finished mix shaped and levelled for its style of music
trigger: regex
pattern: \b(genres?|hip[- ]?hop|rap|trap|edm|techno|dubstep|electronic music|rock|metal|punk|r&b|jazz|blues|folk|classical|orchestral)\b
enabled: true
---

## Genre master

Track N is the finished stereo mix of one song. When the session holds the mix as several tracks, balance them with music-mix first: `eq` and `compressor` work on one track, and the master bus runs only gain, limiter and the filters. Then ask whether to commit the mix with `mix_to_new_track` track_indices=[...], and mute the tracks it came from (`mute_track` track=<each> muted=true) so the mix does not play twice. The session block was written at the start of this turn: for anything changed since, trust the tool results.

1. **Genre and target.** Take the genre's row from the table below; ask when the user's genre fits none. Done when you have the row, an integrated target and a true-peak ceiling: the platform's from loudness-master (-14 LUFS and -1.0 dBTP for Spotify, the default). A user who wants the master louder than -14 LUFS gets a -2.0 dBTP ceiling.
2. **Measure.** `measure` track=N. Done when you have told the user `loudness.integrated_lufs`, `loudness.range_lu`, `peak.true_dbtp` and `rms_dbfs`.
3. **Tone.** `eq` track=N bands=<the row's bands>, unless the row says none. Done when it returns a node; tell the user each move and offer to undo it.
4. **Compress.** `compressor` track=N with the row's ratio, attack_ms and release_ms, and threshold_db=<step 2's rms_dbfs, less the track's gain in the session block>, unless the row says none. Done when `measure` track=N reads a lower `loudness.range_lu` than step 2; if it does not, the threshold sat above the music: say so rather than compressing twice.
5. **Level.** `normalize_loudness` track=N target_lufs=<target> true_peak_ceiling_db=<ceiling>. Done when its result is in hand. When `capped_by_ceiling` is true:
   - Wide row: keep `achieved_lufs` and tell the user: a quieter master with its peaks intact is right for these styles, and Spotify lifts a soft master at playback only as far as its peaks allow.
   - Other rows, `requested_gain_db` − `applied_gain_db` up to about 6 dB: `set_track_gain` track=N db=<`requested_gain_db`>, and step 6 holds the peaks. More than that: tell the user and ask whether to compress more or accept `achieved_lufs`.
6. **Limiter, last and once.** If the session block lists a limiter on the track, or you added one this turn, set its ceiling with `set_effect_params` track=N effect_index=<its index> params={"ceiling_db": <ceiling>}. Otherwise `add_effect` track=N kind=limiter params={"ceiling_db": <ceiling>}, and later edits use its result's `position` as effect_index. Done when the track has one limiter, last in its chain: the `add_effect` result's `position` is `effect_count` − 1, or the session block lists the existing one last.
7. **Measure again.** `measure` track=N. Done when `loudness.integrated_lufs` is within ±0.5 LU of the target (or of the `achieved_lufs` kept in step 5) and `peak.true_dbtp` is at or under the ceiling. Otherwise correct and measure again, three rounds at most, then report the numbers:
   - Too quiet: `gain` track=N db=<target − measured>.
   - True peak over: `set_effect_params` track=N effect_index=<the limiter's index> params={"ceiling_db": <ceiling − overshoot>}.

Report the row you used, the final loudness, loudness range and true peak, and the step 2 numbers beside them.

## Reference

| Row | Genres | Bands | Compressor |
|---|---|---|---|
| Dense | hip-hop, rap, trap, EDM, techno, dubstep | `[{"freq_hz": 50, "gain_db": 1.5, "type": "low_shelf"}, {"freq_hz": 300, "gain_db": -1.5}]` | `compressor` ratio=2.0 attack_ms=30 release_ms=150 |
| Band | rock, metal, punk, R&B | `[{"freq_hz": 250, "gain_db": -1.5}, {"freq_hz": 3000, "gain_db": 1.0}]` | `compressor` ratio=2.0 attack_ms=20 release_ms=200 |
| Wide | jazz, blues, folk, classical, orchestral | `[{"freq_hz": 12000, "gain_db": 1.0, "type": "high_shelf"}]`; none for classical | `compressor` ratio=1.5 attack_ms=50 release_ms=300; none for classical |

- **Targets.** Spotify plays every master at -14 LUFS and asks for a true peak under -1 dBTP, or under -2 dBTP for a master louder than -14 LUFS, and it raises a soft master only as far as its peaks leave 1 dB of headroom (https://support.spotify.com/us/artists/article/loudness-normalization/). A louder master is turned down, so the genre changes the tone and the dynamics, not the delivery level. AES TD1008 suggests album normalization for classical music because of its wide dynamic range (https://aes.org/wp-content/uploads/2026/10/20210924_TD1008_v3.13.pdf): that is why the Wide row keeps its peaks.
- **Small moves.** The bands and ratios are starting points for a mix that is already balanced: 1 to 2 dB of EQ and a ratio of 2 or less. A mix that needs more goes back to music-mix.
- **Order.** Steps 3 and 4 rewrite the track's own audio, which plays before the track gain; the gain (steps 5 and 7) runs before the track's effect chain, so the limiter, last in the chain, holds every gain change.
- **The limiter effect, not the limiter tool.** The tool bakes into the track's file, before the track gain. The ceiling is a sample-peak ceiling, and true peak can sit just above it; that is why step 7 lowers the ceiling by the overshoot.
