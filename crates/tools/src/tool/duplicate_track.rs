use serde::Deserialize;
use serde_json::{json, Value};
use session::TrackId;
use crate::schema::anthropic_tool;
use crate::tool::util::{append_state, check_track_index, load_head_state};
use crate::{Tool, ToolContext, ToolResult};

pub(crate) fn validate_track_idx(idx: usize, len: usize) -> Result<(), String> {
    if idx >= len { Err(format!("track {idx} out of range (len={len})")) } else { Ok(()) }
}

#[derive(Debug, Deserialize)]
struct Args { track: usize }

pub struct DuplicateTrackTool;

impl Tool for DuplicateTrackTool {
    fn name(&self) -> &'static str { "duplicate_track" }

    fn schema(&self) -> Value {
        anthropic_tool("duplicate_track",
            "Create an exact copy of a track (same clips, gain, pan, effects). The duplicate is appended after all existing tracks. Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": { "track": { "type": "integer" } },
                "required": ["track"]
            }))
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a, Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };
        let mut state = match load_head_state(ctx) {
            Ok(s) => s, Err(e) => return Ok(ToolResult::Error(e)),
        };
        if let Err(e) = check_track_index(&state.tracks, args.track) {
            return Ok(ToolResult::Error(e));
        }
        let mut cloned = state.tracks[args.track].clone();
        cloned.id = TrackId::new();
        cloned.name = format!("{} (copy)", cloned.name);
        state.tracks.push(cloned);
        let new_id = match append_state(ctx, state, format!("duplicate_track {}", args.track)) {
            Ok(id) => id, Err(e) => return Ok(ToolResult::Error(e)),
        };
        Ok(ToolResult::Ok(json!({ "node_id": new_id.to_hex(), "summary": format!("Duplicated track {}", args.track) })))
    }
}

#[cfg(test)]
mod tests {
    use super::validate_track_idx;
    #[test]
    fn rejects_out_of_range() { assert!(validate_track_idx(5, 3).is_err()); }
    #[test]
    fn accepts_valid() { assert!(validate_track_idx(2, 5).is_ok()); }
}
