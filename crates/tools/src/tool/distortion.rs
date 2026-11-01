use serde::Deserialize;
use serde_json::Value;

use crate::schema::anthropic_tool;
use crate::tool::util::EffectEdit;
use crate::{Tool, ToolContext, ToolResult};

pub(crate) use audio_dsp::effects::distortion::apply_distortion;

#[derive(Debug, Deserialize)]
struct Args {
    track: usize,
    drive: Option<f32>,
    tone: Option<f32>,
}

pub struct DistortionTool;

impl Tool for DistortionTool {
    fn name(&self) -> &'static str {
        "distortion"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "distortion",
            "Apply soft-clip distortion (tanh waveshaper) followed by a tone filter. drive > 1 increases gain before clipping; tone (0=dark, 1=bright) controls the output filter. Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
                    "drive": { "type": "number", "default": 3.0, "description": "Pre-gain multiplier (1=clean, 10=heavy)" },
                    "tone": { "type": "number", "default": 0.5, "description": "Tone brightness 0..1" }
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
        let drive = args.drive.unwrap_or(3.0).max(1.0);
        let tone = args.tone.unwrap_or(0.5).clamp(0.0, 1.0);
        let (dr, tn) = (drive, tone);
        Ok(EffectEdit::on(args.track).run(
            ctx,
            format!(
                "distortion track {} drive={:.1} tone={:.2}",
                args.track, drive, tone
            ),
            move |samples, sr, channels| apply_distortion(samples, sr, channels, dr, tn),
        ))
    }
}
