//! `edit_at_markers` — cut, delete, silence, split or join the audio under
//! one or more markers (#766 item 2).
//!
//! A podcast's workflow is marker-driven: mark the ums, the false start, the
//! ad break, then act on every mark at once. The tool is a reader of markers
//! over the internals of the time-range tools and adds no audio code of its
//! own: a cut is [`cut_span`] (what `cut_range` calls), a silence is
//! [`silence_spans`] (`silence_region`), a split is `split_clip`'s
//! [`split_at`](crate::tool::split_clip) and a join is [`join_run`]
//! (`join_clips`). So the same span edited under a marker and by its time
//! leaves the same session, which `tests/marker_audio_edits.rs` pins byte for
//! byte.
//!
//! **What a marker means.** A region is its own span, `[start, end)` seconds.
//! A point marker has no length, so the span actions (`cut`, `delete`,
//! `silence`, `join`) refuse it unless `width` gives one (`[time, time +
//! width)`); `split` needs only a time, so a region splits at both of its
//! edges and a point at its time, and `width` does not apply to it.
//!
//! **What each action does.**
//!
//! * `cut` removes the span and closes the gap, as `cut_range` does: sync-lock
//!   carries it to the other tracks, the labels and (for a locked track) the
//!   transcript follow it. A region marker that was cut collapses to nothing
//!   and is dropped with its audio (counted in `dropped_labels`); a point
//!   marker at the cut's first instant stays at the seam, as `cut_range`
//!   keeps a mark on a cut's edge.
//! * `delete` removes the span and leaves a gap of the same length where it
//!   was: nothing after it moves, so no label does either, and sync-lock
//!   takes the same span from the other tracks. (`silence` keeps the clip and
//!   zeroes its samples; `delete` leaves no audio there at all.)
//! * `silence` zeroes the span of this track, as `silence_region`. It moves
//!   nothing and, like `silence_region`, does not follow sync-lock.
//! * `split` cuts the clip playing at each time in two. A time that is a
//!   clip's edge already, or that no clip plays at, splits nothing and is
//!   counted in `skipped`; a call that splits nothing is refused.
//! * `join` makes the clips lying wholly under the span one, as `join_clips`
//!   does (nothing rendered when they are back to back in one file). It needs
//!   at least two, in one unbroken run.
//!
//! **A call changes everything or nothing**, and appends one node however
//! many markers it names: a marker that is unknown, a name that fits more than
//! one, a point without a width, a span past the end of the track and two
//! spans that overlap are all refused before anything is touched. A name is
//! matched whole, in any case (`find_markers` finds by part of a name); an id
//! is exact. The spans are worked latest first, so an earlier span is still
//! where it was when its turn comes.

use serde::Deserialize;
use serde_json::{json, Value};
use session::{Annotation, AnnotationId, AnnotationKind, SessionState};
use uuid::Uuid;

use crate::schema::anthropic_tool;
use crate::tool::cut_range::{cut_scope, cut_span};
use crate::tool::join_clips::join_run;
use crate::tool::markers::{echo, names_text, MAX_BATCH};
use crate::tool::silence_region::silence_spans;
use crate::tool::split_clip::{split_track_at, SplitOutcome};
use crate::tool::time_edit::{apply, Rates, Scope, TimeEdit};
use crate::tool::util::{
    append_state, check_sample_range, check_seconds_order, check_track_index, load_head_state,
    parse_args, seconds_to_clip_frames, seconds_to_frames, set_session_length, timeline_end,
    track_clip_rate,
};
use crate::{Tool, ToolContext, ToolResult};

/// What to do to the audio under the markers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Action {
    Cut,
    Delete,
    Silence,
    Split,
    Join,
}

