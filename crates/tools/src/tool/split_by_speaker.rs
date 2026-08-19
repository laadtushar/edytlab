//! `split_by_speaker` — turn one track and a set of speaker segments
//! into one track per speaker (#168 §2).
//!
//! This is the half of diarization that is not a model. The model's job
//! is to say *who spoke when*; this tool's job is to turn that answer
//! into an arrangement you can actually mix — separate gain, EQ and
//! noise treatment per voice, which is the thing that makes every
//! subsequent edit on an interview easier.
//!
//! Splitting them apart is worth having on its own, because the
//! segments do not have to come from a model. A producer with a rough
//! cue sheet, or an agent reading a transcript that already names
//! speakers, can supply them just as well.
//!
//! ## No new audio is written
//!
//! The obvious implementation renders one WAV per speaker. This one
//! writes nothing: each speaker's track is a set of [`Clip`]s pointing
//! at the *same* source file, with `source_offset` and `length` framing
//! that speaker's turns.
//!
//! That is not only cheaper — a five-minute stereo interview costs
//! ~55 MB per rendered copy, and #98 exists because those accumulate —
//! it is what makes the acceptance criterion true *by construction*
//! rather than by measurement: combined playback is sample-identical to
//! the original because it is playing the original samples, from the
//! same file, at the same timeline positions.
//!
//! It also makes the boundaries editable for free. A speaker turn is an
//! ordinary clip, so the clip strip already draws it, already lets you
//! drag it, and (since #217) already animates it moving. Diarization is
//! never perfect, and the correction path was the part that would
//! otherwise need building.
//!
//! ## Every sample lands in exactly one track
//!
//! Two things would break the identity property, and both are handled
//! rather than assumed away:
//!
//! - **Gaps.** Real diarization marks speech, not silence, so audio
//!   between turns belongs to nobody. Dropping it would quietly lose
//!   room tone, breaths and music. It goes to an `unassigned` track.
//! - **Overlaps.** Real diarization also emits overlapping turns,
//!   because people talk over each other. A sample copied into two
//!   tracks would play twice and sum to double. Each sample goes to the
//!   first segment covering it, and the overlaps are *reported* in the
//!   result rather than silently resolved — the user should know their
//!   crosstalk landed on one speaker.
//!
//! ## What it refuses
//!
//! A clip carrying a time-stretch factor, a pitch shift or a beat grid
//! is rejected rather than split. Those describe a transformation of a
//! whole clip, and there is no honest answer for what half of one
//! means — a beat grid is stated relative to the clip start, so slicing
//! silently reinterprets every entry in it. A volume envelope is
//! refused for the same reason.
//!
//! This is deliberately a refusal and not a best effort: the failure
//! mode of guessing is a session that renders differently than it did
//! before, which is the one thing an edit must never do quietly.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::{json, Value};
use session::state::{Clip, Track, TrackId};

use crate::schema::anthropic_tool;
use crate::tool::util::{append_state, check_track_index, load_head_state, timeline_end};
use crate::{Tool, ToolContext, ToolResult};

/// Default name for audio no segment claims.
const UNASSIGNED: &str = "unassigned";

#[derive(Debug, Deserialize)]
struct Segment {
    start_sec: f64,
    end_sec: f64,
    speaker: String,
}

#[derive(Debug, Deserialize)]
struct Args {
    track: usize,
    segments: Vec<Segment>,
    #[serde(default)]
    unassigned_name: Option<String>,
}

/// A half-open frame range on the track timeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: u64,
    pub end: u64,
}

impl Span {
    fn is_empty(&self) -> bool {
        self.end <= self.start
    }
}

/// Assign each frame of `[0, total)` to exactly one owner.
///
/// `segments` are `(span, owner)` in the order the caller gave them;
/// the first segment covering a frame wins it. Returns the per-owner
/// spans in first-appearance order, plus the spans nobody claimed.
///
/// Kept separate from the tool so the partitioning — which is the only
/// part with any real logic in it — is testable without a session, a
/// store or a WAV on disk.
pub fn partition(segments: &[(Span, String)], total: u64) -> (Vec<(String, Vec<Span>)>, Vec<Span>) {
    // Order of first appearance, so the output track order matches the
    // order the caller listed speakers in rather than an alphabetical
    // one they did not ask for.
    let mut order: Vec<String> = Vec::new();
    let mut owned: BTreeMap<String, Vec<Span>> = BTreeMap::new();

    // `taken` is the set of frames already claimed, kept as a sorted,
    // disjoint span list. Walking it is O(segments²) in the worst case
    // and segments number in the hundreds, so this stays well inside
    // "obviously correct beats clever".
    let mut taken: Vec<Span> = Vec::new();

    for (span, owner) in segments {
        let span = Span {
            start: span.start,
            end: span.end.min(total),
        };
        if span.is_empty() {
            continue;
        }
        let free = subtract(span, &taken);
        if free.is_empty() {
            continue;
        }
        if !owned.contains_key(owner) {
            order.push(owner.clone());
        }
        owned.entry(owner.clone()).or_default().extend(&free);
        for f in &free {
            taken.push(*f);
        }
        taken.sort_by_key(|s| s.start);
        taken = coalesce(&taken);
    }

    let leftover = subtract(
        Span {
            start: 0,
            end: total,
        },
        &taken,
    );

    let by_owner = order
        .into_iter()
        .map(|o| {
            let mut spans = owned.remove(&o).unwrap_or_default();
            spans.sort_by_key(|s| s.start);
            (o, coalesce(&spans))
        })
        .collect();

    (by_owner, leftover)
}

