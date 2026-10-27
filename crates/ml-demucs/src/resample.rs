//! Sample-rate conversion in pieces, to and from the 44.1 kHz the network
//! takes ([`crate::separate::SAMPLE_RATE`]).
//!
//! `rubato`'s FFT resampler works on fixed blocks and delays what it is fed
//! by half a block, and it knows nothing of where the audio ends. A stem has
//! to line up with the track it came from to the sample and be exactly as long
//! as the audio it is a stem of, so this module does what ml-whisper's
//! one-shot 16 kHz converter does, but for a stream:
//!
//! * **No delay.** The first `output_delay()` frames are dropped, so frame 0
//!   of the output is frame 0 of the input.
//! * **The right length.** After [`RateConverter::finish`] the output is
//!   `round(frames * to / from)` frames: the block that is not full is padded
//!   with silence and flushed until the delayed tail has come out, then cut.
//! * **A bounded buffer.** Input of any size may be pushed in any pieces; what
//!   is held is less than one block, and the output of a push is only what
//!   that push completed. The result does not depend on how the input was
//!   split.

use rubato::{FftFixedInOut, Resampler};

use crate::{DemucsError, Result};

/// Input frames per block asked of the resampler. 1024 amortises the FFT
/// without making a block's delay (half of it) audible in the output's
/// alignment, which is corrected anyway.
const BLOCK_FRAMES: usize = 1024;

/// A converter from `from` Hz to `to` Hz for `channels` planar channels.
pub struct RateConverter {
    /// `None` when the rates are equal: frames pass through.
    resampler: Option<FftFixedInOut<f32>>,
    from: u32,
    to: u32,
    channels: usize,
    /// Input not yet a whole block, per channel.
    pending: Vec<Vec<f32>>,
    in_buf: Vec<Vec<f32>>,
    out_buf: Vec<Vec<f32>>,
    /// Output frames still to drop to take the resampler's delay off.
    skip: usize,
    frames_in: u64,
    frames_out: u64,
}

impl RateConverter {
    pub fn new(from: u32, to: u32, channels: usize) -> Result<Self> {
        if from == 0 || to == 0 || channels == 0 {
            return Err(DemucsError::Resample(format!(
                "cannot convert {channels} channels from {from} Hz to {to} Hz"
            )));
        }
        let resampler = if from == to {
            None
        } else {
            Some(
                FftFixedInOut::<f32>::new(from as usize, to as usize, BLOCK_FRAMES, channels)
                    .map_err(|e| DemucsError::Resample(e.to_string()))?,
            )
        };
        let (in_buf, out_buf, skip) = match &resampler {
            Some(r) => (
                vec![vec![0.0; r.input_frames_max()]; channels],
                vec![vec![0.0; r.output_frames_max()]; channels],
                r.output_delay(),
            ),
            None => (Vec::new(), Vec::new(), 0),
        };
        Ok(Self {
            resampler,
            from,
            to,
            channels,
            pending: vec![Vec::new(); channels],
            in_buf,
            out_buf,
            skip,
            frames_in: 0,
            frames_out: 0,
        })
    }

    /// Frames a conversion of `frames` input frames comes to once finished:
    /// `round(frames * to / from)`.
    pub fn output_frames(&self, frames: u64) -> u64 {
        output_frames(frames, self.from, self.to)
    }

    /// Feed `input` (one slice per channel, all the same length) and append
    /// what it completed to `out` (one `Vec` per channel).
    pub fn push(&mut self, input: &[&[f32]], out: &mut [Vec<f32>]) -> Result<()> {
        if input.len() != self.channels || out.len() != self.channels {
            return Err(DemucsError::Resample(format!(
                "{} channels given, {} expected",
                input.len(),
                self.channels
            )));
        }
        let frames = input[0].len();
        if input.iter().any(|c| c.len() != frames) {
            return Err(DemucsError::Resample("channels differ in length".into()));
        }
        self.frames_in += frames as u64;
        if self.resampler.is_none() {
            for (o, i) in out.iter_mut().zip(input) {
                o.extend_from_slice(i);
            }
            self.frames_out += frames as u64;
            return Ok(());
        }
        for (p, i) in self.pending.iter_mut().zip(input) {
            p.extend_from_slice(i);
        }
        self.drain(out, false)
    }

