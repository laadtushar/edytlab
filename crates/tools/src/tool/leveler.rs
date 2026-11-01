use crate::schema::anthropic_tool;
use crate::tool::util::EffectEdit;
use crate::{Tool, ToolContext, ToolResult};
use serde::Deserialize;
use serde_json::Value;

pub(crate) use audio_dsp::effects::leveler::apply_leveler;

#[derive(Debug, Deserialize)]
struct Args {
    track: usize,
    target_db: f32,
    start_sec: Option<f64>,
    end_sec: Option<f64>,
}

pub struct LevelerTool;

impl Tool for LevelerTool {
    fn name(&self) -> &'static str {
        "leveler"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "leveler",
            "Apply dynamic leveling: normalise each short window to a target RMS level. Reduces variation between loud and quiet passages. Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
                    "target_db": { "type": "number", "description": "Target RMS level in dBFS (e.g. -18)" },
                    "start_sec": { "type": "number" },
                    "end_sec": { "type": "number" }
                },
                "required": ["track", "target_db"]
            }),
        )
    }

    // A pure edit of the head's audio and one node on top of it: safe on a
    // staged store, and safe to run again on a newer head (#421).
    fn runs_off_the_lock(&self) -> bool {
        true
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };

        let target = args.target_db;
        Ok(EffectEdit::on(args.track).run_in_window(
            ctx,
            args.start_sec,
            args.end_sec,
            format!("leveler track {} target={}dB", args.track, target),
            move |window, sr, channels| apply_leveler(window, sr, channels, target, 100),
        ))
    }
}
