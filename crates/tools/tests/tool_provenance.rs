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
    write_sine_at(&dir.join(name), 440.0)
}

/// Write a sine of `freq` to `path`, replacing whatever was there.
/// Different frequency, different audio — which is the case a content
/// hash has to notice.
fn write_sine_at(path: &Path, freq: f32) -> std::path::PathBuf {
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

fn ok(result: ToolResult) -> Value {
    match result {
        ToolResult::Ok(v) => v,
        ToolResult::Error(msg) => panic!("expected Ok, got Error({msg})"),
    }
}

struct Session {
    dir: TempDir,
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
        dir: tmp,
    }
}

impl Session {
    fn call(&mut self, tool: &str, args: Value) -> Value {
        let mut ctx = ToolContext {
            store: &mut self.store,
            engine: &mut self.engine,
            user_message: "",
            clipboard: &mut self.clipboard,
            allowed_tools: None,
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

/// `load` reads a file from somewhere else on disk. It used to be
/// unreplayable for that reason, which made *every* chain unreplayable
/// at its root. #163 closes it over what it read: the content hash of
/// the audio imported, so a replay can check the source is still the
/// same audio rather than trusting a path.
#[test]
fn loading_a_file_records_the_audio_it_imported() {
    let mut s = session();
    s.load();
    let op = s.head_op().expect("load records an op");
    assert_eq!(op.tool, "load");
    assert!(
        op.reproducible,
        "load closes over its source now, so a chain is replayable at its root"
    );

    let hash = op.inputs["source"]["audio_hash"]
        .as_str()
        .expect("the imported audio is pinned by content");
    assert_eq!(hash.len(), 64, "a blake3 hex digest");
    assert!(
        op.inputs["source"]["path"].as_str().is_some(),
        "the path is recorded too — it is how a replay finds the file again"
    );
}

/// Pinning by content and not by path is the point: a source that has
/// changed since it was imported must be refused by name, because
/// replaying against different audio would produce output that is
/// quietly not what was recorded.
#[test]
fn a_changed_source_is_refused_and_named() {
    let mut s = session();
    s.load();
    let head = s.store.head().expect("head");
    assert!(
        tools::verify_chain(&s.store, head).is_empty(),
        "a freshly loaded session verifies"
    );

    // Rewrite the source with different audio, same path.
    write_sine_at(&s.dir.path().join("in.wav"), 880.0);

    let problems = tools::verify_chain(&s.store, head);
    let reasons: Vec<String> = problems.iter().map(|p| p.reason.clone()).collect();
    assert!(
        reasons
            .iter()
            .any(|r| r.contains("has changed") && r.contains("in.wav")),
        "expected a refusal naming the file, got {reasons:?}"
    );
}

/// A source that is simply gone is a different refusal, and also names
/// the file — "cannot rebuild" without saying what is missing is not
/// actionable.
#[test]
fn a_missing_source_is_refused_and_named() {
    let mut s = session();
    s.load();
    let head = s.store.head().expect("head");
    std::fs::remove_file(s.dir.path().join("in.wav")).expect("remove source");

    let problems = tools::verify_chain(&s.store, head);
    assert!(
        problems
            .iter()
            .any(|p| p.reason.contains("gone") && p.reason.contains("in.wav")),
        "expected a refusal naming the missing file, got {problems:?}"
    );
}

/// The clipboard was the hardest hidden input: it lived in memory and
/// was never persisted, so after a paste the audio existed only inside
/// the derived file. `copy_region` now writes a CAS blob and the paste
/// references it.
#[test]
fn a_paste_closes_over_the_clipboard_it_spliced() {
    let mut s = session();
    s.load();
    s.call(
        "copy_region",
        json!({ "track": 0, "range": { "start_sec": 0.0, "end_sec": 0.2 } }),
    );
    s.call("paste_region", json!({ "track": 0, "at": 0.5 }));

    let op = s.head_op().expect("paste records an op");
    assert_eq!(op.tool, "paste_region");
    assert!(
        op.reproducible,
        "a paste whose clipboard is on disk is replayable"
    );
    let hash = op.inputs["clipboard"]
        .as_str()
        .expect("the pasted audio is named by content");

    // And the blob is really there — a reference to a file that does not
    // exist would be a worse lie than admitting the paste was opaque.
    let blob = s
        .dir
        .path()
        .join(".audiograph")
        .join("clipboard")
        .join(format!("{hash}.wav"));
    assert!(
        blob.is_file(),
        "clipboard blob missing at {}",
        blob.display()
    );
    assert!(
        tools::verify_chain(&s.store, s.store.head().unwrap()).is_empty(),
        "a paste with its blob present verifies"
    );
}

/// And if the blob is deleted, the chain says so rather than reporting
/// history it cannot actually rebuild.
#[test]
fn a_missing_clipboard_blob_is_reported() {
    let mut s = session();
    s.load();
    s.call(
        "copy_region",
        json!({ "track": 0, "range": { "start_sec": 0.0, "end_sec": 0.2 } }),
    );
    s.call("paste_region", json!({ "track": 0, "at": 0.5 }));

    let dir = s.dir.path().join(".audiograph").join("clipboard");
    for entry in std::fs::read_dir(&dir).expect("clipboard dir").flatten() {
        std::fs::remove_file(entry.path()).expect("remove blob");
    }

    let problems = tools::verify_chain(&s.store, s.store.head().unwrap());
    assert!(
        problems
            .iter()
            .any(|p| p.reason.contains("clipboard blob is missing")),
        "expected the missing blob to be reported, got {problems:?}"
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
        rebuildable, files,
        "every chain runs back through `load`, which now closes over the \
         file it read (#163) — so this history is rebuildable rather than \
         merely deletable, which is the number #98's decision turns on"
    );
    assert!(v["history"]["rebuildable_bytes"].is_number());
}
