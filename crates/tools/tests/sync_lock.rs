//! Sync-lock: an edit to one track is an edit to the timeline (#170 §3).
//!
//! The case this exists for is an interview recorded one track per
//! speaker. Cutting a sentence out of one of them leaves every later
//! word on that track early by the length of the cut while the other
//! speaker stays put, and the conversation comes apart — silently, with
//! nothing about the edit saying it will.
//!
//! So the tests are about alignment, not about clip arithmetic: after
//! the edit, does a thing that was simultaneous stay simultaneous?

use std::path::{Path, PathBuf};

use hound::{SampleFormat, WavSpec, WavWriter};
use serde_json::{json, Value};
use tempfile::TempDir;
use tools::{ToolContext, ToolDispatcher, ToolResult};

const SAMPLE_RATE: u32 = 48_000;

fn write_sine(path: &Path, seconds: usize) -> PathBuf {
    let spec = WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut w = WavWriter::create(path, spec).expect("wav writer");
    for n in 0..(SAMPLE_RATE as usize * seconds) {
        let t = n as f32 / SAMPLE_RATE as f32;
        let s = (2.0 * std::f32::consts::PI * 440.0 * t).sin() * 0.25;
        w.write_sample((s * 32_767.0) as i16).unwrap();
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
    /// Two speakers, ten seconds each, recorded together.
    fn new() -> Self {
        let dir = TempDir::new().expect("tempdir");
        let a = write_sine(&dir.path().join("host.wav"), 10);
        let b = write_sine(&dir.path().join("guest.wav"), 10);
        let store = session::Store::open(dir.path()).expect("open store");
        let mut s = Self {
            _dir: dir,
            store,
            engine: audio_engine::Engine::new(),
            dispatcher: ToolDispatcher::default_dispatcher(),
            clipboard: None,
        };
        s.call("load", json!({ "path": a.to_string_lossy() }));
        s.call("load", json!({ "path": b.to_string_lossy() }));
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

    fn state(&self) -> session::SessionState {
        let head = self.store.head().expect("a head");
        self.store.get(head).expect("head node").state
    }

    /// Where a track's timeline ends, in samples.
    fn track_len(&self, track: usize) -> u64 {
        self.state().tracks[track]
            .clips
            .iter()
            .map(|c| c.start_in_track + c.length)
            .max()
            .unwrap_or(0)
    }

    fn node_count(&self) -> usize {
        self.store.list_nodes().expect("nodes").len()
    }
}

fn ok(r: ToolResult) -> Value {
    match r {
        ToolResult::Ok(v) => v,
        ToolResult::Error(m) => panic!("expected Ok, got Error({m})"),
    }
}

/// Off by default, and the old behaviour is exactly what it was.
#[test]
fn without_sync_lock_a_cut_touches_only_its_own_track() {
    let mut s = Session::new();
    assert!(!s.state().sync_lock, "off unless asked for");

    let before = s.track_len(1);
    ok(s.call(
        "cut_range",
        json!({ "track": 0, "start_sample": 0, "end_sample": 48_000 }),
    ));

    assert_eq!(s.track_len(1), before, "the other track must not move");
    assert_eq!(s.track_len(0), before - 48_000, "and this one shortens");
}

/// The interview case: a cut on one speaker takes the same span from
/// the other, so what was simultaneous stays simultaneous.
#[test]
fn with_sync_lock_a_cut_shortens_every_track() {
    let mut s = Session::new();
    ok(s.call("set_sync_lock", json!({ "enabled": true })));

    let before = s.track_len(1);
    let v = ok(s.call(
        "cut_range",
        json!({ "track": 0, "start_sample": 96_000, "end_sample": 144_000 }),
    ));
    assert_eq!(v["synced_tracks"], json!(1), "{v}");

    assert_eq!(s.track_len(0), before - 48_000);
    assert_eq!(
        s.track_len(1),
        before - 48_000,
        "the second speaker loses the same second, or the two drift apart"
    );
}

/// Inserting has to open the gap everywhere for the same reason.
#[test]
fn with_sync_lock_an_insert_lengthens_every_track() {
    let mut s = Session::new();
    ok(s.call("set_sync_lock", json!({ "enabled": true })));

    let before = s.track_len(1);
    ok(s.call(
        "insert_silence",
        json!({ "track": 0, "at": 2.0, "duration": 1.0 }),
    ));

    assert_eq!(s.track_len(0), before + 48_000);
    assert_eq!(s.track_len(1), before + 48_000, "the gap opens on both");
}

/// An insert in the middle splits the other track's clip and moves the
/// tail, rather than moving the whole thing — the audio before the
/// insert point has not moved and must not.
#[test]
fn an_insert_moves_only_what_comes_after_it() {
    let mut s = Session::new();
    ok(s.call("set_sync_lock", json!({ "enabled": true })));
    ok(s.call(
        "insert_silence",
        json!({ "track": 0, "at": 4.0, "duration": 2.0 }),
    ));

    let other = &s.state().tracks[1];
    assert_eq!(other.clips.len(), 2, "the clip is split at the gap");
    assert_eq!(other.clips[0].start_in_track, 0, "the head has not moved");
    assert_eq!(other.clips[0].length, 4 * 48_000);
    assert_eq!(
        other.clips[1].start_in_track,
        6 * 48_000,
        "the tail starts after the two-second gap"
    );
    assert_eq!(
        other.clips[1].source_offset,
        4 * 48_000,
        "and plays from where it left off, not from the beginning",
    );
}

/// "One undoable node, not one per track" — the acceptance says so, and
/// it is the difference between undo restoring the edit and undo
/// restoring half of it.
#[test]
fn a_sync_locked_edit_is_a_single_node() {
    let mut s = Session::new();
    ok(s.call("set_sync_lock", json!({ "enabled": true })));

    let before = s.node_count();
    ok(s.call(
        "cut_range",
        json!({ "track": 0, "start_sample": 0, "end_sample": 48_000 }),
    ));
    assert_eq!(s.node_count(), before + 1, "one node, both tracks");
}

/// Undo is the whole point of it being one node.
#[test]
fn undoing_a_sync_locked_cut_restores_both_tracks() {
    let mut s = Session::new();
    ok(s.call("set_sync_lock", json!({ "enabled": true })));
    let (a, b) = (s.track_len(0), s.track_len(1));

    let before_head = s.store.head().expect("head");
    ok(s.call(
        "cut_range",
        json!({ "track": 0, "start_sample": 0, "end_sample": 48_000 }),
    ));
    assert_ne!(s.track_len(0), a);

    s.store.set_head(before_head).expect("undo");
    assert_eq!(s.track_len(0), a, "the cut track is back");
    assert_eq!(s.track_len(1), b, "and so is the one it dragged along");
}

/// The mode travels with the session, so a project re-opened later
/// still edits the way it was left.
#[test]
fn the_mode_is_part_of_the_session() {
    let mut s = Session::new();
    ok(s.call("set_sync_lock", json!({ "enabled": true })));
    assert!(s.state().sync_lock);

    ok(s.call("set_sync_lock", json!({ "enabled": false })));
    assert!(!s.state().sync_lock);
}

/// Setting it to what it already is should not cost the user an undo
/// step that does nothing.
#[test]
fn setting_it_to_what_it_already_is_appends_nothing() {
    let mut s = Session::new();
    let before = s.node_count();
    let v = ok(s.call("set_sync_lock", json!({ "enabled": false })));
    assert_eq!(v["changed"], json!(false), "{v}");
    assert_eq!(s.node_count(), before, "no node for a no-op");
}
