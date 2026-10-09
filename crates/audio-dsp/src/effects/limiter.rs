//! The destructive limiter: [`Limiter`], run over a whole buffer.
//!
//! This used to be its own hard clip, a second copy of the arithmetic in
//! `dynamics::Limiter`. It is a thin wrapper now, so the tool that
//! rewrites a file and the effect that runs at render time cannot
//! drift apart: there is one limiter, and this is the one-shot way to
//! call it.

use crate::dynamics::Limiter;
use crate::Processor;

/// Limit `samples` (interleaved, `channels` wide, at `sr` Hz) so that
/// none exceeds `ceiling_db` dBFS, recovering the gain over
/// `release_ms`.
///
/// The gain is shared by all channels of a frame and starts at unity at
/// the first sample. A buffer that is a selected *range* of a longer
/// track is therefore limited as if it stood alone: any gain reduction
/// still in force at its last sample is not carried into the audio
/// that follows, so a range that ends mid-release hands back to
/// unprocessed audio with a step in level.
pub fn apply_limiter(
    samples: &mut [f32],
    sr: u32,
    channels: usize,
    ceiling_db: f32,
    release_ms: f32,
) {
    Limiter::new(ceiling_db, release_ms, sr).process(samples, channels);
}

#[cfg(test)]
mod tests {
    use super::apply_limiter;
    use crate::dynamics::{Limiter, DEFAULT_RELEASE_MS};
    use crate::Processor;

    /// Was `clips_above_ceiling`; the guarantee is the same, only the
    /// way it is met has changed.
    #[test]
    fn nothing_exceeds_the_ceiling() {
        let mut samples = vec![0.5f32, 0.8, 1.5, -1.2, 0.3];
        apply_limiter(&mut samples, 44100, 1, -6.0, DEFAULT_RELEASE_MS);
        let ceiling = 10.0f32.powf(-6.0 / 20.0);
        for s in &samples {
            assert!(
                s.abs() <= ceiling + 1e-5,
                "sample {s} exceeds ceiling {ceiling}"
            );
        }
    }

    /// The one-shot form and the streaming form are the same limiter.
    /// If this fails, someone has given the destructive tool its own
    /// arithmetic again.
    #[test]
    fn one_shot_is_the_streaming_limiter() {
        let signal: Vec<f32> = (0..4_000)
            .map(|n| ((n as f32) * 0.05).sin() * 1.7 + ((n as f32) * 0.31).sin() * 0.4)
            .collect();

        let mut one_shot = signal.clone();
        apply_limiter(&mut one_shot, 44_100, 2, -3.0, 120.0);

        let mut streamed = signal;
        Limiter::new(-3.0, 120.0, 44_100).process(&mut streamed, 2);

        assert_eq!(one_shot, streamed);
    }
}
