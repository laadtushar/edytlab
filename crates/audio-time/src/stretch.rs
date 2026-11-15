//! Time-stretching primitive.
//!
//! Output duration equals `input_duration / factor`:
//!
//! * `factor = 0.5` → 2× slower (twice as long)
//! * `factor = 1.0` → identity
//! * `factor = 2.0` → 2× faster (half as long)
//!
//! See the crate-level docs for the Phase-2 stub status.

use crate::vocoder::{deinterleave, interleave, stretch_mono, stretch_varying};
use crate::{check_channels, Error, Result};

/// Time-stretch interleaved f32 samples by `factor`.
///
/// Output length is `input_len / factor`, exactly — the tool contract
/// promises a duration, so the frame arithmetic is trimmed to match
/// rather than left a few percent short.
///
/// `preserve_formants` is accepted and currently ignored. Formant
/// preservation needs spectral-envelope estimation the phase vocoder
/// does not do; the argument stays in the signature because it is part
/// of the tool schema, and honouring it is an improvement to this
/// function rather than a change to its callers.
///
/// Each channel is processed independently, which is standard and is
/// why a hard-panned stereo source can drift slightly in image at large
/// factors.
///
/// # Errors
///
/// * [`Error::InvalidFactor`] — `factor` is non-positive or non-finite.
/// * [`Error::ChannelMismatch`] — `channels` is zero, or `samples.len()`
///   does not divide evenly by `channels`.
pub fn time_stretch(
    samples: &[f32],
    sample_rate: u32,
    channels: u16,
    factor: f32,
    preserve_formants: bool,
) -> Result<Vec<f32>> {
    if !factor.is_finite() || factor <= 0.0 {
        return Err(Error::InvalidFactor(factor));
    }
    check_channels(samples.len(), channels)?;
    tracing::trace!(
        sample_rate,
        channels,
        factor,
        preserve_formants,
        "audio-time::time_stretch"
    );
    if samples.is_empty() {
        return Ok(Vec::new());
    }
    // `factor` is a speed, the vocoder takes a length ratio, and they are
    // reciprocal: factor 2.0 means twice as fast, which is half as long.
    let ratio = 1.0 / factor;
    let planes: Vec<Vec<f32>> = deinterleave(samples, channels as usize)
        .iter()
        .map(|p| stretch_mono(p, ratio))
        .collect();
    Ok(interleave(&planes))
}

/// Smallest start or end factor a ramped stretch accepts (ten times slower).
pub const MIN_RAMP_FACTOR: f32 = 0.1;
/// Largest start or end factor a ramped stretch accepts (ten times faster).
pub const MAX_RAMP_FACTOR: f32 = 10.0;

/// Where input position `input_pos` lands in the output of a ramped
/// stretch of an input `input_len` long, in the same unit as both.
///
/// The factor (a speed) moves linearly with *input* position from
/// `start_factor` to `end_factor`, so the local length ratio is
/// `1 / factor(p)` and the output position is its integral:
/// `len * ln(f(p) / f0) / (f1 - f0)`. With equal factors that is
/// `p / f0`, which the closed form cannot say (it divides by zero), so
/// a difference too small to resolve takes the limit.
pub fn ramp_output_position(
    input_pos: f64,
    input_len: f64,
    start_factor: f64,
    end_factor: f64,
) -> f64 {
    let p = input_pos.clamp(0.0, input_len);
    let delta = end_factor - start_factor;
    if (delta / start_factor).abs() < 1e-9 {
        return p / start_factor;
    }
    let here = start_factor + delta * p / input_len;
    input_len * (here / start_factor).ln() / delta
}

