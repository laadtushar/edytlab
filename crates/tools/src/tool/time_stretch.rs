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

use crate::schema::{anthropic_tool, object_schema};
use std::cell::RefCell;
use std::rc::Rc;

use crate::tool::util::{destructive_edit_then, remap_after_scale};
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
            "Stretch or compress a track in time without changing its pitch. factor=0.5 is 2x slower (twice as long), factor=2.0 is 2x faster (half as long). `preserve_formants` does nothing here and is accepted only because the two tools share a shape: a time stretch moves no frequency, so there are no formants to hold in place. Use it on `pitch_shift`, which does move them. Quality is a phase vocoder's: sustained material is clean and attacks are preserved by onset-triggered phase resets, but dense material can sound slightly phasey and factors far from 1.0 make that worse. Use `change_speed` instead when the pitch should move with the speed. Appends a new session node.",
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

        let (factor, preserve_formants, track) = (args.factor, args.preserve_formants, args.track);

        // `destructive_edit_rechannel` hands the closure the channel
        // count, which the vocoder needs to process each channel
        // separately. The layout is unchanged, so the source count goes
        // straight back.
        // Shared rather than a plain local because both the edit
        // closure and the after-hook need it: the hook must not rescale
        // the labels for a stretch that did not happen.
        let failure: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
        let failure_edit = Rc::clone(&failure);
        let failure_hook = Rc::clone(&failure);
        let result = destructive_edit_then(
            ctx,
            track,
            move |samples, sample_rate, channels| {
                match audio_time::time_stretch(
                    samples,
                    sample_rate,
                    channels,
                    factor,
                    preserve_formants,
                ) {
                    Ok(out) => *samples = out,
                    // The buffer is left untouched, so the edit writes the
                    // audio back unchanged and the caller gets the reason.
                    Err(e) => *failure_edit.borrow_mut() = Some(e.to_string()),
                }
                (sample_rate, channels)
            },
            move |state, _| {
                // A stretch re-times the whole recording: output
                // duration is input ÷ factor, so every mark and word
                // moves by the same ratio (#231). Nothing is dropped.
                //
                // Skipped when the DSP failed: that path writes the
                // buffer back unchanged, so the audio did not move and
                // neither may the labels. The node is still appended
                // either way — see #277.
                if failure_hook.borrow().is_none() {
                    remap_after_scale(state, 1.0 / factor as f64);
                }
                Default::default()
            },
            format!(
                "time_stretch track {track} factor {factor:.4} \
                 (preserve_formants={preserve_formants})"
            ),
        );

        let failed = failure.borrow().clone();
        if let Some(msg) = failed {
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
