//! Progress and cancellation for a batch (#169 §1).
//!
//! In its own test binary on purpose. The progress sink and the cancel
//! flag are process-wide — see `tools::progress` for why — so a test
//! that asserts on them must be the only batch running in its process.
//! Rust runs each integration-test *file* as its own process but the
//! tests *within* one in parallel threads, so living beside
//! `batch_apply.rs` meant collecting another test's events and having
//! its `begin()` clear this one's cancel.
//!
//! That collision is a property of the product too, stated plainly: one
//! foreground batch at a time, and a cancel button means "stop the one
//! I am watching".

//! One chain across a folder (#169).
//!
//! Podcast work arrives as twelve files, not one. The claims worth
//! testing are the ones that make a batch usable unattended: every file
//! gets its **own** session, a failure on one does not abandon the
//! rest, and the report says which was which and why.

use serde_json::{json, Value};

#[path = "support/batch.rs"]
mod batch;
#[path = "support/harness.rs"]
mod harness;
use batch::{make_folder, make_recipe};
use harness::{ok, Session};

/// The run says where it is, file by file, and stops when asked.
///
/// The cancel is triggered *from the progress sink* rather than before
/// the call, because that is the real interaction: the user clicks
/// Cancel while the batch is running. A cancel set beforehand is
/// deliberately cleared by `begin()` — see `tools::progress` — so that
/// a stop meant for one run cannot kill the next.
///
/// One test rather than several because the sink can only be registered
/// once per process, and these are stages of one sequence.
#[test]
fn a_batch_reports_progress_and_stops_between_files_when_asked() {
    use std::sync::{Arc, Mutex};

    let seen: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
    let cancel_after_first = Arc::new(Mutex::new(false));

    let sink_seen = Arc::clone(&seen);
    let arm = Arc::clone(&cancel_after_first);
    assert!(
        tools::progress::set_sink(move |e| {
            sink_seen.lock().unwrap().push(e.clone());
            // Stand in for the user hitting Cancel while file 0 runs.
            if *arm.lock().unwrap() && e["index"] == json!(0) {
                tools::progress::request_cancel();
            }
        }),
        "this is the only registration in this process"
    );

    let mut s = Session::empty();
    let recipe = make_recipe(&mut s);
    let takes = make_folder(s.dir.path());

    // ── It reports, file by file ──────────────────────────────────
    ok(s.call(
        "batch_apply",
        json!({
            "recipe_path": recipe.to_string_lossy(),
            "input_dir": takes.to_string_lossy(),
            "output_dir": s.dir.path().join("out-a").to_string_lossy(),
        }),
    ));

    let events = seen.lock().unwrap().clone();
    let per_file: Vec<_> = events.iter().filter(|e| e["done"].is_null()).collect();
    assert_eq!(per_file.len(), 3, "one event per file: {events:?}");
    assert_eq!(per_file[0]["index"], json!(0));
    assert_eq!(per_file[0]["total"], json!(3));
    assert_eq!(
        per_file[2]["succeeded"],
        json!(2),
        "progress so far, not the final total"
    );
    assert_eq!(
        events.last().map(|e| e["done"].clone()),
        Some(json!(true)),
        "and a final event: {events:?}"
    );

    // ── Cancelling mid-run stops it between files ─────────────────
    seen.lock().unwrap().clear();
    *cancel_after_first.lock().unwrap() = true;

    let v = ok(s.call(
        "batch_apply",
        json!({
            "recipe_path": recipe.to_string_lossy(),
            "input_dir": takes.to_string_lossy(),
            "output_dir": s.dir.path().join("out-b").to_string_lossy(),
        }),
    ));

    assert_eq!(v["cancelled"], json!(true), "{v}");
    assert_eq!(
        v["attempted"],
        json!(1),
        "the file already in flight finishes; the next never starts: {v}"
    );
    assert_eq!(
        v["files"],
        json!(3),
        "and it still says how many there were"
    );
    assert!(
        v["summary"]
            .as_str()
            .unwrap_or_default()
            .contains("Stopped partway"),
        "the summary has to say it stopped: {v}"
    );

    // Nothing half-written: an unstarted file has no project at all,
    // rather than one whose history ends mid-chain.
    let out_b = s.dir.path().join("out-b");
    assert!(
        out_b.join("take-0").exists(),
        "the one it did finish is there"
    );
    assert!(
        !out_b.join("take-1").exists(),
        "and the one it never started has no project: {v}"
    );

    // ── A stale cancel does not poison the next run ───────────────
    *cancel_after_first.lock().unwrap() = false;
    let v = ok(s.call(
        "batch_apply",
        json!({
            "recipe_path": recipe.to_string_lossy(),
            "input_dir": takes.to_string_lossy(),
            "output_dir": s.dir.path().join("out-c").to_string_lossy(),
        }),
    ));
    assert_eq!(
        v["cancelled"],
        json!(false),
        "a stale cancel must not carry: {v}"
    );
    assert_eq!(v["succeeded"], json!(3), "{v}");
}
