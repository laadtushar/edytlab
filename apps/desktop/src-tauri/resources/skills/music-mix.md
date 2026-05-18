---
name: music-mix
description: Music mixing and mastering workflow
trigger: keywords
keywords: [music, mix, mixing, master, mastering, song, track, beat, instrument, band]
enabled: true
---

## Music Mixing Workflow

For music tracks, suggested workflow:

1. **Gain staging**: `gain` — set each track so the mix peaks around -6 dBFS before processing
2. **EQ**: `eq` — cut competing frequencies between instruments; each instrument owns a frequency range
3. **Compression**: `compressor` — tighten transients; gentle settings (ratio=2.0) for glue compression
4. **Reverb**: `reverb` room_size=0.4, wet=0.2 — add space; keep wet low for upfront sounds
5. **Stereo width**: `stereo_widener` — widen pads and synths, keep kick and bass narrow
6. **Limiter**: `limiter` ceiling_db=-0.3 — prevent digital clipping on the final output

Use `mix_to_new_track` to commit a stem mix. Use `plot_spectrum` to compare frequency balance between tracks.
