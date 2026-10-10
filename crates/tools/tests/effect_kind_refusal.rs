//! A chain effect the renderer cannot stream is refused where it is added
//! (#520).
//!
//! `add_effect` used to store any kind it was given. The renderer streams
//! a chain a chunk at a time, and eleven kinds that exist as destructive
//! tools (reverb, echo, eq, compressor...) cannot run that way yet, so a
//! chain holding one failed every render until it was removed. These tests
//! pin that the refusal comes at the edit, names the kinds that do work so
//! the model can choose, and leaves the session exactly as it was.
//!
//! The two lists below are written out on purpose. The engine's registry
//! is what the tool consults; spelling the kinds here is what makes a
//! change to either one a visible edit to this file rather than a silent
//! one.

use std::path::{Path, PathBuf};

use hound::{SampleFormat, WavSpec, WavWriter};
use serde_json::{json, Value};
use session::EffectInstance;
use tempfile::TempDir;
use tools::{ToolContext, ToolDispatcher, ToolResult};

const SAMPLE_RATE: u32 = 48_000;

/// What the streaming renderer can run in a chain today.
const STREAMABLE: [&str; 5] = [
    "gain",
    "limiter",
    "low_pass_filter",
    "high_pass_filter",
    "notch_filter",
];

/// Effects that exist as destructive tools but cannot run in a chain yet.
const NOT_YET_STREAMABLE: [&str; 11] = [
    "tremolo",
    "phaser",
    "echo",
    "reverb",
    "noise_gate",
    "leveler",
    "de_esser",
    "distortion",
    "stereo_widener",
    "eq",
    "compressor",
];

