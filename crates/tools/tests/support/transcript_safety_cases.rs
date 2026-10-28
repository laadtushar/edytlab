use std::sync::Arc;

use serde_json::json;

use super::{speech, track_project, word, Fake, Project};

#[test]
fn missing_transcript_errors_name_transcribe_without_creating_files_or_history() {
    for with_audio in [false, true] {
        let mut project = Project::new(Arc::new(Fake::new(vec![])));
        if with_audio {
            project.load("untranscribed.wav", &speech::tone(0.1, 220.0));
        }
        let before = project.store.head();
        let count = project.node_count();
        for (tool, args) in [
            ("export_transcript", json!({"out_path": "missing.srt"})),
            ("transcript_to_markers", json!({})),
        ] {
            let error = project.err(tool, args);
            assert!(error.contains("transcribe"), "{tool}: {error}");
            assert_eq!(project.store.head(), before);
            assert_eq!(project.node_count(), count);
            assert!(!project.dir.path().join("missing.srt").exists());
        }
    }
}

#[test]
fn export_refuses_audio_from_project_history_without_replacing_its_bytes() {
    let mut project = track_project(vec![word("Protected.", 0.125, 1.0)]);
    let source = project.dir.path().join("speech.wav");
    let original = std::fs::read(&source).unwrap();
    // The earlier node still needs the recording even when the current state
    // no longer has a clip naming it.
    let mut node = project.store.get(project.store.head().unwrap()).unwrap();
    node.state.tracks.clear();
    project.store.append(node).unwrap();
    let head = project.store.head();
    let count = project.node_count();

    let error = project.err("export_transcript", json!({"out_path": source}));

    assert!(error.contains("not written"), "{error}");
    assert_eq!(std::fs::read(&source).unwrap(), original);
    assert_eq!(project.store.head(), head);
    assert_eq!(project.node_count(), count);
}

#[test]
fn malformed_stored_word_times_refuse_before_any_publication() {
    let mut project = track_project(vec![word("Valid.", 0.125, 1.0)]);
    let destination = project.dir.path().join("existing.srt");
    std::fs::write(&destination, b"previous successful export\n").unwrap();
    for words in [
        vec![word("Negative.", -0.25, 1.0)],
        vec![word("Backwards.", 1.0, 0.5)],
        vec![word("First", 1.0, 1.5), word("Second.", 0.5, 2.0)],
        vec![word("   \r\n\t", 0.0, 1.0)],
        vec![],
    ] {
        let mut node = project.store.get(project.store.head().unwrap()).unwrap();
        node.state.transcript.as_mut().unwrap().words = words;
        project.store.append(node).unwrap();
        let before = project.state();
        let head = project.store.head();
        let count = project.node_count();
        for (tool, args) in [
            ("export_transcript", json!({"out_path": destination})),
            ("transcript_to_markers", json!({})),
        ] {
            let error = project.err(tool, args);
            assert!(error.contains("transcript"), "{tool}: {error}");
            assert_eq!(project.state(), before);
            assert_eq!(project.store.head(), head);
            assert_eq!(project.node_count(), count);
            assert_eq!(
                std::fs::read(&destination).unwrap(),
                b"previous successful export\n"
            );
        }
    }
}

#[test]
fn transcript_tools_preserve_legacy_envelope_units_without_loading_audio() {
    let mut project = track_project(vec![word("Legacy.", 0.125, 1.0)]);
    let mut node = project.store.get(project.store.head().unwrap()).unwrap();
    node.state.sample_rate = 48_000;
    let clip = &mut node.state.tracks[0].clips[0];
    clip.volume_envelope_frames = session::EnvelopeFrames::Project;
    clip.volume_envelope = vec![
        session::EnvelopePoint {
            time_samples: 0,
            gain_db: -12.0,
        },
        session::EnvelopePoint {
            time_samples: 48_000,
            gain_db: 0.0,
        },
    ];
    project.store.append(node).unwrap();
    let before = project.state();
    let head = project.store.head();
    let count = project.node_count();

    project.ok("export_transcript", json!({"out_path": "legacy.srt"}));
    assert_eq!(project.state(), before);
    assert_eq!(project.store.head(), head);
    assert_eq!(project.node_count(), count);

    project.ok("transcript_to_markers", json!({}));
    let mut after = project.state();
    assert_eq!(after.annotations.len(), before.annotations.len() + 1);
    after.annotations = before.annotations.clone();
    assert_eq!(after, before);
    assert_eq!(project.node_count(), count + 1);
}

