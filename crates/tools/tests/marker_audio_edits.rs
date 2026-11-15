//! Marker-driven edits (#766, scope item 2): `edit_at_markers` cuts, deletes,
//! silences, splits or joins the audio under one or more markers.
//!
//! The tool is a thin reader of markers over the internals the time-range
//! tools already use (`cut_range`, `silence_region`, `split_clip`,
//! `join_clips`), so the tests that matter most say the two agree: the same
//! span, cut under a marker or by `cut_range`, leaves the same session. The
//! rest pin what is the marker tool's own: how a marker is named, what a span
//! is, what a call refuses (and that a refusal appends no node), one node a
//! call, and that sync-lock and the labels follow a cut as they do for
//! `cut_range`.
//!
//! The session is two ten-second tracks at 48 kHz (a host and a guest), so a
//! second is 48 000 samples and a marker at `2.0` is sample 96 000.

use std::path::Path;

use serde_json::{json, Value};
use session::{Annotation, AnnotationKind, SessionState};
use tools::ToolDispatcher;

#[path = "support/harness.rs"]
mod harness;
use harness::{write_frames, write_tone, Session};

const RATE: u32 = 48_000;

/// Two tracks of ten seconds each, with `marks` placed by `add_markers`
/// (a `Value` per marker: `{name, time}` or `{name, start, end}`).
fn session_with(marks: Value) -> Session {
    let mut s = Session::empty();
    for name in ["host", "guest"] {
        let wav = write_tone(&s.dir.path().join(format!("{name}.wav")), RATE, 10);
        s.load(&wav);
    }
    if marks.as_array().is_some_and(|m| !m.is_empty()) {
        s.ok("add_markers", json!({ "markers": marks }));
    }
    s
}

impl Session {
    fn at_markers(&mut self, action: &str, track: usize, markers: &[&str]) -> Value {
        self.ok(
            "edit_at_markers",
            json!({ "action": action, "track": track, "markers": markers }),
        )
    }

    fn refused_at_markers(&mut self, args: Value) -> String {
        let before = self.head();
        let nodes = self.node_count();
        // A refusal is the tool's own, or the schema's before the tool runs.
        let message = match self.try_call("edit_at_markers", args) {
            Ok(tools::ToolResult::Error(m)) => m,
            Ok(tools::ToolResult::Ok(v)) => panic!("expected a refusal, got Ok({v})"),
            Err(e) => e.to_string(),
        };
        assert_eq!(self.head(), before, "a refusal moved the head");
        assert_eq!(self.node_count(), nodes, "a refusal appended a node");
        message
    }

    fn clips(&self, track: usize) -> Vec<(u64, u64, u64)> {
        self.state().tracks[track]
            .clips
            .iter()
            .map(|c| (c.start_in_track, c.source_offset, c.length))
            .collect()
    }

    fn len(&self, track: usize) -> u64 {
        self.clips(track)
            .iter()
            .map(|(start, _, len)| start + len)
            .max()
            .unwrap_or(0)
    }

    /// `(name, kind)` of every label, in stored order. The ids are random per
    /// session, so a comparison across sessions leaves them out.
    fn labels(&self) -> Vec<(String, AnnotationKind)> {
        self.state()
            .annotations
            .into_iter()
            .map(|a| (a.name, a.kind))
            .collect()
    }

    fn label_time(&self, name: &str) -> f64 {
        match self.find(name).kind {
            AnnotationKind::Marker { time_sec } => time_sec,
            AnnotationKind::Region { start_sec, .. } => start_sec,
        }
    }

    fn find(&self, name: &str) -> Annotation {
        self.state()
            .annotations
            .into_iter()
            .find(|a| a.name == name)
            .unwrap_or_else(|| panic!("no label named {name:?}"))
    }
}

/// The tracks of a state as JSON: clips, gains, effects, everything a render
/// reads. Two sessions made the same way have the same tracks, but for what
/// is random or per-folder: a track's id, and the folder its source sits in
/// (an edit that wrote audio names the file by its content, so the file name
/// stays).
fn tracks_json(state: &SessionState) -> Value {
    let mut tracks = serde_json::to_value(&state.tracks).expect("tracks serialise");
    for track in tracks.as_array_mut().expect("an array") {
        track.as_object_mut().expect("a track").remove("id");
        for clip in track["clips"].as_array_mut().expect("clips") {
            let name = Path::new(clip["source_path"].as_str().expect("a path"))
                .file_name()
                .map(|n| n.to_string_lossy().into_owned());
            clip["source_path"] = json!(name);
        }
    }
    tracks
}

fn names(v: &Value) -> String {
    v["summary"].as_str().unwrap_or_default().to_string()
}

// ---------------------------------------------------------------------
// cut
// ---------------------------------------------------------------------