/// `span` minus every span in `taken` (which must be sorted+disjoint).
fn subtract(span: Span, taken: &[Span]) -> Vec<Span> {
    let mut out = Vec::new();
    let mut cursor = span.start;
    for t in taken {
        if t.end <= cursor {
            continue;
        }
        if t.start >= span.end {
            break;
        }
        if t.start > cursor {
            out.push(Span {
                start: cursor,
                end: t.start.min(span.end),
            });
        }
        cursor = cursor.max(t.end);
        if cursor >= span.end {
            break;
        }
    }
    if cursor < span.end {
        out.push(Span {
            start: cursor,
            end: span.end,
        });
    }
    out.into_iter().filter(|s| !s.is_empty()).collect()
}

/// Merge touching or overlapping spans in a sorted list.
fn coalesce(spans: &[Span]) -> Vec<Span> {
    let mut out: Vec<Span> = Vec::new();
    for s in spans {
        match out.last_mut() {
            Some(last) if s.start <= last.end => last.end = last.end.max(s.end),
            _ => out.push(*s),
        }
    }
    out
}

/// The parts of `clips` that fall inside `span`, as clips keeping their
/// original timeline positions.
///
/// Timeline position is preserved rather than rebased to zero: a
/// speaker's second turn has to stay where it was in the conversation,
/// or the tracks no longer line up with each other.
pub fn slice_clips(clips: &[Clip], span: Span) -> Vec<Clip> {
    let mut out = Vec::new();
    for clip in clips {
        let clip_start = clip.start_in_track;
        let clip_end = clip_start.saturating_add(clip.length);
        let start = clip_start.max(span.start);
        let end = clip_end.min(span.end);
        if end <= start {
            continue;
        }
        let mut sliced = clip.clone();
        sliced.start_in_track = start;
        // How far into this clip the slice begins, added to wherever the
        // clip already started inside its source file.
        sliced.source_offset = clip.source_offset + (start - clip_start);
        sliced.length = end - start;
        out.push(sliced);
    }
    out
}

/// Why a clip cannot be sliced, if it cannot be.
fn unsliceable(clip: &Clip) -> Option<&'static str> {
    if clip.time_stretch_factor.is_some() {
        return Some("a time-stretch factor");
    }
    if clip.pitch_shift_semitones.is_some() {
        return Some("a pitch shift");
    }
    if clip.beat_grid.is_some() {
        return Some("a beat grid");
    }
    if !clip.volume_envelope.is_empty() {
        return Some("a volume envelope");
    }
    None
}

pub struct SplitBySpeakerTool;

