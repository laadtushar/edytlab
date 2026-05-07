//! `trim` — keep only `[start_sample, end_sample)` of a track.
//!
//! Mirrors `cut_range` but keeps the slice instead of removing it. The
//! resulting track has a single clip whose `source_offset` advances by
//! `start_sample` and whose `length` is `end - start`.

use serde::Deserialize;
use serde_json::{json, Value};
use session::Clip;

use crate::schema::{anthropic_tool, object_schema};
use crate::tool::util::{append_state, check_sample_range, check_track_index, load_head_state};
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
        let track_len = track.clips.iter().map(|c| c.length).max().unwrap_or(0);
        let (start, end) = match check_sample_range(args.start_sample, args.end_sample, track_len) {
            Ok(p) => p,
            Err(msg) => return Ok(ToolResult::Error(msg)),
        };

        let Some(clip) = track.clips.first().cloned() else {
            return Ok(ToolResult::Error(
                "track has no clips; nothing to trim".into(),
            ));
        };

        let new_length = end - start;
        let trimmed = Clip {
            source_path: clip.source_path.clone(),
            start_in_track: 0,
            source_offset: clip.source_offset + start,
            length: new_length,
            content_hash: clip.content_hash,
        };

        track.clips = vec![trimmed];
        state.length_samples = new_length;

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
