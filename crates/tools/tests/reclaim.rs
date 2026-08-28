//! Reclaiming derived audio without losing work (#98).
//!
//! The ticket's warning is the thing to test against: *the code is easy
//! and the wrong policy silently loses work*. So these are mostly about
//! what the sweep refuses to touch, and about the one property that
//! makes deleting safe at all — that a swept file comes back with
//! exactly the bytes it had.

use std::path::{Path, PathBuf};

use hound::{SampleFormat, WavSpec, WavWriter};
use serde_json::{json, Value};
use tempfile::TempDir;
use tools::{ToolContext, ToolDispatcher, ToolResult};

const SR: u32 = 48_000;

fn write_tone(path: &Path, seconds: f64) -> PathBuf {
    let spec = WavSpec {
        channels: 1,
        sample_rate: SR,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut w = WavWriter::create(path, spec).expect("wav writer");
    for n in 0..((SR as f64 * seconds) as usize) {
        let t = n as f32 / SR as f32;
        w.write_sample(((2.0 * std::f32::consts::PI * 440.0 * t).sin() * 8000.0) as i16)
            .unwrap();
    }
    w.finalize().unwrap();
    path.to_path_buf()
}

struct Session {
    dir: TempDir,
    store: session::Store,
    engine: audio_engine::Engine,
    dispatcher: ToolDispatcher,
    clipboard: Option<Vec<f32>>,
}

impl Session {
    fn new() -> Self {
        let dir = TempDir::new().expect("tempdir");
        let src = write_tone(&dir.path().join("take.wav"), 4.0);
        let store = session::Store::open(dir.path()).expect("open store");
        let mut s = Self {
            dir,
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
            allowed_tools: None,
        };
        self.dispatcher.invoke(tool, args, &mut ctx).unwrap()
    }

    /// A few *destructive* edits, so `derived/` has history in it.
    ///
    /// `silence_region` rather than `gain`: gain is a track property and
    /// writes no audio at all, so a history made of it would leave the
    /// directory empty and these tests passing for the wrong reason.
    fn make_history(&mut self) {
        for start in [0.0, 1.0, 2.0, 3.0] {
            ok(self.call(
                "silence_region",
                json!({ "track": 0, "start_sec": start, "end_sec": start + 0.2 }),
            ));
        }
    }

    fn derived(&self) -> PathBuf {
        self.dir.path().join(".audiograph").join("derived")
    }

    fn derived_files(&self) -> Vec<PathBuf> {
        std::fs::read_dir(self.derived())
            .map(|d| {
                d.flatten()
                    .map(|e| e.path())
                    .filter(|p| p.is_file())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn derived_bytes(&self) -> u64 {
        self.derived_files()
            .iter()
            .filter_map(|p| std::fs::metadata(p).ok())
            .map(|m| m.len())
            .sum()
    }

    fn head_paths(&self) -> Vec<PathBuf> {
        let head = self.store.head().expect("head");
        let node = self.store.get(head).expect("node");
        node.state
            .tracks
            .iter()
            .flat_map(|t| t.clips.iter().map(|c| c.source_path.clone()))
            .collect()
    }
}

fn ok(r: ToolResult) -> Value {
    match r {
        ToolResult::Ok(v) => v,
        ToolResult::Error(m) => panic!("expected Ok, got Error({m})"),
    }
}

/// Under the cap, nothing happens — so calling this after every edit
/// costs a directory listing and no deletions.
#[test]
fn a_directory_under_the_cap_is_left_alone() {
    let mut s = Session::new();
    s.make_history();

    let before = s.derived_files().len();
    let report = tools::reclaim::sweep(&s.store, u64::MAX).expect("sweep");

    assert_eq!(report.removed_files, 0);
    assert_eq!(report.freed_bytes, 0);
    assert_eq!(s.derived_files().len(), before);
}

/// The rule everything else rests on: whatever the cap, the session you
/// are looking at keeps working.
#[test]
fn the_files_the_head_names_are_never_swept() {
    let mut s = Session::new();
    s.make_history();

    // A cap of zero asks for everything to go.
    tools::reclaim::sweep(&s.store, 0).expect("sweep");

    for path in s.head_paths() {
        assert!(
            path.is_file(),
            "the head's audio must survive any sweep: {}",
            path.display()
        );
    }
}

/// A sweep frees measurably — the acceptance criterion, stated as a
/// number rather than as "some files went".
#[test]
fn a_session_with_history_shrinks_measurably() {
    let mut s = Session::new();
    s.make_history();

    let before = s.derived_bytes();
    assert!(before > 0, "there is history to reclaim");

    let report = tools::reclaim::sweep(&s.store, 0).expect("sweep");
    let after = s.derived_bytes();

    assert!(
        report.removed_files > 0,
        "something was reclaimed: {report:?}"
    );
    assert!(after < before, "{after} should be less than {before}");
    assert_eq!(before - after, report.freed_bytes, "the report is honest");
}

/// **The property that makes deleting safe.** A swept file comes back
/// with exactly the bytes it had — the CAS name is the hash of the
/// samples, so anything else would land under a different name.
#[test]
fn a_swept_file_comes_back_byte_identical() {
    let mut s = Session::new();
    s.make_history();
    let head = s.store.head().expect("head");

    // Pick a history file — one not named by the head — and remember it.
    let head_paths = s.head_paths();
    let victim = s
        .derived_files()
        .into_iter()
        .find(|p| !head_paths.contains(p))
        .expect("some history to sweep");
    let original = std::fs::read(&victim).expect("read before");

    std::fs::remove_file(&victim).expect("remove");
    assert!(!victim.is_file());

    // The node that names it is an ancestor of the head; rebuilding the
    // head's chain regenerates every file along it.
    tools::rederive::ensure_present(&s.store, head, &victim).expect("rebuild");

    let rebuilt = std::fs::read(&victim).expect("read after");
    assert_eq!(
        original.len(),
        rebuilt.len(),
        "a rebuilt file must be the same size"
    );
    assert!(original == rebuilt, "and byte-for-byte the same audio");
}

/// Rebuilding something that is already there is a no-op, so callers
/// can use it as "make sure this exists" without checking first.
#[test]
fn rebuilding_a_file_that_is_present_does_nothing() {
    let mut s = Session::new();
    s.make_history();
    let head = s.store.head().expect("head");
    let path = s.head_paths().into_iter().next().expect("a clip");
    let before = std::fs::read(&path).unwrap();

    assert!(tools::rederive::ensure_present(&s.store, head, &path).expect("present"));
    assert_eq!(std::fs::read(&path).unwrap(), before);
}

/// A file a node names but nothing can rebuild is kept, however far
/// over the cap the directory is. Deleting it is the one action that
/// would actually lose work.
#[test]
fn a_referenced_file_with_no_way_back_is_kept() {
    let mut s = Session::new();
    s.make_history();

    // A derived file that only an op-less node names. That is the shape
    // a pre-provenance session has, and the shape a chain containing an
    // ML step has: something is there, and nothing says how to remake it.
    let no_way_back = s.derived().join("deadbeef.wav");
    std::fs::write(&no_way_back, vec![0u8; 64_000]).unwrap();

    let head = s.store.head().expect("head");
    let mut state = s.store.get(head).expect("node").state;
    state.tracks[0].clips[0].source_path = no_way_back.clone();
    s.store
        .append(session::SessionNode {
            id: session::NodeId([0u8; 32]),
            parent: None,
            created_at: chrono::Utc::now(),
            label: Some("no op recorded".into()),
            reasoning: None,
            state,
            op: None,
        })
        .expect("append");
    // Appending moved the head onto that node; put it back so the file
    // is history rather than protected-as-live.
    s.store.set_head(head).expect("restore head");

    let report = tools::reclaim::sweep(&s.store, 0).expect("sweep");
    assert!(
        no_way_back.is_file(),
        "a file with no way back must survive: {report:?}"
    );
    assert!(
        report.kept_unrebuildable > 0,
        "and the sweep must say why it could not reach the cap: {report:?}"
    );
}

/// A file in `derived/` that *no* node names is unreachable for good —
/// states are immutable and content-addressed, so nothing can come to
/// reference it later. That is the one category worth deleting outright,
/// and it goes before anything regenerable is touched.
#[test]
fn an_orphan_is_freed_before_anything_regenerable() {
    let mut s = Session::new();
    s.make_history();

    let orphan = s.derived().join("00000000-orphan.wav");
    std::fs::write(&orphan, vec![0u8; 128_000]).unwrap();

    // A cap that only needs the orphan's worth of space freed.
    let cap = s.derived_bytes() - 100_000;
    let report = tools::reclaim::sweep(&s.store, cap).expect("sweep");

    assert!(!orphan.is_file(), "the orphan goes: {report:?}");
    assert_eq!(
        report.removed_orphans, 1,
        "and is counted as one: {report:?}"
    );
    assert_eq!(
        report.removed_files, 1,
        "nothing regenerable was touched to get under the cap: {report:?}"
    );
}

/// A project with no derived directory at all is not an error — a
/// session that has only loaded audio has nothing to reclaim.
#[test]
fn a_project_with_nothing_derived_is_fine() {
    let s = Session::new();
    let report = tools::reclaim::sweep(&s.store, 0).expect("sweep");
    assert_eq!(report.removed_files, 0);
}

// ─── compact_session — the deliberate half (#98 option b) ─────────────

/// It reports and changes nothing until asked twice. A destructive
/// sweep across a whole session is not something to discover afterwards.
#[test]
fn compaction_is_a_dry_run_by_default() {
    let mut s = Session::new();
    for _ in 0..4 {
        s.make_history();
    }

    let nodes_before = s.store.list_nodes().unwrap().len();
    let bytes_before = s.derived_bytes();

    let v = ok(s.call("compact_session", json!({ "keep_last": 2 })));

    assert_eq!(v["applied"], json!(false), "{v}");
    assert!(v["prunable_nodes"].as_u64().unwrap_or(0) > 0, "{v}");
    assert!(
        v["summary"]
            .as_str()
            .unwrap_or_default()
            .contains("apply: true"),
        "it has to say how to actually do it: {v}"
    );
    assert_eq!(
        s.store.list_nodes().unwrap().len(),
        nodes_before,
        "a dry run removes no history"
    );
    assert_eq!(s.derived_bytes(), bytes_before, "and no audio");
}

/// Applied, it removes history and the audio only that history named.
#[test]
fn compaction_removes_history_and_its_audio() {
    let mut s = Session::new();
    for _ in 0..4 {
        s.make_history();
    }

    let nodes_before = s.store.list_nodes().unwrap().len();
    let bytes_before = s.derived_bytes();

    let v = ok(s.call("compact_session", json!({ "keep_last": 2, "apply": true })));

    assert_eq!(v["applied"], json!(true), "{v}");
    assert!(v["removed_nodes"].as_u64().unwrap_or(0) > 0, "{v}");
    assert!(
        s.store.list_nodes().unwrap().len() < nodes_before,
        "history is shorter"
    );
    assert!(s.derived_bytes() < bytes_before, "and the disk is smaller");
}

/// **The line that must not be crossed.** Whatever is pruned, the
/// session you are looking at still opens and still plays.
#[test]
fn compaction_never_breaks_the_current_head() {
    let mut s = Session::new();
    for _ in 0..4 {
        s.make_history();
    }
    let head = s.store.head().expect("head");

    ok(s.call("compact_session", json!({ "keep_last": 1, "apply": true })));

    let node = s.store.get(head).expect("the head node still reads back");
    for track in &node.state.tracks {
        for clip in &track.clips {
            assert!(
                clip.source_path.is_file(),
                "the head's audio must survive compaction: {}",
                clip.source_path.display()
            );
        }
    }
}

/// Recent undo keeps working, **and the pruned boundary reads as the
/// beginning of history rather than as a corrupt store.**
///
/// Without cutting the oldest survivor's parent link, walking back hits
/// "node not found" — indistinguishable from damage.
#[test]
fn the_kept_history_walks_back_and_then_simply_ends() {
    let mut s = Session::new();
    for _ in 0..4 {
        s.make_history();
    }

    ok(s.call("compact_session", json!({ "keep_last": 3, "apply": true })));

    // Walk back as undo would. Every step must resolve, and the walk
    // must terminate by running out of parents — never by failing.
    let mut cursor = s.store.head();
    let mut steps = 0;
    while let Some(id) = cursor {
        let node = s
            .store
            .get(id)
            .unwrap_or_else(|e| panic!("undo hit a gap after {steps} step(s): {e}"));
        steps += 1;
        cursor = node.parent;
    }

    assert_eq!(steps, 3, "exactly the three kept nodes, then a clean end");
}

/// A session already within the limit says so rather than reporting a
/// zero-byte success.
#[test]
fn a_short_session_has_nothing_to_compact() {
    let mut s = Session::new();
    s.make_history();

    let v = ok(s.call("compact_session", json!({ "keep_last": 100 })));
    assert_eq!(v["prunable_nodes"], json!(0), "{v}");
    assert!(
        v["summary"]
            .as_str()
            .unwrap_or_default()
            .contains("Nothing to compact"),
        "{v}"
    );
}
