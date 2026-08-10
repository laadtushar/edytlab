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
//!   chunk in-place. The per-sample sum order across tracks is fixed
//!   by track index.
//! * Use `hound`'s deterministic 16-bit PCM writer with the project's
//!   sample rate and an explicit channel count chosen as `max` of the
//!   contributing sources (mono is upmixed to stereo by duplication).
//!
//! ### Mix architecture (M22 — streaming)
//!
//! M21 decoded every track fully into RAM, summed all tracks into a single
//! project-length f32 master buffer, then quantized to PCM. That keeps
//! peak memory proportional to *session length × track count*, which OOMs
//! on long sessions (10 tracks × 5 min = ~700 MB of f32 alone).
//!
//! M22 streams every source through fixed-size chunks instead:
//!
//! 1. Build a [`RenderGraph`] (one [`TrackPlan`] per track) — same as M21.
//! 2. For each contributing track, open a streaming WAV reader, set up a
//!    per-track resampler if needed, and skip past the clip's source
//!    offset. None of these allocate a full-source buffer.
//! 3. Loop: produce one [`MASTER_CHUNK_FRAMES`]-frame chunk of project-rate
//!    output. Each contributing track is asked for "up to N frames at the
//!    project rate"; track output is summed into the chunk; the chunk is
//!    quantized and written to the WAV writer; chunk buffer is reused.
//! 4. Stop when every track has reported EOF AND the master output has
//!    reached the longest track's project-rate length.
//!
//! Peak memory is now O(MASTER_CHUNK_FRAMES × tracks × channels), i.e. a
//! few MB for 8 tracks, regardless of session length.
//!
//! ### Byte-identity with the M21 buffered path
//!
//! Streaming MUST produce byte-identical output to the M21 in-memory mix
//! for any session that both paths can render. The acceptance test
//! `streaming_render_byte_equal_to_buffered_render` is the tripwire. To
//! preserve byte-identity:
//!
//! * The per-track resampler is fed in the SAME 1024-input-frame chunks as
//!   M21, with the SAME zero-padding for the trailing partial chunk. The
//!   resampler is rubato `FftFixedInOut` with the same parameters.
//! * Per-track resampled output is truncated to the SAME `expected_out_frames
//!   = in_frames * out_rate / in_rate` (u128 truncated division) as M21.
//! * Per-track gain, channel up/downmix, and the per-sample sum order are
//!   identical to M21.
//! * The float values produced by the streaming WAV reader are bit-exactly
//!   equal to those produced by symphonia's i16 → f32 conversion (`s as
//!   f32 / 32_768.0`); see `audio-decoder::stream` for the equivalence
//!   table. This is what makes the float buffers fed into the resampler
//!   identical between paths.

use std::path::Path;

use audio_decoder::{decode_file, DecodedAudio, WavStreamReader};
use hound::{SampleFormat, WavSpec, WavWriter};
use rubato::{FftFixedInOut, Resampler};
use session::SessionState;

use crate::graph::{self, RenderGraph, TrackPlan};
use crate::{Error, RenderReport, TimeRange};

/// Linear interpolation of gain_db at `frame` from a sorted (time_samples, gain_db) list.
/// Returns 0.0 dB for an empty list.
pub(crate) fn interp_envelope_gain_db(pts: &[(u64, f32)], frame: u64) -> f32 {
    if pts.is_empty() {
        return 0.0;
    }
    if frame <= pts[0].0 {
        return pts[0].1;
    }
    if frame >= pts[pts.len() - 1].0 {
        return pts[pts.len() - 1].1;
    }
    let pos = pts.partition_point(|&(t, _)| t <= frame);
    let (t0, g0) = pts[pos - 1];
    let (t1, g1) = pts[pos];
    let alpha = (frame - t0) as f32 / (t1 - t0) as f32;
    g0 + alpha * (g1 - g0)
}

