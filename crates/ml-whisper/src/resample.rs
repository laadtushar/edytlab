//! Any sample rate and channel count to the 16 kHz mono Whisper hears.
//!
//! The rate conversion is `rubato`'s FFT resampler, which the workspace
//! already links (`audio-time` and `audio-engine` have no sample-rate
//! converter of their own to reuse). What this module adds is the two things
//! a streaming resampler does not do by itself and a transcript depends on:
//!
//! * **No delay.** A block-FFT resampler outputs what it has been fed with a
//!   latency of half an output chunk (`output_delay()`: 15 ms from 44.1 kHz,
//!   64 ms from 8 kHz). Word times are measured from the start of the buffer
//!   this returns, so that latency would be a bias on every word. The first
//!   `output_delay()` frames are dropped.
//! * **The right length.** The last block is zero-padded and flushed until the
//!   delayed tail has come out, then the result is cut to
//!   `round(frames * 16000 / rate)` samples: no silence added past the end of
//!   the audio, none missing from it.
//!
//! A buffer already at 16 kHz is only downmixed. Several channels are
//! averaged, which is what Whisper's own `ffmpeg -ac 1` does.

use rubato::{FftFixedInOut, Resampler};

use crate::{Result, WhisperError};

/// The rate Whisper was trained at.
const TARGET_RATE: u32 = 16_000;

/// Resample interleaved `f32` PCM at `sr` Hz with `channels` channels to
/// 16 kHz mono `f32`.
///
/// Strategy: downmix-then-resample. Whisper expects mono, and downmixing
/// before the resampler halves the FFT cost on stereo inputs.
///
/// If the input is already 16 kHz the resampler is bypassed entirely (the
/// input is downmixed and returned as-is). Otherwise the output is aligned
/// with the input, sample 0 to sample 0, and is `round(frames * 16000 / sr)`
/// long; see the module docs.
pub fn resample_to_16khz_mono(samples: &[f32], sr: u32, channels: u16) -> Result<Vec<f32>> {
    if channels == 0 {
        return Err(WhisperError::InvalidAudio("0 channels".into()));
    }
    if sr == 0 {
        return Err(WhisperError::InvalidAudio("0 Hz sample rate".into()));
    }
    let chans = channels as usize;
    if !samples.len().is_multiple_of(chans) {
        return Err(WhisperError::InvalidAudio(format!(
            "interleaved buffer length {} not divisible by channels {chans}",
            samples.len()
        )));
    }

    // Downmix to mono first.
    let frames = samples.len() / chans;
    let mut mono = Vec::with_capacity(frames);
    if chans == 1 {
        mono.extend_from_slice(samples);
    } else {
        let inv = 1.0 / chans as f32;
        for f in 0..frames {
            let mut acc = 0.0f32;
            for ch in 0..chans {
                acc += samples[f * chans + ch];
            }
            mono.push(acc * inv);
        }
    }

    if sr == TARGET_RATE || mono.is_empty() {
        return Ok(mono);
    }
    resample_mono(&mono, sr)
}

