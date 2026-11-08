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
    pub kind: BandKind,
}

/// Which Audio-EQ-Cookbook filter a band is. `Peak` is the original (and
/// default) behaviour; the shelves were added for #765.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BandKind {
    #[default]
    Peak,
    /// Boosts or cuts everything below `freq_hz` (the cookbook's lowShelf).
    LowShelf,
    /// Boosts or cuts everything above `freq_hz` (the cookbook's highShelf).
    HighShelf,
}

// ---------------------------------------------------------------------------
// Biquad peak-EQ filter
// ---------------------------------------------------------------------------

/// Coefficients for one biquad peak-EQ band, pre-normalised by a0.
#[derive(Debug, Clone, Copy)]
struct BiquadCoeffs {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
}

/// The highest centre or corner frequency a band keeps at `sample_rate`:
/// 0.45 of the rate, which leaves headroom below Nyquist, where the
/// bilinear transform's frequency warping is still well behaved. A band
/// above it is pulled down to it ([`safe_w0`]), so a caller that would
/// rather leave such a band out (the graphic EQ, #765) asks here.
pub fn band_ceiling_hz(sample_rate: u32) -> f64 {
    (f64::from(sample_rate.max(1)) * 0.45).max(1.0)
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
    let ceiling = band_ceiling_hz(sample_rate);
    let f = freq_hz as f64;
    let clamped = if f.is_finite() {
        f.clamp(1.0, ceiling)
    } else {
        ceiling
    };
    2.0 * std::f64::consts::PI * clamped / sr
}

impl BiquadCoeffs {
    fn for_band(b: &EqBand, sample_rate: u32) -> Self {
        match b.kind {
            BandKind::Peak => Self::peak_eq(b.freq_hz, b.gain_db, b.q, sample_rate),
            BandKind::LowShelf => Self::shelf(false, b.freq_hz, b.gain_db, b.q, sample_rate),
            BandKind::HighShelf => Self::shelf(true, b.freq_hz, b.gain_db, b.q, sample_rate),
        }
    }

