use crate::schema::anthropic_tool;
use crate::tool::util::{
    biquad_process, check_track_index, destructive_edit, load_head_state, BiquadCoeffs,
};
use crate::{Tool, ToolContext, ToolResult};
use serde::Deserialize;
use serde_json::Value;

pub(crate) fn apply_low_pass(
    samples: &mut [f32],
    sr: u32,
    channels: usize,
    cutoff_hz: f32,
    start_sec: Option<f64>,
    end_sec: Option<f64>,
) {
    let channels = channels.max(1);
    let len_frames = samples.len() / channels;
    let start = start_sec
        .map(|s| ((s * sr as f64) as usize).min(len_frames))
        .unwrap_or(0);
    let end = end_sec
        .map(|e| ((e * sr as f64) as usize).min(len_frames))
        .unwrap_or(len_frames);
    let coeffs = BiquadCoeffs::low_pass(cutoff_hz, sr);
    biquad_process(samples, channels, &coeffs, start, end);
}

#[derive(Debug, Deserialize)]
struct Args {
    track: usize,
    cutoff_hz: f32,
    start_sec: Option<f64>,
    end_sec: Option<f64>,
}

pub struct LowPassFilterTool;

impl Tool for LowPassFilterTool {
    fn name(&self) -> &'static str {
        "low_pass_filter"
    }

    fn schema(&self) -> Value {
        anthropic_tool("low_pass_filter",
            "Apply a Butterworth low-pass filter to a track, removing frequencies above cutoff_hz. Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
                    "cutoff_hz": { "type": "number" },
                    "start_sec": { "type": "number" },
                    "end_sec": { "type": "number" }
                },
                "required": ["track", "cutoff_hz"]
            }))
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };
        if args.cutoff_hz <= 0.0 {
            return Ok(ToolResult::Error("cutoff_hz must be positive".into()));
        }
        let channels = {
            let state = match load_head_state(ctx) {
                Ok(s) => s,
                Err(e) => return Ok(ToolResult::Error(e)),
            };
            if let Err(e) = check_track_index(&state.tracks, args.track) {
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
        let (cutoff, s, e) = (args.cutoff_hz, args.start_sec, args.end_sec);
        Ok(destructive_edit(
            ctx,
            args.track,
            move |samples, sr| apply_low_pass(samples, sr, channels, cutoff, s, e),
            format!(
                "low_pass_filter track {} cutoff={:.0}Hz",
                args.track, cutoff
            ),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::apply_low_pass;
    #[test]
    fn passes_dc() {
        let mut samples = vec![1.0f32; 4410];
        apply_low_pass(&mut samples, 44100, 1, 5000.0, None, None);
        let tail_mean: f32 = samples[4000..].iter().sum::<f32>() / 410.0;
        assert!(
            tail_mean > 0.9,
            "DC should pass through LPF, got {tail_mean}"
        );
    }
}
