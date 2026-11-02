//! An edit remaps a transcript only when the audio the words name moved
//! (#925).
//!
//! A transcript made from a track holds seconds of that track (#384), so a
//! cut or an insert on track A moves the words of a transcript of track B
//! only when sync-lock carried the edit to B. A transcript of a file is
//! seconds of the file, which no edit of the timeline moves. A transcript
//! with no recorded source (a project from before sources were kept) cannot
//! say, and keeps the behaviour it always had: every edit that remaps
//! a transcript remaps it.
//!
//! Labels are not tested here: they are the session's and move with every
//! edit, sync-lock or not (`labels_follow_edits.rs`).

use serde_json::json;
use session::TranscriptSource;

#[path = "support/harness.rs"]
mod harness;
use harness::{write_tone, Session};

const RATE: u32 = 48_000;

/// Two 4 s tone tracks, sync-lock as asked.
fn two_tracks(sync_lock: bool) -> Session {
    let mut s = Session::empty();
    for i in 0..2 {
        let p = write_tone(&s.dir.path().join(format!("t{i}.wav")), RATE, 4);
        s.load(&p);
    }
    if sync_lock {
        s.ok("set_sync_lock", json!({ "enabled": true }));
    }
    s
}

/// Which audio the single word "one" (2.0 to 2.5 s) names.
#[derive(Clone, Copy)]
enum Of {
    Track(usize),
    File,
    Unrecorded,
}

fn with_word(s: &mut Session, of: Of) {
    match of {
        Of::Track(t) => s.with_track_transcript(t, &[("one", 2.0, 2.5)]),
        Of::Unrecorded => s.with_transcript(&[("one", 2.0, 2.5)]),
        Of::File => {
            s.with_transcript(&[("one", 2.0, 2.5)]);
            let mut state = s.state();
            state.transcript.as_mut().unwrap().source = Some(TranscriptSource::File);
            s.append_state("file transcript", state);
        }
    }
}

fn word(s: &Session) -> (f32, f32) {
    let t = s.state().transcript.expect("a transcript");
    (t.words[0].start_s, t.words[0].end_s)
}

const STAYS: (f32, f32) = (2.0, 2.5);

/// `insert_silence` of 2 s at 1.0 s on track 0.
fn insert(s: &mut Session) {
    s.ok(
        "insert_silence",
        json!({ "track": 0, "at": 1.0, "duration": 2.0 }),
    );
}
const AFTER_INSERT: (f32, f32) = (4.0, 4.5);

/// `cut_range` of 1.0 to 2.0 s on track 0.
fn cut(s: &mut Session) {
    s.ok(
        "cut_range",
        json!({ "track": 0, "start_sample": 48_000, "end_sample": 96_000 }),
    );
}
const AFTER_CUT: (f32, f32) = (1.0, 1.5);

#[test]
fn without_sync_lock_a_transcript_of_another_track_stays_put() {
    for edit in [insert, cut] {
        let mut s = two_tracks(false);
        with_word(&mut s, Of::Track(1));
        edit(&mut s);
        assert_eq!(word(&s), STAYS, "track 1's audio did not move");
    }
}

#[test]
fn without_sync_lock_a_transcript_of_the_edited_track_moves() {
    let mut s = two_tracks(false);
    with_word(&mut s, Of::Track(0));
    insert(&mut s);
    assert_eq!(word(&s), AFTER_INSERT);

    let mut s = two_tracks(false);
    with_word(&mut s, Of::Track(0));
    cut(&mut s);
    assert_eq!(word(&s), AFTER_CUT);
}

#[test]
fn with_sync_lock_the_transcript_of_either_track_moves() {
    for track in [0, 1] {
        let mut s = two_tracks(true);
        with_word(&mut s, Of::Track(track));
        insert(&mut s);
        assert_eq!(
            word(&s),
            AFTER_INSERT,
            "insert, transcript of track {track}"
        );

        let mut s = two_tracks(true);
        with_word(&mut s, Of::Track(track));
        cut(&mut s);
        assert_eq!(word(&s), AFTER_CUT, "cut, transcript of track {track}");
    }
}

#[test]
fn a_transcript_of_a_file_never_moves() {
    for sync_lock in [false, true] {
        let mut s = two_tracks(sync_lock);
        with_word(&mut s, Of::File);
        insert(&mut s);
        assert_eq!(word(&s), STAYS, "insert, sync-lock {sync_lock}");
        cut(&mut s);
        assert_eq!(word(&s), STAYS, "cut, sync-lock {sync_lock}");
    }
}

/// No recorded source keeps what it always did: it follows the edit whichever
/// track was edited and whether or not sync-lock is on.
#[test]
fn a_transcript_with_no_source_follows_every_edit_as_before() {
    for sync_lock in [false, true] {
        let mut s = two_tracks(sync_lock);
        with_word(&mut s, Of::Unrecorded);
        insert(&mut s);
        assert_eq!(word(&s), AFTER_INSERT, "sync-lock {sync_lock}");
    }
    let mut s = two_tracks(false);
    with_word(&mut s, Of::Unrecorded);
    s.ok(
        "insert_silence",
        json!({ "track": 1, "at": 1.0, "duration": 2.0 }),
    );
    assert_eq!(word(&s), AFTER_INSERT, "an edit of track 1 too");
}

/// The other remapping tools read the same rule.
#[test]
fn trim_paste_and_change_speed_move_only_the_words_of_their_track() {
    // trim track 0 to 1.0..3.0 s: track 1's words stay, track 0's go 1 s back.
    let trim = |s: &mut Session| {
        s.ok(
            "trim",
            json!({ "track": 0, "start_sample": 48_000, "end_sample": 144_000 }),
        );
    };
    let mut s = two_tracks(false);
    with_word(&mut s, Of::Track(1));
    trim(&mut s);
    assert_eq!(word(&s), STAYS, "trim, transcript of track 1");
    let mut s = two_tracks(false);
    with_word(&mut s, Of::Track(0));
    trim(&mut s);
    assert_eq!(word(&s), (1.0, 1.5), "trim, transcript of track 0");

    // change_speed x2 on track 0: track 1's words stay, track 0's halve.
    let speed = |s: &mut Session| {
        s.ok("change_speed", json!({ "track": 0, "factor": 2.0 }));
    };
    let mut s = two_tracks(true);
    with_word(&mut s, Of::Track(1));
    speed(&mut s);
    assert_eq!(word(&s), STAYS, "change_speed moves no other track");
    let mut s = two_tracks(false);
    with_word(&mut s, Of::Track(0));
    speed(&mut s);
    assert_eq!(word(&s), (1.0, 1.25), "change_speed, transcript of track 0");

    // paste_region: a 1 s clipboard at 1.0 s of track 0 (sync-lock off).
    let paste = |s: &mut Session| {
        s.ok(
            "copy_region",
            json!({ "track": 0, "range": { "start_sec": 0.0, "end_sec": 1.0 } }),
        );
        s.ok("paste_region", json!({ "track": 0, "at": 1.0 }));
    };
    let mut s = two_tracks(false);
    with_word(&mut s, Of::Track(1));
    paste(&mut s);
    assert_eq!(word(&s), STAYS, "paste, transcript of track 1");
    let mut s = two_tracks(false);
    with_word(&mut s, Of::Track(0));
    paste(&mut s);
    assert_eq!(word(&s), (3.0, 3.5), "paste, transcript of track 0");
}
