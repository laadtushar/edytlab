//! Offline render execution.
//!
//! ### Determinism invariant
//!
//! Every byte of the produced WAV must be a deterministic function of the
//! input `SessionState` and source file contents. To uphold that across
//! Mac and Windows we:
//!
//! * Iterate samples (and tracks) with plain `for` loops in a fixed order.
//!   No `rayon::par_iter`, no `rayon::scope`, no `iter().fold()` parallel
//!   reductions. `rayon` is in `Cargo.toml` per the Phase 1 plan but is
//!   intentionally unused here.
//! * Mix tracks in `state.tracks` insertion order — never `HashMap`-iter
//!   order, never sorted by some derived key.
//! * Apply effects in declaration order, never `HashMap`-iteration order.
//! * Avoid `f32` summations that depend on associativity. Inside a single
//!   track each sample is read once and added to the running `f32` master
//!   buffer in-place. The per-sample sum order across tracks is fixed
//!   by track index.
//! * Use `hound`'s deterministic 16-bit PCM writer with the project's
//!   sample rate and an explicit channel count chosen as `max` of the
//!   contributing sources (mono is upmixed to stereo by duplication).
//!
//! ### Mix architecture (M21)
//!
//! 1. Build a [`RenderGraph`] (one [`TrackPlan`] per track).
//! 2. For each plan, decode -> trim -> resample to project rate -> apply
//!    gain -> upmix to project channel count.
//! 3. Sum frame-by-frame into a project-rate `Vec<f32>`.
//! 4. Quantize and write 16-bit PCM.
//!
//! The final length is the longest contributing track's frame count; tracks
//! that ended earlier contribute zeros for the remainder.

use std::path::Path;

use audio_decoder::{decode_file, DecodedAudio};
use hound::{SampleFormat, WavSpec, WavWriter};
use rubato::{FftFixedInOut, Resampler};
use session::SessionState;

use crate::graph::{self, RenderGraph, TrackPlan};
use crate::mixer::apply_gain_db;
use crate::{Error, RenderReport, TimeRange};

/// Special-case path: a unity render with no gain change must be byte-identical
/// to the source WAV. Any re-encoding (decode -> f32 -> requantize -> hound
/// write) introduces non-zero header differences (chunk ordering, fact chunks,
/// padding), so for the unity case we copy bytes directly.
///
/// Inverted check: we only take the byte-copy path when we have positively
/// verified the rendered file would be byte-equivalent to the source. Adding a
/// new processing knob (pan, effect, clip trim, mute, solo, second track) must
/// NOT silently fall through to byte-copy and bypass downstream logic.
fn is_unity_passthrough(graph: &RenderGraph, range: Option<TimeRange>) -> bool {
    range.is_none() && graph.unity_passthrough_track.is_some()
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

    render_processed(&graph, out, range)
}

/// Resolve a caller-supplied [`TimeRange`] into `(start_frame, end_frame)` in
/// the project-rate mixdown.
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
            if (r.start_frame as usize) > total_frames {
                return Err(Error::InvalidRange);
            }
            // End may exceed total; clamping it down is defensible since the
            // caller asked for "up to" `end_frame` and the source is the
            // hard limit.
            let s = r.start_frame as usize;
            let e = (r.end_frame as usize).min(total_frames);
            Ok((s, e))
        }
    }
}

fn render_unity_copy(graph: &RenderGraph, out: &Path) -> Result<RenderReport, Error> {
    let unity = graph
        .unity_passthrough_track
        .as_ref()
        .expect("is_unity_passthrough gated this branch");
    let bytes = std::fs::read(&unity.source_path)?;
    std::fs::write(out, &bytes)?;

    // Decode just to compute the report (peak, frames, duration). We pay this
    // cost once per render, never per-sample, so it doesn't affect the
    // 20× real-time budget.
    let decoded = decode_file(&unity.source_path)?;
    Ok(report_from_decoded(&decoded))
}

/// One contributing track materialised at the project rate.
///
/// We keep each track's samples interleaved at the project channel count so
/// the final summation is a flat per-sample loop. Channel count is uniform
/// across all tracks (the mix uses the max channel count among
/// contributors).
struct PreparedTrack {
    samples: Vec<f32>,
}

