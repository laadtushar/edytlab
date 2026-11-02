---
name: silence-cleaner
description: Silence and dead air: found, reviewed with the user, removed
trigger: regex
pattern: \b(silence|silences|silent|dead air|pauses?|gaps?)\b
enabled: true
---

## Silence cleaning

Track N is the track to clean. Steps 1 and 3 take the same threshold_db and min_silence_ms.

1. **Find.** `silence_finder` track=N threshold_db=<-60 for a clean studio recording, -50 for a typical room, -40 for a noisy one> min_silence_ms=1000. Done when `count` is above 0. At 0, raise threshold_db by 6 dB (no higher than -30) and look again; after three tries at 0, tell the user the track has no silence at these settings and stop.
2. **Review.** Show the user the regions: how many, the total seconds, the longest. Done when they approve the removal. For an audiobook, stop here: ACX wants room tone in the pauses, so the pauses stay.
3. **Remove.** `truncate_silence` track=N with step 1's threshold_db and min_silence_ms. With more than one track, every other track falls out of sync, because only this one moves: ask the user before running it. Done when it returns a node.
4. **Verify.** `silence_finder` track=N with the same arguments. Done when its `count` is 0. Report how many seconds were removed.

## Reference

- Both tools read the track's own audio, before its track gain, and count a region as silent only when every sample in it is under threshold_db. So the threshold has to clear the room tone's peaks, which sit well above the RMS noise floor `measure` reports; and that floor includes the track gain, so subtract the gain (from the session block) before comparing.
- min_silence_ms is the shortest pause removed, and a pause it takes goes whole, not shortened: keep it at 1000 ms or more so only dead air goes and natural pauses stay.
