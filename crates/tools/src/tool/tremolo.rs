use serde::Deserialize;
use serde_json::Value;

use crate::schema::anthropic_tool;
use crate::tool::util::EffectEdit;
use crate::{Tool, ToolContext, ToolResult};

pub(crate) use audio_dsp::effects::tremolo::apply_tremolo;

#[derive(Debug, Deserialize)]
struct Args {
    track: usize,
    rate_hz: Option<f32>,
    depth: Option<f32>,
}

pub struct TremoloTool;

impl Tool for TremoloTool {
    fn name(&self) -> &'static str {
        "tremolo"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "tremolo",
            "Apply tremolo (LFO amplitude modulation). rate_hz controls oscillation speed; depth (0..1) controls modulation depth. Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
                    "rate_hz": { "type": "number", "default": 4.0, "description": "LFO rate in Hz" },
                    "depth": { "type": "number", "default": 0.5, "description": "Modulation depth 0..1" }
                },
                "required": ["track"]
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
        let rate = args.rate_hz.unwrap_or(4.0).max(0.1);
        let depth = args.depth.unwrap_or(0.5).clamp(0.0, 1.0);
        let (r, d) = (rate, depth);
        Ok(EffectEdit::on(args.track).run(
            ctx,
            format!(
                "tremolo track {} rate={:.1}Hz depth={:.2}",
                args.track, rate, depth
            ),
            move |samples, sr, channels| apply_tremolo(samples, sr, channels, r, d),
        ))
    }
}
