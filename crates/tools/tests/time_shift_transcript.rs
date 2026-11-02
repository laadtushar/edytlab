//! Track-sourced words follow the actual timeline movement of their audio.

#[path = "support/harness.rs"]
mod harness;

use harness::Session;
use serde_json::json;
use session::TranscriptSource;

fn words(s: &Session) -> Vec<(f32, f32)> {
    s.state()
        .transcript
        .unwrap()
        .words
        .iter()
        .map(|w| (w.start_s, w.end_s))
        .collect()
}

#[test]
fn source_words_follow_the_applied_shift_and_history_keeps_the_old_times() {
    // The session and target track have different sample rates.
    let mut s = Session::with_tracks(&[(48_000, vec![0.25; 48_000]), (8_000, vec![0.25; 8_000])]);
    s.with_track_transcript(1, &[("hello", 0.5, 0.75)]);
    let old_head = s.head();
    let old_count = s.node_count();
    let shifted = s.ok("time_shift", json!({"track": 1, "offset_sec": 2.0}));
    assert_eq!(shifted["applied_sec"], 2.0);
    assert_eq!(words(&s), vec![(2.5, 2.75)]);
    assert_eq!(s.state().tracks[1].clips[0].start_in_track, 16_000);
    s.assert_one_node_appended_after(old_head, old_count);
    let old = s.store.get(old_head).unwrap().state.transcript.unwrap();
    assert_eq!((old.words[0].start_s, old.words[0].end_s), (0.5, 0.75));

    let clamped = s.ok("time_shift", json!({"track": 1, "offset_sec": -3.0}));
    assert_eq!(clamped["applied_sec"], -2.0);
    assert_eq!(clamped["clamped"], true);
    assert_eq!(s.state().tracks[1].clips[0].start_in_track, 0);
    assert_eq!(words(&s), vec![(0.5, 0.75)]);
}

#[test]
fn another_tracks_words_are_unchanged() {
    let mut s = Session::with_tracks(&[(8_000, vec![0.25; 8_000]), (8_000, vec![0.25; 8_000])]);
    s.with_track_transcript(0, &[("hello", 0.5, 0.75)]);
    let before = s.state().transcript;
    s.ok("time_shift", json!({"track": 1, "offset_sec": 1.0}));
    assert_eq!(s.state().transcript, before);
}

#[test]
fn file_and_unknown_source_words_are_unchanged() {
    for source in [Some(TranscriptSource::File), None] {
        let mut s = Session::with_sine(8_000, 1);
        s.with_transcript(&[("hello", 0.5, 0.75)]);
        let mut state = s.state();
        state.transcript.as_mut().unwrap().source = source;
        s.append_state("set source", state);
        let before = s.state().transcript;
        s.ok("time_shift", json!({"track": 0, "offset_sec": 1.0}));
        assert_eq!(s.state().transcript, before);
    }
}

#[test]
fn cutting_a_shifted_word_removes_the_audio_it_describes() {
    let mut s = Session::with_sine(8_000, 1);
    s.with_track_transcript(0, &[("hello", 0.5, 0.75)]);
    s.ok("time_shift", json!({"track": 0, "offset_sec": 2.0}));
    s.ok(
        "cut_words",
        json!({"track": 0, "from_word": 0, "to_word": 1}),
    );
    let state = s.state();
    let remaining: u64 = state.tracks[0].clips.iter().map(|c| c.length).sum();
    assert_eq!(remaining, 6_000, "a quarter second of audio was removed");
}

#[test]
fn an_empty_track_does_not_move_its_transcript() {
    let mut s = Session::with_sine(8_000, 1);
    s.with_track_transcript(0, &[("hello", 0.5, 0.75)]);
    let mut state = s.state();
    state.tracks[0].clips.clear();
    s.append_state("empty track", state);
    s.ok("time_shift", json!({"track": 0, "offset_sec": 2.0}));
    assert_eq!(words(&s), vec![(0.5, 0.75)]);
}
