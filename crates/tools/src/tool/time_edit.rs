//! A time edit, applied once and everywhere it has to be (ADR 0004, #715).
//!
//! An edit that shifts time can move four things: the edited track's clips,
//! the other tracks (under sync-lock), the labels, and the transcript's
//! words. Each tool used to pick its own subset of helpers, and the fixes
//! came one tool at a time (#203, #231, #278, #543, #572, #673, #678, #925,
//! #926, #899). [`apply`] is the one place that decides, so a tool states
//! *what happened to time* ([`TimeEdit`]) and *what it is* ([`Scope`]), and
//! cannot forget a record.
//!
//! ```text
//! apply(state, edit, scope, rates) -> TimeEditReport
//! ```
//!
//! It is a functional core: no I/O, no clock, nothing read but its
//! arguments. It takes the state by `&mut` where ADR 0004 wrote
//! `&SessionState -> SessionState`, because the callers are hooks that hold a
//! `&mut SessionState` and `remove_fillers` applies hundreds of cuts to one
//! state, which a clone each would make quadratic in the transcript. The one
//! thing that reads a file, the sample rate of a clip's source, is
//! [`Scope::rates`], asked **before** the tool edits anything so a track with
//! no single rate refuses the whole call.
//!
//! # The rules it holds
//!
//! * A time is in **seconds** and becomes frames at the rate of the track it
//!   is applied to, never the session's (#234, #673): the edited track's
//!   clips at its rate, each other track's at its own.
//! * **Labels** move with every edit that moves time (#203).
//! * **Words** are seconds of the audio they were made from, so they move
//!   only when that audio did (#925, #926): a transcript of a track follows
//!   an edit of that track, or any edit when sync-lock carried it to every
//!   track; a transcript of a file never moves; one with no recorded source
//!   (an older project) follows every edit that remaps a transcript. A
//!   [`TimeEdit::Shift`] is stricter: it moves only the words of the track it
//!   shifted, and no label (#899).
//! * A tool opts in to sync-lock with [`SyncLock::Follows`]; which tools do
//!   is a product decision this module does not widen.
//! * An edit that gains nothing in a track's frames (a gap shorter than a
//!   frame) leaves that track's clips alone rather than splitting a clip for
//!   nothing.
//! * `SessionState::length_samples` is never written here: the caller sets it
//!   once, last, with `util::set_session_length` (#678).
//!
//! # What stays in the tool
//!
//! The audio itself. `truncate_silence`, `time_stretch` and the other
//! destructive edits write a new file and then describe the time change with
//! [`Clips::Rewritten`]; `time_shift`, `move_clip` and `align_end_to_end`
//! move their clips by their own clamps and describe it with
//! [`TimeEdit::Shift`]; `cut_words` and `remove_fillers` remove the words
//! they cut instead of clipping them ([`Words::Caller`]).

use session::{Clip, SessionState};

use crate::tool::util::{seconds_to_frames, slice_envelope, track_clip_rate, track_clip_rates};

/// What happened to time, in seconds of the timeline as it was before.
#[derive(Clone, Copy)]
pub(crate) enum TimeEdit<'a> {
    /// `[start, end)` was removed and the rest closed up (`cut_range`,
    /// `cut_words`, `remove_fillers`, `truncate_silence`).
    Cut {
        /// First removed second.
        start: f64,
        /// First second after the removed span.
        end: f64,
    },
    /// `[start, end)` was removed and the rest stayed where it was
    /// (`edit_at_markers` delete). Only clips change: no label or word moves.
    Hollow {
        /// First removed second.
        start: f64,
        /// First second after the removed span.
        end: f64,
    },
    /// Only `[start, end)` was kept and moved to 0 (`trim`). Labels and words
    /// move as two cuts, the tail first, which drops what lay outside.
    Keep {
        /// First kept second.
        start: f64,
        /// First second after the kept span.
        end: f64,
        /// Where the timeline ended before the edit.
        until: f64,
    },
    /// `len` seconds were opened at `at`: a clip across it is split and
    /// everything after moves later (`insert_silence`, `paste_region`,
    /// `paulstretch` with a range). A `len` of 0 or less moves no clip but
    /// still remaps the records by it.
    Gap {
        /// Where the time was opened.
        at: f64,
        /// How much time was opened.
        len: f64,
    },
    /// Every position was multiplied by `factor`, new length over old
    /// (`change_speed`, `time_stretch`, `paulstretch` without a range).
    /// Nothing is dropped.
    Scale {
        /// New length over old length.
        factor: f64,
    },
    /// `[start, end)` was re-timed to `out_len` seconds by a varying amount
    /// (a ramped `time_stretch`). `map` takes an old second to its new one;
    /// the other tracks open or close the difference at `end`.
    Warp {
        /// First re-timed second.
        start: f64,
        /// First second after the re-timed span.
        end: f64,
        /// How long the span is afterwards.
        out_len: f64,
        /// An old second to its new one: non-decreasing, nothing dropped.
        map: &'a dyn Fn(f64) -> f64,
    },
    /// The clips of the edited track were moved by `by` seconds (`time_shift`,
    /// `move_clip`, `align_end_to_end`; the tool moved them, by its own
    /// clamp). Moves the words of a transcript of that track and nothing
    /// else; `starting_in` limits it to the words that start in `[from, to)`
    /// (one clip's old place) and puts the words back in time order.
    Shift {
        /// The shift the clips actually took, in seconds.
        by: f64,
        /// Only the words starting in this span of seconds.
        starting_in: Option<(f64, f64)>,
    },
}

/// Whether a tool carries its edit to the other tracks when the session is
/// sync-locked (#170 §3). Declared by the tool, never decided by a branch in
/// it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum SyncLock {
    /// The edit is the edited track's own, whatever the mode.
    Ignores,
    /// With sync-lock on, every other track gets the same seconds in its own
    /// frames.
    Follows,
}

/// Who changes the edited track's clips.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Clips {
    /// [`apply`] does, for `Cut`, `Hollow`, `Keep` and `Gap`: a timeline edit
    /// with no new audio (`cut_range`, `trim`, `cut_words`).
    Slide,
    /// The tool already did: it wrote a new file for the track
    /// (`insert_silence`, `change_speed`, ...) or moved the clips by its own
    /// clamp. `Scale`, `Warp` and `Shift` are always this.
    Rewritten,
}

/// Who changes the transcript.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Words {
    /// [`apply`] does, by the rule in the module.
    Follow,
    /// The tool does: `cut_words` and `remove_fillers` remove the words they
    /// cut, where a cut would clip them to what survives.
    Caller,
}

/// What a tool is, to an edit: which track it edited and which records are
/// its to move.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Scope {
    /// The track the tool edited.
    pub track: usize,
    /// Whether the tool follows sync-lock.
    pub sync_lock: SyncLock,
    /// Who changes the edited track's clips.
    pub clips: Clips,
    /// Who changes the transcript.
    pub words: Words,
}

impl Scope {
    /// A scope for an edit of `track`.
    pub(crate) fn new(track: usize, sync_lock: SyncLock, clips: Clips, words: Words) -> Self {
        Self {
            track,
            sync_lock,
            clips,
            words,
        }
    }

    /// A scope for an edit of `track` alone whose clips the tool already
    /// changed: a rewrite of its audio (`change_speed`, `time_stretch`,
    /// `truncate_silence`) or a move of its clips (`time_shift`, `move_clip`,
    /// `align_end_to_end`). Carries nothing to other tracks and counts no
    /// frames, so it needs no [`Rates`].
    pub(crate) fn alone(track: usize) -> Self {
        Self::new(track, SyncLock::Ignores, Clips::Rewritten, Words::Follow)
    }

    /// The rates `apply` will count frames at, asked before the tool edits
    /// anything (#234, #673).
    ///
    /// Sync-lock on and followed: every track's rate, as
    /// `track_clip_rates`. Otherwise only the edited track's, and only when
    /// [`Clips::Slide`] makes `apply` count its frames. `Err` for a track
    /// whose clips come from files of different rates (`track_clip_rate`'s
    /// refusal), so the call refuses before any node is appended.
    pub(crate) fn rates(&self, state: &SessionState) -> Result<Rates, String> {
        if self.sync_lock == SyncLock::Follows && state.sync_lock {
            return track_clip_rates(state).map(Rates);
        }
        match state.tracks.get(self.track) {
            Some(track) if self.clips == Clips::Slide => {
                let rate = track_clip_rate(track, state.sample_rate)?;
                Ok(Rates::of_edited(state, self.track, rate))
            }
            _ => Ok(Rates::default()),
        }
    }
}

/// The rate each track's clips are counted in, one per track in order, from
/// [`Scope::rates`]. A track that was not asked about answers the session's.
#[derive(Clone, Debug, Default)]
pub(crate) struct Rates(Vec<f64>);

impl Rates {
    /// The rates when only the edited track's frames are counted and its rate
    /// is already known (`trim`, which does not follow sync-lock, has it from
    /// `track_clip_rate` already).
    pub(crate) fn of_edited(state: &SessionState, track: usize, rate: f64) -> Self {
        let mut rates = vec![f64::from(state.sample_rate.max(1)); state.tracks.len()];
        if let Some(slot) = rates.get_mut(track) {
            *slot = rate;
        }
        Self(rates)
    }
}

