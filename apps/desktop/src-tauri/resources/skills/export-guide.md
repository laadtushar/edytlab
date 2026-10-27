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
- `normalize` target_dbfs=-1.0 for consistent peak level
- `limiter` ceiling_db=-0.3 as the last level change, after `normalize` (gain first, limiter last; never add gain after the limiter). The limiter's ceiling is a sample-peak ceiling, so inter-sample (true) peaks can still go over it.
- `measure` (omit `track` to measure the mix) and read `peak.true_dbtp`. If the user has a true-peak target (for example -1 dBTP) and `peak.true_dbtp` is above it, lower `ceiling_db` on the limiter by the overshoot and measure again.

Ask the user for their target platform to give specific loudness recommendations.
