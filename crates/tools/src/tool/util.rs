//! Shared helpers for individual tools.
//!
//! Tools that mutate state need to (1) load the current head's state,
//! (2) clone-and-modify it, and (3) append a new node. Argument
//! validation also has a few common patterns (track index in range,
//! sample range well-formed). Centralised here so each tool stays
//! focused on its semantics.

use std::path::{Path, PathBuf};

use chrono::Utc;
use serde_json::json;
use session::{Clip, EnvelopePoint, NodeId, SessionNode, SessionState, Track};

use crate::{ToolContext, ToolResult};

/// Gain of a volume envelope at `frame`, matching the render engine's
/// interpolation exactly: flat before the first point, flat after the
/// last, linear in between.
///
/// Duplicated here rather than shared with `audio-engine` because the
/// engine's copy is `pub(crate)` and this crate only needs it to *slice*
/// a curve, never to render one. If the two ever disagree, the engine is
/// the authority — a slice that interpolates differently would shift the
/// automation by a fraction of a dB at the boundary.
fn envelope_gain_db_at(points: &[EnvelopePoint], frame: u64) -> f32 {
    if points.is_empty() {
        return 0.0;
    }
    if frame <= points[0].time_samples {
        return points[0].gain_db;
    }
    let last = &points[points.len() - 1];
    if frame >= last.time_samples {
        return last.gain_db;
    }
    let pos = points.partition_point(|p| p.time_samples <= frame);
    let a = &points[pos - 1];
    let b = &points[pos];
    let alpha = (frame - a.time_samples) as f32 / (b.time_samples - a.time_samples) as f32;
    a.gain_db + alpha * (b.gain_db - a.gain_db)
}

/// Restrict a clip's volume envelope to the window
/// `[from_frames, from_frames + len_frames)` and re-base it to zero.
///
/// Envelope times are relative to the clip's own start, so a tool that
/// cuts or splits a clip has to move them. Copying the points across
/// verbatim — which `split_clip` did — leaves the second half playing
/// the *beginning* of the curve: a fade-out written across a clip
/// restarts at full volume after the split.
///
/// Boundary points are synthesised at both ends so the surviving curve
/// keeps its shape. Without the one at 0 the sub-clip would start at
/// whatever gain the first *retained* point holds rather than the gain
/// the curve actually had there; without the one at `len_frames` the
/// tail would flatten off instead of continuing its ramp.
pub(crate) fn slice_envelope(
    points: &[EnvelopePoint],
    from_frames: u64,
    len_frames: u64,
) -> Vec<EnvelopePoint> {
    if points.is_empty() || len_frames == 0 {
        return Vec::new();
    }
    let end = from_frames.saturating_add(len_frames);
    let mut out = Vec::with_capacity(points.len() + 2);

    out.push(EnvelopePoint {
        time_samples: 0,
        gain_db: envelope_gain_db_at(points, from_frames),
    });
    for p in points {
        if p.time_samples > from_frames && p.time_samples < end {
            out.push(EnvelopePoint {
                time_samples: p.time_samples - from_frames,
                gain_db: p.gain_db,
            });
        }
    }
    out.push(EnvelopePoint {
        time_samples: len_frames,
        gain_db: envelope_gain_db_at(points, end),
    });

    // A window that lands entirely inside one flat segment produces the
    // same gain at both ends and nothing between; two identical points
    // say no more than one does.
    if out.len() == 2 && out[0].gain_db == out[1].gain_db {
        out.truncate(1);
    }
    out
}