/// The contract: cutting under a marker is `cut_range` on the marker's span,
/// byte for byte (tracks, length, labels).
#[test]
fn a_cut_under_a_region_is_cut_range_on_its_span() {
    let marks =
        json!([{ "name": "um", "start": 2.0, "end": 3.0 }, { "name": "later", "time": 6.0 }]);
    let mut by_range = session_with(marks.clone());
    by_range.ok(
        "cut_range",
        json!({ "track": 0, "start_sample": 96_000, "end_sample": 144_000 }),
    );
    let mut by_marker = session_with(marks);
    let v = by_marker.at_markers("cut", 0, &["um"]);

    let (a, b) = (by_range.state(), by_marker.state());
    assert_eq!(tracks_json(&a), tracks_json(&b), "the tracks differ");
    assert_eq!(a.length_samples, b.length_samples);
    assert_eq!(a.sync_lock, b.sync_lock);
    assert_eq!(by_range.labels(), by_marker.labels(), "the labels differ");
    assert_eq!(v["dropped_labels"], json!(1), "the cut region goes: {v}");
    assert_eq!(by_marker.len(0), 10 * RATE as u64 - 48_000);
}

#[test]
fn several_cuts_are_one_node_and_equal_the_cuts_made_latest_first() {
    let marks = json!([
        { "name": "a", "start": 1.0, "end": 2.0 },
        { "name": "b", "start": 4.0, "end": 4.5 },
        { "name": "c", "start": 7.0, "end": 9.0 },
        { "name": "kept", "time": 9.5 },
    ]);
    let mut by_range = session_with(marks.clone());
    for (s, e) in [(336_000, 432_000), (192_000, 216_000), (48_000, 96_000)] {
        by_range.ok(
            "cut_range",
            json!({ "track": 0, "start_sample": s, "end_sample": e }),
        );
    }
    let mut by_marker = session_with(marks);
    let before = by_marker.node_count();
    // Named out of time order: the tool orders them.
    let v = by_marker.at_markers("cut", 0, &["b", "c", "a"]);

    assert_eq!(
        by_marker.node_count(),
        before + 1,
        "one call, one node (one undo)"
    );
    assert_eq!(
        tracks_json(&by_range.state()),
        tracks_json(&by_marker.state())
    );
    assert_eq!(by_range.labels(), by_marker.labels());
    assert_eq!(v["dropped_labels"], json!(3), "{v}");
    // 1 + 0.5 + 2 seconds are gone, and the mark after them moved by that.
    assert_eq!(by_marker.len(0), 10 * RATE as u64 - 168_000);
    assert!((by_marker.label_time("kept") - 6.0).abs() < 1e-9);
}

#[test]
fn a_cut_names_its_markers_in_the_node_label() {
    let mut s = session_with(json!([{ "name": "um", "start": 2.0, "end": 3.0 }]));
    s.at_markers("cut", 0, &["um"]);
    let node = s.store.get(s.head()).expect("head");
    let label = node.label.unwrap_or_default();
    assert!(label.contains("edit_at_markers cut"), "{label}");
    assert!(label.contains("um"), "{label}");
}

#[test]
fn a_cut_by_id_is_a_cut_by_name() {
    let marks = json!([{ "name": "um", "start": 2.0, "end": 3.0 }]);
    let mut by_name = session_with(marks.clone());
    by_name.at_markers("cut", 0, &["um"]);
    let mut by_id = session_with(marks);
    let id = by_id.find("um").id.0.to_string();
    by_id.at_markers("cut", 0, &[&id]);
    assert_eq!(tracks_json(&by_name.state()), tracks_json(&by_id.state()));
}

#[test]
fn a_name_is_matched_whole_and_without_regard_to_case() {
    let mut s = session_with(json!([
        { "name": "Intro", "start": 0.0, "end": 1.0 },
        { "name": "Introduction", "start": 5.0, "end": 6.0 },
    ]));
    // "intro" is not a part of a name here, it is a name: only `Intro`.
    s.at_markers("cut", 0, &["intro"]);
    assert_eq!(s.len(0), 10 * RATE as u64 - 48_000);
    assert!(s
        .state()
        .annotations
        .iter()
        .any(|a| a.name == "Introduction"));
}

#[test]
fn a_point_marker_is_cut_with_a_width_and_stays_at_the_seam() {
    let mut s = session_with(json!([
        { "name": "pop", "time": 2.0 },
        { "name": "after", "time": 5.0 },
    ]));
    let v = s.ok(
        "edit_at_markers",
        json!({ "action": "cut", "track": 0, "markers": ["pop"], "width": 0.5 }),
    );
    assert_eq!(s.len(0), 10 * RATE as u64 - 24_000);
    // The point at the cut's first instant is kept at the seam, as
    // `cut_range` keeps a mark on a cut's edge; the one after moves earlier.
    assert_eq!(v["dropped_labels"], json!(0), "{v}");
    assert!((s.label_time("pop") - 2.0).abs() < 1e-9);
    assert!((s.label_time("after") - 4.5).abs() < 1e-9);
}

#[test]
fn a_point_marker_without_a_width_is_refused_for_every_action_that_needs_a_span() {
    let mut s = session_with(json!([{ "name": "pop", "time": 2.0 }]));
    for action in ["cut", "delete", "silence", "join"] {
        let m = s.refused_at_markers(json!({ "action": action, "track": 0, "markers": ["pop"] }));
        assert!(m.contains("'pop'"), "{m}");
        assert!(m.contains("point marker"), "{m}");
        assert!(m.contains("width"), "{m}");
    }
}