impl Action {
    fn as_str(self) -> &'static str {
        match self {
            Self::Cut => "cut",
            Self::Delete => "delete",
            Self::Silence => "silence",
            Self::Split => "split",
            Self::Join => "join",
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    action: Action,
    track: usize,
    markers: Vec<String>,
    width: Option<f64>,
}

/// A marker's stretch of the timeline, in session seconds.
#[derive(Debug, Clone)]
struct Span {
    name: String,
    start: f64,
    end: f64,
}

impl Span {
    /// `'name' (1.000-2.000 s)`, for a refusal.
    fn describe(&self) -> String {
        format!(
            "'{}' ({:.3}-{:.3} s)",
            echo(&self.name),
            self.start,
            self.end
        )
    }
}

fn refuse(message: impl Into<String>) -> crate::Result<ToolResult> {
    Ok(ToolResult::Error(message.into()))
}

/// The marker `reference` means: the one with that id, or the one with that
/// name (whole, any case). Refuses when none does and when several do, with
/// the ids to choose from.
fn resolve<'a>(state: &'a SessionState, reference: &str) -> Result<&'a Annotation, String> {
    let wanted = reference.trim();
    if let Ok(id) = Uuid::parse_str(wanted).map(AnnotationId) {
        if let Some(found) = state.annotations.iter().find(|a| a.id == id) {
            return Ok(found);
        }
    }
    let lower = wanted.to_lowercase();
    let named: Vec<&Annotation> = state
        .annotations
        .iter()
        .filter(|a| a.name.trim().to_lowercase() == lower)
        .collect();
    match named.as_slice() {
        [one] => Ok(one),
        [] => Err(format!(
            "no marker is named or has the id '{}'; `find_markers` lists them",
            echo(reference)
        )),
        many => {
            let ids: Vec<String> = many.iter().map(|a| a.id.0.to_string()).collect();
            Err(format!(
                "{} markers are named '{}'; pass the id of the one you mean: {}",
                many.len(),
                echo(reference),
                ids.join(", ")
            ))
        }
    }
}

/// The markers a call names, each once, in the order given.
fn resolve_all(state: &SessionState, references: &[String]) -> Result<Vec<Annotation>, String> {
    if references.is_empty() {
        return Err("`markers` is empty; name at least one marker".into());
    }
    if references.len() > MAX_BATCH {
        return Err(format!(
            "that names {} markers; the most one call takes is {MAX_BATCH}",
            references.len()
        ));
    }
    let mut found: Vec<Annotation> = Vec::with_capacity(references.len());
    for reference in references {
        let marker = resolve(state, reference)?;
        if !found.iter().any(|f| f.id == marker.id) {
            found.push(marker.clone());
        }
    }
    Ok(found)
}

/// The span a marker stands for: a region's own, or a point's with `width`.
fn span_of(marker: &Annotation, width: Option<f64>) -> Result<Span, String> {
    let (start, end) = match marker.kind {
        AnnotationKind::Region { start_sec, end_sec } => (start_sec, end_sec),
        AnnotationKind::Marker { time_sec } => match width {
            Some(w) if w.is_finite() && w > 0.0 => (time_sec, time_sec + w),
            Some(w) => {
                return Err(format!(
                    "`width` must be a positive, finite number of seconds; got {w}"
                ))
            }
            None => {
                return Err(format!(
                    "'{}' is a point marker, with no length to act on: pass `width` (seconds from \
                     the marker) or use action split",
                    echo(&marker.name)
                ))
            }
        },
    };
    check_seconds_order(start, end).map_err(|e| format!("'{}': {e}", echo(&marker.name)))?;
    Ok(Span {
        name: marker.name.clone(),
        start,
        end,
    })
}

/// The spans of every marker, in time order, refusing a pair that overlap
/// (spans that only touch are fine).
fn spans_of(markers: &[Annotation], width: Option<f64>) -> Result<Vec<Span>, String> {
    let mut spans = markers
        .iter()
        .map(|m| span_of(m, width))
        .collect::<Result<Vec<_>, _>>()?;
    spans.sort_by(|a, b| a.start.total_cmp(&b.start));
    for pair in spans.windows(2) {
        if pair[1].start < pair[0].end {
            return Err(format!(
                "{} and {} overlap; act on one of them, or on markers that do not",
                pair[0].describe(),
                pair[1].describe()
            ));
        }
    }
    Ok(spans)
}

/// The times a split is made at, from earliest, each once: a region's two
/// edges, a point's time.
fn split_times(markers: &[Annotation]) -> Result<Vec<(String, f64)>, String> {
    let mut times = Vec::new();
    for m in markers {
        let at = match m.kind {
            AnnotationKind::Marker { time_sec } => vec![time_sec],
            AnnotationKind::Region { start_sec, end_sec } => vec![start_sec, end_sec],
        };
        for t in at {
            if !t.is_finite() || t < 0.0 {
                return Err(format!(
                    "'{}' is at {t} s; a split needs a time of 0 or more",
                    echo(&m.name)
                ));
            }
            times.push((m.name.clone(), t));
        }
    }
    times.sort_by(|a, b| a.1.total_cmp(&b.1));
    times.dedup_by(|b, a| a.1 == b.1);
    Ok(times)
}

/// A span in the edited track's own frames, checked against the track's end
/// as `cut_range` checks its samples.
fn frames_of(span: &Span, rate: f64, track_len: u64) -> Result<(u64, u64), String> {
    let (start, end) = (
        seconds_to_frames(span.start, rate),
        seconds_to_frames(span.end, rate),
    );
    check_sample_range(start, end, track_len).map_err(|e| format!("{}: {e}", span.describe()))
}

