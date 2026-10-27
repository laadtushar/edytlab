---
name: export-guide
description: Audio export workflow and format guidance
trigger: regex
pattern: \b(exports?|exporting|exported|bounce|bouncing|render(ing)?|save as|wav|flac|mp3)\b
enabled: true
---

## Export Guide

When the user wants to export or save audio:

1. **Whole mix** (every track and the master bus): `render_final` format="wav", "flac" or "mp3", with the head node as its node id. No `mix_to_new_track` is needed.
2. **Single track**: `export_multiple` track_indices=[N], output_dir="exports", format="wav"
3. **Several tracks**: `export_multiple` with all desired track_indices

Before export, `measure` (omit `track` to measure the mix). Stop when `clipping.events` is 0 and `peak.true_dbtp` is at or under -1.0 (or the user's own true-peak target). If not:
- `normalize` target_dbfs=-1.0 lowers or raises the **peak** only, for headroom. It does not set loudness: for a platform loudness target use `normalize_loudness` (loudness-master has the steps).
- Hold the peaks with one limiter, last in the chain and after any gain: `add_effect` kind=limiter params={"ceiling_db": -1.0} (scope=master for the mix, track=N for one track). If the session block already lists a limiter there, lower it with `set_effect_params` instead of adding a second. Do not use the `limiter` tool for this: it bakes into the file before the track's gain, so a later gain lifts the peaks past it.
- The limiter's ceiling is a sample-peak ceiling. If `peak.true_dbtp` is still over, lower `ceiling_db` by the overshoot and measure again (three passes at most).

Ask the user for their target platform to give specific loudness recommendations.
