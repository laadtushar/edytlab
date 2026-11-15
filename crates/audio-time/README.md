# audio-time

Time-stretch and pitch-shift primitives for the edytlab audio pipeline.

## Status

Implemented, on a pure-Rust phase vocoder (`src/vocoder.rs`, built on
`realfft`):

- `time_stretch` — duration without pitch
- `pitch_shift` — pitch without duration, with optional formant
  preservation (`src/formant.rs`)
- `warp_to_grid` — moves beats onto a target grid in a single vocoder
  pass, so there is no seam at the beats (`src/warp.rs`)
- `paulstretch` — Paul Nasca's extreme stretch (1x to 50x): magnitudes kept,
  phases drawn at random from a seed, overlap-added; not a vocoder, and
  built for ambient textures (`src/paulstretch.rs`)

The session-level tools in `crates/tools` (`time_stretch`, `pitch_shift`,
`align_to_beat`, `paulstretch`) call these directly.

The original plan (M20, with the DSP deferred to M28) was an FFI to the
Rubber Band C++ library. It was dropped: it needs a different native
package and a C++ toolchain on each of the three CI targets. The vocoder
resets phase on spectral-flux onsets and uses identity phase locking;
its remaining limits are documented in `src/vocoder.rs` and on the
functions themselves.

## Tests

```sh
cargo test -p audio-time
```

The integration tests in `tests/integration.rs` cover argument
validation, formant preservation (the resonances hold while the pitch
moves), and beat warping (a click track lands on the target grid, with
no discontinuity at segment boundaries and stereo kept frame-aligned).

`tests/paulstretch.rs` covers Paulstretch: output length, RMS and spectral
peak for a steady tone, seed determinism, stereo and other sample rates,
and refused arguments.