/// Materialise a track's timeline as one WAV and return its path.
///
/// A track with a single clip already *is* a file on disk, and callers
/// that only need something to draw can use `source_path` directly. A
/// track split by a cut is not any single file, which is why the desktop
/// app's `list_tracks` reported no audio path for one at all and the
/// timeline lane came back blank.
///
/// The CAS name is hashed from the **clip descriptors**, not the audio.
/// That is what keeps this cheap enough to call from a listing: the same
/// clip list always yields the same audio, so a repeat call finds the
/// file already there and never touches the sources. Only a genuine miss
/// pays for a decode.
pub fn flattened_track_wav(project_dir: &Path, clips: &[Clip]) -> Result<PathBuf, String> {
    if clips.is_empty() {
        return Err("track has no clips".to_string());
    }

    let mut hasher = blake3::Hasher::new();
    for c in clips {
        hasher.update(c.source_path.to_string_lossy().as_bytes());
        hasher.update(&c.start_in_track.to_le_bytes());
        hasher.update(&c.source_offset.to_le_bytes());
        hasher.update(&c.length.to_le_bytes());
    }
    let hash_hex = hasher.finalize().to_hex().to_string();

    // Inside the project (#156), not beside whichever source happened to
    // be first: a project has to contain the audio it points at, or it
    // is not a thing anyone can copy or move.
    let derived_dir: PathBuf = crate::provenance::derived_dir(project_dir);
    let cas_path = derived_dir.join(format!("track-{hash_hex}.wav"));
    if cas_path.exists() {
        return Ok(cas_path);
    }

    let audio = flatten_track(clips)?;
    std::fs::create_dir_all(&derived_dir).map_err(|e| {
        format!(
            "failed to create derived dir {}: {e}",
            derived_dir.display()
        )
    })?;
    audio_engine::write_wav(
        &audio.window,
        audio.sample_rate,
        audio.channels.max(1),
        &cas_path,
    )
    .map_err(|e| format!("failed to write {}: {e}", cas_path.display()))?;
    Ok(cas_path)
}

/// Where a track's timeline ends: the furthest point any clip reaches.
///
/// Not `max(clip.length)`, which is what `cut_range` and `trim` used to
/// measure. On a track split into a 2 000-frame head and a 5 000-frame
/// tail, that reads 5 000 — so a range at 6 000, which is squarely
/// inside the timeline, came back "out of range".
pub(crate) fn timeline_end(clips: &[Clip]) -> u64 {
    clips
        .iter()
        .map(|c| c.start_in_track.saturating_add(c.length))
        .max()
        .unwrap_or(0)
}

/// The part of `clip` lying inside the timeline window `[from, to)`, moved
/// so the window's origin sits at `new_origin`.
///
/// `None` when the clip doesn't reach into the window at all. Everything
/// that travels with a clip travels with the piece: the source window
/// narrows to match, and the volume envelope is sliced to the same span so
/// automation stays attached to the audio it was written for.
fn clip_window(clip: &Clip, from: u64, to: u64, new_origin: u64) -> Option<Clip> {
    let clip_start = clip.start_in_track;
    let clip_end = clip_start.saturating_add(clip.length);
    let lo = clip_start.max(from);
    let hi = clip_end.min(to);
    if hi <= lo {
        return None;
    }
    let into_clip = lo - clip_start;
    let len = hi - lo;
    Some(Clip {
        source_path: clip.source_path.clone(),
        start_in_track: new_origin + (lo - from),
        source_offset: clip.source_offset.saturating_add(into_clip),
        length: len,
        content_hash: clip.content_hash,
        time_stretch_factor: clip.time_stretch_factor,
        pitch_shift_semitones: clip.pitch_shift_semitones,
        beat_grid: clip.beat_grid.clone(),
        volume_envelope: slice_envelope(&clip.volume_envelope, into_clip, len),
    })
}

/// Remove the timeline range `[start, end)` from a track and close the gap.
///
/// Every clip is considered, not just the first. A clip wholly before the
/// cut is untouched; one wholly after slides left by the cut's length; one
/// straddling it contributes whichever of its two ends survive. Rewriting
/// only `clips[0]` and assigning the result over `track.clips` — which is
/// what this used to do — deleted every other clip on the track, so a
/// second cut silently truncated the track at the first cut's join.
pub(crate) fn cut_timeline(clips: &[Clip], start: u64, end: u64) -> Vec<Clip> {
    let mut out = Vec::with_capacity(clips.len() + 1);
    for clip in clips {
        if let Some(head) = clip_window(clip, 0, start, 0) {
            out.push(head);
        }
        // The tail's window opens at `end` and its origin is `start`, which
        // is the leftward slide: a clip sitting at `end + d` lands at
        // `start + d`.
        if let Some(tail) = clip_window(clip, end, u64::MAX, start) {
            out.push(tail);
        }
    }
    out.sort_by_key(|c| c.start_in_track);
    out
}

