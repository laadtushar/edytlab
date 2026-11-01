use serde::Deserialize;
use serde_json::Value;

use crate::schema::anthropic_tool;
use crate::tool::util::EffectEdit;
use crate::{Tool, ToolContext, ToolResult};

pub(crate) use audio_dsp::effects::stereo_widener::apply_stereo_widener;

#[derive(Debug, Deserialize)]
struct Args {
    track: usize,
    width: Option<f32>,
}

pub struct StereoWidenerTool;

impl Tool for StereoWidenerTool {
    fn name(&self) -> &'static str {
        "stereo_widener"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "stereo_widener",
            "Widen or narrow the stereo field using M/S processing. width=0 collapses to mono, width=1 is original, width=2 doubles the stereo width. Requires stereo track. Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
                    "width": { "type": "number", "default": 1.5, "description": "Stereo width (0=mono, 1=original, 2=extra wide)" }
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
        let width = args.width.unwrap_or(1.5).max(0.0);
        let w = width;
        Ok(EffectEdit::on(args.track)
            .stereo_only("stereo_widener")
            .run(
                ctx,
                format!("stereo_widener track {} width={:.2}", args.track, width),
                move |samples, sr, channels| apply_stereo_widener(samples, sr, channels, w),
            ))
    }
}
