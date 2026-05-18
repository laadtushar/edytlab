---
name: podcast-cleanup
description: Podcast and voice recording cleanup workflow
trigger: keywords
keywords: [podcast, voice, interview, speech, dialogue, narration, recording, vocal]
enabled: true
---

## Podcast Cleanup Workflow

For podcast or voice recordings, apply these steps in order:

1. **Noise reduction**: `noise_reduction` — removes background hiss/hum
2. **Noise gate**: `noise_gate` threshold_db=-50, attack_ms=10, release_ms=100 — silences gaps between speech
3. **EQ**: `eq` — boost 2–4 kHz for presence, cut 200–400 Hz for clarity, cut below 80 Hz for rumble
4. **De-esser**: `de_esser` frequency_hz=7000, threshold_db=-20 — tames harsh S sounds
5. **Compression**: `compressor` threshold_db=-20, ratio=3.0, attack_ms=10, release_ms=60 — consistent levels
6. **Normalize**: `normalize` target_db=-1.0 — broadcast-ready peak

Ask the user before applying each step. Suggest using `silence_finder` first to identify long silences.
