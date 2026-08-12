//! Direct Form II biquad filtering.
//!
//! Moved here from `crates/tools/src/tool/util.rs` unchanged — the
//! coefficient formulas, the `safe_w0` clamp and the sample loop are
//! the same arithmetic in the same order, so filtered output is
//! bit-identical to before the move.
//!
//! Four tools share this (`low_pass_filter`, `high_pass_filter`,
//! `notch_filter`, `de_esser`), and the render path will want it too,
//! which is exactly why it could not stay `pub(crate)` in `tools`.

/// Direct Form II biquad state, one per channel.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct BiquadState {
    pub z1: f32,
    pub z2: f32,
}

impl BiquadState {
    pub fn new() -> Self {
        Self { z1: 0.0, z2: 0.0 }
    }
}

/// Biquad coefficients `[b0, b1, b2, a1, a2]`, with `a0` normalised to 1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BiquadCoeffs {
    pub b0: f32,
    pub b1: f32,
    pub b2: f32,
    pub a1: f32,
    pub a2: f32,
}

/// Normalised angular frequency for a biquad, held safely below Nyquist.
///
/// The coefficient formulas below assume `0 < w0 < π`. At or above
/// Nyquist `sin(w0)` goes to zero and then negative, which flips the
/// sign of `alpha` and pushes the filter's poles outside the unit
/// circle — it stops attenuating and starts diverging exponentially,
/// so the render saturates into a full-scale square wave. Asking a
/// 44.1 kHz track for a 30 kHz low-pass is an easy thing for a model to
/// do, and the intent ("pass everything") is clear, so the frequency is
/// clamped rather than rejected.
pub fn safe_w0(freq_hz: f32, sample_rate: u32) -> f32 {
    let sr = sample_rate.max(1) as f32;
    // 0.45·sr keeps a little headroom below Nyquist, where the bilinear
    // transform's frequency warping is still well behaved.
    let ceiling = sr * 0.45;
    let clamped = if freq_hz.is_finite() {
        freq_hz.clamp(1.0, ceiling.max(1.0))
    } else {
        ceiling.max(1.0)
    };
    2.0 * std::f32::consts::PI * clamped / sr
}

impl BiquadCoeffs {
    /// Second-order Butterworth high-pass filter.
    pub fn high_pass(cutoff_hz: f32, sample_rate: u32) -> Self {
        let w0 = safe_w0(cutoff_hz, sample_rate);
        let alpha = w0.sin() / (2.0 * 0.707_f32);
        let cos_w0 = w0.cos();
        let b0 = (1.0 + cos_w0) / 2.0;
        let b1 = -(1.0 + cos_w0);
        let b2 = (1.0 + cos_w0) / 2.0;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cos_w0;
        let a2 = 1.0 - alpha;
        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
        }
    }

    /// Second-order Butterworth low-pass filter.
    pub fn low_pass(cutoff_hz: f32, sample_rate: u32) -> Self {
        let w0 = safe_w0(cutoff_hz, sample_rate);
        let alpha = w0.sin() / (2.0 * 0.707_f32);
        let cos_w0 = w0.cos();
        let b0 = (1.0 - cos_w0) / 2.0;
        let b1 = 1.0 - cos_w0;
        let b2 = (1.0 - cos_w0) / 2.0;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cos_w0;
        let a2 = 1.0 - alpha;
        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
        }
    }

    /// Notch (band-reject) filter.
    pub fn notch(center_hz: f32, q: f32, sample_rate: u32) -> Self {
        let w0 = safe_w0(center_hz, sample_rate);
        let alpha = w0.sin() / (2.0 * q);
        let cos_w0 = w0.cos();
        let b0 = 1.0;
        let b1 = -2.0 * cos_w0;
        let b2 = 1.0;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cos_w0;
        let a2 = 1.0 - alpha;
        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
        }
    }
}

/// Process interleaved `samples` in-place with a biquad filter.
/// Only processes the frame range `[start_frame, end_frame)`.
///
/// One-shot: state starts clean and is discarded. That is what a
/// destructive tool wants, since it holds the whole track. Use
/// [`Biquad`] when the signal arrives in pieces.
pub fn biquad_process(
    samples: &mut [f32],
    channels: usize,
    coeffs: &BiquadCoeffs,
    start_frame: usize,
    end_frame: usize,
) {
    let channels = channels.max(1);
    let total_frames = samples.len() / channels;
    let end = end_frame.min(total_frames);
    let start = start_frame.min(end);
    let mut states: Vec<BiquadState> = (0..channels).map(|_| BiquadState::new()).collect();
    for frame in start..end {
        let base = frame * channels;
        for (ch, st) in states.iter_mut().enumerate() {
            let idx = base + ch;
            samples[idx] = step(coeffs, st, samples[idx]);
        }
    }
}

/// One sample through one channel's filter state.
///
/// Extracted so the one-shot and streaming paths cannot drift: both call
/// this, so there is a single definition of what the filter does.
#[inline]
fn step(coeffs: &BiquadCoeffs, st: &mut BiquadState, x: f32) -> f32 {
    let y = coeffs.b0 * x + st.z1;
    st.z1 = coeffs.b1 * x - coeffs.a1 * y + st.z2;
    st.z2 = coeffs.b2 * x - coeffs.a2 * y;
    y
}