fn render_processed(
    graph: &RenderGraph,
    out: &Path,
    range: Option<TimeRange>,
) -> Result<RenderReport, Error> {
    // Decode every contributing track's source and trim to the clip window.
    // Tracks that don't contribute (muted-with-solo-elsewhere, or empty)
    // still occupy a slot in the prepared list as `None` so indexes line
    // up with `state.tracks` for diagnostics.
    let mut decoded_tracks: Vec<Option<DecodedClip>> = Vec::with_capacity(graph.tracks.len());
    let mut max_channels: u16 = 1;

    for plan in &graph.tracks {
        if !plan.contributes || plan.length == 0 {
            decoded_tracks.push(None);
            continue;
        }
        let decoded = decode_file(&plan.source_path)?;
        if decoded.channels > max_channels {
            max_channels = decoded.channels;
        }
        decoded_tracks.push(Some(DecodedClip { decoded, plan }));
    }

    let project_rate = graph.project_sample_rate;

    // Materialise each track at the project rate / project channel count.
    // After this loop every `Some(track)` has the same per-frame channel
    // count and the same sample rate, so summation is a flat add.
    let mut prepared: Vec<Option<PreparedTrack>> = Vec::with_capacity(decoded_tracks.len());
    for entry in decoded_tracks {
        match entry {
            None => prepared.push(None),
            Some(dc) => prepared.push(Some(prepare_track(dc, project_rate, max_channels)?)),
        }
    }

    // Master buffer length = the longest contributing track. Tracks that end
    // earlier contribute zeros for the remainder (no per-track padding —
    // we stop reading from them at their end).
    let chans = max_channels as usize;
    let total_frames = prepared
        .iter()
        .filter_map(|p| p.as_ref())
        .map(|p| p.samples.len() / chans)
        .max()
        .unwrap_or(0);

    let (start_frame, end_frame) = resolve_range(range, total_frames)?;
    let frames_to_write = end_frame - start_frame;

    // Sum into a master f32 buffer at project rate. Track summation order is
    // fixed by `prepared`'s index, which matches `state.tracks`. Within a
    // track the per-sample read is in interleaved (frame, channel) order.
    let mut master = vec![0.0f32; frames_to_write * chans];
    for entry in prepared.iter() {
        let Some(track) = entry else { continue };
        let track_frames = track.samples.len() / chans;
        // Per-track contribution starts at master frame 0; we don't yet
        // honor `start_in_track` offsets across tracks (M22 will). Each
        // track is mixed flush to time zero.
        let copy_start = start_frame.min(track_frames);
        let copy_end = end_frame.min(track_frames);
        if copy_end <= copy_start {
            continue;
        }
        let src = &track.samples[copy_start * chans..copy_end * chans];
        let dst_offset = (copy_start - start_frame) * chans;
        let dst = &mut master[dst_offset..dst_offset + src.len()];
        // Plain indexed loop — see determinism invariant.
        for (i, s) in src.iter().enumerate() {
            dst[i] += *s;
        }
    }

    // Quantize and write 16-bit PCM at project rate.
    let spec = WavSpec {
        channels: max_channels,
        sample_rate: project_rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut writer = WavWriter::create(out, spec)?;
    let mut peak: f32 = 0.0;
    for s in &master {
        let abs = s.abs();
        if abs > peak {
            peak = abs;
        }
        let q = (s * 32_768.0)
            .round()
            .clamp(i16::MIN as f32, i16::MAX as f32) as i16;
        writer.write_sample(q)?;
    }
    writer.finalize()?;

    Ok(RenderReport {
        frames_written: frames_to_write as u64,
        sample_rate: project_rate,
        channels: max_channels,
        peak_dbfs: peak_to_dbfs(peak),
    })
}

struct DecodedClip<'a> {
    decoded: DecodedAudio,
    plan: &'a TrackPlan,
}

/// Trim `decoded` to the clip window, resample to `project_rate`, upmix /
/// downmix to `out_channels`, and apply the track's gain in dB.
///
/// The output is interleaved at `out_channels` and `project_rate`.
fn prepare_track(
    dc: DecodedClip<'_>,
    project_rate: u32,
    out_channels: u16,
) -> Result<PreparedTrack, Error> {
    let DecodedClip { decoded, plan } = dc;
    let in_channels = decoded.channels as usize;
    let in_rate = decoded.sample_rate;
    let total_frames = decoded.samples.len() / in_channels;

    // Trim to the clip window.
    let clip_start = (plan.source_offset as usize).min(total_frames);
    let clip_end = ((plan.source_offset.saturating_add(plan.length)) as usize).min(total_frames);
    if clip_start > clip_end {
        return Err(Error::InvalidRange);
    }
    let trimmed: Vec<f32> =
        decoded.samples[clip_start * in_channels..clip_end * in_channels].to_vec();

    // Resample (if needed) at the source's channel count, then channel-map.
    let at_project_rate: Vec<f32> = if in_rate == project_rate {
        trimmed
    } else {
        resample_interleaved(&trimmed, in_rate, project_rate, in_channels)?
    };

    // Channel map: mono -> N (duplicate), stereo -> stereo (passthrough),
    // stereo -> mono is not produced by the current planner (max channels
    // is taken across contributors). The third combination would be
    // `out_channels < in_channels`; we average channels in that case for
    // robustness but it's not currently exercised.
    let out_chans = out_channels as usize;
    let mapped = if in_channels == out_chans {
        at_project_rate
    } else if in_channels == 1 && out_chans > 1 {
        let frames = at_project_rate.len();
        let mut out = Vec::with_capacity(frames * out_chans);
        for s in &at_project_rate {
            for _ in 0..out_chans {
                out.push(*s);
            }
        }
        out
    } else if in_channels > 1 && out_chans == 1 {
        let frames = at_project_rate.len() / in_channels;
        let mut out = Vec::with_capacity(frames);
        let recip = 1.0 / in_channels as f32;
        for f in 0..frames {
            let mut sum = 0.0f32;
            for c in 0..in_channels {
                sum += at_project_rate[f * in_channels + c];
            }
            out.push(sum * recip);
        }
        out
    } else {
        // Mismatched stereo widths (e.g. 5.1 -> stereo) are out of scope.
        return Err(Error::UnsupportedChannelMap {
            from: in_channels as u16,
            to: out_channels,
        });
    };

    let mut samples = mapped;
    apply_gain_db(&mut samples, plan.gain_db);
    Ok(PreparedTrack { samples })
}