impl Tool for SplitBySpeakerTool {
    fn name(&self) -> &'static str {
        "split_by_speaker"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "split_by_speaker",
            "Split one track into a separate track per speaker, given speaker segments in seconds. \
             Each speaker's track references the same audio, so combined playback is unchanged — \
             what changes is that each voice can now be mixed, EQ'd and de-noised on its own. \
             Audio no segment covers goes to an 'unassigned' track so nothing is lost, and \
             overlapping segments are awarded to the first one listed and reported back. \
             Replaces the source track in place. Appends a new session node.",
            json!({
                "type": "object",
                "properties": {
                    "track": {
                        "type": "integer",
                        "description": "Index of the track to split"
                    },
                    "segments": {
                        "type": "array",
                        "description": "Speaker turns. Overlaps are awarded to the earlier entry.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "start_sec": { "type": "number" },
                                "end_sec": { "type": "number" },
                                "speaker": {
                                    "type": "string",
                                    "description": "Speaker label; becomes the track name"
                                }
                            },
                            "required": ["start_sec", "end_sec", "speaker"]
                        }
                    },
                    "unassigned_name": {
                        "type": "string",
                        "description": "Name for the track holding audio no segment covers (default 'unassigned')"
                    }
                },
                "required": ["track", "segments"]
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };

        if args.segments.is_empty() {
            return Ok(ToolResult::Error(
                "segments must not be empty; there is nothing to split on".to_string(),
            ));
        }

        let mut state = match load_head_state(ctx) {
            Ok(s) => s,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        if let Err(e) = check_track_index(&state.tracks, args.track) {
            return Ok(ToolResult::Error(e));
        }

        let source = &state.tracks[args.track];
        for clip in &source.clips {
            if let Some(why) = unsliceable(clip) {
                return Ok(ToolResult::Error(format!(
                    "track {} has a clip carrying {why}, which has no meaning once the clip is \
                     split; flatten or remove it first",
                    args.track
                )));
            }
        }

        let sr = state.sample_rate.max(1) as f64;
        let total = timeline_end(&source.clips);
        if total == 0 {
            return Ok(ToolResult::Error(format!(
                "track {} has no audio to split",
                args.track
            )));
        }

        // Seconds in, frames from here on. Negative and reversed inputs
        // are clamped to empty rather than rejected: a diariser emitting
        // a zero-length turn is noise, not a reason to refuse the whole
        // batch.
        let mut spans: Vec<(Span, String)> = Vec::new();
        for seg in &args.segments {
            let start = (seg.start_sec.max(0.0) * sr).round() as u64;
            let end = (seg.end_sec.max(0.0) * sr).round() as u64;
            spans.push((
                Span {
                    start: start.min(total),
                    end: end.min(total),
                },
                seg.speaker.clone(),
            ));
        }

        // Report overlap before it is resolved, so "your crosstalk went
        // to whoever spoke first" is something the user is told rather
        // than something they discover.
        let overlaps = count_overlaps(&spans);

        let (by_speaker, leftover) = partition(&spans, total);
        if by_speaker.is_empty() {
            return Ok(ToolResult::Error(
                "no segment covered any audio on that track".to_string(),
            ));
        }

        let source = state.tracks[args.track].clone();
        let unassigned_name = args
            .unassigned_name
            .unwrap_or_else(|| UNASSIGNED.to_string());

        let mut new_tracks: Vec<Track> = Vec::new();
        for (speaker, spans) in &by_speaker {
            let clips: Vec<Clip> = spans
                .iter()
                .flat_map(|s| slice_clips(&source.clips, *s))
                .collect();
            new_tracks.push(Track {
                id: TrackId::new(),
                name: speaker.clone(),
                clips,
                // The source track's mix settings carry over to every
                // speaker. Splitting is not the moment to change how
                // anything sounds — that is the *next* edit, and it is
                // the whole reason for splitting.
                gain_db: source.gain_db,
                pan: source.pan,
                muted: source.muted,
                soloed: source.soloed,
                effects: source.effects.clone(),
                sends: source.sends.clone(),
            });
        }

        let leftover_frames: u64 = leftover.iter().map(|s| s.end - s.start).sum();
        if !leftover.is_empty() {
            let clips: Vec<Clip> = leftover
                .iter()
                .flat_map(|s| slice_clips(&source.clips, *s))
                .collect();
            if !clips.is_empty() {
                new_tracks.push(Track {
                    id: TrackId::new(),
                    name: unassigned_name.clone(),
                    clips,
                    gain_db: source.gain_db,
                    pan: source.pan,
                    muted: source.muted,
                    soloed: source.soloed,
                    effects: source.effects.clone(),
                    sends: source.sends.clone(),
                });
            }
        }

        let speaker_count = by_speaker.len();
        let names: Vec<String> = new_tracks.iter().map(|t| t.name.clone()).collect();

        // In place, so the split tracks sit where the original did and
        // the rest of the arrangement keeps its indices.
        state.tracks.splice(args.track..=args.track, new_tracks);

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
                "split_by_speaker track {} → {} speaker(s)",
                args.track, speaker_count
            ),
        ) {
            Ok(id) => id,
            Err(msg) => return Ok(ToolResult::Error(msg)),
        };

        Ok(ToolResult::Ok(json!({
            "node_id": new_id.to_hex(),
            "tracks": names,
            "speakers": speaker_count,
            "unassigned_samples": leftover_frames,
            "overlapping_segments": overlaps,
            "summary": format!(
                "Split track {} into {} speaker track(s): {}{}{}; new head {}",
                args.track,
                speaker_count,
                names.join(", "),
                if leftover_frames > 0 {
                    format!(
                        ". {:.2}s no segment covered went to \"{}\"",
                        leftover_frames as f64 / sr,
                        unassigned_name
                    )
                } else {
                    String::new()
                },
                if overlaps > 0 {
                    format!(
                        ". {overlaps} overlapping segment(s) were awarded to whoever was listed first"
                    )
                } else {
                    String::new()
                },
                new_id.to_hex(),
            ),
        })))
    }
}

/// How many segments overlap an earlier one.
fn count_overlaps(spans: &[(Span, String)]) -> usize {
    let mut n = 0;
    for (i, (a, _)) in spans.iter().enumerate() {
        if a.is_empty() {
            continue;
        }
        if spans[..i]
            .iter()
            .any(|(b, _)| !b.is_empty() && a.start < b.end && b.start < a.end)
        {
            n += 1;
        }
    }
    n
}
