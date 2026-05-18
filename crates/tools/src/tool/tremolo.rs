use serde::Deserialize;
use serde_json::Value;

use crate::schema::anthropic_tool;
use crate::tool::util::destructive_edit;
use crate::{Tool, ToolContext, ToolResult};

pub(crate) fn apply_tremolo(
    samples: &mut [f32],
    sr: u32,
    channels: usize,
    rate_hz: f32,
    depth: f32,
) {
    let channels = channels.max(1);
    let depth = depth.clamp(0.0, 1.0);
    let n_frames = samples.len() / channels;
    for frame in 0..n_frames {
        let lfo = (2.0 * std::f32::consts::PI * rate_hz * frame as f32 / sr as f32).cos();
        let gain = 1.0 - depth * (1.0 - lfo) / 2.0;
        for ch in 0..channels {
            samples[frame * channels + ch] *= gain;
        }
    }
}

#[derive(Debug, Deserialize)]
struct Args {
    track: usize,
    rate_hz: Option<f32>,
    depth: Option<f32>,
}

pub struct TremoloTool;

impl Tool for TremoloTool {
    fn name(&self) -> &'static str {
        "tremolo"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "tremolo",
            "Apply tremolo (LFO amplitude modulation). rate_hz controls oscillation speed; depth (0..1) controls modulation depth. Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
                    "rate_hz": { "type": "number", "default": 4.0, "description": "LFO rate in Hz" },
                    "depth": { "type": "number", "default": 0.5, "description": "Modulation depth 0..1" }
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
        let rate = args.rate_hz.unwrap_or(4.0).max(0.1);
        let depth = args.depth.unwrap_or(0.5).clamp(0.0, 1.0);
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
        let (r, d) = (rate, depth);
        Ok(destructive_edit(
            ctx,
            args.track,
            move |samples, sr| {
                apply_tremolo(samples, sr, channels, r, d);
            },
            format!(
                "tremolo track {} rate={:.1}Hz depth={:.2}",
                args.track, rate, depth
            ),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::apply_tremolo;

    #[test]
    fn modulates_amplitude() {
        let mut samples = vec![1.0f32; 44100];
        apply_tremolo(&mut samples, 44100, 1, 5.0, 0.5);
        let at_max = samples[0];
        let at_min = samples[44100 / (5 * 2)];
        assert!(
            at_max > at_min,
            "tremolo should create amplitude variation, max={at_max} min={at_min}"
        );
    }
}