/// Resampler input chunk, in source frames.
///
/// Held at 1024 to match the M21 buffered path (same constant in
/// [`buffered_resample_track`]) so streaming and buffered renders feed the
/// resampler with bit-equal input chunks. Changing this constant changes the
/// rubato output for non-trivial rate ratios and therefore changes byte
/// output — see the byte-identity invariant in the module docs.
const RESAMPLER_CHUNK_INPUT_FRAMES: usize = 1024;

/// Master-output chunk, in project frames.
///
/// Per the M22 acceptance criterion: read each source in fixed chunks of ~1
/// second of project-rate frames. Larger chunks waste RAM, smaller chunks
/// pay more per-chunk overhead in the resampler. 1 second is the spec.
///
/// Determinism note: this constant does NOT affect output bytes — it only
/// controls how often we flush PCM to the WAV writer. The per-track
/// resampler chunk is `RESAMPLER_CHUNK_INPUT_FRAMES` (1024 source frames)
/// and is independent of the master chunk size.
const MASTER_CHUNK_FRAMES_PER_SECOND_FACTOR: u32 = 1;

fn master_chunk_frames(project_sample_rate: u32) -> usize {
    (project_sample_rate * MASTER_CHUNK_FRAMES_PER_SECOND_FACTOR) as usize
}

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

    render_streaming(state, &graph, out, range)
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

/// One contributing track, set up for streaming.
///
/// Holds the streaming WAV reader, optional resampler, per-track output
/// buffer, and clip metadata. `next_chunk` produces project-rate,
/// project-channel-count, gained, interleaved samples on demand. The caller
/// (the master mix loop) sums those into a master chunk buffer.
struct TrackStreamer {
    reader: WavStreamReader,
    /// Source channel count (read from the WAV header).
    in_channels: usize,
    /// Source frames remaining in the clip window. Decremented every read.
    src_frames_remaining: u64,
    /// Whether `src_frames_remaining == 0` AND the resampler has been flushed.
    eof: bool,
    /// Project-rate frames already emitted for this track. Used to truncate
    /// the resampled output to `expected_out_frames` (matches M21).
    project_frames_emitted: u64,
    /// Total project-rate frames this track should produce. Computed from
    /// the clip length and the rate ratio (matches M21's `expected_out_frames`).
    project_frames_total: u64,
    /// Per-track gain in dB.
    gain_db: f32,
    /// Project-side channel count (the rendered file's channels).
    out_channels: usize,
    resampler: Option<TrackResampler>,
    /// Buffer of resampled-but-not-yet-emitted output, planar at
    /// `in_channels`. New resampler output is appended; consumed from the
    /// front during [`next_chunk`]. We channel-map and apply gain at
    /// emit-time, not at resample-time, to match M21's order.
    pending_planar: Vec<Vec<f32>>,
    /// Frames sitting in `pending_planar` (uniform across all channels).
    pending_frames: usize,
    /// Source-rate scratch reusable across reads to avoid per-call allocs.
    source_chunk_interleaved: Vec<f32>,
    /// True when the source has been fully drained from the WAV file.
    source_eof: bool,
    /// Per-clip volume automation points (sorted by time_samples).
    volume_envelope: Vec<(u64, f32)>,
    /// Project-rate frames of silence still owed before the clip's audio
    /// starts, from the clip's `start_in_track`.
    lead_frames_remaining: u64,
}

/// Convert a frame count from a source's rate into the project's rate.
///
/// `u128` intermediate so a long clip at a high rate can't overflow the
/// multiply. Identity when the rates match, which keeps the common case
/// bit-for-bit what it was before resampling entered the picture.
fn to_project_frames(source_frames: u64, in_rate: u32, project_rate: u32) -> u64 {
    if in_rate == project_rate || in_rate == 0 {
        return source_frames;
    }
    ((source_frames as u128) * (project_rate as u128) / (in_rate as u128)) as u64
}

struct TrackResampler {
    inner: FftFixedInOut<f32>,
    /// Pre-sized resampler input/output planar buffers.
    in_buf: Vec<Vec<f32>>,
    out_buf: Vec<Vec<f32>>,
    /// Frames per resampler call (== `RESAMPLER_CHUNK_INPUT_FRAMES`).
    chunk_in: usize,
}