fn write_sine(path: &Path) -> PathBuf {
    let spec = WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut w = WavWriter::create(path, spec).expect("wav writer");
    for n in 0..(SAMPLE_RATE as usize / 2) {
        let t = n as f32 / SAMPLE_RATE as f32;
        let s = (2.0 * std::f32::consts::PI * 300.0 * t).sin() * 0.3;
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
    clipboard: Option<tools::Clipboard>,
}

impl Session {
    fn new() -> Self {
        let dir = TempDir::new().expect("tempdir");
        let src = write_sine(&dir.path().join("in.wav"));
        let store = session::Store::open(dir.path()).expect("open store");
        let mut s = Self {
            _dir: dir,
            store,
            engine: audio_engine::Engine::new(),
            dispatcher: ToolDispatcher::default_dispatcher(),
            clipboard: None,
        };
        ok(s.call("load", json!({ "path": src.to_string_lossy() })));
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

    /// Track 0's chain at the head.
    fn chain(&self) -> Vec<EffectInstance> {
        let node = self.store.get(self.store.head().expect("a head")).unwrap();
        node.state.tracks[0].effects.clone()
    }

    /// Put `effect` on track 0 the way a project saved by an older build
    /// holds one: written straight into a node, not through a tool.
    fn plant(&mut self, effect: EffectInstance) {
        let mut node = self.store.get(self.store.head().expect("a head")).unwrap();
        node.state.tracks[0].effects.push(effect);
        self.store.append(node).expect("append");
    }

    /// The session renders: the question a stored effect decides.
    fn renders(&mut self) -> Result<Value, String> {
        let node_id = self.head();
        match self.call("render_preview", json!({ "node_id": node_id })) {
            ToolResult::Ok(v) => Ok(v),
            ToolResult::Error(m) => Err(m),
        }
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

/// **The ticket.** Every kind the renderer cannot stream is refused, with
/// the kinds that work in the message, and nothing is written.
#[test]
fn add_effect_refuses_each_kind_the_renderer_cannot_stream() {
    let mut s = Session::new();
    let head = s.head();
    let nodes = s.node_count();

    for kind in NOT_YET_STREAMABLE {
        let msg = err(s.call("add_effect", json!({ "track": 0, "kind": kind })));

        assert!(msg.contains(kind), "{kind}: should name the kind: {msg}");
        for works in STREAMABLE {
            assert!(
                msg.contains(works),
                "{kind}: should list `{works}` among the kinds that work: {msg}"
            );
        }
        assert!(
            msg.contains(&format!("`{kind}` tool")),
            "{kind}: should point at the destructive tool of that name: {msg}"
        );

        assert_eq!(s.head(), head, "{kind}: a refusal must not move the head");
        assert_eq!(
            s.node_count(),
            nodes,
            "{kind}: a refusal must write no node"
        );
        assert!(s.chain().is_empty(), "{kind}: the chain must stay empty");
    }

    s.renders()
        .expect("a refused effect leaves a session that renders");
}

/// The destructive tool the refusal points at has to exist, or the advice
/// is a dead end.
#[test]
fn the_destructive_tool_a_refusal_names_exists() {
    let d = ToolDispatcher::default_dispatcher();
    for kind in NOT_YET_STREAMABLE {
        assert!(
            d.get(kind).is_some(),
            "the refusal for `{kind}` points at a `{kind}` tool, which is not registered"
        );
    }
}

/// A kind that exists nowhere is a different mistake from one that is not
/// ready yet, and gets its own message. It is refused too: stored, it would
/// fail every render exactly as an unstreamable one does.
#[test]
fn add_effect_refuses_an_unknown_kind_and_lists_what_works() {
    let mut s = Session::new();
    let head = s.head();

    for kind in ["wobbulator", "Reverb", "GAIN", " gain", "gain ", ""] {
        let msg = err(s.call("add_effect", json!({ "track": 0, "kind": kind })));
        assert!(
            msg.contains("unknown effect kind"),
            "{kind:?}: an unknown kind should read as unknown: {msg}"
        );
        for works in STREAMABLE {
            assert!(
                msg.contains(works),
                "{kind:?}: should list `{works}`: {msg}"
            );
        }
        assert_eq!(s.head(), head, "{kind:?}: a refusal must not move the head");
    }
}

/// The refusal must not reach the kinds that work: each is accepted with no
/// parameters, and a chain of all of them renders. This is the property the
/// tool and the renderer have to share, checked end to end.
#[test]
fn every_kind_add_effect_accepts_renders() {
    let mut s = Session::new();
    for kind in STREAMABLE {
        ok(s.call("add_effect", json!({ "track": 0, "kind": kind })));
    }
    assert_eq!(s.chain().len(), STREAMABLE.len());
    s.renders()
        .unwrap_or_else(|e| panic!("a chain of accepted kinds failed to render: {e}"));
}

/// Bypass is a way into the chain too. A bypassed effect of a kind the
/// renderer cannot stream (an old project's, or a hand-edited one) renders
/// fine, because the renderer skips bypassed entries; switching it back on
/// would make every render fail, so that is refused.
#[test]
fn a_stored_unstreamable_effect_cannot_be_switched_back_on() {
    let mut s = Session::new();
    s.plant(EffectInstance {
        kind: "reverb".into(),
        params: json!({}),
        bypassed: true,
    });
    s.renders()
        .expect("a bypassed effect is skipped, so the session renders");
    let head = s.head();

    let msg = err(s.call(
        "set_effect_bypassed",
        json!({ "track": 0, "effect_index": 0, "bypassed": false }),
    ));
    assert!(msg.contains("reverb"), "should name the kind: {msg}");
    for works in STREAMABLE {
        assert!(msg.contains(works), "should list `{works}`: {msg}");
    }
    assert_eq!(s.head(), head, "a refusal must not move the head");
    assert!(s.chain()[0].bypassed, "the effect must stay bypassed");
}

/// Bypassing, and switching a streamable effect on and off, are untouched.
#[test]
fn bypass_still_works_for_every_kind_that_can_be_bypassed() {
    let mut s = Session::new();
    s.plant(EffectInstance {
        kind: "reverb".into(),
        params: json!({}),
        bypassed: true,
    });
    ok(s.call("add_effect", json!({ "track": 0, "kind": "gain" })));

    // Already bypassed, and stays so: allowed, since it changes nothing
    // the renderer sees.
    ok(s.call(
        "set_effect_bypassed",
        json!({ "track": 0, "effect_index": 0, "bypassed": true }),
    ));
    ok(s.call(
        "set_effect_bypassed",
        json!({ "track": 0, "effect_index": 1, "bypassed": true }),
    ));
    ok(s.call(
        "set_effect_bypassed",
        json!({ "track": 0, "effect_index": 1, "bypassed": false }),
    ));
    s.renders().expect("renders");
}

/// `set_effect_params` edits the parameter object, never the kind, so it
/// cannot be a way to turn a good effect into one the renderer cannot run.
#[test]
fn set_effect_params_cannot_change_an_effects_kind() {
    let mut s = Session::new();
    ok(s.call(
        "add_effect",
        json!({ "track": 0, "kind": "gain", "params": { "db": -3.0 } }),
    ));

    for replace in [false, true] {
        let out = ok(s.call(
            "set_effect_params",
            json!({
                "track": 0,
                "effect_index": 0,
                "params": { "kind": "reverb", "db": -6.0 },
                "replace": replace,
            }),
        ));
        assert_eq!(out["kind"], "gain", "replace={replace}: {out}");
        assert_eq!(s.chain()[0].kind, "gain", "replace={replace}");
    }
    s.renders().expect("renders");
}

/// The text the model reads in `add_effect`'s `kind` field names the kinds
/// that work, and none that do not. It is a literal in the schema, so this
/// is what keeps it in step with the registry.
#[test]
fn the_kind_description_names_exactly_the_kinds_that_work() {
    let d = ToolDispatcher::default_dispatcher();
    let schemas = d.tool_schemas();
    let add = schemas
        .as_array()
        .expect("schemas are an array")
        .iter()
        .find(|t| t["name"] == "add_effect")
        .expect("add_effect is registered");
    let text = add["input_schema"]["properties"]["kind"]["description"]
        .as_str()
        .expect("kind has a description");

    // Whole words, so a short kind like `eq` is not found inside "frequency".
    let words: Vec<&str> = text
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .collect();
    for kind in STREAMABLE {
        assert!(
            words.contains(&kind),
            "kind description omits `{kind}`: {text}"
        );
    }
    for kind in NOT_YET_STREAMABLE {
        assert!(
            !words.contains(&kind),
            "kind description offers `{kind}`, which the renderer cannot run: {text}"
        );
    }
}
