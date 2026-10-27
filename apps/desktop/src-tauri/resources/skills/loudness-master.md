---
name: loudness-master
description: Loudness normalization to broadcast and streaming standards
trigger: keywords
keywords: [loudness, lufs, loud, quiet, normalize, volume, streaming, broadcast, spotify, youtube]
enabled: true
---

## Loudness Mastering

Platform target levels (integrated LUFS):
- **Streaming** (Spotify, Apple Music, YouTube Music): -14 LUFS
- **YouTube video**: -14 LUFS
- **Podcast**: -16 LUFS
- **Broadcast TV**: -23 LUFS (EBU R128)

Order matters: gain first, limiter last, and never gain after the limiter. A gain applied after limiting raises the peaks the limiter just held back above its ceiling.

Workflow:
1. **Measure**: `measure` track=0 — read `loudness.integrated_lufs` and `peak.true_dbtp` before changing anything.
2. **Gain to the target**: `normalize_loudness` target_lufs=-14 — sets the track gain to reach it. Use `preset="spotify"`, `"youtube"`, `"apple_podcasts"` or `"broadcast"` instead of a number when the user names a platform. If it reports a non-zero `shortfall_db`, the gain needed would put peaks over its ceiling, so it applied less. Set the full gain yourself with `set_track_gain` track=0 db=`requested_gain_db` (from its result) and let step 3 hold the peaks.
3. **Limit last**: `add_effect` track=0 kind=limiter params={"ceiling_db": -1.0} — the true-peak ceiling, as the final stage. Use the effect, not the `limiter` tool: the effect runs after the track's gain, while the tool bakes into the file and only sees the audio before the gain, so the gain would lift the peaks past the ceiling again.
4. **Measure again**: `measure` track=0. Check `loudness.integrated_lufs` is at the target and `peak.true_dbtp` is at or under the ceiling.
   - Loudness fell short because the limiter took energy off the peaks: raise the gain by the shortfall with `set_track_gain` (the limiter still comes after it) and measure again. Allow up to three passes, then report the numbers.
   - True peak is over the ceiling: the limiter's ceiling is a sample-peak ceiling, so lower it with `set_effect_params` track=0 effect_index=0 params={"ceiling_db": -1.5} (use the limiter's position in the chain), then measure again.

If a gain has to change after the limiter is on the track, change the track gain (it still runs before the limiter) and never add a `gain` effect after the limiter in the chain.

`normalize_loudness` caps its own gain at `true_peak_ceiling_db` (default -1 dBFS) rather than clipping, and reports `achieved_lufs` and `shortfall_db` when it could not reach the target.

Other tools, and when they are the wrong choice:
- `normalize` target_dbfs=-1.0 sets the **peak**, not the loudness. Peak normalization cannot match perceived level between files, so use it for headroom, not for delivery.
- `leveler` target_db=-14 evens out level *within* a track using RMS windowing, not true LUFS. It is for a performance that drifts, not for hitting a platform target.

For broadcast-critical work, verify with an external LUFS meter after export.