/// Keep only the timeline range `[start, end)` and re-base it to zero.
pub(crate) fn keep_timeline(clips: &[Clip], start: u64, end: u64) -> Vec<Clip> {
    let mut out: Vec<Clip> = clips
        .iter()
        .filter_map(|c| clip_window(c, start, end, 0))
        .collect();
    out.sort_by_key(|c| c.start_in_track);
    out
}

/// Load the current head's [`SessionState`]. Returns `Err(message)`
/// shaped for [`crate::ToolResult::Error`] when there is no head or the
/// store can't materialise the node.
pub(crate) fn load_head_state(ctx: &ToolContext) -> Result<SessionState, String> {
    let head = ctx
        .store
        .head()
        .ok_or_else(|| "no session loaded; call `load` first".to_string())?;
    let node = ctx
        .store
        .get(head)
        .map_err(|e| format!("failed to read head node: {e}"))?;
    Ok(node.state)
}

/// Wrap `state` in a [`SessionNode`] and append it to the store. The
/// store overwrites `parent` and `id`, so we leave them at sentinel
/// values.
pub(crate) fn append_state(
    ctx: &mut ToolContext,
    state: SessionState,
    label: impl Into<String>,
) -> Result<NodeId, String> {
    let node = SessionNode {
        id: NodeId([0u8; 32]),
        parent: None,
        created_at: Utc::now(),
        label: Some(label.into()),
        reasoning: None,
        state,
        op: None,
    };
    ctx.store
        .append(node)
        .map_err(|e| format!("session append failed: {e}"))
}

/// Look up `track_index` against `tracks`, producing an actionable
/// error message when out of range (matches the format pinned by the
/// M08 acceptance criteria).
pub(crate) fn check_track_index(tracks: &[Track], track_index: usize) -> Result<(), String> {
    if track_index >= tracks.len() {
        return Err(format!(
            "track index {track_index} out of range; session has {} track{}",
            tracks.len(),
            if tracks.len() == 1 { "" } else { "s" },
        ));
    }
    Ok(())
}

/// Run a destructive sample-buffer edit against the first clip of
/// `state.tracks[track_idx]`, write the result to a CAS-addressed WAV
/// under the source's sibling `derived/` directory, swap the clip to
/// point at the new file, and append a new session node.
///
/// The `edit_fn` receives the clip's interleaved sample window and the
/// source sample rate. It mutates the buffer in place (length changes
/// allowed — `insert_silence` extends, the others preserve length).
///
/// Returns a [`ToolResult::Ok`] with `{ node_id, summary }` on success
/// or a [`ToolResult::Error`] with a human-readable message on any
/// validation / IO failure. The dispatcher contract is "all tool-level
/// failures are surfaced as `ToolResult::Error`", same as `gain` and
/// `cut_range`.
pub(crate) fn destructive_edit<F>(
    ctx: &mut ToolContext,
    track_idx: usize,
    edit_fn: F,
    label: impl Into<String>,
) -> ToolResult
where
    F: FnOnce(&mut Vec<f32>, u32),
{
    // The overwhelming majority of edits leave the channel layout
    // alone, so they keep the two-argument closure and simply hand the
    // incoming count straight back.
    destructive_edit_rechannel(
        ctx,
        track_idx,
        |samples, sample_rate, channels| {
            edit_fn(samples, sample_rate);
            channels
        },
        label,
    )
}

/// A track's clips decoded and laid out on one timeline.
pub(crate) struct TrackAudio {
    /// Interleaved samples from track frame 0 to the last clip's end.
    pub(crate) window: Vec<f32>,
    pub(crate) sample_rate: u32,
    pub(crate) channels: u16,
}

