//! Offline render execution.
//!
//! ### Determinism invariant
//!
//! Every byte of the produced WAV must be a deterministic function of the
//! input `SessionState` and source file contents. To uphold that across
//! Mac and Windows we:
//!
//! * Iterate samples with plain `for` loops in a fixed order. No
//!   `rayon::par_iter`, no `rayon::scope`, no `iter().fold()` parallel
//!   reductions. `rayon` is in `Cargo.toml` per the Phase 1 plan but is
//!   intentionally unused here.
//! * Apply effects in declaration order, never `HashMap`-iteration order.
//! * Avoid `f32` summations that depend on associativity (Phase 1 has no
//!   summing — single-track only — so this is trivially upheld; Phase 2
//!   must pin a fixed mix order).
//! * Use `hound`'s deterministic 16-bit PCM writer with the source's
//!   sample rate and explicit stereo or mono channel count derived from
//!   the decoded source.

use std::path::Path;

use audio_decoder::{decode_file, DecodedAudio};
// `decode_file`/`DecodedAudio` are still used by the processed render path; the
// unity fast path now streams via `hound` directly and never allocates a Vec.
use hound::{SampleFormat, WavSpec, WavWriter};
use session::SessionState;

use crate::graph::{self, RenderGraph};
use crate::mixer::apply_gain_db;
use crate::{Error, RenderReport, TimeRange};

/// Special-case path: a unity render with no gain change must be byte-identical
/// to the source WAV. Any re-encoding (decode -> f32 -> requantize -> hound
/// write) introduces non-zero header differences (chunk ordering, fact chunks,
/// padding), so for the unity case we copy bytes directly.
///
/// Inverted check: we only take the byte-copy path when we have positively
/// verified the rendered file would be byte-equivalent to the source. Adding a
/// new processing knob (pan, effect, clip trim, mute, solo) must NOT silently
/// fall through to byte-copy and bypass downstream logic. Phase 2 fields are
/// gated here too — when their semantics arrive, this gate will keep the fast
/// path correct by default.
fn is_unity_passthrough(graph: &RenderGraph, range: Option<TimeRange>) -> bool {
    range.is_none()
        && graph.track_gain_db == 0.0
        && graph.track_pan == 0.0
        && !graph.muted
        && !graph.soloed
        && graph.effects_empty
        && graph.clip_covers_full_source
}

pub fn render(
    state: &SessionState,
    out: &Path,
    range: Option<TimeRange>,
) -> Result<RenderReport, Error> {
    let graph = graph::build(state)?;

    if is_unity_passthrough(&graph, range) {
        return render_unity_copy(&graph, out);
    }

    let decoded = decode_file(&graph.source_path)?;
    render_processed(&graph, decoded, out, range)
}

/// Resolve a caller-supplied [`TimeRange`] into `(start_frame, end_frame)` in
/// the decoded sample buffer.
///
/// CRITICAL: order validation runs on the RAW values from `range` BEFORE any
/// clamping. Clamping first lets pathological inputs like
/// `start=200, end=150, total=100` collapse to `(100, 100)` and silently
/// produce a zero-frame render. Validate, then clamp.
pub(crate) fn resolve_range(
    range: Option<TimeRange>,
    total_frames: usize,
) -> Result<(usize, usize), Error> {
    match range {
        None => Ok((0, total_frames)),
        Some(r) => {
            if r.start_frame >= r.end_frame {
                return Err(Error::InvalidRange);
            }
            // Compare in u64 space so a u64 start frame larger than usize::MAX
            // on a 32-bit target can't truncate-wrap into "in bounds".
            if r.start_frame > total_frames as u64 {
                return Err(Error::InvalidRange);
            }
            // End may exceed total; clamping is defensible since the caller
            // asked for "up to" `end_frame` and the source is the hard limit.
            // Cast is safe now: start <= total_frames (usize) so it fits.
            let s = r.start_frame as usize;
            let e = (r.end_frame.min(total_frames as u64)) as usize;
            Ok((s, e))
        }
    }
}

fn render_unity_copy(graph: &RenderGraph, out: &Path) -> Result<RenderReport, Error> {
    // Use std::fs::copy to leverage OS-level fast paths (sendfile,
    // copy_file_range, CopyFileEx) instead of round-tripping the entire file
    // through a userspace Vec<u8>. For a 10-minute stereo WAV this avoids a
    // 60+ MB heap allocation.
    std::fs::copy(&graph.source_path, out)?;

    // Compute the report by streaming the WAV header + i16 samples directly
    // via hound, never decoding to f32 or allocating the full sample buffer.
    // This keeps the unity fast path O(n) integer-scan with no heap growth,
    // instead of the previous full decode_file() which allocated a Vec<f32>
    // sized to the entire source.
    report_from_wav_stream(&graph.source_path)
}

/// Stream the source WAV's int16 samples to compute a [`RenderReport`] without
/// allocating a full sample Vec. Used by the unity fast path so the report
/// fidelity matches the processed path while preserving the byte-copy speedup.
fn report_from_wav_stream(path: &Path) -> Result<RenderReport, Error> {
    let mut reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    let total_samples = reader.duration() as u64;
    let channels = spec.channels;
    let sample_rate = spec.sample_rate;

    let mut peak_abs: i32 = 0;
    // Plain sequential scan over int16 samples. No f32 conversion, no Vec.
    // Note: i16::MIN.abs() overflows i16 but fits in i32, hence the i32 acc.
    for sample in reader.samples::<i16>() {
        let s = sample? as i32;
        let a = s.abs();
        if a > peak_abs {
            peak_abs = a;
        }
    }

    // Convert int peak to the same f32 [0.0, 1.0] domain used by the
    // processed path's peak_to_dbfs, dividing by 32768.0 (i16 full-scale).
    let peak = (peak_abs as f32) / 32_768.0;

    Ok(RenderReport {
        frames_written: total_samples,
        sample_rate,
        channels,
        peak_dbfs: peak_to_dbfs(peak),
    })
}

fn render_processed(
    graph: &RenderGraph,
    mut decoded: DecodedAudio,
    out: &Path,
    range: Option<TimeRange>,
) -> Result<RenderReport, Error> {
    apply_gain_db(&mut decoded.samples, graph.track_gain_db);

    let channels = decoded.channels;
    let sample_rate = decoded.sample_rate;
    let total_frames = decoded.samples.len() / channels as usize;

    let (start_frame, end_frame) = resolve_range(range, total_frames)?;

    let spec = WavSpec {
        channels,
        sample_rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut writer = WavWriter::create(out, spec)?;

    let chans = channels as usize;
    let mut peak: f32 = 0.0;
    // Plain indexed loop, single-threaded. See determinism invariant.
    for frame in start_frame..end_frame {
        for ch in 0..chans {
            let s = decoded.samples[frame * chans + ch];
            let abs = s.abs();
            if abs > peak {
                peak = abs;
            }
            let q = (s * 32_768.0)
                .round()
                .clamp(i16::MIN as f32, i16::MAX as f32) as i16;
            writer.write_sample(q)?;
        }
    }
    writer.finalize()?;

    let frames_written = end_frame - start_frame;
    Ok(RenderReport {
        frames_written: frames_written as u64,
        sample_rate,
        channels,
        peak_dbfs: peak_to_dbfs(peak),
    })
}

fn peak_to_dbfs(peak: f32) -> f32 {
    if peak <= 0.0 {
        f32::NEG_INFINITY
    } else {
        20.0 * peak.log10()
    }
}
