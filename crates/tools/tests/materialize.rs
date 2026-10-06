//! Swept audio comes back before anything reads it (#98, #356).
//!
//! The history sweep deletes audio only older nodes name, once a replay
//! has proved it can come back. These check the other half: every tool
//! that renders a node or moves the head to one puts that node's audio
//! back first, through `rederive::materialize` — so a swept node plays,
//! renders and exports exactly as it did before the sweep.
//!
//! Each builds real history through the dispatcher, sweeps it with the
//! real sweep at cap 0, and then targets the head's parent, whose audio
//! the sweep removed.

use std::path::{Path, PathBuf};

use hound::{SampleFormat, WavSpec, WavWriter};
use serde_json::{json, Value};
use session::NodeId;
use tempfile::TempDir;
use tools::{ToolContext, ToolDispatcher, ToolResult};

const SR: u32 = 48_000;

fn write_tone(path: &Path, seconds: f64) {
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
}

struct Session {
    dir: TempDir,
    store: session::Store,
    engine: audio_engine::Engine,
    dispatcher: ToolDispatcher,
    clipboard: Option<tools::Clipboard>,
}

impl Session {
    /// A take and four destructive edits to it, so the history names
    /// derived audio the head does not.
    fn with_history() -> Self {
        let dir = TempDir::new().expect("tempdir");
        let take = dir.path().join("take.wav");
        write_tone(&take, 4.0);
        let store = session::Store::open(dir.path()).expect("open store");
        let mut s = Self {
            dir,
            store,
            engine: audio_engine::Engine::new(),
            dispatcher: ToolDispatcher::default_dispatcher(),
            clipboard: None,
        };
        ok(s.call("load", json!({ "path": take })));
        for start in [0.0, 1.0, 2.0, 3.0] {
            ok(s.call(
                "silence_region",
                json!({ "track": 0, "start_sec": start, "end_sec": start + 0.2 }),
            ));
        }
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
        self.dispatcher
            .invoke(tool, args, &mut ctx)
            .expect("invoke")
    }

    fn head(&self) -> NodeId {
        self.store.head().expect("head")
    }

    /// The head's parent: history, whose derived audio the head does not
    /// name.
    fn parent(&self) -> NodeId {
        self.store
            .get(self.head())
            .expect("head node")
            .parent
            .expect("the head has a parent")
    }

    /// What `node` names, with the bytes each file holds now.
    fn audio_of(&self, node: NodeId) -> Vec<(PathBuf, Vec<u8>)> {
        self.store
            .get(node)
            .expect("node")
            .state
            .tracks
            .iter()
            .flat_map(|t| t.clips.iter().map(|c| c.source_path.clone()))
            .map(|p| {
                let bytes = std::fs::read(&p).expect("read before the sweep");
                (p, bytes)
            })
            .collect()
    }

    /// Sweep everything the sweep may, and check it reached `node`.
    fn sweep_away(&self, node: NodeId) {
        tools::reclaim::sweep(&self.store, 0).expect("sweep");
        assert!(
            !tools::rederive::missing_paths(&self.store, node).is_empty(),
            "the sweep removed the node's audio, or this checks nothing"
        );
    }

    fn assert_back(&self, before: &[(PathBuf, Vec<u8>)]) {
        for (path, bytes) in before {
            let now = std::fs::read(path)
                .unwrap_or_else(|_| panic!("{} was not put back", path.display()));
            assert!(&now == bytes, "{} came back different", path.display());
        }
    }
}

fn ok(r: ToolResult) -> Value {
    match r {
        ToolResult::Ok(v) => v,
        ToolResult::Error(m) => panic!("expected Ok, got Error({m})"),
    }
}

#[test]
fn a_swept_node_comes_back_whole_and_byte_identical() {
    let s = Session::with_history();
    let parent = s.parent();
    let before = s.audio_of(parent);
    s.sweep_away(parent);

    let rebuilt = tools::rederive::materialize(&s.store, parent).expect("materialize");

    assert!(!rebuilt.is_empty(), "it says what it rebuilt");
    s.assert_back(&before);
    assert!(tools::rederive::missing_paths(&s.store, parent).is_empty());
}

#[test]
fn a_node_with_everything_on_disk_needs_nothing() {
    let s = Session::with_history();
    let head = s.head();
    let before = s.audio_of(head);

    assert_eq!(
        tools::rederive::materialize(&s.store, head).expect("materialize"),
        Vec::<PathBuf>::new()
    );
    s.assert_back(&before);
}

