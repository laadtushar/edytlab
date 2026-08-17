//! `storage_report` measures the problem #98 describes.
//!
//! The interesting claim is not "it adds up file sizes" — it is the
//! three-way split. #98's central observation is that undo makes every
//! node reachable by design, so "unreachable from the head" is not the
//! same as "safe to delete" and a naive mark-and-sweep would free
//! nothing. A report that collapsed the categories would make a sweep
//! look safe when it is not, which is the specific mistake that ticket
//! warns about.
//!
//! So each test here pins one boundary: that editing moves bytes from
//! nowhere into *history* rather than making them disappear, that the
//! head's own audio is never counted as reclaimable, and that a file
//! nothing names is found.

use std::path::Path;

use hound::{SampleFormat, WavSpec, WavWriter};
use serde_json::{json, Value};
use tempfile::TempDir;
use tools::{ToolContext, ToolDispatcher, ToolResult};

const SAMPLE_RATE: u32 = 48_000;

fn write_sine(dir: &Path, name: &str) -> std::path::PathBuf {
    let path = dir.join(name);
    let spec = WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut w = WavWriter::create(&path, spec).expect("wav writer");
    for n in 0..SAMPLE_RATE as usize {
        let t = n as f32 / SAMPLE_RATE as f32;
        let s = (2.0 * std::f32::consts::PI * 440.0 * t).sin() * 0.25;
        w.write_sample((s * 32_767.0) as i16).unwrap();
    }
    w.finalize().unwrap();
    path
}

fn ok(result: ToolResult) -> Value {
    match result {
        ToolResult::Ok(v) => v,
        ToolResult::Error(msg) => panic!("expected Ok, got Error({msg})"),
    }
}

/// Load a file, run `edits` destructive edits, and return the report.
fn report_after(edits: usize) -> (TempDir, Value) {
    let tmp = TempDir::new().expect("tempdir");
    let src = write_sine(tmp.path(), "in.wav");
    let mut store = session::Store::open(tmp.path()).expect("open store");
    let mut engine = audio_engine::Engine::new();
    let dispatcher = ToolDispatcher::default_dispatcher();
    let mut clipboard: Option<Vec<f32>> = None;
    let mut ctx = ToolContext {
        store: &mut store,
        engine: &mut engine,
        user_message: "",
        clipboard: &mut clipboard,
    };

    ok(dispatcher
        .invoke("load", json!({ "path": src.to_string_lossy() }), &mut ctx)
        .unwrap());
    for i in 0..edits {
        // A *destructive* edit — `gain` only records a track gain and
        // writes no audio at all, so it would measure nothing. Each
        // range is longer than the last so every edit produces distinct
        // samples and therefore a distinct content hash; repeating one
        // would hit the CAS and write no new file.
        let end = 0.1 * (i as f64 + 1.0);
        ok(dispatcher
            .invoke(
                "silence_region",
                json!({ "track": 0, "start_sec": 0.0, "end_sec": end }),
                &mut ctx,
            )
            .unwrap());
    }
    let v = ok(dispatcher
        .invoke("storage_report", json!({}), &mut ctx)
        .unwrap());
    (tmp, v)
}

fn u(v: &Value, path: &[&str]) -> u64 {
    let mut cur = v;
    for k in path {
        cur = &cur[k];
    }
    cur.as_u64()
        .unwrap_or_else(|| panic!("expected a number at {path:?}, got {cur}"))
}

/// **A project contains the audio it points at** (#156).
///
/// Derived files used to be written to `<source_dir>/derived/`, so a
/// project folder held only `project.json` and `.audiograph/` while
/// every sample sat next to whatever the user happened to open. A
/// project was therefore not a thing you could copy, move or back up.
#[test]
fn derived_audio_is_written_inside_the_project() {
    let tmp = TempDir::new().expect("tempdir");
    // The source lives somewhere else entirely, which is the normal
    // case: a user opens a file from their music folder.
    let elsewhere = TempDir::new().expect("source dir");
    let src = write_sine(elsewhere.path(), "in.wav");

    let mut store = session::Store::open(tmp.path()).expect("open store");
    let mut engine = audio_engine::Engine::new();
    let dispatcher = ToolDispatcher::default_dispatcher();
    let mut clipboard: Option<Vec<f32>> = None;
    let mut ctx = ToolContext {
        store: &mut store,
        engine: &mut engine,
        user_message: "",
        clipboard: &mut clipboard,
    };

    ok(dispatcher
        .invoke("load", json!({ "path": src.to_string_lossy() }), &mut ctx)
        .unwrap());
    ok(dispatcher
        .invoke(
            "silence_region",
            json!({ "track": 0, "start_sec": 0.0, "end_sec": 0.2 }),
            &mut ctx,
        )
        .unwrap());

    let head = ctx.store.head().expect("a head");
    let node = ctx.store.get(head).expect("head node");
    let derived = &node.state.tracks[0].clips[0].source_path;

    assert!(
        derived.starts_with(tmp.path()),
        "derived audio landed outside the project: {}",
        derived.display()
    );
    assert!(
        !derived.starts_with(elsewhere.path()),
        "derived audio was written beside the source: {}",
        derived.display()
    );
    assert!(derived.is_file(), "and it must actually be there");
}

/// A session with no edits has written no derived audio, and the report
/// says so rather than erroring or inventing a number.
#[test]
fn a_fresh_session_reports_nothing_derived() {
    let (_tmp, v) = report_after(0);
    assert_eq!(u(&v, &["total_bytes"]), 0);
    assert_eq!(u(&v, &["history", "files"]), 0);
    assert_eq!(u(&v, &["unreferenced", "files"]), 0);
}

