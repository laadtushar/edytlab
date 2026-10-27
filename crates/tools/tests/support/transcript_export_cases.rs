use serde_json::json;

use super::{file_project, track_project, word};

#[test]
fn exports_srt_golden_without_changing_project_history() {
    let mut project = track_project(vec![
        word("Hello,", 0.125, 0.5),
        word("world.", 0.75, 1.25),
        word("Second", 3.0, 3.25),
        word("cue.", 3.5, 4.0),
    ]);
    let before = serde_json::to_value(project.state()).unwrap();
    let head_before = project.store.head();
    let nodes_before = project.node_count();

    project.ok(
        "export_transcript",
        json!({ "out_path": "captions.srt", "format": "srt" }),
    );

    assert_eq!(
        std::fs::read_to_string(project.dir.path().join("captions.srt")).unwrap(),
        include_str!("../fixtures/transcript_export/track.srt")
    );
    assert_eq!(serde_json::to_value(project.state()).unwrap(), before);
    assert_eq!(project.store.head(), head_before);
    assert_eq!(project.node_count(), nodes_before);
}

#[test]
fn exports_vtt_file_times_with_escaped_text_and_source_metadata() {
    let mut project = file_project(vec![
        word("<Hello,>", 0.125, 0.5),
        word("&", 0.75, 1.0),
        word("world.", 1.125, 1.25),
        word("Second", 3.0, 3.25),
        word("cue.", 3.5, 4.0),
    ]);

    let result = project.ok(
        "export_transcript",
        json!({ "out_path": "captions.vtt", "format": "vtt" }),
    );

    assert_eq!(
        std::fs::read_to_string(project.dir.path().join("captions.vtt")).unwrap(),
        include_str!("../fixtures/transcript_export/file.vtt")
    );
    assert_eq!(result["format"], "vtt");
    assert_eq!(result["word_count"], 5);
    assert_eq!(result["cue_count"], 2);
    assert_eq!(result["time_basis"], "file");
    assert_eq!(
        result["source"],
        json!({
            "kind": "file",
            "time_basis": "file",
            "stable_track_id": null,
            "current_track_index": null,
            "track_present": null,
        })
    );
    assert!(!result.to_string().contains("speech.wav"));
    assert!(!result.to_string().contains("Hello"));
}

#[test]
fn exports_plain_text_segments_without_timestamps_or_entities() {
    let mut project = track_project(vec![
        word("Literal", 0.1, 0.3),
        word("<tag>", 0.35, 0.5),
        word("&amp;", 0.55, 0.7),
        word("stays.", 0.75, 1.0),
        word("Next", 2.0, 2.2),
        word("line.", 2.3, 2.5),
    ]);

    let result = project.ok(
        "export_transcript",
        json!({ "out_path": "transcript.txt", "format": "txt" }),
    );

    let expected = include_str!("../fixtures/transcript_export/plain.txt");
    assert_eq!(
        std::fs::read_to_string(project.dir.path().join("transcript.txt")).unwrap(),
        expected
    );
    assert_eq!(result["format"], "txt");
    assert_eq!(result["cue_count"], 2);
    assert_eq!(result["byte_count"], expected.len());
    assert_eq!(result["time_basis"], "session");
}

#[test]
fn exports_segments_using_the_configured_pause_threshold() {
    let mut project = file_project(vec![word("Before", 0.0, 0.25), word("after", 0.75, 1.0)]);

    let result = project.ok(
        "export_transcript",
        json!({ "out_path": "pauses.txt", "format": "txt", "pause_sec": 0.5 }),
    );

    assert_eq!(
        std::fs::read_to_string(project.dir.path().join("pauses.txt")).unwrap(),
        "Before\nafter\n"
    );
    assert_eq!(result["cue_count"], 2);
}

#[test]
fn exports_segments_without_exceeding_configured_cue_duration() {
    let mut project = file_project(vec![
        word("One", 0.0, 1.5),
        word("two", 1.5, 3.0),
        word("three", 3.0, 4.5),
        word("four", 4.5, 6.0),
    ]);

    let result = project.ok(
        "export_transcript",
        json!({ "out_path": "duration.srt", "max_cue_sec": 3.0 }),
    );

    assert_eq!(
        std::fs::read_to_string(project.dir.path().join("duration.srt")).unwrap(),
        "1\n00:00:00,000 --> 00:00:03,000\nOne two\n\n2\n00:00:03,000 --> 00:00:06,000\nthree four\n\n"
    );
    assert_eq!(result["cue_count"], 2);
}

#[test]
fn exports_wrapped_cues_with_at_most_two_configured_width_lines() {
    let mut project = file_project(vec![
        word("One", 0.0, 0.25),
        word("two", 0.25, 0.5),
        word("three", 0.5, 0.75),
        word("four", 0.75, 1.0),
        word("five", 1.0, 1.25),
        word("six", 1.25, 1.5),
    ]);

    let result = project.ok(
        "export_transcript",
        json!({ "out_path": "wrapped.srt", "max_chars_per_line": 5 }),
    );

    assert_eq!(
        std::fs::read_to_string(project.dir.path().join("wrapped.srt")).unwrap(),
        "1\n00:00:00,000 --> 00:00:00,500\nOne\ntwo\n\n2\n00:00:00,500 --> 00:00:01,000\nthree\nfour\n\n3\n00:00:01,000 --> 00:00:01,500\nfive\nsix\n\n"
    );
    assert_eq!(result["cue_count"], 3);
}