/// What an edit did that a tool reports, the same fields everywhere.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct TimeEditReport {
    /// How many other tracks sync-lock carried the edit to.
    pub synced_tracks: usize,
    /// How many labels the edit dropped: the one outcome a tool has to be
    /// able to mention, since a label is text the user wrote.
    pub dropped_labels: usize,
}

/// Apply `edit` to `state` as `scope` says: the clips, the other tracks when
/// the scope follows sync-lock and the session is locked, the labels and the
/// words. See the module for the rules.
pub(crate) fn apply(
    state: &mut SessionState,
    edit: TimeEdit<'_>,
    scope: &Scope,
    rates: &Rates,
) -> TimeEditReport {
    let fallback = f64::from(state.sample_rate.max(1));
    let rate_of = |track: usize| rates.0.get(track).copied().unwrap_or(fallback);
    let carried = scope.sync_lock == SyncLock::Follows && state.sync_lock;
    let mut report = TimeEditReport::default();

    // Sync-lock (#170 §3): an interview is one track per speaker, and a cut
    // out of one desynchronises the conversation unless the others lose the
    // same span. One node, not one per track, so undo puts the whole edit
    // back; the caller appends it.
    let slides = scope.clips == Clips::Slide && !matches!(edit, TimeEdit::Warp { .. });
    if slides {
        let rate = rate_of(scope.track);
        if let Some(track) = state.tracks.get_mut(scope.track) {
            if let Some(clips) = clips_after(&edit, &track.clips, rate) {
                track.clips = clips;
            }
        }
    }
    let gains_time = !matches!(edit, TimeEdit::Gap { len, .. } if len <= 0.0);
    if carried && gains_time {
        for (i, track) in state.tracks.iter_mut().enumerate() {
            if i == scope.track || track.clips.is_empty() {
                continue;
            }
            if let Some(clips) = clips_after(&edit, &track.clips, rate_of(i)) {
                track.clips = clips;
                report.synced_tracks += 1;
            }
        }
    }

    let words = words_follow(state, scope, carried);
    report.dropped_labels = remap(state, &edit, scope.track, words);
    report
}

/// The clips of one track after `edit`, counted at `rate`; `None` when the
/// edit does not move a track's clips (`Scale`, `Shift`).
fn clips_after(edit: &TimeEdit<'_>, clips: &[Clip], rate: f64) -> Option<Vec<Clip>> {
    let frames = |seconds: f64| seconds_to_frames(seconds, rate);
    match *edit {
        TimeEdit::Cut { start, end } => Some(cut_timeline(clips, frames(start), frames(end))),
        TimeEdit::Hollow { start, end } => {
            let (from, to) = (frames(start), frames(end));
            Some(insert_gap_timeline(
                &cut_timeline(clips, from, to),
                from,
                to.saturating_sub(from),
            ))
        }
        TimeEdit::Keep { start, end, .. } => Some(keep_timeline(clips, frames(start), frames(end))),
        TimeEdit::Gap { at, len } => Some(open_gap(clips, frames(at), frames(len))),
        TimeEdit::Warp {
            start,
            end,
            out_len,
            ..
        } => {
            let delta = out_len - (end - start);
            if delta >= 0.0 {
                Some(open_gap(clips, frames(end), frames(delta)))
            } else {
                let (from, to) = (frames(start + out_len), frames(end));
                Some(if from >= to {
                    clips.to_vec()
                } else {
                    cut_timeline(clips, from, to)
                })
            }
        }
        TimeEdit::Scale { .. } | TimeEdit::Shift { .. } => None,
    }
}

/// `clips` with `len` frames opened at `at`, or untouched when `len` is 0:
/// a gap of nothing would still split a clip that spans it.
fn open_gap(clips: &[Clip], at: u64, len: u64) -> Vec<Clip> {
    if len == 0 {
        clips.to_vec()
    } else {
        insert_gap_timeline(clips, at, len)
    }
}

/// Whether the transcript's words are seconds of audio this edit moved.
///
/// A transcript of a track follows an edit of that track, or any edit when
/// sync-lock `carried` it to every track; one of a file never does (its
/// seconds are the file's, not the timeline's); one of a track that is gone
/// describes nothing. A transcript with no recorded source (a project from
/// before sources were kept) cannot say whose seconds it holds, so it follows
/// every edit that remaps a transcript, as it always did (#925, #926).
fn words_follow(state: &SessionState, scope: &Scope, carried: bool) -> bool {
    if scope.words == Words::Caller {
        return false;
    }
    match state.transcript.as_ref().map(|t| &t.source) {
        None => false,
        Some(None) => true,
        Some(Some(session::TranscriptSource::File)) => false,
        Some(Some(session::TranscriptSource::Track { track })) => {
            carried
                || state
                    .tracks
                    .get(scope.track)
                    .is_some_and(|edited| &edited.id == track)
        }
    }
}

/// Move the labels, and the words when `words` says so, for `edit`. Returns
/// the labels dropped.
fn remap(state: &mut SessionState, edit: &TimeEdit<'_>, track: usize, words: bool) -> usize {
    match *edit {
        TimeEdit::Cut { start, end } => cut_records(state, words, start, end),
        // Two cuts, tail first: cutting the head renumbers everything after
        // it, so doing it first would make `until`, measured before the
        // edit, the wrong bound for the tail.
        TimeEdit::Keep { start, end, until } => {
            cut_records(state, words, end, until) + cut_records(state, words, 0.0, start)
        }
        TimeEdit::Hollow { .. } => 0,
        TimeEdit::Gap { at, len } => {
            state.annotations = insert_annotations(&state.annotations, at, len);
            if words {
                state.transcript = insert_transcript(&state.transcript, at, len);
            }
            0
        }
        TimeEdit::Scale { factor } => {
            state.annotations = scale_annotations(&state.annotations, factor);
            if words {
                state.transcript = scale_transcript(&state.transcript, factor);
            }
            0
        }
        TimeEdit::Warp { map, .. } => {
            warp_records(state, words, map);
            0
        }
        TimeEdit::Shift { by, starting_in } => {
            shift_words(state, track, by, starting_in);
            0
        }
    }
}

/// Labels (and words, when `words`) after a cut of `[start, end)`.
fn cut_records(state: &mut SessionState, words: bool, start: f64, end: f64) -> usize {
    let (kept, dropped) = cut_annotations(&state.annotations, start, end);
    state.annotations = kept;
    if words {
        let (moved, _) = cut_transcript(&state.transcript, start, end);
        state.transcript = moved;
    }
    dropped
}

/// Labels (and words, when `words`) through `map`, a non-decreasing function
/// from old seconds to new: a mark inside a re-timed stretch moves in
/// proportion to where it sat, one before it stays, and one after it moves by
/// what the stretch gained or lost. Nothing is dropped.
fn warp_records(state: &mut SessionState, words: bool, map: &dyn Fn(f64) -> f64) {
    use session::AnnotationKind::{Marker, Region};
    for a in &mut state.annotations {
        a.kind = match a.kind {
            Marker { time_sec } => Marker {
                time_sec: map(time_sec),
            },
            Region { start_sec, end_sec } => Region {
                start_sec: map(start_sec),
                end_sec: map(end_sec),
            },
        };
    }
    if words {
        if let Some(t) = state.transcript.as_mut() {
            for w in &mut t.words {
                w.start_s = map(w.start_s as f64) as f32;
                w.end_s = map(w.end_s as f64) as f32;
            }
        }
    }
}

/// Move the words of a transcript that was made from track `track` by `by`
/// seconds, because the track's clips moved by that much (#899).
///
/// A transcript made from a track holds seconds of that track as it sits on
/// the timeline (#384), so a tool that moves the track's clips has to move the
/// words with them, or `cut_words` accepts the track and cuts audio the words
/// do not describe. A transcript of another track, of a file, or with no
/// recorded source is left alone: its words are not seconds of this track.
///
/// `by` is the shift the clips *actually* took, in seconds (frames moved over
/// the track's clip rate), so a clamped shift moves the words by the clamp,
/// not by the request. A word never ends up before 0 s, as no clip does.
///
/// `starting_in` limits the move to the words that start in that span of
/// seconds `[from, to)` (the old place of one clip, for `move_clip`), and
/// then puts the words back in time order; `None` moves every word, which
/// keeps their order.
fn shift_words(state: &mut SessionState, track: usize, by: f64, starting_in: Option<(f64, f64)>) {
    let Some(id) = state.tracks.get(track).map(|t| t.id.clone()) else {
        return;
    };
    let Some(transcript) = state.transcript.as_mut() else {
        return;
    };
    if transcript.source != Some(session::TranscriptSource::Track { track: id }) || by == 0.0 {
        return;
    }
    for w in &mut transcript.words {
        let start = f64::from(w.start_s);
        if let Some((from, to)) = starting_in {
            if start < from || start >= to {
                continue;
            }
        }
        w.start_s = (start + by).max(0.0) as f32;
        w.end_s = (f64::from(w.end_s) + by).max(0.0) as f32;
    }
    // Moving one clip past another moves its words past theirs, and the
    // transcript is kept in time order (`transcript_cues::validate` refuses
    // one that is not, so `export_transcript` and `transcript_to_markers`
    // would stop working). A stable sort keeps words that start together in
    // the order they were.
    if starting_in.is_some() {
        transcript
            .words
            .sort_by(|a, b| a.start_s.total_cmp(&b.start_s));
    }
}

