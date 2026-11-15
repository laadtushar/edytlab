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
use crate::tool::time_edit::{apply, Clips, Scope, SyncLock, TimeEdit, Words};
use crate::tool::util::{
    append_state, check_sample_range, load_track_state_and_rate, parse_args, set_session_length,
    snap_range, timeline_end,
};
use crate::{Tool, ToolContext, ToolResult};

#[derive(Debug, Deserialize)]
struct Args {
    track: usize,
    start_sample: u64,
    end_sample: u64,
    #[serde(default)]
    snap_to_zero_crossing: bool,
}

/// The scope of a cut: it slides the track's clips, carries the same seconds to
/// the other tracks under sync-lock (#170 §3) and moves the labels and words
/// with the audio. Shared by `cut_range` and `edit_at_markers`, so the two
/// cannot differ.
pub(crate) fn cut_scope(track: usize) -> Scope {
    Scope::new(track, SyncLock::Follows, Clips::Slide, Words::Follow)
}

/// Remove the frames `[start, end)` of `track` from `state` at `rate`, the
/// edited track's clip rate (see `Scope::rates`). Returns the
/// [`TimeEditReport`](crate::tool::time_edit::TimeEditReport): how many other tracks
/// sync-lock carried the cut to and how many labels it dropped.
///
/// Does not set the session length; the caller does that once, after its last
/// cut.
pub(crate) fn cut_span(
    state: &mut session::SessionState,
    scope: &Scope,
    rates: &crate::tool::time_edit::Rates,
    rate: f64,
    start: u64,
    end: u64,
) -> crate::tool::time_edit::TimeEditReport {
    let edit = TimeEdit::Cut {
        start: start as f64 / rate,
        end: end as f64 / rate,
    };
    apply(state, edit, scope, rates)
}

pub struct CutRangeTool;

impl Tool for CutRangeTool {
    fn name(&self) -> &'static str {
        "cut_range"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "cut_range",
            "Remove [start_sample, end_sample) from a track and shift the rest left. Appends a new session node parented to the current head.",
            object_schema(&[
                ("track", "integer", true),
                ("start_sample", "integer", true),
                ("end_sample", "integer", true),
                ("snap_to_zero_crossing", "boolean", false),
            ]),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match parse_args(args) {
            Ok(a) => a,
            Err(r) => return Ok(r),
        };

        let (mut state, rate) = match load_track_state_and_rate(ctx, args.track) {
            Ok(p) => p,
            Err(msg) => return Ok(ToolResult::Error(msg)),
        };
        // A sync-locked track gets the same seconds in its own frames.
        let scope = cut_scope(args.track);
        let rates = match scope.rates(&state) {
            Ok(r) => r,
            Err(msg) => return Ok(ToolResult::Error(msg)),
        };
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

        let (start, end, snap_note) = if args.snap_to_zero_crossing {
            snap_range(&track.clips, start, end, track_len)
        } else {
            (start, end, None)
        };
        let cut_len = end - start;

        if track.clips.is_empty() {
            return Ok(ToolResult::Error(
                "track has no clips; nothing to cut".into(),
            ));
        }

        let report = cut_span(&mut state, &scope, &rates, rate, start, end);
        let (synced, dropped) = (report.synced_tracks, report.dropped_labels);

        set_session_length(&mut state);

        let new_id = match append_state(
            ctx,
            state,
            format!("cut_range track {} [{}..{})", args.track, start, end),
        ) {
            Ok(id) => id,
            Err(msg) => return Ok(ToolResult::Error(msg)),
        };

        let mut out = json!({
            "node_id": new_id.to_hex(),
            "removed_samples": cut_len,
            "synced_tracks": synced,
            // Worth naming: a dropped label is user-authored text that
            // is now gone, and the only place that can be said is here.
            "dropped_labels": dropped,
            "summary": format!(
                "Cut [{}, {}) ({} samples) from track {}{}; new head {}",
                start,
                end,
                cut_len,
                args.track,
                if synced > 0 {
                    format!(" and {synced} other track(s), sync-locked")
                } else {
                    String::new()
                },
                new_id.to_hex(),
            ),
        });
        if let Some(note) = snap_note {
            out["snap_note"] = note.into();
        }
        Ok(ToolResult::Ok(out))
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