#[test]
fn indivisible_unicode_word_wraps_without_losing_text_or_inventing_times() {
    let mut project = file_project(vec![word("αβγδεζη", 0.125, 1.25), word("完了", 1.25, 1.5)]);

    for format in ["srt", "txt"] {
        let result = project.ok(
            "export_transcript",
            json!({
                "out_path": format!("unicode.{format}"),
                "format": format,
                "max_chars_per_line": 2
            }),
        );
        let expected = if format == "srt" {
            include_str!("../fixtures/transcript_export/unicode.srt")
        } else {
            "αβγδεζη\n完了\n"
        };
        assert_eq!(
            std::fs::read_to_string(project.dir.path().join(format!("unicode.{format}"))).unwrap(),
            expected
        );
        assert_eq!(result["cue_count"], 2);
    }
}

#[test]
fn rounds_milliseconds_once_with_carries_positive_duration_and_large_hours() {
    let mut project = file_project(vec![word("Initial.", 0.0, 1.0)]);
    let mut node = project.store.get(project.store.head().unwrap()).unwrap();
    node.state.transcript.as_mut().unwrap().words = vec![
        word("Tiny.", 0.0001, 0.0004),
        word("Minute.", 59.9996, 60.125),
        word("Hour.", 3599.9996, 3600.25),
        word("Large.", 360000.0, 360001.0),
    ];
    project.store.append(node).unwrap();

    project.ok("export_transcript", json!({ "out_path": "boundaries.srt" }));

    assert_eq!(
        std::fs::read_to_string(project.dir.path().join("boundaries.srt")).unwrap(),
        include_str!("../fixtures/transcript_export/boundaries.srt")
    );
}

#[test]
fn overlapping_words_keep_the_maximum_end_even_when_later_ends_regress() {
    let mut project = file_project(vec![
        word("Long", 0.0, 4.0),
        word("inside.", 1.0, 2.0),
        word("Overlap.", 1.5, 2.5),
    ]);

    let result = project.ok("export_transcript", json!({ "out_path": "overlap.srt" }));

    assert_eq!(
        std::fs::read_to_string(project.dir.path().join("overlap.srt")).unwrap(),
        "1\n00:00:00,000 --> 00:00:04,000\nLong inside.\n\n2\n00:00:01,500 --> 00:00:02,500\nOverlap.\n\n"
    );
    assert_eq!(result["cue_count"], 2);
}

#[test]
fn vtt_escapes_entities_and_arrows_once_and_normalizes_source_newlines() {
    let mut project = file_project(vec![word(
        "<b>&amp; -->\r\n\r\n00:00:09.000 --> 00:00:10.000\r\ninjected",
        0.125,
        1.25,
    )]);

    let result = project.ok(
        "export_transcript",
        json!({ "out_path": "literal.vtt", "format": "vtt", "max_chars_per_line": 200 }),
    );

    assert_eq!(
        std::fs::read_to_string(project.dir.path().join("literal.vtt")).unwrap(),
        include_str!("../fixtures/transcript_export/literal.vtt")
    );
    assert_eq!(result["cue_count"], 1);
}

#[test]
fn srt_keeps_literal_markup_and_normalizes_source_newlines() {
    let mut project = file_project(vec![word("<b>&amp; -->\r\n\r\nliteral", 0.125, 1.25)]);

    project.ok("export_transcript", json!({ "out_path": "literal.srt" }));

    assert_eq!(
        std::fs::read_to_string(project.dir.path().join("literal.srt")).unwrap(),
        "1\n00:00:00,125 --> 00:00:01,250\n<b>&amp; --> literal\n\n"
    );
}

#[test]
fn export_keeps_recorded_times_and_stable_source_after_reorder_and_clip_move() {
    let mut project = track_project(vec![word("Recorded.", 0.125, 1.25)]);
    let source_id = project.state().tracks[0].id.clone();
    project.load("other.wav", &super::speech::tone(0.25, 330.0));
    // Reordering changes positions, while the transcript's track ID remains
    // the identity captured when transcription ran.
    let mut node = project.store.get(project.store.head().unwrap()).unwrap();
    node.state.tracks.swap(0, 1);
    project.store.append(node).unwrap();
    project.ok(
        "move_clip",
        json!({ "track": 1, "clip_index": 0, "start_sec": 8.0 }),
    );
    let before = project.state();
    let head = project.store.head();
    let count = project.node_count();

    let result = project.ok("export_transcript", json!({ "out_path": "recorded.srt" }));

    assert_eq!(
        std::fs::read_to_string(project.dir.path().join("recorded.srt")).unwrap(),
        "1\n00:00:00,125 --> 00:00:01,250\nRecorded.\n\n"
    );
    assert_eq!(result["time_basis"], "session");
    assert_eq!(result["source"]["stable_track_id"], json!(source_id));
    assert_eq!(result["source"]["current_track_index"], 1);
    assert_eq!(result["source"]["track_present"], true);
    assert_eq!(project.state(), before);
    assert_eq!(project.store.head(), head);
    assert_eq!(project.node_count(), count);
}
