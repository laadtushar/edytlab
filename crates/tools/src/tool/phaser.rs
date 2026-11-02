use serde::Deserialize;
use serde_json::Value;

use crate::schema::anthropic_tool;
use crate::tool::util::{parse_args, EffectEdit};
use crate::{Tool, ToolContext, ToolResult};

pub(crate) use audio_dsp::effects::phaser::apply_phaser;

#[derive(Debug, Deserialize)]
struct Args {
    track: usize,
    rate_hz: Option<f32>,
    depth: Option<f32>,
    stages: Option<u32>,
}

pub struct PhaserTool;

impl Tool for PhaserTool {
    fn name(&self) -> &'static str {
        "phaser"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "phaser",
            "Apply a phaser effect using an all-pass filter chain with LFO sweep. rate_hz controls LFO speed; depth is the wet blend; stages sets the filter chain length (2-12). Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
                    "rate_hz": { "type": "number", "default": 0.5 },
                    "depth": { "type": "number", "default": 0.5 },
                    "stages": { "type": "integer", "default": 4 }
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
        let args: Args = match parse_args(args) {
            Ok(a) => a,
            Err(r) => return Ok(r),
        };
        let rate = args.rate_hz.unwrap_or(0.5).max(0.01);
        let depth = args.depth.unwrap_or(0.5).clamp(0.0, 1.0);
        let stages = args.stages.unwrap_or(4).clamp(2, 12);
        let (r, d, st) = (rate, depth, stages);
        Ok(EffectEdit::on(args.track).run(
            ctx,
            format!(
                "phaser track {} rate={:.2}Hz depth={:.2} stages={}",
                args.track, rate, depth, stages
            ),
            move |samples, sr, channels| apply_phaser(samples, sr, channels, r, d, st),
        ))
    }
}