#[test]
fn a_width_that_is_not_a_positive_number_is_refused() {
    let mut s = session_with(json!([{ "name": "pop", "time": 2.0 }]));
    for width in [json!(0.0), json!(-1.0)] {
        let m = s.refused_at_markers(
            json!({ "action": "cut", "track": 0, "markers": ["pop"], "width": width }),
        );
        assert!(m.contains("width"), "{m}");
    }
}

#[test]
fn a_cut_past_the_end_of_the_track_is_refused_as_cut_range_refuses_it() {
    let mut s = session_with(json!([{ "name": "late", "start": 9.5, "end": 12.0 }]));
    let m = s.refused_at_markers(json!({ "action": "cut", "track": 0, "markers": ["late"] }));
    assert!(m.contains("'late'"), "{m}");
    assert!(m.contains("exceeds track length"), "{m}");
}

#[test]
fn a_span_before_zero_is_refused() {
    let mut s = session_with(json!([]));
    // `label` takes a negative time that `add_markers` would refuse.
    s.ok(
        "label",
        json!({ "name": "early", "start": -1.0, "end": 1.0 }),
    );
    let m = s.refused_at_markers(json!({ "action": "cut", "track": 0, "markers": ["early"] }));
    assert!(m.contains("'early'"), "{m}");
    assert!(m.contains("negative"), "{m}");
}

#[test]
fn a_track_with_no_clips_has_nothing_to_cut() {
    let mut s = Session::with_annotations(vec![harness::region("um", 0.0, 1.0)], 0);
    let m = s.refused_at_markers(json!({ "action": "cut", "track": 0, "markers": ["um"] }));
    assert!(m.contains("track"), "{m}");
}

#[test]
fn a_track_whose_clips_are_gone_has_nothing_to_remove_or_silence() {
    let mut s = session_with(json!([{ "name": "um", "start": 2.0, "end": 3.0 }]));
    let mut state = s.state();
    state.tracks[0].clips.clear();
    s.append_state("fixture: an empty track", state);
    for action in ["cut", "delete", "silence"] {
        let m = s.refused_at_markers(json!({ "action": action, "track": 0, "markers": ["um"] }));
        assert!(m.contains("no clips"), "{action}: {m}");
    }
}

// ---------------------------------------------------------------------
// sync-lock and the labels
// ---------------------------------------------------------------------

#[test]
fn with_sync_lock_a_cut_takes_the_same_span_from_every_track() {
    let marks = json!([
        { "name": "um", "start": 2.0, "end": 3.0 },
        { "name": "after", "time": 6.0 },
    ]);
    let mut by_range = session_with(marks.clone());
    by_range.ok("set_sync_lock", json!({ "enabled": true }));
    by_range.ok(
        "cut_range",
        json!({ "track": 0, "start_sample": 96_000, "end_sample": 144_000 }),
    );
    let mut s = session_with(marks);
    s.ok("set_sync_lock", json!({ "enabled": true }));
    let v = s.at_markers("cut", 0, &["um"]);

    assert_eq!(v["synced_tracks"], json!(1), "{v}");
    assert_eq!(
        s.len(1),
        10 * RATE as u64 - 48_000,
        "the guest loses the second too"
    );
    assert_eq!(tracks_json(&by_range.state()), tracks_json(&s.state()));
    assert_eq!(by_range.labels(), s.labels());
    assert!((s.label_time("after") - 5.0).abs() < 1e-9);
}

#[test]
fn without_sync_lock_the_other_tracks_are_left_alone_but_the_labels_still_move() {
    let mut s = session_with(json!([
        { "name": "um", "start": 2.0, "end": 3.0 },
        { "name": "after", "time": 6.0 },
    ]));
    let guest = s.clips(1);
    let v = s.at_markers("cut", 0, &["um"]);

    assert_eq!(v["synced_tracks"], json!(0), "{v}");
    assert_eq!(
        s.clips(1),
        guest,
        "sync-lock is off: the guest does not move"
    );
    assert_eq!(s.len(0), 10 * RATE as u64 - 48_000);
    // Labels are the session's, not a track's: they follow every cut, as
    // they do for `cut_range` (#203).
    assert!((s.label_time("after") - 5.0).abs() < 1e-9);
}

#[test]
fn a_cut_carries_the_transcript_of_a_locked_track_with_it_and_only_then() {
    let words = [("hello", 1.0, 1.5), ("there", 6.0, 6.5)];
    for (locked, want) in [(true, 5.0), (false, 6.0)] {
        let mut s = session_with(json!([{ "name": "um", "start": 2.0, "end": 3.0 }]));
        s.with_track_transcript(1, &words);
        if locked {
            s.ok("set_sync_lock", json!({ "enabled": true }));
        }
        // Cut track 0; the transcript is of track 1, so it moves only when
        // the lock carried the cut to track 1 (#925).
        s.at_markers("cut", 0, &["um"]);
        let t = s.state().transcript.expect("a transcript");
        assert!(
            (t.words[1].start_s - want).abs() < 1e-3,
            "sync-lock {locked}: {:?}",
            t.words
        );
    }
}

