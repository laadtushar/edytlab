//! `gain` — apply a constant gain (in dB) to a track.
//!
//! Implementation note: `Track::gain_db` is summed into any prior gain
//! so two consecutive `gain(t, +3.0)` calls yield a track with
//! `gain_db == +6.0`. This is what the property test on gain
//! composition exercises.

use serde::Deserialize;
use serde_json::{json, Value};

use crate::schema::{anthropic_tool, object_schema};
use crate::tool::util::{append_state, check_track_index, load_head_state};
use crate::{Tool, ToolContext, ToolResult};

#[derive(Debug, Deserialize)]
struct Args {
    track: usize,
    db: f32,
}

pub struct GainTool;

impl Tool for GainTool {
    fn name(&self) -> &'static str {
        "gain"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "gain",
            "Apply a constant gain in decibels to a track. Composes additively with any existing track gain. Appends a new session node parented to the current head.",
            object_schema(&[("track", "integer", true), ("db", "number", true)]),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };

        if !args.db.is_finite() {
            return Ok(ToolResult::Error(format!(
                "gain db must be finite; got {}",
                args.db
            )));
        }

        let mut state = match load_head_state(ctx) {
            Ok(s) => s,
            Err(msg) => return Ok(ToolResult::Error(msg)),
        };

        if let Err(msg) = check_track_index(&state.tracks, args.track) {
            return Ok(ToolResult::Error(msg));
        }

        let prior = state.tracks[args.track].gain_db;
        let new_gain = prior + args.db;
        state.tracks[args.track].gain_db = new_gain;

        let new_id = match append_state(
            ctx,
            state,
            format!("gain track {} {:+} dB", args.track, args.db),
        ) {
            Ok(id) => id,
            Err(msg) => return Ok(ToolResult::Error(msg)),
        };

        Ok(ToolResult::Ok(json!({
            "node_id": new_id.to_hex(),
            "track_gain_db": new_gain,
            "summary": format!(
                "Applied {:+} dB gain to track {} (now {:+} dB); new head {}",
                args.db, args.track, new_gain, new_id.to_hex(),
            ),
        })))
    }
}
