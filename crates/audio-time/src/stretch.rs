//! Time-stretching primitive.
//!
//! Output duration equals `input_duration / factor`:
//!
//! * `factor = 0.5` → 2× slower (twice as long)
//! * `factor = 1.0` → identity
//! * `factor = 2.0` → 2× faster (half as long)
//!
//! See the crate-level docs for the Phase-2 stub status.

use crate::{check_channels, Error, Result};

/// Time-stretch interleaved f32 samples by `factor`.
///
/// `preserve_formants = true` requests vocal-formant preservation. With
/// the real Rubber Band backend (landing in M28) this maps to
/// `RubberBandPreserveFormants`; in the Phase 2 stub the flag is
/// validated but the function returns [`Error::NotImplemented`].
///
/// # Errors
///
/// * [`Error::InvalidFactor`] — `factor` is non-positive or non-finite.
/// * [`Error::ChannelMismatch`] — `channels` is zero, or `samples.len()`
///   does not divide evenly by `channels`.
/// * [`Error::NotImplemented`] — arguments are valid but the DSP
///   backend is not yet wired up.
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
    // Suppress unused-variable warnings until M28 fills in the body.
    // The trace is useful for telemetry once the backend is live.
    tracing::trace!(
        sample_rate,
        channels,
        factor,
        preserve_formants,
        "audio-time::time_stretch stub invoked"
    );
    let _ = sample_rate;
    Err(Error::NotImplemented)
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

    #[test]
    fn valid_args_return_not_implemented() {
        let r = time_stretch(&[0.0; 1024], 48_000, 1, 0.5, true);
        assert_eq!(r, Err(Error::NotImplemented));
    }
}
