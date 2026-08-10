//! `pitch_shift` — record a per-clip pitch shift in semitones.
//!
//! Phase 2 (M20) metadata-only tool, mirroring `time_stretch`. Argument
//! validation uses the same ±48-semitone bound as
//! `audio_time::pitch_shift`. Composition is **additive** in semitones:
//! a `+12` followed by `-12` returns to the source pitch. The audio
//! engine applies the stored value at render time in M22+.

use serde::Deserialize;
use serde_json::{json, Value};

use audio_time::shift::MAX_SEMITONES;

use crate::schema::{anthropic_tool, object_schema};
use crate::tool::util::{append_state, check_track_index, load_head_state};
use crate::{Tool, ToolContext, ToolResult};

#[derive(Debug, Deserialize)]
struct Args {
    track: usize,
    semitones: f32,
    #[serde(default)]
    preserve_formants: bool,
}

pub struct PitchShiftTool;

impl Tool for PitchShiftTool {
    fn name(&self) -> &'static str {
        "pitch_shift"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "pitch_shift",
            "Record a pitch shift in semitones on every clip of a track (+12 is one octave up, -12 one octave down), with an optional request for vocal-formant preservation. NOT YET APPLIED AT RENDER: the value is recorded on the session and survives save/load, but the audio engine does not read it, so the rendered output is unchanged. Do not tell the user the audio has changed, and do not plan a duration or pitch around it. No tool shifts pitch audibly today; say so rather than reporting success.",
            object_schema(&[
                ("track", "integer", true),
                ("semitones", "number", true),
                ("preserve_formants", "boolean", false),
            ]),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };

        if !args.semitones.is_finite() || args.semitones.abs() > MAX_SEMITONES {
            return Ok(ToolResult::Error(format!(
                "invalid semitones: {} (must be finite and within ±{MAX_SEMITONES})",
                args.semitones
            )));
        }

        let mut state = match load_head_state(ctx) {
            Ok(s) => s,
            Err(msg) => return Ok(ToolResult::Error(msg)),
        };

        if let Err(msg) = check_track_index(&state.tracks, args.track) {
            return Ok(ToolResult::Error(msg));
        }

        // Compose additively: +12 then -12 cancels.
        for clip in &mut state.tracks[args.track].clips {
            let prior = clip.pitch_shift_semitones.unwrap_or(0.0);
            let composed = prior + args.semitones;
            // Re-validate the composed value so a long chain that
            // wanders out of the supported range is caught here rather
            // than at render time.
            if composed.abs() > MAX_SEMITONES {
                return Ok(ToolResult::Error(format!(
                    "composed pitch shift {composed} exceeds ±{MAX_SEMITONES}",
                )));
            }
            clip.pitch_shift_semitones = if composed == 0.0 {
                None
            } else {
                Some(composed)
            };
        }

        let new_id = match append_state(
            ctx,
            state,
            format!(
                "pitch_shift track {} {:+} semitones (preserve_formants={})",
                args.track, args.semitones, args.preserve_formants
            ),
        ) {
            Ok(id) => id,
            Err(msg) => return Ok(ToolResult::Error(msg)),
        };

        Ok(ToolResult::Ok(json!({
            "node_id": new_id.to_hex(),
            "semitones": args.semitones,
            "preserve_formants": args.preserve_formants,
            "applied_at_render": false,
            "summary": format!(
                "Recorded a {:+} semitone pitch shift on track {}. The render does not apply it \
                 yet, so the audio is unchanged. New head {}",
                args.semitones, args.track, new_id.to_hex(),
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
        let s = PitchShiftTool.schema();
        assert_eq!(s["name"], "pitch_shift");
        let required = s["input_schema"]["required"].as_array().unwrap();
        let names: Vec<&str> = required.iter().map(|v| v.as_str().unwrap()).collect();
        assert!(names.contains(&"track"));
        assert!(names.contains(&"semitones"));
    }

    #[test]
    fn dispatcher_registers_pitch_shift() {
        let d = ToolDispatcher::default_dispatcher();
        assert!(d.get("pitch_shift").is_some());
    }
}
