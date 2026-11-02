//! `align_end_to_end` — put tracks one after another on the timeline (#771).
//!
//! Joining a set of takes into a sequence is a `time_shift` per track and a
//! sum in the agent's head. This does the sum: the first track named stays
//! where it is, and each next track moves, as a whole, until its first clip
//! starts where the previous one's last clip ends.
//!
//! Each track is counted in the frames of its own clips (#234, #673): a
//! 16 kHz voice after a 48 kHz bed starts at the bed's end in seconds, which
//! is a different number of frames. Like `time_shift`, which it is made of, it
//! moves only the tracks named. It does not consult sync-lock (that keeps
//! tracks together through an edit that shifts time, and this is the edit
//! that sets them apart), and labels stay at the seconds they were at. A
//! transcript made from a moved track goes with it: its word times are
//! seconds of that track as it sits on the timeline (#384), so they are moved
//! by what the track moved, or `cut_words` would cut audio the words do not
//! describe.

use serde::Deserialize;
use serde_json::{json, Value};

use crate::schema::anthropic_tool;
use crate::tool::util::{
    append_state, check_track_index, load_head_state, seconds_to_frames, set_session_length,
    shift_track_transcript, timeline_end, track_clip_rate,
};
use crate::{Tool, ToolContext, ToolResult};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    tracks: Vec<usize>,
    #[serde(default)]
    gap_sec: f64,
}

pub struct AlignEndToEndTool;

impl Tool for AlignEndToEndTool {
    fn name(&self) -> &'static str {
        "align_end_to_end"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "align_end_to_end",
            "Place tracks one after another in the order given: the first stays put, and each \
             next track moves, clips and spacing together, to start where the previous one ends. \
             Other tracks stay. Appends a new session node.",
            json!({
                "type": "object",
                "properties": {
                    "tracks": {
                        "type": "array",
                        "items": { "type": "integer" },
                        "minItems": 2
                    },
                    "gap_sec": {
                        "type": "number",
                        "minimum": 0,
                        "description": "Silence between tracks. Default 0."
                    }
                },
                "required": ["tracks"],
                "additionalProperties": false
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };
        if !args.gap_sec.is_finite() || args.gap_sec < 0.0 {
            return Ok(ToolResult::Error(format!(
                "gap_sec must be finite and >= 0; got {}",
                args.gap_sec
            )));
        }
        if args.tracks.len() < 2 {
            return Ok(ToolResult::Error(
                "name at least two tracks; one track is already end to end with itself".into(),
            ));
        }
        let mut state = match load_head_state(ctx) {
            Ok(s) => s,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        // Everything is checked before anything moves.
        let mut rates = Vec::with_capacity(args.tracks.len());
        for (i, &t) in args.tracks.iter().enumerate() {
            if let Err(e) = check_track_index(&state.tracks, t) {
                return Ok(ToolResult::Error(e));
            }
            if args.tracks[..i].contains(&t) {
                return Ok(ToolResult::Error(format!(
                    "track {t} is named twice; each track goes in once"
                )));
            }
            let track = &state.tracks[t];
            if track.clips.is_empty() {
                return Ok(ToolResult::Error(format!(
                    "track {t} has no clips; there is nothing to place"
                )));
            }
            match track_clip_rate(track, state.sample_rate) {
                Ok(r) => rates.push(r),
                Err(msg) => return Ok(ToolResult::Error(msg)),
            }
        }

        // The end of the previous track, in seconds, whatever its rate.
        let mut end_sec = {
            let first = &state.tracks[args.tracks[0]];
            timeline_end(&first.clips) as f64 / rates[0]
        };
        let mut moved = Vec::new();
        for (&t, &rate) in args.tracks.iter().zip(&rates).skip(1) {
            let target = seconds_to_frames(end_sec + args.gap_sec, rate) as i64;
            let clips = &mut state.tracks[t].clips;
            let earliest = clips.iter().map(|c| c.start_in_track).min().unwrap_or(0);
            let delta = target - earliest as i64;
            for clip in clips.iter_mut() {
                clip.start_in_track = (clip.start_in_track as i64 + delta).max(0) as u64;
            }
            shift_track_transcript(&mut state, t, delta as f64 / rate, None);
            let clips = &state.tracks[t].clips;
            end_sec = timeline_end(clips) as f64 / rate;
            moved.push(json!({ "track": t, "start_sec": target as f64 / rate }));
        }
        set_session_length(&mut state);

        let order = args
            .tracks
            .iter()
            .map(|t| t.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        let new_id = match append_state(ctx, state, format!("align_end_to_end tracks {order}")) {
            Ok(id) => id,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        Ok(ToolResult::Ok(json!({
            "node_id": new_id.to_hex(),
            "moved": moved,
            "summary": format!(
                "Placed tracks {order} end to end{}; track {} stayed where it was",
                if args.gap_sec > 0.0 {
                    format!(" with {:.3}s between them", args.gap_sec)
                } else {
                    String::new()
                },
                args.tracks[0]
            ),
        })))
    }
}
