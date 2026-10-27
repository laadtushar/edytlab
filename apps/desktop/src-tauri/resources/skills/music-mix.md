---
name: music-mix
description: Music mixing and mastering workflow
trigger: regex
pattern: \b(mixing|mix ?down|mastering|music|songs?|stems?|instrumentals?|beats?|drums?|bass)\b
enabled: true
---

## Music Mixing Workflow

For music tracks, suggested workflow:

1. **Gain staging**: `gain` — set each track so the mix peaks around -6 dBFS before processing
2. **EQ**: `eq` — cut competing frequencies between instruments; each instrument owns a frequency range
3. **Compression**: `compressor` — tighten transients; gentle settings (ratio=2.0) for glue compression
4. **Reverb**: `reverb` room_size=0.4, wet=0.2 — add space; keep wet low for upfront sounds
5. **Stereo width**: `stereo_widener` — widen pads and synths, keep kick and bass narrow
6. **Limiter, once**: `add_effect` scope=master kind=limiter params={"ceiling_db": -1.0} — prevent digital clipping on the final output. On the master bus it limits the whole mix and keeps limiting when a track is edited later. If the session block already lists a limiter on the master, change it with `set_effect_params` scope=master instead of adding a second. Do not use the `limiter` tool here: it bakes into one track's file and the master bus never sees it

Effects on the whole mix (a limiter, a gain, a filter) belong on the master bus with `add_effect` scope=master, not on a track made by `mix_to_new_track`: a mixed-down track is a copy that goes stale when a source track changes. `set_effect_params`, `set_effect_bypassed` and `remove_effect` with scope=master change or remove it. Use `mix_to_new_track` only to commit a stem mix. Use `plot_spectrum` to compare frequency balance between tracks.

Stop: `measure` (omit `track` to measure the mix) and check `clipping.events` is 0 and `peak.true_dbtp` is at or under -1.0. If not, lower the limiter's ceiling by the overshoot and measure again (three passes at most). For a platform loudness target (Spotify, YouTube), follow loudness-master after this: its limiter is this one.
