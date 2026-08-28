//! `move_clip` — place a single clip at a new position in its track.
//!
//! `time_shift` moves every clip on a track together. This moves one,
//! which is what a timeline drag is, and what "pull the second half a
//! bit later" means once a track has been cut.

use serde::Deserialize;
use serde_json::{json, Value};

use crate::schema::anthropic_tool;
use crate::tool::util::{append_state, check_track_index, load_head_state, seconds_to_clip_frames};
use crate::{Tool, ToolContext, ToolResult};

#[derive(Debug, Deserialize)]
struct Args {
    track: usize,
    clip_index: usize,
    start_sec: f64,
}

/// Recompute the session length from the clips that are left.
///
/// The timeline's length is the furthest point any clip reaches, so
/// moving or removing the last clip changes it. Leaving it stale makes
/// a render pad the end with silence that the arrangement no longer
/// has.
pub(crate) fn recompute_length(state: &mut session::SessionState) {
    state.length_samples = state
        .tracks
        .iter()
        .flat_map(|t| t.clips.iter().map(|c| c.start_in_track + c.length))
        .max()
        .unwrap_or(0);
}

pub struct MoveClipTool;

impl Tool for MoveClipTool {
    fn name(&self) -> &'static str {
        "move_clip"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "move_clip",
            "Move one clip to a new start position within its track, leaving the other clips \
             where they are. Use time_shift to move a whole track together. Appends a new \
             session node.",
            json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer", "minimum": 0 },
                    "clip_index": { "type": "integer", "minimum": 0 },
                    "start_sec": {
                        "type": "number",
                        "minimum": 0,
                        "description": "New start of the clip, in seconds from the top of the timeline."
                    }
                },
                "required": ["track", "clip_index", "start_sec"]
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };
        if !args.start_sec.is_finite() || args.start_sec < 0.0 {
            return Ok(ToolResult::Error(format!(
                "start_sec must be finite and >= 0; got {}",
                args.start_sec
            )));
        }
        let mut state = match load_head_state(ctx) {
            Ok(s) => s,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        if let Err(e) = check_track_index(&state.tracks, args.track) {
            return Ok(ToolResult::Error(e));
        }
        let track = &mut state.tracks[args.track];
        let clip = match track.clips.get_mut(args.clip_index) {
            Some(c) => c,
            None => {
                return Ok(ToolResult::Error(format!(
                    "clip_index {} out of range; track {} has {} clip{}",
                    args.clip_index,
                    args.track,
                    track.clips.len(),
                    if track.clips.len() == 1 { "" } else { "s" },
                )))
            }
        };
        // The clip's own frame domain, not the session's (#234). On a
        // 44.1 kHz bed in a 48 kHz project the two differ by 8.8%, so
        // "move to 30 s" using the session rate lands at 27.6 s — and
        // `list_tracks` read the field back through the same wrong
        // conversion, so the timeline agreed with the request while the
        // render disagreed with both.
        clip.start_in_track = match seconds_to_clip_frames(clip, args.start_sec) {
            Ok(f) => f,
            Err(msg) => return Ok(ToolResult::Error(msg)),
        };

        // Clips are kept in start order. Nothing enforces it in the type,
        // but `list_tracks` renders them in vector order and
        // `flattened_track_wav` concatenates them in vector order, so a
        // clip dragged past its neighbour would draw and render in the
        // wrong place while the session data was correct.
        track.clips.sort_by_key(|c| c.start_in_track);
        recompute_length(&mut state);

        let new_id = match append_state(
            ctx,
            state,
            format!(
                "move_clip track {} clip {} -> {:.3}s",
                args.track, args.clip_index, args.start_sec
            ),
        ) {
            Ok(id) => id,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        Ok(ToolResult::Ok(json!({
            "node_id": new_id.to_hex(),
            "summary": format!(
                "Moved track {} clip {} to {:.3}s",
                args.track, args.clip_index, args.start_sec
            )
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::recompute_length;
    use session::{Clip, SessionState, Track, TrackId};

    fn state_with(clips: Vec<(u64, u64)>) -> SessionState {
        SessionState {
            tracks: vec![Track {
                id: TrackId::new(),
                name: "t".into(),
                clips: clips
                    .into_iter()
                    .map(|(start, len)| Clip {
                        source_path: "x.wav".into(),
                        start_in_track: start,
                        source_offset: 0,
                        length: len,
                        content_hash: None,
                        time_stretch_factor: None,
                        pitch_shift_semitones: None,
                        beat_grid: None,
                        volume_envelope: Vec::new(),
                    })
                    .collect(),
                gain_db: 0.0,
                pan: 0.0,
                muted: false,
                soloed: false,
                sends: Vec::new(),
                effects: Vec::new(),
            }],
            bus_routing: session::BusGraph::default(),
            master_chain: Vec::new(),
            tempo_map: session::TempoMap::default(),
            key_map: None,
            transcript: None,
            sample_rate: 48_000,
            length_samples: 0,
            annotations: Vec::new(),
            sync_lock: false,
        }
    }

    #[test]
    fn length_is_the_furthest_clip_end() {
        let mut s = state_with(vec![(0, 100), (500, 200)]);
        recompute_length(&mut s);
        assert_eq!(s.length_samples, 700);
    }

    /// Not the last clip in the vector — the furthest one. A clip moved
    /// earlier leaves an earlier-indexed clip reaching further.
    #[test]
    fn length_does_not_assume_the_last_clip_is_the_furthest() {
        let mut s = state_with(vec![(500, 200), (0, 100)]);
        recompute_length(&mut s);
        assert_eq!(s.length_samples, 700);
    }

    #[test]
    fn an_empty_session_has_zero_length() {
        let mut s = state_with(vec![]);
        recompute_length(&mut s);
        assert_eq!(s.length_samples, 0);
    }
}
