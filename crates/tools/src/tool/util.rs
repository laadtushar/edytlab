//! Shared helpers for individual tools.
//!
//! Tools that mutate state need to (1) load the current head's state,
//! (2) clone-and-modify it, and (3) append a new node. Argument
//! validation also has a few common patterns (track index in range,
//! sample range well-formed). Centralised here so each tool stays
//! focused on its semantics.

use chrono::Utc;
use session::{NodeId, SessionNode, SessionState, Track};

use crate::ToolContext;

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
