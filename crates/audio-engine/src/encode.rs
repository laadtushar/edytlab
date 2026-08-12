//! WAV encoding helper for destructive edits.
//!
//! Tools that mutate sample data (fade, reverse, insert_silence, ...) write
//! the result to a fresh CAS-addressed WAV via [`write_wav`]. The format
//! mirrors the quantization style already used by the streaming render path
//! in [`crate::render::render_streaming`]: interleaved 16-bit PCM, rounded
//! and clamped to `i16` range.
//!
//! Quantization MUST match the render path so a destructive edit that
//! happens to be a no-op (e.g. fade with `start == end`) produces a file
//! whose decoded samples are bit-equal to what the renderer would have
//! emitted from the source. See `render.rs` for the determinism contract.
//!
//! The scale factor is `32_768.0`, matching the render path and the
//! decoder. This used to be `32_767.0`, on the reasoning that a sample
//! of exactly `1.0` should land on `i16::MAX` rather than wrap — but the
//! `clamp` on the next line already guarantees that, so the smaller
//! factor bought nothing and cost accuracy.
//!
//! It cost it asymmetrically. The decoder produces `v as f32 / 32_768.0`
//! for a 16-bit sample `v`, so multiplying by `32_768.0` returns exactly
//! `v` (integers below 2^24 are exact in `f32`), while multiplying by
//! `32_767.0` returns `v - v/32_768`, which rounds down to `v - 1` for
//! every `|v| > 16_384`. Every destructive edit pulled the loud half of
//! the signal one LSB toward zero — and, since edits chain, did it again
//! on the next one. The claim two paragraphs up, that a no-op edit is
//! bit-equal to no edit, was false for exactly that reason.
//!
//! `crates/tools/tests/untested_destructive_tools.rs` pins both halves:
//! a no-op edit is byte-identical to no edit, and ten of them still are.

use std::path::Path;

use hound::{SampleFormat, WavSpec, WavWriter};

use crate::{Error, Result};

/// Write interleaved `samples` to a 16-bit PCM WAV at `out`.
///
/// `channels` is the interleave stride; `samples.len()` must be a
/// multiple of `channels` (the caller is responsible — this function
/// writes whatever it's given and lets `hound` flag spec violations).
pub fn write_wav(samples: &[f32], sample_rate: u32, channels: u16, out: &Path) -> Result<()> {
    let spec = WavSpec {
        channels,
        sample_rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut writer = WavWriter::create(out, spec).map_err(Error::from)?;
    for &s in samples {
        let q = (s * 32_768.0)
            .round()
            .clamp(i16::MIN as f32, i16::MAX as f32) as i16;
        writer.write_sample(q).map_err(Error::from)?;
    }
    writer.finalize().map_err(Error::from)?;
    Ok(())
}
