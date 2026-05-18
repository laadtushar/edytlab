use crate::schema::anthropic_tool;
use crate::tool::util::destructive_edit;
use crate::{Tool, ToolContext, ToolResult};
use serde::Deserialize;
use serde_json::Value;

pub(crate) fn apply_noise_gate(
    samples: &mut [f32],
    sr: u32,
    channels: usize,
    threshold_db: f32,
    attack_ms: f32,
    release_ms: f32,
) {
    let channels = channels.max(1);
    let threshold_lin = 10.0f32.powf(threshold_db / 20.0);
    let attack_coeff = (-1.0 / (attack_ms * 0.001 * sr as f32)).exp();
    let release_coeff = (-1.0 / (release_ms * 0.001 * sr as f32)).exp();
    let mut gain = 0.0f32;
    let n_frames = samples.len() / channels;
    for frame in 0..n_frames {
        let peak = (0..channels)
            .map(|ch| samples[frame * channels + ch].abs())
            .fold(0.0f32, f32::max);
        let target = if peak >= threshold_lin {
            1.0f32
        } else {
            0.0f32
        };
        let coeff = if target > gain {
            attack_coeff
        } else {
            release_coeff
        };
        gain = target + coeff * (gain - target);
        for ch in 0..channels {
            samples[frame * channels + ch] *= gain;
        }
    }
}

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

#[cfg(test)]
mod tests {
    use super::apply_noise_gate;

    #[test]
    fn silences_below_threshold() {
        let mut samples: Vec<f32> = vec![0.005, 0.005, 0.5, 0.5, 0.005, 0.005];
        apply_noise_gate(&mut samples, 100, 1, -40.0, 1.0, 10.0);
        // Gate opens with attack, so gain ramps up but is close to 1.0
        assert!(samples[2] > 0.49, "above-threshold sample mostly untouched");
        assert!(samples[3] > 0.49, "above-threshold sample mostly untouched");
        // Gate closes with release, so below-threshold samples get silenced
        assert!(samples[0].abs() < 0.01, "below threshold silenced");
    }
}