// ---- Moved from `tool::util`: the clip slicers and the records' remaps -----------

/// The part of `clip` lying inside the timeline window `[from, to)`, moved
/// so the window's origin sits at `new_origin`.
///
/// `None` when the clip doesn't reach into the window at all. Everything
/// that travels with a clip travels with the piece: the source window
/// narrows to match, and the volume envelope is sliced to the same span so
/// automation stays attached to the audio it was written for.
fn clip_window(clip: &Clip, from: u64, to: u64, new_origin: u64) -> Option<Clip> {
    let clip_start = clip.start_in_track;
    let clip_end = clip_start.saturating_add(clip.length);
    let lo = clip_start.max(from);
    let hi = clip_end.min(to);
    if hi <= lo {
        return None;
    }
    let into_clip = lo - clip_start;
    let len = hi - lo;
    let mut piece = Clip {
        source_path: clip.source_path.clone(),
        start_in_track: new_origin + (lo - from),
        source_offset: clip.source_offset.saturating_add(into_clip),
        length: len,
        content_hash: clip.content_hash,
        time_stretch_factor: clip.time_stretch_factor,
        pitch_shift_semitones: clip.pitch_shift_semitones,
        beat_grid: clip.beat_grid.clone(),
        volume_envelope: Vec::new(),
        volume_envelope_frames: session::EnvelopeFrames::default(),
    };
    piece.set_envelope(slice_envelope(&clip.volume_envelope, into_clip, len));
    Some(piece)
}

/// Remove the timeline range `[start, end)` from a track and close the gap.
///
/// Every clip is considered, not just the first. A clip wholly before the
/// cut is untouched; one wholly after slides left by the cut's length; one
/// straddling it contributes whichever of its two ends survive. Rewriting
/// only `clips[0]` and assigning the result over `track.clips` — which is
/// what this used to do — deleted every other clip on the track, so a
/// second cut silently truncated the track at the first cut's join.
fn cut_timeline(clips: &[Clip], start: u64, end: u64) -> Vec<Clip> {
    let mut out = Vec::with_capacity(clips.len() + 1);
    for clip in clips {
        if let Some(head) = clip_window(clip, 0, start, 0) {
            out.push(head);
        }
        // The tail's window opens at `end` and its origin is `start`, which
        // is the leftward slide: a clip sitting at `end + d` lands at
        // `start + d`.
        if let Some(tail) = clip_window(clip, end, u64::MAX, start) {
            out.push(tail);
        }
    }
    out.sort_by_key(|c| c.start_in_track);
    out
}

/// Shift every clip at or after `at` right by `len` samples, splitting
/// any clip that straddles the point.
///
/// The insert half of sync-lock (#170 §3): the target track has silence
/// spliced into its samples, and every other track has to open the same
/// gap or the tracks come out of alignment by exactly the amount that
/// was inserted.
fn insert_gap_timeline(clips: &[Clip], at: u64, len: u64) -> Vec<Clip> {
    let mut out = Vec::with_capacity(clips.len() + 1);
    for clip in clips {
        let start = clip.start_in_track;
        let end = start + clip.length;
        if end <= at {
            // Wholly before the point: untouched.
            out.push(clip.clone());
        } else if start >= at {
            // Wholly after: slides right.
            let mut moved = clip.clone();
            moved.start_in_track = start + len;
            out.push(moved);
        } else {
            // Straddling: the gap opens inside it, so it becomes two.
            if let Some(head) = clip_window(clip, 0, at, 0) {
                out.push(head);
            }
            if let Some(tail) = clip_window(clip, at, u64::MAX, at + len) {
                out.push(tail);
            }
        }
    }
    out.sort_by_key(|c| c.start_in_track);
    out
}

/// Keep only the timeline range `[start, end)` and re-base it to zero.
fn keep_timeline(clips: &[Clip], start: u64, end: u64) -> Vec<Clip> {
    let mut out: Vec<Clip> = clips
        .iter()
        .filter_map(|c| clip_window(c, start, end, 0))
        .collect();
    out.sort_by_key(|c| c.start_in_track);
    out
}
// =============================================================================
// Annotations that follow the audio (#203 §1)
// =============================================================================

/// Move annotations to keep up with a cut of `[start_sec, end_sec)`.
///
/// A label names a *moment in the recording*, not an offset in a file.
/// Cutting thirty seconds out of the middle and leaving the labels where
/// they were renames every chapter after the cut to something thirty
/// seconds off — silently, and only discovered when someone ships the
/// episode and the chapter marks land mid-sentence.
///
/// Returns the surviving annotations and how many were dropped, because
/// dropping is the one outcome the caller has to be able to mention.
///
/// The rules are the ones a person would apply by hand:
///
/// * Before the cut — untouched.
/// * After the cut — slides left by the cut's length.
/// * A **marker strictly inside** the cut — dropped. The instant it
///   named is not in the recording any more, and moving it to the seam
///   would be inventing a position the user never chose.
/// * A **marker exactly on either boundary** — kept, at the seam. The
///   audio range removed is half-open, so `start_sec` is technically
///   the first removed instant; but a mark placed deliberately at the
///   edge of a cut is naming the edit, and after the cut the seam is
///   still there. Dropping it would delete a label for a position that
///   still exists.
/// * A **region overlapping** the cut — clipped to what survives on
///   either side, and dropped only if nothing does. A chapter that
///   started before the cut still starts where it did.
fn cut_annotations(
    annotations: &[session::Annotation],
    start_sec: f64,
    end_sec: f64,
) -> (Vec<session::Annotation>, usize) {
    use session::AnnotationKind::{Marker, Region};

    let len = (end_sec - start_sec).max(0.0);
    // Where a point in the original lands afterwards. Points inside the
    // cut collapse onto the seam, which is right for a region edge and
    // wrong for a marker — hence markers are handled separately.
    let map = |t: f64| {
        if t <= start_sec {
            t
        } else {
            (t - len).max(start_sec)
        }
    };

    let mut out = Vec::with_capacity(annotations.len());
    let mut dropped = 0;

    for a in annotations {
        match a.kind {
            Marker { time_sec } => {
                if time_sec > start_sec && time_sec < end_sec {
                    dropped += 1;
                    continue;
                }
                out.push(session::Annotation {
                    kind: Marker {
                        time_sec: map(time_sec),
                    },
                    ..a.clone()
                });
            }
            Region {
                start_sec: s,
                end_sec: e,
            } => {
                let (ns, ne) = (map(s), map(e));
                // A region wholly inside the cut collapses to zero
                // length; there is nothing left of it to label.
                if ne - ns <= f64::EPSILON {
                    dropped += 1;
                    continue;
                }
                out.push(session::Annotation {
                    kind: Region {
                        start_sec: ns,
                        end_sec: ne,
                    },
                    ..a.clone()
                });
            }
        }
    }
    (out, dropped)
}

/// Move annotations to keep up with `len_sec` of silence inserted at
/// `at_sec`.
///
/// Mirror of [`cut_annotations`], and nothing is ever dropped — an
/// insert only ever makes room. A region that *spans* the insert point
/// grows to contain the new silence, which is the reading that matches
/// what the user sees: the passage they marked is now longer.
fn insert_annotations(
    annotations: &[session::Annotation],
    at_sec: f64,
    len_sec: f64,
) -> Vec<session::Annotation> {
    use session::AnnotationKind::{Marker, Region};

    let shift = |t: f64| if t < at_sec { t } else { t + len_sec };

    annotations
        .iter()
        .map(|a| match a.kind {
            Marker { time_sec } => session::Annotation {
                kind: Marker {
                    time_sec: shift(time_sec),
                },
                ..a.clone()
            },
            Region { start_sec, end_sec } => session::Annotation {
                kind: Region {
                    start_sec: shift(start_sec),
                    // A region *spanning* the insert stretches; one that
                    // merely ends where the silence begins does not.
                    // Regions are half-open, so an end exactly at
                    // `at_sec` is the first instant outside the region —
                    // the silence goes after it, and the passage the
                    // user marked is no longer than it was.
                    end_sec: if end_sec > at_sec {
                        end_sec + len_sec
                    } else {
                        end_sec
                    },
                },
                ..a.clone()
            },
        })
        .collect()
}

// =============================================================================
// Transcript remapping
// =============================================================================
//
// The transcript is the second time-addressed record in `SessionState`,
// and it was the one nobody moved. `cut_annotations` had existed since
// #203; the words beside them went on describing the timeline as it was
// before the edit.
//
// That is worse than a stale chapter mark, because the transcript is not
// only *read* by the user — it is read by `cut_words`, `select_region`
// and `duck_under_speech`, which turn a word's `start_s` into a sample
// offset and edit there. A stale transcript therefore makes a later text
// edit delete audio somewhere else entirely, while reporting the word it
// thought it removed.
//
// These mirror the annotation helpers above deliberately, including the
// boundary rules: a word is a half-open span, so it behaves like a
// `Region`, not like a `Marker`.

