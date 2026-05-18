---
name: export-guide
description: Audio export workflow and format guidance
trigger: keywords
keywords: [export, save, output, render, finish, done, final, wav, flac, format]
enabled: true
---

## Export Guide

When the user wants to export or save audio:

1. **Single track export**: `export_multiple` track_indices=[N], output_dir="exports", format="wav"
2. **Mixed export**: `mix_to_new_track` first, then export the new mixed track
3. **Multiple tracks**: `export_multiple` with all desired track_indices

Before export, recommend:
- `normalize` target_db=-1.0 for consistent peak level
- `limiter` ceiling_db=-0.3 for true peak compliance

Ask the user for their target platform to give specific loudness recommendations.
