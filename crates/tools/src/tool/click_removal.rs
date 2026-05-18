use crate::schema::anthropic_tool;
use crate::tool::util::destructive_edit;
use crate::{Tool, ToolContext, ToolResult};
use serde::Deserialize;
use serde_json::Value;

fn median3(a: f32, b: f32, c: f32) -> f32 {
    let mut v = [a, b, c];
    v.sort_by(|x, y| x.partial_cmp(y).unwrap());
    v[1]
}

pub(crate) fn apply_click_removal(samples: &mut [f32], _sr: u32, channels: usize, threshold: f32) {
    let channels = channels.max(1);
    let n_frames = samples.len() / channels;
    if n_frames < 3 {
        return;
    }
    for frame in 1..n_frames - 1 {
        for ch in 0..channels {
            let prev = samples[(frame - 1) * channels + ch];
            let curr = samples[frame * channels + ch];
            let next = samples[(frame + 1) * channels + ch];
            let med = median3(prev, curr, next);
            if (curr - med).abs() > threshold {
                samples[frame * channels + ch] = med;
            }
        }
    }
}

#[derive(Debug, Deserialize)]
struct Args {
    track: usize,
    threshold: Option<f32>,
    start_sec: Option<f64>,
    end_sec: Option<f64>,
}

pub struct ClickRemovalTool;

impl Tool for ClickRemovalTool {
    fn name(&self) -> &'static str {
        "click_removal"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "click_removal",
            "Remove clicks and pops by detecting sample spikes (via median filter) and replacing them with interpolated values. threshold is the amplitude deviation that triggers detection. Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
                    "threshold": { "type": "number", "default": 0.5, "description": "Amplitude spike threshold (linear, 0..1 scale)" },
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
        let threshold = args.threshold.unwrap_or(0.5).max(0.0);
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
        let (thresh, s, e) = (threshold, args.start_sec, args.end_sec);
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
                apply_click_removal(
                    &mut samples[start * ch.max(1)..end * ch.max(1)],
                    sr,
                    ch,
                    thresh,
                );
            },
            format!(
                "click_removal track {} threshold={:.3}",
                args.track, threshold
            ),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::apply_click_removal;

    #[test]
    fn removes_spike() {
        let mut samples = vec![0.1f32; 100];
        samples[50] = 10.0;
        apply_click_removal(&mut samples, 44100, 1, 3.0);
        assert!(
            samples[50].abs() < 1.0,
            "spike should be attenuated, got {}",
            samples[50]
        );
        assert!((samples[49] - 0.1).abs() < 0.02, "neighbors untouched");
    }
}