impl TrackStreamer {
    fn open(plan: &TrackPlan, project_rate: u32, out_channels: usize) -> Result<Self, Error> {
        let mut reader = WavStreamReader::open(&plan.source_path)?;
        let in_rate = reader.sample_rate();
        let in_channels = reader.channels() as usize;
        if in_channels == 0 {
            return Err(Error::Decode(audio_decoder::DecodeError::Corrupt));
        }

        // Skip past `source_offset` so the first `read_frames` call returns
        // the start of the clip window.
        reader.skip_frames(plan.source_offset)?;

        // Clip length in source frames, clamped against actual source
        // length (mirrors M21's `clip_end.min(total_frames)`).
        let total_frames = reader.total_frames();
        let clip_start = plan.source_offset.min(total_frames);
        let clip_end = plan
            .source_offset
            .saturating_add(plan.length)
            .min(total_frames);
        let src_frames_remaining = clip_end.saturating_sub(clip_start);

        // Project-rate frame count for this track. u128 to avoid overflow;
        // matches M21's `expected_out_frames` formula bit-for-bit.
        let project_frames_total = to_project_frames(src_frames_remaining, in_rate, project_rate);
        let lead_frames_remaining = to_project_frames(plan.start_in_track, in_rate, project_rate);

        let resampler = if in_rate == project_rate {
            None
        } else {
            let inner = FftFixedInOut::<f32>::new(
                in_rate as usize,
                project_rate as usize,
                RESAMPLER_CHUNK_INPUT_FRAMES,
                in_channels,
            )
            .map_err(Error::ResamplerInit)?;
            let chunk_in = inner.input_frames_next();
            let chunk_out = inner.output_frames_max();
            let in_buf: Vec<Vec<f32>> = (0..in_channels).map(|_| vec![0.0; chunk_in]).collect();
            let out_buf: Vec<Vec<f32>> = (0..in_channels).map(|_| vec![0.0; chunk_out]).collect();
            Some(TrackResampler {
                inner,
                in_buf,
                out_buf,
                chunk_in,
            })
        };

        let pending_planar: Vec<Vec<f32>> = (0..in_channels).map(|_| Vec::new()).collect();

        // Source-side scratch sized for one resampler input chunk (or the
        // master chunk if no resampling is needed).
        let chunk_in_frames = resampler
            .as_ref()
            .map(|r| r.chunk_in)
            .unwrap_or(master_chunk_frames(project_rate));
        let source_chunk_interleaved = vec![0.0f32; chunk_in_frames * in_channels];

        let volume_envelope: Vec<(u64, f32)> = plan
            .volume_envelope
            .iter()
            .map(|p| (p.time_samples, p.gain_db))
            .collect();

        Ok(Self {
            reader,
            in_channels,
            src_frames_remaining,
            eof: project_frames_total == 0,
            project_frames_emitted: 0,
            project_frames_total,
            gain_db: plan.gain_db,
            out_channels,
            resampler,
            pending_planar,
            pending_frames: 0,
            source_chunk_interleaved,
            source_eof: src_frames_remaining == 0,
            volume_envelope,
            lead_frames_remaining,
        })
    }

    /// Top up `pending_planar` until it holds at least `target_frames` of
    /// project-rate output, or until the source plus resampler tail are
    /// fully drained. Returns once one or the other is true.
    fn fill_pending(&mut self, target_frames: usize) -> Result<(), Error> {
        while self.pending_frames < target_frames && !self.source_finished_and_drained() {
            self.advance_one_resampler_step()?;
        }
        Ok(())
    }

