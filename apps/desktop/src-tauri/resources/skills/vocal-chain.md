---
name: vocal-chain
description: Professional vocal processing chain
trigger: keywords
keywords: [vocal, vocals, singer, singing, lead, harmony, chorus]
enabled: true
---

## Vocal Processing Chain

Standard professional vocal chain order:

1. **High-pass filter**: `high_pass_filter` cutoff_hz=100 — remove low-end rumble and plosives
2. **De-esser**: `de_esser` frequency_hz=7500, threshold_db=-18 — control sibilance before compression
3. **Compression**: `compressor` threshold_db=-18, ratio=4.0, attack_ms=5, release_ms=50 — control dynamics
4. **EQ**: `eq` — boost air (12–16 kHz), boost presence (3–5 kHz), cut mud (300–500 Hz)
5. **Reverb**: `reverb` room_size=0.3, wet=0.2 — place vocal in the mix space
6. **Limiter**: `limiter` ceiling_db=-1.0 — catch peaks

Tune compression attack to let initial consonants through. A slightly longer attack gives more punch.
