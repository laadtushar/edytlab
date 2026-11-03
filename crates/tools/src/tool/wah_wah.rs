use serde::Deserialize;
use serde_json::Value;

use crate::schema::anthropic_tool;
use crate::tool::util::{parse_args, EffectEdit};
use crate::{Tool, ToolContext, ToolResult};
use audio_dsp::effects::wah_wah::{apply_wah_wah, clamp_base_freq};

#[derive(Debug, Deserialize)]
struct Args {
    track: usize,
    rate_hz: Option<f32>,
    depth: Option<f32>,
    base_freq_hz: Option<f32>,
    resonance: Option<f32>,
}

pub struct WahWahTool;

impl Tool for WahWahTool {
    fn name(&self) -> &'static str {
        "wah_wah"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "wah_wah",
            "Apply a wah-wah effect with an LFO-swept band-pass filter. Depth controls the sweep up to two octaves either side of base_freq_hz; resonance is the filter Q. Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
                    "rate_hz": { "type": "number", "default": 1.0, "description": "LFO rate in Hz (0.1 to 10)" },
                    "depth": { "type": "number", "default": 0.5, "description": "Sweep depth (0 to 1)" },
                    "base_freq_hz": { "type": "number", "default": 1000.0, "description": "Centre frequency in Hz (200 to 5000)" },
                    "resonance": { "type": "number", "default": 2.0, "description": "Filter Q (0.1 to 10)" }
                },
                "required": ["track"]
            }),
        )
    }

    fn runs_off_the_lock(&self) -> bool {
        true
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match parse_args(args) {
            Ok(a) => a,
            Err(r) => return Ok(r),
        };
        let rate = args.rate_hz.unwrap_or(1.0).clamp(0.1, 10.0);
        let depth = args.depth.unwrap_or(0.5).clamp(0.0, 1.0);
        let base = clamp_base_freq(args.base_freq_hz.unwrap_or(1000.0));
        let resonance = args.resonance.unwrap_or(2.0).clamp(0.1, 10.0);
        Ok(EffectEdit::on(args.track).run(
            ctx,
            format!("wah_wah track {} rate={rate:.1}Hz depth={depth:.2} base={base:.1}Hz Q={resonance:.2}", args.track),
            move |samples, sr, channels| apply_wah_wah(samples, sr, channels, rate, depth, base, resonance),
        ))
    }
}