    /// RBJ Audio-EQ-Cookbook low/high shelf, with Q in place of the
    /// cookbook's S: `alpha = sin(w0) / (2Q)`.
    fn shelf(high: bool, freq_hz: f32, gain_db: f32, q: f32, sample_rate: u32) -> Self {
        let a = 10f64.powf(gain_db as f64 / 40.0);
        let w0 = safe_w0(freq_hz, sample_rate);
        let cos = w0.cos();
        let two_sqrt_a_alpha = 2.0 * a.sqrt() * (w0.sin() / (2.0 * q as f64));
        let (b0, b1, b2, a0, a1, a2);
        if high {
            b0 = a * ((a + 1.0) + (a - 1.0) * cos + two_sqrt_a_alpha);
            b1 = -2.0 * a * ((a - 1.0) + (a + 1.0) * cos);
            b2 = a * ((a + 1.0) + (a - 1.0) * cos - two_sqrt_a_alpha);
            a0 = (a + 1.0) - (a - 1.0) * cos + two_sqrt_a_alpha;
            a1 = 2.0 * ((a - 1.0) - (a + 1.0) * cos);
            a2 = (a + 1.0) - (a - 1.0) * cos - two_sqrt_a_alpha;
        } else {
            b0 = a * ((a + 1.0) - (a - 1.0) * cos + two_sqrt_a_alpha);
            b1 = 2.0 * a * ((a - 1.0) - (a + 1.0) * cos);
            b2 = a * ((a + 1.0) - (a - 1.0) * cos - two_sqrt_a_alpha);
            a0 = (a + 1.0) + (a - 1.0) * cos + two_sqrt_a_alpha;
            a1 = -2.0 * ((a - 1.0) + (a + 1.0) * cos);
            a2 = (a + 1.0) + (a - 1.0) * cos - two_sqrt_a_alpha;
        }
        BiquadCoeffs {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
        }
    }

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

/// Apply a chain of EQ bands (peak or shelf) to an interleaved f32 buffer.
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
        .map(|b| BiquadCoeffs::for_band(b, sample_rate))
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
            kind: BandKind::Peak,
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
            kind: BandKind::Peak,
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
            kind: BandKind::Peak,
        }];
        apply_eq(&mut samples, 1, sr, &bands);

        let peak = samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(
            peak.is_finite() && peak < 4.0,
            "a band above Nyquist must stay bounded, got peak {peak}"
        );
    }

    // ---- shelves: RBJ Audio-EQ-Cookbook, checked against its formulas ----

    /// |H(e^{jw})| in dB of a biquad with a0 already normalised to 1.
    fn response_db(c: &BiquadCoeffs, freq_hz: f64, sr: f64) -> f64 {
        let w = 2.0 * std::f64::consts::PI * freq_hz / sr;
        let (c1, s1, c2, s2) = (w.cos(), w.sin(), (2.0 * w).cos(), (2.0 * w).sin());
        let (nr, ni) = (c.b0 + c.b1 * c1 + c.b2 * c2, -(c.b1 * s1 + c.b2 * s2));
        let (dr, di) = (1.0 + c.a1 * c1 + c.a2 * c2, -(c.a1 * s1 + c.a2 * s2));
        10.0 * ((nr * nr + ni * ni) / (dr * dr + di * di)).log10()
    }

    #[test]
    fn low_shelf_coefficients_match_the_cookbook() {
        // Hand-evaluated from the cookbook for fs=48000, f0=200, +6 dB,
        // Q=1/sqrt(2) (S=1): A=10^(6/40), alpha=sin(w0)/(2Q).
        let c = BiquadCoeffs::shelf(false, 200.0, 6.0, std::f32::consts::FRAC_1_SQRT_2, 48_000);
        let a = 10f64.powf(6.0 / 40.0);
        let w0 = 2.0 * std::f64::consts::PI * 200.0 / 48_000.0;
        let al = w0.sin() / (2.0 * std::f64::consts::FRAC_1_SQRT_2);
        let k = 2.0 * a.sqrt() * al;
        let a0 = (a + 1.0) + (a - 1.0) * w0.cos() + k;
        let want = [
            a * ((a + 1.0) - (a - 1.0) * w0.cos() + k) / a0,
            2.0 * a * ((a - 1.0) - (a + 1.0) * w0.cos()) / a0,
            a * ((a + 1.0) - (a - 1.0) * w0.cos() - k) / a0,
            -2.0 * ((a - 1.0) + (a + 1.0) * w0.cos()) / a0,
            ((a + 1.0) + (a - 1.0) * w0.cos() - k) / a0,
        ];
        let got = [c.b0, c.b1, c.b2, c.a1, c.a2];
        for (g, w) in got.iter().zip(want) {
            assert!((g - w).abs() < 1e-6, "{got:?} vs {want:?}");
        }
    }

    #[test]
    fn shelves_have_the_stated_gain_at_dc_nyquist_and_corner() {
        let sr = 48_000.0;
        for gain in [6.0f64, -9.0] {
            let low = BiquadCoeffs::shelf(false, 500.0, gain as f32, 0.707, 48_000);
            // DC: the full gain. Far above the corner: unity. Corner: half the gain.
            assert!(
                (response_db(&low, 1.0, sr) - gain).abs() < 0.05,
                "low DC {gain}"
            );
            assert!(
                response_db(&low, 23_999.0, sr).abs() < 0.05,
                "low Nyquist {gain}"
            );
            assert!(
                (response_db(&low, 500.0, sr) - gain / 2.0).abs() < 0.05,
                "low corner {gain}"
            );

            let high = BiquadCoeffs::shelf(true, 4_000.0, gain as f32, 0.707, 48_000);
            assert!(response_db(&high, 1.0, sr).abs() < 0.05, "high DC {gain}");
            assert!(
                (response_db(&high, 23_999.0, sr) - gain).abs() < 0.3,
                "high Nyquist {gain}"
            );
            assert!(
                (response_db(&high, 4_000.0, sr) - gain / 2.0).abs() < 0.05,
                "high corner {gain}"
            );
        }
    }

    #[test]
    fn zero_gain_shelf_is_a_passthrough() {
        let c = BiquadCoeffs::shelf(true, 3_000.0, 0.0, 1.0, 44_100);
        assert!((c.b0 - 1.0).abs() < 1e-12 && c.b1 == c.a1 && c.b2 == c.a2);
    }

    #[test]
    fn processed_tone_matches_the_shelf_gain() {
        let sr = 48_000u32;
        let tone = |hz: f32| -> Vec<f32> {
            (0..sr as usize * 2)
                .map(|i| (2.0 * std::f32::consts::PI * hz * i as f32 / sr as f32).sin() * 0.1)
                .collect()
        };
        let tail_rms = |v: &[f32]| rms(&v[v.len() / 2..]);
        let band = EqBand {
            freq_hz: 1_000.0,
            gain_db: 12.0,
            q: 0.707,
            kind: BandKind::LowShelf,
        };
        let mut low = tone(100.0);
        let before = tail_rms(&low);
        apply_eq(&mut low, 1, sr, &[band]);
        let db = 20.0 * (tail_rms(&low) / before).log10();
        assert!(
            (db - 12.0).abs() < 0.5,
            "100 Hz under a 12 dB low shelf: {db}"
        );
        let mut hi = tone(10_000.0);
        let before = tail_rms(&hi);
        apply_eq(&mut hi, 1, sr, &[band]);
        assert!(
            (tail_rms(&hi) / before - 1.0).abs() < 0.05,
            "10 kHz must pass a low shelf"
        );
    }
}
