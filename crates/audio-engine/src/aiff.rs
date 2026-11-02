//! AIFF export: 16-bit big-endian PCM (Apple's Audio Interchange File
//! Format, version 1, not AIFF-C).
//!
//! In-house because the format is three chunks and the container is
//! the only part that is not already in the tree. The samples go
//! through [`crate::encode::quantise`], so an AIFF and a WAV of one
//! render hold the same 16-bit numbers and decode to the same floats.
//!
//! Layout (Apple, "Audio Interchange File Format: AIFF", 1989):
//! `FORM <size> AIFF`, a `COMM` chunk (channels, frames, bits, rate as
//! an 80-bit IEEE extended float), then `SSND` (offset 0, block size 0,
//! the interleaved samples). Every chunk size is a big-endian `u32`, so
//! a file past 4 GiB is refused rather than written with a wrapped size.

use std::io::Write;
use std::path::Path;

use crate::encode::quantise;
use crate::{Error, Result};

/// `FORM` size field for a file of `data` bytes of samples: `AIFF` (4),
/// `COMM` header + body (8 + 18), `SSND` header + preamble (8 + 8).
const FORM_OVERHEAD: u64 = 4 + 26 + 16;

/// A sample rate as the 10 bytes AIFF stores it in: sign and 15-bit
/// exponent, then a 64-bit mantissa with an explicit leading one. Every
/// `u32` rate is an integer, so the mantissa is the rate shifted up
/// until its top bit is set.
fn extended(rate: u32) -> [u8; 10] {
    let mut out = [0u8; 10];
    if rate == 0 {
        return out;
    }
    let top = 31 - rate.leading_zeros(); // floor(log2(rate))
    let exponent = 16_383 + top as u16;
    let mantissa = u64::from(rate) << (63 - top);
    out[..2].copy_from_slice(&exponent.to_be_bytes());
    out[2..].copy_from_slice(&mantissa.to_be_bytes());
    out
}

/// Write interleaved `samples` to a 16-bit AIFF at `out`.
///
/// `samples.len()` must be a multiple of `channels`; a trailing partial
/// frame is dropped, as the frame count in `COMM` has to match `SSND`.
pub fn write_aiff(samples: &[f32], sample_rate: u32, channels: u16, out: &Path) -> Result<()> {
    if channels == 0 {
        return Err(Error::UnsupportedChannelMap { from: 0, to: 2 });
    }
    if sample_rate == 0 {
        return Err(Error::Encode("aiff: sample rate is 0".into()));
    }
    let frames = samples.len() / channels as usize;
    let data_bytes = frames as u64 * u64::from(channels) * 2;
    if data_bytes + FORM_OVERHEAD > u64::from(u32::MAX) {
        return Err(Error::Encode(
            "aiff: the audio is past the format's 4 GiB limit; export as flac or wav".into(),
        ));
    }

    let file = std::fs::File::create(out)?;
    let mut w = std::io::BufWriter::new(file);
    w.write_all(b"FORM")?;
    w.write_all(&((data_bytes + FORM_OVERHEAD) as u32).to_be_bytes())?;
    w.write_all(b"AIFF")?;

    w.write_all(b"COMM")?;
    w.write_all(&18u32.to_be_bytes())?;
    w.write_all(&channels.to_be_bytes())?;
    w.write_all(&(frames as u32).to_be_bytes())?;
    w.write_all(&16u16.to_be_bytes())?;
    w.write_all(&extended(sample_rate))?;

    w.write_all(b"SSND")?;
    w.write_all(&((data_bytes + 8) as u32).to_be_bytes())?;
    w.write_all(&0u32.to_be_bytes())?; // offset
    w.write_all(&0u32.to_be_bytes())?; // block size
    for &s in &samples[..frames * channels as usize] {
        w.write_all(&quantise(s).to_be_bytes())?;
    }
    w.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extended_matches_known_encodings() {
        // 44,100 Hz is the textbook example: 400E AC44 0000 0000 0000.
        assert_eq!(extended(44_100), [0x40, 0x0E, 0xAC, 0x44, 0, 0, 0, 0, 0, 0]);
        // 48,000 Hz: 400E BB80 ...
        assert_eq!(extended(48_000), [0x40, 0x0E, 0xBB, 0x80, 0, 0, 0, 0, 0, 0]);
        // 1 Hz: exponent 16383, mantissa 1 << 63.
        assert_eq!(extended(1), [0x3F, 0xFF, 0x80, 0, 0, 0, 0, 0, 0, 0]);
    }
}
