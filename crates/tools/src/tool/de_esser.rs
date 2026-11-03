use serde::Deserialize;
use serde_json::Value;

use crate::schema::anthropic_tool;
use crate::tool::util::{parse_args, EffectEdit};
use crate::{Tool, ToolContext, ToolResult};

pub(crate) use audio_dsp::effects::de_esser::apply_de_esser;

#[derive(Debug, Deserialize)]
struct Args {
    track: usize,
    frequency_hz: Option<f32>,
    threshold_db: f32,
    start_sec: Option<f64>,
    end_sec: Option<f64>,
}

pub struct DeEsserTool;

impl Tool for DeEsserTool {
    fn name(&self) -> &'static str {
        "de_esser"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "de_esser",
            "Reduce harsh sibilant 's' and 'sh' sounds. frequency_hz sets where sibilance detection begins (default 7000Hz); threshold_db is the compression trigger level. Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
                    "frequency_hz": { "type": "number", "default": 7000.0 },
                    "threshold_db": { "type": "number", "description": "Detection threshold in dBFS (e.g. -20)" },
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
        let args: Args = match parse_args(args) {
            Ok(a) => a,
            Err(r) => return Ok(r),
        };

        let freq = args.frequency_hz.unwrap_or(7000.0).max(1000.0);
        let (f, t) = (freq, args.threshold_db);
        Ok(EffectEdit::on(args.track).run_in_window(
            ctx,
            args.start_sec,
            args.end_sec,
            format!(
                "de_esser track {} freq={:.0}Hz threshold={}dB",
                args.track, freq, args.threshold_db
            ),
            move |window, sr, channels| apply_de_esser(window, sr, channels, f, t),
        ))
    }
}
