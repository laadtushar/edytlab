use crate::schema::anthropic_tool;
use crate::tool::util::{append_state, args_and_head, check_track_index};
use crate::{Tool, ToolContext, ToolResult};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Debug, Deserialize)]
struct Args {
    track: usize,
    solo: bool,
}

pub struct SoloTrackTool;

impl Tool for SoloTrackTool {
    fn name(&self) -> &'static str {
        "solo_track"
    }

    fn schema(&self) -> Value {
        anthropic_tool("solo_track",
            "Solo or un-solo a track. When any track is soloed, only soloed tracks play in the mix. Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
                    "solo": { "type": "boolean" }
                },
                "required": ["track", "solo"]
            }))
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let (args, mut state) = match args_and_head::<Args>(args, ctx) {
            Ok(v) => v,
            Err(r) => return Ok(r),
        };
        if let Err(e) = check_track_index(&state.tracks, args.track) {
            return Ok(ToolResult::Error(e));
        }
        state.tracks[args.track].soloed = args.solo;
        let new_id = match append_state(
            ctx,
            state,
            format!("solo_track {} -> {}", args.track, args.solo),
        ) {
            Ok(id) => id,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        Ok(ToolResult::Ok(
            json!({ "node_id": new_id.to_hex(), "summary": format!("Track {} solo={}", args.track, args.solo) }),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::SoloTrackTool;
    use crate::Tool;
    #[test]
    fn tool_name_is_solo_track() {
        assert_eq!(SoloTrackTool.name(), "solo_track");
    }
}