fn resample_mono(mono: &[f32], sr: u32) -> Result<Vec<f32>> {
    let wanted =
        ((mono.len() as u64 * u64::from(TARGET_RATE) + u64::from(sr) / 2) / u64::from(sr)) as usize;

    // 1024-frame chunk — small enough for tens-of-ms latency, large enough
    // to amortize FFT cost. We're running off the audio thread here so the
    // chunk size is throughput-bound, not latency-bound.
    let mut resampler = FftFixedInOut::<f32>::new(sr as usize, TARGET_RATE as usize, 1024, 1)?;
    let delay = resampler.output_delay();
    let mut in_buf = vec![vec![0.0f32; resampler.input_frames_max()]; 1];
    let mut out_buf = vec![vec![0.0f32; resampler.output_frames_max()]; 1];

    // The delayed tail is `delay` frames long, so the output has to reach
    // `delay + wanted` before it can be cut.
    let mut out: Vec<f32> = Vec::with_capacity(delay + wanted + out_buf[0].len());
    let mut pos = 0usize;
    while out.len() < delay + wanted {
        let needed = resampler.input_frames_next();
        // Past the end of the audio this is silence, which flushes the delay.
        let available = mono.len().saturating_sub(pos).min(needed);
        in_buf[0].clear();
        in_buf[0].extend_from_slice(&mono[pos..pos + available]);
        in_buf[0].resize(needed, 0.0);
        let (_in_used, out_written) = resampler.process_into_buffer(&in_buf, &mut out_buf, None)?;
        out.extend_from_slice(&out_buf[0][..out_written]);
        pos += available;
    }
    out.drain(..delay);
    out.truncate(wanted);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resample_passthrough_at_16khz() {
        // 16 kHz mono: should return the input unchanged.
        let input: Vec<f32> = (0..1600).map(|i| (i as f32) * 0.001).collect();
        let out = resample_to_16khz_mono(&input, 16_000, 1).unwrap();
        assert_eq!(out, input);
    }

    #[test]
    fn resample_stereo_downmix() {
        // 16 kHz stereo with L=1.0, R=-1.0 -> mono should be ~0.0 everywhere.
        let frames = 1024;
        let mut interleaved = Vec::with_capacity(frames * 2);
        for _ in 0..frames {
            interleaved.push(1.0);
            interleaved.push(-1.0);
        }
        let out = resample_to_16khz_mono(&interleaved, 16_000, 2).unwrap();
        assert_eq!(out.len(), frames);
        for s in out {
            assert!(s.abs() < 1e-6, "downmix not zero: {s}");
        }
    }

    /// A 1 kHz tone burst from exactly 1.000 s to 1.100 s inside 3 s of
    /// silence, at `sr`, as `channels` identical interleaved channels.
    fn burst(sr: u32, channels: usize) -> Vec<f32> {
        let n = (3 * sr) as usize;
        let (start, len) = (sr as usize, sr as usize / 10);
        let mut mono = vec![0.0f32; n];
        for (i, s) in mono[start..start + len].iter_mut().enumerate() {
            *s = (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / sr as f32).sin() * 0.5;
        }
        mono.iter()
            .flat_map(|&s| std::iter::repeat_n(s, channels))
            .collect()
    }

    /// First and last sample, in seconds, whose magnitude is over 0.1.
    fn extent(samples: &[f32]) -> (f32, f32) {
        let first = samples.iter().position(|v| v.abs() > 0.1).expect("a burst");
        let last = samples
            .iter()
            .rposition(|v| v.abs() > 0.1)
            .expect("a burst");
        (first as f32 / 16_000.0, last as f32 / 16_000.0)
    }

    #[test]
    fn resampling_keeps_time_and_length() {
        // The FFT resampler delays what it resamples by half a chunk (15 ms
        // at 44.1 kHz, 64 ms at 8 kHz) and its flush adds up to a chunk of
        // silence at the end. Word times are measured from the start of the
        // buffer, so a delay is a bias on every word, and a long tail is time
        // the audio does not have. Before this module did anything about it
        // the 44.1 kHz burst came out at 1.0151-1.1149 s and the 8 kHz
        // buffer was 49 152 samples long.
        for (sr, channels) in [
            (44_100, 1),
            (48_000, 2),
            (22_050, 1),
            (8_000, 1),
            (96_000, 2),
            (32_000, 1),
        ] {
            let out = resample_to_16khz_mono(&burst(sr, channels), sr, channels as u16).unwrap();
            assert_eq!(out.len(), 48_000, "{sr} Hz x {channels}: three seconds");
            let (first, last) = extent(&out);
            assert!(
                (first - 1.0).abs() < 0.004 && (last - 1.1).abs() < 0.004,
                "{sr} Hz x {channels}: the burst is at {first:.4}-{last:.4} s, want 1.000-1.100"
            );
        }
    }

    #[test]
    fn the_length_is_the_rounded_ratio_for_awkward_lengths() {
        // 1 s + 7 frames at 44.1 kHz is 16 002.5 samples: round half up.
        for (frames, sr) in [
            (44_107usize, 44_100u32),
            (1, 44_100),
            (1023, 48_000),
            (100_001, 22_050),
        ] {
            let input = vec![0.25f32; frames];
            let out = resample_to_16khz_mono(&input, sr, 1).unwrap();
            let want = ((frames as u64 * 16_000 + u64::from(sr) / 2) / u64::from(sr)) as usize;
            assert_eq!(out.len(), want, "{frames} frames at {sr} Hz");
        }
    }

    #[test]
    fn a_constant_stays_constant_away_from_the_edges() {
        // DC passes through unchanged (the resampler is not scaling).
        let input = vec![0.5f32; 44_100];
        let out = resample_to_16khz_mono(&input, 44_100, 1).unwrap();
        for (i, v) in out.iter().enumerate().skip(400).take(out.len() - 800) {
            assert!((v - 0.5).abs() < 0.01, "sample {i} is {v}");
        }
    }

    #[test]
    fn empty_input_is_empty_output() {
        let out = resample_to_16khz_mono(&[], 44_100, 2).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn rejects_zero_channels() {
        let err = resample_to_16khz_mono(&[], 16_000, 0).unwrap_err();
        assert!(matches!(err, WhisperError::InvalidAudio(_)));
    }

    #[test]
    fn rejects_zero_rate() {
        let err = resample_to_16khz_mono(&[0.0], 0, 1).unwrap_err();
        assert!(matches!(err, WhisperError::InvalidAudio(_)));
    }

    #[test]
    fn rejects_misaligned_buffer() {
        // 3 samples for stereo is not a whole number of frames.
        let err = resample_to_16khz_mono(&[1.0, 2.0, 3.0], 16_000, 2).unwrap_err();
        assert!(matches!(err, WhisperError::InvalidAudio(_)));
    }
}
