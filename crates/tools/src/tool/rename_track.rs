use serde::Deserialize;
use serde_json::{json, Value};
use crate::schema::anthropic_tool;
use crate::tool::util::{append_state, check_track_index, load_head_state};
use crate::{Tool, ToolContext, ToolResult};

pub(crate) fn validate_name(n: &str) -> Result<(), String> {
    if n.trim().is_empty() { Err("name must not be empty".into()) } else { Ok(()) }
}

#[derive(Debug, Deserialize)]
struct Args { track: usize, name: String }

pub struct RenameTrackTool;

impl Tool for RenameTrackTool {
    fn name(&self) -> &'static str { "rename_track" }

    fn schema(&self) -> Value {
        anthropic_tool(
            "rename_track",
            "Rename a track. Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
                    "name": { "type": "string", "minLength": 1 }
                },
                "required": ["track", "name"]
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };
        if let Err(e) = validate_name(&args.name) {
            return Ok(ToolResult::Error(e));
        }
        let mut state = match load_head_state(ctx) {
            Ok(s) => s,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        if let Err(e) = check_track_index(&state.tracks, args.track) {
            return Ok(ToolResult::Error(e));
        }
        state.tracks[args.track].name = args.name.clone();
        let new_id = match append_state(ctx, state, format!("rename_track {} -> {}", args.track, args.name)) {
            Ok(id) => id,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        Ok(ToolResult::Ok(json!({ "node_id": new_id.to_hex(), "summary": format!("Renamed track {} to {:?}", args.track, args.name) })))
    }
}

#[cfg(test)]
mod tests {
    use super::validate_name;
    #[test]
    fn rejects_empty() { assert!(validate_name("").is_err()); }
    #[test]
    fn accepts_valid() { assert!(validate_name("Vocals").is_ok()); }
}
