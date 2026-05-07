# `stems_30s_pop_clip/` — Demucs reference fixture (M18)

This directory holds the per-stem ground-truth WAVs that gate
acceptance criterion #1 of M18 (`output stems' RMS correlation with
ground-truth stems > 0.85 per stem`).

**Status (Phase 2 sandbox):** intentionally empty.

## Why empty

Sourcing the fixture requires two things this sandbox can't do
automatically:

1. A 30-second public-domain (or otherwise license-clean) pop clip
   that exercises all four Demucs stems. Free Music Archive has
   plenty of candidates — the M28 demo milestone picks one and
   commits it.
2. The four ground-truth stems for that clip. These come either from
   running a reference Demucs PyTorch checkpoint over the clip
   off-line, or from the source multitrack if the artist publishes
   stems.

Both steps are blocked on M28's "pick a license-clean track" gate.

## Expected layout once landed

```
tests/golden/stems_30s_pop_clip/
├── README.md          (this file)
├── source.wav         (30 sec input mix, 44.1 kHz stereo)
├── vocals.wav         (ground-truth vocals stem)
├── drums.wav          (ground-truth drums stem)
├── bass.wav           (ground-truth bass stem)
└── other.wav          (ground-truth "other" stem)
```

All five WAVs at the same sample rate, channel count, and length as
`source.wav`.

## Test gating

`crates/ml-demucs/tests/cache_smoke.rs` checks for
`assets/models/htdemucs.onnx` first; when that file lands the
sandbox-gating test (`separate_returns_not_implemented_in_phase2_sandbox`)
flips from "expect ModelMissing" to a real RMS-correlation assertion
against the four reference stems above.

The tool-side smoke
(`crates/tools/tests/tools_integration.rs::separate_stems_returns_actionable_error_when_model_missing`)
stays as-is — it's a stable contract regardless of whether the model
is present.

## Acceptance criterion

For each of the four stems:

```
correlate(rms_envelope(generated), rms_envelope(ground_truth)) > 0.85
```

over a 50-ms hop window. Spectral metric per the plan §M18 — RMS
envelope correlation rather than waveform correlation, because Demucs
outputs are not phase-aligned with the source.