    /// Flush: pad the last block with silence, run it and as many more as it
    /// takes for the delayed tail to come out, and cut the output to its
    /// exact length. Nothing may be pushed afterwards.
    pub fn finish(&mut self, out: &mut [Vec<f32>]) -> Result<()> {
        if out.len() != self.channels {
            return Err(DemucsError::Resample("wrong number of channels".into()));
        }
        if self.resampler.is_none() {
            return Ok(());
        }
        self.drain(out, true)?;
        let wanted = output_frames(self.frames_in, self.from, self.to);
        // How far this call's output has gone past the end: cut it back.
        let excess = self.frames_out.saturating_sub(wanted) as usize;
        if excess > 0 {
            for o in out.iter_mut() {
                o.truncate(o.len().saturating_sub(excess));
            }
            self.frames_out = wanted;
        }
        Ok(())
    }

    /// Run whole blocks; with `flush`, then zero blocks until the output
    /// reaches the length the input is owed.
    fn drain(&mut self, out: &mut [Vec<f32>], flush: bool) -> Result<()> {
        let wanted = output_frames(self.frames_in, self.from, self.to);
        let resampler = self.resampler.as_mut().expect("checked by the callers");
        loop {
            let needed = resampler.input_frames_next();
            let have = self.pending[0].len();
            if have < needed {
                if !flush || self.frames_out >= wanted {
                    return Ok(());
                }
                // Silence past the end flushes the delay.
                for p in &mut self.pending {
                    p.resize(needed, 0.0);
                }
            }
            for (buf, p) in self.in_buf.iter_mut().zip(&mut self.pending) {
                buf.clear();
                buf.extend(p.drain(..needed));
            }
            let (_, written) = resampler
                .process_into_buffer(&self.in_buf, &mut self.out_buf, None)
                .map_err(|e| DemucsError::Resample(e.to_string()))?;
            let skipped = self.skip.min(written);
            self.skip -= skipped;
            for (o, buf) in out.iter_mut().zip(&self.out_buf) {
                o.extend_from_slice(&buf[skipped..written]);
            }
            self.frames_out += (written - skipped) as u64;
        }
    }
}

/// `round(frames * to / from)`.
pub fn output_frames(frames: u64, from: u32, to: u32) -> u64 {
    (frames * u64::from(to) + u64::from(from) / 2) / u64::from(from)
}

