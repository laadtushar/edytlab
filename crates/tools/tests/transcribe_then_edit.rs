//! A transcript made by `transcribe` is edited by words (#384).
//!
//! `cut_words` and `remove_fillers` cut the audio at a transcript's word
//! times. Those are seconds of the audio the words were made from, so a
//! transcript records where it came from, and an edit by words cuts that
//! track and no other: the wrong track's audio going, with a success to say so,
//! was the destructive mistake waiting for the day transcripts became real.
//!
//! The words here come from a recogniser that is not a model, so this runs in
//! CI. `transcribe_end_to_end.rs` does the same with the real model.

#[path = "support/speech.rs"]
mod speech;

use std::sync::Arc;

use serde_json::json;
use session::TranscriptSource;
use speech::{tone, word, Fake, Project};
use tools::tool::transcribe::SpeechToText;

fn talk() -> Vec<session::TranscriptWord> {
    vec![
        word("So,", 0.2, 0.5),
        word("um", 0.6, 0.9),
        word("this", 1.0, 1.3),
        word("is", 1.35, 1.5),
        word("uh", 1.6, 1.9),
        word("good.", 2.0, 2.6),
    ]
}

/// Two tracks of four seconds each, with the talk transcribed from track 1.
fn session_with_talk_on_track_1() -> (Arc<Fake>, Project) {
    let fake = Arc::new(Fake::new(talk()));
    let mut p = Project::new(Arc::clone(&fake) as Arc<dyn SpeechToText>);
    p.load("music.wav", &tone(4.0, 220.0));
    p.load("voice.wav", &tone(4.0, 330.0));
    (fake, p)
}

fn length_of(p: &Project, track: usize) -> u64 {
    p.state().tracks[track]
        .clips
        .iter()
        .map(|c| c.length)
        .sum::<u64>()
}

#[test]
fn the_words_of_track_1_cut_track_1_without_being_told() {
    let (_fake, mut p) = session_with_talk_on_track_1();
    p.ok("transcribe", json!({ "track": 1 }));
    let (music, voice) = (length_of(&p, 0), length_of(&p, 1));

    // No `track`: the transcript's.
    let v = p.ok("cut_words", json!({ "from_word": 1, "to_word": 2 }));

    assert_eq!(v["removed_text"], "um");
    assert_eq!(length_of(&p, 0), music, "the other track is untouched");
    assert!(length_of(&p, 1) < voice, "the transcribed track is cut");
    // The words that remain are shifted and still say where they came from.
    let t = p.state().transcript.unwrap();
    assert_eq!(t.words.len(), 5);
    assert!(matches!(t.source, Some(TranscriptSource::Track { .. })));
}

#[test]
fn cutting_another_track_by_those_words_is_refused() {
    let (_fake, mut p) = session_with_talk_on_track_1();
    p.ok("transcribe", json!({ "track": 1 }));
    let (music, voice) = (length_of(&p, 0), length_of(&p, 1));
    let nodes = p.node_count();

    for (tool, args) in [
        (
            "cut_words",
            json!({ "from_word": 1, "to_word": 2, "track": 0 }),
        ),
        ("remove_fillers", json!({ "apply": true, "track": 0 })),
    ] {
        let msg = p.err(tool, args);
        assert!(msg.contains("is of track 1"), "{tool}: {msg}");
        assert!(msg.contains("Pass track 1"), "{tool}: {msg}");
    }
    assert_eq!((length_of(&p, 0), length_of(&p, 1)), (music, voice));
    assert_eq!(p.node_count(), nodes, "nothing was appended");
}

#[test]
fn the_fillers_of_a_transcript_are_removed_from_its_own_track() {
    let (_fake, mut p) = session_with_talk_on_track_1();
    p.ok("transcribe", json!({ "track": 1 }));
    let (music, voice) = (length_of(&p, 0), length_of(&p, 1));

    let report = p.ok("remove_fillers", json!({}));
    assert_eq!(report["found"], 2, "um and uh: {report}");
    assert_eq!(report["applied"], false);
    assert_eq!(length_of(&p, 1), voice, "a report changes nothing");

    let done = p.ok("remove_fillers", json!({ "apply": true }));
    assert_eq!(done["applied"], true);
    assert_eq!(length_of(&p, 0), music);
    assert!(length_of(&p, 1) < voice);
    let words: Vec<String> = p
        .state()
        .transcript
        .unwrap()
        .words
        .iter()
        .map(|w| w.text.clone())
        .collect();
    assert_eq!(words, ["So,", "this", "is", "good."]);
}

