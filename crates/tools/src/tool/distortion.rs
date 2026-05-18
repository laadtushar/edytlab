use serde::Deserialize;
use serde_json::Value;

use crate::schema::anthropic_tool;
use crate::tool::util::destructive_edit;
use crate::{Tool, ToolContext, ToolResult};

pub(crate) fn apply_distortion(
    samples: &mut [f32],
    sr: u32,
    channels: usize,
    drive: f32,
    tone: f32,
) {
    let channels = channels.max(1);
    let drive = drive.max(1.0);
    let tone = tone.clamp(0.0, 1.0);
    let tanh_drive = drive.tanh().max(1e-6);
    for s in samples.iter_mut() {
        *s = (*s * drive).tanh() / tanh_drive;
    }
    let cutoff = 200.0 + tone * 8000.0;
    let k = (-2.0 * std::f32::consts::PI * cutoff / sr as f32).exp();
    let n_frames = samples.len() / channels;
    for ch in 0..channels {
        let mut z = 0.0f32;
        for frame in 0..n_frames {
            let idx = frame * channels + ch;
            z = samples[idx] * (1.0 - k) + z * k;
            samples[idx] = z;
        }
    }
}

#[derive(Debug, Deserialize)]
struct Args {
    track: usize,
    drive: Option<f32>,
    tone: Option<f32>,
}

pub struct DistortionTool;

impl Tool for DistortionTool {
    fn name(&self) -> &'static str {
        "distortion"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "distortion",
            "Apply soft-clip distortion (tanh waveshaper) followed by a tone filter. drive > 1 increases gain before clipping; tone (0=dark, 1=bright) controls the output filter. Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
                    "drive": { "type": "number", "default": 3.0, "description": "Pre-gain multiplier (1=clean, 10=heavy)" },
                    "tone": { "type": "number", "default": 0.5, "description": "Tone brightness 0..1" }
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
        let drive = args.drive.unwrap_or(3.0).max(1.0);
        let tone = args.tone.unwrap_or(0.5).clamp(0.0, 1.0);
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
        let (dr, tn) = (drive, tone);
        Ok(destructive_edit(
            ctx,
            args.track,
            move |samples, sr| {
                apply_distortion(samples, sr, channels, dr, tn);
            },
            format!(
                "distortion track {} drive={:.1} tone={:.2}",
                args.track, drive, tone
            ),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::apply_distortion;

    #[test]
    fn high_drive_clips() {
        let mut samples = vec![0.5f32; 100];
        apply_distortion(&mut samples, 44100, 1, 10.0, 0.5);
        let max = samples.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(
            max <= 1.0 + 1e-5,
            "hard-clipped output should be within [-1,1]"
        );
    }

    #[test]
    fn low_drive_doesnt_clip() {
        let mut samples = vec![0.1f32; 100];
        apply_distortion(&mut samples, 44100, 1, 1.0, 0.5);
        let max = samples.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        // At low drive with low signal, output should remain well below 1
        assert!(max < 0.15, "low-drive processing of 0.1 amplitude signal");
    }
}
