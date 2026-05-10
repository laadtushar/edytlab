//! Splice silence into a buffer at a given offset.

use crate::schema::anthropic_tool;
use crate::tool::util::destructive_edit;
use crate::{Tool, ToolContext, ToolResult};
use serde::Deserialize;
use serde_json::Value;

pub fn apply_insert_silence(
    samples: &mut Vec<f32>,
    sample_rate: u32,
    at_sec: f64,
    duration_sec: f64,
) -> Result<(), InsertSilenceError> {
    if duration_sec < 0.0 {
        return Err(InsertSilenceError::NegativeDuration(duration_sec));
    }
    if at_sec < 0.0 {
        return Err(InsertSilenceError::NegativeOffset(at_sec));
    }
    let offset = ((at_sec * sample_rate as f64) as usize).min(samples.len());
    let count = (duration_sec * sample_rate as f64) as usize;
    samples.splice(offset..offset, std::iter::repeat_n(0.0, count));
    Ok(())
}

#[derive(Debug, Deserialize)]
pub struct InsertSilenceParams {
    pub at: f64,
    pub duration: f64,
}

pub fn dispatch_insert_silence(
    params: InsertSilenceParams,
    samples: &mut Vec<f32>,
    sample_rate: u32,
) -> Result<(), InsertSilenceError> {
    apply_insert_silence(samples, sample_rate, params.at, params.duration)
}

#[derive(Debug, thiserror::Error)]
pub enum InsertSilenceError {
    #[error("duration must be >= 0; got {0}")]
    NegativeDuration(f64),
    #[error("at must be >= 0; got {0}")]
    NegativeOffset(f64),
}

// ---------------------------------------------------------------------------
// Tool trait impl
// ---------------------------------------------------------------------------

pub struct InsertSilenceTool;

impl Tool for InsertSilenceTool {
    fn name(&self) -> &'static str {
        "insert_silence"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "insert_silence",
            "Insert a region of silence at a time offset in a track. \
             Extends the track length by `duration` seconds. \
             Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
                    "at": { "type": "number", "description": "Offset in seconds where silence is inserted" },
                    "duration": { "type": "number", "description": "Duration of silence in seconds" }
                },
                "required": ["track", "at", "duration"],
                "additionalProperties": false
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        #[derive(serde::Deserialize)]
        struct Args {
            track: usize,
            at: f64,
            duration: f64,
        }

        let parsed: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };

        let at = parsed.at;
        let duration = parsed.duration;
        let track = parsed.track;

        Ok(destructive_edit(
            ctx,
            track,
            move |samples, sample_rate| {
                let _ = apply_insert_silence(samples, sample_rate, at, duration);
            },
            format!("insert {duration:.2}s silence at {at:.2}s on track {track}"),
        ))
    }
}
