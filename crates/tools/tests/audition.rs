//! Hearing an effect before it becomes a node (#166).
//!
//! The claim is not that a file appears — it is that **nothing is
//! committed**. Choosing a reverb value used to cost one node per
//! guess: apply, listen, undo. So the tests check the session is
//! untouched, that the audition is genuinely different audio from the
//! dry session, and that accepting is the ordinary one-node operation
//! it always was.

use std::path::{Path, PathBuf};

use hound::{SampleFormat, WavSpec, WavWriter};
use serde_json::{json, Value};
use tempfile::TempDir;
use tools::{ToolContext, ToolDispatcher, ToolResult};

const SAMPLE_RATE: u32 = 48_000;

fn write_sine(path: &Path) -> PathBuf {
    let spec = WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut w = WavWriter::create(path, spec).expect("wav writer");
    for n in 0..(SAMPLE_RATE as usize * 2) {
        let t = n as f32 / SAMPLE_RATE as f32;
        // Two tones, so a filter has something to remove.
        let s = ((2.0 * std::f32::consts::PI * 300.0 * t).sin()
            + (2.0 * std::f32::consts::PI * 9000.0 * t).sin())
            * 0.2;
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
        let src = write_sine(&dir.path().join("in.wav"));
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

    fn head(&self) -> String {
        self.store.head().expect("a head").to_hex()
    }

    fn node_count(&self) -> usize {
        self.store.list_nodes().expect("list nodes").len()
    }
}

fn ok(r: ToolResult) -> Value {
    match r {
        ToolResult::Ok(v) => v,
        ToolResult::Error(m) => panic!("expected Ok, got Error({m})"),
    }
}

fn err(r: ToolResult) -> String {
    match r {
        ToolResult::Error(m) => m,
        ToolResult::Ok(v) => panic!("expected Error, got Ok({v})"),
    }
}

fn rms(path: &Path) -> f32 {
    let d = audio_decoder::decode_file(path).expect("decode");
    let sum: f32 = d.samples.iter().map(|s| s * s).sum();
    (sum / d.samples.len().max(1) as f32).sqrt()
}

/// **The point of the ticket.** An audition changes nothing: same head,
/// same node count, before and after.
#[test]
fn an_audition_commits_nothing() {
    let mut s = Session::new();
    let head_before = s.head();
    let nodes_before = s.node_count();

    let v = ok(s.call(
        "audition_effect",
        json!({ "track": 0, "kind": "low_pass_filter", "params": { "cutoff_hz": 1000 } }),
    ));

    assert!(PathBuf::from(v["path"].as_str().unwrap()).is_file());
    assert_eq!(s.head(), head_before, "the head must not move");
    assert_eq!(s.node_count(), nodes_before, "no node may be appended");
    assert!(
        v["summary"].as_str().unwrap().contains("add_effect"),
        "the result should say how to keep it: {v}"
    );
}

/// And it is really the effect being heard, not the dry session with a
/// new name. A 1 kHz low-pass on a signal that is half 9 kHz has to
/// come back quieter.
#[test]
fn the_audition_is_the_effect_applied() {
    let mut s = Session::new();

    let dry = ok(s.call(
        "audition_effect",
        json!({ "track": 0, "kind": "gain", "params": { "db": 0.0 }, "end_sec": 1.0 }),
    ));
    let wet = ok(s.call(
        "audition_effect",
        json!({
            "track": 0,
            "kind": "low_pass_filter",
            "params": { "cutoff_hz": 1000 },
            "end_sec": 1.0,
        }),
    ));

    let dry_rms = rms(Path::new(dry["path"].as_str().unwrap()));
    let wet_rms = rms(Path::new(wet["path"].as_str().unwrap()));
    assert!(
        wet_rms < dry_rms * 0.9,
        "a 1 kHz low-pass should audibly remove the 9 kHz half: dry {dry_rms}, wet {wet_rms}"
    );
}

/// Nudging a parameter back to something already heard is instant,
/// which is what makes auditioning usable on a slider.
#[test]
fn the_same_audition_twice_is_cached() {
    let mut s = Session::new();
    let args = json!({
        "track": 0,
        "kind": "low_pass_filter",
        "params": { "cutoff_hz": 2000 },
        "end_sec": 1.0,
    });

    let first = ok(s.call("audition_effect", args.clone()));
    assert_eq!(first["cached"], json!(false));
    let second = ok(s.call("audition_effect", args));
    assert_eq!(second["cached"], json!(true));
    assert_eq!(second["path"], first["path"]);
}

/// Different settings are different audio, so they must not share a
/// file — a cache hit across parameters would play the wrong thing.
#[test]
fn different_parameters_get_different_auditions() {
    let mut s = Session::new();
    let a = ok(s.call(
        "audition_effect",
        json!({ "track": 0, "kind": "low_pass_filter", "params": { "cutoff_hz": 1000 }, "end_sec": 1.0 }),
    ));
    let b = ok(s.call(
        "audition_effect",
        json!({ "track": 0, "kind": "low_pass_filter", "params": { "cutoff_hz": 4000 }, "end_sec": 1.0 }),
    ));
    assert_ne!(a["path"], b["path"]);
}

/// A different region of the same settings is different audio too.
#[test]
fn a_different_region_is_a_different_audition() {
    let mut s = Session::new();
    let effect = |start: f64, end: f64| {
        json!({
            "track": 0,
            "kind": "gain",
            "params": { "db": -3.0 },
            "start_sec": start,
            "end_sec": end,
        })
    };
    let a = ok(s.call("audition_effect", effect(0.0, 1.0)));
    let b = ok(s.call("audition_effect", effect(1.0, 2.0)));
    assert_ne!(a["path"], b["path"]);
}

/// Auditions are excerpts and must never be served as a whole mix
/// (#164's cache invariant, one directory over).
#[test]
fn auditions_are_kept_out_of_the_preview_cache() {
    let mut s = Session::new();
    ok(s.call(
        "audition_effect",
        json!({ "track": 0, "kind": "gain", "params": { "db": -3.0 } }),
    ));

    let store = s.dir.path().join(".audiograph");
    assert!(
        store.join("auditions").is_dir(),
        "auditions have their own home"
    );
    let previews = store.join("previews");
    let preview_files = std::fs::read_dir(&previews)
        .map(|d| d.flatten().count())
        .unwrap_or(0);
    assert_eq!(
        preview_files, 0,
        "an audition must not land in the preview cache"
    );
}

/// Accepting is `add_effect`: exactly one node, as applying it directly
/// always was.
#[test]
fn accepting_an_audition_is_one_ordinary_node() {
    let mut s = Session::new();
    ok(s.call(
        "audition_effect",
        json!({ "track": 0, "kind": "low_pass_filter", "params": { "cutoff_hz": 1000 } }),
    ));
    let before = s.node_count();

    ok(s.call(
        "add_effect",
        json!({ "track": 0, "kind": "low_pass_filter", "params": { "cutoff_hz": 1000 } }),
    ));
    assert_eq!(s.node_count(), before + 1, "exactly one node");
}

#[test]
fn a_bad_track_is_refused_by_name() {
    let mut s = Session::new();
    let msg = err(s.call(
        "audition_effect",
        json!({ "track": 7, "kind": "gain", "params": { "db": 0.0 } }),
    ));
    assert!(msg.contains('7'), "should name the track asked for: {msg}");
}

#[test]
fn an_inverted_region_is_refused() {
    let mut s = Session::new();
    let msg = err(s.call(
        "audition_effect",
        json!({ "track": 0, "kind": "gain", "params": { "db": 0.0 }, "start_sec": 1.0, "end_sec": 0.5 }),
    ));
    assert!(msg.contains("greater than"), "{msg}");
}