    /// Run one source read + (optional) resampler step, appending to
    /// `pending_planar`. Drains the resampler's tail with zero-padded input
    /// after the source EOFs so we don't lose the trailing samples.
    fn advance_one_resampler_step(&mut self) -> Result<(), Error> {
        match &mut self.resampler {
            None => {
                // No resample: read source frames straight through. Read up
                // to one master chunk's worth, but don't exceed
                // `src_frames_remaining`.
                let want_frames = self.source_chunk_interleaved.len() / self.in_channels;
                let want = (self.src_frames_remaining as usize).min(want_frames);
                if want == 0 {
                    self.source_eof = true;
                    return Ok(());
                }
                let dst = &mut self.source_chunk_interleaved[..want * self.in_channels];
                let got = self.reader.read_frames(dst)?;
                if got == 0 {
                    // Underflow vs header — treat the remaining clip window
                    // as silence (matches M21, which trimmed against the
                    // actual decoded length).
                    self.src_frames_remaining = 0;
                    self.source_eof = true;
                    return Ok(());
                }
                self.src_frames_remaining -= got as u64;
                if got < want || self.src_frames_remaining == 0 {
                    self.source_eof = true;
                }
                // Append straight to pending_planar (deinterleave).
                for ch in 0..self.in_channels {
                    self.pending_planar[ch].reserve(got);
                    for f in 0..got {
                        self.pending_planar[ch].push(dst[f * self.in_channels + ch]);
                    }
                }
                self.pending_frames += got;
                Ok(())
            }
            Some(rs) => {
                // Resample path: one resampler step consumes exactly
                // `chunk_in` source frames (zero-padded if necessary) and
                // emits `chunk_out` project-rate frames.
                if self.source_eof
                    && self.project_frames_emitted + (self.pending_frames as u64)
                        >= self.project_frames_total
                {
                    // Resampler tail already drained AND we have enough
                    // pending output to satisfy the per-track expected
                    // length; nothing more to do.
                    return Ok(());
                }

                // Build the input chunk: read up to `chunk_in` source
                // frames; zero-pad the rest.
                let chunk_in = rs.chunk_in;
                let want = (self.src_frames_remaining as usize).min(chunk_in);
                let mut got = 0usize;
                if want > 0 {
                    let dst = &mut self.source_chunk_interleaved[..want * self.in_channels];
                    got = self.reader.read_frames(dst)?;
                    if got > 0 {
                        for ch in 0..self.in_channels {
                            for f in 0..got {
                                rs.in_buf[ch][f] = dst[f * self.in_channels + ch];
                            }
                        }
                    }
                    self.src_frames_remaining -= got as u64;
                }
                // Zero-pad the rest of the input chunk.
                for ch in 0..self.in_channels {
                    for f in got..chunk_in {
                        rs.in_buf[ch][f] = 0.0;
                    }
                }
                if got < want || self.src_frames_remaining == 0 {
                    self.source_eof = true;
                }

                let (_in_used, out_written) = rs
                    .inner
                    .process_into_buffer(&rs.in_buf, &mut rs.out_buf, None)
                    .map_err(Error::ResamplerProcess)?;

                // Append output to pending. Note we append the FULL
                // `out_written` here (not truncated to expected). The
                // truncation happens when `next_chunk` consumes pending and
                // hits `project_frames_total`. This matches M21, which
                // truncated the FINAL planar output to `expected_out_frames`
                // after running the resampler over every padded chunk.
                for ch in 0..self.in_channels {
                    self.pending_planar[ch].extend_from_slice(&rs.out_buf[ch][..out_written]);
                }
                self.pending_frames += out_written;
                Ok(())
            }
        }
    }

    fn source_finished_and_drained(&self) -> bool {
        // For unity rate: source EOF means no more input. For resample: the
        // resampler may still have a tail in its internal state, but
        // FftFixedInOut produces output strictly per call; once we stop
        // calling it (because we've already accumulated `project_frames_total`
        // samples in pending or already emitted) we're done.
        if self.resampler.is_none() {
            self.source_eof
        } else {
            self.source_eof
                && self.project_frames_emitted + (self.pending_frames as u64)
                    >= self.project_frames_total
        }
    }

