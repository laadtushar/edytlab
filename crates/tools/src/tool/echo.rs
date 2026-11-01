use crate::schema::anthropic_tool;
use crate::tool::util::EffectEdit;
use crate::{Tool, ToolContext, ToolResult};
use serde::Deserialize;
use serde_json::Value;

pub(crate) use audio_dsp::effects::echo::apply_echo;

#[derive(Debug, Deserialize)]
struct Args {
    track: usize,
    delay_ms: f32,
    decay: Option<f32>,
}

pub struct EchoTool;

impl Tool for EchoTool {
    fn name(&self) -> &'static str {
        "echo"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "echo",
            "Add a single echo (delay + decay). delay_ms is the echo offset in milliseconds; decay (0..1) is the echo amplitude. Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
                    "delay_ms": { "type": "number", "description": "Echo delay in milliseconds" },
                    "decay": { "type": "number", "default": 0.5, "description": "Echo amplitude 0..1" },
                    "start_sec": { "type": "number" },
                    "end_sec": { "type": "number" }
                },
                "required": ["track", "delay_ms"]
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
        if args.delay_ms <= 0.0 {
            return Ok(ToolResult::Error("delay_ms must be positive".into()));
        }
        let decay = args.decay.unwrap_or(0.5).clamp(0.0, 1.0);
        let (delay, d) = (args.delay_ms, decay);
        Ok(EffectEdit::on(args.track).run(
            ctx,
            format!(
                "echo track {} delay={}ms decay={:.2}",
                args.track, args.delay_ms, decay
            ),
            move |samples, sr, channels| apply_echo(samples, sr, channels, delay, d),
        ))
    }
}
