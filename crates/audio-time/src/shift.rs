//! Pitch-shifting primitive.
//!
//! Pitch shift in semitones; positive values transpose up, negative
//! down. `+12` semitones is one octave up (frequency ×2); `-12` is one
//! octave down (frequency ÷2).
//!
//! See the crate-level docs for the Phase-2 stub status.

use crate::{check_channels, Error, Result};

/// Maximum allowed |semitones|. Rubber Band degrades severely outside
/// roughly ±36; ±48 is a generous, round limit for argument validation.
pub const MAX_SEMITONES: f32 = 48.0;

/// Pitch-shift interleaved f32 samples by `semitones`.
///
/// `preserve_formants = true` requests vocal-formant preservation. With
/// the real Rubber Band backend (landing in M28) this maps to
/// `RubberBandPreserveFormants`; in the Phase 2 stub the flag is
/// validated but the function returns [`Error::NotImplemented`].
///
/// # Errors
///
/// * [`Error::InvalidSemitones`] — `semitones` is non-finite or its
///   absolute value exceeds [`MAX_SEMITONES`].
/// * [`Error::ChannelMismatch`] — `channels` is zero, or `samples.len()`
///   does not divide evenly by `channels`.
/// * [`Error::NotImplemented`] — arguments are valid but the DSP
///   backend is not yet wired up.
pub fn pitch_shift(
    samples: &[f32],
    sample_rate: u32,
    channels: u16,
    semitones: f32,
    preserve_formants: bool,
) -> Result<Vec<f32>> {
    if !semitones.is_finite() || semitones.abs() > MAX_SEMITONES {
        return Err(Error::InvalidSemitones(semitones));
    }
    check_channels(samples.len(), channels)?;
    tracing::trace!(
        sample_rate,
        channels,
        semitones,
        preserve_formants,
        "audio-time::pitch_shift stub invoked"
    );
    let _ = sample_rate;
    Err(Error::NotImplemented)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_nan_semitones() {
        let r = pitch_shift(&[0.0; 4], 48_000, 1, f32::NAN, false);
        assert!(matches!(r, Err(Error::InvalidSemitones(_))));
    }

    #[test]
    fn rejects_out_of_range_semitones() {
        let r = pitch_shift(&[0.0; 4], 48_000, 1, 60.0, false);
        assert!(matches!(r, Err(Error::InvalidSemitones(_))));
        let r = pitch_shift(&[0.0; 4], 48_000, 1, -60.0, false);
        assert!(matches!(r, Err(Error::InvalidSemitones(_))));
    }

    #[test]
    fn rejects_zero_channels() {
        let r = pitch_shift(&[0.0; 4], 48_000, 0, 0.0, false);
        assert!(matches!(r, Err(Error::ChannelMismatch(_))));
    }

    #[test]
    fn valid_args_return_not_implemented() {
        let r = pitch_shift(&[0.0; 1024], 48_000, 1, 12.0, true);
        assert_eq!(r, Err(Error::NotImplemented));
    }

    #[test]
    fn boundary_semitones_accepted() {
        let r = pitch_shift(&[0.0; 1024], 48_000, 1, MAX_SEMITONES, false);
        assert_eq!(r, Err(Error::NotImplemented));
        let r = pitch_shift(&[0.0; 1024], 48_000, 1, -MAX_SEMITONES, false);
        assert_eq!(r, Err(Error::NotImplemented));
    }
}