/// Resample interleaved `samples` from `in_rate` to `out_rate` using
/// rubato's FFT resampler. The output preserves interleaving.
///
/// `FftFixedInOut` operates on planar buffers in fixed-size chunks; we
/// deinterleave into planar, run the resampler chunk-by-chunk (final chunk
/// is zero-padded so we don't lose the tail), and reinterleave the output.
fn resample_interleaved(
    samples: &[f32],
    in_rate: u32,
    out_rate: u32,
    channels: usize,
) -> Result<Vec<f32>, Error> {
    if samples.is_empty() {
        return Ok(Vec::new());
    }
    let in_frames = samples.len() / channels;

    // 1024-frame chunk matches audio-io's realtime resampler. The exact
    // chunk size doesn't change correctness because we feed full chunks
    // (zero-padding the tail) and clip the output to the expected frame
    // count.
    let mut resampler =
        FftFixedInOut::<f32>::new(in_rate as usize, out_rate as usize, 1024, channels)
            .map_err(Error::ResamplerInit)?;
    let chunk = resampler.input_frames_next();

    // Deinterleave to planar.
    let mut planar: Vec<Vec<f32>> = (0..channels)
        .map(|ch| {
            (0..in_frames)
                .map(|f| samples[f * channels + ch])
                .collect::<Vec<f32>>()
        })
        .collect();

    // Pad each plane up to a multiple of `chunk` so the resampler consumes
    // every input frame; trailing zeros after the real audio are harmless.
    let pad_to = in_frames.div_ceil(chunk) * chunk;
    for plane in &mut planar {
        plane.resize(pad_to, 0.0);
    }

    let mut out_planes: Vec<Vec<f32>> = (0..channels).map(|_| Vec::new()).collect();
    let mut in_buf: Vec<Vec<f32>> = (0..channels).map(|_| vec![0.0f32; chunk]).collect();
    let out_chunk = resampler.output_frames_max();
    let mut out_buf: Vec<Vec<f32>> = (0..channels).map(|_| vec![0.0f32; out_chunk]).collect();

    let mut pos = 0;
    while pos + chunk <= pad_to {
        for ch in 0..channels {
            in_buf[ch].copy_from_slice(&planar[ch][pos..pos + chunk]);
        }
        let (_in_used, out_written) = resampler
            .process_into_buffer(&in_buf, &mut out_buf, None)
            .map_err(Error::ResamplerProcess)?;
        for ch in 0..channels {
            out_planes[ch].extend_from_slice(&out_buf[ch][..out_written]);
        }
        pos += chunk;
    }

    // Trim to the expected output length, computed from the rate ratio.
    // We multiply through u128 to avoid overflow on long files.
    let expected_out_frames =
        ((in_frames as u128) * (out_rate as u128) / (in_rate as u128)) as usize;
    for plane in &mut out_planes {
        if plane.len() > expected_out_frames {
            plane.truncate(expected_out_frames);
        } else if plane.len() < expected_out_frames {
            plane.resize(expected_out_frames, 0.0);
        }
    }

    // Reinterleave. Plain indexed loop for determinism — see the
    // file-level invariant.
    let mut out = Vec::with_capacity(expected_out_frames * channels);
    #[allow(clippy::needless_range_loop)]
    for f in 0..expected_out_frames {
        for ch in 0..channels {
            out.push(out_planes[ch][f]);
        }
    }
    Ok(out)
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
