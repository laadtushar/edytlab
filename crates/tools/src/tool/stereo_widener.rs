use serde::Deserialize;
use serde_json::Value;

use crate::schema::anthropic_tool;
use crate::tool::util::destructive_edit;
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

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };
        let width = args.width.unwrap_or(1.5).max(0.0);
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
        if channels < 2 {
            return Ok(ToolResult::Error(
                "stereo_widener requires a stereo track".into(),
            ));
        }
        let w = width;
        Ok(destructive_edit(
            ctx,
            args.track,
            move |samples, sr| {
                apply_stereo_widener(samples, sr, channels, w);
            },
            format!("stereo_widener track {} width={:.2}", args.track, width),
        ))
    }
}
