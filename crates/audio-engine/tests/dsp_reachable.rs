//! `audio-engine` can call the shared DSP.
//!
//! This looks trivial and is the whole point of #101. Before the
//! extraction every effect algorithm was `pub(crate)` inside
//! `crates/tools`, and `tools` depends on `audio-engine` rather than the
//! reverse — so the render path could not reach them at all. Wiring it
//! up would have needed either a circular dependency, which Cargo
//! rejects, or a second copy of each algorithm living here.
//!
//! A second copy is not a hypothetical risk. `eq`, `compressor` and
//! `noise_reduction` each carried a hand-copied version of the shared
//! destructive-edit path, and every copy kept the `clips.first()` bug
//! after the original was fixed, because none of them called it.
//!
//! So this test exists to fail loudly if someone removes the
//! `audio-dsp` dependency as "unused" before the render-time chain
//! (#110, #102) lands and starts using it in earnest. It is a guard on
//! the crate graph, not on the arithmetic — `audio-dsp` tests that
//! itself.

use audio_dsp::{Biquad, BiquadCoeffs, Processor};

/// A low-pass has to attenuate a tone well above its cutoff.
///
/// Deliberately a behavioural assertion rather than "it compiles": a
/// dependency that resolves but whose functions are never exercised is
/// exactly the kind of thing that rots unnoticed.
#[test]
fn the_render_crate_can_run_a_filter() {
    let sample_rate = 48_000u32;
    let tone: Vec<f32> = (0..4_800)
        .map(|n| (2.0 * std::f32::consts::PI * 8_000.0 * n as f32 / sample_rate as f32).sin() * 0.5)
        .collect();

    let mut filtered = tone.clone();
    Biquad::new(BiquadCoeffs::low_pass(500.0, sample_rate), 1).process(&mut filtered, 1);

    // Measure past the filter's settling time so the comparison is of
    // steady state rather than of the initial transient.
    let rms = |x: &[f32]| (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt();
    let before = rms(&tone[1_000..]);
    let after = rms(&filtered[1_000..]);

    assert!(
        after < before * 0.1,
        "a 500 Hz low-pass should crush an 8 kHz tone: {before:.4} -> {after:.4}"
    );
}

/// The property the renderer depends on: it feeds audio one chunk at a
/// time, and a processor whose state reset at each boundary would make
/// the output depend on the chunk size — which `render.rs` documents
/// that it does not.
#[test]
fn a_processor_survives_being_fed_in_render_sized_chunks() {
    let sample_rate = 48_000u32;
    let signal: Vec<f32> = (0..10_000)
        .map(|n| (2.0 * std::f32::consts::PI * 300.0 * n as f32 / sample_rate as f32).sin() * 0.4)
        .collect();
    let coeffs = BiquadCoeffs::low_pass(2_000.0, sample_rate);

    let mut whole = signal.clone();
    Biquad::new(coeffs, 2).process(&mut whole, 2);

    // A short final chunk is the case the render path actually hits.
    let mut chunked = signal.clone();
    let mut proc = Biquad::new(coeffs, 2);
    for chunk in chunked.chunks_mut(3_333) {
        proc.process(chunk, 2);
    }

    assert_eq!(
        whole, chunked,
        "chunking changed the output; state is not crossing the seams"
    );
}