/// Only `derived/` is the app's to delete, and the only place a replay
/// writes. The user's own source going missing is theirs to fix, and
/// the read reports it as it always has.
#[test]
fn a_missing_source_outside_derived_is_left_to_the_read() {
    let s = Session::with_history();
    std::fs::rename(
        s.dir.path().join("take.wav"),
        s.dir.path().join("moved.wav"),
    )
    .expect("move the take");
    let loaded = s
        .store
        .list_nodes()
        .expect("nodes")
        .into_iter()
        .find(|n| n.parent.is_none())
        .expect("the load")
        .id;

    assert_eq!(
        tools::rederive::materialize(&s.store, loaded).expect("nothing of ours is missing"),
        Vec::<PathBuf>::new()
    );
}

/// A derived file nothing can rebuild is refused, by name, rather than
/// left for a render to play as silence.
#[test]
fn a_file_nothing_can_rebuild_is_refused_by_name() {
    let s = Session::with_history();
    let parent = s.parent();
    let (victim, _) = s
        .audio_of(parent)
        .into_iter()
        .find(|(p, _)| p.starts_with(s.dir.path().join(".audiograph")))
        .expect("derived audio");
    std::fs::remove_file(&victim).expect("remove");
    // Every chain starts by loading the take; without it nothing replays.
    std::fs::remove_file(s.dir.path().join("take.wav")).expect("remove the take");

    let err = tools::rederive::materialize(&s.store, parent).expect_err("cannot rebuild");
    let name = victim.file_name().unwrap().to_string_lossy().to_string();
    assert!(err.contains(&name), "the refusal names the file: {err}");
}

#[test]
fn revert_to_a_swept_node_puts_its_audio_back() {
    let mut s = Session::with_history();
    let parent = s.parent();
    let before = s.audio_of(parent);
    s.sweep_away(parent);

    ok(s.call("revert_to", json!({ "target": parent.to_hex() })));

    s.assert_back(&before);
}

#[test]
fn forking_at_a_swept_node_puts_its_audio_back() {
    let mut s = Session::with_history();
    let parent = s.parent();
    let before = s.audio_of(parent);
    s.sweep_away(parent);

    ok(s.call("fork_node", json!({ "from": parent.to_hex() })));

    assert_eq!(s.head(), parent, "the fork is the new head");
    s.assert_back(&before);
}

#[test]
fn a_diff_applied_to_a_swept_node_puts_its_audio_back() {
    let mut s = Session::with_history();
    let parent = s.parent();
    let before = s.audio_of(parent);
    s.sweep_away(parent);
    let track_id = s.store.get(parent).expect("node").state.tracks[0]
        .id
        .clone();
    let ops = serde_json::to_value(session::SessionDiff {
        modified: vec![(
            session::DiffOp::TrackGain {
                track_id: track_id.clone(),
                value: 0.0,
            },
            session::DiffOp::TrackGain {
                track_id,
                value: -3.0,
            },
        )],
        ..Default::default()
    })
    .expect("diff");

    ok(s.call(
        "apply_diff",
        json!({ "from_node": parent.to_hex(), "branches": [{ "ops": ops }] }),
    ));

    // The branch names the same audio as the node it came from.
    s.assert_back(&before);
}

#[test]
fn previewing_a_swept_node_puts_its_audio_back() {
    let mut s = Session::with_history();
    let parent = s.parent();
    let before = s.audio_of(parent);
    s.sweep_away(parent);

    ok(s.call("render_preview", json!({ "node_id": parent.to_hex() })));

    s.assert_back(&before);
}

#[test]
fn exporting_a_swept_node_puts_its_audio_back() {
    let mut s = Session::with_history();
    let parent = s.parent();
    let before = s.audio_of(parent);
    s.sweep_away(parent);
    let out = s.dir.path().join("export.wav");

    ok(s.call(
        "render_final",
        json!({ "node_id": parent.to_hex(), "format": "wav", "out_path": out }),
    ));

    s.assert_back(&before);
    assert!(out.is_file(), "and the export was written");
}

/// Refusing is the whole point: a revert to a node that cannot be
/// rebuilt must not leave the head on it.
#[test]
fn a_revert_that_cannot_rebuild_leaves_the_head_where_it_was() {
    let mut s = Session::with_history();
    let parent = s.parent();
    let head = s.head();
    s.sweep_away(parent);
    std::fs::remove_file(s.dir.path().join("take.wav")).expect("remove the take");

    match s.call("revert_to", json!({ "target": parent.to_hex() })) {
        ToolResult::Error(msg) => assert!(msg.contains("rebuild"), "says why: {msg}"),
        ToolResult::Ok(v) => panic!("reverted to audio that is gone: {v}"),
    }
    assert_eq!(s.head(), head);
}
