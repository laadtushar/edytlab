//! Reverse the sample order in a sub-range, or the entire buffer
//! when no range is provided.

use crate::schema::anthropic_tool;
use crate::tool::util::destructive_edit;
use crate::util::range_resolver::{resolve as resolve_range, RangeError};
use crate::{Range, Tool, ToolContext, ToolResult};
use serde::Deserialize;
use serde_json::Value;

pub fn apply_reverse(samples: &mut [f32], sample_rate: u32, range: Option<Range>) {
    match range {
        Some(r) => {
            let start = (r.start_sec * sample_rate as f64) as usize;
            let end = ((r.end_sec * sample_rate as f64) as usize).min(samples.len());
            if end > start {
                samples[start..end].reverse();
            }
        }
        None => samples.reverse(),
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
) -> Result<(), ReverseError> {
    let range = resolve_range(params.range, user_message, false).map_err(ReverseError::Range)?;
    apply_reverse(samples, sample_rate, range);
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
                apply_reverse(samples, sample_rate, range);
            },
            label,
        ))
    }
}
