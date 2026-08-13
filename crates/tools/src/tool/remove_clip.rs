//! `remove_clip` — drop one clip from a track, leaving the rest.
//!
//! `remove_track` removes the whole lane. Once a track has been cut,
//! "delete that bit" means one clip, and there was no way to say it.

use serde::Deserialize;
use serde_json::{json, Value};

use crate::schema::anthropic_tool;
use crate::tool::move_clip::recompute_length;
use crate::tool::util::{append_state, check_track_index, load_head_state};
use crate::{Tool, ToolContext, ToolResult};

#[derive(Debug, Deserialize)]
struct Args {
    track: usize,
    clip_index: usize,
}

pub struct RemoveClipTool;

impl Tool for RemoveClipTool {
    fn name(&self) -> &'static str {
        "remove_clip"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "remove_clip",
            "Remove one clip from a track, leaving the other clips where they are. The gap it \
             leaves is silence — this does not close up the timeline. Use remove_track to drop \
             a whole track. Appends a new session node.",
            json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer", "minimum": 0 },
                    "clip_index": { "type": "integer", "minimum": 0 }
                },
                "required": ["track", "clip_index"]
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };
        let mut state = match load_head_state(ctx) {
            Ok(s) => s,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        if let Err(e) = check_track_index(&state.tracks, args.track) {
            return Ok(ToolResult::Error(e));
        }
        let track = &mut state.tracks[args.track];
        if args.clip_index >= track.clips.len() {
            return Ok(ToolResult::Error(format!(
                "clip_index {} out of range; track {} has {} clip{}",
                args.clip_index,
                args.track,
                track.clips.len(),
                if track.clips.len() == 1 { "" } else { "s" },
            )));
        }
        track.clips.remove(args.clip_index);
        let remaining = track.clips.len();
        recompute_length(&mut state);

        let new_id = match append_state(
            ctx,
            state,
            format!("remove_clip track {} clip {}", args.track, args.clip_index),
        ) {
            Ok(id) => id,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        Ok(ToolResult::Ok(json!({
            "node_id": new_id.to_hex(),
            "summary": format!(
                "Removed clip {} from track {}; {} clip{} left",
                args.clip_index,
                args.track,
                remaining,
                if remaining == 1 { "" } else { "s" }
            )
        })))
    }
}
