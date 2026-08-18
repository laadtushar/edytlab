//! Snapshot-locked JSON roundtrip for `SessionNode`.
//!
//! The fixture below is constructed deterministically (no `Utc::now`,
//! no random UUIDs). It serializes to `tests/snapshots/sample_node.json`,
//! which is checked into the repo. The test deserializes the snapshot,
//! re-serializes, and asserts byte-equal — any change to the on-disk
//! schema requires updating the snapshot file explicitly.

use std::path::PathBuf;

use chrono::TimeZone;
use session::{
    Bus, BusGraph, Clip, EffectInstance, KeyMap, KeySegment, NodeId, Send, SessionNode,
    SessionState, TempoMap, TempoSegment, Track, TrackId, Transcript, TranscriptWord,
};
use uuid::Uuid;

fn fixture() -> SessionNode {
    let track_uuid = Uuid::parse_str("11111111-2222-3333-4444-555555555555").unwrap();
    let bus_uuid = Uuid::parse_str("66666666-7777-8888-9999-aaaaaaaaaaaa").unwrap();

    let state = SessionState {
        tracks: vec![Track {
            id: TrackId(track_uuid),
            name: "Vocals".into(),
            clips: vec![Clip {
                source_path: PathBuf::from("media/vocal_take_1.wav"),
                start_in_track: 0,
                source_offset: 1_024,
                length: 480_000,
                content_hash: Some([0x42; 32]),
                time_stretch_factor: None,
                pitch_shift_semitones: None,
                beat_grid: None,
                volume_envelope: Vec::new(),
            }],
            gain_db: -3.0,
            pan: 0.25,
            muted: false,
            soloed: false,
            effects: vec![EffectInstance {
                kind: "gain".into(),
                params: serde_json::json!({ "db": -3.0 }),
                bypassed: false,
            }],
            // Deliberately empty: `sends` is `skip_serializing_if` empty,
            // so a session written before buses existed must serialise
            // byte-identically. The snapshot below is that proof, and it
            // only holds if this fixture stays send-free. Send
            // round-tripping is covered separately.
            sends: vec![],
        }],
        bus_routing: BusGraph {
            buses: vec![Bus {
                id: bus_uuid,
                name: "VocalBus".into(),
                effects: vec![],
            }],
        },
        master_chain: vec![],
        tempo_map: TempoMap {
            default_bpm: 120.0,
            segments: vec![TempoSegment {
                start_sample: 0,
                bpm: 120.0,
            }],
        },
        key_map: Some(KeyMap {
            segments: vec![KeySegment {
                start_sample: 0,
                key: "Cmaj".into(),
            }],
        }),
        transcript: Some(Transcript {
            words: vec![TranscriptWord {
                text: "hello".into(),
                start_s: 0.0,
                end_s: 0.5,
                confidence: 0.97,
            }],
        }),
        sample_rate: 48_000,
        length_samples: 480_000,
        annotations: Vec::new(),
        sync_lock: false,
    };

    let id = NodeId::from_state(&state).unwrap();
    SessionNode {
        id,
        parent: None,
        created_at: chrono::Utc.with_ymd_and_hms(2026, 5, 6, 12, 0, 0).unwrap(),
        label: Some("seed".into()),
        reasoning: Some("initial fixture for snapshot test".into()),
        state,
        op: None,
    }
}

const SNAPSHOT_PATH: &str = "tests/snapshots/sample_node.json";

#[test]
fn snapshot_roundtrips_byte_equal() {
    let node = fixture();
    let serialized = serde_json::to_string_pretty(&node).unwrap();

    let snapshot_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(SNAPSHOT_PATH);

    // Refresh the snapshot when explicitly requested. CI never sets
    // this, so drift fails the test.
    if std::env::var("UPDATE_SESSION_SNAPSHOT").is_ok() {
        std::fs::create_dir_all(snapshot_path.parent().unwrap()).unwrap();
        std::fs::write(&snapshot_path, &serialized).unwrap();
    }

    let on_disk = std::fs::read_to_string(&snapshot_path).unwrap_or_else(|_| {
        panic!(
            "snapshot missing at {}; run with UPDATE_SESSION_SNAPSHOT=1 to create",
            snapshot_path.display()
        )
    });

    assert_eq!(
        serialized, on_disk,
        "snapshot drift; run with UPDATE_SESSION_SNAPSHOT=1 to refresh"
    );

    let parsed: SessionNode = serde_json::from_str(&on_disk).unwrap();
    let reserialized = serde_json::to_string_pretty(&parsed).unwrap();
    assert_eq!(reserialized, on_disk, "roundtrip not byte-equal");
}

/// `sends` is a schema addition, so both directions matter: a session
/// with sends must survive a round trip, and one without must serialise
/// as though the field never existed (which the snapshot above pins).
#[test]
fn sends_round_trip_and_stay_absent_when_empty() {
    let bus_id = Uuid::parse_str("66666666-7777-8888-9999-aaaaaaaaaaaa").unwrap();
    let mut state = fixture().state;
    state.tracks[0].sends = vec![Send {
        bus_id,
        level_db: -6.0,
    }];

    let json = serde_json::to_string(&state).expect("serialise");
    assert!(json.contains("sends"), "a non-empty sends list must appear");

    let back: SessionState = serde_json::from_str(&json).expect("deserialise");
    assert_eq!(back.tracks[0].sends.len(), 1);
    assert_eq!(back.tracks[0].sends[0].bus_id, bus_id);
    assert_eq!(back.tracks[0].sends[0].level_db, -6.0);

    state.tracks[0].sends.clear();
    let empty = serde_json::to_string(&state).expect("serialise");
    assert!(
        !empty.contains("sends"),
        "an empty sends list must not appear at all, or every session \
         file written before buses existed changes on the next save"
    );
}

/// A session file predating the field must load.
#[test]
fn a_session_without_sends_still_loads() {
    let json = serde_json::to_string(&fixture().state).expect("serialise");
    assert!(!json.contains("sends"), "fixture should have no sends");
    let back: SessionState = serde_json::from_str(&json).expect("deserialise");
    assert!(back.tracks[0].sends.is_empty());
}
