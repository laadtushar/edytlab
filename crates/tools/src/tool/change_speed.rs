use serde::Deserialize;
use serde_json::Value;
use crate::schema::anthropic_tool;
use crate::tool::util::destructive_edit;
use crate::{Tool, ToolContext, ToolResult};

/// Linear interpolation resampling. `factor` > 1 = faster (shorter), < 1 = slower (longer).
pub(crate) fn apply_change_speed(samples: &[f32], channels: usize, factor: f32) -> Vec<f32> {
    let channels = channels.max(1);
    let in_frames = samples.len() / channels;
    let out_frames = ((in_frames as f32 / factor).round() as usize).max(1);
    let mut out = Vec::with_capacity(out_frames * channels);
    for out_f in 0..out_frames {
        let src_f = out_f as f32 * factor;
        let lo = (src_f as usize).min(in_frames.saturating_sub(1));
        let hi = (lo + 1).min(in_frames.saturating_sub(1));
        let t = src_f - lo as f32;
        for ch in 0..channels {
            let a = samples[lo * channels + ch];
            let b = samples[hi * channels + ch];
            out.push(a + (b - a) * t);
        }
    }
    out
}

#[derive(Debug, Deserialize)]
struct Args { track: usize, factor: f32 }

pub struct ChangeSpeedTool;

impl Tool for ChangeSpeedTool {
    fn name(&self) -> &'static str { "change_speed" }

    fn schema(&self) -> Value {
        anthropic_tool("change_speed",
            "Resample a track to change playback speed without pitch preservation. factor > 1 speeds up (shorter duration), factor < 1 slows down (longer). Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
                    "factor": { "type": "number", "exclusiveMinimum": 0.0, "description": "Speed multiplier, e.g. 2.0 = double speed" }
                },
                "required": ["track", "factor"]
            }))
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a, Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };
        if args.factor <= 0.0 || !args.factor.is_finite() {
            return Ok(ToolResult::Error("factor must be a positive finite number".into()));
        }
        let channels = {
            let state = match crate::tool::util::load_head_state(ctx) {
                Ok(s) => s, Err(e) => return Ok(ToolResult::Error(e)),
            };
            if let Err(e) = crate::tool::util::check_track_index(&state.tracks, args.track) {
                return Ok(ToolResult::Error(e));
            }
            let clip = state.tracks[args.track].clips.first().cloned();
            if let Some(c) = clip {
                audio_decoder::decode_file(&c.source_path).map(|d| d.channels as usize).unwrap_or(1)
            } else { return Ok(ToolResult::Error(format!("track {} has no clips", args.track))); }
        };
        let factor = args.factor;
        Ok(destructive_edit(ctx, args.track,
            move |samples, _sr| {
                let resampled = apply_change_speed(samples, channels, factor);
                *samples = resampled;
            },
            format!("change_speed track {} x{:.3}", args.track, args.factor),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::apply_change_speed;
    #[test]
    fn double_speed_halves_length() {
        let samples: Vec<f32> = (0..100).map(|i| i as f32 / 100.0).collect();
        let result = apply_change_speed(&samples, 1, 2.0);
        assert_eq!(result.len(), 50);
    }
    #[test]
    fn half_speed_doubles_length() {
        let samples: Vec<f32> = (0..100).map(|i| i as f32 / 100.0).collect();
        let result = apply_change_speed(&samples, 1, 0.5);
        assert_eq!(result.len(), 200);
    }
    #[test]
    fn factor_one_is_identity() {
        let samples: Vec<f32> = vec![0.1, 0.2, 0.3, 0.4];
        let result = apply_change_speed(&samples, 1, 1.0);
        assert_eq!(result.len(), 4);
        for (a, b) in samples.iter().zip(result.iter()) {
            assert!((a - b).abs() < 1e-5);
        }
    }
}
