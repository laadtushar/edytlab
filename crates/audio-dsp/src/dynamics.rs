//! Level processors: a constant [`Gain`] and a peak [`Limiter`].
//!
//! [`Gain`] is per-sample and memoryless, so no chunking can change its
//! output. [`Limiter`] carries one number between calls — how much gain
//! reduction is still being recovered — which is what makes it a limiter
//! rather than a clip, and is why it is a [`Processor`] with the
//! chunk-invariance contract that implies.

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

/// Release time, in milliseconds, used when the caller does not give one.
///
/// Slow enough that the gain does not re-open inside every cycle of a
/// 50 Hz bass note (a release shorter than the period rides the waveform
/// itself, which is a distortion of its own), fast enough that the dip a
/// kick drum causes is gone well before the next beat.
pub const DEFAULT_RELEASE_MS: f32 = 80.0;

/// Shortest release [`Limiter::new`] will use. Below a millisecond the
/// gain follows individual cycles of audible frequencies and the
/// limiter turns back into a clipper; a release of zero *is* a clipper.
pub const MIN_RELEASE_MS: f32 = 1.0;

/// Longest release [`Limiter::new`] will use. Five seconds is already a
/// level ride rather than a limiter.
pub const MAX_RELEASE_MS: f32 = 5_000.0;

/// Gain reduction below this (about -0.0000009 dB) is dropped to zero
/// rather than chased forever, so that after a transient the limiter
/// really does return to exactly unity and passes audio bit-for-bit
/// again. The reduction decays geometrically and never reaches zero on
/// its own.
const REDUCTION_FLOOR: f64 = 1e-7;

/// Zero-latency peak limiter with instant attack and exponential
/// release.
///
/// It replaces a hard clip. The old limiter replaced every sample past
/// the ceiling with the ceiling, which flattens the top of each
/// transient and adds harmonics that were not in the source. This one
/// turns the *gain* down instead, so the waveform keeps its shape and
/// only its level moves.
///
/// ## What it guarantees
///
/// * **The output never exceeds the ceiling.** At every frame the gain
///   applied is at most `ceiling / peak`, where `peak` is the largest
///   `|x|` across all channels in that frame. There is no lookahead, so
///   the gain has to arrive on the very sample that needs it: attack is
///   instant, and this holds for any input, however abrupt. It is a
///   *sample*-peak ceiling, not an inter-sample (true-peak) one.
/// * **Zero latency.** Output sample `n` is a function of input up to
///   and including sample `n`. Nothing is delayed, so a chain
///   containing a limiter needs no latency compensation.
/// * **Channel-linked.** One gain per frame, applied to every channel,
///   chosen by the loudest channel. A hard-left transient turns the
///   right channel down by the same amount instead of leaving it alone,
///   so the stereo image does not lurch.
/// * **Untouched when there is nothing to do.** With no gain reduction
///   in force and every sample under the ceiling, samples pass through
///   bit-for-bit.
///
/// ## How the gain moves
///
/// When a frame needs more reduction than is currently applied, the gain
/// drops to exactly what it needs, immediately. Otherwise the reduction
/// decays by a fixed factor per frame, `exp(-1 / (release * rate))`, so
/// the gain climbs back to unity along a single exponential: after one
/// release time the remaining reduction is `1/e` of what it was, after
/// five it is under 1%. The coefficient comes from the sample rate, so
/// a given `release_ms` is the same duration at 8 kHz and at 192 kHz.
///
/// ## What it costs
///
/// Instant attack with no lookahead means the gain *steps* on the sample
/// that first crosses the ceiling, where a lookahead limiter would have
/// eased in ahead of it. The step is small next to a clip (it is a level
/// change of a few dB on the loudest part of the signal, not the
/// removal of everything past the ceiling) but it is not zero. A
/// lookahead mode would trade latency for removing it.
///
/// ## Chunking
///
/// State persists between [`process`](Processor::process) calls, and
/// feeding a signal in whole frames, in any block sizes, gives
/// bit-identical output to feeding it at once. The renderer only ever
/// hands out whole frames.
///
/// A chunk that *ends in the middle of a frame* is the one case the
/// [`Processor`] contract cannot be fully honoured: the first channels
/// of that frame must be written before the last ones exist, so their
/// gain cannot depend on them. The ceiling still holds (the late
/// channels are tightened to it, and the tightened gain carries on into
/// the next frame) but the split frame's gain may differ from the one
/// an unsplit run would have chosen.
///
/// ## Non-finite values
///
/// A non-finite `ceiling_db` falls back to full scale and a non-finite
/// `release_ms` to [`DEFAULT_RELEASE_MS`], as before. An infinite sample
/// is clamped to `±ceiling` and a `NaN` sample passes through
/// untouched, both exactly as the hard clip treated them; neither is
/// allowed to move the gain, so one bad sample cannot mute what follows.
#[derive(Debug, Clone)]
pub struct Limiter {
    /// Linear ceiling. Never negative, never `NaN`.
    ceiling: f32,
    /// Fraction of the gain reduction that survives one frame.
    release_coeff: f64,
    /// Gain reduction still in force (`1 - gain`), in `0.0..=1.0`.
    ///
    /// Kept apart from the gain because `1 - reduction` stops moving
    /// once the reduction falls under the spacing of `f32` below 1.0,
    /// which would strand the gain a hair short of unity for good.
    reduction: f64,
    /// Gain applied to the frame in progress.
    frame_gain: f32,
    /// Channel index of the next sample: non-zero only between a call
    /// that ended mid-frame and the call that finishes it.
    phase: usize,
}

