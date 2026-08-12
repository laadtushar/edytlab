use crate::schema::anthropic_tool;
use crate::tool::util::{check_optional_seconds_order, destructive_edit};
use crate::{Tool, ToolContext, ToolResult};
use serde::Deserialize;
use serde_json::Value;

pub(crate) use audio_dsp::effects::leveler::apply_leveler;

#[derive(Debug, Deserialize)]
struct Args {
    track: usize,
    target_db: f32,
    start_sec: Option<f64>,
    end_sec: Option<f64>,
}

pub struct LevelerTool;

impl Tool for LevelerTool {
    fn name(&self) -> &'static str {
        "leveler"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "leveler",
            "Apply dynamic leveling: normalise each short window to a target RMS level. Reduces variation between loud and quiet passages. Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
                    "target_db": { "type": "number", "description": "Target RMS level in dBFS (e.g. -18)" },
                    "start_sec": { "type": "number" },
                    "end_sec": { "type": "number" }
                },
                "required": ["track", "target_db"]
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };

        // A reversed window would survive independent clamping and
        // panic on the slice below.
        if let Err(e) = check_optional_seconds_order(args.start_sec, args.end_sec) {
            return Ok(ToolResult::Error(e));
        }
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
        let (target, s, e) = (args.target_db, args.start_sec, args.end_sec);
        Ok(destructive_edit(
            ctx,
            args.track,
            move |samples, sr| {
                let ch = channels;
                let len_frames = samples.len() / ch.max(1);
                let start = s
                    .map(|sec| ((sec * sr as f64) as usize).min(len_frames))
                    .unwrap_or(0);
                let end = e
                    .map(|sec| ((sec * sr as f64) as usize).min(len_frames))
                    .unwrap_or(len_frames);
                apply_leveler(
                    &mut samples[start * ch.max(1)..end * ch.max(1)],
                    sr,
                    ch,
                    target,
                    100,
                );
            },
            format!("leveler track {} target={}dB", args.track, target),
        ))
    }
}
