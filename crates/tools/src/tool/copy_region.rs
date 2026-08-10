//! Extract a time range from a track's audio into the in-memory clipboard.

use serde::Deserialize;
use serde_json::{json, Value};

use crate::schema::anthropic_tool;
use crate::tool::util::{check_track_index, load_head_state};
use crate::util::range_resolver::resolve as resolve_range;
use crate::{Range, Tool, ToolContext, ToolResult};

/// Copy the samples in `[range.start_sec, range.end_sec)` from `samples`
/// (already windowed to the clip) into `clipboard`. The sample buffer
/// uses the same interleaved layout as the decoded source.
/// Copy `range` into the clipboard.
///
/// Seconds convert to *frames* and are scaled by `channels`, so the
/// captured region is the one that was asked for and begins on a frame
/// boundary. Slicing the interleaved buffer as if it were mono grabbed
/// half the requested duration, and an odd start index began the
/// clipboard mid-frame — pasting it back then swapped left and right.
pub fn apply(
    samples: &[f32],
    sample_rate: u32,
    channels: usize,
    range: Range,
    clipboard: &mut Option<Vec<f32>>,
) -> Result<(), CopyError> {
    let stride = channels.max(1);
    let total_frames = samples.len() / stride;
    let start = ((range.start_sec * sample_rate as f64) as usize).min(total_frames);
    let end = ((range.end_sec * sample_rate as f64) as usize).min(total_frames);
    if end <= start {
        return Err(CopyError::EmptyRange);
    }
    *clipboard = Some(samples[start * stride..end * stride].to_vec());
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum CopyError {
    #[error(
        "range produces an empty slice; check start_sec < end_sec and the range is within the clip"
    )]
    EmptyRange,
}

pub struct CopyRegionTool;

impl Tool for CopyRegionTool {
    fn name(&self) -> &'static str {
        "copy_region"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "copy_region",
            "Copy a time range from a track into the in-memory clipboard. \
             Does not modify the session graph. Use paste_region to insert it.",
            json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer", "description": "Zero-based track index" },
                    "range": {
                        "type": "object",
                        "properties": {
                            "start_sec": { "type": "number" },
                            "end_sec":   { "type": "number" }
                        },
                        "required": ["start_sec", "end_sec"],
                        "additionalProperties": false
                    }
                },
                "required": ["track"],
                "additionalProperties": false
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        #[derive(Deserialize)]
        struct Args {
            track: usize,
            range: Option<Range>,
        }

        let parsed: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };

        let range = match resolve_range(parsed.range, ctx.user_message, true) {
            Ok(Some(r)) => r,
            Ok(None) => unreachable!("resolve with required=true always returns Some or Err"),
            Err(e) => return Ok(ToolResult::Error(e.to_string())),
        };

        let state = match load_head_state(ctx) {
            Ok(s) => s,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        if let Err(e) = check_track_index(&state.tracks, parsed.track) {
            return Ok(ToolResult::Error(e));
        }

        let clip = match state.tracks[parsed.track].clips.first() {
            Some(c) => c.clone(),
            None => {
                return Ok(ToolResult::Error(format!(
                    "track {} has no clips",
                    parsed.track
                )))
            }
        };

        let decoded = match audio_decoder::decode_file(&clip.source_path) {
            Ok(d) => d,
            Err(e) => return Ok(ToolResult::Error(format!("decode failed: {e}"))),
        };

        // Slice to the clip window (same logic as `destructive_edit`).
        let channels = decoded.channels as usize;
        let total_frames = (decoded.samples.len() / channels) as u64;
        let src_start = clip.source_offset.min(total_frames);
        let src_end = clip
            .source_offset
            .saturating_add(clip.length)
            .min(total_frames);
        let window =
            &decoded.samples[(src_start as usize * channels)..(src_end as usize * channels)];

        if let Err(e) = apply(window, decoded.sample_rate, channels, range, ctx.clipboard) {
            return Ok(ToolResult::Error(e.to_string()));
        }

        Ok(ToolResult::Ok(json!({
            "summary": format!(
                "copied {:.2}s–{:.2}s from track {} to clipboard",
                range.start_sec, range.end_sec, parsed.track
            )
        })))
    }
}
