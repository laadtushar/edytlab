use crate::schema::anthropic_tool;
use crate::tool::util::EffectEdit;
use crate::{Tool, ToolContext, ToolResult};
use serde::Deserialize;
use serde_json::Value;

pub(crate) use audio_dsp::effects::noise_gate::apply_noise_gate;

#[derive(Debug, Deserialize)]
struct Args {
    track: usize,
    threshold_db: f32,
    attack_ms: Option<f32>,
    release_ms: Option<f32>,
    start_sec: Option<f64>,
    end_sec: Option<f64>,
}

pub struct NoiseGateTool;

impl Tool for NoiseGateTool {
    fn name(&self) -> &'static str {
        "noise_gate"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "noise_gate",
            "Apply a noise gate: audio below threshold_db is silenced. attack_ms and release_ms control how fast the gate opens/closes. Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
                    "threshold_db": { "type": "number", "description": "Gate threshold in dBFS (e.g. -40)" },
                    "attack_ms": { "type": "number", "default": 5.0, "description": "Gate open time in ms" },
                    "release_ms": { "type": "number", "default": 100.0, "description": "Gate close time in ms" },
                    "start_sec": { "type": "number" },
                    "end_sec": { "type": "number" }
                },
                "required": ["track", "threshold_db"]
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

        let attack = args.attack_ms.unwrap_or(5.0).max(0.1);
        let release = args.release_ms.unwrap_or(100.0).max(0.1);
        let thresh = args.threshold_db;
        Ok(EffectEdit::on(args.track).run_in_window(
            ctx,
            args.start_sec,
            args.end_sec,
            format!(
                "noise_gate track {} threshold={}dB",
                args.track, args.threshold_db
            ),
            move |window, sr, channels| {
                apply_noise_gate(window, sr, channels, thresh, attack, release)
            },
        ))
    }
}
