//! Reverse the sample order in a sub-range, or the entire buffer
//! when no range is provided.

use crate::schema::anthropic_tool;
use crate::tool::util::{destructive_edit, track_channels};
use crate::util::range_resolver::{resolve as resolve_range, RangeError};
use crate::{Range, Tool, ToolContext, ToolResult};
use serde::Deserialize;
use serde_json::Value;

/// Reverse `range` (or the whole track when `None`).
///
/// Whole *frames* are swapped so the channel order inside each frame
/// survives. Reversing the interleaved buffer directly turns
/// `[L0,R0,L1,R1]` into `[R1,L1,R0,L0]` — the frames come back in the
/// right order but every one of them has its left and right swapped,
/// mirroring the stereo image across the reversed span.
pub fn apply_reverse(samples: &mut [f32], sample_rate: u32, channels: usize, range: Option<Range>) {
    let stride = channels.max(1);
    let total_frames = samples.len() / stride;
    let (start, end) = match range {
        Some(r) => {
            let s = ((r.start_sec * sample_rate as f64) as usize).min(total_frames);
            let e = ((r.end_sec * sample_rate as f64) as usize).min(total_frames);
            (s.min(e), e)
        }
        None => (0, total_frames),
    };
    if end <= start {
        return;
    }
    let frames = end - start;
    for i in 0..frames / 2 {
        let a = (start + i) * stride;
        let b = (end - 1 - i) * stride;
        for ch in 0..stride {
            samples.swap(a + ch, b + ch);
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct ReverseParams {
    pub range: Option<Range>,
}

pub fn dispatch_reverse(
    params: ReverseParams,
    user_message: &str,
    samples: &mut [f32],
    sample_rate: u32,
    channels: usize,
) -> Result<(), ReverseError> {
    let range = resolve_range(params.range, user_message, false).map_err(ReverseError::Range)?;
    apply_reverse(samples, sample_rate, channels, range);
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum ReverseError {
    #[error("{0}")]
    Range(#[from] RangeError),
}

// ---------------------------------------------------------------------------
// Tool trait impl
// ---------------------------------------------------------------------------

pub struct ReverseTool;

impl Tool for ReverseTool {
    fn name(&self) -> &'static str {
        "reverse"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "reverse",
            "Reverse the sample order of a track, optionally within a sub-range. \
             If range is omitted, the entire track is reversed. \
             Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
                    "range": {
                        "type": "object",
                        "properties": {
                            "start_sec": { "type": "number" },
                            "end_sec": { "type": "number" }
                        },
                        "required": ["start_sec", "end_sec"]
                    }
                },
                "required": ["track"],
                "additionalProperties": false
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        #[derive(serde::Deserialize)]
        struct Args {
            track: usize,
            range: Option<Range>,
        }

        let parsed: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };

        let range =
            match crate::util::range_resolver::resolve(parsed.range, ctx.user_message, false) {
                Ok(r) => r,
                Err(e) => return Ok(ToolResult::Error(e.to_string())),
            };

        let track = parsed.track;
        let channels = match track_channels(ctx, track) {
            Ok(c) => c,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        let label = match range {
            Some(r) => format!(
                "reverse {:.2}s\u{2013}{:.2}s on track {track}",
                r.start_sec, r.end_sec
            ),
            None => format!("reverse track {track} (full)"),
        };

        Ok(destructive_edit(
            ctx,
            track,
            move |samples, sample_rate| {
                apply_reverse(samples, sample_rate, channels, range);
            },
            label,
        ))
    }
}
