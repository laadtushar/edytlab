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

/// Quantise one f32 sample to 16-bit.
///
/// Shared by both encoders so a WAV and a FLAC of the same render are
/// sample-identical rather than merely similar. FLAC is lossless, so
/// that is an equality the round-trip test can assert — and it only
/// holds if both formats round the same way. See the scale-factor note
/// in the module docs for why this is `32_768.0`.
#[inline]
fn quantise(s: f32) -> i16 {
    (s * 32_768.0)
        .round()
        .clamp(i16::MIN as f32, i16::MAX as f32) as i16
}

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
        writer.write_sample(quantise(s)).map_err(Error::from)?;
    }
    writer.finalize().map_err(Error::from)?;
    Ok(())
}

/// Write interleaved `samples` to a 16-bit FLAC at `out`.
///
/// ## Why `flac-codec`
///
/// Chosen for what it *doesn’t* bring: four small pure-Rust
/// dependencies, no build script, no `-sys` crate. The workspace builds
/// on macOS, Windows and Linux in CI, and a native dependency is the
/// kind of thing that breaks all three at once — it is why Rubber Band
/// stayed out of the tree and the time-stretch tools sat unimplemented
/// for months (see `crates/audio-time/src/vocoder.rs`). MIT/Apache-2.0
/// also matches this repo’s licence.
///
/// ## Buffered, not streaming
///
/// The render path writes WAV chunk-by-chunk through `hound`. This
/// takes the whole buffer instead. `render_final` already materialises
/// the mix before calling an encoder, so nothing is made worse today;
/// if that stops being true, `FlacSampleWriter::write` accepts
/// successive slices and this can be fed incrementally without changing
/// its callers.
pub fn write_flac(samples: &[f32], sample_rate: u32, channels: u16, out: &Path) -> Result<()> {
    use flac_codec::encode::{FlacSampleWriter, Options};

    let channel_count =
        u8::try_from(channels.max(1)).map_err(|_| Error::UnsupportedChannelMap {
            from: channels,
            to: 8,
        })?;

    let file = std::fs::File::create(out)?;
    let writer = std::io::BufWriter::new(file);

    // `total_samples` is deliberately `None`. The encoder errors if the
    // declared count and the written count disagree, and passing a
    // length we would have to keep in step with the loop below is a
    // second source of truth for no benefit — the header is patched on
    // finalize either way.
    let mut enc = FlacSampleWriter::new(
        writer,
        Options::default(),
        sample_rate,
        16,
        channel_count,
        None,
    )
    .map_err(|e| Error::Encode(e.to_string()))?;

    // The encoder takes i32 holding 16-bit-range values, quantised
    // identically to the WAV path so the two agree sample-for-sample.
    let quantised: Vec<i32> = samples.iter().map(|&s| quantise(s) as i32).collect();
    enc.write(&quantised)
        .map_err(|e| Error::Encode(e.to_string()))?;
    enc.finalize().map_err(|e| Error::Encode(e.to_string()))?;
    Ok(())
}
