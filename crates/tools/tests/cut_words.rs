//! Editing the audio by editing the transcript (#157).
//!
//! The mapping is the feature: a span of words becomes a span of
//! samples, the audio is cut, and — the part that is easy to get wrong
//! — the words that remain still line up with what is left. A
//! transcript that drifts by the length of every edit is worse than no
//! transcript, because it looks right.

use serde_json::json;

#[path = "support/harness.rs"]
mod harness;
use harness::{err, ok, Session};

const SAMPLE_RATE: u32 = 48_000;
/// Four seconds, so four one-second "words" fit in it.
const SECONDS: usize = 4;

/// One word per second: "one two three four".
fn four_words() -> Vec<(&'static str, f32, f32)> {
    vec![
        ("one", 0.0, 1.0),
        ("two", 1.0, 2.0),
        ("three", 2.0, 3.0),
        ("four", 3.0, 4.0),
    ]
}

/// Deleting a word deletes its audio and closes the gap.
#[test]
fn cutting_a_word_removes_its_audio() {
    let mut s = Session::with_sine(SAMPLE_RATE, SECONDS);
    s.with_transcript(&four_words());
    let before = s.state().length_samples;

    let v = ok(s.call("cut_words", json!({ "from_word": 1, "to_word": 2 })));

    assert_eq!(v["removed_words"], json!(1));
    assert_eq!(v["removed_text"], json!("two"));
    let after = s.state().length_samples;
    assert_eq!(
        after,
        before - SAMPLE_RATE as u64,
        "one second of audio should be gone"
    );
}

/// **The part that is easy to get wrong.** Every word after the cut is
/// now earlier by exactly the removed duration.
#[test]
fn the_remaining_word_timings_still_line_up() {
    let mut s = Session::with_sine(SAMPLE_RATE, SECONDS);
    s.with_transcript(&four_words());

    ok(s.call("cut_words", json!({ "from_word": 1, "to_word": 2 })));

    let words = s.state().transcript.expect("transcript still there").words;
    let texts: Vec<&str> = words.iter().map(|w| w.text.as_str()).collect();
    assert_eq!(texts, vec!["one", "three", "four"], "the cut word is gone");

    // "one" is untouched; "three" and "four" each moved a second early.
    assert!((words[0].start_s - 0.0).abs() < 1e-4);
    assert!(
        (words[1].start_s - 1.0).abs() < 1e-4,
        "three should start at 1s"
    );
    assert!(
        (words[2].start_s - 2.0).abs() < 1e-4,
        "four should start at 2s"
    );
    assert!((words[2].end_s - 3.0).abs() < 1e-4);
}

/// A span of several words is one cut, and one node.
#[test]
fn a_span_of_words_is_one_undoable_node() {
    let mut s = Session::with_sine(SAMPLE_RATE, SECONDS);
    s.with_transcript(&four_words());
    let before = s.node_count();

    let v = ok(s.call("cut_words", json!({ "from_word": 1, "to_word": 3 })));

    assert_eq!(v["removed_words"], json!(2));
    assert_eq!(v["removed_text"], json!("two three"));
    assert_eq!(
        s.node_count(),
        before + 1,
        "one edit is one node, not one per word"
    );

    let words = s.state().transcript.unwrap().words;
    assert_eq!(
        words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>(),
        vec!["one", "four"]
    );
    assert!((words[1].start_s - 1.0).abs() < 1e-4, "four moves to 1s");
}

/// The cut spans the first word's *start* to the last word's *end*.
/// Starting at the first word's end would leave a clipped syllable
/// behind.
#[test]
fn the_span_covers_the_words_completely() {
    let mut s = Session::with_sine(SAMPLE_RATE, SECONDS);
    s.with_transcript(&four_words());

    let v = ok(s.call("cut_words", json!({ "from_word": 0, "to_word": 2 })));
    assert!((v["start_sec"].as_f64().unwrap() - 0.0).abs() < 1e-6);
    assert!((v["end_sec"].as_f64().unwrap() - 2.0).abs() < 1e-6);
    assert!((v["removed_sec"].as_f64().unwrap() - 2.0).abs() < 1e-6);
}

/// A session with no transcript says what to do rather than failing
/// blank.
#[test]
fn no_transcript_says_what_to_do() {
    let mut s = Session::with_sine(SAMPLE_RATE, SECONDS);
    let msg = err(s.call("cut_words", json!({ "from_word": 0, "to_word": 1 })));
    assert!(msg.contains("transcribe"), "should name the way out: {msg}");
}

#[test]
fn an_out_of_range_span_names_the_length() {
    let mut s = Session::with_sine(SAMPLE_RATE, SECONDS);
    s.with_transcript(&four_words());
    let msg = err(s.call("cut_words", json!({ "from_word": 2, "to_word": 99 })));
    assert!(
        msg.contains('4'),
        "should say how many words there are: {msg}"
    );
}

#[test]
fn an_inverted_span_is_refused() {
    let mut s = Session::with_sine(SAMPLE_RATE, SECONDS);
    s.with_transcript(&four_words());
    let msg = err(s.call("cut_words", json!({ "from_word": 3, "to_word": 1 })));
    assert!(msg.contains("greater than"), "{msg}");
}

/// Cutting the tail leaves the head alone and shortens the session.
#[test]
fn cutting_the_last_words_shortens_the_session() {
    let mut s = Session::with_sine(SAMPLE_RATE, SECONDS);
    s.with_transcript(&four_words());
    let before = s.state().length_samples;

    ok(s.call("cut_words", json!({ "from_word": 2, "to_word": 4 })));

    let state = s.state();
    assert_eq!(state.length_samples, before - 2 * SAMPLE_RATE as u64);
    let words = state.transcript.unwrap().words;
    assert_eq!(
        words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>(),
        vec!["one", "two"]
    );
    assert!(
        (words[1].end_s - 2.0).abs() < 1e-4,
        "untouched words do not move"
    );
}
