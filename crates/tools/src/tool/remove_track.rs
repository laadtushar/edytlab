//! `remove_track` — drop a track from the session (M21).
//!
//! Removing a track shifts all higher-indexed tracks down by one. Callers
//! that hold cached track indices must refresh them after the call.

use serde::Deserialize;
use serde_json::{json, Value};

use crate::schema::{anthropic_tool, object_schema};
use crate::tool::util::{append_state, check_track_index, load_head_state};
use crate::{Tool, ToolContext, ToolResult};

#[derive(Debug, Deserialize)]
struct Args {
    track: usize,
}

pub struct RemoveTrackTool;

impl Tool for RemoveTrackTool {
    fn name(&self) -> &'static str {
        "remove_track"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "remove_track",
            "Remove a track from the session. Tracks at higher indices shift down by one. Returns the new session node id and the new track count.",
            object_schema(&[("track", "integer", true)]),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };

        let mut state = match load_head_state(ctx) {
            Ok(s) => s,
            Err(msg) => return Ok(ToolResult::Error(msg)),
        };

        if let Err(msg) = check_track_index(&state.tracks, args.track) {
            return Ok(ToolResult::Error(msg));
        }

        let removed = state.tracks.remove(args.track);
        // Rebuild `length_samples` as the maximum clip end across all
        // remaining tracks. Flatten across tracks since the per-track
        // grouping doesn't matter to the max.
        state.length_samples = state
            .tracks
            .iter()
            .flat_map(|t| &t.clips)
            .map(|c| c.start_in_track + c.length)
            .max()
            .unwrap_or(0);

        let track_count = state.tracks.len();
        let label = format!("remove_track {} ({})", args.track, removed.name);
        let new_id = match append_state(ctx, state, label) {
            Ok(id) => id,
            Err(msg) => return Ok(ToolResult::Error(msg)),
        };

        Ok(ToolResult::Ok(json!({
            "node_id": new_id.to_hex(),
            "removed_track_name": removed.name,
            "track_count": track_count,
            "summary": format!(
                "Removed track {} ({}); session now has {} track{}; new head {}",
                args.track,
                removed.name,
                track_count,
                if track_count == 1 { "" } else { "s" },
                new_id.to_hex(),
            ),
        })))
    }
}
