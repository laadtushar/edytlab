//! Level processors that carry no state.
//!
//! Both of these are per-sample and memoryless, which makes them
//! trivially safe to run on a stream: the output of sample `n` depends
//! on sample `n` and nothing else, so no chunking can change it. They
//! are the right things to build a render-time chain on first, because
//! any bug that shows up is in the plumbing rather than in the DSP.

use crate::Processor;

/// Constant gain in dB.
#[derive(Debug, Clone, Copy)]
pub struct Gain {
    linear: f32,
}

impl Gain {
    pub fn from_db(db: f32) -> Self {
        Self {
            // A non-finite dB value would poison every sample it
            // touched; treat it as unity, which is the harmless reading.
            linear: if db.is_finite() {
                10.0f32.powf(db / 20.0)
            } else {
                1.0
            },
        }
    }

    pub fn linear(&self) -> f32 {
        self.linear
    }
}

impl Processor for Gain {
    fn process(&mut self, chunk: &mut [f32], _channels: usize) {
        for s in chunk.iter_mut() {
            *s *= self.linear;
        }
    }

    fn reset(&mut self) {}
}

/// Brick-wall limiter: hard-clip anything past the ceiling.
///
/// The same arithmetic as `effects::limiter::apply_limiter`, in
/// streaming form. Deliberately not a wrapper around it — the one-shot
/// version takes `&mut [f32]` and would work, but going through
/// `Processor` keeps every chain element the same shape.
#[derive(Debug, Clone, Copy)]
pub struct Limiter {
    ceiling: f32,
}

impl Limiter {
    pub fn from_db(ceiling_db: f32) -> Self {
        Self {
            // An infinite or NaN ceiling would clamp everything to
            // silence or to NaN. Full scale is the reading that does
            // nothing, which is the safe failure.
            ceiling: if ceiling_db.is_finite() {
                10.0f32.powf(ceiling_db / 20.0)
            } else {
                1.0
            },
        }
    }
}

impl Processor for Limiter {
    fn process(&mut self, chunk: &mut [f32], _channels: usize) {
        for s in chunk.iter_mut() {
            if s.abs() > self.ceiling {
                *s = s.signum() * self.ceiling;
            }
        }
    }

    fn reset(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gain_of_zero_db_is_unity() {
        let mut buf = vec![0.5f32, -0.25, 0.0];
        let before = buf.clone();
        Gain::from_db(0.0).process(&mut buf, 1);
        assert_eq!(buf, before);
    }

    #[test]
    fn minus_six_db_roughly_halves() {
        let mut buf = vec![1.0f32];
        Gain::from_db(-6.0).process(&mut buf, 1);
        assert!((buf[0] - 0.501).abs() < 0.01, "got {}", buf[0]);
    }

    #[test]
    fn limiter_clamps_both_polarities_and_leaves_the_rest() {
        let mut buf = vec![0.1f32, 0.9, -0.9, -0.1];
        Limiter::from_db(-6.0).process(&mut buf, 1);
        let ceiling = 10.0f32.powf(-6.0 / 20.0);
        assert_eq!(buf[0], 0.1, "under the ceiling, untouched");
        assert!((buf[1] - ceiling).abs() < 1e-6);
        assert!((buf[2] + ceiling).abs() < 1e-6);
        assert_eq!(buf[3], -0.1);
    }

    /// Non-finite parameters must degrade to a no-op rather than
    /// silencing or NaN-ing the whole render.
    #[test]
    fn non_finite_parameters_are_inert() {
        let mut buf = vec![0.5f32; 4];
        Gain::from_db(f32::NAN).process(&mut buf, 1);
        assert_eq!(buf, vec![0.5f32; 4]);

        let mut buf = vec![0.5f32; 4];
        Limiter::from_db(f32::INFINITY).process(&mut buf, 1);
        assert_eq!(buf, vec![0.5f32; 4]);
    }

    /// The contract the renderer depends on. Both are memoryless, so
    /// this should hold trivially — which is exactly why they are the
    /// right things to build the chain on first.
    #[test]
    fn both_are_chunk_invariant() {
        let signal: Vec<f32> = (0..1_000).map(|n| (n as f32 / 500.0) - 1.0).collect();

        for chunk_len in [1usize, 3, 128, 1_000] {
            let mut whole = signal.clone();
            Gain::from_db(-3.0).process(&mut whole, 2);
            Limiter::from_db(-6.0).process(&mut whole, 2);

            let mut piecewise = signal.clone();
            let mut gain = Gain::from_db(-3.0);
            let mut lim = Limiter::from_db(-6.0);
            for c in piecewise.chunks_mut(chunk_len) {
                gain.process(c, 2);
                lim.process(c, 2);
            }
            assert_eq!(whole, piecewise, "chunk length {chunk_len} changed output");
        }
    }
}
