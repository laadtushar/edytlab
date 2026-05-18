use serde::Deserialize;
use serde_json::Value;

use crate::schema::anthropic_tool;
use crate::tool::util::destructive_edit;
use crate::{Tool, ToolContext, ToolResult};

struct AllPass {
    a1: f32,
    z: f32,
}

impl AllPass {
    fn new(frequency: f32, sr: f32) -> Self {
        let k = (std::f32::consts::PI * frequency / sr).tan();
        let a1 = (k - 1.0) / (k + 1.0);
        Self { a1, z: 0.0 }
    }

    fn process(&mut self, x: f32) -> f32 {
        let y = self.a1 * x + self.z;
        self.z = x - self.a1 * y;
        y
    }
}

pub(crate) fn apply_phaser(
    samples: &mut [f32],
    sr: u32,
    channels: usize,
    rate_hz: f32,
    depth: f32,
    stages: u32,
) {
    let channels = channels.max(1);
    let stages = (stages as usize).clamp(2, 12);
    let n_frames = samples.len() / channels;
    let min_freq = 200.0f32;
    let max_freq = 4000.0f32;
    let mut all_passes: Vec<Vec<AllPass>> = (0..channels)
        .map(|_| {
            (0..stages)
                .map(|_| AllPass::new(min_freq, sr as f32))
                .collect()
        })
        .collect();
    for frame in 0..n_frames {
        let lfo = (2.0 * std::f32::consts::PI * rate_hz * frame as f32 / sr as f32).sin();
        let freq = min_freq + (max_freq - min_freq) * (lfo * 0.5 + 0.5);
        for ch in 0..channels {
            for ap in &mut all_passes[ch] {
                let k = (std::f32::consts::PI * freq / sr as f32).tan();
                ap.a1 = (k - 1.0) / (k + 1.0);
            }
            let x = samples[frame * channels + ch];
            let mut y = x;
            for ap in &mut all_passes[ch] {
                y = ap.process(y);
            }
            samples[frame * channels + ch] = x + y * depth;
        }
    }
}

#[derive(Debug, Deserialize)]
struct Args {
    track: usize,
    rate_hz: Option<f32>,
    depth: Option<f32>,
    stages: Option<u32>,
}

pub struct PhaserTool;

impl Tool for PhaserTool {
    fn name(&self) -> &'static str {
        "phaser"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "phaser",
            "Apply a phaser effect using an all-pass filter chain with LFO sweep. rate_hz controls LFO speed; depth is the wet blend; stages sets the filter chain length (2-12). Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
                    "rate_hz": { "type": "number", "default": 0.5 },
                    "depth": { "type": "number", "default": 0.5 },
                    "stages": { "type": "integer", "default": 4 }
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
        let rate = args.rate_hz.unwrap_or(0.5).max(0.01);
        let depth = args.depth.unwrap_or(0.5).clamp(0.0, 1.0);
        let stages = args.stages.unwrap_or(4).clamp(2, 12);
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
        let (r, d, st) = (rate, depth, stages);
        Ok(destructive_edit(
            ctx,
            args.track,
            move |samples, sr| {
                apply_phaser(samples, sr, channels, r, d, st);
            },
            format!(
                "phaser track {} rate={:.2}Hz depth={:.2} stages={}",
                args.track, rate, depth, stages
            ),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::apply_phaser;

    #[test]
    fn does_not_clip() {
        let mut samples: Vec<f32> = (0..44100).map(|i| (i as f32 * 0.001).sin() * 0.8).collect();
        apply_phaser(&mut samples, 44100, 1, 1.0, 0.7, 4);
        let max = samples.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(
            max <= 1.5,
            "phaser output should not clip excessively, got {max}"
        );
    }
}
