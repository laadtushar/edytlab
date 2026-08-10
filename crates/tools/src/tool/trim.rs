//! `trim` — keep only `[start_sample, end_sample)` of a track.
//!
//! Mirrors `cut_range` but keeps the slice instead of removing it. The
//! window is a range on the track's *timeline*, so every clip reaching
//! into it contributes its overlapping part, re-based to zero.

use serde::Deserialize;
use serde_json::{json, Value};

use crate::schema::{anthropic_tool, object_schema};
use crate::tool::util::{
    append_state, check_sample_range, check_track_index, keep_timeline, load_head_state,
    timeline_end,
};
use crate::{Tool, ToolContext, ToolResult};

#[derive(Debug, Deserialize)]
struct Args {
    track: usize,
    start_sample: u64,
    end_sample: u64,
}

pub struct TrimTool;

impl Tool for TrimTool {
    fn name(&self) -> &'static str {
        "trim"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "trim",
            "Keep only the half-open sample range [start_sample, end_sample) of a track and discard the rest. Appends a new session node parented to the current head.",
            object_schema(&[
                ("track", "integer", true),
                ("start_sample", "integer", true),
                ("end_sample", "integer", true),
            ]),
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

        if let Err(msg) = check_track_index(&state.tracks, args.track) {
            return Ok(ToolResult::Error(msg));
        }

        let track = &mut state.tracks[args.track];
        // Measured across the whole timeline, not the longest single clip
        // — those differ as soon as a cut has split the track.
        let track_len = timeline_end(&track.clips);
        let (start, end) = match check_sample_range(args.start_sample, args.end_sample, track_len) {
            Ok(p) => p,
            Err(msg) => return Ok(ToolResult::Error(msg)),
        };

        if track.clips.is_empty() {
            return Ok(ToolResult::Error(
                "track has no clips; nothing to trim".into(),
            ));
        }

        let new_length = end - start;
        // Keeping the window means keeping it from every clip that reaches
        // into it. Trimming `clips.first()` alone and assigning the result
        // over `track.clips` dropped the rest of a split track, and worse,
        // read the kept span straight out of the first clip's source — so
        // a window spanning an earlier cut's join brought the cut-out
        // audio back.
        track.clips = keep_timeline(&track.clips, start, end);
        state.length_samples = state
            .tracks
            .iter()
            .map(|t| timeline_end(&t.clips))
            .max()
            .unwrap_or(0);

        let new_id = match append_state(
            ctx,
            state,
            format!(
                "trim track {} [{}..{})",
                args.track, args.start_sample, args.end_sample
            ),
        ) {
            Ok(id) => id,
            Err(msg) => return Ok(ToolResult::Error(msg)),
        };

        Ok(ToolResult::Ok(json!({
            "node_id": new_id.to_hex(),
            "kept_samples": new_length,
            "summary": format!(
                "Trimmed track {} to [{}, {}) ({} samples); new head {}",
                args.track, args.start_sample, args.end_sample, new_length, new_id.to_hex(),
            ),
        })))
    }
}
