use serde::Deserialize;
use serde_json::{json, Value};
use crate::schema::anthropic_tool;
use crate::tool::util::{append_state, check_track_index, load_head_state};
use crate::{Tool, ToolContext, ToolResult};

pub(crate) fn clamp_pan(p: f32) -> f32 { p.clamp(-1.0, 1.0) }

#[derive(Debug, Deserialize)]
struct Args { track: usize, pan: f32 }

pub struct SetPanTool;

impl Tool for SetPanTool {
    fn name(&self) -> &'static str { "set_pan" }

    fn schema(&self) -> Value {
        anthropic_tool(
            "set_pan",
            "Set the stereo pan of a track. -1.0 = full left, 0.0 = centre, 1.0 = full right. Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
                    "pan": { "type": "number", "minimum": -1.0, "maximum": 1.0 }
                },
                "required": ["track", "pan"]
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
        let pan = clamp_pan(args.pan);
        state.tracks[args.track].pan = pan;
        let new_id = match append_state(ctx, state, format!("set_pan track {} -> {:.2}", args.track, pan)) {
            Ok(id) => id,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        Ok(ToolResult::Ok(json!({ "node_id": new_id.to_hex(), "pan": pan, "summary": format!("Set track {} pan to {:.2}", args.track, pan) })))
    }
}

#[cfg(test)]
mod tests {
    use super::clamp_pan;
    #[test]
    fn clamps_positive() { assert_eq!(clamp_pan(1.5), 1.0); }
    #[test]
    fn clamps_negative() { assert_eq!(clamp_pan(-2.0), -1.0); }
    #[test]
    fn passes_valid() { assert_eq!(clamp_pan(-0.5), -0.5); }
}
