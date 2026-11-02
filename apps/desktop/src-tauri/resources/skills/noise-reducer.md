---
name: noise-reducer
description: Noise reduction for hiss, hum, buzz and background noise
trigger: regex
pattern: \b(noisy|denois\w*|de-noise|noise (reduction|removal|reducer)|background noise|(remove|reduce|cut|kill|clean up|get rid of) (the |some |all )?noise|hiss|hum|humming|buzz|buzzing|static)\b
enabled: true
---

## Noise reduction

Track N is the noisy track.

1. **Measure.** `measure` track=N. Done when you have told the user `noise_floor.dbfs` and `contrast.difference_db`, and have `noise_floor.at_sec`, the start of the quietest half second. When `noise_floor.found` is false there is no noise to learn from: say so and stop.
2. **Confirm the noise print.** Done when the half second from `noise_floor.at_sec` is known to be a pause, not quiet speech or music: either the recording plainly has pauses (`contrast.difference_db` of 20 dB or more), or the user has confirmed it or named another noise-only stretch.
3. **Reduce.** `noise_reduction` track=N noise_from=<start> noise_to=<start + 0.5> with the step 2 stretch. For heavy noise, use two passes at strength=0.6 rather than one at full strength: the metallic artefacts of over-reduction are worse than moderate noise. Done when it returns a node.
4. **Measure again.** `measure` track=N. Done when `contrast.wcag_20db_pass` is true (speech at least 20 dB over the background) or `noise_floor.dbfs` is at or under -60. Otherwise repeat step 3 with the new `noise_floor.at_sec`, two passes in all; when a pass lowers `noise_floor.dbfs` by less than 2 dB, stop and tell the user the recording is too noisy to fix here.
5. **Gate the residue** (optional). `noise_gate` track=N threshold_db=-55, lower than a podcast's -50 because reduction took most of the noise out already. For an audiobook, skip it: ACX wants room tone in the pauses. Done when it returns a node.
6. **Restore the top end** when the user hears it dulled. `eq` track=N bands=[{"freq_hz": 8000, "gain_db": 2, "type": "high_shelf"}]. Done when the user is satisfied or declines.

Report `noise_floor.dbfs` and `contrast.difference_db` before and after.

The gate and the EQ rewrite the track's own audio, which plays before its track gain, so a threshold is against that audio's level.