/// **The measurement #98 asks for.**
///
/// Each destructive edit writes a file and none are removed, so the
/// total has to grow with the edit count — and the growth has to land in
/// *history*, not in `unreferenced`. Those older files are exactly what
/// undo is holding onto; calling them unreferenced would be the report
/// telling a sweep it may delete the undo stack.
#[test]
fn every_edit_adds_a_file_that_only_history_needs() {
    let (_a, one) = report_after(1);
    let (_b, four) = report_after(4);

    assert!(
        u(&four, &["total_bytes"]) > u(&one, &["total_bytes"]),
        "four edits should cost more than one: {} vs {}",
        u(&four, &["total_bytes"]),
        u(&one, &["total_bytes"])
    );
    // One edit: its output is the head's audio, so nothing is history
    // yet. Four: three superseded files are.
    assert_eq!(
        u(&one, &["history", "files"]),
        0,
        "the only derived file is the one the head uses"
    );
    assert_eq!(
        u(&four, &["history", "files"]),
        3,
        "three superseded files should be held by history, not by the head"
    );
    assert_eq!(
        u(&four, &["unreferenced", "files"]),
        0,
        "a superseded file is still named by its node — calling it \
         unreferenced would invite a sweep to delete the undo stack"
    );
}

/// The head's audio is never in a reclaimable category, whatever else
/// the session has done. Every option in #98 has this as its one
/// non-negotiable.
#[test]
fn the_heads_own_audio_is_always_live() {
    let (_tmp, v) = report_after(3);
    assert!(
        u(&v, &["live", "bytes"]) > 0,
        "the current version's audio must be counted live"
    );
    assert_eq!(u(&v, &["live", "files"]), 1);
}

/// A file in `derived/` that no node names — the leftover of an
/// interrupted edit — is the one category any policy could reclaim
/// without an argument, so the report has to actually find it.
#[test]
fn a_file_no_node_names_is_reported_unreferenced() {
    let tmp = TempDir::new().expect("tempdir");
    let src = write_sine(tmp.path(), "in.wav");
    let mut store = session::Store::open(tmp.path()).expect("open store");
    let mut engine = audio_engine::Engine::new();
    let dispatcher = ToolDispatcher::default_dispatcher();
    let mut clipboard: Option<Vec<f32>> = None;
    let mut ctx = ToolContext {
        store: &mut store,
        engine: &mut engine,
        user_message: "",
        clipboard: &mut clipboard,
    };
    ok(dispatcher
        .invoke("load", json!({ "path": src.to_string_lossy() }), &mut ctx)
        .unwrap());
    ok(dispatcher
        .invoke(
            "silence_region",
            json!({ "track": 0, "start_sec": 0.0, "end_sec": 0.1 }),
            &mut ctx,
        )
        .unwrap());

    // An orphan, as an interrupted edit would leave behind.
    let orphan = tmp
        .path()
        .join(".audiograph")
        .join("derived")
        .join("orphan.wav");
    std::fs::write(&orphan, vec![0u8; 4096]).expect("write orphan");

    let v = ok(dispatcher
        .invoke("storage_report", json!({}), &mut ctx)
        .unwrap());
    assert_eq!(u(&v, &["unreferenced", "files"]), 1);
    assert_eq!(u(&v, &["unreferenced", "bytes"]), 4096);
    let largest = v["largest_unreferenced"]
        .as_array()
        .expect("largest_unreferenced is an array");
    assert_eq!(largest.len(), 1);
    assert!(
        largest[0]["path"]
            .as_str()
            .expect("path")
            .ends_with("orphan.wav"),
        "the report should name the file: {}",
        largest[0]["path"]
    );
}

/// It is a report. #98 is explicit that no policy has been chosen and
/// that the wrong one silently loses work, so this tool must not be the
/// thing that quietly starts deleting.
#[test]
fn the_report_deletes_nothing() {
    let tmp = TempDir::new().expect("tempdir");
    let src = write_sine(tmp.path(), "in.wav");
    let mut store = session::Store::open(tmp.path()).expect("open store");
    let mut engine = audio_engine::Engine::new();
    let dispatcher = ToolDispatcher::default_dispatcher();
    let mut clipboard: Option<Vec<f32>> = None;
    let mut ctx = ToolContext {
        store: &mut store,
        engine: &mut engine,
        user_message: "",
        clipboard: &mut clipboard,
    };
    ok(dispatcher
        .invoke("load", json!({ "path": src.to_string_lossy() }), &mut ctx)
        .unwrap());
    for end in [0.1, 0.2, 0.3] {
        ok(dispatcher
            .invoke(
                "silence_region",
                json!({ "track": 0, "start_sec": 0.0, "end_sec": end }),
                &mut ctx,
            )
            .unwrap());
    }
    let orphan = tmp
        .path()
        .join(".audiograph")
        .join("derived")
        .join("orphan.wav");
    std::fs::write(&orphan, vec![0u8; 1024]).expect("write orphan");

    let before: Vec<_> = std::fs::read_dir(tmp.path().join(".audiograph").join("derived"))
        .expect("read derived")
        .flatten()
        .map(|e| e.path())
        .collect();
    ok(dispatcher
        .invoke("storage_report", json!({}), &mut ctx)
        .unwrap());
    let after: Vec<_> = std::fs::read_dir(tmp.path().join(".audiograph").join("derived"))
        .expect("read derived")
        .flatten()
        .map(|e| e.path())
        .collect();

    assert_eq!(before.len(), after.len(), "the report removed a file");
    assert!(orphan.exists(), "the report deleted the orphan");
}