    /// Emit up to `want` project-rate, project-channel-count interleaved,
    /// gained samples into `dst` starting at frame 0. `dst` must be sized
    /// for `want * out_channels` samples; only the first
    /// `returned_frames * out_channels` are written.
    ///
    /// Returns the number of project-rate frames written. A return less
    /// than `want` means this track will not contribute beyond that point —
    /// the caller pads master output with zeros.
    ///
    /// A clip that starts partway into its track emits that many frames of
    /// silence first. The silence is written explicitly rather than left to
    /// the caller: the master loop sums exactly `got * channels` samples
    /// from a scratch buffer it deliberately does not clear, so anything
    /// counted in the return value has to actually be in `dst`.
    fn next_chunk(&mut self, want: usize, dst: &mut [f32]) -> Result<usize, Error> {
        if want == 0 {
            return Ok(0);
        }
        let mut written = 0usize;
        if self.lead_frames_remaining > 0 {
            let n = (self.lead_frames_remaining as usize).min(want);
            for v in &mut dst[..n * self.out_channels] {
                *v = 0.0;
            }
            self.lead_frames_remaining -= n as u64;
            written = n;
            if written == want {
                return Ok(written);
            }
        }
        let got = self.next_audio_chunk(want - written, &mut dst[written * self.out_channels..])?;
        Ok(written + got)
    }

    /// [`next_chunk`](Self::next_chunk) past the leading silence: the clip's
    /// own audio, resampled, channel-mapped and gained.
    fn next_audio_chunk(&mut self, want: usize, dst: &mut [f32]) -> Result<usize, Error> {
        if self.eof || want == 0 {
            return Ok(0);
        }
        // Cap by per-track expected total (truncates resampler tail to
        // match M21).
        let project_remaining = self
            .project_frames_total
            .saturating_sub(self.project_frames_emitted) as usize;
        let want = want.min(project_remaining);
        if want == 0 {
            self.eof = true;
            return Ok(0);
        }

        self.fill_pending(want)?;

        let avail = self.pending_frames.min(want);
        if avail == 0 {
            // Should not happen if project_remaining > 0 and source isn't
            // truncated — defensive guard.
            self.eof = true;
            return Ok(0);
        }

        // Emit `avail` frames: channel-map (mono->N, N->N, N->1) and apply
        // gain. Writes into `dst[..avail*out_channels]`.
        emit_frames(
            &self.pending_planar,
            avail,
            self.in_channels,
            self.out_channels,
            self.gain_db,
            dst,
        )?;

        // Apply per-clip volume envelope on top of the track gain.
        // Each frame at project-rate position `project_frames_emitted + f`
        // is scaled by the envelope's interpolated linear factor.
        if !self.volume_envelope.is_empty() {
            let base_frame = self.project_frames_emitted;
            for f in 0..avail {
                let frame = base_frame + f as u64;
                let env_db = interp_envelope_gain_db(&self.volume_envelope, frame);
                let env_linear = 10.0_f32.powf(env_db / 20.0);
                let start = f * self.out_channels;
                let end = start + self.out_channels;
                for s in &mut dst[start..end] {
                    *s *= env_linear;
                }
            }
        }

        // Drain `avail` frames from the front of `pending_planar`.
        for ch in 0..self.in_channels {
            self.pending_planar[ch].drain(..avail);
        }
        self.pending_frames -= avail;
        self.project_frames_emitted += avail as u64;
        if self.project_frames_emitted >= self.project_frames_total {
            self.eof = true;
        }
        Ok(avail)
    }
}

