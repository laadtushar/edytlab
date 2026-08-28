//! `render_preview` reuses a render instead of redoing it (#164).
//!
//! The claim worth testing is not "a file appears in a directory" — it
//! is that the second call does no work and hands back *the same
//! bytes*. Determinism is what makes the cache safe (a node id is a
//! hash of the session state, and rendering that state is
//! reproducible), so a test that only checked the path would pass even
//! if the cached file were something else entirely.
//!
//! The other half is the miss: change the session and the node id
//! changes, so there is no invalidation step that could be forgotten.

use std::path::{Path, PathBuf};

use hound::{SampleFormat, WavSpec, WavWriter};
use serde_json::{json, Value};
use tempfile::TempDir;
use tools::{ToolContext, ToolDispatcher, ToolResult};

const SAMPLE_RATE: u32 = 48_000;

fn write_sine(dir: &Path, name: &str) -> PathBuf {
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

struct Harness {
    _tmp: TempDir,
    project: PathBuf,
    store: session::Store,
    engine: audio_engine::Engine,
    dispatcher: ToolDispatcher,
    clipboard: Option<Vec<f32>>,
}

impl Harness {
    fn new() -> Self {
        let tmp = TempDir::new().expect("tempdir");
        let project = tmp.path().to_path_buf();
        let src = write_sine(&project, "in.wav");
        let store = session::Store::open(&project).expect("open store");
        let mut h = Self {
            _tmp: tmp,
            project,
            store,
            engine: audio_engine::Engine::new(),
            dispatcher: ToolDispatcher::default_dispatcher(),
            clipboard: None,
        };
        h.call("load", json!({ "path": src.to_string_lossy() }));
        h
    }

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

    fn head(&self) -> String {
        self.store.head().expect("a head").to_hex()
    }

    fn preview(&mut self) -> Value {
        let node = self.head();
        self.call("render_preview", json!({ "node_id": node }))
    }
}

/// Rendering the same head twice must do no work the second time, and
/// the file it hands back must be the same bytes — not merely a file
/// with the same name.
#[test]
fn the_second_render_of_a_head_is_a_cache_hit() {
    let mut h = Harness::new();

    let first = h.preview();
    assert_eq!(
        first["cached"],
        json!(false),
        "first render should be a miss"
    );
    let path = PathBuf::from(first["path"].as_str().unwrap());
    let bytes = std::fs::read(&path).expect("the rendered preview");
    assert!(!bytes.is_empty());

    let second = h.preview();
    assert_eq!(second["cached"], json!(true), "second render should hit");
    assert_eq!(second["path"], first["path"], "the hit named another file");
    assert_eq!(
        std::fs::read(&path).expect("still there"),
        bytes,
        "a cached preview must be byte-identical to the render it replaces",
    );

    // And the numbers a hit reports come from the file, so they match.
    assert_eq!(second["frames_written"], first["frames_written"]);
    assert_eq!(second["sample_rate"], first["sample_rate"]);
    assert_eq!(second["channels"], first["channels"]);
}

/// Editing changes the session state, which changes the node id, which
/// misses. There is no invalidation step to get wrong.
#[test]
fn an_edit_misses_and_leaves_the_old_entry_intact() {
    let mut h = Harness::new();

    let before = h.preview();
    let before_path = PathBuf::from(before["path"].as_str().unwrap());

    h.call(
        "silence_region",
        json!({ "track": 0, "start_sec": 0.0, "end_sec": 0.2 }),
    );

    let after = h.preview();
    assert_eq!(after["cached"], json!(false), "a new head must not hit");
    assert_ne!(after["path"], before["path"]);

    // Undo means going back, so the previous render has to still be
    // there — that is the case the cache exists for.
    assert!(
        before_path.is_file(),
        "the previous head's preview was thrown away"
    );
}

/// Stepping back to an earlier head plays instantly: the entry from
/// before the edit is still a hit.
#[test]
fn undo_then_redo_hits_in_both_directions() {
    let mut h = Harness::new();

    let first_head = h.head();
    h.preview();

    h.call(
        "silence_region",
        json!({ "track": 0, "start_sec": 0.0, "end_sec": 0.2 }),
    );
    let second_head = h.head();
    h.preview();

    let back = h.call("render_preview", json!({ "node_id": first_head }));
    assert_eq!(back["cached"], json!(true), "undo re-rendered");

    let forward = h.call("render_preview", json!({ "node_id": second_head }));
    assert_eq!(forward["cached"], json!(true), "redo re-rendered");
}

/// A ranged render is an excerpt, not the mix, and must never be stored
/// under the node's name — serving one as a whole-session preview would
/// be a cache hit that returns the wrong audio.
#[test]
fn a_ranged_render_is_not_cached_as_the_whole_mix() {
    let mut h = Harness::new();
    let node = h.head();

    let excerpt = h.call(
        "render_preview",
        json!({ "node_id": node, "range": [0, 1000] }),
    );
    let excerpt_path = PathBuf::from(excerpt["path"].as_str().unwrap());
    let cache_dir = h.project.join(".audiograph").join("previews");
    assert!(
        !excerpt_path.starts_with(&cache_dir),
        "a ranged render was written into the preview cache: {}",
        excerpt_path.display(),
    );

    // The full render that follows is a genuine miss rather than a hit
    // on the excerpt.
    let full = h.preview();
    assert_eq!(full["cached"], json!(false));
    assert!(
        full["frames_written"].as_u64().unwrap() > 1000,
        "the whole-session render returned an excerpt",
    );
}

/// `storage_report` has to be able to see the cache, or a bounded cache
/// is one nobody can observe working.
#[test]
fn storage_report_counts_the_preview_cache() {
    let mut h = Harness::new();

    let before = h.call("storage_report", json!({}));
    assert_eq!(before["preview_cache"]["files"], json!(0));

    h.preview();

    let after = h.call("storage_report", json!({}));
    assert_eq!(after["preview_cache"]["files"], json!(1));
    assert!(after["preview_cache"]["bytes"].as_u64().unwrap() > 0);
    assert!(after["preview_cache"]["cap_bytes"].as_u64().unwrap() > 0);
    // The cache is not derived edit history and must not be counted as
    // such — that number drives #98's policy decision.
    assert_eq!(after["history"]["files"], before["history"]["files"]);
}
