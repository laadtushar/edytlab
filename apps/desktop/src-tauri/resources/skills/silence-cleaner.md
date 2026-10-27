---
name: silence-cleaner
description: Find and remove silent regions
trigger: regex
pattern: \b(silence|silences|silent|dead air|pauses?|gaps?)\b
enabled: true
---

## Silence Cleaning Workflow

To identify and remove silence:

1. **Find silences**: `silence_finder` threshold_db=-50, min_silence_ms=500 — lists all silent regions
2. **Review**: Show user the regions before removing
3. **Remove**: `truncate_silence` threshold_db=-50, min_silence_ms=500 — removes silent regions destructively

Adjust threshold_db based on the recording's noise floor:
- Clean studio recording: -60 dBFS
- Typical room noise: -50 dBFS
- Noisy environment: -40 dBFS

For podcasts, prefer -50 dBFS with 300–500 ms minimum to preserve natural pauses.

Stop: run `silence_finder` again with the same threshold_db and min_silence_ms. Done when it lists no region; if one is left, run `truncate_silence` once more, then report it. For an ACX audiobook keep the room tone in its pauses: use `silence_finder` to look, and do not truncate.
