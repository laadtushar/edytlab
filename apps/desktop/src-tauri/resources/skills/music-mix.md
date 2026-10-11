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
6. **Limiter**: `add_effect` scope=master kind=limiter params={"ceiling_db": -0.3} — prevent digital clipping on the final output. On the master bus it limits the whole mix and keeps limiting when a track is edited later; `limiter` on a track only limits that one track

Effects on the whole mix (a limiter, a gain, a filter) belong on the master bus with `add_effect` scope=master, not on a track made by `mix_to_new_track`: a mixed-down track is a copy that goes stale when a source track changes. `set_effect_params`, `set_effect_bypassed` and `remove_effect` with scope=master change or remove it. Use `mix_to_new_track` only to commit a stem mix. Use `plot_spectrum` to compare frequency balance between tracks.
