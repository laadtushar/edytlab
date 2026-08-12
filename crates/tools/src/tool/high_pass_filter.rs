use crate::schema::anthropic_tool;
use crate::tool::util::{check_track_index, destructive_edit, load_head_state};
use crate::{Tool, ToolContext, ToolResult};
use serde::Deserialize;
use serde_json::Value;

pub(crate) use audio_dsp::effects::high_pass_filter::apply_high_pass;

#[derive(Debug, Deserialize)]
struct Args {
    track: usize,
    cutoff_hz: f32,
    start_sec: Option<f64>,
    end_sec: Option<f64>,
}

pub struct HighPassFilterTool;

impl Tool for HighPassFilterTool {
    fn name(&self) -> &'static str {
        "high_pass_filter"
    }

    fn schema(&self) -> Value {
        anthropic_tool("high_pass_filter",
            "Apply a Butterworth high-pass filter to a track, removing frequencies below cutoff_hz. Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
                    "cutoff_hz": { "type": "number", "description": "Cutoff frequency in Hz" },
                    "start_sec": { "type": "number" },
                    "end_sec": { "type": "number" }
                },
                "required": ["track", "cutoff_hz"]
            }))
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };
        if args.cutoff_hz <= 0.0 {
            return Ok(ToolResult::Error("cutoff_hz must be positive".into()));
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
        let (cutoff, s, e) = (args.cutoff_hz, args.start_sec, args.end_sec);
        Ok(destructive_edit(
            ctx,
            args.track,
            move |samples, sr| apply_high_pass(samples, sr, channels, cutoff, s, e),
            format!(
                "high_pass_filter track {} cutoff={:.0}Hz",
                args.track, cutoff
            ),
        ))
    }
}
