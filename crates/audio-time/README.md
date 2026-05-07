# audio-time

Time-stretch and pitch-shift primitives for the edytlab audio pipeline.

## Phase 2 status: stub

The plan for M20 calls for `rubberband-sys` (FFI to the Rubber Band C++
library) as the backend. Building it requires the system Rubber Band
library plus a working C++ toolchain on every target — `librubberband-dev`
on Linux, `vcpkg` on Windows, `brew` or `vcpkg` on macOS. M20's risk
register flags this FFI layer as Medium risk; to keep the M20 PR scoped
to API + caching + tool integration, the actual DSP is deferred to M28.

Until M28 lands, the public functions in this crate (`time_stretch`,
`pitch_shift`) validate their arguments and return
`Error::NotImplemented`. The session-level tools in `crates/tools`
(`time_stretch`, `pitch_shift`, `align_to_beat`) record the requested
parameters on the targeted clip; the audio engine will apply them at
render time once M22+ teaches it about per-clip time/pitch transforms.

## Tests

```sh
cargo test -p audio-time
```

The integration tests in `tests/integration.rs` cover argument
validation and the `NotImplemented` contract. The plan's quantitative
acceptance criteria (round-trip RMS ≤ -40 dBFS, formant preservation
within 5%, frequency tolerance ±2 Hz) gate the M28 PR, not this one.
