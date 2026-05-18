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
1. **Normalize peak**: `normalize` target_db=-1.0 — set ceiling first
2. **Limit**: `limiter` ceiling_db=-1.0 — ensure true peak compliance
3. **Leveler**: `leveler` target_db=-14 — match perceived loudness to target

Note: `leveler` uses RMS windowing, not true LUFS measurement. For broadcast-critical work, verify with an external LUFS meter after export.
