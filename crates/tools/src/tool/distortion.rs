use serde::Deserialize;
use serde_json::Value;

use crate::schema::anthropic_tool;
use crate::tool::util::destructive_edit;
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

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };
        let drive = args.drive.unwrap_or(3.0).max(1.0);
        let tone = args.tone.unwrap_or(0.5).clamp(0.0, 1.0);
        let channels = {
            let state = match crate::tool::util::load_head_state(ctx) {
                Ok(s) => s,
                Err(e) => return Ok(ToolResult::Error(e)),
            };
            if let Err(e) = crate::tool::util::check_track_index(&state.tracks, args.track) {
                return Ok(ToolResult::Error(e));
            }
            let clip = state.tracks[args.track].clips.first().cloned();
            if let Some(c) = clip {
                audio_decoder::decode_file(&c.source_path)
                    .map(|d| d.channels as usize)
                    .unwrap_or(1)
            } else {
                return Ok(ToolResult::Error(format!(
                    "track {} has no clips",
                    args.track
                )));
            }
        };
        let (dr, tn) = (drive, tone);
        Ok(destructive_edit(
            ctx,
            args.track,
            move |samples, sr| {
                apply_distortion(samples, sr, channels, dr, tn);
            },
            format!(
                "distortion track {} drive={:.1} tone={:.2}",
                args.track, drive, tone
            ),
        ))
    }
}