#[test]
fn sync_lock_carries_a_cut_across_tracks_of_different_rates() {
    let mut s = Session::empty();
    let a = write_frames(&s.dir.path().join("a.wav"), 48_000, 1, 480_000, |n| {
        (n % 100) as f32 / 400.0
    });
    let b = write_frames(&s.dir.path().join("b.wav"), 16_000, 1, 160_000, |n| {
        (n % 100) as f32 / 400.0
    });
    s.load(&a);
    s.load(&b);
    s.ok(
        "add_markers",
        json!({ "markers": [{ "name": "um", "start": 2.0, "end": 3.0 }] }),
    );
    s.ok("set_sync_lock", json!({ "enabled": true }));
    s.at_markers("cut", 0, &["um"]);
    assert_eq!(s.len(0), 480_000 - 48_000);
    assert_eq!(
        s.len(1),
        160_000 - 16_000,
        "a second is 16 000 frames of the voice"
    );
}

// ---------------------------------------------------------------------
// delete
// ---------------------------------------------------------------------

#[test]
fn a_delete_removes_the_audio_and_leaves_the_gap_in_place() {
    let mut s = session_with(json!([
        { "name": "um", "start": 2.0, "end": 3.0 },
        { "name": "after", "time": 6.0 },
    ]));
    let labels = s.labels();
    let v = s.at_markers("delete", 0, &["um"]);

    // Nothing after the gap moved, nothing is there inside it.
    assert_eq!(
        s.clips(0),
        vec![(0, 0, 96_000), (144_000, 144_000, 336_000)],
        "{v}"
    );
    assert_eq!(s.len(0), 10 * RATE as u64);
    assert_eq!(s.labels(), labels, "a delete moves no label");
}

#[test]
fn a_delete_follows_sync_lock_by_the_same_seconds() {
    let mut s = session_with(json!([{ "name": "um", "start": 2.0, "end": 3.0 }]));
    s.ok("set_sync_lock", json!({ "enabled": true }));
    let v = s.at_markers("delete", 0, &["um"]);
    assert_eq!(v["synced_tracks"], json!(1), "{v}");
    assert_eq!(s.clips(0), s.clips(1));
    assert_eq!(
        s.clips(1),
        vec![(0, 0, 96_000), (144_000, 144_000, 336_000)]
    );

    let mut off = session_with(json!([{ "name": "um", "start": 2.0, "end": 3.0 }]));
    let guest = off.clips(1);
    off.at_markers("delete", 0, &["um"]);
    assert_eq!(off.clips(1), guest);
}

#[test]
fn several_deletes_are_one_node() {
    let mut s = session_with(json!([
        { "name": "a", "start": 1.0, "end": 2.0 },
        { "name": "b", "start": 5.0, "end": 6.0 },
    ]));
    let before = s.node_count();
    s.at_markers("delete", 0, &["a", "b"]);
    assert_eq!(s.node_count(), before + 1);
    assert_eq!(
        s.clips(0),
        vec![
            (0, 0, 48_000),
            (96_000, 96_000, 144_000),
            (288_000, 288_000, 192_000)
        ]
    );
}

// ---------------------------------------------------------------------
// silence
// ---------------------------------------------------------------------

#[test]
fn a_silence_under_a_region_is_silence_region_on_its_span() {
    let marks = json!([{ "name": "um", "start": 2.0, "end": 3.0 }]);
    let mut by_range = session_with(marks.clone());
    by_range.ok(
        "silence_region",
        json!({ "track": 0, "start_sec": 2.0, "end_sec": 3.0 }),
    );
    let mut by_marker = session_with(marks);
    by_marker.at_markers("silence", 0, &["um"]);

    assert_eq!(
        tracks_json(&by_range.state()),
        tracks_json(&by_marker.state())
    );
    assert_eq!(by_range.render_pcm(None), by_marker.render_pcm(None));
    // Silence moves nothing: no cut, no gap, the labels stay.
    assert_eq!(by_range.labels(), by_marker.labels());
}

#[test]
fn several_silences_are_one_node_and_both_spans_are_zero() {
    let mut s = session_with(json!([
        { "name": "a", "start": 1.0, "end": 2.0 },
        { "name": "b", "start": 5.0, "end": 6.0 },
    ]));
    let before = s.node_count();
    s.at_markers("silence", 0, &["a", "b"]);
    assert_eq!(s.node_count(), before + 1);

    let mut by_range = session_with(json!([]));
    by_range.ok(
        "silence_region",
        json!({ "track": 0, "start_sec": 1.0, "end_sec": 2.0 }),
    );
    by_range.ok(
        "silence_region",
        json!({ "track": 0, "start_sec": 5.0, "end_sec": 6.0 }),
    );
    assert_eq!(s.render_pcm(None), by_range.render_pcm(None));
}

#[test]
fn a_silence_does_not_follow_sync_lock() {
    let mut s = session_with(json!([{ "name": "um", "start": 2.0, "end": 3.0 }]));
    s.ok("set_sync_lock", json!({ "enabled": true }));
    let guest = s.clips(1);
    s.at_markers("silence", 0, &["um"]);
    assert_eq!(
        s.clips(1),
        guest,
        "silence_region leaves the other tracks alone"
    );
}

