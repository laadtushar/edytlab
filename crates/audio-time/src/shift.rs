//! Pitch-shifting primitive.
//!
//! Pitch shift in semitones; positive values transpose up, negative
//! down. `+12` semitones is one octave up (frequency ×2); `-12` is one
//! octave down (frequency ÷2).
//!
//! See the crate-level docs for the Phase-2 stub status.

use crate::vocoder::{deinterleave, interleave, resample_mono, stretch_mono};
use crate::{check_channels, Error, Result};

/// Maximum allowed |semitones|. Rubber Band degrades severely outside
/// roughly ±36; ±48 is a generous, round limit for argument validation.
pub const MAX_SEMITONES: f32 = 48.0;

/// Pitch-shift interleaved f32 samples by `semitones`.
///
/// Duration is preserved; only pitch moves. This is the composition of
/// the two primitives the vocoder provides: stretch the timeline by the
/// pitch ratio, then read the result back that many times faster. The
/// stretch is what makes it a pitch shift rather than the speed change
/// `change_speed` already offered — replaying faster alone would shorten
/// the audio too.
///
/// `preserve_formants` is accepted and currently ignored, so a large
/// shift on a voice will sound like the classic chipmunk or giant
/// rather than the same person singing higher. Fixing that needs
/// spectral-envelope estimation; the flag stays in the signature
/// because it is part of the tool schema.
///
/// # Errors
///
/// * [`Error::InvalidSemitones`] — `semitones` is non-finite or its
///   absolute value exceeds [`MAX_SEMITONES`].
/// * [`Error::ChannelMismatch`] — `channels` is zero, or `samples.len()`
///   does not divide evenly by `channels`.
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
        "audio-time::pitch_shift"
    );
    if samples.is_empty() {
        return Ok(Vec::new());
    }
    // Twelve semitones to the octave, and an octave doubles the frequency.
    let rate = 2f32.powf(semitones / 12.0);
    let frames = samples.len() / channels as usize;
    let planes: Vec<Vec<f32>> = deinterleave(samples, channels as usize)
        .iter()
        .map(|p| {
            // Stretch to `rate` times the length, then read back `rate`
            // times as fast: the length returns to where it started and
            // every frequency is multiplied by `rate`.
            let stretched = stretch_mono(p, rate);
            let mut shifted = resample_mono(&stretched, rate);
            // Round-off in the two length calculations can leave a frame
            // either side; the caller was promised the original duration.
            shifted.resize(frames, 0.0);
            shifted
        })
        .collect();
    Ok(interleave(&planes))
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

    fn sine(freq: f32, sr: u32, len: usize) -> Vec<f32> {
        (0..len)
            .map(|n| (2.0 * std::f32::consts::PI * freq * n as f32 / sr as f32).sin() * 0.5)
            .collect()
    }

    /// Zero-crossing frequency estimate over the steady middle, avoiding
    /// the overlap-add ramps at either end.
    fn est_freq(samples: &[f32], sr: u32) -> f32 {
        let s = &samples[samples.len() / 4..samples.len() * 3 / 4];
        let crossings = s
            .windows(2)
            .filter(|w| (w[0] <= 0.0 && w[1] > 0.0) || (w[0] >= 0.0 && w[1] < 0.0))
            .count();
        (crossings as f32 / 2.0) * sr as f32 / s.len() as f32
    }

    /// An octave up doubles the frequency and leaves the duration alone.
    /// That second half is the whole point — `change_speed` could already
    /// raise pitch, but only by shortening the audio.
    #[test]
    fn twelve_semitones_up_doubles_the_frequency() {
        let sr = 44_100;
        let input = sine(440.0, sr, sr as usize / 2);
        let out = pitch_shift(&input, sr, 1, 12.0, false).unwrap();

        assert_eq!(out.len(), input.len(), "duration must not change");
        let f = est_freq(&out, sr);
        assert!((f - 880.0).abs() < 40.0, "expected ~880 Hz, got {f}");
    }

    #[test]
    fn twelve_semitones_down_halves_the_frequency() {
        let sr = 44_100;
        let input = sine(440.0, sr, sr as usize / 2);
        let out = pitch_shift(&input, sr, 1, -12.0, false).unwrap();

        assert_eq!(out.len(), input.len());
        let f = est_freq(&out, sr);
        assert!((f - 220.0).abs() < 20.0, "expected ~220 Hz, got {f}");
    }

    #[test]
    fn zero_semitones_keeps_the_frequency() {
        let sr = 44_100;
        let input = sine(440.0, sr, sr as usize / 2);
        let out = pitch_shift(&input, sr, 1, 0.0, false).unwrap();

        assert_eq!(out.len(), input.len());
        let f = est_freq(&out, sr);
        assert!((f - 440.0).abs() < 20.0, "expected ~440 Hz, got {f}");
    }

    #[test]
    fn boundary_semitones_are_accepted() {
        let input = vec![0.0f32; 4096];
        assert!(pitch_shift(&input, 48_000, 1, MAX_SEMITONES, false).is_ok());
        assert!(pitch_shift(&input, 48_000, 1, -MAX_SEMITONES, false).is_ok());
    }

    #[test]
    fn stereo_stays_frame_aligned_and_the_same_length() {
        let sr = 44_100;
        let mono = sine(330.0, sr, sr as usize / 2);
        let stereo: Vec<f32> = mono.iter().flat_map(|s| [*s, *s]).collect();
        let out = pitch_shift(&stereo, sr, 2, 7.0, false).unwrap();
        assert_eq!(out.len(), stereo.len());
    }

    #[test]
    fn empty_input_is_empty_output_not_an_error() {
        assert_eq!(pitch_shift(&[], 48_000, 1, 5.0, false).unwrap(), Vec::new());
    }
}
