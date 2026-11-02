use crate::schema::anthropic_tool;
use crate::tool::util::{parse_args, EffectEdit};
use crate::{Tool, ToolContext, ToolResult};
use serde::Deserialize;
use serde_json::Value;

pub(crate) use audio_dsp::effects::high_pass_filter::apply_high_pass;

#[derive(Debug, Deserialize)]
struct Args {
    track: usize,
    cutoff_hz: f32,
    start_sec: Option<f64>,
    end_sec: Option<f64>,
}

pub struct HighPassFilterTool;

impl Tool for HighPassFilterTool {
    fn name(&self) -> &'static str {
        "high_pass_filter"
    }

    fn schema(&self) -> Value {
        anthropic_tool("high_pass_filter",
            "Apply a Butterworth high-pass filter to a track, removing frequencies below cutoff_hz. Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
                    "cutoff_hz": { "type": "number", "description": "Cutoff frequency in Hz" },
                    "start_sec": { "type": "number" },
                    "end_sec": { "type": "number" }
                },
                "required": ["track", "cutoff_hz"]
            }))
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
        if args.cutoff_hz <= 0.0 {
            return Ok(ToolResult::Error("cutoff_hz must be positive".into()));
        }
        let (cutoff, s, e) = (args.cutoff_hz, args.start_sec, args.end_sec);
        Ok(EffectEdit::on(args.track).run(
            ctx,
            format!(
                "high_pass_filter track {} cutoff={:.0}Hz",
                args.track, cutoff
            ),
            move |samples, sr, channels| apply_high_pass(samples, sr, channels, cutoff, s, e),
        ))
    }
}
