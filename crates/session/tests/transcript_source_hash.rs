//! A transcript that does not say where it came from hashes as it always did
//! (#384).
//!
//! `Transcript::source` records which track (or file) the words were made
//! from. Node ids are the hash of the state's JSON, so a new field that
//! serialised when empty would give every existing project's nodes new ids
//! the next time anything re-hashed them. The field is defaulted when absent
//! and skipped when `None`. The ids and the JSON below were produced by the
//! code before the field existed.

use session::{
    BusGraph, NodeId, SessionState, TempoMap, TrackId, Transcript, TranscriptSource, TranscriptWord,
};

fn state(transcript: Option<Transcript>) -> SessionState {
    SessionState {
        tracks: Vec::new(),
        bus_routing: BusGraph::default(),
        master_chain: Vec::new(),
        tempo_map: TempoMap::default(),
        key_map: None,
        transcript,
        sample_rate: 48_000,
        length_samples: 0,
        annotations: Vec::new(),
        sync_lock: false,
    }
}

fn hi() -> Vec<TranscriptWord> {
    vec![TranscriptWord {
        text: "hi".into(),
        start_s: 0.0,
        end_s: 0.25,
        confidence: 1.0,
    }]
}

/// The id of a state with no transcript, as the code before the field gave it.
const NO_TRANSCRIPT_ID: &str = "2f571864eb05fdff81bee7e30ccab9e8c3b340099d6491af8825e182816b08f9";
/// The id of a state with the transcript `hi()`, and no source.
const UNSOURCED_ID: &str = "3673b4cf5980a335f1be4a303329602843fd8cd82e4aede04dcd4d718f09d49e";

#[test]
fn a_state_with_no_transcript_hashes_as_before() {
    let id = NodeId::from_state(&state(None)).unwrap();
    assert_eq!(id.to_hex(), NO_TRANSCRIPT_ID);
}

#[test]
fn a_transcript_with_no_source_hashes_and_serialises_as_before() {
    let s = state(Some(Transcript::new(hi())));
    assert_eq!(NodeId::from_state(&s).unwrap().to_hex(), UNSOURCED_ID);
    let json = serde_json::to_value(&s).unwrap();
    assert_eq!(
        json["transcript"],
        serde_json::json!({
            "words": [{ "text": "hi", "start_s": 0.0, "end_s": 0.25, "confidence": 1.0 }]
        }),
        "an empty source must not appear in the JSON"
    );
}

#[test]
fn json_written_before_the_field_reads_with_no_source() {
    let old = serde_json::json!({
        "words": [{ "text": "hi", "start_s": 0.0, "end_s": 0.25, "confidence": 1.0 }]
    });
    let t: Transcript = serde_json::from_value(old).unwrap();
    assert_eq!(t.source, None);
    assert_eq!(t.words.len(), 1);
}

#[test]
fn a_source_is_part_of_the_state_and_round_trips() {
    let track = TrackId::new();
    let from_track = Transcript {
        words: hi(),
        source: Some(TranscriptSource::Track {
            track: track.clone(),
        }),
    };
    let from_file = Transcript {
        words: hi(),
        source: Some(TranscriptSource::File),
    };
    let a = NodeId::from_state(&state(Some(from_track.clone()))).unwrap();
    let b = NodeId::from_state(&state(Some(from_file.clone()))).unwrap();
    assert_ne!(a.to_hex(), UNSOURCED_ID);
    assert_ne!(
        a, b,
        "the source is content: a different one is a different node"
    );

    for t in [from_track, from_file] {
        let json = serde_json::to_string(&t).unwrap();
        let back: Transcript = serde_json::from_str(&json).unwrap();
        assert_eq!(back, t);
    }
    // The shape the window and the docs describe.
    let json = serde_json::to_value(Transcript {
        words: Vec::new(),
        source: Some(TranscriptSource::File),
    })
    .unwrap();
    assert_eq!(json["source"], serde_json::json!({ "kind": "file" }));
}
