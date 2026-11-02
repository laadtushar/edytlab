---
name: loudness-master
description: Loudness to a platform's LUFS target: streaming, podcast or broadcast
trigger: regex
pattern: \b(loudness|lufs|lkfs|r128|normali[sz](e|es|ed|ing|ation)|streaming|broadcast|spotify|youtube|apple music)\b
enabled: true
---

## Loudness mastering

Track N is the track being delivered. For a mix of several tracks, use the master-bus variant under Reference.

1. **Target.** Take the integrated LUFS from the table below for the platform the user named; ask when they named none. Done when you have one LUFS target and a true-peak ceiling (-1.0 dBTP unless the user gave another).
2. **Measure.** `measure` track=N. Done when you have told the user `loudness.integrated_lufs` and `peak.true_dbtp`.
3. **Gain.** `normalize_loudness` track=N preset=<the table's preset>, or target_lufs=<target> for a platform with none. Done when its result is in hand. A non-zero `shortfall_db` means it held the gain back for the peaks: then `set_track_gain` track=N db=<its `requested_gain_db`>, and step 4 holds the peaks.
4. **Limiter, last and once.** `add_effect` track=N kind=limiter params={"ceiling_db": -1.0}, appended to the end of the chain. When the session block already lists a limiter on the track, set its ceiling with `set_effect_params` instead. Done when the session block lists exactly one limiter on the track, last in its chain.
5. **Measure again.** `measure` track=N. Done when `loudness.integrated_lufs` is within ±0.5 LU of the target and `peak.true_dbtp` is at or under the ceiling. Otherwise correct and measure again, three rounds at most, then report the numbers:
   - Too quiet (the limiter took energy off the peaks): `gain` track=N db=<target − measured>.
   - True peak over: `set_effect_params` track=N effect_index=<the limiter's index> params={"ceiling_db": <ceiling − overshoot>}.

Report the final loudness and true peak. For a broadcast delivery, tell the user to check the exported file on an external LUFS meter.

## Reference

| Platform | Integrated | `normalize_loudness` preset |
|---|---|---|
| Spotify | -14 LUFS (https://support.spotify.com/us/artists/article/loudness-normalization/) | spotify |
| YouTube | -14 LUFS | youtube |
| Podcast (Apple Podcasts) | -16 LUFS | apple_podcasts |
| Broadcast TV (EBU R128) | -23 LUFS | broadcast |
| Apple Music | Apple publishes no figure: use -14 and tell the user it is an assumption | none |

**Order.** The track gain (`normalize_loudness`, `set_track_gain`, `gain`) runs before the track's effect chain, so the limiter, last in the chain, holds every gain change. Make every later level change on the track gain; a gain effect placed after the limiter lifts the peaks past it.

**The limiter effect, not the limiter tool.** The tool bakes into the track's file, which plays before the track gain, so the gain lifts the peaks past it again. The ceiling is a sample-peak ceiling, and true peak can sit just above it; that is why step 5 lowers the ceiling by the overshoot.

**A mix of several tracks.** Measure the mix (`measure` with no track), hold the peaks on the master bus with `add_effect` scope=master kind=limiter params={"ceiling_db": -1.0}, and make up the level with a master gain before it: `add_effect` scope=master kind=gain params={"db": <target − measured>} position=0. `set_effect_params` scope=master changes either.
