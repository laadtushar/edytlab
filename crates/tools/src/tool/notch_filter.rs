use crate::schema::anthropic_tool;
use crate::tool::util::{check_track_index, destructive_edit, load_head_state};
use crate::{Tool, ToolContext, ToolResult};
use serde::Deserialize;
use serde_json::Value;

pub(crate) use audio_dsp::effects::notch_filter::apply_notch;

#[derive(Debug, Deserialize)]
struct Args {
    track: usize,
    center_hz: f32,
    q: f32,
    start_sec: Option<f64>,
    end_sec: Option<f64>,
}

pub struct NotchFilterTool;

impl Tool for NotchFilterTool {
    fn name(&self) -> &'static str {
        "notch_filter"
    }

    fn schema(&self) -> Value {
        anthropic_tool("notch_filter",
            "Apply a notch (band-reject) filter to a track, attenuating frequencies near center_hz. q controls the width: higher Q = narrower notch. Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
                    "center_hz": { "type": "number", "description": "Center frequency to reject in Hz" },
                    "q": { "type": "number", "description": "Quality factor (sharpness); typical range 0.5..30" },
                    "start_sec": { "type": "number" },
                    "end_sec": { "type": "number" }
                },
                "required": ["track", "center_hz", "q"]
            }))
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };
        if args.center_hz <= 0.0 {
            return Ok(ToolResult::Error("center_hz must be positive".into()));
        }
        if args.q <= 0.0 {
            return Ok(ToolResult::Error("q must be positive".into()));
        }
        let channels = {
            let state = match load_head_state(ctx) {
                Ok(s) => s,
                Err(e) => return Ok(ToolResult::Error(e)),
            };
            if let Err(e) = check_track_index(&state.tracks, args.track) {
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
        let (center, q, s, e) = (args.center_hz, args.q, args.start_sec, args.end_sec);
        Ok(destructive_edit(
            ctx,
            args.track,
            move |samples, sr| apply_notch(samples, sr, channels, center, q, s, e),
            format!(
                "notch_filter track {} center={:.0}Hz q={:.1}",
                args.track, center, q
            ),
        ))
    }
}