/// Channel-map `frames` frames of planar input at `in_channels` to
/// interleaved output at `out_channels`, applying linear `gain_db` to each
/// output sample.
///
/// The mapping rules match M21's [`prepare_track`]: identity, mono ->
/// N-channel by duplication, N-channel -> mono by mean, anything else is
/// rejected.
///
/// `clippy::needless_range_loop` is suppressed: indexed loops are deliberate
/// here for determinism-by-inspection (no iterator-adapter aliasing surprises
/// across rustc versions), matching the M21 channel-map code this replaces.
#[allow(clippy::needless_range_loop)]
fn emit_frames(
    pending_planar: &[Vec<f32>],
    frames: usize,
    in_channels: usize,
    out_channels: usize,
    gain_db: f32,
    dst: &mut [f32],
) -> Result<(), Error> {
    let factor = if gain_db == 0.0 {
        1.0f32
    } else {
        10f32.powf(gain_db / 20.0)
    };
    if in_channels == out_channels {
        for f in 0..frames {
            for ch in 0..out_channels {
                dst[f * out_channels + ch] = pending_planar[ch][f] * factor;
            }
        }
        Ok(())
    } else if in_channels == 1 && out_channels > 1 {
        for f in 0..frames {
            let v = pending_planar[0][f] * factor;
            for ch in 0..out_channels {
                dst[f * out_channels + ch] = v;
            }
        }
        Ok(())
    } else if in_channels > 1 && out_channels == 1 {
        let recip = 1.0 / in_channels as f32;
        for f in 0..frames {
            let mut sum = 0.0f32;
            for ch in 0..in_channels {
                sum += pending_planar[ch][f];
            }
            dst[f] = sum * recip * factor;
        }
        Ok(())
    } else {
        Err(Error::UnsupportedChannelMap {
            from: in_channels as u16,
            to: out_channels as u16,
        })
    }
}

