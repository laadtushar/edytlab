//! A track with no single rate refuses a time edit before anything is
//! touched (ADR 0004, #715).
//!
//! `time_edit::Scope::rates` is asked before a tool edits, so a track whose
//! clips come from files of different rates (#673: a second is not one number
//! of frames there) refuses the whole call and appends no node. `cut_range`,
//! `trim`, `time_shift` and `split_by_speaker` are in `clip_rate_mixed.rs`,
//! the ramps in `sliding_stretch.rs`, `paulstretch` with a range in
//! `paulstretch.rs`; these are the other tools that make the call, and the
//! two tools that ask whenever sync-lock is on, carrying anything or not.

use serde_json::json;

#[path = "support/harness.rs"]
mod harness;
use harness::{write_tone, Session};

const BED_RATE: u32 = 48_000;
const VOICE_RATE: u32 = 16_000;

/// A 48 kHz bed (track 0) and a 16 kHz voice (track 1) whose second clip is
/// the bed's file, so the voice track has no single rate. `sync_lock` says
/// whether the session carries an edit to every track.
fn mixed(sync_lock: bool) -> Session {
    let mut s = Session::empty();
    let bed = write_tone(&s.dir.path().join("bed.wav"), BED_RATE, 6);
    let voice = write_tone(&s.dir.path().join("voice.wav"), VOICE_RATE, 4);
    s.load(&bed);
    s.load(&voice);
    let mut state = s.state();
    let extra = state.tracks[0].clips[0].clone();
    state.tracks[1].clips.push(session::Clip {
        start_in_track: 4 * u64::from(VOICE_RATE),
        ..extra
    });
    s.append_state("mixed", state);
    if sync_lock {
        s.ok("set_sync_lock", json!({ "enabled": true }));
    }
    s
}

/// `tool` refuses with the rate message and leaves the history as it was.
fn refuses(s: &mut Session, tool: &str, args: serde_json::Value) {
    let (head, nodes, state) = (s.head(), s.node_count(), s.state());
    let msg = s.err(tool, args);
    assert!(msg.contains("different sample rates"), "{tool}: {msg}");
    assert_eq!((s.head(), s.node_count()), (head, nodes), "{tool}");
    assert_eq!(s.state(), state, "{tool} changed nothing");
}

#[test]
fn insert_silence_under_sync_lock_refuses_a_track_with_two_rates() {
    let mut s = mixed(true);
    refuses(
        &mut s,
        "insert_silence",
        json!({ "track": 0, "at": 1.0, "duration": 0.5 }),
    );
}

#[test]
fn paste_region_under_sync_lock_refuses_a_track_with_two_rates() {
    let mut s = mixed(true);
    s.ok(
        "copy_region",
        json!({ "track": 0, "range": { "start_sec": 0.0, "end_sec": 1.0 } }),
    );
    refuses(&mut s, "paste_region", json!({ "track": 0, "at": 1.0 }));
}

/// Without sync-lock nothing asks about the other tracks: the same edit goes
/// through, because it never reaches the voice.
#[test]
fn without_sync_lock_the_other_tracks_rates_are_not_asked() {
    let mut s = mixed(false);
    s.ok(
        "insert_silence",
        json!({ "track": 0, "at": 1.0, "duration": 0.5 }),
    );
}

/// A whole-track stretch carries nothing to the other tracks, and is refused
/// all the same when sync-lock is on and one of them has no single rate: the
/// question is asked of every tool that follows sync-lock, not only of the
/// edits that carry (as it always was).
#[test]
fn a_whole_track_stretch_under_sync_lock_asks_every_tracks_rate_too() {
    let mut s = mixed(true);
    refuses(&mut s, "paulstretch", json!({ "track": 0, "factor": 2.0 }));
    refuses(
        &mut s,
        "time_stretch",
        json!({ "track": 0, "factor": 1.0, "end_factor": 0.5 }),
    );
}

#[test]
fn cut_words_refuses_a_track_with_two_rates() {
    let mut s = mixed(false);
    s.with_track_transcript(1, &[("one", 0.0, 1.0), ("two", 1.0, 2.0)]);
    refuses(
        &mut s,
        "cut_words",
        json!({ "from_word": 0, "to_word": 1, "track": 1 }),
    );
}

#[test]
fn remove_fillers_refuses_a_track_with_two_rates() {
    let mut s = mixed(false);
    s.with_track_transcript(1, &[("one", 0.0, 1.0), ("um", 1.0, 1.5), ("two", 1.5, 2.5)]);
    refuses(
        &mut s,
        "remove_fillers",
        json!({ "apply": true, "words": ["um"], "track": 1 }),
    );
}
