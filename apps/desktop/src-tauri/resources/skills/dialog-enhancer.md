---
name: dialog-enhancer
description: Dialog and interview clarity enhancement
trigger: regex
pattern: \b(dialog(ue)?s?|interviews?|conversations?|clarity|intelligibility)\b
enabled: true
---

## Dialog Enhancement

For interview and dialog recordings:

1. **High-pass**: `high_pass_filter` cutoff_hz=120 — remove handling noise and low rumble
2. **Noise reduction**: `noise_reduction` noise_duration_sec=1.0 — clean background (it learns the noise from the first second, so that stretch must be room tone)
3. **Mid-frequency EQ**: `eq` — boost 1–4 kHz for intelligibility (where consonants live)
4. **Compression**: `compressor` threshold_db=-20, ratio=3.0, attack_ms=15 — even out speaker level differences
5. **Noise gate**: `noise_gate` threshold_db=-45 — clean between sentences. Higher than the podcast gate (-50) because speakers bleed into each other's microphones between sentences. Skip the gate for an ACX audiobook, which needs natural room tone.

For multi-speaker interviews:
- Process each speaker track separately before mixing
- Use `stereo_to_mono` if needed for consistent panning
- Use `mix_to_new_track` to create the final combined track

Stop: `measure` track=N and read `contrast.wcag_20db_pass` (speech at least 20 dB over the background). If it is false, repeat step 2 once more, then report the numbers; two passes at most.