/// Move transcript words to keep up with a cut of `[start_sec, end_sec)`.
///
/// Returns the surviving words and how many were dropped. A word is a
/// span, so the `Region` rules apply: one that overlaps the cut is
/// clipped to what survives, and dropped only when nothing does.
fn cut_transcript(
    transcript: &Option<session::Transcript>,
    start_sec: f64,
    end_sec: f64,
) -> (Option<session::Transcript>, usize) {
    let Some(t) = transcript else {
        return (None, 0);
    };

    let len = (end_sec - start_sec).max(0.0);
    let map = |x: f64| {
        if x <= start_sec {
            x
        } else {
            (x - len).max(start_sec)
        }
    };

    let mut words = Vec::with_capacity(t.words.len());
    let mut dropped = 0;

    for w in &t.words {
        let (ns, ne) = (map(w.start_s as f64), map(w.end_s as f64));
        // The whole word was inside the cut: its audio is gone, and a
        // zero-width word at the seam would be a word the listener can
        // never hear.
        if ne - ns <= f64::EPSILON {
            dropped += 1;
            continue;
        }
        words.push(session::TranscriptWord {
            text: w.text.clone(),
            start_s: ns as f32,
            end_s: ne as f32,
            confidence: w.confidence,
        });
    }

    (
        Some(session::Transcript {
            words,
            source: t.source.clone(),
        }),
        dropped,
    )
}

/// Mirror of [`cut_transcript`] for `len_sec` of audio inserted at
/// `at_sec`. Nothing is ever dropped — an insert only makes room.
fn insert_transcript(
    transcript: &Option<session::Transcript>,
    at_sec: f64,
    len_sec: f64,
) -> Option<session::Transcript> {
    let t = transcript.as_ref()?;

    let shift = |x: f64| if x < at_sec { x } else { x + len_sec };

    Some(session::Transcript {
        words: t
            .words
            .iter()
            .map(|w| session::TranscriptWord {
                text: w.text.clone(),
                start_s: shift(w.start_s as f64) as f32,
                // A word *spanning* the insert grows to contain it; one
                // that merely ends where the silence begins does not.
                end_s: if (w.end_s as f64) > at_sec {
                    (w.end_s as f64 + len_sec) as f32
                } else {
                    w.end_s
                },
                confidence: w.confidence,
            })
            .collect(),
        source: t.source.clone(),
    })
}

/// Rescale annotations for an edit that re-times the whole timeline by
/// `factor` (new length ÷ old length).
///
/// Speed and time-stretch do not remove a span, they re-time
/// everything, so nothing is ever dropped and every position simply
/// multiplies.
fn scale_annotations(annotations: &[session::Annotation], factor: f64) -> Vec<session::Annotation> {
    use session::AnnotationKind::{Marker, Region};

    annotations
        .iter()
        .map(|a| match a.kind {
            Marker { time_sec } => session::Annotation {
                kind: Marker {
                    time_sec: time_sec * factor,
                },
                ..a.clone()
            },
            Region { start_sec, end_sec } => session::Annotation {
                kind: Region {
                    start_sec: start_sec * factor,
                    end_sec: end_sec * factor,
                },
                ..a.clone()
            },
        })
        .collect()
}