// ---------------------------------------------------------------------
// split
// ---------------------------------------------------------------------

#[test]
fn a_split_under_a_region_splits_at_its_start_and_its_end() {
    let marks = json!([{ "name": "um", "start": 2.0, "end": 3.0 }]);
    let mut by_clip = session_with(marks.clone());
    by_clip.ok(
        "split_clip",
        json!({ "track": 0, "clip_index": 0, "at_sec": 2.0 }),
    );
    by_clip.ok(
        "split_clip",
        json!({ "track": 0, "clip_index": 1, "at_sec": 3.0 }),
    );
    let mut s = session_with(marks);
    let v = s.at_markers("split", 0, &["um"]);

    assert_eq!(v["splits"], json!(2), "{v}");
    assert_eq!(tracks_json(&by_clip.state()), tracks_json(&s.state()));
    assert_eq!(s.clips(0).len(), 3);
}

#[test]
fn a_split_at_a_point_marker_needs_no_width() {
    let mut s = session_with(json!([{ "name": "here", "time": 4.0 }]));
    let v = s.at_markers("split", 0, &["here"]);
    assert_eq!(v["splits"], json!(1), "{v}");
    assert_eq!(
        s.clips(0),
        vec![(0, 0, 192_000), (192_000, 192_000, 288_000)]
    );
}

#[test]
fn a_split_leaves_a_clip_edge_alone_and_says_so() {
    let mut s = session_with(json!([
        { "name": "start", "time": 0.0 },
        { "name": "mid", "time": 4.0 },
    ]));
    let v = s.at_markers("split", 0, &["start", "mid"]);
    assert_eq!(v["splits"], json!(1), "{v}");
    assert_eq!(v["skipped"], json!(1), "{v}");
    assert_eq!(s.clips(0).len(), 2);
}

#[test]
fn a_split_that_splits_nothing_is_refused() {
    let mut s = session_with(json!([
        { "name": "start", "time": 0.0 },
        { "name": "end", "time": 10.0 },
        { "name": "past", "time": 30.0 },
    ]));
    let m = s.refused_at_markers(
        json!({ "action": "split", "track": 0, "markers": ["start", "end", "past"] }),
    );
    assert!(m.contains("nothing to split"), "{m}");
}

#[test]
fn a_split_in_a_gap_of_the_track_splits_nothing() {
    let mut s = session_with(json!([
        { "name": "um", "start": 2.0, "end": 3.0 },
        { "name": "in-gap", "time": 2.5 },
    ]));
    s.at_markers("delete", 0, &["um"]);
    let m = s.refused_at_markers(json!({ "action": "split", "track": 0, "markers": ["in-gap"] }));
    assert!(m.contains("nothing to split"), "{m}");
}

#[test]
fn a_split_ignores_a_width() {
    let mut s = session_with(json!([{ "name": "here", "time": 4.0 }]));
    let v = s.ok(
        "edit_at_markers",
        json!({ "action": "split", "track": 0, "markers": ["here"], "width": 2.0 }),
    );
    assert_eq!(v["splits"], json!(1), "{v}");
}

#[test]
fn two_markers_at_one_time_split_once() {
    let mut s = session_with(json!([
        { "name": "x", "time": 4.0 },
        { "name": "y", "time": 4.0 },
    ]));
    let v = s.at_markers("split", 0, &["x", "y"]);
    assert_eq!(v["splits"], json!(1), "{v}");
    assert_eq!(s.clips(0).len(), 2);
}

// ---------------------------------------------------------------------
// join
// ---------------------------------------------------------------------

/// A track of clips cut at 2, 2.5 and 3 seconds, so a region over 2..3 holds
/// two of them.
fn split_session(marks: Value) -> Session {
    let mut s = session_with(marks);
    for at in [2.0, 2.5, 3.0] {
        s.at_marker_free_split(at);
    }
    s
}

impl Session {
    fn at_marker_free_split(&mut self, at: f64) {
        let clips = self.state().tracks[0].clips.clone();
        let index = clips
            .iter()
            .position(|c| {
                let (s, e) = (c.start_in_track, c.start_in_track + c.length);
                (at * RATE as f64) as u64 > s && ((at * RATE as f64) as u64) < e
            })
            .expect("a clip plays there");
        self.ok(
            "split_clip",
            json!({ "track": 0, "clip_index": index, "at_sec": at }),
        );
    }
}

#[test]
fn a_join_under_a_region_is_join_clips_on_the_clips_wholly_under_it() {
    let marks = json!([{ "name": "um", "start": 2.0, "end": 3.0 }]);
    let mut by_clips = split_session(marks.clone());
    assert_eq!(by_clips.clips(0).len(), 4);
    by_clips.ok(
        "join_clips",
        json!({ "track": 0, "first_clip": 1, "last_clip": 2 }),
    );
    let mut s = split_session(marks);
    let v = s.at_markers("join", 0, &["um"]);

    assert_eq!(v["joined_clips"], json!(2), "{v}");
    assert_eq!(v["rendered"], json!(false), "back to back in one file: {v}");
    assert_eq!(tracks_json(&by_clips.state()), tracks_json(&s.state()));
    assert_eq!(s.clips(0).len(), 3);
}