#[test]
fn a_track_that_moved_is_still_the_track_the_words_are_of() {
    let (_fake, mut p) = session_with_talk_on_track_1();
    p.ok("transcribe", json!({ "track": 1 }));
    // The music track goes; the voice is track 0 now.
    p.ok("remove_track", json!({ "track": 0 }));
    let voice = length_of(&p, 0);

    p.ok("cut_words", json!({ "from_word": 1, "to_word": 2 }));

    assert!(length_of(&p, 0) < voice);
    let msg = p.err(
        "cut_words",
        json!({ "from_word": 0, "to_word": 1, "track": 1 }),
    );
    assert!(
        msg.contains("track 1") || msg.contains("out of range"),
        "{msg}"
    );
}

#[test]
fn words_of_a_track_that_has_gone_cut_nothing() {
    let (_fake, mut p) = session_with_talk_on_track_1();
    p.ok("transcribe", json!({ "track": 1 }));
    p.ok("remove_track", json!({ "track": 1 }));
    let nodes = p.node_count();

    for (tool, args) in [
        ("cut_words", json!({ "from_word": 1, "to_word": 2 })),
        (
            "cut_words",
            json!({ "from_word": 1, "to_word": 2, "track": 0 }),
        ),
        ("remove_fillers", json!({ "apply": true })),
    ] {
        let msg = p.err(tool, args.clone());
        assert!(
            msg.contains("no longer in the session"),
            "{tool} {args}: {msg}"
        );
    }
    assert_eq!(p.node_count(), nodes);
}

#[test]
fn the_words_of_a_file_cut_only_the_track_the_caller_names() {
    let (_fake, mut p) = session_with_talk_on_track_1();
    let file = p.dir.path().join("voice.wav");
    p.ok("transcribe", json!({ "path": file.to_string_lossy() }));
    let (music, voice) = (length_of(&p, 0), length_of(&p, 1));

    let msg = p.err("cut_words", json!({ "from_word": 1, "to_word": 2 }));
    assert!(msg.contains("made from a file"), "{msg}");
    assert!(msg.contains("Pass `track`"), "{msg}");
    let msg = p.err("remove_fillers", json!({ "apply": true }));
    assert!(msg.contains("made from a file"), "{msg}");
    assert_eq!((length_of(&p, 0), length_of(&p, 1)), (music, voice));

    p.ok(
        "cut_words",
        json!({ "from_word": 1, "to_word": 2, "track": 1 }),
    );
    assert!(length_of(&p, 1) < voice);
}

/// A transcript written before the field existed says nothing about where it
/// came from, and is cut as it always was: the track named, else track 0.
#[test]
fn a_transcript_with_no_source_is_cut_as_it_always_was() {
    let (_fake, mut p) = session_with_talk_on_track_1();
    p.ok("transcribe", json!({ "track": 1 }));
    // The same words, with the source taken off, as an old project has them.
    let mut state = p.state();
    state.transcript.as_mut().unwrap().source = None;
    let node = session::SessionNode {
        id: session::NodeId([0u8; 32]),
        parent: None,
        created_at: chrono::Utc::now(),
        label: Some("old transcript".into()),
        reasoning: None,
        state,
        op: None,
    };
    p.store.append(node).expect("append");
    let (music, voice) = (length_of(&p, 0), length_of(&p, 1));

    p.ok("cut_words", json!({ "from_word": 1, "to_word": 2 }));
    assert!(length_of(&p, 0) < music, "track 0, as before");
    assert_eq!(length_of(&p, 1), voice);
    p.ok(
        "cut_words",
        json!({ "from_word": 0, "to_word": 1, "track": 1 }),
    );
    assert!(length_of(&p, 1) < voice);
}