/// Time-stretch interleaved f32 samples with a factor that ramps
/// linearly, along the input, from `start_factor` to `end_factor`.
///
/// Both are speeds as in [`time_stretch`] (0.5 twice as slow, 2.0 twice
/// as fast). A tape-stop is `1.0` to `0.1`; a gradual slow-down at the
/// end of a track is `1.0` to `0.5`. Pitch is unchanged. The output is
/// [`ramp_output_position`] of the whole input long, to the frame.
///
/// One vocoder pass runs the whole ramp (see [`crate::warp`]), so there
/// is no seam where the ratio changes. Equal factors are an ordinary
/// [`time_stretch`] and give the same samples.
///
/// # Errors
///
/// * [`Error::InvalidRampFactor`] — either factor is non-finite or
///   outside [`MIN_RAMP_FACTOR`]..=[`MAX_RAMP_FACTOR`].
/// * [`Error::ChannelMismatch`] — as for [`time_stretch`].
pub fn time_stretch_ramp(
    samples: &[f32],
    sample_rate: u32,
    channels: u16,
    start_factor: f32,
    end_factor: f32,
    preserve_formants: bool,
) -> Result<Vec<f32>> {
    for f in [start_factor, end_factor] {
        if !(MIN_RAMP_FACTOR..=MAX_RAMP_FACTOR).contains(&f) {
            return Err(Error::InvalidRampFactor(f));
        }
    }
    check_channels(samples.len(), channels)?;
    if start_factor == end_factor {
        return time_stretch(
            samples,
            sample_rate,
            channels,
            start_factor,
            preserve_formants,
        );
    }
    if samples.is_empty() {
        return Ok(Vec::new());
    }
    let frames = samples.len() / channels as usize;
    let (f0, f1) = (start_factor as f64, end_factor as f64);
    let target_len =
        (ramp_output_position(frames as f64, frames as f64, f0, f1).round() as usize).max(1);
    let ratio_at = |pos: usize| {
        let f = f0 + (f1 - f0) * (pos.min(frames) as f64 / frames as f64);
        (1.0 / f) as f32
    };
    let planes: Vec<Vec<f32>> = deinterleave(samples, channels as usize)
        .iter()
        .map(|p| stretch_varying(p, &ratio_at, target_len, true))
        .collect();
    Ok(interleave(&planes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_zero_factor() {
        let r = time_stretch(&[0.0; 4], 48_000, 1, 0.0, false);
        assert!(matches!(r, Err(Error::InvalidFactor(_))));
    }

    #[test]
    fn rejects_negative_factor() {
        let r = time_stretch(&[0.0; 4], 48_000, 1, -1.0, false);
        assert!(matches!(r, Err(Error::InvalidFactor(_))));
    }

    #[test]
    fn rejects_nan_factor() {
        let r = time_stretch(&[0.0; 4], 48_000, 1, f32::NAN, false);
        assert!(matches!(r, Err(Error::InvalidFactor(_))));
    }

    #[test]
    fn rejects_infinite_factor() {
        let r = time_stretch(&[0.0; 4], 48_000, 1, f32::INFINITY, false);
        assert!(matches!(r, Err(Error::InvalidFactor(_))));
    }

    #[test]
    fn ramp_position_is_the_integral_of_the_ratio() {
        // Constant factor: position / factor, including the limit case.
        assert!((ramp_output_position(50.0, 100.0, 2.0, 2.0) - 25.0).abs() < 1e-12);
        assert!((ramp_output_position(50.0, 100.0, 2.0, 2.0 + 1e-12) - 25.0).abs() < 1e-6);
        // 1 -> 0.5 over 100: 100 * ln(0.5) / -0.5.
        let total = ramp_output_position(100.0, 100.0, 1.0, 0.5);
        assert!((total - 100.0 * 2.0 * 2f64.ln()).abs() < 1e-9, "{total}");
        // Positions outside the input are held at its ends.
        assert_eq!(ramp_output_position(-5.0, 100.0, 1.0, 0.5), 0.0);
        assert_eq!(ramp_output_position(500.0, 100.0, 1.0, 0.5), total);
    }

    #[test]
    fn rejects_zero_channels() {
        let r = time_stretch(&[0.0; 4], 48_000, 0, 1.0, false);
        assert!(matches!(r, Err(Error::ChannelMismatch(_))));
    }

    #[test]
    fn rejects_uneven_channel_split() {
        let r = time_stretch(&[0.0; 5], 48_000, 2, 1.0, false);
        assert!(matches!(r, Err(Error::ChannelMismatch(_))));
    }

    /// `factor` is a speed: 2.0 is twice as fast, so half as long.
    #[test]
    fn factor_two_halves_the_duration() {
        let input: Vec<f32> = (0..48_000)
            .map(|n| (2.0 * std::f32::consts::PI * 440.0 * n as f32 / 48_000.0).sin() * 0.5)
            .collect();
        let out = time_stretch(&input, 48_000, 1, 2.0, false).unwrap();
        assert_eq!(out.len(), 24_000);
    }

    #[test]
    fn factor_half_doubles_the_duration() {
        let input: Vec<f32> = (0..48_000)
            .map(|n| (2.0 * std::f32::consts::PI * 440.0 * n as f32 / 48_000.0).sin() * 0.5)
            .collect();
        let out = time_stretch(&input, 48_000, 1, 0.5, false).unwrap();
        assert_eq!(out.len(), 96_000);
    }

    /// Stereo has to come back interleaved, in frames.
    #[test]
    fn stereo_keeps_its_channel_count() {
        let input: Vec<f32> = (0..96_000)
            .map(|n| (2.0 * std::f32::consts::PI * 220.0 * n as f32 / 48_000.0).sin() * 0.4)
            .collect();
        let out = time_stretch(&input, 48_000, 2, 2.0, false).unwrap();
        assert_eq!(out.len() % 2, 0, "output must stay frame-aligned");
        assert_eq!(out.len(), 48_000);
    }

    #[test]
    fn empty_input_is_empty_output_not_an_error() {
        assert_eq!(
            time_stretch(&[], 48_000, 1, 2.0, false).unwrap(),
            Vec::new()
        );
    }
}
