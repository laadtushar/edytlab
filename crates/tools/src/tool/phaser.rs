use serde::Deserialize;
use serde_json::Value;

use crate::schema::anthropic_tool;
use crate::tool::util::destructive_edit;
use crate::{Tool, ToolContext, ToolResult};

pub(crate) use audio_dsp::effects::phaser::apply_phaser;

#[derive(Debug, Deserialize)]
struct Args {
    track: usize,
    rate_hz: Option<f32>,
    depth: Option<f32>,
    stages: Option<u32>,
}

pub struct PhaserTool;

impl Tool for PhaserTool {
    fn name(&self) -> &'static str {
        "phaser"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "phaser",
            "Apply a phaser effect using an all-pass filter chain with LFO sweep. rate_hz controls LFO speed; depth is the wet blend; stages sets the filter chain length (2-12). Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
                    "rate_hz": { "type": "number", "default": 0.5 },
                    "depth": { "type": "number", "default": 0.5 },
                    "stages": { "type": "integer", "default": 4 }
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
        let rate = args.rate_hz.unwrap_or(0.5).max(0.01);
        let depth = args.depth.unwrap_or(0.5).clamp(0.0, 1.0);
        let stages = args.stages.unwrap_or(4).clamp(2, 12);
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
        let (r, d, st) = (rate, depth, stages);
        Ok(destructive_edit(
            ctx,
            args.track,
            move |samples, sr| {
                apply_phaser(samples, sr, channels, r, d, st);
            },
            format!(
                "phaser track {} rate={:.2}Hz depth={:.2} stages={}",
                args.track, rate, depth, stages
            ),
        ))
    }
}
