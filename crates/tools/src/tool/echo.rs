use crate::schema::anthropic_tool;
use crate::tool::util::EffectEdit;
use crate::{Tool, ToolContext, ToolResult};
use serde::Deserialize;
use serde_json::Value;

pub(crate) use audio_dsp::effects::echo::{apply_echo, apply_multitap};

#[derive(Debug, Deserialize)]
struct Args {
    track: usize,
    delay_ms: Option<f32>,
    decay: Option<f32>,
    taps: Option<Vec<Tap>>,
    feedback: Option<f32>,
}

#[derive(Debug, Deserialize)]
struct Tap(f32, f32);

pub struct EchoTool;

impl Tool for EchoTool {
    fn name(&self) -> &'static str {
        "echo"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "echo",
            "Add an echo: delay_ms (ms) + decay (0..1), or taps [[delay_ms, gain], ...] with optional feedback (0..0.95). Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
                    "delay_ms": { "type": "number" },
                    "taps": { "type": "array", "items": { "type": "array" } },
                    "feedback": { "type": "number" },
                    "decay": { "type": "number", "default": 0.5 },
                    "start_sec": { "type": "number" },
                    "end_sec": { "type": "number" }
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
        let taps: Option<Vec<(f32, f32)>> = match (&args.taps, args.delay_ms) {
            (Some(_), Some(_)) => {
                return Ok(ToolResult::Error("give delay_ms or taps, not both".into()))
            }
            (None, None) => return Ok(ToolResult::Error("give delay_ms or taps".into())),
            (Some(t), None) => {
                if t.is_empty() {
                    return Ok(ToolResult::Error("taps must not be empty".into()));
                }
                if t.iter()
                    .any(|x| !x.0.is_finite() || x.0 <= 0.0 || !x.1.is_finite())
                {
                    return Ok(ToolResult::Error(
                        "each tap needs delay_ms > 0 and a finite gain".into(),
                    ));
                }
                Some(t.iter().map(|x| (x.0, x.1)).collect())
            }
            (None, Some(d)) => {
                if args.feedback.is_some() {
                    return Ok(ToolResult::Error("feedback applies to taps only".into()));
                }
                if !d.is_finite() || d <= 0.0 {
                    return Ok(ToolResult::Error("delay_ms must be positive".into()));
                }
                None
            }
        };
        let decay = args.decay.unwrap_or(0.5).clamp(0.0, 1.0);
        if let Some(taps) = taps {
            let feedback = args.feedback.unwrap_or(0.0);
            let label = format!(
                "echo track {} {} taps feedback={:.2}",
                args.track,
                taps.len(),
                feedback
            );
            return Ok(
                EffectEdit::on(args.track).run(ctx, label, move |samples, sr, channels| {
                    apply_multitap(samples, sr, channels, &taps, feedback)
                }),
            );
        }
        let delay_ms = args.delay_ms.unwrap_or_default();
        let (delay, d) = (delay_ms, decay);
        Ok(EffectEdit::on(args.track).run(
            ctx,
            format!(
                "echo track {} delay={}ms decay={:.2}",
                args.track, delay_ms, decay
            ),
            move |samples, sr, channels| apply_echo(samples, sr, channels, delay, d),
        ))
    }
}
