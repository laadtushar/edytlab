//! `set_track_gain` — set a track's absolute gain in dB (M21).
//!
//! Distinct from `gain` (which is additive). Two consecutive
//! `set_track_gain(t, -3)` calls leave the track at -3 dB, not -6 dB.
//! Useful when the agent has computed the exact gain it wants from a
//! `analyze_track` measurement and doesn't want compounding semantics.

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

pub struct SetTrackGainTool;

impl Tool for SetTrackGainTool {
    fn name(&self) -> &'static str {
        "set_track_gain"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "set_track_gain",
            "Set a track's gain in decibels (absolute, not additive). Replaces any prior gain on the track. Use `gain` for additive composition.",
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
        state.tracks[args.track].gain_db = args.db;

        let new_id = match append_state(
            ctx,
            state,
            format!("set_track_gain track {} = {:+} dB", args.track, args.db),
        ) {
            Ok(id) => id,
            Err(msg) => return Ok(ToolResult::Error(msg)),
        };

        Ok(ToolResult::Ok(json!({
            "node_id": new_id.to_hex(),
            "prior_gain_db": prior,
            "track_gain_db": args.db,
            "summary": format!(
                "Set track {} gain to {:+} dB (was {:+} dB); new head {}",
                args.track, args.db, prior, new_id.to_hex(),
            ),
        })))
    }
}
