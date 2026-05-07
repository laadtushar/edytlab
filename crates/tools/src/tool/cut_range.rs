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
            "Remove the half-open TRACK-relative sample range [start_sample, end_sample) from a track and shift the remainder left. `start_sample` and `end_sample` are measured against the track timeline (the maximum of `clip.start_in_track + clip.length` across all clips on the track), not against any individual clip. Appends a new session node parented to the current head.",
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
        // Track length is the rightmost clip-end on the timeline, not the
        // length of any single clip. Phase 1 has at most one clip per track
        // (so this is just `start_in_track + length`), but writing it this
        // way keeps the bounds check correct once Phase 2 adds multi-clip
        // tracks without us having to remember to revisit it.
        let track_len = track
            .clips
            .iter()
            .map(|c| c.start_in_track + c.length)
            .max()
            .unwrap_or(0);
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
        // Phase 1: single-clip track. Multi-clip support arrives in Phase 2.
        // Guard explicitly against multi-clip tracks so a future caller (or a
        // bug elsewhere that creates more clips) can't silently drop data:
        // the splice below replaces `track.clips` wholesale and would lose
        // clips 1..n. The render engine also only consumes the first clip
        // today, so even if we left them in place they wouldn't render.
        if track.clips.len() > 1 {
            return Ok(ToolResult::Error(format!(
                "track {} has {} clips; cut_range only supports single-clip tracks in Phase 1 (multi-clip arrives in Phase 2)",
                args.track,
                track.clips.len()
            )));
        }
        let Some(clip) = track.clips.first().cloned() else {
            return Ok(ToolResult::Error(
                "track has no clips; nothing to cut".into(),
            ));
        };

        // Translate the track-relative cut to clip-relative offsets. With the
        // Phase 1 single-clip-at-zero invariant `clip.start_in_track == 0`,
        // these are identical to `start` / `end`.
        let clip_cut_start = start.saturating_sub(clip.start_in_track);
        let clip_cut_end = end.saturating_sub(clip.start_in_track);

        let mut new_clips: Vec<Clip> = Vec::new();
        if clip_cut_start > 0 {
            new_clips.push(Clip {
                source_path: clip.source_path.clone(),
                start_in_track: clip.start_in_track,
                source_offset: clip.source_offset,
                length: clip_cut_start,
                content_hash: clip.content_hash,
            });
        }
        if clip_cut_end < clip.length {
            // After the cut, the second segment moves left by `cut_len`.
            new_clips.push(Clip {
                source_path: clip.source_path.clone(),
                start_in_track: clip.start_in_track + clip_cut_start,
                source_offset: clip.source_offset + clip_cut_end,
                length: clip.length - clip_cut_end,
                content_hash: clip.content_hash,
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
