---
name: loudness-master
description: Loudness to a platform's LUFS target: streaming, podcast or broadcast
trigger: regex
pattern: \b(loudness|lufs|lkfs|r128|normali[sz](e|es|ed|ing|ation)|streaming|broadcast|spotify|youtube|apple music)\b
enabled: true
---

## Loudness mastering

Track N is the track being delivered. For a mix of several tracks, use the master-bus variant under Reference. The session block was written at the start of this turn: for anything changed since, trust the tool results.

1. **Target.** Take the integrated LUFS from the table below for the platform the user named; ask when they named none. Done when you have one LUFS target and a true-peak ceiling (-1.0 dBTP unless the user gave another).
2. **Measure.** `measure` track=N. Done when you have told the user `loudness.integrated_lufs` and `peak.true_dbtp`.
3. **Gain.** `normalize_loudness` track=N preset=<the table's preset>, or target_lufs=<target> for a platform with none. Done when its result is in hand. When `capped_by_ceiling` is true it held the gain back for the peaks:
   - `requested_gain_db` − `applied_gain_db` up to about 6 dB: `set_track_gain` track=N db=<`requested_gain_db`>, and step 4 holds the peaks.
   - More than that: the limiter would take it all off the peaks and be heard. Tell the user and ask whether to compress first or accept a quieter result.
4. **Limiter, last and once.** If the session block lists a limiter on the track, or you added one this turn, set its ceiling with `set_effect_params` track=N effect_index=<its index> params={"ceiling_db": -1.0}. Otherwise `add_effect` track=N kind=limiter params={"ceiling_db": -1.0}, and later edits use its result's `position` as effect_index. Done when the track has one limiter, last in its chain: the `add_effect` result's `position` is `effect_count` − 1, or the session block lists the existing one last.
5. **Measure again.** `measure` track=N. Done when `loudness.integrated_lufs` is within ±0.5 LU of the target and `peak.true_dbtp` is at or under the ceiling. Otherwise correct and measure again, three rounds at most, then report the numbers:
   - Too quiet (the limiter took energy off the peaks): `gain` track=N db=<target − measured>.
   - True peak over: `set_effect_params` track=N effect_index=<the limiter's index> params={"ceiling_db": <ceiling − overshoot>}.

Report the final loudness and true peak. For a broadcast delivery, tell the user to check the exported file on an external LUFS meter.

## Reference

| Platform | Integrated | `normalize_loudness` preset |
|---|---|---|
| Spotify | -14 LUFS (https://support.spotify.com/us/artists/article/loudness-normalization/) | spotify |
| YouTube | -14 LUFS, the commonly measured reference: YouTube publishes no figure | youtube |
| Podcast (Apple Podcasts) | -16 LUFS | apple_podcasts |
| Broadcast TV (EBU R128) | -23 LUFS | broadcast |
| Apple Music | -16 LUFS, the commonly measured Sound Check reference: Apple publishes no figure, so tell the user | none |

**Order.** The track gain (`normalize_loudness`, `set_track_gain`, `gain`) runs before the track's effect chain, so the limiter, last in the chain, holds every gain change. Make every later level change on the track gain; a gain effect placed after the limiter lifts the peaks past it.

**The limiter effect, not the limiter tool.** The tool bakes into the track's file, which plays before the track gain, so the gain lifts the peaks past it again. The ceiling is a sample-peak ceiling, and true peak can sit just above it; that is why step 5 lowers the ceiling by the overshoot.

**A mix of several tracks.** Measure the mix (`measure` with no track) and work on the master bus: the limiter is `add_effect` scope=master kind=limiter params={"ceiling_db": -1.0}, and the level is one master gain before it, `add_effect` scope=master kind=gain params={"db": <target − measured>} position=0. Add each once; after that, change them with `set_effect_params` scope=master effect_index=<its index>. A gain inserted at the front of the chain after the limiter was added moves the limiter up one place, so its index is then one more than `add_effect` reported.
