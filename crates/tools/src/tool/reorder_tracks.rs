//! `reorder_tracks` — move a track to a new position in the track list
//! (#771).
//!
//! Tools name a track by its position, so a reorder renumbers tracks. The
//! off-lock re-run rule (`shared.rs`, step 5) is what keeps a call that was
//! made before the reorder from landing on another track: it remembers each
//! named track's id and refuses to run on a position that now holds a
//! different one.

use serde::Deserialize;
use serde_json::{json, Value};

use crate::schema::{anthropic_tool, object_schema};
use crate::tool::util::{append_state, check_track_index, load_head_state};
use crate::{Tool, ToolContext, ToolResult};

#[derive(Debug, Deserialize)]
struct Args {
    track: usize,
    new_index: usize,
}

pub struct ReorderTracksTool;

impl Tool for ReorderTracksTool {
    fn name(&self) -> &'static str {
        "reorder_tracks"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "reorder_tracks",
            "Move a track to new_index; the tracks between shift by one, so indices change.",
            object_schema(&[("track", "integer", true), ("new_index", "integer", true)]),
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
        let count = state.tracks.len();
        if args.new_index >= count {
            return Ok(ToolResult::Error(format!(
                "new_index {} out of range (session has {count} track{})",
                args.new_index,
                if count == 1 { "" } else { "s" }
            )));
        }
        if args.track == args.new_index {
            return Ok(ToolResult::Error(format!(
                "track {} is already at index {}; nothing to move",
                args.track, args.new_index
            )));
        }

        let track = state.tracks.remove(args.track);
        let name = track.name.clone();
        state.tracks.insert(args.new_index, track);

        let order: Vec<&str> = state.tracks.iter().map(|t| t.name.as_str()).collect();
        let summary_order = order.join(", ");
        let new_id = match append_state(
            ctx,
            state,
            format!(
                "reorder_tracks {} -> {} ({name})",
                args.track, args.new_index
            ),
        ) {
            Ok(id) => id,
            Err(msg) => return Ok(ToolResult::Error(msg)),
        };

        Ok(ToolResult::Ok(json!({
            "node_id": new_id.to_hex(),
            "from": args.track,
            "to": args.new_index,
            "summary": format!(
                "Moved track {} ({name}) to index {}; order is now: {summary_order}; new head {}",
                args.track,
                args.new_index,
                new_id.to_hex(),
            ),
        })))
    }
}
