//! `time_stretch` — change a track's duration without moving its pitch.
//!
//! The DSP is `audio_time::time_stretch`, a phase vocoder. This tool
//! applies it to the track's samples and writes the result the way every
//! other destructive tool does — a new content-addressed WAV and a new
//! session node, so undo still works.
//!
//! It used to record `time_stretch_factor` on each clip and leave the
//! audio alone, on the understanding that the render engine would honour
//! the factor later. The engine never learned to, so the tool reported
//! success for a change nobody could hear. That field is no longer
//! written: the audio *is* the state now, and writing both would risk a
//! double application if some future render path did start reading it.
//!
//! Composition falls out of that. Two consecutive stretches compose
//! because the second stretches audio the first already stretched — no
//! factor multiplication, and no way for a recorded number to drift out
//! of step with the samples.

use serde::Deserialize;
use serde_json::Value;

use crate::schema::anthropic_tool;

use crate::tool::ramp::{self, Ramp};
use crate::tool::util::{destructive_edit_then, parse_args, remap_after_scale, Moved};
use crate::util::range_resolver::resolve as resolve_range;
use crate::{Range, Tool, ToolContext, ToolResult};

#[derive(Debug, Deserialize)]
struct Args {
    track: usize,
    factor: f32,
    #[serde(default)]
    preserve_formants: bool,
    end_factor: Option<f32>,
    range: Option<Range>,
}

pub struct TimeStretchTool;

impl Tool for TimeStretchTool {
    fn name(&self) -> &'static str {
        "time_stretch"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "time_stretch",
            "Stretch or compress a track in time, keeping its pitch. factor=0.5 is 2x slower (twice as long), factor=2.0 is 2x faster (half as long). `preserve_formants` does nothing here (no frequency moves); it is accepted only to match `pitch_shift`. Phase vocoder: attacks are kept, dense material can sound slightly phasey, more so far from 1.0. Use `change_speed` when the pitch should move with the speed. `end_factor` ramps the factor linearly along the input from `factor` to it (both 0.1 to 10), for a tape-stop or a closing slow-down; `range` limits the edit to a span. What follows moves by the length gained or lost, on every track with sync-lock. Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "track": { "type": "integer" },
                    "factor": { "type": "number" },
                    "preserve_formants": { "type": "boolean" },
                    "end_factor": { "type": "number" },
                    "range": {
                        "type": "object",
                        "properties": {
                            "start_sec": { "type": "number" },
                            "end_sec": { "type": "number" }
                        }
                    }
                },
                "required": ["track", "factor"]
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

        let range = match resolve_range(args.range, "", false) {
            Ok(r) => r,
            Err(e) => return Ok(ToolResult::Error(e.to_string())),
        };
        // A ramp, or a span, is the sliding stretch. Bounds are checked
        // before any arithmetic on them.
        if args.end_factor.is_some() || range.is_some() {
            let bounds = audio_time::MIN_RAMP_FACTOR..=audio_time::MAX_RAMP_FACTOR;
            for (name, v) in [
                ("factor", Some(args.factor)),
                ("end_factor", args.end_factor),
            ] {
                if let Some(v) = v.filter(|v| !bounds.contains(v)) {
                    return Ok(ToolResult::Error(format!(
                        "invalid {name}: {v} (must be finite and within {} to {})",
                        audio_time::MIN_RAMP_FACTOR,
                        audio_time::MAX_RAMP_FACTOR
                    )));
                }
            }
            let end = args.end_factor.unwrap_or(args.factor);
            // The same value at both ends of the whole track is the plain
            // stretch below, byte for byte.
            if range.is_some() || end != args.factor {
                let span = match range {
                    Some(r) => format!(" {:.2}s\u{2013}{:.2}s", r.start_sec, r.end_sec),
                    None => String::new(),
                };
                let label = format!(
                    "time_stretch track {} factor {:.4} to {end:.4}{span} \
                     (preserve_formants={})",
                    args.track, args.factor, args.preserve_formants
                );
                return Ok(ramp::run(
                    ctx,
                    args.track,
                    range,
                    Ramp::Stretch {
                        start: args.factor,
                        end,
                    },
                    args.preserve_formants,
                    label,
                ));
            }
        }

        // Argument validation goes through the same path as the
        // eventual DSP call so the error surface is consistent.
        if !args.factor.is_finite() || args.factor <= 0.0 {
            return Ok(ToolResult::Error(format!(
                "invalid factor: {} (must be finite and > 0)",
                args.factor
            )));
        }

        let (factor, preserve_formants, track) = (args.factor, args.preserve_formants, args.track);

        // The closure is handed the channel count, which the vocoder
        // needs to process each channel separately. The layout is
        // unchanged, so the source count goes straight back.
        //
        // A DSP failure returns `Err`, which aborts the edit before any
        // node is appended (#277). It used to be recorded in a shared
        // cell and reported after the fact, by which point the session
        // had already gained a node whose audio matched its parent —
        // and, on a split track, had had its clips flattened into one.
        Ok(destructive_edit_then(
            ctx,
            track,
            move |samples, sample_rate, channels| {
                let out = audio_time::time_stretch(
                    samples,
                    sample_rate,
                    channels,
                    factor,
                    preserve_formants,
                )
                .map_err(|e| e.to_string())?;
                *samples = out;
                Ok((sample_rate, channels))
            },
            move |state, edited| {
                // A stretch re-times the whole recording: output
                // duration is input ÷ factor, so every mark and word
                // moves by the same ratio (#231). Nothing is dropped.
                //
                // Unconditional now: the hook only runs when the edit
                // succeeded.
                remap_after_scale(state, Moved::track(edited), 1.0 / factor as f64);
                serde_json::Map::default()
            },
            format!(
                "time_stretch track {track} factor {factor:.4} \
                 (preserve_formants={preserve_formants})"
            ),
        ))
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
