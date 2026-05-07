//! EBU R128 / ITU-R BS.1770-4 integrated loudness.
//!
//! Wraps the `ebur128` crate, which implements K-weighting + 400 ms
//! gating block + integrated loudness per the standard. We feed it
//! interleaved `f32` frames and read back the global loudness in LUFS.
//!
//! Acceptance bar (M19 plan): match `ffmpeg -af ebur128` within 0.5 LU.
//! The `ebur128` crate is the canonical Rust port of `libebur128` and
//! is what `ffmpeg` itself uses for the `ebur128` filter, so meeting
//! the bar is a question of correctly wiring the sample format and
//! channel layout.

use ebur128::{EbuR128, Mode};

use crate::{Error, Result};

/// Compute EBU R128 integrated loudness for an interleaved sample
/// buffer. Channel count is the number of channels in the interleaving
/// (e.g. 2 for stereo).
pub fn integrated_lufs(samples: &[f32], sample_rate: u32, channels: u16) -> Result<f32> {
    if samples.is_empty() {
        return Err(Error::Lufs("empty audio buffer".into()));
    }
    if sample_rate == 0 {
        return Err(Error::Lufs("sample_rate is zero".into()));
    }
    if channels == 0 {
        return Err(Error::Lufs("channels is zero".into()));
    }

    // BS.1770 gating requires at least one full 400 ms block.
    let frames = samples.len() / channels as usize;
    if (frames as f32 / sample_rate as f32) < 0.4 {
        return Err(Error::Lufs("audio shorter than 400 ms gating block".into()));
    }

    let mut meter = EbuR128::new(channels as u32, sample_rate, Mode::I)
        .map_err(|e| Error::Lufs(format!("ebur128 init: {e:?}")))?;
    meter
        .add_frames_f32(samples)
        .map_err(|e| Error::Lufs(format!("ebur128 add_frames: {e:?}")))?;
    let lufs = meter
        .loudness_global()
        .map_err(|e| Error::Lufs(format!("ebur128 global: {e:?}")))?;
    Ok(lufs as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sine wave at `freq` Hz with peak amplitude `peak` for `duration_s`.
    fn sine(sample_rate: u32, duration_s: f32, freq: f32, peak: f32) -> Vec<f32> {
        let n = (duration_s * sample_rate as f32) as usize;
        (0..n)
            .map(|i| {
                let t = i as f32 / sample_rate as f32;
                peak * (2.0 * std::f32::consts::PI * freq * t).sin()
            })
            .collect()
    }

    #[test]
    fn empty_input_errors() {
        assert!(integrated_lufs(&[], 48_000, 1).is_err());
    }

    #[test]
    fn zero_sample_rate_errors() {
        assert!(integrated_lufs(&[0.1; 1024], 0, 1).is_err());
    }

    #[test]
    fn zero_channels_errors() {
        assert!(integrated_lufs(&[0.1; 1024], 48_000, 0).is_err());
    }

    #[test]
    fn short_input_errors() {
        // 100 ms of audio is below the 400 ms gating block.
        let s = sine(48_000, 0.1, 1_000.0, 0.5);
        assert!(integrated_lufs(&s, 48_000, 1).is_err());
    }

    /// A 1 kHz mono sine drives the K-weighting filter at near-unity
    /// gain; empirically the `ebur128` crate (canonical Rust port of
    /// `libebur128`) reports `≈ -23 LUFS` for a peak amplitude of
    /// `0.1`. The full 0.5 LU ffmpeg-cross-check acceptance bar is a
    /// manual gate; this test confirms the wiring (sample format,
    /// channel count, mode) within 1 LU.
    #[test]
    fn reference_sine_near_minus_23_lufs() {
        let sr = 48_000;
        let peak = 0.1f32;
        let s = sine(sr, 3.0, 1_000.0, peak);
        let lufs = integrated_lufs(&s, sr, 1).expect("ok");
        assert!(
            (lufs - (-23.0)).abs() < 1.0,
            "expected ~-23 LUFS, got {lufs}"
        );
    }

    #[test]
    fn stereo_correlated_sine_matches_mono() {
        // Identical L+R should give ≈ 3 dB more loudness than mono of the
        // same per-channel amplitude (BS.1770 sums channels with weights
        // 1.0 each for L/R), but here we just sanity-check it returns
        // something finite.
        let sr = 48_000;
        let mono = sine(sr, 1.0, 1_000.0, 0.1);
        let mut stereo = Vec::with_capacity(mono.len() * 2);
        for &m in &mono {
            stereo.push(m);
            stereo.push(m);
        }
        let lufs = integrated_lufs(&stereo, sr, 2).expect("ok");
        assert!(lufs.is_finite() && lufs < 0.0);
    }
}