/// Remove `[start, end)` of `track`, in its own frames at `rate`, and leave a
/// gap of that length where it was, in `state`; sync-lock does the same to the
/// other tracks, by seconds. Returns how many other tracks it reached. No label
/// or word moves: nothing after the span did.
fn delete_span(
    state: &mut SessionState,
    scope: &Scope,
    rates: &Rates,
    rate: f64,
    (start, end): (u64, u64),
) -> usize {
    let edit = TimeEdit::Hollow {
        start: start as f64 / rate,
        end: end as f64 / rate,
    };
    apply(state, edit, scope, rates).synced_tracks
}

/// The run of clips of `track` lying wholly under `span`, as indices, in the
/// clips' own frames (#234). Refuses fewer than two, and clips that are not
/// one unbroken run (one that is not wholly under the span lies between).
fn clips_under(state: &SessionState, track: usize, span: &Span) -> Result<(usize, usize), String> {
    let mut under = Vec::new();
    for (i, clip) in state.tracks[track].clips.iter().enumerate() {
        let (from, to) = (
            seconds_to_clip_frames(clip, span.start)?,
            seconds_to_clip_frames(clip, span.end)?,
        );
        if clip.start_in_track >= from && clip.start_in_track.saturating_add(clip.length) <= to {
            under.push(i);
        }
    }
    let (Some(&first), Some(&last)) = (under.first(), under.last()) else {
        return Err(format!(
            "no clip of track {track} lies wholly under {}; a join needs at least two (split at \
             its edges first)",
            span.describe()
        ));
    };
    if under.len() < 2 {
        return Err(format!(
            "only one clip of track {track} lies wholly under {}; a join needs at least two",
            span.describe()
        ));
    }
    if last - first + 1 != under.len() {
        return Err(format!(
            "the clips of track {track} under {} are not one run: a clip that is not wholly under \
             it lies between them",
            span.describe()
        ));
    }
    Ok((first, last))
}

/// The `edit_at_markers` tool: cut, delete, silence, split or join the audio
/// under markers (see the module).
pub struct EditAtMarkersTool;

impl Tool for EditAtMarkersTool {
    fn name(&self) -> &'static str {
        "edit_at_markers"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "edit_at_markers",
            "Edit the audio under markers/regions, named or by id (find_markers). cut: remove and \
             close the gap (as cut_range; sync-lock and labels follow). delete: remove, leave a \
             gap. silence: zero (as silence_region). split: cut clips at the marker's edges. \
             join: make the clips under it one (as join_clips). A point marker needs width \
             seconds, except for split. All markers = one undo step; any problem changes nothing.",
            json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["cut", "delete", "silence", "split", "join"] },
                    "track": { "type": "integer" },
                    "markers": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Marker names (whole, any case) or ids"
                    },
                    "width": { "type": "number", "description": "Seconds a point marker spans" }
                },
                "required": ["action", "track", "markers"],
                "additionalProperties": false
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match parse_args(args) {
            Ok(a) => a,
            Err(r) => return Ok(r),
        };
        let state = match load_head_state(ctx) {
            Ok(s) => s,
            Err(e) => return refuse(e),
        };
        let markers = match resolve_all(&state, &args.markers) {
            Ok(m) => m,
            Err(e) => return refuse(e),
        };
        if let Err(e) = check_track_index(&state.tracks, args.track) {
            return refuse(e);
        }
        let label = format!(
            "edit_at_markers {} track {} [{}]",
            args.action.as_str(),
            args.track,
            names_text(markers.iter().map(|m| m.name.as_str()))
        );
        let result = match args.action {
            Action::Split => split(ctx, state, &args, &markers, label),
            _ => match spans_of(&markers, args.width) {
                Ok(spans) => match args.action {
                    Action::Cut | Action::Delete => remove(ctx, state, &args, &spans, label),
                    Action::Silence => Ok(silence(ctx, &args, &spans, label)),
                    _ => join(ctx, state, &args, &spans, label),
                },
                Err(e) => Err(e),
            },
        };
        match result {
            Ok(r) => Ok(r),
            Err(e) => refuse(e),
        }
    }
}

