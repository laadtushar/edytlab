use crate::schema::anthropic_tool;
use crate::tool::util::destructive_edit;
use crate::{Tool, ToolContext, ToolResult};
use serde::Deserialize;
use serde_json::Value;

pub(crate) use audio_dsp::effects::reverb::apply_reverb;

#[derive(Debug, Deserialize)]
struct Args {
    track: usize,
    room_size: Option<f32>,
    damping: Option<f32>,
    wet: Option<f32>,
}

pub struct ReverbTool;

impl Tool for ReverbTool {
    fn name(&self) -> &'static str {
        "reverb"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "reverb",
            "Apply Freeverb algorithmic reverb. room_size (0-1) controls reverb length, damping (0-1) controls high-freq decay, wet (0-1) is the wet/dry blend. Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
                    "room_size": { "type": "number", "default": 0.5, "description": "Room size 0..1" },
                    "damping": { "type": "number", "default": 0.5, "description": "High-freq damping 0..1" },
                    "wet": { "type": "number", "default": 0.3, "description": "Wet mix 0..1" },
                    "start_sec": { "type": "number" },
                    "end_sec": { "type": "number" }
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
        let room = args.room_size.unwrap_or(0.5);
        let damp = args.damping.unwrap_or(0.5);
        let wet = args.wet.unwrap_or(0.3);
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
        Ok(destructive_edit(
            ctx,
            args.track,
            move |samples, sr| {
                let mut v = samples.to_vec();
                apply_reverb(&mut v, sr, channels, room, damp, wet);
                *samples = v;
            },
            format!(
                "reverb track {} room={:.2} wet={:.2}",
                args.track, room, wet
            ),
        ))
    }
}
