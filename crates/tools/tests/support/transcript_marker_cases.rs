use serde_json::json;
use session::AnnotationKind;

use super::{file_project, track_project, word};

#[test]
fn segment_markers_append_one_node_and_preserve_existing_state() {
    let mut project = track_project(vec![
        word("Hello,", 0.125, 0.5),
        word("world.", 0.75, 1.25),
        word("Second", 3.0, 3.25),
        word("cue.", 3.5, 4.0),
    ]);
    project.ok("label", json!({ "name": "Existing", "time": 2.0 }));
    let before = project.state();
    let nodes_before = project.node_count();

    let result = project.ok("transcript_to_markers", json!({ "mode": "segment" }));

    let after = project.state();
    assert_eq!(project.node_count(), nodes_before + 1);
    assert_eq!(result["marker_count"], 2);
    assert_eq!(after.annotations.len(), 3);
    assert_eq!(after.annotations[0], before.annotations[0]);
    assert_eq!(after.annotations[1].name, "Hello, world.");
    assert_eq!(
        after.annotations[1].kind,
        AnnotationKind::Marker { time_sec: 0.125 }
    );
    assert_eq!(after.annotations[2].name, "Second cue.");
    assert_eq!(
        after.annotations[2].kind,
        AnnotationKind::Marker { time_sec: 3.0 }
    );
    let mut expected = before;
    expected.annotations = after.annotations.clone();
    assert_eq!(
        serde_json::to_value(after).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
}

#[test]
fn file_source_requires_an_explicit_offset_before_adding_markers() {
    let mut project = file_project(vec![word("File cue.", 0.125, 1.0)]);
    let before = project.state();
    let head_before = project.store.head();
    let nodes_before = project.node_count();

    let error = project.err("transcript_to_markers", json!({}));

    assert!(error.contains("offset_sec"), "{error}");
    assert!(error.contains("file"), "{error}");
    assert_eq!(project.state(), before);
    assert_eq!(project.store.head(), head_before);
    assert_eq!(project.node_count(), nodes_before);
}

#[test]
fn file_markers_apply_explicit_positive_offset_and_preserve_existing_state() {
    let mut project = file_project(vec![
        word("Hello,", 0.125, 0.5),
        word("world.", 0.75, 1.25),
        word("Second", 3.0, 3.25),
        word("cue.", 3.5, 4.0),
    ]);
    project.ok("label", json!({ "name": "Existing", "time": 2.0 }));
    let before = project.state();
    let nodes_before = project.node_count();

    let result = project.ok("transcript_to_markers", json!({ "offset_sec": 1.0 }));

    let after = project.state();
    assert_eq!(project.node_count(), nodes_before + 1);
    assert_eq!(result["marker_count"], 2);
    assert_eq!(result["applied_offset_sec"], 1.0);
    assert_eq!(after.annotations.len(), 3);
    assert_eq!(after.annotations[0], before.annotations[0]);
    assert_eq!(
        after.annotations[1].kind,
        AnnotationKind::Marker { time_sec: 1.125 }
    );
    assert_eq!(
        after.annotations[2].kind,
        AnnotationKind::Marker { time_sec: 4.0 }
    );
    let mut expected = before;
    expected.annotations = after.annotations.clone();
    assert_eq!(
        serde_json::to_value(after).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
}

#[test]
fn offset_that_would_place_a_marker_before_zero_refuses_atomically() {
    let mut project = file_project(vec![
        word("First cue.", 0.125, 1.0),
        word("Second cue.", 3.0, 4.0),
    ]);
    project.ok("label", json!({ "name": "Existing", "time": 2.0 }));
    let before = project.state();
    let head_before = project.store.head();
    let nodes_before = project.node_count();

    let error = project.err("transcript_to_markers", json!({ "offset_sec": -0.25 }));

    assert!(error.contains("offset"), "{error}");
    assert!(error.contains("nonnegative"), "{error}");
    assert_eq!(project.state(), before);
    assert_eq!(project.store.head(), head_before);
    assert_eq!(project.node_count(), nodes_before);
}

#[test]
fn sentence_markers_ignore_pauses_and_keep_a_trailing_fragment() {
    let mut project = track_project(vec![
        word("One", 0.125, 0.5),
        word("sentence.", 2.0, 2.5),
        word("A", 2.625, 2.75),
        word("question?", 3.0, 3.5),
        word("Trailing", 5.0, 5.5),
        word("fragment", 6.5, 7.0),
    ]);
    let nodes_before = project.node_count();

    let result = project.ok("transcript_to_markers", json!({ "mode": "sentence" }));

    let after = project.state();
    assert_eq!(project.node_count(), nodes_before + 1);
    assert_eq!(result["marker_count"], 3);
    assert_eq!(
        after
            .annotations
            .iter()
            .map(|marker| (marker.name.as_str(), marker.kind.clone()))
            .collect::<Vec<_>>(),
        vec![
            ("One sentence.", AnnotationKind::Marker { time_sec: 0.125 }),
            ("A question?", AnnotationKind::Marker { time_sec: 2.625 }),
            (
                "Trailing fragment",
                AnnotationKind::Marker { time_sec: 5.0 }
            ),
        ]
    );
}

#[test]
fn markers_refuse_a_removed_source_track_even_with_an_explicit_offset() {
    let mut project = track_project(vec![word("Original source.", 0.125, 1.0)]);
    project.ok("label", json!({ "name": "Existing", "time": 2.0 }));
    project.ok("remove_track", json!({ "track": 0 }));
    project.load("replacement.wav", &super::speech::tone(1.0, 330.0));
    let before = project.state();
    let head_before = project.store.head();
    let nodes_before = project.node_count();

    for args in [json!({}), json!({ "offset_sec": 0.0 })] {
        let error = project.err("transcript_to_markers", args);

        assert!(error.contains("source track"), "{error}");
        assert!(error.contains("removed"), "{error}");
        assert_eq!(project.state(), before);
        assert_eq!(project.store.head(), head_before);
        assert_eq!(project.node_count(), nodes_before);
    }
}

#[test]
fn markers_follow_stable_source_identity_without_retiming_recorded_words() {
    let mut project = track_project(vec![word("Recorded time.", 0.125, 1.0)]);
    let source_id = project.state().tracks[0].id.clone();
    project.load("other.wav", &super::speech::tone(1.0, 330.0));
    project.ok("time_shift", json!({ "track": 0, "offset_sec": 4.0 }));
    let mut node = project.store.get(project.store.head().unwrap()).unwrap();
    node.state.tracks.swap(0, 1);
    project.store.append(node).unwrap();
    let before = project.state();
    let nodes_before = project.node_count();

    let result = project.ok("transcript_to_markers", json!({}));

    let after = project.state();
    assert_eq!(result["time_basis"], "session");
    assert_eq!(result["applied_offset_sec"], 0.0);
    assert_eq!(
        result["source"],
        json!({
            "kind": "track",
            "time_basis": "session",
            "stable_track_id": source_id,
            "current_track_index": 1,
            "track_present": true,
        })
    );
    assert_eq!(
        after.annotations[0].kind,
        AnnotationKind::Marker { time_sec: 0.125 }
    );
    assert_eq!(project.node_count(), nodes_before + 1);
    let mut expected = before;
    expected.annotations = after.annotations.clone();
    assert_eq!(after, expected);
}

#[test]
fn unknown_source_requires_an_offset_and_accepts_explicit_zero() {
    let mut project = track_project(vec![word("Legacy source.", 0.125, 1.0)]);
    let mut node = project.store.get(project.store.head().unwrap()).unwrap();
    node.state.transcript.as_mut().unwrap().source = None;
    project.store.append(node).unwrap();
    let before = project.state();
    let head_before = project.store.head();
    let nodes_before = project.node_count();

    let error = project.err("transcript_to_markers", json!({}));

    assert!(error.contains("unknown"), "{error}");
    assert!(error.contains("offset_sec"), "{error}");
    assert_eq!(project.state(), before);
    assert_eq!(project.store.head(), head_before);
    assert_eq!(project.node_count(), nodes_before);

    let result = project.ok("transcript_to_markers", json!({ "offset_sec": 0.0 }));

    assert_eq!(result["time_basis"], "unknown");
    assert_eq!(result["source"]["kind"], "unknown");
    assert_eq!(result["applied_offset_sec"], 0.0);
    assert_eq!(project.node_count(), nodes_before + 1);
    assert_eq!(
        project.state().annotations[0].kind,
        AnnotationKind::Marker { time_sec: 0.125 }
    );
}

#[test]
fn signed_negative_offset_can_move_a_marker_to_exactly_zero() {
    let mut project = file_project(vec![word("At zero.", 1.125, 2.0), word("Later.", 3.0, 4.0)]);
    let nodes_before = project.node_count();

    let result = project.ok("transcript_to_markers", json!({ "offset_sec": -1.125 }));

    let after = project.state();
    assert_eq!(result["applied_offset_sec"], -1.125);
    assert_eq!(result["marker_count"], 2);
    assert_eq!(project.node_count(), nodes_before + 1);
    assert_eq!(
        after.annotations[0].kind,
        AnnotationKind::Marker { time_sec: 0.0 }
    );
    assert_eq!(
        after.annotations[1].kind,
        AnnotationKind::Marker { time_sec: 1.875 }
    );
}

#[test]
fn marker_labels_keep_six_words_and_at_most_eighty_unicode_characters() {
    let long_word = format!("{}.", "界".repeat(100));
    let mut project = track_project(vec![
        word("one two three four five six seven.", 0.125, 1.0),
        word(&long_word, 3.0, 4.0),
    ]);

    let result = project.ok("transcript_to_markers", json!({}));

    let after = project.state();
    assert_eq!(result["marker_count"], 2);
    assert_eq!(after.annotations[0].name, "one two three four five six");
    assert_eq!(after.annotations[1].name, "界".repeat(80));
}

#[test]
fn sentence_markers_ignore_segment_duration_limits() {
    let mut project = track_project(vec![
        word("One", 0.125, 0.5),
        word("long", 3.0, 3.5),
        word("sentence.", 9.0, 9.5),
    ]);

    let result = project.ok("transcript_to_markers", json!({ "mode": "sentence" }));

    let after = project.state();
    assert_eq!(result["marker_count"], 1);
    assert_eq!(after.annotations[0].name, "One long sentence.");
    assert_eq!(
        after.annotations[0].kind,
        AnnotationKind::Marker { time_sec: 0.125 }
    );
}

#[test]
fn marker_offsets_use_recorded_submillisecond_times_not_caption_rounding() {
    let recorded_start = 0.1254f32;
    let offset = -0.1253;
    let expected = f64::from(recorded_start) + offset;
    assert!(expected > 0.0);
    let mut project = file_project(vec![word("Precise.", recorded_start, 1.0)]);
    let nodes_before = project.node_count();

    let result = project.ok("transcript_to_markers", json!({"offset_sec": offset}));

    assert_eq!(result["marker_count"], 1);
    assert_eq!(project.node_count(), nodes_before + 1);
    let state = project.state();
    let AnnotationKind::Marker { time_sec } = state.annotations[0].kind else {
        panic!("expected a point marker");
    };
    // JSON persistence can round a double by one ULP, not by a millisecond.
    assert!((time_sec - expected).abs() <= f64::EPSILON * expected.abs());
}
