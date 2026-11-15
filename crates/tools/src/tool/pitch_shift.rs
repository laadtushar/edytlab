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

use crate::schema::anthropic_tool;
use crate::tool::ramp::{self, Ramp};
use crate::tool::util::{destructive_edit_rechannel, parse_args};
use crate::util::range_resolver::resolve as resolve_range;
use crate::{Range, Tool, ToolContext, ToolResult};

#[derive(Debug, Deserialize)]
struct Args {
    track: usize,
    semitones: f32,
    #[serde(default)]
    preserve_formants: bool,
    end_semitones: Option<f32>,
    range: Option<Range>,
}

pub struct PitchShiftTool;

impl Tool for PitchShiftTool {
    fn name(&self) -> &'static str {
        "pitch_shift"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "pitch_shift",
            "Shift a track's pitch in semitones keeping its duration. +12 is one octave up, -12 one octave down; the range is +/-48. Set `preserve_formants` for voices: the vocal tract's resonances stay put while the harmonics move, so it sounds like the same person singing higher, not a chipmunk or giant. Leave it off for instruments. Most reliable within about a fifth. Phase vocoder: attacks are kept, dense material can sound slightly phasey. `end_semitones` ramps the shift linearly in semitones along the input from `semitones` to it, for a glide; `range` limits the edit to a span. Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "track": { "type": "integer" },
                    "semitones": { "type": "number" },
                    "preserve_formants": { "type": "boolean" },
                    "end_semitones": { "type": "number" },
                    "range": {
                        "type": "object",
                        "properties": {
                            "start_sec": { "type": "number" },
                            "end_sec": { "type": "number" }
                        }
                    }
                },
                "required": ["track", "semitones"]
            }),
        )
    }

    // A pure edit of the head's audio and one node on top of it: safe on a
    // staged store, and safe to run again on a newer head (#421).
    fn runs_off_the_lock(&self) -> bool {
        true
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match parse_args(args) {
            Ok(a) => a,
            Err(r) => return Ok(r),
        };

        if !args.semitones.is_finite() || args.semitones.abs() > MAX_SEMITONES {
            return Ok(ToolResult::Error(format!(
                "invalid semitones: {} (must be finite and within ±{MAX_SEMITONES})",
                args.semitones
            )));
        }

        if let Some(end) = args.end_semitones {
            if !end.is_finite() || end.abs() > MAX_SEMITONES {
                return Ok(ToolResult::Error(format!(
                    "invalid end_semitones: {end} (must be finite and within ±{MAX_SEMITONES})"
                )));
            }
        }
        let range = match resolve_range(args.range, "", false) {
            Ok(r) => r,
            Err(e) => return Ok(ToolResult::Error(e.to_string())),
        };
        let end = args.end_semitones.unwrap_or(args.semitones);
        // The same value at both ends of the whole track is the plain
        // shift below, byte for byte.
        if range.is_some() || end != args.semitones {
            let span = match range {
                Some(r) => format!(" {:.2}s\u{2013}{:.2}s", r.start_sec, r.end_sec),
                None => String::new(),
            };
            let label = format!(
                "pitch_shift track {} {:+} to {end:+} semitones{span} \
                 (preserve_formants={})",
                args.track, args.semitones, args.preserve_formants
            );
            return Ok(ramp::run(
                ctx,
                args.track,
                range,
                Ramp::Pitch {
                    start: args.semitones,
                    end,
                },
                args.preserve_formants,
                label,
            ));
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
