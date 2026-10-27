---
name: podcast-cleanup
description: Podcast and voice recording cleanup workflow
trigger: regex
pattern: \b(podcasts?|voice[ -]?overs?|voice (memo|recording)s?|spoken word)\b
enabled: true
---

## Podcast Cleanup Workflow

For podcast or voice recordings, apply these steps in order:

1. **Noise reduction**: `noise_reduction` noise_duration_sec=1.0 — removes background hiss/hum (learns the noise from the first second, so that stretch must be room tone)
2. **Noise gate**: `noise_gate` threshold_db=-50, attack_ms=10, release_ms=100 — silences gaps between speech. The level suits a typical room; skip the gate for an ACX audiobook, which needs natural room tone.
3. **EQ**: `eq` — boost 2–4 kHz for presence, cut 200–400 Hz for clarity, cut below 80 Hz for rumble
4. **De-esser**: `de_esser` frequency_hz=7000, threshold_db=-20 — tames harsh S sounds
5. **Compression**: `compressor` threshold_db=-20, ratio=3.0, attack_ms=10, release_ms=60 — consistent levels
6. **Level**: `normalize_loudness` target_lufs=-16 — the podcast delivery level. `normalize` sets only the peak, so it cannot make episodes match. If the result has a non-zero `shortfall_db`, finish with loudness-master's limiter step (one `add_effect` limiter, last).

Suggest using `silence_finder` first to identify long silences.

Stop: `measure` (omit `track` for the mix). Done when `loudness.integrated_lufs` is within 1 LU of -16, `peak.true_dbtp` is at or under -1.0 and `clipping.events` is 0. Report the numbers.
