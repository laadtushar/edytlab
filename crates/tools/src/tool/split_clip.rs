use serde::Deserialize;
use serde_json::{json, Value};
use session::Clip;
use crate::schema::anthropic_tool;
use crate::tool::util::{append_state, check_track_index, load_head_state};
use crate::{Tool, ToolContext, ToolResult};

/// Split `clip` into two clips at `at_frames` frames from the clip's start_in_track.
/// `at_frames` is relative to the clip's start (i.e. within [1, length-1]).
/// Returns (left, right) or Err if split point is outside the clip.
pub(crate) fn split_at(clip: &Clip, at_frames: u64) -> Result<(Clip, Clip), String> {
    if at_frames == 0 || at_frames >= clip.length {
        return Err(format!(
            "split point {at_frames} must be in (0, {})",
            clip.length
        ));
    }
    let left = Clip {
        source_path: clip.source_path.clone(),
        start_in_track: clip.start_in_track,
        source_offset: clip.source_offset,
        length: at_frames,
        content_hash: clip.content_hash,
        time_stretch_factor: clip.time_stretch_factor,
        pitch_shift_semitones: clip.pitch_shift_semitones,
        beat_grid: clip.beat_grid.clone(),
        volume_envelope: clip.volume_envelope.clone(),
    };
    let right = Clip {
        source_path: clip.source_path.clone(),
        start_in_track: clip.start_in_track + at_frames,
        source_offset: clip.source_offset + at_frames,
        length: clip.length - at_frames,
        content_hash: clip.content_hash,
        time_stretch_factor: clip.time_stretch_factor,
        pitch_shift_semitones: clip.pitch_shift_semitones,
        beat_grid: clip.beat_grid.clone(),
        volume_envelope: clip.volume_envelope.clone(),
    };
    Ok((left, right))
}

#[derive(Debug, Deserialize)]
struct Args {
    track: usize,
    clip_index: usize,
    at_sec: f64,
}

pub struct SplitClipTool;

impl Tool for SplitClipTool {
    fn name(&self) -> &'static str {
        "split_clip"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "split_clip",
            "Split a clip into two at the specified time position. Both resulting clips reference the same source file with adjusted offsets. Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
                    "clip_index": {
                        "type": "integer",
                        "description": "Zero-based clip index within the track"
                    },
                    "at_sec": {
                        "type": "number",
                        "description": "Position to split at, in seconds from track start"
                    }
                },
                "required": ["track", "clip_index", "at_sec"]
            }),
        )
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
        let track = &state.tracks[args.track];
        if args.clip_index >= track.clips.len() {
            return Ok(ToolResult::Error(format!(
                "clip_index {} out of range (track has {} clips)",
                args.clip_index,
                track.clips.len()
            )));
        }
        let clip = track.clips[args.clip_index].clone();
        let at_frames = (args.at_sec * state.sample_rate as f64) as u64;
        // Convert absolute track position to offset within clip
        let clip_at = at_frames.saturating_sub(clip.start_in_track);
        let (left, right) = match split_at(&clip, clip_at) {
            Ok(pair) => pair,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        let track = &mut state.tracks[args.track];
        track.clips.remove(args.clip_index);
        track.clips.insert(args.clip_index, right);
        track.clips.insert(args.clip_index, left);
        let new_id = match append_state(
            ctx,
            state,
            format!(
                "split_clip track {} clip {} at {:.3}s",
                args.track, args.clip_index, args.at_sec
            ),
        ) {
            Ok(id) => id,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        Ok(ToolResult::Ok(json!({
            "node_id": new_id.to_hex(),
            "summary": format!(
                "Split track {} clip {} at {:.3}s",
                args.track, args.clip_index, args.at_sec
            )
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::split_at;
    use session::Clip;
    use std::path::PathBuf;

    fn make_clip(start_in_track: u64, source_offset: u64, length: u64) -> Clip {
        Clip {
            source_path: PathBuf::from("/tmp/test.wav"),
            start_in_track,
            source_offset,
            length,
            content_hash: None,
            time_stretch_factor: None,
            pitch_shift_semitones: None,
            beat_grid: None,
            volume_envelope: vec![],
        }
    }

    #[test]
    fn splits_middle() {
        let clip = make_clip(0, 0, 100);
        let (a, b) = split_at(&clip, 40).unwrap();
        assert_eq!(a.start_in_track, 0);
        assert_eq!(a.source_offset, 0);
        assert_eq!(a.length, 40);
        assert_eq!(b.start_in_track, 40);
        assert_eq!(b.source_offset, 40);
        assert_eq!(b.length, 60);
    }

    #[test]
    fn rejects_split_at_start() {
        let clip = make_clip(0, 0, 100);
        assert!(split_at(&clip, 0).is_err());
    }

    #[test]
    fn rejects_split_at_end() {
        let clip = make_clip(0, 0, 100);
        assert!(split_at(&clip, 100).is_err());
    }
}
