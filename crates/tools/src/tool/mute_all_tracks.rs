use crate::schema::anthropic_tool;
use crate::tool::util::{append_state, args_and_head};
use crate::{Tool, ToolContext, ToolResult};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Debug, Deserialize)]
struct Args {
    muted: bool,
}

pub struct MuteAllTracksTool;

impl Tool for MuteAllTracksTool {
    fn name(&self) -> &'static str {
        "mute_all_tracks"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "mute_all_tracks",
            "Mute or unmute every track at once. Solo is untouched. New node.",
            serde_json::json!({
                "type": "object",
                "properties": { "muted": { "type": "boolean" } },
                "required": ["muted"]
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let (args, mut state) = match args_and_head::<Args>(args, ctx) {
            Ok(v) => v,
            Err(r) => return Ok(r),
        };
        if state.tracks.is_empty() {
            return Ok(ToolResult::Ok(
                json!({ "summary": "The session has no tracks; nothing to change" }),
            ));
        }
        let total = state.tracks.len();
        let mut changed = 0usize;
        for track in &mut state.tracks {
            if track.muted != args.muted {
                track.muted = args.muted;
                changed += 1;
            }
        }
        let word = if args.muted { "muted" } else { "unmuted" };
        if changed == 0 {
            return Ok(ToolResult::Ok(json!({
                "summary": format!("All {total} tracks are already {word}; nothing to change"),
            })));
        }
        let new_id = match append_state(ctx, state, format!("mute_all_tracks -> {}", args.muted)) {
            Ok(id) => id,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        Ok(ToolResult::Ok(json!({
            "node_id": new_id.to_hex(),
            "summary": format!("{word} {changed} of {total} tracks"),
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::MuteAllTracksTool;
    use crate::Tool;
    #[test]
    fn tool_name_is_mute_all_tracks() {
        assert_eq!(MuteAllTracksTool.name(), "mute_all_tracks");
    }
}