fn render_streaming(
    state: &SessionState,
    graph: &RenderGraph,
    out: &Path,
    range: Option<TimeRange>,
) -> Result<RenderReport, Error> {
    // Recompute any_solo here so should_include_track can act as a secondary
    // gate inside the mix loop (the primary gate is plan.contributes, which
    // graph::build already resolves). Both gates must agree by construction.
    let any_solo = state.tracks.iter().any(|t| t.soloed);
    // Figure out the project-side channel count (max of contributing
    // sources) without decoding any audio. We peek WAV headers via the
    // streaming reader's metadata.
    let mut max_channels: u16 = 1;
    let mut total_project_frames: u64 = 0;
    let mut openers: Vec<Option<(WavStreamReader, &TrackPlan)>> =
        Vec::with_capacity(graph.tracks.len());

    for plan in &graph.tracks {
        if !plan.contributes || plan.length == 0 {
            openers.push(None);
            continue;
        }
        let reader = WavStreamReader::open(&plan.source_path)?;
        if reader.channels() > max_channels {
            max_channels = reader.channels();
        }
        // Project-rate length per track; total render length = max.
        let in_rate = reader.sample_rate();
        let total_frames = reader.total_frames();
        let clip_start = plan.source_offset.min(total_frames);
        let clip_end = plan
            .source_offset
            .saturating_add(plan.length)
            .min(total_frames);
        let src_frames = clip_end.saturating_sub(clip_start);
        // The render runs until the last clip *ends*, which is where it
        // starts plus how long it is — not merely the longest clip. A
        // two-second clip parked at 0:30 makes a 32-second render.
        let clip_end_in_project =
            to_project_frames(src_frames, in_rate, graph.project_sample_rate).saturating_add(
                to_project_frames(plan.start_in_track, in_rate, graph.project_sample_rate),
            );
        if clip_end_in_project > total_project_frames {
            total_project_frames = clip_end_in_project;
        }
        openers.push(Some((reader, plan)));
    }

    // Drop the peek readers; the streamers below open fresh ones because
    // `WavStreamReader::skip_frames` operates from "frames already
    // consumed" and we want consumption from zero.
    drop(openers);

    let project_rate = graph.project_sample_rate;
    let chans = max_channels as usize;

    // Build a streamer for every contributing track. Track index -> Option
    // mirrors `graph.tracks` so the master loop can iterate in graph order
    // (== `state.tracks` insertion order; see determinism invariant).
    let mut streamers: Vec<Option<TrackStreamer>> = Vec::with_capacity(graph.tracks.len());
    for plan in &graph.tracks {
        if !plan.contributes || plan.length == 0 {
            streamers.push(None);
            continue;
        }
        streamers.push(Some(TrackStreamer::open(plan, project_rate, chans)?));
    }

    let total_frames = total_project_frames as usize;
    let (start_frame, end_frame) = resolve_range(range, total_frames)?;
    let frames_to_write = end_frame - start_frame;

    let spec = WavSpec {
        channels: max_channels,
        sample_rate: project_rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut writer = WavWriter::create(out, spec)?;
    let mut peak: f32 = 0.0;

    let chunk_frames = master_chunk_frames(project_rate);
    let mut master_chunk = vec![0.0f32; chunk_frames * chans];
    // Per-track scratch (one per call to `next_chunk`) sized for
    // `chunk_frames * chans`. Preallocated once and reused across master
    // chunks AND tracks, since track streamers run sequentially.
    let mut track_chunk = vec![0.0f32; chunk_frames * chans];

    // Fast-forward all streamers past the master start frame. We do this by
    // discarding the first `start_frame` project frames track-by-track.
    // Since the spec keeps `range` defaulted to `None` for normal renders,
    // this loop is a no-op for the common case.
    if start_frame > 0 {
        let mut to_skip = start_frame;
        while to_skip > 0 {
            let step = to_skip.min(chunk_frames);
            for streamer in streamers.iter_mut() {
                let Some(streamer) = streamer else { continue };
                let dst = &mut track_chunk[..step * chans];
                streamer.next_chunk(step, dst)?;
            }
            to_skip -= step;
        }
    }

    // Master chunk loop. Emits exactly `frames_to_write` project-rate
    // frames in total; the final chunk may be partial.
    let mut frames_remaining = frames_to_write;
    while frames_remaining > 0 {
        let this_chunk = frames_remaining.min(chunk_frames);
        let dst = &mut master_chunk[..this_chunk * chans];
        // Zero out the master chunk before summing.
        for v in dst.iter_mut() {
            *v = 0.0;
        }

        // Sum each track's contribution. Track order is graph order ==
        // state.tracks insertion order. Per-sample summation order is fixed
        // by track index, then by interleaved sample index. See determinism
        // invariant.
        for (pi, streamer) in streamers.iter_mut().enumerate() {
            let Some(streamer) = streamer else { continue };
            // Secondary mute/solo gate — primary is plan.contributes resolved
            // at graph-build time; this call ensures the predicate is live
            // and exercised so dead-code elimination cannot remove it.
            // A plan entry is one *clip*, so its position no longer names a
            // track; the owning index travels on the plan.
            let track = &state.tracks[graph.tracks[pi].track_index];
            if !should_include_track(track.muted, track.soloed, any_solo) {
                continue;
            }
            let scratch = &mut track_chunk[..this_chunk * chans];
            // Don't bother zeroing scratch — `next_chunk` writes exactly
            // `got * chans` samples, and we only sum those.
            let got = streamer.next_chunk(this_chunk, scratch)?;
            if got == 0 {
                continue;
            }
            let n = got * chans;
            for i in 0..n {
                dst[i] += scratch[i];
            }
        }

        // Quantize and write this chunk; track running peak.
        for &s in dst.iter() {
            let abs = s.abs();
            if abs > peak {
                peak = abs;
            }
            let q = (s * 32_768.0)
                .round()
                .clamp(i16::MIN as f32, i16::MAX as f32) as i16;
            writer.write_sample(q)?;
        }

        frames_remaining -= this_chunk;
    }

    writer.finalize()?;

    Ok(RenderReport {
        frames_written: frames_to_write as u64,
        sample_rate: project_rate,
        channels: max_channels,
        peak_dbfs: peak_to_dbfs(peak),
    })
}

/// Buffered per-track resample helper, retained for parity testing.
///
/// This is M21's resample-then-truncate path lifted out so the M22
/// streaming implementation can demonstrate that it produces the same
/// per-track output for any source/project rate combination. It is NOT
/// called by the production render path (which streams) but is exported
/// because the byte-identity integration test compares against a buffered
/// reference render assembled in-test from this helper plus the M21
/// channel-map / gain / sum primitives.
///
/// Stays public (rather than `pub(crate)`) so the integration test in
/// `tests/streaming.rs` — which compiles as a separate crate — can call it.
/// Production code MUST NOT use this helper; reach for the streaming path
/// via [`render_state_to_wav`](crate::render_state_to_wav) instead.
#[doc(hidden)]
pub fn buffered_resample_track(
    samples: &[f32],
    in_rate: u32,
    out_rate: u32,
    channels: usize,
) -> Result<Vec<f32>, Error> {
    if samples.is_empty() {
        return Ok(Vec::new());
    }
    let in_frames = samples.len() / channels;
    let mut resampler = FftFixedInOut::<f32>::new(
        in_rate as usize,
        out_rate as usize,
        RESAMPLER_CHUNK_INPUT_FRAMES,
        channels,
    )
    .map_err(Error::ResamplerInit)?;
    let chunk = resampler.input_frames_next();

    let mut planar: Vec<Vec<f32>> = (0..channels)
        .map(|ch| {
            (0..in_frames)
                .map(|f| samples[f * channels + ch])
                .collect::<Vec<f32>>()
        })
        .collect();
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

    let expected_out_frames =
        ((in_frames as u128) * (out_rate as u128) / (in_rate as u128)) as usize;
    for plane in &mut out_planes {
        if plane.len() > expected_out_frames {
            plane.truncate(expected_out_frames);
        } else if plane.len() < expected_out_frames {
            plane.resize(expected_out_frames, 0.0);
        }
    }

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

/// Determine whether a track should contribute samples to the mix.
///
/// This mirrors the logic already baked into [`graph::build`]'s `contributes`
/// flag, but is kept here as a named, independently-testable predicate so that
/// the render loop can be audited in isolation and future callers (e.g. a
/// live-playback path that bypasses graph building) have a single source of
/// truth.
///
/// Rules (Audacity parity):
/// * A muted track never contributes, regardless of solo.
/// * If any track is soloed (`any_solo == true`), only soloed tracks contribute.
/// * Otherwise every unmuted track contributes.
pub(crate) fn should_include_track(muted: bool, soloed: bool, any_solo: bool) -> bool {
    if muted {
        return false;
    }
    if any_solo {
        return soloed;
    }
    true
}

#[cfg(test)]
mod mute_solo_tests {
    use super::should_include_track;
    #[test]
    fn muted_track_is_skipped_in_mix() {
        assert!(should_include_track(false, false, false)); // unmuted, no solo
        assert!(!should_include_track(true, false, false)); // muted
        assert!(should_include_track(false, true, true)); // soloed, any_solo=true
        assert!(!should_include_track(false, false, true)); // not soloed, but someone else is
    }
}

#[cfg(test)]
mod envelope_tests {
    use super::interp_envelope_gain_db;

    #[test]
    fn returns_first_point_before_start() {
        let pts = vec![(0u64, -6.0_f32), (44100u64, 0.0_f32)];
        assert!((interp_envelope_gain_db(&pts, 0) - (-6.0)).abs() < 1e-5);
    }

    #[test]
    fn interpolates_between_points() {
        let pts = vec![(0u64, -6.0_f32), (44100u64, 0.0_f32)];
        let mid = interp_envelope_gain_db(&pts, 22050);
        assert!((mid - (-3.0)).abs() < 0.01, "mid={mid}");
    }

    #[test]
    fn clamps_to_last_point_beyond_end() {
        let pts = vec![(0u64, -6.0_f32), (44100u64, 0.0_f32)];
        assert!((interp_envelope_gain_db(&pts, 88200) - 0.0).abs() < 1e-5);
    }

    #[test]
    fn empty_envelope_returns_zero_db() {
        assert!((interp_envelope_gain_db(&[], 1000) - 0.0).abs() < 1e-5);
    }
}