/// Decode every clip on a track and lay them out on a single buffer,
/// starting at track frame 0.
///
/// A track is a `Vec<Clip>`, and both `cut_range` with an interior range
/// and `split_clip` leave two of them behind. Editing only `clips[0]` —
/// which is what this used to do — reverbs the first half of a cut track
/// and leaves the second half dry, with a hard seam at the join.
///
/// The buffer starts at frame 0 rather than at the first clip so the
/// seconds a tool was handed still mean what the user meant. `fade
/// 0s–2s` has to fade the first two seconds *of the track*; if the
/// buffer began at a clip that starts at 0:05, the same call would fade
/// 0:05–0:07 instead. Gaps between clips — and any lead-in before the
/// first — are silence, exactly as the render engine treats them.
///
/// Clips are required to agree on sample rate and channel count. In
/// practice they always do, because multiple clips only ever arise from
/// splitting one source, and mixing rates here would need a resampler
/// the tool layer doesn't have. Disagreement is reported rather than
/// papered over.
///
/// Read-only tools want this too. Analysing `clips[0]`'s *source file*
/// answers a question about the file on disk, not about the track: after
/// a cut the file still contains the audio the cut removed, and after a
/// split it contains only the part the first clip happens to point at.
/// What the user asked about is the timeline.
pub(crate) fn flatten_track(clips: &[session::Clip]) -> Result<TrackAudio, String> {
    let mut decoded_clips: Vec<(&session::Clip, audio_decoder::DecodedAudio)> =
        Vec::with_capacity(clips.len());
    let mut sample_rate = 0u32;
    let mut channels = 0u16;

    for clip in clips {
        let decoded = audio_decoder::decode_file(&clip.source_path)
            .map_err(|e| format!("failed to decode {}: {e}", clip.source_path.display()))?;
        if decoded.channels == 0 {
            return Err(format!(
                "source {} has zero channels",
                clip.source_path.display()
            ));
        }
        if sample_rate == 0 {
            sample_rate = decoded.sample_rate;
            channels = decoded.channels;
        } else if decoded.sample_rate != sample_rate || decoded.channels != channels {
            return Err(format!(
                "track mixes formats across clips ({sample_rate} Hz / {channels} ch vs \
                 {} Hz / {} ch in {}); split the edit per clip or render the track first",
                decoded.sample_rate,
                decoded.channels,
                clip.source_path.display()
            ));
        }
        decoded_clips.push((clip, decoded));
    }

    let stride = channels as usize;
    let total_frames = clips
        .iter()
        .map(|c| c.start_in_track.saturating_add(c.length))
        .max()
        .unwrap_or(0) as usize;
    let mut window = vec![0.0f32; total_frames * stride];

    for (clip, decoded) in &decoded_clips {
        let src_total = (decoded.samples.len() / stride) as u64;
        let src_start = clip.source_offset.min(src_total);
        let src_end = clip
            .source_offset
            .saturating_add(clip.length)
            .min(src_total);
        let frames = src_end.saturating_sub(src_start) as usize;
        if frames == 0 {
            continue;
        }
        let src = &decoded.samples[(src_start as usize) * stride..(src_end as usize) * stride];
        let dst_start = (clip.start_in_track as usize) * stride;
        // `total_frames` is the furthest clip end, so a clip can only
        // overrun it when its source is shorter than its declared length
        // — already handled by clamping to `src_total` above. The `min`
        // is belt and braces against a malformed session.
        let dst_end = (dst_start + frames * stride).min(window.len());
        let copied = dst_end.saturating_sub(dst_start);
        // Overlapping clips sum, matching how the render engine mixes
        // them; a plain copy would silently drop whichever came first.
        for (d, s) in window[dst_start..dst_end].iter_mut().zip(&src[..copied]) {
            *d += *s;
        }
    }

    Ok(TrackAudio {
        window,
        sample_rate,
        channels,
    })
}

/// [`destructive_edit`] for edits that change the channel layout.
///
/// The closure receives the source's channel count and returns the
/// count its buffer now has. That return value is what gets written
/// into the WAV header and used to recompute the clip length — the
/// plain `destructive_edit` always wrote the *source's* count, so a
/// tool that halved or doubled the buffer produced a file whose header
/// disagreed with its contents. Playback then reinterprets the frames:
/// half the samples under a stereo header plays twice as fast and an
/// octave high, and twice the samples under a mono header plays half
/// as fast and an octave low.
pub(crate) fn destructive_edit_rechannel<F>(
    ctx: &mut ToolContext,
    track_idx: usize,
    edit_fn: F,
    label: impl Into<String>,
) -> ToolResult
where
    F: FnOnce(&mut Vec<f32>, u32, u16) -> u16,
{
    // Almost every edit keeps the sample rate it was handed, so the
    // three-argument closure stays the common case and the rate is
    // simply passed back through.
    destructive_edit_resample(
        ctx,
        track_idx,
        |samples, sample_rate, channels| {
            let channels_out = edit_fn(samples, sample_rate, channels);
            (sample_rate, channels_out)
        },
        label,
    )
}