impl Limiter {
    /// A limiter with its ceiling at `ceiling_db` dBFS, recovering over
    /// `release_ms` milliseconds, for audio at `sample_rate` Hz.
    ///
    /// `release_ms` is clamped to [`MIN_RELEASE_MS`]..=[`MAX_RELEASE_MS`].
    pub fn new(ceiling_db: f32, release_ms: f32, sample_rate: u32) -> Self {
        // An infinite or NaN ceiling would clamp everything to silence
        // or to NaN. Full scale is the reading that does nothing to
        // in-range audio, which is the safe failure.
        let ceiling = if ceiling_db.is_finite() {
            10.0f32.powf(ceiling_db / 20.0)
        } else {
            1.0
        };
        let release_ms = if release_ms.is_finite() {
            release_ms.clamp(MIN_RELEASE_MS, MAX_RELEASE_MS)
        } else {
            DEFAULT_RELEASE_MS
        };
        let rate = f64::from(sample_rate.max(1));
        Self {
            ceiling,
            release_coeff: (-1.0 / (f64::from(release_ms) * 0.001 * rate)).exp(),
            reduction: 0.0,
            frame_gain: 1.0,
            phase: 0,
        }
    }

    /// The ceiling as a linear amplitude. No output sample exceeds it.
    pub fn ceiling(&self) -> f32 {
        self.ceiling
    }

    /// The gain applied to the most recent frame, `1.0` when the
    /// limiter is idle. Useful as a gain-reduction meter.
    pub fn gain(&self) -> f32 {
        self.frame_gain
    }

    /// Start a frame: let the reduction in force recover by one step.
    fn release(&mut self) {
        self.reduction *= self.release_coeff;
        if self.reduction < REDUCTION_FLOOR {
            self.reduction = 0.0;
        }
        self.frame_gain = (1.0 - self.reduction) as f32;
    }

    /// Bring the gain down, if need be, so that a sample of magnitude
    /// `peak` fits under the ceiling. Never raises it.
    fn attack(&mut self, peak: f32) {
        if peak > self.ceiling {
            let needed = self.ceiling / peak;
            if needed < self.frame_gain {
                self.frame_gain = needed;
                self.reduction = 1.0 - f64::from(needed);
            }
        }
    }

    /// Apply the frame's gain to one sample.
    fn shape(&self, x: f32) -> f32 {
        if x.is_finite() {
            // `x * gain` can land one ulp past the ceiling when the
            // gain is `ceiling / |x|` rounded. This clamp is that ulp
            // and nothing more: the gain has already done the work.
            (x * self.frame_gain).clamp(-self.ceiling, self.ceiling)
        } else if x.is_infinite() {
            x.signum() * self.ceiling
        } else {
            x
        }
    }

    /// One frame, or the leading part of one if the chunk ends inside it.
    fn limit_frame(&mut self, frame: &mut [f32]) {
        self.release();
        let peak = frame
            .iter()
            .filter(|s| s.is_finite())
            .fold(0.0f32, |m, s| m.max(s.abs()));
        self.attack(peak);
        for s in frame.iter_mut() {
            *s = self.shape(*s);
        }
    }

    /// A sample belonging to a frame whose earlier channels have
    /// already been written.
    fn limit_late(&mut self, x: f32) -> f32 {
        if x.is_finite() {
            self.attack(x.abs());
        }
        self.shape(x)
    }
}

impl Processor for Limiter {
    fn process(&mut self, chunk: &mut [f32], channels: usize) {
        let channels = channels.max(1);
        if self.phase >= channels {
            self.phase = 0;
        }

        let mut at = 0;
        // Finish the frame the previous call left open.
        if self.phase != 0 {
            let take = (channels - self.phase).min(chunk.len());
            for s in &mut chunk[..take] {
                *s = self.limit_late(*s);
            }
            self.phase = (self.phase + take) % channels;
            at = take;
        }

        while at < chunk.len() {
            let end = (at + channels).min(chunk.len());
            self.limit_frame(&mut chunk[at..end]);
            self.phase = (end - at) % channels;
            at = end;
        }
    }

