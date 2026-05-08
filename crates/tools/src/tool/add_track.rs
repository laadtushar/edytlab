//! `add_track` — append an empty track to the session (M21).
//!
//! Useful as a session-shaping primitive when the agent wants to set up
//! routing or gain on a track before deciding which file to load into it.
//! The new track has no clips; it contributes silence at render time
//! until populated.

use serde::Deserialize;
use serde_json::{json, Value};
use session::{Track, TrackId};

use crate::schema::anthropic_tool;
use crate::tool::util::{append_state, load_head_state};
use crate::{Tool, ToolContext, ToolResult};

#[derive(Debug, Deserialize)]
struct Args {
    #[serde(default)]
    name: Option<String>,
}

pub struct AddTrackTool;

impl Tool for AddTrackTool {
    fn name(&self) -> &'static str {
        "add_track"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "add_track",
            "Append a new empty track to the current session. Returns the new session node id and the new track's index. The track contributes silence until a clip is loaded onto it.",
            json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string" },
                },
                "required": [],
                "additionalProperties": false,
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
            Err(msg) => return Ok(ToolResult::Error(msg)),
        };

        let new_index = state.tracks.len();
        let name = args.name.unwrap_or_else(|| format!("track {new_index}"));
        state.tracks.push(Track {
            id: TrackId::new(),
            name: name.clone(),
            clips: Vec::new(),
            gain_db: 0.0,
            pan: 0.0,
            muted: false,
            soloed: false,
            effects: Vec::new(),
        });

        let new_id = match append_state(ctx, state, format!("add_track {name}")) {
            Ok(id) => id,
            Err(msg) => return Ok(ToolResult::Error(msg)),
        };

        Ok(ToolResult::Ok(json!({
            "node_id": new_id.to_hex(),
            "track_index": new_index,
            "summary": format!(
                "Added empty track {new_index} ({name}); new head {}",
                new_id.to_hex(),
            ),
        })))
    }
}