/// Convert whole planar channels in one go: [`RateConverter`] with a single
/// push and a finish. For tests, and for audio short enough not to matter.
pub fn convert(channels: &[Vec<f32>], from: u32, to: u32) -> Result<Vec<Vec<f32>>> {
    let mut converter = RateConverter::new(from, to, channels.len())?;
    let mut out = vec![Vec::new(); channels.len()];
    let input: Vec<&[f32]> = channels.iter().map(Vec::as_slice).collect();
    converter.push(&input, &mut out)?;
    converter.finish(&mut out)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 1 kHz burst from exactly 1.000 s to 1.100 s in `seconds` of
    /// silence, at `rate`.
    fn burst(rate: u32, seconds: u32) -> Vec<f32> {
        let n = (seconds * rate) as usize;
        let (start, len) = (rate as usize, rate as usize / 10);
        let mut v = vec![0.0f32; n];
        for (i, s) in v[start..start + len].iter_mut().enumerate() {
            *s = (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / rate as f32).sin() * 0.5;
        }
        v
    }

    fn extent(v: &[f32], rate: u32) -> (f32, f32) {
        let first = v.iter().position(|a| a.abs() > 0.1).expect("a burst");
        let last = v.iter().rposition(|a| a.abs() > 0.1).expect("a burst");
        (first as f32 / rate as f32, last as f32 / rate as f32)
    }

    #[test]
    fn equal_rates_pass_through_untouched() {
        let input = vec![vec![0.1f32, 0.2, 0.3], vec![-0.1, -0.2, -0.3]];
        assert_eq!(convert(&input, 44_100, 44_100).unwrap(), input);
    }

    #[test]
    fn time_and_length_are_kept_in_both_directions() {
        for (from, to) in [
            (48_000, 44_100),
            (44_100, 48_000),
            (22_050, 44_100),
            (44_100, 22_050),
            (96_000, 44_100),
            (8_000, 44_100),
            (44_100, 32_000),
        ] {
            let out = convert(&[burst(from, 3)], from, to).unwrap();
            assert_eq!(
                out[0].len() as u64,
                output_frames(3 * u64::from(from), from, to)
            );
            assert_eq!(
                out[0].len(),
                (3 * to) as usize,
                "{from} -> {to}: three seconds"
            );
            let (first, last) = extent(&out[0], to);
            assert!(
                (first - 1.0).abs() < 0.004 && (last - 1.1).abs() < 0.004,
                "{from} -> {to}: the burst is at {first:.4}-{last:.4} s, want 1.000-1.100"
            );
        }
    }

    #[test]
    fn the_length_is_the_rounded_ratio_for_awkward_lengths() {
        for (frames, from, to) in [
            (1usize, 44_100u32, 48_000u32),
            (44_107, 44_100, 16_000),
            (1023, 48_000, 44_100),
            (1025, 48_000, 44_100),
            (100_001, 22_050, 44_100),
            (0, 48_000, 44_100),
        ] {
            let out = convert(&[vec![0.25f32; frames]], from, to).unwrap();
            let want = (frames as u64 * u64::from(to) + u64::from(from) / 2) / u64::from(from);
            assert_eq!(out[0].len() as u64, want, "{frames} frames, {from} -> {to}");
        }
    }

    /// The pieces the input comes in change nothing: the blocks are the
    /// resampler's, not the caller's.
    #[test]
    fn the_result_does_not_depend_on_how_the_input_was_split() {
        let input = burst(48_000, 3);
        let whole = convert(std::slice::from_ref(&input), 48_000, 44_100).unwrap();
        for piece in [1usize, 7, 100, 1023, 1024, 1025, 5000, 70_000] {
            let mut converter = RateConverter::new(48_000, 44_100, 1).unwrap();
            let mut out = vec![Vec::new()];
            for chunk in input.chunks(piece) {
                converter.push(&[chunk], &mut out).unwrap();
                assert!(
                    converter.pending[0].len() < BLOCK_FRAMES * 2,
                    "piece {piece}"
                );
            }
            converter.finish(&mut out).unwrap();
            assert_eq!(out[0], whole[0], "pieces of {piece}");
        }
    }

    #[test]
    fn channels_are_converted_independently() {
        let left = burst(48_000, 2);
        let right: Vec<f32> = left.iter().map(|v| -v * 0.5).collect();
        let both = convert(&[left.clone(), right], 48_000, 44_100).unwrap();
        let alone = convert(&[left], 48_000, 44_100).unwrap();
        assert_eq!(both[0], alone[0]);
        for (l, r) in both[0].iter().zip(&both[1]) {
            assert!((r + l * 0.5).abs() < 1e-5);
        }
    }

    /// Down and back is the same audio to a low error for anything below
    /// both Nyquists.
    #[test]
    fn a_round_trip_comes_back() {
        let tone: Vec<f32> = (0..96_000)
            .map(|i| {
                let t = i as f32 / 48_000.0;
                0.4 * (2.0 * std::f32::consts::PI * 440.0 * t).sin()
                    + 0.2 * (2.0 * std::f32::consts::PI * 3_000.0 * t).sin()
            })
            .collect();
        let down = convert(std::slice::from_ref(&tone), 48_000, 44_100).unwrap();
        let back = convert(&down, 44_100, 48_000).unwrap();
        assert_eq!(back[0].len(), tone.len());
        let worst = (2_000..tone.len() - 2_000)
            .map(|i| (back[0][i] - tone[i]).abs())
            .fold(0.0f32, f32::max);
        assert!(worst < 5e-3, "worst error {worst}");
    }

    #[test]
    fn unusable_requests_are_errors() {
        assert!(RateConverter::new(0, 44_100, 2).is_err());
        assert!(RateConverter::new(44_100, 0, 2).is_err());
        assert!(RateConverter::new(44_100, 48_000, 0).is_err());
        let mut c = RateConverter::new(44_100, 48_000, 2).unwrap();
        let mut out = vec![Vec::new(); 2];
        assert!(
            c.push(&[&[0.0; 4]], &mut out).is_err(),
            "one channel for two"
        );
        assert!(
            c.push(&[&[0.0; 4], &[0.0; 3]], &mut out).is_err(),
            "uneven channels"
        );
    }
}
