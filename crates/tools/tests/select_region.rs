//! Turning a description into a range (#168 §3).
//!
//! The leverage here is indirect: **every range-taking tool already
//! exists**, and none of them could be reached by describing where to
//! apply it. So the tests are about the two things that make a resolved
//! range usable — that it lands on the right audio, and that a
//! description which does not resolve produces a refusal rather than
//! the closest thing found. A selection that is nearly right is worse
//! than none, because the next call is a destructive edit.

use std::path::{Path, PathBuf};

use hound::{SampleFormat, WavSpec, WavWriter};
use serde_json::{json, Value};
use tempfile::TempDir;
use tools::{ToolContext, ToolDispatcher, ToolResult};

const SR: u32 = 48_000;

fn write_tone(path: &Path, seconds: usize) -> PathBuf {
    let spec = WavSpec {
        channels: 1,
        sample_rate: SR,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut w = WavWriter::create(path, spec).expect("wav writer");
    for n in 0..(SR as usize * seconds) {
        let t = n as f32 / SR as f32;
        w.write_sample(((2.0 * std::f32::consts::PI * 440.0 * t).sin() * 8000.0) as i16)
            .unwrap();
    }
    w.finalize().unwrap();
    path.to_path_buf()
}

struct Session {
    _dir: TempDir,
    store: session::Store,
    engine: audio_engine::Engine,
    dispatcher: ToolDispatcher,
    clipboard: Option<Vec<f32>>,
}

impl Session {
    fn new() -> Self {
        let dir = TempDir::new().expect("tempdir");
        let src = write_tone(&dir.path().join("take.wav"), 30);
        let store = session::Store::open(dir.path()).expect("open store");
        let mut s = Self {
            _dir: dir,
            store,
            engine: audio_engine::Engine::new(),
            dispatcher: ToolDispatcher::default_dispatcher(),
            clipboard: None,
        };
        s.call("load", json!({ "path": src.to_string_lossy() }));
        s
    }

    fn call(&mut self, tool: &str, args: Value) -> ToolResult {
        let mut ctx = ToolContext {
            store: &mut self.store,
            engine: &mut self.engine,
            user_message: "",
            clipboard: &mut self.clipboard,
        };
        self.dispatcher.invoke(tool, args, &mut ctx).unwrap()
    }

    /// Two sentences with a long gap, so passages are unambiguous.
    fn with_transcript(&mut self, words: &[(&str, f32, f32)]) {
        let head = self.store.head().expect("head");
        let mut state = self.store.get(head).expect("node").state;
        state.transcript = Some(session::Transcript {
            words: words
                .iter()
                .map(|(text, a, b)| session::TranscriptWord {
                    text: (*text).to_string(),
                    start_s: *a,
                    end_s: *b,
                    confidence: 0.9,
                })
                .collect(),
        });
        self.store
            .append(session::SessionNode {
                id: session::NodeId([0u8; 32]),
                parent: None,
                created_at: chrono::Utc::now(),
                label: Some("transcript".into()),
                reasoning: None,
                state,
                op: None,
            })
            .expect("append");
    }
}

fn ok(r: ToolResult) -> Value {
    match r {
        ToolResult::Ok(v) => v,
        ToolResult::Error(m) => panic!("expected Ok, got Error({m})"),
    }
}

fn err(r: ToolResult) -> String {
    match r {
        ToolResult::Error(m) => m,
        ToolResult::Ok(v) => panic!("expected Error, got Ok({v})"),
    }
}

/// Two sentences: 2–4s and 12–15s.
fn transcript() -> Vec<(&'static str, f32, f32)> {
    vec![
        ("The", 2.0, 2.3),
        ("latency", 2.3, 3.0),
        ("was", 3.0, 3.4),
        ("terrible", 3.4, 4.0),
        ("We", 12.0, 12.4),
        ("fixed", 12.4, 13.0),
        ("the", 13.0, 13.3),
        ("latency", 13.3, 14.0),
        ("problem", 14.0, 15.0),
    ]
}

/// The headline case: a phrase becomes the range of the words that say
/// it, with real timings.
#[test]
fn a_phrase_resolves_to_the_words_that_say_it() {
    let mut s = Session::new();
    s.with_transcript(&transcript());

    let v = ok(s.call("select_region", json!({ "text": "the latency" })));

    // "The latency" — words 0 and 1, so 2.0s to 3.0s.
    assert!((v["start_sec"].as_f64().unwrap() - 2.0).abs() < 0.01, "{v}");
    assert!((v["end_sec"].as_f64().unwrap() - 3.0).abs() < 0.01, "{v}");
}

/// Punctuation and case are the transcriber's, not the user's — so
/// "latency" must match "latency," and "Latency".
#[test]
fn matching_ignores_case_and_the_punctuation_a_transcriber_adds() {
    let mut s = Session::new();
    s.with_transcript(&[("Well,", 1.0, 1.4), ("LATENCY.", 1.4, 2.0)]);

    let v = ok(s.call("select_region", json!({ "text": "latency" })));
    assert!((v["start_sec"].as_f64().unwrap() - 1.4).abs() < 0.01, "{v}");
}

/// A repeated phrase is addressable rather than ambiguous.
#[test]
fn a_repeated_phrase_can_be_asked_for_by_occurrence() {
    let mut s = Session::new();
    s.with_transcript(&transcript());

    let first = ok(s.call("select_region", json!({ "text": "latency" })));
    let second = ok(s.call(
        "select_region",
        json!({ "text": "latency", "occurrence": 2 }),
    ));

    assert!(
        (first["start_sec"].as_f64().unwrap() - 2.3).abs() < 0.01,
        "{first}"
    );
    assert!(
        (second["start_sec"].as_f64().unwrap() - 13.3).abs() < 0.01,
        "{second}"
    );
}

/// **The refusal that matters.** A phrase that is not there selects
/// nothing — the next tool call is a destructive edit, so the nearest
/// match would be worse than an error.
#[test]
fn a_phrase_that_is_not_there_refuses_rather_than_guessing() {
    let mut s = Session::new();
    s.with_transcript(&transcript());

    let msg = err(s.call("select_region", json!({ "text": "bandwidth" })));
    assert!(msg.contains("not in the transcript"), "{msg}");
    assert!(
        msg.contains("nearly right is worse"),
        "and says why it did not approximate: {msg}"
    );
}

/// Asking for the fifth of two says so, rather than clamping.
#[test]
fn an_occurrence_past_the_end_says_how_many_there_are() {
    let mut s = Session::new();
    s.with_transcript(&transcript());

    let msg = err(s.call(
        "select_region",
        json!({ "text": "latency", "occurrence": 5 }),
    ));
    assert!(msg.contains("appears 2 time(s)"), "{msg}");
}

/// "The last thing he said" without having to know how many there were.
#[test]
fn a_negative_passage_counts_from_the_end() {
    let mut s = Session::new();
    s.with_transcript(&transcript());

    let last = ok(s.call("select_region", json!({ "speech_passage": -1 })));
    assert!(
        (last["start_sec"].as_f64().unwrap() - 12.0).abs() < 0.01,
        "{last}"
    );
    assert!(
        (last["end_sec"].as_f64().unwrap() - 15.0).abs() < 0.01,
        "{last}"
    );

    let first = ok(s.call("select_region", json!({ "speech_passage": 1 })));
    assert!(
        (first["start_sec"].as_f64().unwrap() - 2.0).abs() < 0.01,
        "{first}"
    );
}

/// Beat-based descriptions work too, which is the other half of the
/// acceptance — music, not just speech.
#[test]
fn a_beat_range_resolves_through_the_tempo_map() {
    let mut s = Session::new();
    // Default tempo is 120 BPM, so a beat is half a second.
    let v = ok(s.call("select_region", json!({ "from_beat": 1, "to_beat": 9 })));

    assert!((v["start_sec"].as_f64().unwrap() - 0.0).abs() < 0.01, "{v}");
    assert!(
        (v["end_sec"].as_f64().unwrap() - 4.0).abs() < 0.01,
        "eight beats at 120 BPM is four seconds: {v}"
    );
}

/// Padding, for a selection that needs to breathe — and it must not go
/// negative at the start of the session.
#[test]
fn padding_widens_the_range_without_going_below_zero() {
    let mut s = Session::new();
    s.with_transcript(&[("Hello", 0.2, 0.6)]);

    let v = ok(s.call("select_region", json!({ "text": "hello", "pad_sec": 1.0 })));
    assert_eq!(
        v["start_sec"].as_f64().unwrap(),
        0.0,
        "clamped, not negative: {v}"
    );
    assert!((v["end_sec"].as_f64().unwrap() - 1.6).abs() < 0.01, "{v}");
}

/// Selecting by what was said needs to know what was said.
#[test]
fn selecting_by_text_without_a_transcript_says_what_to_run() {
    let mut s = Session::new();
    let msg = err(s.call("select_region", json!({ "text": "anything" })));
    assert!(msg.contains("transcribe"), "{msg}");
}

/// Two selectors mean two different regions; picking one silently would
/// ignore half of what was asked.
#[test]
fn giving_two_selectors_is_refused() {
    let mut s = Session::new();
    s.with_transcript(&transcript());

    let msg = err(s.call(
        "select_region",
        json!({ "text": "latency", "speech_passage": 1 }),
    ));
    assert!(msg.contains("one selector"), "{msg}");
}

/// And giving none says what the options are.
#[test]
fn giving_no_selector_says_what_to_pass() {
    let mut s = Session::new();
    let msg = err(s.call("select_region", json!({})));
    assert!(msg.contains("speech_passage"), "{msg}");
}

/// The point of the whole thing: the resolved range feeds any tool that
/// takes one, with no changes to that tool.
#[test]
fn a_resolved_range_drives_an_existing_range_taking_tool() {
    let mut s = Session::new();
    s.with_transcript(&transcript());

    let sel = ok(s.call("select_region", json!({ "text": "the latency" })));
    let (start, end) = (
        sel["start_sec"].as_f64().unwrap(),
        sel["end_sec"].as_f64().unwrap(),
    );

    // `silence_region` knew nothing about descriptions and needed no
    // changes to be driven by one.
    let v = ok(s.call(
        "silence_region",
        json!({ "track": 0, "start_sec": start, "end_sec": end }),
    ));
    assert!(v["node_id"].is_string(), "the edit landed: {v}");
}
