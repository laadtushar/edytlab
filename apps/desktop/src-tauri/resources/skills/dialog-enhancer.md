---
name: dialog-enhancer
description: Dialog and interview clarity enhancement
trigger: keywords
keywords: [dialog, dialogue, interview, clarity, intelligibility, speaker, speakers, conversation]
enabled: true
---

## Dialog Enhancement

For interview and dialog recordings:

1. **High-pass**: `high_pass_filter` cutoff_hz=120 — remove handling noise and low rumble
2. **Noise reduction**: `noise_reduction` — clean background
3. **Mid-frequency EQ**: `eq` — boost 1–4 kHz for intelligibility (where consonants live)
4. **Compression**: `compressor` threshold_db=-20, ratio=3.0, attack_ms=15 — even out speaker level differences
5. **Noise gate**: `noise_gate` threshold_db=-45 — clean between sentences

For multi-speaker interviews:
- Process each speaker track separately before mixing
- Use `stereo_to_mono` if needed for consistent panning
- Use `mix_to_new_track` to create the final combined track
