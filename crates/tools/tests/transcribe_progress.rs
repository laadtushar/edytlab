//! What the progress strip is told while `transcribe` runs, and Cancel (#384).
//!
//! The sink is process-wide and first-wins (`tools::progress::set_sink`), and
//! so is the cancel switch, so these share a binary, set the sink once, and
//! take turns.

#[path = "support/speech.rs"]
mod speech;

use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use serde_json::{json, Value};
use speech::{tone, word, Fake, Project};
use tools::tool::transcribe::{SpeechError, SpeechToText};

static EVENTS: Mutex<Vec<Value>> = Mutex::new(Vec::new());
static TURN: Mutex<()> = Mutex::new(());

/// One test at a time, with the sink set and an empty list of events.
fn turn() -> MutexGuard<'static, ()> {
    static SET: OnceLock<()> = OnceLock::new();
    SET.get_or_init(|| {
        assert!(tools::progress::set_sink(|event| {
            EVENTS.lock().unwrap().push(event);
        }));
    });
    let guard = TURN.lock().unwrap_or_else(|e| e.into_inner());
    EVENTS.lock().unwrap().clear();
    tools::progress::begin();
    guard
}

fn transcribe_events() -> Vec<Value> {
    EVENTS
        .lock()
        .unwrap()
        .iter()
        .filter(|e| e["kind"] == "transcribe")
        .cloned()
        .collect()
}

fn project(windows: usize) -> (Arc<Fake>, Project) {
    let mut fake = Fake::new(vec![word("Hello.", 0.3, 0.8)]);
    fake.windows = windows;
    let fake = Arc::new(fake);
    let mut p = Project::new(Arc::clone(&fake) as Arc<dyn SpeechToText>);
    p.load("a.wav", &tone(2.0, 220.0));
    (fake, p)
}

#[test]
fn each_window_is_reported_and_a_done_closes_the_strip() {
    let _turn = turn();
    let (_fake, mut p) = project(3);
    p.ok("transcribe", json!({ "track": 0 }));

    let events = transcribe_events();
    assert_eq!(
        events,
        vec![
            json!({ "kind": "transcribe", "index": 0, "total": 3, "file": "track 0" }),
            json!({ "kind": "transcribe", "index": 1, "total": 3, "file": "track 0" }),
            json!({ "kind": "transcribe", "index": 2, "total": 3, "file": "track 0" }),
            json!({ "kind": "transcribe", "done": true, "cancelled": false, "total": 3, "file": "track 0" }),
        ]
    );
}

#[test]
fn a_file_is_labelled_by_its_name_and_not_its_path() {
    let _turn = turn();
    let (_fake, mut p) = project(1);
    let file = p.dir.path().join("interview.wav");
    speech::write_wav(&file, &tone(1.0, 440.0));
    p.ok("transcribe", json!({ "path": file.to_string_lossy() }));
    let events = transcribe_events();
    assert_eq!(events[0]["file"], "interview.wav", "{events:?}");
}

#[test]
fn a_run_that_fails_still_clears_the_strip() {
    let _turn = turn();
    let (fake, mut p) = project(2);
    *fake.fails_with.lock().unwrap() = Some(SpeechError::Failed("the decoder gave up".into()));
    p.err("transcribe", json!({ "track": 0 }));

    let events = transcribe_events();
    let last = events.last().expect("events");
    assert_eq!(last["done"], true, "{events:?}");
    assert_eq!(last["cancelled"], false);
    assert_eq!(events.len(), 3, "two windows and the done: {events:?}");
}

#[test]
fn an_answer_from_the_cache_shows_no_strip_at_all() {
    let _turn = turn();
    let (fake, mut p) = project(2);
    p.ok("transcribe", json!({ "track": 0 }));
    EVENTS.lock().unwrap().clear();

    let v = p.ok("transcribe", json!({ "track": 0 }));
    assert_eq!(v["cached"], true);
    assert_eq!(fake.runs(), 1);
    assert!(transcribe_events().is_empty(), "{:?}", transcribe_events());
}

#[test]
fn a_setup_problem_shows_no_strip() {
    let _turn = turn();
    let (fake, mut p) = project(2);
    *fake.model.lock().unwrap() = Err(SpeechError::ModelNotDownloaded);
    p.err("transcribe", json!({ "track": 0 }));
    assert!(transcribe_events().is_empty());
}

/// Cancel is pressed after the second window. The run stops at the next
/// check, appends nothing, says it was cancelled and not to retry, and the
/// strip is told `cancelled`.
#[test]
fn cancel_stops_the_run_between_windows_and_appends_nothing() {
    let _turn = turn();
    let (fake, mut p) = project(4);
    *fake.cancel_after.lock().unwrap() = Some(1);
    let nodes = p.node_count();

    let msg = p.err("transcribe", json!({ "track": 0 }));

    assert!(msg.contains("cancelled by the user"), "{msg}");
    assert!(msg.contains("Nothing was changed"), "{msg}");
    assert_eq!(p.node_count(), nodes, "no node");
    assert!(p.state().transcript.is_none());
    assert!(p.cache_files().is_empty(), "a cancelled run is not cached");

    let events = transcribe_events();
    let windows = events.iter().filter(|e| e.get("index").is_some()).count();
    assert_eq!(windows, 2, "windows 0 and 1 were shown: {events:?}");
    let last = events.last().unwrap();
    assert_eq!(last["done"], true);
    assert_eq!(last["cancelled"], true, "{events:?}");
}

/// A Cancel pressed when nothing was running must not stop the next
/// transcription: the run clears it before its first window.
#[test]
fn a_cancel_left_over_from_before_does_not_stop_the_next_run() {
    let _turn = turn();
    let (_fake, mut p) = project(2);
    tools::progress::request_cancel();
    let v = p.ok("transcribe", json!({ "track": 0 }));
    assert_eq!(v["word_count"], 1);
    assert!(!tools::progress::cancelled());
}
