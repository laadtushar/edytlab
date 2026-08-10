//! `align_to_beat` — record a target beat grid on a track.
//!
//! Phase 2 (M20) metadata-only tool. The grid is a sequence of target
//! beat times in seconds (relative to clip start). At render time the
//! audio engine resamples each beat-bound chunk so that the source
//! beats land on the target grid (M22+). Phase 2 stores the grid on
//! every clip of the targeted track and surfaces a "applied at next
//! render" message.

use serde::Deserialize;
use serde_json::{json, Value};

use crate::schema::anthropic_tool;
use crate::tool::util::{append_state, check_track_index, load_head_state};
use crate::{Tool, ToolContext, ToolResult};

#[derive(Debug, Deserialize)]
struct Args {
    track: usize,
    beat_grid: Vec<f32>,
}

pub struct AlignToBeatTool;

impl Tool for AlignToBeatTool {
    fn name(&self) -> &'static str {
        "align_to_beat"
    }

    fn schema(&self) -> Value {
        // Hand-rolled schema: `object_schema` doesn't compose array
        // item types. `beat_grid` items are required to be numbers.
        let input_schema = json!({
            "type": "object",
            "properties": {
                "track": { "type": "integer" },
                "beat_grid": {
                    "type": "array",
                    "items": { "type": "number" },
                    "minItems": 1,
                },
            },
            "required": ["track", "beat_grid"],
            "additionalProperties": false,
        });
        anthropic_tool(
            "align_to_beat",
            "Record a target beat grid (an array of beat times in seconds, relative to clip start) on every clip of a track. NOT YET APPLIED AT RENDER: the value is recorded on the session and survives save/load, but the audio engine does not read it, so the rendered output is unchanged. Do not tell the user the audio has changed, and do not plan a duration or pitch around it. Nothing warps audio to a grid today; say so rather than reporting success.",
            input_schema,
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };

        if args.beat_grid.is_empty() {
            return Ok(ToolResult::Error(
                "beat_grid must contain at least one beat time".into(),
            ));
        }
        for (i, &t) in args.beat_grid.iter().enumerate() {
            if !t.is_finite() || t < 0.0 {
                return Ok(ToolResult::Error(format!(
                    "beat_grid[{i}] = {t} must be finite and non-negative",
                )));
            }
        }
        // Strictly increasing — duplicate beats would zero-divide a
        // resampler at render time.
        for i in 1..args.beat_grid.len() {
            if args.beat_grid[i] <= args.beat_grid[i - 1] {
                return Ok(ToolResult::Error(format!(
                    "beat_grid must be strictly increasing; got {} after {} at index {i}",
                    args.beat_grid[i],
                    args.beat_grid[i - 1],
                )));
            }
        }

        let mut state = match load_head_state(ctx) {
            Ok(s) => s,
            Err(msg) => return Ok(ToolResult::Error(msg)),
        };

        if let Err(msg) = check_track_index(&state.tracks, args.track) {
            return Ok(ToolResult::Error(msg));
        }

        let beats_count = args.beat_grid.len();
        for clip in &mut state.tracks[args.track].clips {
            clip.beat_grid = Some(args.beat_grid.clone());
        }

        let new_id = match append_state(
            ctx,
            state,
            format!("align_to_beat track {} ({beats_count} beats)", args.track),
        ) {
            Ok(id) => id,
            Err(msg) => return Ok(ToolResult::Error(msg)),
        };

        Ok(ToolResult::Ok(json!({
            "node_id": new_id.to_hex(),
            "beats": beats_count,
            "applied_at_render": false,
            "summary": format!(
                "Recorded a beat grid ({beats_count} beats) on track {}. The render does not warp \
                 audio to it yet, so the audio is unchanged. New head {}",
                args.track, new_id.to_hex(),
            ),
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dispatcher::ToolDispatcher;

    #[test]
    fn schema_advertises_required_args() {
        let s = AlignToBeatTool.schema();
        assert_eq!(s["name"], "align_to_beat");
        let required = s["input_schema"]["required"].as_array().unwrap();
        let names: Vec<&str> = required.iter().map(|v| v.as_str().unwrap()).collect();
        assert!(names.contains(&"track"));
        assert!(names.contains(&"beat_grid"));
    }

    #[test]
    fn dispatcher_registers_align_to_beat() {
        let d = ToolDispatcher::default_dispatcher();
        assert!(d.get("align_to_beat").is_some());
    }
}
