//! `cut_range` — remove samples in `[start_sample, end_sample)` and
//! shift the remainder left.
//!
//! Phase 1 only ever has 0..1 clips per track; we model the cut by
//! splitting the clip into `[0, start)` and `[end, length)` segments
//! that point back at the same source file with adjusted
//! `source_offset` / `length`. The audio engine's `RenderGraph` only
//! reads the FIRST clip today, so to keep render output correct we
//! splice the source into ONE clip when the cut is at the head or tail
//! of the existing clip; for an interior cut we still emit two clips,
//! understanding the engine will need a Phase 2 update to honor the
//! second segment. The acceptance test (cut at the tail) is the demo
//! path and works with single-clip rendering today.

use serde::Deserialize;
use serde_json::{json, Value};
use session::Clip;

use crate::schema::{anthropic_tool, object_schema};
use crate::tool::util::{append_state, check_sample_range, check_track_index, load_head_state};
use crate::{Tool, ToolContext, ToolResult};

#[derive(Debug, Deserialize)]
struct Args {
    track: usize,
    start_sample: u64,
    end_sample: u64,
}

pub struct CutRangeTool;

impl Tool for CutRangeTool {
    fn name(&self) -> &'static str {
        "cut_range"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "cut_range",
            "Remove the half-open sample range [start_sample, end_sample) from a track and shift the remainder left. Appends a new session node parented to the current head.",
            object_schema(&[
                ("track", "integer", true),
                ("start_sample", "integer", true),
                ("end_sample", "integer", true),
            ]),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };

        let mut state = match load_head_state(ctx) {
            Ok(s) => s,
            Err(msg) => return Ok(ToolResult::Error(msg)),
        };

        if let Err(msg) = check_track_index(&state.tracks, args.track) {
            return Ok(ToolResult::Error(msg));
        }

        let track = &mut state.tracks[args.track];
        let track_len = track.clips.iter().map(|c| c.length).max().unwrap_or(0);
        let (start, end) = match check_sample_range(args.start_sample, args.end_sample, track_len) {
            Ok(p) => p,
            Err(msg) => return Ok(ToolResult::Error(msg)),
        };

        let cut_len = end - start;

        // Phase 1: single-clip track. Rewrite the clip's source_offset/length
        // to splice out the range. If the cut is purely at the tail/head
        // we keep one clip; an interior cut emits two clips, with the second
        // shifted to start_in_track == start. The render engine reads the
        // first clip today, so we order them by start_in_track for clarity.
        let Some(clip) = track.clips.first().cloned() else {
            return Ok(ToolResult::Error(
                "track has no clips; nothing to cut".into(),
            ));
        };

        let mut new_clips: Vec<Clip> = Vec::new();
        if start > 0 {
            new_clips.push(Clip {
                source_path: clip.source_path.clone(),
                start_in_track: clip.start_in_track,
                source_offset: clip.source_offset,
                length: start,
                content_hash: clip.content_hash,
                // Preserve any M20 time/pitch/beat metadata on each
                // half of the cut.
                time_stretch_factor: clip.time_stretch_factor,
                pitch_shift_semitones: clip.pitch_shift_semitones,
                beat_grid: clip.beat_grid.clone(),
                volume_envelope: Vec::new(),
            });
        }
        if end < clip.length {
            // After the cut, the second segment moves left by `cut_len`.
            new_clips.push(Clip {
                source_path: clip.source_path.clone(),
                start_in_track: clip.start_in_track + start,
                source_offset: clip.source_offset + end,
                length: clip.length - end,
                content_hash: clip.content_hash,
                time_stretch_factor: clip.time_stretch_factor,
                pitch_shift_semitones: clip.pitch_shift_semitones,
                beat_grid: clip.beat_grid.clone(),
                volume_envelope: Vec::new(),
            });
        }

        track.clips = new_clips;
        state.length_samples = state.length_samples.saturating_sub(cut_len);

        let new_id = match append_state(
            ctx,
            state,
            format!(
                "cut_range track {} [{}..{})",
                args.track, args.start_sample, args.end_sample
            ),
        ) {
            Ok(id) => id,
            Err(msg) => return Ok(ToolResult::Error(msg)),
        };

        Ok(ToolResult::Ok(json!({
            "node_id": new_id.to_hex(),
            "removed_samples": cut_len,
            "summary": format!(
                "Cut [{}, {}) ({} samples) from track {}; new head {}",
                args.start_sample, args.end_sample, cut_len, args.track, new_id.to_hex(),
            ),
        })))
    }
}
