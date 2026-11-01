//! Filler-word removal (#165).
//!
//! The code is a filter plus a cut. The judgement is what needs
//! testing: that it reports before it acts, that it does not strip
//! every hesitation into rushed-sounding speech, that a pause is left
//! where the breath was, and that forty-seven fillers are one undoable
//! edit rather than forty-seven.

use serde_json::json;

#[path = "support/harness.rs"]
mod harness;
use harness::{ok, Session};
use tools::ToolResult;

const SAMPLE_RATE: u32 = 48_000;
const SECONDS: usize = 10;

/// "so um the thing is uh basically fine" — two hesitations, and a
/// marker mid-sentence that is doing rhythmic work.
fn speech() -> Vec<(&'static str, f32, f32)> {
    vec![
        ("So", 0.0, 0.4),
        ("um", 0.4, 0.8),
        ("the", 0.8, 1.0),
        ("thing", 1.0, 1.4),
        ("is", 1.4, 1.6),
        ("uh", 1.6, 2.0),
        ("basically", 2.0, 2.6),
        ("fine", 2.6, 3.0),
    ]
}

/// **It reports and does not act.** A destructive edit across a whole
/// track is not something to do because it was mentioned.
#[test]
fn the_default_is_a_report_not_an_edit() {
    let mut s = Session::with_sine(SAMPLE_RATE, SECONDS);
    s.with_transcript(&speech());
    let before = s.node_count();
    let length_before = s.state().length_samples;

    let v = ok(s.call("remove_fillers", json!({})));

    assert_eq!(v["applied"], json!(false));
    assert_eq!(v["found"], json!(2), "um and uh: {v}");
    assert!(v["would_save_sec"].as_f64().unwrap() > 0.0);
    assert!(
        v["summary"].as_str().unwrap().contains("apply"),
        "should say how to proceed: {v}"
    );
    assert_eq!(s.node_count(), before, "nothing may be appended");
    assert_eq!(s.state().length_samples, length_before);
}

/// **Not every hesitation.** "basically" sits mid-sentence with no
/// pause around it, so it is carrying rhythm and stays.
#[test]
fn a_marker_mid_sentence_is_left_alone() {
    let mut s = Session::with_sine(SAMPLE_RATE, SECONDS);
    s.with_transcript(&speech());

    let v = ok(s.call("remove_fillers", json!({})));
    let words: Vec<&str> = v["words"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w["text"].as_str().unwrap())
        .collect();
    assert_eq!(words, vec!["um", "uh"]);
    assert!(
        !words.contains(&"basically"),
        "a marker with no pause around it is doing work"
    );
}

/// The same marker standing alone between pauses is a filler.
#[test]
fn a_marker_standing_alone_is_removed() {
    let mut s = Session::with_sine(SAMPLE_RATE, SECONDS);
    s.with_transcript(&[
        ("So", 0.0, 0.4),
        // Half a second of silence either side.
        ("basically", 1.0, 1.6),
        ("fine", 2.2, 2.6),
    ]);

    let v = ok(s.call("remove_fillers", json!({})));
    assert_eq!(v["found"], json!(1), "{v}");
    assert_eq!(v["words"][0]["text"], json!("basically"));
}

/// Forty-seven fillers is one edit, not forty-seven.
#[test]
fn applying_is_one_undoable_node() {
    let mut s = Session::with_sine(SAMPLE_RATE, SECONDS);
    s.with_transcript(&speech());
    let before = s.node_count();

    let v = ok(s.call("remove_fillers", json!({ "apply": true })));

    assert_eq!(v["applied"], json!(true));
    assert_eq!(v["found"], json!(2));
    assert_eq!(s.node_count(), before + 1, "one node for the whole edit");
}

/// The words go, and the ones that remain still line up.
#[test]
fn the_transcript_and_the_audio_stay_in_step() {
    let mut s = Session::with_sine(SAMPLE_RATE, SECONDS);
    s.with_transcript(&speech());
    let length_before = s.state().length_samples;

    let v = ok(s.call("remove_fillers", json!({ "apply": true, "keep_gap_ms": 0 })));

    let state = s.state();
    let words = state.transcript.expect("transcript").words;
    assert_eq!(
        words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>(),
        vec!["So", "the", "thing", "is", "basically", "fine"]
    );
    // Two 0.4s fillers removed with no gap retained: "the" starts where
    // "um" did.
    assert!(
        (words[1].start_s - 0.4).abs() < 1e-3,
        "got {}",
        words[1].start_s
    );
    assert!(
        (words[4].start_s - 1.2).abs() < 1e-3,
        "got {}",
        words[4].start_s
    );

    let removed = v["removed_sec"].as_f64().unwrap();
    assert!((removed - 0.8).abs() < 1e-3, "0.8s of filler: {removed}");
    assert!(state.length_samples < length_before);
}

/// **A pause is left where the breath was.** Cutting dead against the
/// next word makes speech sound spliced.
#[test]
fn a_pause_is_left_behind() {
    let mut s = Session::with_sine(SAMPLE_RATE, SECONDS);
    s.with_transcript(&speech());

    let v = ok(s.call(
        "remove_fillers",
        json!({ "apply": true, "keep_gap_ms": 100 }),
    ));

    // Two fillers of 0.4s each, 0.1s retained from each.
    let removed = v["removed_sec"].as_f64().unwrap();
    assert!(
        (removed - 0.6).abs() < 1e-3,
        "0.3s should survive as pauses: removed {removed}"
    );
    assert!(v["summary"].as_str().unwrap().contains("pause"), "{v}");
}

/// The word list is the user's. Fillers are language- and
/// speaker-specific, and one person's filler is another's emphasis.
#[test]
fn a_custom_word_list_replaces_the_built_in_one() {
    let mut s = Session::with_sine(SAMPLE_RATE, SECONDS);
    s.with_transcript(&speech());

    let v = ok(s.call("remove_fillers", json!({ "words": ["basically"] })));
    assert_eq!(v["found"], json!(1), "{v}");
    assert_eq!(
        v["words"][0]["text"],
        json!("basically"),
        "a supplied list is taken at its word, pauses or not"
    );
}

#[test]
fn a_clean_transcript_says_so() {
    let mut s = Session::with_sine(SAMPLE_RATE, SECONDS);
    s.with_transcript(&[("So", 0.0, 0.4), ("fine", 0.4, 0.8)]);
    let v = ok(s.call("remove_fillers", json!({})));
    assert_eq!(v["found"], json!(0));
    assert!(v["summary"].as_str().unwrap().contains("No filler"));
}

#[test]
fn no_transcript_says_what_to_do() {
    let mut s = Session::with_sine(SAMPLE_RATE, SECONDS);
    let msg = match s.call("remove_fillers", json!({})) {
        ToolResult::Error(m) => m,
        ToolResult::Ok(v) => panic!("expected an error, got {v}"),
    };
    assert!(msg.contains("transcribe"), "{msg}");
}

/// Punctuation and case must not hide a filler.
#[test]
fn matching_ignores_case_and_punctuation() {
    let mut s = Session::with_sine(SAMPLE_RATE, SECONDS);
    s.with_transcript(&[("Um,", 0.0, 0.4), ("right", 0.4, 0.8), ("UH", 0.8, 1.2)]);
    let v = ok(s.call("remove_fillers", json!({})));
    assert_eq!(v["found"], json!(2), "\"Um,\" and \"UH\" are fillers: {v}");
}
