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
    Bus, BusGraph, Clip, EffectInstance, KeyMap, KeySegment, NodeId, SessionNode, SessionState,
    TempoMap, TempoSegment, Track, TrackId, Transcript, TranscriptWord,
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
    };

    let id = NodeId::from_state(&state).unwrap();
    SessionNode {
        id,
        parent: None,
        created_at: chrono::Utc.with_ymd_and_hms(2026, 5, 6, 12, 0, 0).unwrap(),
        label: Some("seed".into()),
        reasoning: Some("initial fixture for snapshot test".into()),
        state,
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
