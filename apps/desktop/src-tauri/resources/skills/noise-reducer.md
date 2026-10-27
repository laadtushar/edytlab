---
name: noise-reducer
description: Noise reduction and audio cleanup
trigger: regex
pattern: \b(noisy|denois\w*|de-noise|noise (reduction|removal|reducer)|background noise|(remove|reduce|cut|kill|clean up|get rid of) (the |some |all )?noise|hiss|hum|humming|buzz|buzzing|static)\b
enabled: true
---

## Noise Reduction Workflow

1. **Analyze**: `silence_finder` threshold_db=-50 — find a region that is pure noise (no signal) to understand the noise floor, and `measure` track=N to read `noise_floor.dbfs` and `contrast.difference_db`
2. **Reduce**: `noise_reduction` noise_duration_sec=1.0 — applies spectral subtraction, learning the noise from the first second of the track, so that stretch must be noise only
3. **Gate**: `noise_gate` threshold_db=-55 — gates the residue below the signal. It sits lower than the podcast gate (-50) and the dialog gate (-45) because reduction has already taken most of the noise out. Skip the gate for an ACX audiobook: it needs natural room tone.

For severe noise:
- Apply `noise_reduction` twice with lower strength rather than once with high strength
- Avoid over-processing: artifacts (metallic or robotic sound) are worse than moderate noise

After noise reduction, apply `eq` to restore any high-frequency detail that was attenuated.

Stop: `measure` track=N again. Done when `contrast.wcag_20db_pass` is true (speech at least 20 dB over the background) or `noise_floor.dbfs` is at or under -60. Allow two reduction passes; if a pass lowers `noise_floor.dbfs` by less than 2 dB, stop and say the recording is too noisy to fix here.
