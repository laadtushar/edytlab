//! Peaking-EQ bands.
//!
//! ## Why this is not the shared biquad
//!
//! `crate::biquad` already has a Direct Form II biquad, and this is a
//! second one. That is deliberate, not an oversight:
//!
//! * **Precision.** This one is `f64` throughout. A peaking band at
//!   high Q is the case where `f32` coefficient error is audible, and
//!   the shared filter is `f32` because the render path's other users
//!   don't need more.
//! * **Filter type.** `BiquadCoeffs` in `crate::biquad` builds
//!   high-pass, low-pass and notch. Peaking EQ is a different cookbook
//!   formula with a gain term, and its state is transposed Direct Form
//!   II (`w1`/`w2`) rather than the direct form used there.
//!
//! Unifying them would mean either dropping this to `f32`, which
//! changes every EQ render, or promoting the shared one to `f64`, which
//! changes every filter render. Neither belongs in a move whose whole
//! claim is that output is unchanged. If they should converge, that is
//! its own change with its own before/after evidence.

/// One peaking-EQ band.
///
/// Deliberately not the tool's `Band`: that one derives `Deserialize`,
/// and this crate has no serde dependency and should keep it that way —
/// it sits under the render path, so anything it pulls in is pulled
/// into rendering.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EqBand {
    pub freq_hz: f32,
    pub gain_db: f32,
    pub q: f32,
}

// ---------------------------------------------------------------------------
// Biquad peak-EQ filter
// ---------------------------------------------------------------------------

/// Coefficients for one biquad peak-EQ band, pre-normalised by a0.
struct BiquadCoeffs {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
}

/// Normalised angular frequency for a band, held safely below Nyquist.
///
/// The cookbook formulas assume `0 < w0 < π`. Past Nyquist `sin(w0)`
/// turns negative, which flips the sign of `alpha` and can drive `a0`
/// through zero — the band stops shaping the signal and starts
/// diverging exponentially, so the render saturates into a full-scale
/// square wave. A band above Nyquist has nothing to act on anyway, so
/// the frequency is clamped rather than rejected. `crate::tool::util`
/// carries the same guard for the shared biquad constructors.
fn safe_w0(freq_hz: f32, sample_rate: u32) -> f64 {
    let sr = sample_rate.max(1) as f64;
    // 0.45·sr leaves headroom below Nyquist, where the bilinear
    // transform's frequency warping is still well behaved.
    let ceiling = (sr * 0.45).max(1.0);
    let f = freq_hz as f64;
    let clamped = if f.is_finite() {
        f.clamp(1.0, ceiling)
    } else {
        ceiling
    };
    2.0 * std::f64::consts::PI * clamped / sr
}

impl BiquadCoeffs {
    fn peak_eq(freq_hz: f32, gain_db: f32, q: f32, sample_rate: u32) -> Self {
        let a = 10f64.powf(gain_db as f64 / 40.0);
        let w0 = safe_w0(freq_hz, sample_rate);
        let alpha = w0.sin() / (2.0 * q as f64);

        let a0 = 1.0 + alpha / a;
        BiquadCoeffs {
            b0: (1.0 + alpha * a) / a0,
            b1: (-2.0 * w0.cos()) / a0,
            b2: (1.0 - alpha * a) / a0,
            a1: (-2.0 * w0.cos()) / a0,
            a2: (1.0 - alpha / a) / a0,
        }
    }
}

/// Per-channel delay state for direct-form-II transposed.
#[derive(Clone, Default)]
struct BiquadState {
    w1: f64,
    w2: f64,
}

impl BiquadState {
    fn process(&mut self, x: f64, c: &BiquadCoeffs) -> f64 {
        let y = c.b0 * x + self.w1;
        self.w1 = c.b1 * x - c.a1 * y + self.w2;
        self.w2 = c.b2 * x - c.a2 * y;
        y
    }
}

/// Apply a chain of peak-EQ bands to an interleaved f32 buffer.
///
/// `channels` is the interleave stride. Each channel gets independent
/// biquad state so the filter is correct regardless of channel count.
pub fn apply_eq(samples: &mut [f32], channels: usize, sample_rate: u32, bands: &[EqBand]) {
    if channels == 0 || bands.is_empty() || samples.is_empty() {
        return;
    }

    // Pre-compute coefficients for each band.
    let coeffs: Vec<BiquadCoeffs> = bands
        .iter()
        .map(|b| BiquadCoeffs::peak_eq(b.freq_hz, b.gain_db, b.q, sample_rate))
        .collect();

    // One state vector per (band, channel).
    let mut states: Vec<Vec<BiquadState>> = coeffs
        .iter()
        .map(|_| vec![BiquadState::default(); channels])
        .collect();

    for (i, sample) in samples.iter_mut().enumerate() {
        let ch = i % channels;
        let mut x = *sample as f64;
        for (band_idx, coeff) in coeffs.iter().enumerate() {
            x = states[band_idx][ch].process(x, coeff);
        }
        *sample = x as f32;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rms(samples: &[f32]) -> f32 {
        if samples.is_empty() {
            return 0.0;
        }
        let sum: f32 = samples.iter().map(|s| s * s).sum();
        (sum / samples.len() as f32).sqrt()
    }
    #[test]
    fn unity_gain_passthrough() {
        let sr = 44100u32;
        let freq = 1000.0f32;
        let n = sr as usize;
        let mut samples: Vec<f32> = (0..n)
            .map(|i| (2.0 * std::f32::consts::PI * freq * i as f32 / sr as f32).sin())
            .collect();
        let original = samples.clone();

        let bands = vec![EqBand {
            freq_hz: 1000.0,
            gain_db: 0.0,
            q: 1.0,
        }];
        apply_eq(&mut samples, 1, sr, &bands);

        // Unity gain band should leave samples essentially unchanged.
        for (a, b) in original.iter().zip(samples.iter()) {
            assert!(
                (a - b).abs() < 1e-5,
                "sample changed by more than 1e-5: {a} vs {b}"
            );
        }
    }
    #[test]
    fn boost_increases_rms_near_freq() {
        let sr = 44100u32;
        let freq = 1000.0f32;
        let n = sr as usize;
        let mut samples: Vec<f32> = (0..n)
            .map(|i| (2.0 * std::f32::consts::PI * freq * i as f32 / sr as f32).sin())
            .collect();
        let rms_before = rms(&samples);

        let bands = vec![EqBand {
            freq_hz: 1000.0,
            gain_db: 6.0,
            q: 1.0,
        }];
        apply_eq(&mut samples, 1, sr, &bands);

        let rms_after = rms(&samples);
        assert!(
            rms_after > rms_before * 1.5,
            "expected RMS to increase substantially: before={rms_before}, after={rms_after}"
        );
    }
    /// A band above Nyquist must not turn the EQ into an oscillator.
    ///
    /// Unclamped, `peak_eq(30_000, …)` at 44.1 kHz puts the poles
    /// outside the unit circle and the buffer grows without bound —
    /// the render clips into a full-scale square wave.
    #[test]
    fn band_above_nyquist_stays_bounded() {
        let sr = 44100u32;
        let mut samples: Vec<f32> = (0..sr as usize)
            .map(|i| (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / sr as f32).sin() * 0.5)
            .collect();

        let bands = vec![EqBand {
            freq_hz: 30_000.0,
            gain_db: 6.0,
            q: 1.0,
        }];
        apply_eq(&mut samples, 1, sr, &bands);

        let peak = samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(
            peak.is_finite() && peak < 4.0,
            "a band above Nyquist must stay bounded, got peak {peak}"
        );
    }
}
