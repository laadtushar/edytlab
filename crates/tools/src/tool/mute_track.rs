use serde::Deserialize;
use serde_json::{json, Value};
use crate::schema::anthropic_tool;
use crate::tool::util::{append_state, check_track_index, load_head_state};
use crate::{Tool, ToolContext, ToolResult};

#[derive(Debug, Deserialize)]
struct Args { track: usize, muted: bool }

pub struct MuteTrackTool;

impl Tool for MuteTrackTool {
    fn name(&self) -> &'static str { "mute_track" }

    fn schema(&self) -> Value {
        anthropic_tool("mute_track",
            "Mute or unmute a track. Muted tracks produce silence in the mix. Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
                    "muted": { "type": "boolean" }
                },
                "required": ["track", "muted"]
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
        state.tracks[args.track].muted = args.muted;
        let new_id = match append_state(ctx, state, format!("mute_track {} -> {}", args.track, args.muted)) {
            Ok(id) => id, Err(e) => return Ok(ToolResult::Error(e)),
        };
        Ok(ToolResult::Ok(json!({ "node_id": new_id.to_hex(), "summary": format!("Track {} muted={}", args.track, args.muted) })))
    }
}

#[cfg(test)]
mod tests {
    use super::MuteTrackTool;
    use crate::Tool;
    #[test]
    fn tool_name_is_mute_track() {
        assert_eq!(MuteTrackTool.name(), "mute_track");
    }
}
