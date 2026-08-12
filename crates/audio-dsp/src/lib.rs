//! Sample-level DSP, shared by the destructive tools and the renderer.
//!
//! ## Why this crate exists
//!
//! Every effect algorithm used to live `pub(crate)` inside
//! `crates/tools`, and `tools` depends on `audio-engine` rather than the
//! other way round. Render-time effects have to run *inside*
//! `audio-engine`, so reaching the algorithms from there would have
//! meant either a circular dependency — which Cargo rejects outright —
//! or a second copy of each algorithm in the engine.
//!
//! A second copy is not hypothetical: `eq`, `compressor` and
//! `noise_reduction` each carried a hand-copied version of the shared
//! destructive-edit path, and all three copies kept the `clips.first()`
//! bug after the original was fixed, because none of them called it.
//!
//! So this crate sits *below* both. `tools` calls it for one-shot
//! destructive edits; `audio-engine` calls it for render-time chains.
//! One implementation, two callers, no cycle.
//!
//! ## Shape
//!
//! Two forms, deliberately:
//!
//! * **One-shot** functions over a whole buffer, which is what a
//!   destructive tool wants — it has the entire track in memory and
//!   writes a new file.
//! * **[`Processor`]** implementations, which carry their state across
//!   calls. The renderer works in chunks (one second at a time), and an
//!   algorithm with internal state — a filter's delay line, a
//!   compressor's envelope follower, a reverb's comb buffers — produces
//!   different output if that state is reset at every chunk boundary.
//!
//! The one-shot form is a thin wrapper over the streaming one wherever
//! both exist, so there is still only one implementation to be wrong.
//!
//! ## Determinism
//!
//! `audio-engine`'s render path commits to byte-identical output for a
//! given input. Everything here is plain sequential float arithmetic in
//! a fixed order — no parallel reductions, no iteration over hash maps,
//! nothing whose result depends on how the work was scheduled. Keep it
//! that way; see `render.rs`'s determinism invariant.

pub mod biquad;

pub use biquad::{Biquad, BiquadCoeffs, BiquadState};

/// A stateful audio processor that can be fed a signal in pieces.
///
/// The renderer hands out one chunk at a time and the same processor
/// instance sees every chunk in order, so anything that needs history —
/// a delay line, an envelope follower — keeps working across the seams.
///
/// Implementations must satisfy one property, and it is load-bearing:
/// **the output must not depend on how the input was divided into
/// chunks.** Feeding a buffer in one call, or in a hundred, must produce
/// the same samples. `audio-engine` documents that its master chunk size
/// does not affect output bytes, and a processor that resets state per
/// call would quietly make that false.
pub trait Processor {
    /// Process one chunk of interleaved samples in place.
    ///
    /// `channels` is the interleave stride and is constant for the
    /// lifetime of the processor. `chunk.len()` need not be a multiple
    /// of it — the renderer's final chunk is short — so implementations
    /// index by sample rather than assuming whole frames.
    fn process(&mut self, chunk: &mut [f32], channels: usize);

    /// Drop any accumulated state, as if the processor were new.
    ///
    /// Used when the same processor is reused across independent
    /// renders; not called mid-signal.
    fn reset(&mut self);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The invariant every `Processor` owes the renderer, exercised on
    /// the one implementation this crate starts with.
    ///
    /// A biquad is the cheapest possible way to get this wrong twice
    /// over: reset its `z1`/`z2` at each call and the output picks up a
    /// discontinuity at every seam; derive the channel from the position
    /// *within* the chunk and an odd-length chunk swaps the channels
    /// from there on.
    ///
    /// **Stereo with odd chunk lengths on purpose.** An earlier version
    /// of this test used mono and passed while the channel-phase bug was
    /// live — mono cannot express it, because every sample belongs to
    /// the same channel. The odd lengths below are what force a chunk to
    /// start mid-frame.
    #[test]
    fn chunking_does_not_change_a_processors_output() {
        let sr = 48_000;
        let channels = 2;
        // Distinct content per channel, so a swap is visible at all.
        let signal: Vec<f32> = (0..4_000)
            .map(|i| {
                let n = (i / channels) as f32;
                let hz = if i % channels == 0 { 500.0 } else { 3_000.0 };
                (2.0 * std::f32::consts::PI * hz * n / sr as f32).sin() * 0.5
            })
            .collect();
        let coeffs = BiquadCoeffs::low_pass(1_000.0, sr);

        let mut whole = signal.clone();
        Biquad::new(coeffs, channels).process(&mut whole, channels);

        for chunk_len in [1usize, 3, 7, 64, 999, 4_000] {
            let mut piecewise = signal.clone();
            let mut proc = Biquad::new(coeffs, channels);
            for chunk in piecewise.chunks_mut(chunk_len) {
                proc.process(chunk, channels);
            }
            let worst = whole
                .iter()
                .zip(piecewise.iter())
                .fold(0.0f32, |m, (a, b)| m.max((a - b).abs()));
            assert!(
                worst < 1e-6,
                "chunk length {chunk_len} changed the output by {worst}; \
                 either the filter state or the channel phase is not \
                 surviving the seam"
            );
        }
    }

    #[test]
    fn reset_returns_a_processor_to_its_initial_state() {
        let coeffs = BiquadCoeffs::low_pass(1_000.0, 48_000);
        let mut proc = Biquad::new(coeffs, 1);

        let mut first = vec![1.0f32; 64];
        proc.process(&mut first, 1);

        // Without the reset the decaying tail of the first burst would
        // still be in the delay line and would leak into this one.
        proc.reset();
        let mut second = vec![1.0f32; 64];
        proc.process(&mut second, 1);

        let mut fresh = vec![1.0f32; 64];
        Biquad::new(coeffs, 1).process(&mut fresh, 1);
        assert_eq!(second, fresh, "reset did not clear the delay line");
    }
}