/// [`destructive_edit_rechannel`] for edits that change the sample rate.
///
/// The closure returns the rate *and* the channel count its buffer now
/// has, and both go into the WAV header. `resample_track` is the one
/// tool that needs this: it writes a file at a rate the source never
/// had, which is why it carried its own copy of this function — and why
/// that copy went on editing `clips[0]` alone after the shared path
/// learned to flatten a split track.
pub(crate) fn destructive_edit_resample<F>(
    ctx: &mut ToolContext,
    track_idx: usize,
    edit_fn: F,
    label: impl Into<String>,
) -> ToolResult
where
    F: FnOnce(&mut Vec<f32>, u32, u16) -> (u32, u16),
{
    let label = label.into();

    let mut state = match load_head_state(ctx) {
        Ok(s) => s,
        Err(msg) => return ToolResult::Error(msg),
    };

    if let Err(msg) = check_track_index(&state.tracks, track_idx) {
        return ToolResult::Error(msg);
    }

    let clips = state.tracks[track_idx].clips.clone();
    let Some(first) = clips.first().cloned() else {
        return ToolResult::Error(format!("track {track_idx} has no clips; nothing to edit"));
    };

    let TrackAudio {
        mut window,
        sample_rate,
        channels,
    } = match flatten_track(&clips) {
        Ok(a) => a,
        Err(msg) => return ToolResult::Error(msg),
    };

    // Apply the user-provided edit. It reports the rate and channel count
    // its buffer now has, either of which may differ from the source's.
    let (rate_out, channels_out) = edit_fn(&mut window, sample_rate, channels);
    let rate_out = rate_out.max(1);
    let channels_out = channels_out.max(1);
    let stride_out = channels_out as usize;

    // CAS-address the result under `<project>/.audiograph/derived/`.
    let derived_dir: PathBuf = crate::provenance::derived_dir(ctx.store.project_dir());
    if let Err(e) = std::fs::create_dir_all(&derived_dir) {
        return ToolResult::Error(format!(
            "failed to create derived dir {}: {e}",
            derived_dir.display()
        ));
    }

    // Hash the post-edit interleaved samples. We serialize each f32 as
    // little-endian bytes so the hash is deterministic across platforms
    // and across rustc versions (no transmute / no endianness assumption).
    let mut hasher = blake3::Hasher::new();
    for s in &window {
        hasher.update(&s.to_le_bytes());
    }
    let hash = hasher.finalize();
    let hash_hex = hash.to_hex().to_string();
    let cas_path = derived_dir.join(format!("{hash_hex}.wav"));

    if !cas_path.exists() {
        if let Err(e) = audio_engine::write_wav(&window, rate_out, channels_out, &cas_path) {
            return ToolResult::Error(format!(
                "failed to write CAS wav {}: {e}",
                cas_path.display()
            ));
        }
    }

    // The edited buffer is the whole track laid end to end, so the track
    // collapses to the single clip that buffer now represents. For the
    // common one-clip track this is the same rewrite as before —
    // `start_in_track` was already 0 and the other fields are carried
    // over — and for a split track it is the join.
    let new_length_frames = (window.len() / stride_out) as u64;
    state.tracks[track_idx].clips = vec![session::Clip {
        source_path: cas_path,
        start_in_track: 0,
        source_offset: 0,
        length: new_length_frames,
        content_hash: Some(*hash.as_bytes()),
        time_stretch_factor: first.time_stretch_factor,
        pitch_shift_semitones: first.pitch_shift_semitones,
        beat_grid: first.beat_grid.clone(),
        volume_envelope: first.volume_envelope.clone(),
    }];

    // Recompute `length_samples` as the max of every track's max-clip
    // length. This matches the convention used elsewhere in the
    // dispatcher (cut_range tracks length deltas; gain leaves it alone;
    // here the clip itself changes length, so a fresh max is safest).
    state.length_samples = state
        .tracks
        .iter()
        .flat_map(|t| t.clips.iter().map(|c| c.start_in_track + c.length))
        .max()
        .unwrap_or(0);

    let new_id = match append_state(ctx, state, label.clone()) {
        Ok(id) => id,
        Err(msg) => return ToolResult::Error(msg),
    };

    ToolResult::Ok(json!({
        "node_id": new_id.to_hex(),
        "summary": label,
    }))
}