#[test]
fn several_joins_are_one_node() {
    let mut s = session_with(json!([
        { "name": "a", "start": 0.0, "end": 2.0 },
        { "name": "b", "start": 2.0, "end": 3.0 },
    ]));
    for at in [1.0, 2.0, 2.5, 3.0] {
        s.at_marker_free_split(at);
    }
    // Clips: [0,1) [1,2) [2,2.5) [2.5,3) [3,10): a holds two, b holds two.
    let before = s.node_count();
    let v = s.at_markers("join", 0, &["a", "b"]);
    assert_eq!(s.node_count(), before + 1, "one node");
    assert_eq!(v["joined_clips"], json!(4), "{v}");
    assert_eq!(s.clips(0).len(), 3, "[0,2) [2,3) [3,10)");
}

#[test]
fn overlapping_markers_are_refused_for_a_join() {
    let mut s = split_session(json!([
        { "name": "um", "start": 2.0, "end": 3.0 },
        { "name": "x", "start": 0.0, "end": 2.5 },
    ]));
    let m = s.refused_at_markers(json!({ "action": "join", "track": 0, "markers": ["um", "x"] }));
    assert!(m.contains("overlap"), "{m}");
}

#[test]
fn a_join_with_fewer_than_two_clips_under_the_marker_is_refused() {
    let mut s = session_with(json!([{ "name": "um", "start": 2.0, "end": 3.0 }]));
    let m = s.refused_at_markers(json!({ "action": "join", "track": 0, "markers": ["um"] }));
    assert!(m.contains("'um'"), "{m}");
    assert!(m.contains("at least two"), "{m}");
}

#[test]
fn a_join_whose_clips_are_not_one_run_is_refused() {
    let mut s = session_with(json!([{ "name": "wide", "start": 2.0, "end": 8.0 }]));
    for at in [2.0, 3.0, 8.0] {
        s.at_marker_free_split(at);
    }
    // Clips: [0,2) [2,3) [3,8) [8,10). Lay a clip that begins before the
    // span and ends inside it between them: it is not wholly under the
    // marker, and it sits between two that are.
    let mut state = s.state();
    let mut straddler = state.tracks[0].clips[1].clone();
    straddler.start_in_track = RATE as u64;
    straddler.length = 3 * RATE as u64 / 2 + RATE as u64 / 2;
    state.tracks[0].clips.insert(2, straddler);
    s.append_state("fixture: an overlapping clip", state);
    let m = s.refused_at_markers(json!({ "action": "join", "track": 0, "markers": ["wide"] }));
    assert!(m.contains("'wide'"), "{m}");
    assert!(m.contains("not one run"), "{m}");
}

// ---------------------------------------------------------------------
// resolving markers, and what is refused
// ---------------------------------------------------------------------

#[test]
fn an_unknown_marker_is_refused_with_a_way_to_find_it() {
    let mut s = session_with(json!([{ "name": "um", "start": 2.0, "end": 3.0 }]));
    let m = s.refused_at_markers(json!({ "action": "cut", "track": 0, "markers": ["um", "nope"] }));
    assert!(m.contains("'nope'"), "{m}");
    assert!(m.contains("find_markers"), "{m}");
}

#[test]
fn an_id_that_matches_no_marker_is_refused() {
    let mut s = session_with(json!([{ "name": "um", "start": 2.0, "end": 3.0 }]));
    let m = s.refused_at_markers(json!({
        "action": "cut", "track": 0,
        "markers": ["00000000-0000-4000-8000-000000000000"]
    }));
    assert!(m.contains("find_markers"), "{m}");
}

#[test]
fn an_ambiguous_name_is_refused_and_the_ids_are_listed() {
    let mut s = session_with(json!([
        { "name": "um", "start": 2.0, "end": 3.0 },
        { "name": "Um", "start": 5.0, "end": 6.0 },
    ]));
    let ids: Vec<String> = s
        .state()
        .annotations
        .iter()
        .map(|a| a.id.0.to_string())
        .collect();
    let m = s.refused_at_markers(json!({ "action": "cut", "track": 0, "markers": ["um"] }));
    assert!(m.contains("2 markers"), "{m}");
    assert!(m.contains("id"), "{m}");
    for id in &ids {
        assert!(m.contains(id.as_str()), "{m}");
    }
    // An id settles it.
    s.at_markers("cut", 0, &[&ids[1]]);
    assert_eq!(s.len(0), 10 * RATE as u64 - 48_000);
}

#[test]
fn naming_one_marker_twice_acts_on_it_once() {
    let mut s = session_with(json!([{ "name": "um", "start": 2.0, "end": 3.0 }]));
    let id = s.find("um").id.0.to_string();
    s.at_markers("cut", 0, &["um", &id, "UM"]);
    assert_eq!(
        s.len(0),
        10 * RATE as u64 - 48_000,
        "cut once, not three times"
    );
}

