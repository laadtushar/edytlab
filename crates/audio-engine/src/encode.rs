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
//! The function multiplies by `32_767.0` (not `32_768.0`) so a sample of
//! exactly `1.0` clamps to `i16::MAX` rather than wrapping. The render
//! path uses `32_768.0` *with* a `clamp` to the same range; both paths
//! produce identical bytes for any in-range input, and the choice here
//! matches the architect's spec.

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
        let q = (s * 32_767.0)
            .round()
            .clamp(i16::MIN as f32, i16::MAX as f32) as i16;
        writer.write_sample(q).map_err(Error::from)?;
    }
    writer.finalize().map_err(Error::from)?;
    Ok(())
}
