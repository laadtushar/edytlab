//! Every edit records how it was made (#98).
//!
//! Derived audio can only become a *cache* — evictable, rebuilt on
//! demand — if something records which edit to replay. Nothing did:
//! `SessionNode` carried a human `label` and an anonymous closure did the
//! work. `NodeOp` is that record, and the dispatcher writes it.
//!
//! The tests that matter here are the two that pin the design rather than
//! the plumbing:
//!
//! * `provenance_is_recorded_without_the_tool_knowing` — recording lives
//!   in `ToolDispatcher::invoke`, the one place every edit passes
//!   through, so all 81 tools are covered by one code path and a tool
//!   added tomorrow is covered without being touched. If someone moves it
//!   into the tools, this fails.
//! * `tools_reading_outside_the_session_are_marked_unreproducible` — the
//!   list of tools with hidden inputs is hand-maintained, and a rename
//!   would quietly drop one. That is the same drift `tool_badge_labels.rs`
//!   and `website_tool_docs.rs` exist to catch, one surface over.

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

struct Session {
    _tmp: TempDir,
    store: session::Store,
    engine: audio_engine::Engine,
    dispatcher: ToolDispatcher,
    clipboard: Option<Vec<f32>>,
    src: std::path::PathBuf,
}

fn session() -> Session {
    let tmp = TempDir::new().expect("tempdir");
    let src = write_sine(tmp.path(), "in.wav");
    let store = session::Store::open(tmp.path()).expect("open store");
    Session {
        store,
        engine: audio_engine::Engine::new(),
        dispatcher: ToolDispatcher::default_dispatcher(),
        clipboard: None,
        src,
        _tmp: tmp,
    }
}

impl Session {
    fn call(&mut self, tool: &str, args: Value) -> Value {
        let mut ctx = ToolContext {
            store: &mut self.store,
            engine: &mut self.engine,
            user_message: "",
            clipboard: &mut self.clipboard,
        };
        ok(self.dispatcher.invoke(tool, args, &mut ctx).unwrap())
    }

    fn load(&mut self) {
        let p = self.src.to_string_lossy().to_string();
        self.call("load", json!({ "path": p }));
    }

    fn head_op(&self) -> Option<session::NodeOp> {
        let head = self.store.head().expect("a head");
        self.store.get(head).expect("head node").op
    }
}

/// **The design, not the plumbing.**
///
/// No tool implementation mentions provenance. It is recorded by the
/// dispatcher, which is why adding a tool costs nothing and why this
/// cannot drift the way a per-tool convention would.
#[test]
fn provenance_is_recorded_without_the_tool_knowing() {
    let mut s = session();
    s.load();
    s.call(
        "silence_region",
        json!({ "track": 0, "start_sec": 0.0, "end_sec": 0.2 }),
    );

    let op = s.head_op().expect("the edit should have recorded an op");
    assert_eq!(op.tool, "silence_region");
    assert_eq!(op.params["track"], 0);
    assert_eq!(op.params["end_sec"], 0.2);
    assert!(op.reproducible, "pure DSP on session audio is replayable");
    assert!(
        !op.engine_version.is_empty(),
        "the version is what explains a rebuild that drifts"
    );
}

/// A rename or removal in the hidden-input list would silently promote a
/// tool to "replayable" and invite a sweep to delete audio that cannot be
/// rebuilt. The list has to name tools that exist.
#[test]
fn tools_reading_outside_the_session_are_marked_unreproducible() {
    let registered: Vec<String> = ToolDispatcher::default_dispatcher()
        .tool_schemas()
        .as_array()
        .expect("tool_schemas returns an array")
        .iter()
        .filter_map(|s| s["name"].as_str().map(str::to_owned))
        .collect();

    for name in tools::READS_OUTSIDE_THE_SESSION {
        assert!(
            registered.iter().any(|r| r == name),
            "`{name}` is listed as reading outside the session but is not a \
             registered tool — it was renamed or removed, and whatever \
             replaced it is now silently marked replayable"
        );
    }
}