/// Interleave stride of the first clip on `track_idx`, read from the
/// decoded source.
///
/// `destructive_edit` hands its closure only `(samples, sample_rate)`,
/// so a tool that needs to convert seconds into an index has to learn
/// the channel count separately. Getting this wrong is not a subtle
/// error: indexing an interleaved stereo buffer as if it were mono
/// covers half the requested span and, when the length lands odd,
/// swaps left and right for everything after it.
pub(crate) fn track_channels(ctx: &mut ToolContext, track_idx: usize) -> Result<usize, String> {
    let state = load_head_state(ctx)?;
    check_track_index(&state.tracks, track_idx)?;
    let clip = state.tracks[track_idx]
        .clips
        .first()
        .ok_or_else(|| format!("track {track_idx} has no clips"))?;
    let decoded = audio_decoder::decode_file(&clip.source_path)
        .map_err(|e| format!("failed to decode {}: {e}", clip.source_path.display()))?;
    Ok((decoded.channels as usize).max(1))
}

/// Reject a `[start_sec, end_sec)` window that is reversed or not a
/// finite number, before it reaches slice arithmetic.
///
/// Tools that take bare `start_sec` / `end_sec` were clamping each end
/// to the buffer length *independently*, which leaves `start > end`
/// intact for a reversed window — and `samples[start..end]` then
/// panics, taking the whole app down. Asking for `start_sec: 10,
/// end_sec: 5` is an easy slip for a model to make, so this is reported
/// the way every other bad argument is, rather than silently treated as
/// an empty selection that hides the mistake.
pub(crate) fn check_seconds_order(start_sec: f64, end_sec: f64) -> Result<(), String> {
    if !start_sec.is_finite() || !end_sec.is_finite() {
        return Err(format!(
            "invalid range: start_sec ({start_sec}) and end_sec ({end_sec}) must be finite numbers"
        ));
    }
    if start_sec < 0.0 || end_sec < 0.0 {
        return Err(format!(
            "invalid range: start_sec ({start_sec}) and end_sec ({end_sec}) must not be negative"
        ));
    }
    if start_sec >= end_sec {
        return Err(format!(
            "invalid range: start_sec ({start_sec}) must be < end_sec ({end_sec})"
        ));
    }
    Ok(())
}

/// [`check_seconds_order`] for tools whose bounds are optional. A
/// missing bound means "from the start" / "to the end", so only a pair
/// that is present on both sides can be out of order.
pub(crate) fn check_optional_seconds_order(
    start_sec: Option<f64>,
    end_sec: Option<f64>,
) -> Result<(), String> {
    match (start_sec, end_sec) {
        (Some(s), Some(e)) => check_seconds_order(s, e),
        (Some(s), None) | (None, Some(s)) if !s.is_finite() || s < 0.0 => Err(format!(
            "invalid range: {s} must be a finite, non-negative number of seconds"
        )),
        _ => Ok(()),
    }
}

/// Validate `[start, end)` against a track's total length. Returns the
/// pair as `(usize, usize)` to make downstream slice math less noisy.
pub(crate) fn check_sample_range(
    start: u64,
    end: u64,
    track_length: u64,
) -> Result<(u64, u64), String> {
    if start >= end {
        return Err(format!(
            "invalid range: start_sample ({start}) must be < end_sample ({end})"
        ));
    }
    if end > track_length {
        return Err(format!(
            "end_sample ({end}) exceeds track length ({track_length})"
        ));
    }
    Ok((start, end))
}

// Biquad filtering lives in the `audio-dsp` crate.
//
// It used to be defined here and shared by four tools. Those tools'
// algorithms have since moved to `audio-dsp` too, so nothing in this
// crate references it any more — the re-export that briefly stood here
// was itself unused, which is the refactor having landed rather than
// something missing.
