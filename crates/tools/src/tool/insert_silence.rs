//! Splice silence into a buffer at a given offset.

use crate::schema::anthropic_tool;
use crate::tool::util::{
    destructive_edit_then, insert_gap_timeline, remap_after_insert, sync_other_tracks,
    track_channels,
};
use crate::{Tool, ToolContext, ToolResult};
use serde::Deserialize;
use serde_json::Value;

/// Splice `duration_sec` of silence in at `at_sec`.
///
/// Both the position and the length are counted in *frames* and scaled
/// by `channels`. Treating an interleaved buffer as mono put the
/// silence at the wrong place and made it the wrong length — but the
/// real damage was an odd sample count, which shifts every frame after
/// the splice by one and swaps left and right for the entire remainder
/// of the track.
pub fn apply_insert_silence(
    samples: &mut Vec<f32>,
    sample_rate: u32,
    channels: usize,
    at_sec: f64,
    duration_sec: f64,
) -> Result<(), InsertSilenceError> {
    if duration_sec < 0.0 {
        return Err(InsertSilenceError::NegativeDuration(duration_sec));
    }
    if at_sec < 0.0 {
        return Err(InsertSilenceError::NegativeOffset(at_sec));
    }
    let stride = channels.max(1);
    let total_frames = samples.len() / stride;
    let at_frame = ((at_sec * sample_rate as f64) as usize).min(total_frames);
    let frames = (duration_sec * sample_rate as f64) as usize;
    let offset = at_frame * stride;
    let count = frames * stride;
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
    channels: usize,
) -> Result<(), InsertSilenceError> {
    apply_insert_silence(samples, sample_rate, channels, params.at, params.duration)
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

        let channels = match track_channels(ctx, track) {
            Ok(c) => c,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        // Sync-lock (#170 §3): the gap has to open on every track, or
        // everything after it drifts by exactly the amount inserted.
        // The other tracks get a timeline shift rather than a rewrite —
        // it is the same silence and it costs no derived file.
        //
        // It happens inside the same node as the sample edit, because
        // "keep the tracks aligned" is one thing to undo.
        Ok(destructive_edit_then(
            ctx,
            track,
            move |samples, sample_rate, chans| {
                let _ = apply_insert_silence(samples, sample_rate, channels, at, duration);
                (sample_rate, chans)
            },
            move |state, edited| {
                // Labels move whether or not sync-lock is on: an insert
                // lengthens the recording, and every mark after the
                // splice point is now that much later (#203).
                // The transcript moves with them (#231): a word after
                // the splice is now that much later, and `cut_words`
                // reads these positions back as sample offsets.
                remap_after_insert(state, at, duration);

                if !state.sync_lock {
                    return;
                }
                let rate = state.sample_rate.max(1) as f64;
                let at_frames = (at * rate).round().max(0.0) as u64;
                let len_frames = (duration * rate).round().max(0.0) as u64;
                if len_frames == 0 {
                    return;
                }
                sync_other_tracks(state, edited, |clips| {
                    insert_gap_timeline(clips, at_frames, len_frames)
                });
            },
            format!("insert {duration:.2}s silence at {at:.2}s on track {track}"),
        ))
    }
}
