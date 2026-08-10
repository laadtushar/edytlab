use serde::Deserialize;
use serde_json::Value;

use crate::schema::anthropic_tool;
use crate::tool::util::destructive_edit_resample;
use crate::{Tool, ToolContext, ToolResult};

/// Linear interpolation resample: interleaved `channels`-channel input
/// at `src_rate` → output at `dst_rate`.
pub(crate) fn linear_resample(
    samples: &[f32],
    channels: usize,
    src_rate: u32,
    dst_rate: u32,
) -> Vec<f32> {
    if src_rate == dst_rate || samples.is_empty() {
        return samples.to_vec();
    }
    let channels = channels.max(1);
    let src_frames = samples.len() / channels;
    let dst_frames = (src_frames as f64 * dst_rate as f64 / src_rate as f64).round() as usize;
    let mut out = Vec::with_capacity(dst_frames * channels);
    for dst_frame in 0..dst_frames {
        let src_pos = dst_frame as f64 * src_rate as f64 / dst_rate as f64;
        let src_lo = src_pos.floor() as usize;
        let src_hi = (src_lo + 1).min(src_frames - 1);
        let frac = (src_pos - src_lo as f64) as f32;
        for ch in 0..channels {
            let lo = samples[src_lo * channels + ch];
            let hi = samples[src_hi * channels + ch];
            out.push(lo + frac * (hi - lo));
        }
    }
    out
}

#[derive(Debug, Deserialize)]
struct Args {
    track: usize,
    target_sample_rate: u32,
}

pub struct ResampleTrackTool;

impl Tool for ResampleTrackTool {
    fn name(&self) -> &'static str {
        "resample_track"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "resample_track",
            "Resample a track to a different sample rate using linear interpolation. Common rates: 22050, 44100, 48000, 96000. Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer", "description": "Track index" },
                    "target_sample_rate": { "type": "integer", "description": "Target sample rate in Hz (e.g. 44100, 48000)" }
                },
                "required": ["track", "target_sample_rate"]
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };
        if args.target_sample_rate == 0 {
            return Ok(ToolResult::Error(
                "target_sample_rate must be positive".into(),
            ));
        }

        let target = args.target_sample_rate;
        let label = format!("resample_track {} to {}Hz", args.track, target);

        // The one tool that writes a file at a rate its source never had,
        // which is why it used to carry its own copy of the destructive-edit
        // path. That copy edited `clips[0]` alone, so resampling a track an
        // interior cut had split converted the head and dropped the tail.
        Ok(destructive_edit_resample(
            ctx,
            args.track,
            move |samples, src_rate, channels| {
                *samples = linear_resample(samples, channels.max(1) as usize, src_rate, target);
                (target, channels)
            },
            label,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::linear_resample;

    #[test]
    fn resample_doubles_length() {
        let input: Vec<f32> = (0..100).map(|i| i as f32 / 100.0).collect();
        let output = linear_resample(&input, 1, 22050, 44100);
        assert!(
            output.len() >= 195 && output.len() <= 205,
            "expected ~200 samples, got {}",
            output.len()
        );
    }

    #[test]
    fn resample_halves_length() {
        let input: Vec<f32> = (0..200).map(|i| i as f32 / 200.0).collect();
        let output = linear_resample(&input, 1, 44100, 22050);
        assert!(
            output.len() >= 95 && output.len() <= 105,
            "expected ~100 samples, got {}",
            output.len()
        );
    }

    #[test]
    fn resample_same_rate_unchanged() {
        let input = vec![0.1f32, 0.5, -0.3, 0.8];
        let output = linear_resample(&input, 1, 44100, 44100);
        assert_eq!(output.len(), input.len());
        for (a, b) in input.iter().zip(output.iter()) {
            assert!((a - b).abs() < 1e-5);
        }
    }
}