/// `cut` and `delete`: both remove the span from every track the edit
/// reaches, latest first, then differ in whether the later audio moves.
fn remove(
    ctx: &mut ToolContext,
    mut state: SessionState,
    args: &Args,
    spans: &[Span],
    label: String,
) -> Result<ToolResult, String> {
    let rate = track_clip_rate(&state.tracks[args.track], state.sample_rate)?;
    let scope = cut_scope(args.track);
    let rates = scope.rates(&state)?;
    let track_len = timeline_end(&state.tracks[args.track].clips);
    if state.tracks[args.track].clips.is_empty() {
        return Err(format!(
            "track {} has no clips; nothing to {}",
            args.track,
            args.action.as_str()
        ));
    }
    let frames = spans
        .iter()
        .map(|s| frames_of(s, rate, track_len))
        .collect::<Result<Vec<_>, _>>()?;

    let (mut synced, mut dropped, mut removed) = (0, 0, 0u64);
    // Latest first: an earlier span is before every cut made, so it is
    // still at its own frames when its turn comes.
    for &(start, end) in frames.iter().rev() {
        removed += end - start;
        if args.action == Action::Cut {
            let report = cut_span(&mut state, &scope, &rates, rate, start, end);
            synced = synced.max(report.synced_tracks);
            dropped += report.dropped_labels;
        } else {
            synced = synced.max(delete_span(&mut state, &scope, &rates, rate, (start, end)));
        }
    }
    set_session_length(&mut state);
    let new_id = append_state(ctx, state, label)?;

    let verb = if args.action == Action::Cut {
        "Cut"
    } else {
        "Deleted"
    };
    let mut out = json!({
        "node_id": new_id.to_hex(),
        "action": args.action.as_str(),
        "spans": spans.len(),
        "removed_samples": removed,
        "synced_tracks": synced,
        "summary": format!(
            "{verb} {} marker span(s) ({removed} samples) from track {}{}; new head {}",
            spans.len(),
            args.track,
            if synced > 0 {
                format!(" and {synced} other track(s), sync-locked")
            } else {
                String::new()
            },
            new_id.to_hex(),
        ),
    });
    if args.action == Action::Cut {
        // A dropped label is user-authored text that is now gone; this is
        // the only place that can be said (as `cut_range` does).
        out["dropped_labels"] = dropped.into();
    }
    Ok(ToolResult::Ok(out))
}

/// `silence`: every span zeroed in one pass, by `silence_region`'s code.
fn silence(ctx: &mut ToolContext, args: &Args, spans: &[Span], label: String) -> ToolResult {
    let seconds: Vec<(f64, f64)> = spans.iter().map(|s| (s.start, s.end)).collect();
    match silence_spans(ctx, args.track, seconds, label) {
        ToolResult::Ok(mut out) => {
            out["action"] = "silence".into();
            out["spans"] = spans.len().into();
            ToolResult::Ok(out)
        }
        refused => refused,
    }
}

/// `split`: the clip playing at each time becomes two.
fn split(
    ctx: &mut ToolContext,
    mut state: SessionState,
    args: &Args,
    markers: &[Annotation],
    label: String,
) -> Result<ToolResult, String> {
    let times = split_times(markers)?;
    let (mut splits, mut skipped) = (0, 0);
    for (_, at) in &times {
        match split_track_at(&mut state, args.track, *at)? {
            SplitOutcome::Split => splits += 1,
            SplitOutcome::Edge | SplitOutcome::Gap => skipped += 1,
        }
    }
    if splits == 0 {
        return Err(format!(
            "nothing to split: every one of the {} time(s) is already a clip edge of track {} or \
             has no clip playing at it",
            times.len(),
            args.track
        ));
    }
    let new_id = append_state(ctx, state, label)?;
    Ok(ToolResult::Ok(json!({
        "node_id": new_id.to_hex(),
        "action": "split",
        "splits": splits,
        "skipped": skipped,
        "summary": format!(
            "Split track {} at {splits} point(s) ({skipped} already an edge or in a gap); new head {}",
            args.track,
            new_id.to_hex()
        ),
    })))
}

/// `join`: the clips under each span become one, latest span first so the
/// indices of an earlier one still hold.
fn join(
    ctx: &mut ToolContext,
    mut state: SessionState,
    args: &Args,
    spans: &[Span],
    label: String,
) -> Result<ToolResult, String> {
    let (mut joined, mut rendered) = (0, false);
    for span in spans.iter().rev() {
        let (first, last) = clips_under(&state, args.track, span)?;
        let run = join_run(ctx, &mut state, args.track, first, last)?;
        joined += run.clips;
        rendered |= !run.merged;
    }
    let new_id = append_state(ctx, state, label)?;
    Ok(ToolResult::Ok(json!({
        "node_id": new_id.to_hex(),
        "action": "join",
        "spans": spans.len(),
        "joined_clips": joined,
        "rendered": rendered,
        "summary": format!(
            "Joined {joined} clip(s) of track {} under {} marker span(s){}; new head {}",
            args.track,
            spans.len(),
            if rendered { " (rendered into a new file)" } else { "" },
            new_id.to_hex()
        ),
    })))
}