    fn reset(&mut self) {
        self.reduction = 0.0;
        self.frame_gain = 1.0;
        self.phase = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: u32 = 48_000;

    fn limiter(ceiling_db: f32) -> Limiter {
        Limiter::new(ceiling_db, DEFAULT_RELEASE_MS, SR)
    }

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

    /// Was `limiter_clamps_both_polarities_and_leaves_the_rest`, which
    /// expected the trailing -0.1 to come through untouched. That was
    /// the clip talking: a limiter that has just turned the gain down
    /// for a 0.9 does not snap back to unity on the next sample, and
    /// the -0.1 behind it is turned down with it. What survives is the
    /// intent — both polarities are held to the ceiling, and nothing
    /// before the first over is touched.
    #[test]
    fn limiter_holds_both_polarities_to_the_ceiling_and_leaves_what_precedes_them() {
        let mut buf = vec![0.1f32, 0.9, -0.9, -0.1];
        limiter(-6.0).process(&mut buf, 1);
        let ceiling = 10.0f32.powf(-6.0 / 20.0);
        assert_eq!(buf[0], 0.1, "before the first over, untouched");
        assert!((buf[1] - ceiling).abs() < 1e-6);
        assert!((buf[2] + ceiling).abs() < 1e-6);

        // Still inside the release of the 0.9: turned down by roughly
        // the same factor, and only roughly, because it has begun to
        // recover.
        let applied = buf[3] / -0.1;
        let held = ceiling / 0.9;
        assert!(
            applied >= held && applied - held < 1e-3,
            "the sample after an over should carry the gain still in force \
             ({held:.4}), got {applied:.4}"
        );
    }

    /// Non-finite parameters must degrade to a no-op rather than
    /// silencing or NaN-ing the whole render.
    #[test]
    fn non_finite_parameters_are_inert() {
        let mut buf = vec![0.5f32; 4];
        Gain::from_db(f32::NAN).process(&mut buf, 1);
        assert_eq!(buf, vec![0.5f32; 4]);

        for ceiling_db in [f32::INFINITY, f32::NEG_INFINITY, f32::NAN] {
            let mut buf = vec![0.5f32; 4];
            Limiter::new(ceiling_db, DEFAULT_RELEASE_MS, SR).process(&mut buf, 1);
            assert_eq!(buf, vec![0.5f32; 4], "ceiling_db = {ceiling_db}");
        }

        // A nonsense release must not poison the coefficient: NaN there
        // would turn the first over into NaN gain for the rest of the
        // render.
        for release_ms in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -5.0, 0.0] {
            let mut buf = vec![0.1f32, 0.9, 0.1, 0.1];
            Limiter::new(-6.0, release_ms, SR).process(&mut buf, 1);
            assert!(
                buf.iter().all(|s| s.is_finite() && *s > 0.0),
                "release_ms = {release_ms}: {buf:?}"
            );
        }
    }

    /// The hard clip let `NaN` through and clamped `inf`. Neither may
    /// now drag the gain with it: a stray infinity would otherwise ask
    /// for a gain of zero and mute everything after it.
    #[test]
    fn a_non_finite_sample_does_not_poison_the_gain() {
        let mut buf = vec![0.25f32, f32::INFINITY, f32::NEG_INFINITY, f32::NAN, 0.25];
        let mut lim = limiter(-6.0);
        lim.process(&mut buf, 1);
        let ceiling = lim.ceiling();
        assert_eq!(buf[0], 0.25);
        assert_eq!(buf[1], ceiling);
        assert_eq!(buf[2], -ceiling);
        assert!(buf[3].is_nan(), "NaN passes through as it always did");
        assert_eq!(buf[4], 0.25, "the sample after the bad ones is untouched");
        assert_eq!(lim.gain(), 1.0);
    }

    /// The contract the renderer depends on.
    ///
    /// Whole frames, in any block size, must give the bytes a single
    /// call gives. Stereo is tested on even chunk lengths only; the
    /// mid-frame case is the documented exception, covered in
    /// `tests/limiter.rs`. Mono has no mid-frame, so it takes the odd
    /// lengths the old version of this test used on stereo.
    #[test]
    fn both_are_chunk_invariant() {
        let signal: Vec<f32> = (0..1_000).map(|n| (n as f32 / 500.0) - 1.0).collect();

        for (channels, lens) in [
            (1usize, vec![1usize, 3, 128, 1_000]),
            (2, vec![2, 6, 128, 1_000]),
        ] {
            let mut whole = signal.clone();
            Gain::from_db(-3.0).process(&mut whole, channels);
            limiter(-6.0).process(&mut whole, channels);

            for chunk_len in lens {
                let mut piecewise = signal.clone();
                let mut gain = Gain::from_db(-3.0);
                let mut lim = limiter(-6.0);
                for c in piecewise.chunks_mut(chunk_len) {
                    gain.process(c, channels);
                    lim.process(c, channels);
                }
                assert_eq!(
                    whole, piecewise,
                    "{channels} ch, chunk length {chunk_len} changed output"
                );
            }
        }
    }

    #[test]
    fn reset_forgets_the_gain_reduction() {
        let mut lim = limiter(-6.0);
        lim.process(&mut [2.0f32, 0.1], 1);
        assert!(lim.gain() < 1.0);

        lim.reset();
        let mut after = vec![0.1f32; 8];
        lim.process(&mut after, 1);
        assert_eq!(after, vec![0.1f32; 8], "reset left gain reduction behind");
    }
}
