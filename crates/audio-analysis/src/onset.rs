//! Onset-strength ("novelty") curves shared by tempo and beat tracking.
//!
//! One STFT pass makes two curves at the same hop:
//!
//! * `full`: log-compressed spectral flux over the whole spectrum (30 Hz to
//!   12 kHz). Every onset counts: kick, snare, hats, plucks.
//! * `low`: the same flux over 30–150 Hz only. In dance, pop and rock this is
//!   the kick drum (and the bass), which sits on the beat, where hats and
//!   shakers sit between the beats. Broadband noise has the most spectral
//!   flux of anything in a mix, so a tracker that follows `full` alone follows
//!   the hats (#696): a 90 BPM track with off-beat hats reads as 180.
//!
//! Both curves are log-compressed (a quiet onset counts as a loud one, up to
//! a point, as in the perceptual onset-strength curves of Böck & Widmer 2013,
//! "Maximum filter vibrato suppression for onset detection", and librosa's
//! `onset_strength`) and then have their slow-moving mean taken out and are
//! half-wave rectified, so a sustained loud passage does not read as a
//! pulse.

use realfft::num_complex::Complex;
use realfft::RealFftPlanner;

use crate::{Error, Result};

/// Window length in seconds: about 46 ms, 2048 samples at 44.1 kHz. Rounded to
/// a power of two for the sample rate.
const WINDOW_SECONDS: f32 = 0.0464;
/// The kick-and-bass band, Hz.
pub(crate) const LOW_BAND_HZ: (f32, f32) = (30.0, 150.0);
/// The top of the full band, Hz.
const FULL_BAND_TOP_HZ: f32 = 12_000.0;
/// Half-width of the window the slow mean is taken over, seconds.
const DETREND_HALF_WINDOW_S: f32 = 0.5;

/// Onset-strength curves of a mono signal, one value per hop.
pub(crate) struct Novelty {
    /// Whole-spectrum onset strength.
    pub full: Vec<f32>,
    /// 30–150 Hz onset strength: the kick.
    pub low: Vec<f32>,
    /// Seconds per value.
    pub hop_s: f32,
    /// Seconds from the start of the audio to the middle of frame 0's
    /// window, plus the measured lead of a flux peak over the true onset
    /// (see [`ONSET_LEAD_FRAMES`]). Add `index * hop_s` for a time.
    pub origin_s: f32,
    /// How strong a strong onset is over the whole spectrum, in the same
    /// log-flux units: tens for drums, a few for a plucked string, near zero
    /// for a pad or a drone, whatever its level.
    pub full_level: f32,
    /// How strong a strong onset is in the low band, in log-flux units (the
    /// 99th percentile of `low` before it is scaled): a kick is 10 or more,
    /// the low-frequency leakage of a hat or a click under 3, and a signal
    /// with nothing under 150 Hz close to zero.
    pub low_level: f32,
}

impl Novelty {
    /// Time in seconds of a (possibly fractional) frame index.
    pub fn time_of(&self, frame: f32) -> f32 {
        self.origin_s + frame * self.hop_s
    }
}

/// How far a log-flux peak sits ahead of the true onset, in hops. The flux at
/// frame `f` is the change from the window before to the one centred at
/// `f`, so it peaks as the attack enters the window, about one hop before the
/// attack is at the window's centre. Measured on synthetic kicks: with this at
/// 0 the beats came 11.8 ms (one hop) early; `tests/tempo.rs` pins the result.
const ONSET_LEAD_FRAMES: f32 = 1.0;

fn window_size(sample_rate: u32) -> usize {
    let target = (sample_rate as f32 * WINDOW_SECONDS).max(256.0);
    1usize << (target.log2().round() as u32)
}

