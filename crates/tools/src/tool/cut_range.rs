//! `cut_range` — remove samples in `[start_sample, end_sample)` and
//! shift the remainder left.
//!
//! The cut is expressed against the track's *timeline*, so it has to
//! consider every clip on it. A clip wholly before the cut is untouched,
//! one wholly after slides left by the cut's length, and one straddling
//! it contributes whichever of its two ends survive — which is how an
//! interior cut comes to leave two clips behind in the first place.

use serde::Deserialize;
use serde_json::{json, Value};

use crate::schema::{anthropic_tool, object_schema};
use crate::tool::util::{
    append_state, check_sample_range, check_track_index, cut_timeline, load_head_state,
    remap_after_cut, sync_other_tracks, timeline_end,
};
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
        // The timeline's end, not the longest single clip: on a track
        // already split by an earlier cut those differ, and measuring by
        // the longest clip rejected ranges that are squarely inside the
        // track.
        let track_len = timeline_end(&track.clips);
        let (start, end) = match check_sample_range(args.start_sample, args.end_sample, track_len) {
            Ok(p) => p,
            Err(msg) => return Ok(ToolResult::Error(msg)),
        };

        let cut_len = end - start;

        if track.clips.is_empty() {
            return Ok(ToolResult::Error(
                "track has no clips; nothing to cut".into(),
            ));
        }

        track.clips = cut_timeline(&track.clips, start, end);

        // Sync-lock (#170 §3): an interview is one track per speaker,
        // and cutting a sentence out of one of them desynchronises the
        // conversation unless the others lose the same span. One node,
        // not one per track — undo has to put the whole edit back.
        let synced = if state.sync_lock {
            sync_other_tracks(&mut state, args.track, |clips| {
                cut_timeline(clips, start, end)
            })
        } else {
            0
        };

        // Labels name moments in the recording, not offsets in a file
        // (#203). Leaving them put would rename every chapter after the
        // cut to something `cut_len` seconds off.
        //
        // The transcript is the same kind of record and was the half
        // nobody moved (#231): `cut_words` turns a word's `start_s` into
        // a sample offset, so a stale transcript makes the *next* text
        // edit delete audio elsewhere while naming the right word.
        let sr = state.sample_rate.max(1) as f64;
        let dropped = remap_after_cut(&mut state, start as f64 / sr, end as f64 / sr);

        state.length_samples = state
            .tracks
            .iter()
            .map(|t| timeline_end(&t.clips))
            .max()
            .unwrap_or(0);

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
            "synced_tracks": synced,
            // Worth naming: a dropped label is user-authored text that
            // is now gone, and the only place that can be said is here.
            "dropped_labels": dropped,
            "summary": format!(
                "Cut [{}, {}) ({} samples) from track {}{}; new head {}",
                args.start_sample,
                args.end_sample,
                cut_len,
                args.track,
                if synced > 0 {
                    format!(" and {synced} other track(s), sync-locked")
                } else {
                    String::new()
                },
                new_id.to_hex(),
            ),
        })))
    }
}

#[cfg(test)]
mod tests {
    use crate::tool::util::slice_envelope;
    use session::EnvelopePoint;

    fn fade_out(len: u64) -> Vec<EnvelopePoint> {
        vec![
            EnvelopePoint {
                time_samples: 0,
                gain_db: 0.0,
            },
            EnvelopePoint {
                time_samples: len,
                gain_db: -20.0,
            },
        ]
    }

    fn gain_at(points: &[EnvelopePoint], frame: u64) -> f32 {
        if points.is_empty() {
            return 0.0;
        }
        if frame <= points[0].time_samples {
            return points[0].gain_db;
        }
        let last = &points[points.len() - 1];
        if frame >= last.time_samples {
            return last.gain_db;
        }
        let pos = points.partition_point(|p| p.time_samples <= frame);
        let a = &points[pos - 1];
        let b = &points[pos];
        let alpha = (frame - a.time_samples) as f32 / (b.time_samples - a.time_samples) as f32;
        a.gain_db + alpha * (b.gain_db - a.gain_db)
    }

    /// Both halves of an interior cut keep the automation that applied to
    /// them. This used to be `Vec::new()` on both sides — the cut reset
    /// the clip to unity gain and nothing said so.
    #[test]
    fn an_interior_cut_keeps_the_automation_on_both_halves() {
        let original = fade_out(1000);

        // Cut [400, 600). Head keeps [0,400); tail is [600,1000)
        // re-based to zero.
        let head = slice_envelope(&original, 0, 400);
        let tail = slice_envelope(&original, 600, 400);

        assert!(!head.is_empty(), "head lost its automation");
        assert!(!tail.is_empty(), "tail lost its automation");

        let head_end = gain_at(&head, 399);
        let want_head_end = gain_at(&original, 399);
        assert!(
            (head_end - want_head_end).abs() < 0.01,
            "head should still read {want_head_end} dB at its end, got {head_end}"
        );

        let tail_start = gain_at(&tail, 0);
        let want_tail_start = gain_at(&original, 600);
        assert!(
            (tail_start - want_tail_start).abs() < 0.01,
            "tail should resume at {want_tail_start} dB, got {tail_start}"
        );
        assert!(
            tail_start < -1.0,
            "{tail_start} dB at the resume point looks like a reset to the start of \
             the curve"
        );
    }
}
