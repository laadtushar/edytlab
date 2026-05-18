use crate::schema::anthropic_tool;
use crate::tool::util::destructive_edit;
use crate::{Tool, ToolContext, ToolResult};
use serde::Deserialize;
use serde_json::Value;

pub(crate) fn apply_limiter(samples: &mut [f32], _sr: u32, _channels: usize, ceiling_db: f32) {
    let ceiling = 10.0f32.powf(ceiling_db / 20.0);
    for s in samples.iter_mut() {
        if s.abs() > ceiling {
            *s = s.signum() * ceiling;
        }
    }
}

#[derive(Debug, Deserialize)]
struct Args {
    track: usize,
    ceiling_db: f32,
    start_sec: Option<f64>,
    end_sec: Option<f64>,
}

pub struct LimiterTool;

impl Tool for LimiterTool {
    fn name(&self) -> &'static str {
        "limiter"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "limiter",
            "Brick-wall limiter: hard-clip any samples exceeding ceiling_db. Prevents digital clipping. Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
                    "ceiling_db": { "type": "number", "description": "Maximum peak level in dBFS (e.g. -1.0)" },
                    "start_sec": { "type": "number" },
                    "end_sec": { "type": "number" }
                },
                "required": ["track", "ceiling_db"]
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };
        if args.ceiling_db > 0.0 {
            return Ok(ToolResult::Error("ceiling_db must be <= 0.0".into()));
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
        let (ceiling, s, e) = (args.ceiling_db, args.start_sec, args.end_sec);
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
                apply_limiter(
                    &mut samples[start * ch.max(1)..end * ch.max(1)],
                    sr,
                    ch,
                    ceiling,
                );
            },
            format!("limiter track {} ceiling={}dBFS", args.track, ceiling),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::apply_limiter;

    #[test]
    fn clips_above_ceiling() {
        let mut samples = vec![0.5f32, 0.8, 1.5, -1.2, 0.3];
        apply_limiter(&mut samples, 44100, 1, -6.0);
        let ceiling = 10.0f32.powf(-6.0 / 20.0);
        for s in &samples {
            assert!(
                s.abs() <= ceiling + 1e-5,
                "sample {s} exceeds ceiling {ceiling}"
            );
        }
    }
}
