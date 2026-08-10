//! Paste clipboard audio into a track at a given time offset.

use serde::Deserialize;
use serde_json::{json, Value};

use crate::schema::anthropic_tool;
use crate::tool::util::{destructive_edit, track_channels};
use crate::{Tool, ToolContext, ToolResult};

/// Splice `clipboard` into `samples` at `at_sec`. The insertion point is
/// clamped to `samples.len()` so a value past the end appends rather than
/// panicking.
pub fn apply(
    samples: &mut Vec<f32>,
    sample_rate: u32,
    channels: usize,
    at_sec: f64,
    clipboard: &Option<Vec<f32>>,
) -> Result<(), PasteError> {
    let data = clipboard.as_ref().ok_or(PasteError::EmptyClipboard)?;
    let stride = channels.max(1);
    let total_frames = samples.len() / stride;
    // Splice on a frame boundary: an offset landing mid-frame would
    // shift every following sample by one and swap left and right for
    // the rest of the track.
    let offset = ((at_sec * sample_rate as f64) as usize).min(total_frames) * stride;
    samples.splice(offset..offset, data.iter().copied());
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum PasteError {
    #[error("clipboard is empty; run copy_region first")]
    EmptyClipboard,
}

pub struct PasteRegionTool;

impl Tool for PasteRegionTool {
    fn name(&self) -> &'static str {
        "paste_region"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "paste_region",
            "Paste the clipboard audio (set by copy_region) into a track at a given offset. \
             The clipboard audio is spliced in; samples after the insertion point are shifted right. \
             Appends a new session node.",
            json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer", "description": "Zero-based track index" },
                    "at":    {
                        "type": "number",
                        "description": "Insertion point in seconds; past the end appends"
                    }
                },
                "required": ["track", "at"],
                "additionalProperties": false
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        #[derive(Deserialize)]
        struct Args {
            track: usize,
            at: f64,
        }

        let parsed: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };

        // Snapshot clipboard now so the closure captures it by value.
        // We check for empty before the closure to give an early error.
        let clipboard_snap = ctx.clipboard.clone();
        if clipboard_snap.is_none() {
            return Ok(ToolResult::Error(
                "clipboard is empty; run copy_region first".into(),
            ));
        }
        let at = parsed.at;
        let track = parsed.track;
        let channels = match track_channels(ctx, track) {
            Ok(c) => c,
            Err(e) => return Ok(ToolResult::Error(e)),
        };

        Ok(destructive_edit(
            ctx,
            track,
            move |samples, sample_rate| {
                // Ignore error: already validated clipboard is Some above.
                let _ = apply(samples, sample_rate, channels, at, &clipboard_snap);
            },
            format!("paste clipboard at {at:.2}s on track {track}"),
        ))
    }
}
