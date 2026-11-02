//! A clip's `content_hash` reads as `None` when the field is missing, and
//! a clip writes and hashes exactly as it did before that was allowed.
//!
//! `content_hash` is read through a custom deserializer
//! (`node::hex_array_32_opt`). serde only treats a missing `Option` as
//! `None` on its own when the field uses the stock deserializer; with
//! `deserialize_with` a missing field is an error unless the field also
//! says `default` (SonarCloud rust:S9334). The field always serialises
//! (`null` or the hex), so every file the store has written carries it;
//! `default` changes what a clip without it reads as and nothing that is
//! written. Node ids are the hash of the state's JSON, so the JSON and
//! the ids below were produced by the code before the attribute changed,
//! and must not move.

use std::path::Path;

use session::{Clip, NodeId, SessionState, TrackId};
use uuid::Uuid;

mod support;
use support::{clip_of, empty_state, track_of};

fn state_with(clip: Clip) -> SessionState {
    let mut state = empty_state();
    state.tracks.push(track_of(
        TrackId(Uuid::from_u128(0x0123_4567_89ab_cdef_0123_4567_89ab_cdef)),
        "Voice",
        vec![clip],
    ));
    state
}

fn unhashed() -> Clip {
    clip_of(Path::new("audio/take1.wav"), 48_000)
}

fn hashed() -> Clip {
    let mut clip = unhashed();
    clip.content_hash = Some([0xab; 32]);
    clip
}

/// The id of `state_with(unhashed())`, from the code before the change.
const UNHASHED_ID: &str = "0bcd5bc47d2293462403de786af8b0eb8026096f8af02102afd083555ae3eca4";
/// The id of `state_with(hashed())`, from the code before the change.
const HASHED_ID: &str = "ba3dc81e5b0715bc2833925aefa714aab9ee2ef645dd67fadda175b0b52919e8";

#[test]
fn a_clip_serialises_its_content_hash_as_before() {
    let none = serde_json::to_value(unhashed()).unwrap();
    assert_eq!(
        none,
        serde_json::json!({
            "source_path": "audio/take1.wav",
            "start_in_track": 0,
            "source_offset": 0,
            "length": 48_000,
            "content_hash": null,
        }),
        "a clip with no hash still writes `content_hash: null`"
    );
    let some = serde_json::to_value(hashed()).unwrap();
    assert_eq!(some["content_hash"], serde_json::json!("ab".repeat(32)));
}

#[test]
fn node_ids_of_states_with_clips_hash_as_before() {
    assert_eq!(
        NodeId::from_state(&state_with(unhashed()))
            .unwrap()
            .to_hex(),
        UNHASHED_ID
    );
    assert_eq!(
        NodeId::from_state(&state_with(hashed())).unwrap().to_hex(),
        HASHED_ID
    );
}

#[test]
fn stored_clips_round_trip_to_the_same_bytes() {
    for clip in [unhashed(), hashed()] {
        let bytes = serde_json::to_vec(&clip).unwrap();
        let back: Clip = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(back, clip);
        assert_eq!(serde_json::to_vec(&back).unwrap(), bytes);
    }
}

#[test]
fn a_clip_without_the_field_reads_as_unhashed() {
    let missing = serde_json::json!({
        "source_path": "audio/take1.wav",
        "start_in_track": 0,
        "source_offset": 0,
        "length": 48_000,
    });
    let clip: Clip = serde_json::from_value(missing).unwrap();
    assert_eq!(clip, unhashed());
    // And it is written back with the field, as every other clip is.
    let state = state_with(clip);
    assert_eq!(NodeId::from_state(&state).unwrap().to_hex(), UNHASHED_ID);
}

#[test]
fn a_malformed_hash_is_still_refused() {
    let bad = serde_json::json!({
        "source_path": "audio/take1.wav",
        "start_in_track": 0,
        "source_offset": 0,
        "length": 48_000,
        "content_hash": "not hex",
    });
    assert!(serde_json::from_value::<Clip>(bad).is_err());
}
