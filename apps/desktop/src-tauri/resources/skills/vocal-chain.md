---
name: vocal-chain
description: Professional vocal processing chain
trigger: regex
pattern: \b(vocals?|vocalist|singers?|singing)\b
enabled: true
---

## Vocal Processing Chain

Standard professional vocal chain order:

1. **High-pass filter**: `high_pass_filter` cutoff_hz=100 — remove low-end rumble and plosives
2. **De-esser**: `de_esser` frequency_hz=7500, threshold_db=-18 — control sibilance before compression
3. **Compression**: `compressor` threshold_db=-18, ratio=4.0, attack_ms=5, release_ms=50 — control dynamics
4. **EQ**: `eq` — boost air (12–16 kHz), boost presence (3–5 kHz), cut mud (300–500 Hz)
5. **Reverb**: `reverb` room_size=0.3, wet=0.2 — place vocal in the mix space
6. **Limiter**: `add_effect` track=N kind=limiter params={"ceiling_db": -1.0} — catch peaks. Skip it if the master bus already has a limiter (session block), so the mix is limited once. Do not use the `limiter` tool: it bakes into the file before the track's gain

Tune compression attack to let initial consonants through. A slightly longer attack gives more punch.

Stop: `measure` track=N. Done when `clipping.events` is 0 and `peak.true_dbtp` is at or under -1.0; if not, lower the compressor threshold or the limiter ceiling by the overshoot and measure again (three passes at most).
