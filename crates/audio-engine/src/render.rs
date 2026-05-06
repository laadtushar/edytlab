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
use hound::{SampleFormat, WavSpec, WavWriter};
use session::SessionState;

use crate::graph::{self, RenderGraph};
use crate::mixer::apply_gain_db;
use crate::{Error, RenderReport, TimeRange};

/// Special-case path: a unity render with no gain change must be byte-identical
/// to the source WAV. Any re-encoding (decode -> f32 -> requantize -> hound
/// write) introduces non-zero header differences (chunk ordering, fact chunks,
/// padding), so for the unity case we copy bytes directly. Higher-level effect
/// chains in Phase 2 will not hit this path.
fn is_unity_passthrough(graph: &RenderGraph, range: Option<TimeRange>) -> bool {
    graph.track_gain_db == 0.0 && range.is_none()
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

fn render_unity_copy(graph: &RenderGraph, out: &Path) -> Result<RenderReport, Error> {
    let bytes = std::fs::read(&graph.source_path)?;
    std::fs::write(out, &bytes)?;

    // Decode just to compute the report (peak, frames, duration). We pay this
    // cost once per render, never per-sample, so it doesn't affect the
    // 20× real-time budget.
    let decoded = decode_file(&graph.source_path)?;
    Ok(report_from_decoded(&decoded))
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

    let (start_frame, end_frame) = match range {
        None => (0usize, total_frames),
        Some(r) => {
            let s = (r.start_frame as usize).min(total_frames);
            let e = (r.end_frame as usize).min(total_frames);
            if e < s {
                return Err(Error::InvalidRange);
            }
            (s, e)
        }
    };

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

fn report_from_decoded(decoded: &DecodedAudio) -> RenderReport {
    let chans = decoded.channels as usize;
    let frames = (decoded.samples.len() / chans) as u64;
    let mut peak: f32 = 0.0;
    for s in &decoded.samples {
        let a = s.abs();
        if a > peak {
            peak = a;
        }
    }
    RenderReport {
        frames_written: frames,
        sample_rate: decoded.sample_rate,
        channels: decoded.channels,
        peak_dbfs: peak_to_dbfs(peak),
    }
}

fn peak_to_dbfs(peak: f32) -> f32 {
    if peak <= 0.0 {
        f32::NEG_INFINITY
    } else {
        20.0 * peak.log10()
    }
}
