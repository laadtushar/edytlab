---
name: export-guide
description: Export the mix or single tracks to files, peaks checked first
trigger: regex
pattern: \b(exports?|exporting|exported|bounce|bouncing|render(ing)?|save as|wav|flac|mp3)\b
enabled: true
---

## Export

1. **What, how, where.** Done when you know what is exported (the whole mix, or which tracks), the format and, for the mix, the file path; ask for whatever the user has not said. For a loudness target (a platform, LUFS), follow loudness-master before step 2: its limiter is the one step 2 checks.
2. **Check the peaks.** `measure` with no track for the mix, or track=N for one track. Done when `clipping.events` is 0 and `peak.true_dbtp` is at or under -1.0 (or the user's own ceiling). Otherwise:
   - Hold them with one limiter, last in the chain: `add_effect` kind=limiter params={"ceiling_db": -1.0} with scope=master for the mix, or track=N for a track. When the session block already lists a limiter in that chain, change it with `set_effect_params` instead.
   - True peak still over: lower `ceiling_db` by the overshoot and measure again. After three rounds, report the numbers and ask whether to export anyway.
3. **Export.**
   - The mix: `render_final` node_id=<the session block's head node> format=<format> out_path=<the user's path>. Done when the result's `path` is the file the user asked for.
   - Tracks as WAV: `export_multiple` track_indices=[N, ...] output_dir="exports". Done when its result lists one file per track.
   - One track in another format: `solo_track` track=N solo=true, `render_final` on the new head, then `solo_track` track=N solo=false. Done when the file is written and the track is no longer soloed.
4. **Report** each file's path and format, and the peak numbers from step 2.

## Reference

- `render_final` renders every track and the master bus; no mix-down track is needed first.
- `export_multiple` writes WAV only, into a folder inside the project (a relative output_dir, no `..`). Each file is one track through its own chain and the master chain.
- Hold the peaks with the limiter effect, not the limiter tool: the tool bakes into the track's file before the track's gain, so a later gain lifts the peaks past it. The ceiling is a sample-peak ceiling, which is why true peak can need it lower.
