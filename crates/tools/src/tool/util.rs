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
use session::{NodeId, SessionNode, SessionState, Track};

use crate::{ToolContext, ToolResult};

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
    let label = label.into();

    let mut state = match load_head_state(ctx) {
        Ok(s) => s,
        Err(msg) => return ToolResult::Error(msg),
    };

    if let Err(msg) = check_track_index(&state.tracks, track_idx) {
        return ToolResult::Error(msg);
    }

    let Some(clip) = state.tracks[track_idx].clips.first().cloned() else {
        return ToolResult::Error(format!("track {track_idx} has no clips; nothing to edit"));
    };

    // Decode the source WAV into interleaved f32. The audio-decoder
    // returns the entire file regardless of clip window, so we slice
    // down to `[source_offset, source_offset + length)` in frames.
    let decoded = match audio_decoder::decode_file(&clip.source_path) {
        Ok(d) => d,
        Err(e) => {
            return ToolResult::Error(format!(
                "failed to decode {}: {e}",
                clip.source_path.display()
            ))
        }
    };
    let sample_rate = decoded.sample_rate;
    let channels = decoded.channels;
    if channels == 0 {
        return ToolResult::Error("source has zero channels".into());
    }
    let stride = channels as usize;
    let total_frames = (decoded.samples.len() / stride) as u64;
    let src_start = clip.source_offset.min(total_frames);
    let src_end = clip
        .source_offset
        .saturating_add(clip.length)
        .min(total_frames);
    let start_idx = (src_start as usize) * stride;
    let end_idx = (src_end as usize) * stride;
    let mut window: Vec<f32> = decoded.samples[start_idx..end_idx].to_vec();

    // Apply the user-provided edit.
    edit_fn(&mut window, sample_rate);

    // CAS-address the result under `<source_dir>/derived/<hash>.wav`.
    let parent: &Path = clip.source_path.parent().unwrap_or_else(|| Path::new("."));
    let derived_dir: PathBuf = parent.join("derived");
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
        if let Err(e) = audio_engine::write_wav(&window, sample_rate, channels, &cas_path) {
            return ToolResult::Error(format!(
                "failed to write CAS wav {}: {e}",
                cas_path.display()
            ));
        }
    }

    // Update the clip in place: point at the new source, zero offset,
    // length recomputed from the post-edit buffer.
    let new_length_frames = (window.len() / stride) as u64;
    let clip_mut = &mut state.tracks[track_idx].clips[0];
    clip_mut.source_path = cas_path;
    clip_mut.source_offset = 0;
    clip_mut.length = new_length_frames;
    clip_mut.content_hash = Some(*hash.as_bytes());

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
