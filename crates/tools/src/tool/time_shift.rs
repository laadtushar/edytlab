use crate::schema::anthropic_tool;
use crate::tool::util::{append_state, check_track_index, load_head_state};
use crate::{Tool, ToolContext, ToolResult};
use serde::Deserialize;
use serde_json::{json, Value};

#[cfg(test)]
pub(crate) fn apply_time_shift(current: u64, delta_samples: u64) -> u64 {
    current + delta_samples
}
pub(crate) fn apply_time_shift_signed(current: u64, delta: i64) -> u64 {
    (current as i64 + delta).max(0) as u64
}

/// The largest part of `delta` the whole track can move without any clip
/// going negative (#245).
///
/// Clamping each clip independently was the defect: a track split at 12000
/// frames shifted by -2s became `[0, 0]`, because both clips hit the floor
/// separately. The spacing was gone for good — shifting forward again gave
/// `[96000, 96000]`, not the original gap — and the now-overlapping clips
/// *sum* on render rather than layering, so the audio was wrong too.
///
/// A shift moves the track, so the clamp belongs to the track: bound the
/// delta by the earliest clip's start and apply that same delta to all of
/// them. Relative spacing is then preserved by construction, and the shift
/// stays invertible up to the clamp.
pub(crate) fn clamp_track_delta(starts: impl IntoIterator<Item = u64>, delta: i64) -> i64 {
    let Some(min_start) = starts.into_iter().min() else {
        return delta;
    };
    // Only a backward shift can hit the floor.
    delta.max(-(min_start as i64))
}

#[derive(Debug, Deserialize)]
struct Args {
    track: usize,
    offset_sec: f64,
}

pub struct TimeShiftTool;

impl Tool for TimeShiftTool {
    fn name(&self) -> &'static str {
        "time_shift"
    }

    fn schema(&self) -> Value {
        anthropic_tool("time_shift",
            "Move a track's clips forward or backward in time. Positive offset_sec moves later, negative moves earlier (clamped to 0). Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
                    "offset_sec": { "type": "number", "description": "Seconds to shift (positive=later, negative=earlier)" }
                },
                "required": ["track", "offset_sec"]
            }))
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };
        let mut state = match load_head_state(ctx) {
            Ok(s) => s,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        if let Err(e) = check_track_index(&state.tracks, args.track) {
            return Ok(ToolResult::Error(e));
        }
        let sr = state.sample_rate as f64;
        let requested = (args.offset_sec * sr) as i64;
        let track = &mut state.tracks[args.track];

        // One delta for the whole track, not one clamp per clip.
        let delta = clamp_track_delta(track.clips.iter().map(|c| c.start_in_track), requested);
        let clamped = delta != requested;

        for clip in &mut track.clips {
            clip.start_in_track = apply_time_shift_signed(clip.start_in_track, delta);
        }
        // Recompute session length
        state.length_samples = state
            .tracks
            .iter()
            .flat_map(|t| t.clips.iter().map(|c| c.start_in_track + c.length))
            .max()
            .unwrap_or(0);
        let new_id = match append_state(
            ctx,
            state,
            format!("time_shift track {} {:+.3}s", args.track, args.offset_sec),
        ) {
            Ok(id) => id,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        // A clamped shift is not the shift that was asked for, and
        // silence about it is how a caller ends up believing the track
        // moved further than it did.
        let applied_sec = delta as f64 / sr;
        Ok(ToolResult::Ok(json!({
            "node_id": new_id.to_hex(),
            "requested_sec": args.offset_sec,
            "applied_sec": applied_sec,
            "clamped": clamped,
            "summary": if clamped {
                format!(
                    "Shifted track {} by {:+.3}s (clamped from {:+.3}s — the track \
                     cannot move earlier than its start; relative spacing kept)",
                    args.track, applied_sec, args.offset_sec
                )
            } else {
                format!("Shifted track {} by {:+.3}s", args.track, args.offset_sec)
            },
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::{apply_time_shift, apply_time_shift_signed};
    #[test]
    fn shifts_positive() {
        assert_eq!(apply_time_shift(0, 44100), 44100);
    }
    #[test]
    fn clamps_negative_to_zero() {
        let start = 44100u64;
        let shift = -(5 * 44100i64);
        assert_eq!(apply_time_shift_signed(start, shift), 0);
    }
}
