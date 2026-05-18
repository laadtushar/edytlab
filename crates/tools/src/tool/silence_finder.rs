use serde::Deserialize;
use serde_json::{json, Value};

use crate::schema::anthropic_tool;
use crate::tool::util::{check_track_index, load_head_state};
use crate::{Tool, ToolContext, ToolResult};

/// Returns (start_sec, end_sec) pairs for silent regions.
pub(crate) fn find_silence_regions_sec(
    samples: &[f32],
    sr: u32,
    channels: usize,
    threshold_db: f32,
    min_silence_ms: f32,
) -> Vec<(f32, f32)> {
    let channels = channels.max(1);
    let threshold_lin = 10.0f32.powf(threshold_db / 20.0);
    let min_frames = ((min_silence_ms * 0.001 * sr as f32) as usize).max(1);
    let n_frames = samples.len() / channels;
    let mut regions = Vec::new();
    let mut silent_start: Option<usize> = None;
    for frame in 0..n_frames {
        let peak = (0..channels)
            .map(|ch| samples[frame * channels + ch].abs())
            .fold(0.0f32, f32::max);
        let is_silent = peak < threshold_lin;
        match (is_silent, silent_start) {
            (true, None) => silent_start = Some(frame),
            (false, Some(start)) => {
                if frame - start >= min_frames {
                    regions.push((start as f32 / sr as f32, frame as f32 / sr as f32));
                }
                silent_start = None;
            }
            _ => {}
        }
    }
    if let Some(start) = silent_start {
        if n_frames - start >= min_frames {
            regions.push((start as f32 / sr as f32, n_frames as f32 / sr as f32));
        }
    }
    regions
}

#[derive(Debug, Deserialize)]
struct Args {
    track: usize,
    threshold_db: f32,
    min_silence_ms: Option<f32>,
}

pub struct SilenceFinderTool;

impl Tool for SilenceFinderTool {
    fn name(&self) -> &'static str {
        "silence_finder"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "silence_finder",
            "Analyse a track and return the time ranges of silent regions. Does not modify audio. Returns a list of {start_sec, end_sec} objects.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
                    "threshold_db": { "type": "number", "description": "Silence floor in dBFS" },
                    "min_silence_ms": { "type": "number", "default": 500.0 }
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
        let min_ms = args.min_silence_ms.unwrap_or(500.0).max(1.0);
        let state = match load_head_state(ctx) {
            Ok(s) => s,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        if let Err(e) = check_track_index(&state.tracks, args.track) {
            return Ok(ToolResult::Error(e));
        }
        let clip = match state.tracks[args.track].clips.first() {
            Some(c) => c.clone(),
            None => {
                return Ok(ToolResult::Error(format!(
                    "track {} has no clips",
                    args.track
                )))
            }
        };
        let decoded = match audio_decoder::decode_file(&clip.source_path) {
            Ok(d) => d,
            Err(e) => return Ok(ToolResult::Error(format!("decode failed: {e}"))),
        };
        let regions = find_silence_regions_sec(
            &decoded.samples,
            decoded.sample_rate,
            decoded.channels as usize,
            args.threshold_db,
            min_ms,
        );
        let region_json: Vec<serde_json::Value> = regions
            .iter()
            .map(|(s, e)| json!({ "start_sec": s, "end_sec": e }))
            .collect();
        let count = region_json.len();
        Ok(ToolResult::Ok(json!({
            "regions": region_json,
            "count": count,
            "summary": format!("Found {} silent region(s) on track {}", count, args.track)
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::find_silence_regions_sec;

    #[test]
    fn finds_two_gaps() {
        let mut samples = vec![0.0f32; 100];
        for i in 0..20 {
            samples[i] = 0.5;
        }
        for i in 50..70 {
            samples[i] = 0.5;
        }
        let regions = find_silence_regions_sec(&samples, 100, 1, -40.0, 100.0);
        assert_eq!(regions.len(), 2);
        assert!((regions[0].0 - 0.2).abs() < 0.02);
        assert!((regions[0].1 - 0.5).abs() < 0.02);
    }
}