/// A biquad that keeps its delay line across calls.
///
/// The renderer feeds a signal one chunk at a time. A filter whose
/// `z1`/`z2` were cleared at each chunk boundary would put a
/// discontinuity at every seam, and would make the render's output
/// depend on the chunk size — which `audio-engine` documents that it
/// does not.
#[derive(Debug, Clone)]
pub struct Biquad {
    coeffs: BiquadCoeffs,
    states: Vec<BiquadState>,
    /// Which channel the next sample belongs to.
    ///
    /// Carried across calls, and that is load-bearing. Chunk boundaries
    /// are not guaranteed to fall on frame boundaries — the renderer's
    /// final chunk is whatever is left — so deriving the channel from
    /// the position *within* the chunk sends the second chunk's left
    /// samples through the right channel's filter and vice versa. The
    /// output is then wrong in a way that depends on the chunk size,
    /// which is exactly what the streaming contract forbids.
    phase: usize,
}

impl Biquad {
    pub fn new(coeffs: BiquadCoeffs, channels: usize) -> Self {
        Self {
            coeffs,
            states: vec![BiquadState::new(); channels.max(1)],
            phase: 0,
        }
    }
}

impl crate::Processor for Biquad {
    fn process(&mut self, chunk: &mut [f32], channels: usize) {
        let channels = channels.max(1);
        if self.states.len() < channels {
            self.states.resize(channels, BiquadState::new());
        }
        // Per sample rather than per frame: a chunk can both start and
        // end mid-frame, and a frame-wise loop would drop the remainder.
        let mut ch = self.phase % channels;
        for s in chunk.iter_mut() {
            *s = step(&self.coeffs, &mut self.states[ch], *s);
            ch += 1;
            if ch == channels {
                ch = 0;
            }
        }
        self.phase = ch;
    }

    fn reset(&mut self) {
        for st in &mut self.states {
            *st = BiquadState::new();
        }
        self.phase = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Moved with the code it covers. A high-pass has to remove DC.
    #[test]
    fn high_pass_removes_dc() {
        let coeffs = BiquadCoeffs::high_pass(1000.0, 44100);
        let mut samples = vec![1.0f32; 4410];
        biquad_process(&mut samples, 1, &coeffs, 0, 4410);
        let tail_mean: f32 = samples[2000..].iter().sum::<f32>() / 2410.0;
        assert!(
            tail_mean.abs() < 0.01,
            "high-pass should remove DC, mean was {tail_mean}"
        );
    }

    /// Above Nyquist the naive coefficients diverge into a full-scale
    /// square wave. `safe_w0` clamps instead, so a nonsense cutoff
    /// degrades to "pass everything" rather than destroying the audio.
    #[test]
    fn a_cutoff_above_nyquist_does_not_explode() {
        let coeffs = BiquadCoeffs::low_pass(30_000.0, 44_100);
        let mut samples: Vec<f32> = (0..4410)
            .map(|n| (2.0 * std::f32::consts::PI * 440.0 * n as f32 / 44_100.0).sin() * 0.5)
            .collect();
        biquad_process(&mut samples, 1, &coeffs, 0, 4410);
        let peak = samples.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(peak < 1.5, "filter diverged, peak {peak}");
    }

    /// The streaming form fed everything at once must agree with the
    /// one-shot form. If these ever disagree the two paths have drifted,
    /// which is the failure this crate exists to prevent.
    #[test]
    fn streaming_and_one_shot_agree_when_given_the_whole_buffer() {
        use crate::Processor;

        let coeffs = BiquadCoeffs::low_pass(1_200.0, 44_100);
        let signal: Vec<f32> = (0..2_000)
            .map(|n| (2.0 * std::f32::consts::PI * 700.0 * n as f32 / 44_100.0).sin() * 0.4)
            .collect();

        let mut one_shot = signal.clone();
        biquad_process(&mut one_shot, 2, &coeffs, 0, 1_000);

        let mut streaming = signal.clone();
        Biquad::new(coeffs, 2).process(&mut streaming, 2);

        assert_eq!(
            one_shot, streaming,
            "the one-shot and streaming filters disagree"
        );
    }

    #[test]
    fn stereo_channels_filter_independently() {
        use crate::Processor;

        let coeffs = BiquadCoeffs::low_pass(1_000.0, 48_000);
        // Left is a burst, right is silence. If the states were shared,
        // the left channel's energy would leak into the right.
        let mut buf = vec![0.0f32; 400];
        for i in (0..400).step_by(2) {
            buf[i] = 1.0;
        }
        Biquad::new(coeffs, 2).process(&mut buf, 2);

        let right_energy: f32 = buf.iter().skip(1).step_by(2).map(|v| v.abs()).sum();
        assert_eq!(
            right_energy, 0.0,
            "the left channel bled into the right; states are shared"
        );
    }
}