#[test]
fn overlapping_spans_are_refused_and_touching_ones_are_not() {
    let mut s = session_with(json!([
        { "name": "a", "start": 1.0, "end": 3.0 },
        { "name": "b", "start": 2.0, "end": 4.0 },
        { "name": "c", "start": 3.0, "end": 5.0 },
    ]));
    let m = s.refused_at_markers(json!({ "action": "cut", "track": 0, "markers": ["a", "b"] }));
    assert!(m.contains("overlap"), "{m}");
    assert!(m.contains("'a'") && m.contains("'b'"), "{m}");
    s.at_markers("cut", 0, &["a", "c"]);
    assert_eq!(s.len(0), 10 * RATE as u64 - 4 * 48_000);
}

#[test]
fn an_empty_list_a_missing_field_a_bad_action_and_a_bad_track_are_refused() {
    let mut s = session_with(json!([{ "name": "um", "start": 2.0, "end": 3.0 }]));
    let m = s.refused_at_markers(json!({ "action": "cut", "track": 0, "markers": [] }));
    assert!(m.contains("at least one"), "{m}");
    let m = s.refused_at_markers(json!({ "action": "cut", "track": 0 }));
    assert!(m.contains("markers"), "{m}");
    let m = s.refused_at_markers(json!({ "action": "explode", "track": 0, "markers": ["um"] }));
    assert!(m.contains("explode") || m.contains("action"), "{m}");
    let m = s.refused_at_markers(json!({ "action": "cut", "track": 5, "markers": ["um"] }));
    assert!(m.contains("track"), "{m}");
    let m =
        s.refused_at_markers(json!({ "action": "cut", "track": 0, "markers": ["um"], "extra": 1 }));
    assert!(m.contains("extra") || m.contains("unknown"), "{m}");
}

#[test]
fn more_markers_than_a_call_may_name_are_refused() {
    let mut s = session_with(json!([{ "name": "um", "start": 2.0, "end": 3.0 }]));
    let many: Vec<&str> = vec!["um"; 501];
    let m = s.refused_at_markers(json!({ "action": "cut", "track": 0, "markers": many }));
    assert!(m.contains("501"), "{m}");
}

#[test]
fn a_very_long_name_is_cut_in_the_refusal() {
    let mut s = session_with(json!([]));
    let long = "x".repeat(500);
    let m = s.refused_at_markers(json!({ "action": "cut", "track": 0, "markers": [long] }));
    assert!(m.len() < 400, "{} bytes", m.len());
    assert!(m.contains('…'), "{m}");
}

// ---------------------------------------------------------------------
// Plan first, and the registry
// ---------------------------------------------------------------------

#[test]
fn it_changes_the_session_so_plan_first_holds_it() {
    let d = ToolDispatcher::default_dispatcher();
    let tool = d.get("edit_at_markers").expect("registered");
    assert!(tool.mutates(), "it appends nodes: Plan first must hold it");
    let schema = tool.schema();
    assert_eq!(schema["name"], json!("edit_at_markers"));
    let actions = &schema["input_schema"]["properties"]["action"]["enum"];
    assert_eq!(
        actions,
        &json!(["cut", "delete", "silence", "split", "join"]),
        "{schema}"
    );
    assert_eq!(
        schema["input_schema"]["required"],
        json!(["action", "track", "markers"])
    );
}

#[test]
fn the_summary_names_what_was_done() {
    let mut s = session_with(json!([{ "name": "um", "start": 2.0, "end": 3.0 }]));
    let v = s.at_markers("cut", 0, &["um"]);
    let text = names(&v);
    assert!(text.contains("Cut"), "{text}");
    assert!(text.contains("track 0"), "{text}");
}

// ---------------------------------------------------------------------
// The refusals of the bodies `edit_at_markers` shares with the range tools
// ---------------------------------------------------------------------

/// A 16 kHz file's clip laid on track 0 after its 48 kHz one, gap and all, so
/// the track mixes rates and its clips are not back to back.
fn with_a_16k_clip_after_the_first(s: &mut Session) {
    let odd = write_frames(&s.dir.path().join("odd.wav"), 16_000, 1, 16_000, |n| {
        (n % 50) as f32 / 200.0
    });
    let mut state = s.state();
    let mut clip = harness::clip(&odd, 12 * RATE as u64, 0, 16_000);
    clip.content_hash = None;
    state.tracks[0].clips.push(clip);
    s.append_state("fixture: a clip at another rate", state);
}

#[test]
fn a_split_at_a_negative_marker_is_refused() {
    let mut s = session_with(json!([]));
    s.ok("label", json!({ "name": "early", "time": -2.0 }));
    let m = s.refused_at_markers(json!({ "action": "split", "track": 0, "markers": ["early"] }));
    assert!(m.contains("'early'"), "{m}");
    assert!(m.contains("0 or more"), "{m}");
}

