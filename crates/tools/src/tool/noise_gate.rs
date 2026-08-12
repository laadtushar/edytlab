use crate::schema::anthropic_tool;
use crate::tool::util::{check_optional_seconds_order, destructive_edit};
use crate::{Tool, ToolContext, ToolResult};
use serde::Deserialize;
use serde_json::Value;

pub(crate) use audio_dsp::effects::noise_gate::apply_noise_gate;

#[derive(Debug, Deserialize)]
struct Args {
    track: usize,
    threshold_db: f32,
    attack_ms: Option<f32>,
    release_ms: Option<f32>,
    start_sec: Option<f64>,
    end_sec: Option<f64>,
}

pub struct NoiseGateTool;

impl Tool for NoiseGateTool {
    fn name(&self) -> &'static str {
        "noise_gate"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "noise_gate",
            "Apply a noise gate: audio below threshold_db is silenced. attack_ms and release_ms control how fast the gate opens/closes. Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
                    "threshold_db": { "type": "number", "description": "Gate threshold in dBFS (e.g. -40)" },
                    "attack_ms": { "type": "number", "default": 5.0, "description": "Gate open time in ms" },
                    "release_ms": { "type": "number", "default": 100.0, "description": "Gate close time in ms" },
                    "start_sec": { "type": "number" },
                    "end_sec": { "type": "number" }
                },
                "required": ["track", "threshold_db"]
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
        let attack = args.attack_ms.unwrap_or(5.0).max(0.1);
        let release = args.release_ms.unwrap_or(100.0).max(0.1);
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
        let (thresh, s, e) = (args.threshold_db, args.start_sec, args.end_sec);
        Ok(destructive_edit(
            ctx,
            args.track,
            move |samples, sr| {
                let channels = channels;
                let len_frames = samples.len() / channels.max(1);
                let start_frame = s
                    .map(|sec| ((sec * sr as f64) as usize).min(len_frames))
                    .unwrap_or(0);
                let end_frame = e
                    .map(|sec| ((sec * sr as f64) as usize).min(len_frames))
                    .unwrap_or(len_frames);
                let start_idx = start_frame * channels.max(1);
                let end_idx = end_frame * channels.max(1);
                apply_noise_gate(
                    &mut samples[start_idx..end_idx],
                    sr,
                    channels,
                    thresh,
                    attack,
                    release,
                );
            },
            format!(
                "noise_gate track {} threshold={}dB",
                args.track, args.threshold_db
            ),
        ))
    }
}
