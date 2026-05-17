//! `set_clip_envelope` — replace the per-clip volume automation curve.

use serde::Deserialize;
use serde_json::{json, Value};
use session::EnvelopePoint;

use crate::schema::anthropic_tool;
use crate::tool::util::{append_state, load_head_state};
use crate::{Tool, ToolContext, ToolResult};

#[derive(Debug, Deserialize)]
struct PointArgs {
    time_sec: f64,
    gain_db: f32,
}

#[derive(Debug, Deserialize)]
struct Args {
    track_index: usize,
    clip_index: usize,
    points: Vec<PointArgs>,
}

pub struct SetClipEnvelopeTool;

impl Tool for SetClipEnvelopeTool {
    fn name(&self) -> &'static str {
        "set_clip_envelope"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "set_clip_envelope",
            "Replace the per-clip volume automation curve. Points are (time_sec, gain_db) pairs; \
             the engine linearly interpolates between them at render time. An empty points array \
             clears any existing envelope (unity gain).",
            json!({
                "type": "object",
                "properties": {
                    "track_index": {
                        "type": "integer",
                        "minimum": 0,
                        "description": "Zero-based index of the target track."
                    },
                    "clip_index": {
                        "type": "integer",
                        "minimum": 0,
                        "description": "Zero-based index of the clip within the track."
                    },
                    "points": {
                        "type": "array",
                        "description": "Automation points sorted by time. Each point has time_sec (seconds from clip start) and gain_db.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "time_sec": { "type": "number" },
                                "gain_db":  { "type": "number" }
                            },
                            "required": ["time_sec", "gain_db"],
                            "additionalProperties": false
                        }
                    }
                },
                "required": ["track_index", "clip_index", "points"],
                "additionalProperties": false
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };

        let mut state = match load_head_state(ctx) {
            Ok(s) => s,
            Err(msg) => return Ok(ToolResult::Error(msg)),
        };

        let track = match state.tracks.get_mut(args.track_index) {
            Some(t) => t,
            None => {
                return Ok(ToolResult::Error(format!(
                    "track_index {} out of range (session has {} tracks)",
                    args.track_index,
                    state.tracks.len()
                )))
            }
        };

        let clip = match track.clips.get_mut(args.clip_index) {
            Some(c) => c,
            None => {
                return Ok(ToolResult::Error(format!(
                    "clip_index {} out of range (track {} has {} clips)",
                    args.clip_index,
                    args.track_index,
                    track.clips.len()
                )))
            }
        };

        let sample_rate = state.sample_rate as f64;
        let mut pts: Vec<EnvelopePoint> = args
            .points
            .iter()
            .map(|p| EnvelopePoint {
                time_samples: (p.time_sec * sample_rate) as u64,
                gain_db: p.gain_db,
            })
            .collect();
        pts.sort_by_key(|p| p.time_samples);
        clip.volume_envelope = pts;

        let point_count = args.points.len();
        let new_id = match append_state(
            ctx,
            state,
            format!(
                "set_clip_envelope track {} clip {} ({} points)",
                args.track_index, args.clip_index, point_count
            ),
        ) {
            Ok(id) => id,
            Err(msg) => return Ok(ToolResult::Error(msg)),
        };

        Ok(ToolResult::Ok(json!({
            "node_id": new_id.to_hex(),
            "summary": format!(
                "Set volume envelope on track {} clip {} ({} points); new head {}",
                args.track_index, args.clip_index, point_count, new_id.to_hex()
            )
        })))
    }
}