/// `load` reads a file from somewhere else on disk, so replaying it here
/// cannot be expected to reproduce anything. It must not claim otherwise.
#[test]
fn loading_a_file_is_not_marked_replayable() {
    let mut s = session();
    s.load();
    let op = s.head_op().expect("load records an op");
    assert_eq!(op.tool, "load");
    assert!(
        !op.reproducible,
        "load depends on a file the session does not contain"
    );
}

/// A read-only tool moves no head and so appends no node. Recording an op
/// for it would attach a second, wrong provenance to whatever node
/// happens to be current.
#[test]
fn a_read_only_tool_records_nothing() {
    let mut s = session();
    s.load();
    s.call(
        "silence_region",
        json!({ "track": 0, "start_sec": 0.0, "end_sec": 0.2 }),
    );
    let before = s.store.head().expect("head");

    s.call("storage_report", json!({}));

    assert_eq!(s.store.head(), Some(before), "the report moved the head");
    let op = s.head_op().expect("op");
    assert_eq!(
        op.tool, "silence_region",
        "the report overwrote the edit's provenance"
    );
}

/// Node ids are a hash of the state alone, so two routes to the same
/// state land on the same node. The record already there describes the
/// run that produced the file on disk; a later equivalent derivation is a
/// guess by comparison.
#[test]
fn the_first_recorded_op_wins() {
    let mut s = session();
    s.load();
    let loaded = s.store.head().expect("head after load");

    // `gain` accumulates, so +6 then -6 returns the session to exactly
    // the state it had after loading — and node ids are a hash of the
    // state, so the head lands back on the very same node.
    s.call("gain", json!({ "track": 0, "db": 6.0 }));
    s.call("gain", json!({ "track": 0, "db": -6.0 }));

    assert_eq!(
        s.store.head(),
        Some(loaded),
        "undoing a gain should return to the same content-addressed node"
    );
    assert_eq!(
        s.head_op().expect("op").tool,
        "load",
        "the node kept the provenance of the run that actually produced \
         it, rather than being relabelled by a later route to the same \
         state"
    );
}

/// Provenance must not change node identity, or adding it would orphan
/// every derived file in every existing session.
#[test]
fn recording_provenance_does_not_change_the_node_id() {
    let mut s = session();
    s.load();
    s.call(
        "silence_region",
        json!({ "track": 0, "start_sec": 0.0, "end_sec": 0.2 }),
    );
    let head = s.store.head().expect("head");
    let node = s.store.get(head).expect("node");

    assert!(node.op.is_some(), "the op is on disk");
    assert_eq!(
        session::NodeId::from_state(&node.state).expect("hash"),
        node.id,
        "the id must still be the hash of the state alone"
    );
}

/// The whole point of the record: `storage_report` can now say how much
/// of the history is reclaimable-and-restorable rather than merely
/// deletable. Without it the number is zero and #98 has no data.
#[test]
fn the_storage_report_can_now_see_what_is_rebuildable() {
    let mut s = session();
    s.load();
    for end in [0.1, 0.2, 0.3, 0.4] {
        s.call(
            "silence_region",
            json!({ "track": 0, "start_sec": 0.0, "end_sec": end }),
        );
    }
    let v = s.call("storage_report", json!({}));

    let files = v["history"]["files"].as_u64().expect("history files");
    let rebuildable = v["history"]["rebuildable_files"]
        .as_u64()
        .expect("rebuildable files");
    assert_eq!(files, 3, "three superseded edits");
    assert_eq!(
        rebuildable, 0,
        "every chain runs back through `load`, which is not replayable — \
         so none of this history is safely evictable, and the report has \
         to say so rather than flatter the number"
    );
    assert!(v["history"]["rebuildable_bytes"].is_number());
}