#[test]
fn stored_transcript_resource_limits_refuse_without_writing_a_file_or_node() {
    let mut project = track_project(vec![word("Bounded.", 0.125, 1.0)]);
    for words in [
        vec![word("a", 0.0, 0.1); 200_001],
        vec![word(&"a".repeat(4 * 1024 * 1024 + 1), 0.0, 0.1)],
        (0..10_001)
            .map(|index| word("Cue.", index as f32, index as f32 + 0.5))
            .collect(),
    ] {
        let mut node = project.store.get(project.store.head().unwrap()).unwrap();
        node.state.transcript.as_mut().unwrap().words = words;
        project.store.append(node).unwrap();
        let head = project.store.head();
        let count = project.node_count();
        for (tool, args) in [
            ("export_transcript", json!({"out_path": "too-large.srt"})),
            ("transcript_to_markers", json!({})),
        ] {
            let error = project.err(tool, args);
            assert!(error.contains("limit"), "{tool}: {error}");
            assert_eq!(project.store.head(), head);
            assert_eq!(project.node_count(), count);
            assert!(!project.dir.path().join("too-large.srt").exists());
        }
    }
}

#[test]
fn vtt_escaping_cannot_expand_a_valid_transcript_past_the_output_limit() {
    let mut project = track_project(vec![word("Bounded.", 0.125, 1.0)]);
    let mut node = project.store.get(project.store.head().unwrap()).unwrap();
    node.state.transcript.as_mut().unwrap().words = vec![word(&"&".repeat(3_500_000), 0.125, 1.0)];
    project.store.append(node).unwrap();
    let head = project.store.head();
    let count = project.node_count();

    let error = project.err(
        "export_transcript",
        json!({"out_path": "too-large.vtt", "format": "vtt"}),
    );

    assert!(error.contains("output limit"), "{error}");
    assert_eq!(project.store.head(), head);
    assert_eq!(project.node_count(), count);
    assert!(!project.dir.path().join("too-large.vtt").exists());
}

#[test]
fn invalid_tool_arguments_are_rejected_before_any_effect() {
    let mut project = track_project(vec![word("Validated.", 0.125, 1.0)]);
    let head = project.store.head();
    let count = project.node_count();
    for (tool, args) in [
        ("export_transcript", json!({})),
        (
            "export_transcript",
            json!({"out_path": "invalid.srt", "format": "pdf"}),
        ),
        (
            "export_transcript",
            json!({"out_path": "invalid.srt", "pause_sec": -1}),
        ),
        (
            "export_transcript",
            json!({"out_path": "invalid.srt", "max_cue_sec": 0}),
        ),
        (
            "export_transcript",
            json!({"out_path": "invalid.srt", "max_chars_per_line": 0}),
        ),
        (
            "export_transcript",
            json!({"out_path": "invalid.srt", "max_chars_per_line": 201}),
        ),
        (
            "export_transcript",
            json!({"out_path": "invalid.srt", "max_chars_per_line": 2.5}),
        ),
        (
            "export_transcript",
            json!({"out_path": "invalid.srt", "unexpected": true}),
        ),
        ("transcript_to_markers", json!({"mode": "all"})),
        ("transcript_to_markers", json!({"offset_sec": "zero"})),
        ("transcript_to_markers", json!({"unexpected": true})),
    ] {
        let mut context = tools::ToolContext {
            store: &mut project.store,
            engine: &mut project.engine,
            user_message: "",
            clipboard: &mut project.clipboard,
            allowed_tools: None,
        };
        assert!(
            project
                .dispatcher
                .invoke(tool, args.clone(), &mut context)
                .is_err(),
            "{tool}: {args}"
        );
        assert_eq!(project.store.head(), head);
        assert_eq!(project.node_count(), count);
        assert!(!project.dir.path().join("invalid.srt").exists());
    }
}

#[test]
fn failed_export_publication_leaves_no_temporary_files_or_history_edits() {
    let mut project = track_project(vec![word("Atomic.", 0.125, 1.0)]);
    let directory = project.dir.path().join("a-directory.srt");
    std::fs::create_dir(&directory).unwrap();
    let head = project.store.head();
    let count = project.node_count();

    let error = project.err("export_transcript", json!({"out_path": directory}));

    assert!(error.contains("failed to publish"), "{error}");
    assert!(directory.is_dir());
    assert_eq!(project.store.head(), head);
    assert_eq!(project.node_count(), count);
    for entry in std::fs::read_dir(project.dir.path()).unwrap() {
        assert!(!entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".transcript-"));
    }
}
