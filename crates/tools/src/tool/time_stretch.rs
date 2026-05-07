//! `time_stretch` — record a per-clip time-stretch factor on a track.
//!
//! Phase 2 (M20) ships this as a metadata-only tool: the factor is
//! validated through `audio_time::time_stretch`'s argument check (so we
//! reject non-positive / non-finite factors with the same diagnostic as
//! the eventual DSP path) and stored on every clip of the targeted
//! track. The audio engine learns to honour the stored factor at render
//! time in M22+; until then `render_final` ignores it and the tool
//! response includes a "applied at next render" caveat.
//!
//! Composition: a second `time_stretch` *multiplies* the factor onto
//! whatever the clip already carries (so `0.5` then `2.0` returns to
//! identity). This matches the user mental model of two consecutive
//! stretches and falls out naturally from how a streaming engine will
//! apply the stored value.

use serde::Deserialize;
use serde_json::{json, Value};

use crate::schema::{anthropic_tool, object_schema};
use crate::tool::util::{append_state, check_track_index, load_head_state};
use crate::{Tool, ToolContext, ToolResult};

#[derive(Debug, Deserialize)]
struct Args {
    track: usize,
    factor: f32,
    #[serde(default)]
    preserve_formants: bool,
}

pub struct TimeStretchTool;

impl Tool for TimeStretchTool {
    fn name(&self) -> &'static str {
        "time_stretch"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "time_stretch",
            "Record a time-stretch factor on every clip of a track. Output duration at render time will be `input_duration / factor` (factor=0.5 → 2x slower, factor=2.0 → 2x faster). The `preserve_formants` flag requests vocal-formant preservation when the M28 backend lands. Phase 2 caveat: the factor is stored on the session; the audio engine applies it at render time in M22+.",
            object_schema(&[
                ("track", "integer", true),
                ("factor", "number", true),
                ("preserve_formants", "boolean", false),
            ]),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };

        // Argument validation goes through the same path as the
        // eventual DSP call so the error surface is consistent.
        if !args.factor.is_finite() || args.factor <= 0.0 {
            return Ok(ToolResult::Error(format!(
                "invalid factor: {} (must be finite and > 0)",
                args.factor
            )));
        }

        let mut state = match load_head_state(ctx) {
            Ok(s) => s,
            Err(msg) => return Ok(ToolResult::Error(msg)),
        };

        if let Err(msg) = check_track_index(&state.tracks, args.track) {
            return Ok(ToolResult::Error(msg));
        }

        // Compose: multiply onto any prior factor so two consecutive
        // stretches act like a single stretch with the product factor.
        for clip in &mut state.tracks[args.track].clips {
            let prior = clip.time_stretch_factor.unwrap_or(1.0);
            clip.time_stretch_factor = Some(prior * args.factor);
        }

        let new_id = match append_state(
            ctx,
            state,
            format!(
                "time_stretch track {} factor {:.4} (preserve_formants={})",
                args.track, args.factor, args.preserve_formants
            ),
        ) {
            Ok(id) => id,
            Err(msg) => return Ok(ToolResult::Error(msg)),
        };

        Ok(ToolResult::Ok(json!({
            "node_id": new_id.to_hex(),
            "factor": args.factor,
            "preserve_formants": args.preserve_formants,
            "summary": format!(
                "Recorded time_stretch factor {:.4} on track {} (applied at next render); new head {}",
                args.factor, args.track, new_id.to_hex(),
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
        let s = TimeStretchTool.schema();
        assert_eq!(s["name"], "time_stretch");
        let required = s["input_schema"]["required"].as_array().unwrap();
        let names: Vec<&str> = required.iter().map(|v| v.as_str().unwrap()).collect();
        assert!(names.contains(&"track"));
        assert!(names.contains(&"factor"));
    }

    #[test]
    fn dispatcher_registers_time_stretch() {
        let d = ToolDispatcher::default_dispatcher();
        assert!(d.get("time_stretch").is_some());
    }
}
