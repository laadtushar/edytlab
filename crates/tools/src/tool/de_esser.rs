use serde::Deserialize;
use serde_json::Value;

use crate::schema::anthropic_tool;
use crate::tool::util::{check_optional_seconds_order, destructive_edit};
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
        let freq = args.frequency_hz.unwrap_or(7000.0).max(1000.0);
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
        let (f, t, s, e) = (freq, args.threshold_db, args.start_sec, args.end_sec);
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
                apply_de_esser(
                    &mut samples[start * ch.max(1)..end * ch.max(1)],
                    sr,
                    ch,
                    f,
                    t,
                );
            },
            format!(
                "de_esser track {} freq={:.0}Hz threshold={}dB",
                args.track, freq, args.threshold_db
            ),
        ))
    }
}
