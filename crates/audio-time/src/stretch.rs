//! Time-stretching primitive.
//!
//! Output duration equals `input_duration / factor`:
//!
//! * `factor = 0.5` → 2× slower (twice as long)
//! * `factor = 1.0` → identity
//! * `factor = 2.0` → 2× faster (half as long)
//!
//! See the crate-level docs for the Phase-2 stub status.

use crate::vocoder::{deinterleave, interleave, stretch_mono};
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
