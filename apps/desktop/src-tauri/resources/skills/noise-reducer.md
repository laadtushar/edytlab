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
2. **Confirm the noise print.** Done when the half second from `noise_floor.at_sec` is known to be a pause, not quiet speech or music: either the recording plainly has pauses (`contrast.difference_db` of 20 dB or more), or the user has confirmed it or named another noise-only stretch. Every pass of step 3 uses this same stretch: once the noise is lower, the quietest half second can move onto quiet speech.
3. **Reduce.** `noise_reduction` track=N noise_from=<the stretch's start> noise_to=<its end>. For heavy noise, use two passes at strength=0.6 rather than one at full strength: the metallic artefacts of over-reduction are worse than moderate noise. Then `measure` track=N. Done when `contrast.wcag_20db_pass` is true (speech at least 20 dB over the background) or `noise_floor.dbfs` is at or under -60. Otherwise run it again, two passes in all; when a pass lowers `noise_floor.dbfs` by less than 2 dB, stop and tell the user the recording is too noisy to fix here. Report `noise_floor.dbfs` and `contrast.difference_db` from step 1 and from the last pass.
4. **Restore the top end** when the user hears it dulled. `eq` track=N bands=[{"freq_hz": 8000, "gain_db": 2, "type": "high_shelf"}]. Done when the user is satisfied or declines.
5. **Gate the residue** (optional, last). `noise_gate` track=N threshold_db=-55, lower than a podcast's -50 because reduction took most of the noise out already. A gated pause is exact silence, which `measure` leaves out of the noise floor, so step 3's numbers are the ones to report, and no denoising follows the gate. For an audiobook, skip it: ACX wants room tone in the pauses. Done when it returns a node.

The gate and the EQ rewrite the track's own audio, which plays before its track gain, so a threshold is against that audio's level.