/// Compute the curves. Errors if the audio is shorter than one window.
pub(crate) fn novelty(samples_mono: &[f32], sample_rate: u32) -> Result<Novelty> {
    if sample_rate == 0 {
        return Err(Error::Analysis("sample_rate is zero".into()));
    }
    let fft = window_size(sample_rate);
    let hop = fft / 4;
    if samples_mono.len() < fft {
        return Err(Error::Analysis("audio shorter than FFT window".into()));
    }

    let bin_hz = sample_rate as f32 / fft as f32;
    let nyquist_bin = fft / 2;
    let low_lo = ((LOW_BAND_HZ.0 / bin_hz).ceil() as usize).max(1);
    let low_hi = ((LOW_BAND_HZ.1 / bin_hz).floor() as usize).max(low_lo);
    let full_lo = low_lo;
    let full_hi = ((FULL_BAND_TOP_HZ / bin_hz).floor() as usize).min(nyquist_bin);

    let mut planner = RealFftPlanner::<f32>::new();
    let r2c = planner.plan_fft_forward(fft);
    let mut input = vec![0.0f32; fft];
    let mut output = vec![Complex::<f32>::new(0.0, 0.0); fft / 2 + 1];
    let window = hann(fft);

    let n_frames = (samples_mono.len() - fft) / hop + 1;
    let mut prev = vec![0.0f32; fft / 2 + 1];
    let mut full = Vec::with_capacity(n_frames);
    let mut low = Vec::with_capacity(n_frames);

    // Scale so a full-scale sine is about 0.25 before the log: a signal 80 dB
    // down still registers, and digital silence is exactly zero.
    let scale = 1.0e4 / fft as f32;

    for f in 0..n_frames {
        let start = f * hop;
        for i in 0..fft {
            input[i] = samples_mono[start + i] * window[i];
        }
        r2c.process(&mut input, &mut output)
            .map_err(|e| Error::Analysis(format!("FFT failed: {e}")))?;

        let mut full_flux = 0.0f32;
        let mut low_flux = 0.0f32;
        for (k, c) in output.iter().enumerate() {
            let mag = (c.re * c.re + c.im * c.im).sqrt();
            let l = (1.0 + scale * mag).ln();
            if k >= full_lo && k <= full_hi {
                let d = l - prev[k];
                if d > 0.0 {
                    full_flux += d;
                    if k <= low_hi {
                        low_flux += d;
                    }
                }
            }
            prev[k] = l;
        }
        full.push(full_flux);
        low.push(low_flux);
    }

    let hop_s = hop as f32 / sample_rate as f32;
    let full = detrend(&full, hop_s);
    let low = detrend(&low, hop_s);
    let low_level = percentile(&low, 0.99);
    let full_level = percentile(&full, 0.99);
    Ok(Novelty {
        full,
        low,
        full_level,
        hop_s,
        origin_s: (fft as f32 / 2.0) / sample_rate as f32 + ONSET_LEAD_FRAMES * hop_s,
        low_level,
    })
}

/// The `q` quantile of the non-zero values of `x`, zero if there are none.
pub(crate) fn percentile(x: &[f32], q: f32) -> f32 {
    let mut v: Vec<f32> = x.iter().copied().filter(|a| *a > 0.0).collect();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    v.get(((v.len() as f32) * q) as usize)
        .or(v.last())
        .copied()
        .unwrap_or(0.0)
}

/// Take a moving mean (±`DETREND_HALF_WINDOW_S`) out of `x` and clamp at zero.
fn detrend(x: &[f32], hop_s: f32) -> Vec<f32> {
    let half = ((DETREND_HALF_WINDOW_S / hop_s).round() as usize).max(1);
    let mut prefix = Vec::with_capacity(x.len() + 1);
    prefix.push(0.0f64);
    for &v in x {
        prefix.push(prefix.last().copied().unwrap_or(0.0) + v as f64);
    }
    (0..x.len())
        .map(|i| {
            let lo = i.saturating_sub(half);
            let hi = (i + half + 1).min(x.len());
            let mean = (prefix[hi] - prefix[lo]) / (hi - lo) as f64;
            (x[i] as f64 - mean).max(0.0) as f32
        })
        .collect()
}

fn hann(n: usize) -> Vec<f32> {
    (0..n)
        .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / (n - 1) as f32).cos())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_is_2048_at_44k_and_scales_with_the_rate() {
        assert_eq!(window_size(44_100), 2048);
        assert_eq!(window_size(48_000), 2048);
        assert_eq!(window_size(22_050), 1024);
        assert_eq!(window_size(96_000), 4096);
    }

    #[test]
    fn silence_has_no_novelty() {
        let n = novelty(&vec![0.0; 44_100], 44_100).unwrap();
        assert!(n.full.iter().all(|v| *v == 0.0));
        assert_eq!(n.low_level, 0.0);
    }

    #[test]
    fn too_short_errors() {
        assert!(novelty(&[0.0; 100], 44_100).is_err());
        assert!(novelty(&[0.0; 44_100], 0).is_err());
    }
}