#[test]
fn a_join_with_one_clip_under_the_marker_is_refused() {
    let mut s = session_with(json!([{ "name": "um", "start": 2.0, "end": 3.0 }]));
    s.at_marker_free_split(2.0);
    s.at_marker_free_split(3.0);
    // Clips: [0,2) [2,3) [3,10): only the middle one is wholly under `um`.
    let m = s.refused_at_markers(json!({ "action": "join", "track": 0, "markers": ["um"] }));
    assert!(m.contains("only one clip"), "{m}");
}

#[test]
fn arguments_the_schema_allows_but_the_tool_cannot_read_are_refused() {
    let mut s = session_with(json!([{ "name": "um", "start": 2.0, "end": 3.0 }]));
    // An integer, so the schema lets it by; a track cannot be negative.
    let m = s.refused_at_markers(json!({ "action": "cut", "track": -1, "markers": ["um"] }));
    assert!(m.contains("invalid arguments"), "{m}");
}

#[test]
fn a_project_with_no_session_has_nothing_to_edit() {
    let mut s = Session::empty();
    let args = json!({ "action": "cut", "track": 0, "markers": ["um"] });
    assert!(matches!(
        s.try_call("edit_at_markers", args),
        Ok(tools::ToolResult::Error(_)) | Err(_)
    ));
    let m = s.err(
        "silence_region",
        json!({ "track": 0, "start_sec": 0.0, "end_sec": 1.0 }),
    );
    assert!(!m.is_empty());
    assert_eq!(s.node_count(), 0, "a refusal appended a node");
}

#[test]
fn a_silence_over_a_source_that_cannot_be_read_is_refused() {
    let mut s = session_with(json!([{ "name": "um", "start": 2.0, "end": 3.0 }]));
    std::fs::write(s.dir.path().join("host.wav"), b"not audio").unwrap();
    let nodes = s.node_count();
    let m = s.refused_at_markers(json!({ "action": "silence", "track": 0, "markers": ["um"] }));
    assert!(!m.is_empty());
    assert_eq!(s.node_count(), nodes);
}

#[test]
fn a_join_of_clips_in_different_formats_is_refused_and_names_the_file() {
    let mut s = session_with(json!([]));
    with_a_16k_clip_after_the_first(&mut s);
    let nodes = s.node_count();
    let args = json!({ "track": 0, "first_clip": 0, "last_clip": 1 });
    let m = s.err("join_clips", args);
    assert!(m.contains("one sample rate"), "{m}");
    assert!(m.contains("odd.wav"), "{m}");
    assert_eq!(s.node_count(), nodes);
}

#[test]
fn a_join_that_cannot_write_its_render_is_refused() {
    let mut s = session_with(json!([]));
    // Two clips with a gap between them, so the join is rendered; `derived`
    // is a file, so the render has nowhere to go.
    s.at_marker_free_split(4.0);
    let mut state = s.state();
    state.tracks[0].clips[1].start_in_track += 48_000;
    s.append_state("fixture: a gap", state);
    let derived = s.dir.path().join(".audiograph").join("derived");
    let _ = std::fs::remove_dir_all(&derived);
    std::fs::write(&derived, b"a file, not a folder").unwrap();
    let nodes = s.node_count();
    let m = s.err("join_clips", json!({ "track": 0, "first_clip": 0 }));
    assert!(m.contains("failed to write"), "{m}");
    assert_eq!(s.node_count(), nodes);
}

#[test]
fn a_locked_cut_on_a_track_that_mixes_rates_is_refused() {
    let mut s = session_with(json!([]));
    with_a_16k_clip_after_the_first(&mut s);
    s.ok("set_sync_lock", json!({ "enabled": true }));
    let nodes = s.node_count();
    let args = json!({ "track": 0, "start_sample": 0, "end_sample": 48_000 });
    let m = s.err("cut_range", args);
    assert!(!m.is_empty());
    assert_eq!(s.node_count(), nodes);
}

#[test]
fn a_locked_cut_is_refused_when_another_track_mixes_rates() {
    // Track 0 mixes rates; the edited track 1 does not, so the refusal is
    // sync-lock's own: it cannot carry a time to a track of two rates.
    let mut s = session_with(json!([{ "name": "um", "start": 2.0, "end": 3.0 }]));
    with_a_16k_clip_after_the_first(&mut s);
    s.ok("set_sync_lock", json!({ "enabled": true }));
    let nodes = s.node_count();
    let args = json!({ "track": 1, "start_sample": 0, "end_sample": 48_000 });
    let m = s.err("cut_range", args);
    assert!(m.contains("rate"), "{m}");
    assert_eq!(s.node_count(), nodes, "a refusal appended a node");
    for action in ["cut", "delete"] {
        let m = s.refused_at_markers(json!({ "action": action, "track": 1, "markers": ["um"] }));
        assert!(m.contains("rate"), "{action}: {m}");
    }
}

#[test]
fn a_silence_on_a_track_that_does_not_exist_is_refused() {
    let mut s = session_with(json!([{ "name": "um", "start": 2.0, "end": 3.0 }]));
    let nodes = s.node_count();
    let m = s.err(
        "silence_region",
        json!({ "track": 99, "start_sec": 0.0, "end_sec": 1.0 }),
    );
    assert!(m.contains("99"), "{m}");
    assert_eq!(s.node_count(), nodes, "a refusal appended a node");
}