/// Rescale transcript words for a whole-timeline re-time by `factor`.
fn scale_transcript(
    transcript: &Option<session::Transcript>,
    factor: f64,
) -> Option<session::Transcript> {
    let t = transcript.as_ref()?;
    Some(session::Transcript {
        words: t
            .words
            .iter()
            .map(|w| session::TranscriptWord {
                text: w.text.clone(),
                start_s: (w.start_s as f64 * factor) as f32,
                end_s: (w.end_s as f64 * factor) as f32,
                confidence: w.confidence,
            })
            .collect(),
        source: t.source.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use session::{
        Annotation, AnnotationId, AnnotationKind, EnvelopePoint, Track, TrackId, Transcript,
        TranscriptSource, TranscriptWord,
    };
    use std::path::{Path, PathBuf};
    use tempfile::TempDir;

    const R48: f64 = 48_000.0;
    const R16: f64 = 16_000.0;

    /// A folder of one-frame WAVs, one per rate: a clip's rate is read from
    /// its file's header, so a test that counts frames needs a real file.
    struct Rig {
        dir: TempDir,
    }

    impl Rig {
        fn new() -> Self {
            Self {
                dir: TempDir::new().expect("tempdir"),
            }
        }

        fn wav(&self, rate: u32) -> PathBuf {
            let path = self.dir.path().join(format!("{rate}.wav"));
            if !path.exists() {
                let spec = hound::WavSpec {
                    channels: 1,
                    sample_rate: rate,
                    bits_per_sample: 16,
                    sample_format: hound::SampleFormat::Int,
                };
                let mut w = hound::WavWriter::create(&path, spec).expect("create wav");
                w.write_sample(0i16).expect("write");
                w.finalize().expect("finalize");
            }
            path
        }

        /// A clip of `len` frames at `start` of the file at `rate`.
        fn clip(&self, rate: u32, start: u64, len: u64) -> Clip {
            clip_of(&self.wav(rate), start, len)
        }
    }

    fn clip_of(path: &Path, start: u64, len: u64) -> Clip {
        Clip {
            source_path: path.to_path_buf(),
            start_in_track: start,
            source_offset: 0,
            length: len,
            content_hash: None,
            time_stretch_factor: None,
            pitch_shift_semitones: None,
            beat_grid: None,
            volume_envelope: Vec::new(),
            volume_envelope_frames: session::EnvelopeFrames::default(),
        }
    }

    fn track_of(clips: Vec<Clip>) -> Track {
        Track {
            id: TrackId::new(),
            name: "t".into(),
            clips,
            gain_db: 0.0,
            pan: 0.0,
            muted: false,
            soloed: false,
            sends: Vec::new(),
            effects: Vec::new(),
        }
    }

    fn state_of(tracks: Vec<Track>, sync_lock: bool) -> SessionState {
        SessionState {
            tracks,
            bus_routing: session::BusGraph::default(),
            master_chain: Vec::new(),
            tempo_map: session::TempoMap::default(),
            key_map: None,
            transcript: None,
            sample_rate: 48_000,
            length_samples: 0,
            annotations: Vec::new(),
            sync_lock,
        }
    }

    /// A 48 kHz bed (4 s) on track 0 and a 16 kHz voice (4 s) on track 1.
    fn bed_and_voice(rig: &Rig, sync_lock: bool) -> SessionState {
        state_of(
            vec![
                track_of(vec![rig.clip(48_000, 0, 192_000)]),
                track_of(vec![rig.clip(16_000, 0, 64_000)]),
            ],
            sync_lock,
        )
    }

    fn marker(t: f64) -> Annotation {
        Annotation {
            id: AnnotationId::new(),
            name: format!("m{t}"),
            kind: AnnotationKind::Marker { time_sec: t },
        }
    }

    fn region(s: f64, e: f64) -> Annotation {
        Annotation {
            id: AnnotationId::new(),
            name: format!("r{s}-{e}"),
            kind: AnnotationKind::Region {
                start_sec: s,
                end_sec: e,
            },
        }
    }

    fn word(text: &str, s: f32, e: f32) -> TranscriptWord {
        TranscriptWord {
            text: text.into(),
            start_s: s,
            end_s: e,
            confidence: 1.0,
        }
    }

    fn times(state: &SessionState) -> Vec<(f64, f64)> {
        state
            .annotations
            .iter()
            .map(|a| match a.kind {
                AnnotationKind::Marker { time_sec } => (time_sec, time_sec),
                AnnotationKind::Region { start_sec, end_sec } => (start_sec, end_sec),
            })
            .collect()
    }

    fn word_times(state: &SessionState) -> Vec<(f32, f32)> {
        state
            .transcript
            .as_ref()
            .map(|t| t.words.iter().map(|w| (w.start_s, w.end_s)).collect())
            .unwrap_or_default()
    }

    /// `(start_in_track, source_offset, length)` of each clip.
    fn shape(track: &Track) -> Vec<(u64, u64, u64)> {
        track
            .clips
            .iter()
            .map(|c| (c.start_in_track, c.source_offset, c.length))
            .collect()
    }

    fn with_transcript(
        state: &mut SessionState,
        source: Option<TranscriptSource>,
        words: Vec<TranscriptWord>,
    ) {
        state.transcript = Some(Transcript { words, source });
    }

    fn of_track(state: &SessionState, i: usize) -> Option<TranscriptSource> {
        Some(TranscriptSource::Track {
            track: state.tracks[i].id.clone(),
        })
    }

    /// The scope of `cut_range`: slides the edited track, follows sync-lock.
    fn sliding(track: usize) -> Scope {
        Scope::new(track, SyncLock::Follows, Clips::Slide, Words::Follow)
    }

    /// The scope of the destructive edits: the audio was rewritten.
    fn rewritten(track: usize, sync_lock: SyncLock) -> Scope {
        Scope::new(track, sync_lock, Clips::Rewritten, Words::Follow)
    }

    /// Apply with the rates the scope asks for, as a tool does.
    fn run(state: &mut SessionState, edit: TimeEdit<'_>, scope: &Scope) -> TimeEditReport {
        let rates = scope.rates(state).expect("rates");
        apply(state, edit, scope, &rates)
    }

    // ---- Cut ---------------------------------------------------------------

    #[test]
    fn a_cut_with_no_transcript_has_no_words_to_move() {
        // A session that was never transcribed: nothing to remap, nothing
        // dropped, and no transcript appears from nowhere.
        assert_eq!(cut_transcript(&None, 1.0, 2.0), (None, 0));
    }

    #[test]
    fn a_cut_slides_the_edited_tracks_clips() {
        let rig = Rig::new();
        let mut s = bed_and_voice(&rig, false);
        let report = run(
            &mut s,
            TimeEdit::Cut {
                start: 1.0,
                end: 2.0,
            },
            &sliding(0),
        );
        assert_eq!(
            shape(&s.tracks[0]),
            vec![(0, 0, 48_000), (48_000, 96_000, 96_000)]
        );
        // Sync-lock is off: the other track stays, nothing was carried.
        assert_eq!(shape(&s.tracks[1]), vec![(0, 0, 64_000)]);
        assert_eq!(report, TimeEditReport::default());
    }

    #[test]
    fn a_cut_of_rewritten_audio_leaves_the_clips_to_the_tool() {
        let rig = Rig::new();
        let mut s = bed_and_voice(&rig, false);
        let before = s.tracks.clone();
        run(
            &mut s,
            TimeEdit::Cut {
                start: 1.0,
                end: 2.0,
            },
            &rewritten(0, SyncLock::Ignores),
        );
        assert_eq!(s.tracks, before);
    }

    #[test]
    fn a_cut_under_sync_lock_takes_the_same_seconds_from_each_track_in_its_own_frames() {
        let rig = Rig::new();
        let mut s = bed_and_voice(&rig, true);
        let report = run(
            &mut s,
            TimeEdit::Cut {
                start: 1.0,
                end: 2.0,
            },
            &sliding(0),
        );
        assert_eq!(
            shape(&s.tracks[0]),
            vec![(0, 0, 48_000), (48_000, 96_000, 96_000)]
        );
        assert_eq!(
            shape(&s.tracks[1]),
            vec![(0, 0, 16_000), (16_000, 32_000, 32_000)]
        );
        assert_eq!(report.synced_tracks, 1);
    }

    #[test]
    fn a_cut_from_the_voice_takes_the_same_seconds_from_the_bed() {
        let rig = Rig::new();
        let mut s = bed_and_voice(&rig, true);
        let report = run(
            &mut s,
            TimeEdit::Cut {
                start: 1.0,
                end: 2.0,
            },
            &sliding(1),
        );
        assert_eq!(
            shape(&s.tracks[1]),
            vec![(0, 0, 16_000), (16_000, 32_000, 32_000)]
        );
        assert_eq!(
            shape(&s.tracks[0]),
            vec![(0, 0, 48_000), (48_000, 96_000, 96_000)]
        );
        assert_eq!(report.synced_tracks, 1);
    }

    #[test]
    fn a_tool_that_does_not_follow_sync_lock_leaves_the_others_when_it_is_on() {
        let rig = Rig::new();
        let mut s = bed_and_voice(&rig, true);
        let scope = Scope::new(0, SyncLock::Ignores, Clips::Slide, Words::Follow);
        let report = run(
            &mut s,
            TimeEdit::Cut {
                start: 1.0,
                end: 2.0,
            },
            &scope,
        );
        assert_eq!(shape(&s.tracks[1]), vec![(0, 0, 64_000)]);
        assert_eq!(report.synced_tracks, 0);
    }

    #[test]
    fn a_tool_that_follows_sync_lock_leaves_the_others_when_it_is_off() {
        let rig = Rig::new();
        let mut s = bed_and_voice(&rig, false);
        let report = run(
            &mut s,
            TimeEdit::Cut {
                start: 1.0,
                end: 2.0,
            },
            &sliding(0),
        );
        assert_eq!(shape(&s.tracks[1]), vec![(0, 0, 64_000)]);
        assert_eq!(report.synced_tracks, 0);
    }

    #[test]
    fn a_track_with_no_clips_is_not_counted_as_synced() {
        let rig = Rig::new();
        let mut s = bed_and_voice(&rig, true);
        s.tracks.push(track_of(Vec::new()));
        let report = run(
            &mut s,
            TimeEdit::Cut {
                start: 1.0,
                end: 2.0,
            },
            &sliding(0),
        );
        assert_eq!(report.synced_tracks, 1);
        assert!(s.tracks[2].clips.is_empty());
    }

    #[test]
    fn a_cut_slices_a_clips_volume_envelope_with_its_pieces() {
        let rig = Rig::new();
        let mut s = bed_and_voice(&rig, false);
        s.tracks[0].clips[0].set_envelope(vec![
            EnvelopePoint {
                time_samples: 0,
                gain_db: 0.0,
            },
            EnvelopePoint {
                time_samples: 192_000,
                gain_db: -12.0,
            },
        ]);
        run(
            &mut s,
            TimeEdit::Cut {
                start: 1.0,
                end: 2.0,
            },
            &sliding(0),
        );
        // The tail's envelope starts where the tail's audio sat in the whole.
        let tail = &s.tracks[0].clips[1];
        assert_eq!(tail.volume_envelope[0].time_samples, 0);
        assert!((tail.volume_envelope[0].gain_db + 6.0).abs() < 1e-4);
    }

    #[test]
    fn a_cut_names_the_labels_it_dropped() {
        let rig = Rig::new();
        let mut s = bed_and_voice(&rig, false);
        s.annotations = vec![
            marker(0.5),      // before: stays
            marker(1.0),      // on the start: stays, at the seam
            marker(1.5),      // inside: dropped
            marker(2.0),      // on the end: stays, at the seam
            marker(3.0),      // after: slides left
            region(0.5, 3.0), // spans the cut: shrinks
            region(1.2, 1.8), // inside: dropped
            region(1.5, 2.5), // overlaps the end: clipped
        ];
        let report = run(
            &mut s,
            TimeEdit::Cut {
                start: 1.0,
                end: 2.0,
            },
            &sliding(0),
        );
        assert_eq!(report.dropped_labels, 2);
        assert_eq!(
            times(&s),
            vec![
                (0.5, 0.5),
                (1.0, 1.0),
                (1.0, 1.0),
                (2.0, 2.0),
                (0.5, 2.0),
                (1.0, 1.5)
            ]
        );
    }

    #[test]
    fn labels_move_even_when_the_edit_does_not_follow_sync_lock() {
        let rig = Rig::new();
        let mut s = bed_and_voice(&rig, true);
        s.annotations = vec![marker(3.0)];
        let scope = Scope::new(0, SyncLock::Ignores, Clips::Slide, Words::Follow);
        run(
            &mut s,
            TimeEdit::Cut {
                start: 1.0,
                end: 2.0,
            },
            &scope,
        );
        assert_eq!(times(&s), vec![(2.0, 2.0)]);
    }

    #[test]
    fn the_words_of_the_edited_track_follow_a_cut() {
        let rig = Rig::new();
        let mut s = bed_and_voice(&rig, false);
        let source = of_track(&s, 0);
        with_transcript(
            &mut s,
            source,
            vec![
                word("a", 0.5, 0.75),
                word("b", 1.25, 1.75),
                word("c", 2.5, 3.0),
            ],
        );
        run(
            &mut s,
            TimeEdit::Cut {
                start: 1.0,
                end: 2.0,
            },
            &sliding(0),
        );
        // "b" lay wholly inside the cut: its audio is gone.
        assert_eq!(word_times(&s), vec![(0.5, 0.75), (1.5, 2.0)]);
    }

    #[test]
    fn the_words_of_another_track_stay_unless_sync_lock_carried_the_cut_to_it() {
        let rig = Rig::new();
        let words = vec![word("a", 2.5, 3.0)];
        let unmoved = vec![(2.5, 3.0)];
        let moved = vec![(1.5, 2.0)];
        let cut = TimeEdit::Cut {
            start: 1.0,
            end: 2.0,
        };

        // Sync-lock off: only track 0 moved, the transcript is track 1's.
        let mut s = bed_and_voice(&rig, false);
        let source = of_track(&s, 1);
        with_transcript(&mut s, source, words.clone());
        run(&mut s, cut, &sliding(0));
        assert_eq!(word_times(&s), unmoved);

        // Sync-lock on, the tool follows it: every track moved.
        let mut s = bed_and_voice(&rig, true);
        let source = of_track(&s, 1);
        with_transcript(&mut s, source, words.clone());
        run(&mut s, cut, &sliding(0));
        assert_eq!(word_times(&s), moved);

        // Sync-lock on, the tool does not follow it: only track 0 moved.
        let mut s = bed_and_voice(&rig, true);
        let source = of_track(&s, 1);
        with_transcript(&mut s, source, words);
        let scope = Scope::new(0, SyncLock::Ignores, Clips::Slide, Words::Follow);
        run(&mut s, cut, &scope);
        assert_eq!(word_times(&s), unmoved);
    }

    #[test]
    fn the_words_of_a_file_never_move_and_older_words_follow_every_edit() {
        let rig = Rig::new();
        let cut = TimeEdit::Cut {
            start: 1.0,
            end: 2.0,
        };

        let mut s = bed_and_voice(&rig, true);
        with_transcript(
            &mut s,
            Some(TranscriptSource::File),
            vec![word("a", 2.5, 3.0)],
        );
        run(&mut s, cut, &sliding(0));
        assert_eq!(word_times(&s), vec![(2.5, 3.0)]);

        // No recorded source: a project from before they were kept.
        let mut s = bed_and_voice(&rig, false);
        with_transcript(&mut s, None, vec![word("a", 2.5, 3.0)]);
        run(&mut s, cut, &sliding(1));
        assert_eq!(word_times(&s), vec![(1.5, 2.0)]);
    }

    #[test]
    fn the_words_of_a_track_that_is_gone_do_not_move() {
        let rig = Rig::new();
        let mut s = bed_and_voice(&rig, false);
        let source = Some(TranscriptSource::Track {
            track: TrackId::new(),
        });
        with_transcript(&mut s, source, vec![word("a", 2.5, 3.0)]);
        run(
            &mut s,
            TimeEdit::Cut {
                start: 1.0,
                end: 2.0,
            },
            &sliding(0),
        );
        assert_eq!(word_times(&s), vec![(2.5, 3.0)]);
    }

    #[test]
    fn a_tool_that_owns_the_words_leaves_them_where_they_were() {
        let rig = Rig::new();
        let mut s = bed_and_voice(&rig, false);
        let source = of_track(&s, 0);
        with_transcript(&mut s, source, vec![word("a", 2.5, 3.0)]);
        let scope = Scope::new(0, SyncLock::Ignores, Clips::Slide, Words::Caller);
        run(
            &mut s,
            TimeEdit::Cut {
                start: 1.0,
                end: 2.0,
            },
            &scope,
        );
        assert_eq!(word_times(&s), vec![(2.5, 3.0)]);
    }

    #[test]
    fn a_cut_on_a_slow_track_moves_the_labels_by_seconds_not_frames() {
        let rig = Rig::new();
        let mut s = bed_and_voice(&rig, false);
        s.annotations = vec![marker(3.0)];
        // 16 000 frames of the voice are one second.
        run(
            &mut s,
            TimeEdit::Cut {
                start: 1.0,
                end: 2.0,
            },
            &sliding(1),
        );
        assert_eq!(times(&s), vec![(2.0, 2.0)]);
        assert_eq!(
            shape(&s.tracks[1]),
            vec![(0, 0, 16_000), (16_000, 32_000, 32_000)]
        );
    }

    #[test]
    fn a_cut_on_a_track_the_state_does_not_have_changes_no_clip() {
        let rig = Rig::new();
        let mut s = bed_and_voice(&rig, false);
        let before = s.tracks.clone();
        s.annotations = vec![marker(3.0)];
        let scope = sliding(7);
        run(
            &mut s,
            TimeEdit::Cut {
                start: 1.0,
                end: 2.0,
            },
            &scope,
        );
        assert_eq!(s.tracks, before);
        assert_eq!(times(&s), vec![(2.0, 2.0)]);
    }

    // ---- Gap ---------------------------------------------------------------

    #[test]
    fn a_gap_splits_the_clip_across_it_and_moves_what_follows() {
        let rig = Rig::new();
        let mut s = bed_and_voice(&rig, false);
        s.tracks[0].clips.push(rig.clip(48_000, 240_000, 48_000));
        run(&mut s, TimeEdit::Gap { at: 1.0, len: 0.5 }, &sliding(0));
        assert_eq!(
            shape(&s.tracks[0]),
            vec![
                (0, 0, 48_000),
                (72_000, 48_000, 144_000),
                (264_000, 0, 48_000)
            ]
        );
    }

    #[test]
    fn a_gap_on_rewritten_audio_leaves_the_edited_clips_and_opens_the_others_under_sync_lock() {
        let rig = Rig::new();
        let mut s = bed_and_voice(&rig, true);
        let edited = s.tracks[0].clone();
        let scope = rewritten(0, SyncLock::Follows);
        let report = run(&mut s, TimeEdit::Gap { at: 1.0, len: 0.5 }, &scope);
        assert_eq!(s.tracks[0], edited);
        // Half a second is 8 000 frames of the voice, at 16 000 from its start.
        assert_eq!(
            shape(&s.tracks[1]),
            vec![(0, 0, 16_000), (24_000, 16_000, 48_000)]
        );
        assert_eq!(report.synced_tracks, 1);
    }

    #[test]
    fn a_gap_shorter_than_a_frame_splits_nothing_but_still_counts_the_track() {
        let rig = Rig::new();
        let mut s = bed_and_voice(&rig, true);
        let before = s.tracks.clone();
        let report = run(&mut s, TimeEdit::Gap { at: 1.0, len: 1e-9 }, &sliding(0));
        assert_eq!(s.tracks, before);
        assert_eq!(report.synced_tracks, 1);
    }

    #[test]
    fn a_gap_that_gains_no_time_moves_no_clip_and_reports_no_synced_track() {
        let rig = Rig::new();
        let mut s = bed_and_voice(&rig, true);
        let before = s.tracks.clone();
        s.annotations = vec![marker(3.0)];
        let report = run(&mut s, TimeEdit::Gap { at: 1.0, len: -0.5 }, &sliding(0));
        assert_eq!(s.tracks, before);
        assert_eq!(report.synced_tracks, 0);
        // The records still take the number they were given, as before.
        assert_eq!(times(&s), vec![(2.5, 2.5)]);
    }

    #[test]
    fn a_gap_moves_a_marker_at_the_point_and_grows_only_a_region_across_it() {
        let rig = Rig::new();
        let mut s = bed_and_voice(&rig, false);
        s.annotations = vec![
            marker(0.5),
            marker(1.0),
            region(0.5, 1.0),
            region(0.5, 2.0),
            region(1.5, 2.0),
        ];
        let report = run(&mut s, TimeEdit::Gap { at: 1.0, len: 0.5 }, &sliding(0));
        assert_eq!(report.dropped_labels, 0);
        assert_eq!(
            times(&s),
            vec![(0.5, 0.5), (1.5, 1.5), (0.5, 1.0), (0.5, 2.5), (2.0, 2.5)]
        );
    }

    #[test]
    fn a_gap_moves_the_words_after_it_and_stretches_one_across_it() {
        let rig = Rig::new();
        let mut s = bed_and_voice(&rig, false);
        let source = of_track(&s, 0);
        with_transcript(
            &mut s,
            source,
            vec![
                word("a", 0.25, 0.75),
                word("b", 0.75, 1.5),
                word("c", 2.0, 2.5),
            ],
        );
        run(
            &mut s,
            TimeEdit::Gap { at: 1.0, len: 0.5 },
            &rewritten(0, SyncLock::Ignores),
        );
        assert_eq!(word_times(&s), vec![(0.25, 0.75), (0.75, 2.0), (2.5, 3.0)]);
    }

    // ---- Scale -------------------------------------------------------------

    #[test]
    fn a_scale_multiplies_every_label_and_word_and_drops_nothing() {
        let rig = Rig::new();
        let mut s = bed_and_voice(&rig, true);
        s.annotations = vec![marker(4.0), region(1.0, 3.0)];
        let source = of_track(&s, 0);
        with_transcript(&mut s, source, vec![word("a", 2.0, 4.0)]);
        let before = s.tracks.clone();
        let report = run(
            &mut s,
            TimeEdit::Scale { factor: 0.5 },
            &rewritten(0, SyncLock::Ignores),
        );
        assert_eq!(times(&s), vec![(2.0, 2.0), (0.5, 1.5)]);
        assert_eq!(word_times(&s), vec![(1.0, 2.0)]);
        assert_eq!(report, TimeEditReport::default());
        // A re-time rewrites the audio, which is the tool's: clips stay.
        assert_eq!(s.tracks, before);
    }

    #[test]
    fn a_scale_carries_nothing_to_the_other_tracks_even_under_sync_lock() {
        let rig = Rig::new();
        let mut s = bed_and_voice(&rig, true);
        let before = s.tracks.clone();
        let scope = Scope::new(0, SyncLock::Follows, Clips::Rewritten, Words::Follow);
        let report = run(&mut s, TimeEdit::Scale { factor: 0.5 }, &scope);
        assert_eq!(s.tracks, before);
        assert_eq!(report.synced_tracks, 0);
    }

    #[test]
    fn a_scale_leaves_the_words_of_another_track_alone() {
        let rig = Rig::new();
        let mut s = bed_and_voice(&rig, true);
        let source = of_track(&s, 1);
        with_transcript(&mut s, source, vec![word("a", 2.0, 4.0)]);
        run(
            &mut s,
            TimeEdit::Scale { factor: 0.5 },
            &rewritten(0, SyncLock::Ignores),
        );
        assert_eq!(word_times(&s), vec![(2.0, 4.0)]);
    }

    #[test]
    fn a_scale_followed_by_its_inverse_is_the_identity_within_rounding() {
        let rig = Rig::new();
        for factor in [0.25, 0.5, 0.8, 1.0, 1.7, 3.0, 10.0] {
            let mut s = bed_and_voice(&rig, false);
            s.annotations = vec![marker(0.1), marker(2.9), region(0.3, 3.7)];
            let before = times(&s);
            let scope = rewritten(0, SyncLock::Ignores);
            run(&mut s, TimeEdit::Scale { factor }, &scope);
            run(
                &mut s,
                TimeEdit::Scale {
                    factor: 1.0 / factor,
                },
                &scope,
            );
            for (a, b) in times(&s).iter().zip(&before) {
                assert!(
                    (a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9,
                    "{factor}"
                );
            }
        }
    }

    // ---- Warp --------------------------------------------------------------

    /// The span [1, 3) played at half speed: 4 s long, two seconds gained.
    fn slower(t: f64) -> f64 {
        if t <= 1.0 {
            t
        } else if t < 3.0 {
            1.0 + (t - 1.0) * 2.0
        } else {
            t + 2.0
        }
    }

    #[test]
    fn a_warp_moves_the_records_through_its_map() {
        let rig = Rig::new();
        let mut s = bed_and_voice(&rig, false);
        s.annotations = vec![marker(0.5), marker(2.0), region(2.0, 3.5)];
        let source = of_track(&s, 0);
        with_transcript(
            &mut s,
            source,
            vec![word("a", 0.5, 0.75), word("b", 2.0, 3.5)],
        );
        let edit = TimeEdit::Warp {
            start: 1.0,
            end: 3.0,
            out_len: 4.0,
            map: &slower,
        };
        let scope = Scope::new(0, SyncLock::Ignores, Clips::Slide, Words::Follow);
        let before = s.tracks.clone();
        let report = run(&mut s, edit, &scope);
        assert_eq!(times(&s), vec![(0.5, 0.5), (3.0, 3.0), (3.0, 5.5)]);
        assert_eq!(word_times(&s), vec![(0.5, 0.75), (3.0, 5.5)]);
        assert_eq!(report, TimeEditReport::default());
        // The edited track's audio was re-timed by the tool, not slid here.
        assert_eq!(s.tracks, before);
    }

    #[test]
    fn a_warp_that_gained_time_opens_that_much_in_the_others_at_the_spans_end() {
        let rig = Rig::new();
        let mut s = bed_and_voice(&rig, true);
        let edit = TimeEdit::Warp {
            start: 1.0,
            end: 3.0,
            out_len: 4.0,
            map: &slower,
        };
        let report = run(&mut s, edit, &rewritten(0, SyncLock::Follows));
        // Two seconds at 3 s: 32 000 frames of the voice, at 48 000.
        assert_eq!(
            shape(&s.tracks[1]),
            vec![(0, 0, 48_000), (80_000, 48_000, 16_000)]
        );
        assert_eq!(report.synced_tracks, 1);
    }

    #[test]
    fn a_warp_that_lost_time_takes_that_much_from_the_others_at_the_spans_end() {
        let rig = Rig::new();
        let mut s = bed_and_voice(&rig, true);
        let faster = |t: f64| t.min(1.0) + (t - 1.0).clamp(0.0, 2.0) * 0.5 + (t - 3.0).max(0.0);
        let edit = TimeEdit::Warp {
            start: 1.0,
            end: 3.0,
            out_len: 1.0,
            map: &faster,
        };
        let report = run(&mut s, edit, &rewritten(0, SyncLock::Follows));
        // [2 s, 3 s) goes: 16 000 frames of the voice from 32 000.
        assert_eq!(
            shape(&s.tracks[1]),
            vec![(0, 0, 32_000), (32_000, 48_000, 16_000)]
        );
        assert_eq!(report.synced_tracks, 1);
    }

    #[test]
    fn a_warp_that_lost_less_than_a_frame_cuts_nothing() {
        let rig = Rig::new();
        let mut s = bed_and_voice(&rig, true);
        let before = s.tracks.clone();
        let edit = TimeEdit::Warp {
            start: 1.0,
            end: 3.0,
            out_len: 2.0 - 1e-9,
            map: &slower,
        };
        let report = run(&mut s, edit, &rewritten(0, SyncLock::Follows));
        assert_eq!(s.tracks, before);
        assert_eq!(report.synced_tracks, 1);
    }

    #[test]
    fn a_warp_of_a_tool_that_ignores_sync_lock_leaves_the_others() {
        let rig = Rig::new();
        let mut s = bed_and_voice(&rig, true);
        let before = s.tracks.clone();
        let edit = TimeEdit::Warp {
            start: 1.0,
            end: 3.0,
            out_len: 4.0,
            map: &slower,
        };
        run(&mut s, edit, &rewritten(0, SyncLock::Ignores));
        assert_eq!(s.tracks, before);
    }

    // ---- Hollow ------------------------------------------------------------

    #[test]
    fn a_hollow_removes_the_audio_and_leaves_the_time_where_it_was() {
        let rig = Rig::new();
        let mut s = bed_and_voice(&rig, true);
        s.annotations = vec![marker(1.5), marker(3.0)];
        let source = of_track(&s, 0);
        with_transcript(&mut s, source, vec![word("a", 1.25, 1.75)]);
        let report = run(
            &mut s,
            TimeEdit::Hollow {
                start: 1.0,
                end: 2.0,
            },
            &sliding(0),
        );
        assert_eq!(
            shape(&s.tracks[0]),
            vec![(0, 0, 48_000), (96_000, 96_000, 96_000)]
        );
        assert_eq!(
            shape(&s.tracks[1]),
            vec![(0, 0, 16_000), (32_000, 32_000, 32_000)]
        );
        assert_eq!(report.synced_tracks, 1);
        // Nothing after it moved, so no label or word did.
        assert_eq!(times(&s), vec![(1.5, 1.5), (3.0, 3.0)]);
        assert_eq!(word_times(&s), vec![(1.25, 1.75)]);
    }

    // ---- Keep --------------------------------------------------------------

    #[test]
    fn a_keep_leaves_the_window_at_zero_and_drops_the_labels_outside_it() {
        let rig = Rig::new();
        let mut s = bed_and_voice(&rig, false);
        s.annotations = vec![
            marker(0.5),
            marker(1.0),
            marker(2.0),
            marker(3.0),
            marker(3.5),
        ];
        let source = of_track(&s, 0);
        with_transcript(
            &mut s,
            source,
            vec![
                word("a", 0.5, 0.75),
                word("b", 1.5, 2.0),
                word("c", 3.25, 3.75),
            ],
        );
        let edit = TimeEdit::Keep {
            start: 1.0,
            end: 3.0,
            until: 4.0,
        };
        let report = run(&mut s, edit, &sliding(0));
        assert_eq!(shape(&s.tracks[0]), vec![(0, 48_000, 96_000)]);
        assert_eq!(report.dropped_labels, 2);
        assert_eq!(times(&s), vec![(0.0, 0.0), (1.0, 1.0), (2.0, 2.0)]);
        assert_eq!(word_times(&s), vec![(0.5, 1.0)]);
    }

    #[test]
    fn a_keep_of_the_whole_track_splits_nothing() {
        let rig = Rig::new();
        let mut s = bed_and_voice(&rig, false);
        let before = s.tracks.clone();
        let edit = TimeEdit::Keep {
            start: 0.0,
            end: 4.0,
            until: 4.0,
        };
        run(&mut s, edit, &sliding(0));
        assert_eq!(s.tracks, before);
    }

    #[test]
    fn a_keep_under_sync_lock_keeps_the_same_seconds_of_the_others() {
        let rig = Rig::new();
        let mut s = bed_and_voice(&rig, true);
        let edit = TimeEdit::Keep {
            start: 1.0,
            end: 3.0,
            until: 4.0,
        };
        let report = run(&mut s, edit, &sliding(0));
        assert_eq!(shape(&s.tracks[1]), vec![(0, 16_000, 32_000)]);
        assert_eq!(report.synced_tracks, 1);
    }

    // ---- Shift -------------------------------------------------------------

    #[test]
    fn a_shift_moves_the_words_of_its_track_and_no_label() {
        let rig = Rig::new();
        let mut s = bed_and_voice(&rig, true);
        s.annotations = vec![marker(1.0)];
        let source = of_track(&s, 0);
        with_transcript(&mut s, source, vec![word("a", 1.0, 1.5)]);
        let before = s.tracks.clone();
        let scope = Scope::new(0, SyncLock::Ignores, Clips::Rewritten, Words::Follow);
        let edit = TimeEdit::Shift {
            by: 2.0,
            starting_in: None,
        };
        let report = apply(&mut s, edit, &scope, &Rates::default());
        assert_eq!(word_times(&s), vec![(3.0, 3.5)]);
        assert_eq!(times(&s), vec![(1.0, 1.0)]);
        assert_eq!(s.tracks, before);
        assert_eq!(report, TimeEditReport::default());
    }

    #[test]
    fn a_shift_leaves_a_transcript_that_is_not_its_tracks() {
        let rig = Rig::new();
        let edit = TimeEdit::Shift {
            by: 2.0,
            starting_in: None,
        };
        let scope = Scope::new(0, SyncLock::Ignores, Clips::Rewritten, Words::Follow);
        for source in [
            Some(TranscriptSource::File),
            None,
            Some(TranscriptSource::Track {
                track: TrackId::new(),
            }),
        ] {
            let mut s = bed_and_voice(&rig, false);
            with_transcript(&mut s, source, vec![word("a", 1.0, 1.5)]);
            apply(&mut s, edit, &scope, &Rates::default());
            assert_eq!(word_times(&s), vec![(1.0, 1.5)]);
        }
        let mut s = bed_and_voice(&rig, false);
        let source = of_track(&s, 1);
        with_transcript(&mut s, source, vec![word("a", 1.0, 1.5)]);
        apply(&mut s, edit, &scope, &Rates::default());
        assert_eq!(word_times(&s), vec![(1.0, 1.5)]);
    }

    #[test]
    fn a_shift_with_no_transcript_or_no_such_track_or_no_distance_changes_nothing() {
        let rig = Rig::new();
        let rates = Rates::default();
        let scope = Scope::new(0, SyncLock::Ignores, Clips::Rewritten, Words::Follow);
        let shift = |by| TimeEdit::Shift {
            by,
            starting_in: None,
        };

        let mut s = bed_and_voice(&rig, false);
        apply(&mut s, shift(2.0), &scope, &rates);
        assert!(s.transcript.is_none());

        let mut s = bed_and_voice(&rig, false);
        let source = of_track(&s, 0);
        with_transcript(&mut s, source, vec![word("a", 1.0, 1.5)]);
        let elsewhere = Scope::new(9, SyncLock::Ignores, Clips::Rewritten, Words::Follow);
        apply(&mut s, shift(2.0), &elsewhere, &rates);
        apply(&mut s, shift(0.0), &scope, &rates);
        assert_eq!(word_times(&s), vec![(1.0, 1.5)]);
    }

    #[test]
    fn a_shift_earlier_than_the_start_stops_at_zero() {
        let rig = Rig::new();
        let mut s = bed_and_voice(&rig, false);
        let source = of_track(&s, 0);
        with_transcript(
            &mut s,
            source,
            vec![word("a", 1.0, 1.5), word("b", 3.0, 3.5)],
        );
        let scope = Scope::new(0, SyncLock::Ignores, Clips::Rewritten, Words::Follow);
        let edit = TimeEdit::Shift {
            by: -2.0,
            starting_in: None,
        };
        apply(&mut s, edit, &scope, &Rates::default());
        assert_eq!(word_times(&s), vec![(0.0, 0.0), (1.0, 1.5)]);
    }

    #[test]
    fn a_shift_of_one_clip_moves_its_words_and_puts_them_back_in_order() {
        let rig = Rig::new();
        let mut s = bed_and_voice(&rig, false);
        let source = of_track(&s, 0);
        with_transcript(
            &mut s,
            source,
            vec![
                word("a", 1.0, 1.5),
                word("b", 2.5, 3.0),
                word("c", 5.0, 5.5),
            ],
        );
        let scope = Scope::new(0, SyncLock::Ignores, Clips::Rewritten, Words::Follow);
        let edit = TimeEdit::Shift {
            by: 2.0,
            starting_in: Some((1.0, 2.0)),
        };
        apply(&mut s, edit, &scope, &Rates::default());
        let words = &s.transcript.as_ref().unwrap().words;
        let order: Vec<&str> = words.iter().map(|w| w.text.as_str()).collect();
        assert_eq!(order, ["b", "a", "c"]);
        assert_eq!(word_times(&s), vec![(2.5, 3.0), (3.0, 3.5), (5.0, 5.5)]);
    }

    // ---- Rates -------------------------------------------------------------

    #[test]
    fn rates_under_sync_lock_are_every_tracks_own() {
        let rig = Rig::new();
        let s = bed_and_voice(&rig, true);
        let rates = sliding(0).rates(&s).unwrap();
        assert_eq!(rates.0, vec![R48, R16]);
    }

    #[test]
    fn rates_without_sync_lock_are_the_slid_tracks_own_only() {
        let rig = Rig::new();
        let s = bed_and_voice(&rig, false);
        assert_eq!(sliding(1).rates(&s).unwrap().0, vec![R48, R16]);
        // The bed's rate is the session's, so the one asked about is told
        // apart from the fallback by asking about the voice.
        assert_eq!(sliding(0).rates(&s).unwrap().0, vec![R48, R48]);
    }

    #[test]
    fn rewritten_audio_asks_no_file_anything() {
        let mut s = state_of(
            vec![track_of(vec![clip_of(
                Path::new("/no/such/file.wav"),
                0,
                10,
            )])],
            false,
        );
        let rates = rewritten(0, SyncLock::Ignores).rates(&s).unwrap();
        assert!(rates.0.is_empty());
        // Not even when the session is locked and the tool ignores it.
        s.sync_lock = true;
        assert!(rewritten(0, SyncLock::Ignores).rates(&s).is_ok());
    }

    #[test]
    fn rates_of_a_known_edited_track_name_no_other() {
        let rig = Rig::new();
        let s = bed_and_voice(&rig, false);
        assert_eq!(Rates::of_edited(&s, 1, R16).0, vec![R48, R16]);
        // A track the state does not have is no slot to fill.
        assert_eq!(Rates::of_edited(&s, 5, R16).0, vec![R48, R48]);
    }

    #[test]
    fn a_track_at_two_rates_refuses_before_anything_is_edited() {
        let rig = Rig::new();
        let mixed = track_of(vec![rig.clip(48_000, 0, 10), rig.clip(16_000, 10, 10)]);

        // The edited track, slid: its own rate is not one number.
        let s = state_of(vec![mixed.clone()], false);
        let msg = sliding(0).rates(&s).unwrap_err();
        assert!(msg.contains("different sample rates"), "{msg}");

        // An other track, carried to by sync-lock.
        let mut s = bed_and_voice(&rig, true);
        s.tracks.push(mixed.clone());
        let msg = sliding(0).rates(&s).unwrap_err();
        assert!(msg.contains("different sample rates"), "{msg}");

        // Not carried to: nothing asks about it.
        s.sync_lock = false;
        assert!(sliding(0).rates(&s).is_ok());
    }

    #[test]
    fn a_track_with_no_rates_asked_counts_at_the_sessions_rate() {
        let rig = Rig::new();
        let mut s = bed_and_voice(&rig, false);
        apply(
            &mut s,
            TimeEdit::Cut {
                start: 1.0,
                end: 2.0,
            },
            &sliding(1),
            &Rates::default(),
        );
        // 48 000 frames a second, which is wrong for the voice (its clip is
        // 64 000 frames, cut from 48 000 to 96 000), and the reason `rates`
        // is asked and not guessed.
        assert_eq!(shape(&s.tracks[1]), vec![(0, 0, 48_000)]);
    }

    // ---- Properties --------------------------------------------------------

    #[test]
    fn nothing_before_an_edit_moves() {
        let rig = Rig::new();
        let edits = [
            TimeEdit::Cut {
                start: 1.0,
                end: 2.0,
            },
            TimeEdit::Gap { at: 1.0, len: 0.5 },
            TimeEdit::Hollow {
                start: 1.0,
                end: 2.0,
            },
            TimeEdit::Warp {
                start: 1.0,
                end: 3.0,
                out_len: 4.0,
                map: &slower,
            },
        ];
        for edit in edits {
            let mut s = bed_and_voice(&rig, true);
            s.annotations = vec![marker(0.0), marker(0.5), region(0.25, 0.75)];
            let source = of_track(&s, 0);
            with_transcript(&mut s, source, vec![word("a", 0.25, 0.75)]);
            let (labels, words) = (times(&s), word_times(&s));
            let scope = sliding(0);
            run(&mut s, edit, &scope);
            assert_eq!(times(&s), labels);
            assert_eq!(word_times(&s), words);
            // The first clip's head, up to the edit, is where it was.
            assert_eq!(s.tracks[0].clips[0].start_in_track, 0);
            assert_eq!(s.tracks[1].clips[0].start_in_track, 0);
        }
    }

    #[test]
    fn a_marker_after_a_cut_moves_by_the_cuts_length() {
        let rig = Rig::new();
        for (start, end) in [(0.0, 0.5), (1.0, 2.0), (0.25, 3.0), (1.5, 1.75)] {
            for at in [end, end + 0.25, end + 1.0, 100.0] {
                let mut s = bed_and_voice(&rig, false);
                s.annotations = vec![marker(at)];
                run(&mut s, TimeEdit::Cut { start, end }, &sliding(0));
                let (moved, _) = times(&s)[0];
                assert!((moved - (at - (end - start)).max(start)).abs() < 1e-12);
            }
        }
    }

    #[test]
    fn tracks_at_different_rates_each_move_by_the_same_seconds_in_their_own_frames() {
        let rig = Rig::new();
        for (start, end) in [(0.5, 1.0), (1.0, 2.0), (1.25, 3.5)] {
            let mut s = bed_and_voice(&rig, true);
            run(&mut s, TimeEdit::Cut { start, end }, &sliding(0));
            for (track, rate) in [(0, R48), (1, R16)] {
                let kept = &s.tracks[track].clips;
                let last = kept.last().unwrap();
                // Length in seconds is what was there, less the cut.
                let seconds = (last.start_in_track + last.length) as f64 / rate;
                assert!((seconds - (4.0 - (end - start))).abs() < 1e-9, "{track}");
            }
        }
    }
}
