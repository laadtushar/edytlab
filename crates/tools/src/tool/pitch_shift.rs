//! `pitch_shift` — move a track's pitch without changing its duration.
//!
//! The DSP is `audio_time::pitch_shift`: the phase vocoder stretches the
//! timeline by the pitch ratio and the result is read back that much
//! faster, so the duration returns to where it started and every
//! frequency is multiplied. That composition is what separates this from
//! `change_speed`, which can raise pitch only by shortening the audio.
//!
//! Like `time_stretch`, this used to record a number on each clip and
//! leave the samples alone, waiting on a render engine that never read
//! it. The audio is the state now; the field is not written.

use serde::Deserialize;
use serde_json::Value;

use audio_time::shift::MAX_SEMITONES;

use crate::schema::{anthropic_tool, object_schema};
use crate::tool::util::destructive_edit_rechannel;
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
            "Shift a track's pitch in semitones without changing its duration. +12 is one octave up, -12 one octave down; the range is +/-48. `preserve_formants` is accepted but not yet honoured, so a large shift on a voice sounds like the classic chipmunk or giant rather than the same person singing higher. Quality is a phase vocoder's: sustained material is clean and attacks are preserved by onset-triggered phase resets, though dense material can sound slightly phasey. Appends a new session node.",
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

        let (semitones, preserve_formants, track) =
            (args.semitones, args.preserve_formants, args.track);

        let mut failure: Option<String> = None;
        let result = destructive_edit_rechannel(
            ctx,
            track,
            |samples, sample_rate, channels| {
                match audio_time::pitch_shift(
                    samples,
                    sample_rate,
                    channels,
                    semitones,
                    preserve_formants,
                ) {
                    Ok(out) => *samples = out,
                    Err(e) => failure = Some(e.to_string()),
                }
                channels
            },
            format!(
                "pitch_shift track {track} {semitones:+} semitones \
                 (preserve_formants={preserve_formants})"
            ),
        );

        if let Some(msg) = failure {
            return Ok(ToolResult::Error(msg));
        }
        Ok(result)
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
