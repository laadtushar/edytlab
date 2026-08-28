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

use std::path::{Path, PathBuf};

use hound::{SampleFormat, WavSpec, WavWriter};
use serde_json::{json, Value};
use tempfile::TempDir;
use tools::{ToolContext, ToolDispatcher, ToolResult};

const SAMPLE_RATE: u32 = 48_000;

fn write_sine(path: &Path, freq: f32) -> PathBuf {
    let spec = WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut w = WavWriter::create(path, spec).expect("wav writer");
    for n in 0..SAMPLE_RATE as usize {
        let t = n as f32 / SAMPLE_RATE as f32;
        let s = (2.0 * std::f32::consts::PI * freq * t).sin() * 0.25;
        w.write_sample((s * 32_767.0) as i16).unwrap();
    }
    w.finalize().unwrap();
    path.to_path_buf()
}

struct Session {
    dir: TempDir,
    store: session::Store,
    engine: audio_engine::Engine,
    dispatcher: ToolDispatcher,
    clipboard: Option<tools::Clipboard>,
}

impl Session {
    fn new() -> Self {
        let dir = TempDir::new().expect("tempdir");
        let store = session::Store::open(dir.path()).expect("open store");
        Self {
            dir,
            store,
            engine: audio_engine::Engine::new(),
            dispatcher: ToolDispatcher::default_dispatcher(),
            clipboard: None,
        }
    }

    fn call(&mut self, tool: &str, args: Value) -> ToolResult {
        let mut ctx = ToolContext {
            store: &mut self.store,
            engine: &mut self.engine,
            user_message: "",
            clipboard: &mut self.clipboard,
            allowed_tools: None,
        };
        self.dispatcher.invoke(tool, args, &mut ctx).unwrap()
    }
}

fn ok(r: ToolResult) -> Value {
    match r {
        ToolResult::Ok(v) => v,
        ToolResult::Error(m) => panic!("expected Ok, got Error({m})"),
    }
}

/// A recipe that loads a file and quietens it, exported from a throwaway
/// session — the same way a user would make one.
fn make_recipe(s: &mut Session) -> PathBuf {
    let src = write_sine(&s.dir.path().join("template.wav"), 440.0);
    ok(s.call("load", json!({ "path": src.to_string_lossy() })));
    ok(s.call("gain", json!({ "track": 0, "db": -6.0 })));
    let recipe = s.dir.path().join("chain.json");
    ok(s.call(
        "export_recipe",
        json!({ "out_path": recipe.to_string_lossy(), "name": "quieten" }),
    ));
    recipe
}

/// A folder of three takes.
fn make_folder(root: &Path) -> PathBuf {
    let dir = root.join("takes");
    std::fs::create_dir_all(&dir).unwrap();
    for (i, freq) in [220.0, 440.0, 880.0].into_iter().enumerate() {
        write_sine(&dir.join(format!("take-{i}.wav")), freq);
    }
    // Something that is not audio, which must be ignored rather than
    // reported as a failure.
    std::fs::write(dir.join("notes.txt"), "not audio").unwrap();
    dir
}

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

    let mut s = Session::new();
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
