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

Workflow:
1. **Match the target**: `normalize_loudness` target_lufs=-14 — measures integrated loudness (EBU R128) and sets the track gain to reach it. Use `preset="spotify"`, `"youtube"`, `"apple_podcasts"` or `"broadcast"` instead of a number when the user names a platform.
2. **Limit**: `limiter` ceiling_db=-1.0 — ensure true peak compliance

`normalize_loudness` caps its own gain at `true_peak_ceiling_db` (default -1 dBFS) rather than clipping, and reports `achieved_lufs` and `shortfall_db` when it could not reach the target. A non-zero shortfall means the track needs limiting first — run `limiter`, then normalize again.

Other tools, and when they are the wrong choice:
- `normalize` target_dbfs=-1.0 sets the **peak**, not the loudness. Peak normalization cannot match perceived level between files, so use it for headroom, not for delivery.
- `leveler` target_db=-14 evens out level *within* a track using RMS windowing, not true LUFS. It is for a performance that drifts, not for hitting a platform target.

For broadcast-critical work, verify with an external LUFS meter after export.
