---
name: noise-reducer
description: Noise reduction and audio cleanup
trigger: keywords
keywords: [noise, hiss, hum, buzz, background, static, cleanup, remove noise]
enabled: true
---

## Noise Reduction Workflow

1. **Analyze**: `silence_finder` — find a region that is pure noise (no signal) to understand the noise floor
2. **Reduce**: `noise_reduction` — applies spectral subtraction
3. **Gate**: `noise_gate` threshold_db=-55 — gates remaining noise below the signal

For severe noise:
- Apply `noise_reduction` twice with lower strength rather than once with high strength
- Avoid over-processing: artifacts (metallic or robotic sound) are worse than moderate noise

After noise reduction, apply `eq` to restore any high-frequency detail that was attenuated.
