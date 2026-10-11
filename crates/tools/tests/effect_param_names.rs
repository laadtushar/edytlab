//! A chain effect's parameter names are checked where the effect is added
//! or changed (#579).
//!
//! The renderer reads a gain's level from `db` and, for a name it does not
//! read, falls back to the default without a word. `add_effect` and
//! `set_effect_params` used to store whatever object they were given, so
//! `{"gain_db": 4.9}` on a gain was a success that did nothing: a 0 dB
//! gain. The model only found out by rendering and measuring again.
//!
//! These tests pin that the refusal comes at the edit, names the key that
//! is not a parameter and the ones that are, leaves the session as it was,
//! and holds for every kind on every chain (a track's, the master bus's, a
//! bus's) and in both tools, and in `audition_effect`, which promises that
//! `add_effect` with the same arguments keeps what it played.
//!
//! The table below is written out on purpose. The engine's registry is what
//! the tools consult; spelling the names here makes a change to either one a
//! visible edit to this file rather than a silent one.
//!
//! Only new calls are checked. A project saved by an older build can hold an
//! effect whose params carry a name no renderer reads, and it must still
//! open, render and be edited; the last group of tests pins that.

use std::path::{Path, PathBuf};

use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
use serde_json::{json, Value};
use session::EffectInstance;
use tempfile::TempDir;
use tools::{ToolContext, ToolDispatcher, ToolResult};

const SAMPLE_RATE: u32 = 48_000;

/// Each kind the renderer streams, with the parameter names it reads.
const KINDS: [(&str, &[&str]); 5] = [
    ("gain", &["db"]),
    ("limiter", &["ceiling_db", "release_ms"]),
    ("low_pass_filter", &["cutoff_hz"]),
    ("high_pass_filter", &["cutoff_hz"]),
    ("notch_filter", &["center_hz", "q"]),
];

/// The chain a call edits.
#[derive(Debug, Clone, Copy)]
enum Chain {
    Track,
    Master,
    Bus,
}

const CHAINS: [Chain; 3] = [Chain::Track, Chain::Master, Chain::Bus];

impl Chain {
    /// The arguments that name this chain.
    fn target(self) -> Value {
        match self {
            Chain::Track => json!({ "track": 0 }),
            Chain::Master => json!({ "scope": "master" }),
            Chain::Bus => json!({ "scope": "bus", "bus": "Drums" }),
        }
    }

    /// `args` with this chain's target merged in.
    fn with(self, mut args: Value) -> Value {
        let target = self.target();
        for (k, v) in target.as_object().expect("an object") {
            args[k] = v.clone();
        }
        args
    }
}

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
    dir: TempDir,
    store: session::Store,
    engine: audio_engine::Engine,
    dispatcher: ToolDispatcher,
    clipboard: Option<tools::Clipboard>,
}

impl Session {
    /// One track with a sine on it, and a bus called "Drums" that it sends
    /// to, so a chain on any of the three is heard.
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
        ok(s.call("load", json!({ "path": src.to_string_lossy() })));
        let bus = ok(s.call("create_bus", json!({ "name": "Drums" })));
        let id = bus["bus_id"].as_str().expect("a bus id").to_string();
        ok(s.call(
            "set_send",
            json!({ "track": 0, "bus_id": id, "level_db": 0.0 }),
        ));
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

    fn state(&self) -> session::SessionState {
        self.store
            .get(self.store.head().expect("a head"))
            .expect("head node")
            .state
    }

    /// The chain at the head.
    fn chain(&self, which: Chain) -> Vec<EffectInstance> {
        let state = self.state();
        match which {
            Chain::Track => state.tracks[0].effects.clone(),
            Chain::Master => state.master_chain.clone(),
            Chain::Bus => state.bus_routing.buses[0].effects.clone(),
        }
    }

    /// Put `effect` on `which` the way a project saved by an older build
    /// holds one: written straight into a node, not through a tool.
    fn plant(&mut self, which: Chain, effect: EffectInstance) {
        let mut node = self.store.get(self.store.head().expect("a head")).unwrap();
        match which {
            Chain::Track => node.state.tracks[0].effects.push(effect),
            Chain::Master => node.state.master_chain.push(effect),
            Chain::Bus => node.state.bus_routing.buses[0].effects.push(effect),
        }
        self.store.append(node).expect("append");
    }

    /// Render the head the way the app does.
    fn render(&self, name: &str) -> Vec<i16> {
        let out = self.dir.path().join(name);
        audio_engine::render_state_to_wav(&self.state(), &out, None).expect("render");
        WavReader::open(&out)
            .expect("open render")
            .samples::<i16>()
            .map(|r| r.expect("sample"))
            .collect()
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

/// The whole words of `msg`, so that `db` is found in "takes: db" and not
/// inside "gain_db" or "ceiling_db".
fn words(msg: &str) -> Vec<&str> {
    msg.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .filter(|w| !w.is_empty())
        .collect()
}

/// Whether `msg` names `word` as a word of its own.
fn names(msg: &str, word: &str) -> bool {
    words(msg).contains(&word)
}

/// Every name the table gives to some kind and not to `kind`.
fn names_of_other_kinds(kind: &str) -> Vec<&'static str> {
    let own = KINDS.iter().find(|(k, _)| *k == kind).expect("a kind").1;
    let mut out: Vec<&'static str> = KINDS
        .iter()
        .flat_map(|(_, names)| names.iter().copied())
        .filter(|n| !own.contains(n))
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// **The ticket.** `{"gain_db": 4.9}` on a gain, as the agent wrote it on the
/// master bus, is refused with the name the gain does read, and nothing is
/// written.
#[test]
fn gain_db_on_a_gain_is_refused_and_the_message_names_db() {
    for chain in CHAINS {
        let mut s = Session::new();
        let head = s.head();
        let nodes = s.node_count();

        let msg = err(s.call(
            "add_effect",
            chain.with(json!({ "kind": "gain", "params": { "gain_db": 4.9 }, "position": 0 })),
        ));

        assert!(
            names(&msg, "gain_db"),
            "{chain:?}: names the bad key: {msg}"
        );
        assert!(names(&msg, "gain"), "{chain:?}: names the kind: {msg}");
        assert!(
            names(&msg, "db"),
            "{chain:?}: names the key that works: {msg}"
        );
        assert_eq!(
            s.head(),
            head,
            "{chain:?}: a refusal must not move the head"
        );
        assert_eq!(s.node_count(), nodes, "{chain:?}: and writes no node");
        assert!(
            s.chain(chain).is_empty(),
            "{chain:?}: the chain stays empty"
        );
    }
}

/// Every kind, on every chain: a name that is not a parameter is refused;
/// the message names it, the kind, and each name that kind does take, and
/// nothing is written.
#[test]
fn add_effect_refuses_an_unknown_name_for_every_kind_on_every_chain() {
    for chain in CHAINS {
        let mut s = Session::new();
        let head = s.head();
        let nodes = s.node_count();

        for (kind, param_names) in KINDS {
            let msg = err(s.call(
                "add_effect",
                chain.with(json!({ "kind": kind, "params": { "wobble": 1.0 } })),
            ));

            assert!(
                names(&msg, "wobble"),
                "{chain:?} {kind}: names the bad key: {msg}"
            );
            assert!(names(&msg, kind), "{chain:?} {kind}: names the kind: {msg}");
            for name in param_names {
                assert!(
                    names(&msg, name),
                    "{chain:?} {kind}: should list `{name}`: {msg}"
                );
            }
            for other in names_of_other_kinds(kind) {
                assert!(
                    !names(&msg, other),
                    "{chain:?} {kind}: lists `{other}`, which {kind} does not take: {msg}"
                );
            }
            assert_eq!(s.head(), head, "{chain:?} {kind}: the head must not move");
            assert_eq!(s.node_count(), nodes, "{chain:?} {kind}: no node");
            assert!(s.chain(chain).is_empty(), "{chain:?} {kind}: chain empty");
        }
    }
}

/// A name that is right for another kind is still wrong for this one: the
/// check is per kind, not against every name the registry knows.
#[test]
fn a_name_that_belongs_to_another_kind_is_refused() {
    let mut s = Session::new();
    let head = s.head();
    let msg = err(s.call(
        "add_effect",
        json!({ "track": 0, "kind": "gain", "params": { "ceiling_db": -1.0 } }),
    ));
    assert!(names(&msg, "ceiling_db"), "{msg}");
    assert!(names(&msg, "db"), "{msg}");
    assert_eq!(s.head(), head);

    // Names are matched exactly, as the renderer matches them.
    for name in ["DB", "Db", " db", "db "] {
        let msg = err(s.call(
            "add_effect",
            json!({ "track": 0, "kind": "gain", "params": { name: 3.0 } }),
        ));
        assert!(msg.contains("unknown"), "{name:?}: {msg}");
    }
    assert_eq!(s.head(), head);
}

/// One bad key among good ones refuses the whole call, and every bad key is
/// named, so the model fixes them in one go.
#[test]
fn one_bad_key_among_good_ones_refuses_the_call_and_all_bad_keys_are_named() {
    let mut s = Session::new();
    let head = s.head();
    let msg = err(s.call(
        "add_effect",
        json!({
            "track": 0, "kind": "limiter",
            "params": { "ceiling_db": -1.0, "attack_ms": 5.0, "lookahead": 2.0 },
        }),
    ));
    assert!(
        names(&msg, "attack_ms") && names(&msg, "lookahead"),
        "{msg}"
    );
    assert_eq!(s.head(), head);
    assert!(s.chain(Chain::Track).is_empty());
}

/// The refusal must not reach what works: every name of every kind is
/// accepted on every chain, stored as given, no params and empty params are
/// accepted, and the chains render.
#[test]
fn every_name_every_kind_takes_is_accepted_on_every_chain_and_renders() {
    for chain in CHAINS {
        let mut s = Session::new();
        for (kind, param_names) in KINDS {
            let params: serde_json::Map<String, Value> = param_names
                .iter()
                .map(|n| {
                    let v = if n.ends_with("_hz") { 1_000.0 } else { 1.0 };
                    (n.to_string(), json!(v))
                })
                .collect();
            ok(s.call(
                "add_effect",
                chain.with(json!({ "kind": kind, "params": Value::Object(params.clone()) })),
            ));
            let stored = s.chain(chain);
            assert_eq!(
                stored.last().expect("the effect").params,
                Value::Object(params),
                "{chain:?} {kind}: params are stored as given"
            );

            // No params, and an empty object, are the defaults.
            ok(s.call("add_effect", chain.with(json!({ "kind": kind }))));
            ok(s.call(
                "add_effect",
                chain.with(json!({ "kind": kind, "params": {} })),
            ));
        }
        assert_eq!(s.chain(chain).len(), KINDS.len() * 3, "{chain:?}");
        s.render("chain.wav");
    }
}

/// The same check in `set_effect_params`, in both of its modes. A refusal
/// leaves the stored params as they were.
#[test]
fn set_effect_params_refuses_an_unknown_name_merging_or_replacing() {
    for chain in CHAINS {
        for replace in [false, true] {
            let mut s = Session::new();
            ok(s.call(
                "add_effect",
                chain.with(json!({ "kind": "gain", "params": { "db": -3.0 } })),
            ));
            let head = s.head();
            let nodes = s.node_count();

            let msg = err(s.call(
                "set_effect_params",
                chain.with(json!({
                    "effect_index": 0, "params": { "gain_db": 4.9 }, "replace": replace,
                })),
            ));

            let label = format!("{chain:?} replace={replace}");
            assert!(names(&msg, "gain_db"), "{label}: names the bad key: {msg}");
            assert!(
                names(&msg, "db"),
                "{label}: names the key that works: {msg}"
            );
            assert_eq!(s.head(), head, "{label}: the head must not move");
            assert_eq!(s.node_count(), nodes, "{label}: no node");
            assert_eq!(
                s.chain(chain)[0].params,
                json!({ "db": -3.0 }),
                "{label}: the stored params stay as they were"
            );
        }
    }
}

/// A patch with a good key and a bad one applies neither: half a change
/// would look like success.
#[test]
fn set_effect_params_applies_none_of_a_patch_that_has_a_bad_key() {
    let mut s = Session::new();
    ok(s.call(
        "add_effect",
        json!({ "scope": "master", "kind": "limiter", "params": { "ceiling_db": -1.0 } }),
    ));
    let head = s.head();
    let msg = err(s.call(
        "set_effect_params",
        json!({
            "scope": "master", "effect_index": 0,
            "params": { "ceiling_db": -6.0, "threshold": -6.0 },
        }),
    ));
    assert!(names(&msg, "threshold"), "{msg}");
    assert_eq!(s.head(), head);
    assert_eq!(
        s.chain(Chain::Master)[0].params,
        json!({ "ceiling_db": -1.0 })
    );
}

/// The check asks the effect's own kind, so each kind's names work in the
/// effect that has them, in both modes.
#[test]
fn set_effect_params_still_changes_every_name_every_kind_takes() {
    for chain in CHAINS {
        let mut s = Session::new();
        for (i, (kind, param_names)) in KINDS.iter().enumerate() {
            ok(s.call("add_effect", chain.with(json!({ "kind": kind }))));
            for replace in [false, true] {
                let params: serde_json::Map<String, Value> = param_names
                    .iter()
                    .map(|n| (n.to_string(), json!(2.0)))
                    .collect();
                let out = ok(s.call(
                    "set_effect_params",
                    chain.with(json!({
                        "effect_index": i, "params": Value::Object(params.clone()),
                        "replace": replace,
                    })),
                ));
                assert_eq!(out["params"], Value::Object(params), "{chain:?} {kind}");
            }
        }
    }
}

/// `audition_effect` says "call add_effect with the same arguments to keep
/// it", so it must not play what add_effect would refuse: an audition of a
/// gain whose level it never read would sound like nothing happened. Refused
/// before anything is rendered, and no excerpt is written.
#[test]
fn audition_effect_refuses_an_unknown_name_and_renders_nothing() {
    for chain in CHAINS {
        let mut s = Session::new();
        let head = s.head();
        let msg = err(s.call(
            "audition_effect",
            chain.with(json!({ "kind": "gain", "params": { "gain_db": 4.9 } })),
        ));
        assert!(names(&msg, "gain_db"), "{chain:?}: {msg}");
        assert!(names(&msg, "db"), "{chain:?}: {msg}");
        assert_eq!(s.head(), head);

        let dir = s.dir.path().join(".audiograph").join("auditions");
        let written = std::fs::read_dir(&dir).map(|d| d.count()).unwrap_or(0);
        assert_eq!(written, 0, "{chain:?}: a refused audition renders nothing");

        // The same call with the name the gain reads is an audition.
        ok(s.call(
            "audition_effect",
            chain.with(json!({ "kind": "gain", "params": { "db": 4.9 } })),
        ));
    }
}

// --- projects saved before the check -----------------------------------------

/// An effect saved with a name no renderer reads, as the ticket's agent
/// wrote one. Loading must keep it, rendering must run it, and the tools
/// must edit the chain around it.
fn stale_gain() -> EffectInstance {
    EffectInstance {
        kind: "gain".into(),
        params: json!({ "gain_db": 4.9 }),
        bypassed: false,
    }
}

/// **Validation is for new calls, not for loading.** The stored effect
/// survives a reopen of the project untouched, and renders as the 0 dB gain
/// it always was.
#[test]
fn a_stored_effect_with_an_unknown_name_still_loads_and_renders() {
    for chain in CHAINS {
        let mut s = Session::new();
        let before = s.render("before.wav");
        s.plant(chain, stale_gain());
        let head = s.head();

        // A fresh store over the same project folder is a reopen.
        let reopened = session::Store::open(s.dir.path()).expect("reopen the project");
        let node = reopened
            .get(reopened.head().expect("the head survives"))
            .expect("the head node loads");
        assert_eq!(reopened.head().unwrap().to_hex(), head);
        let stored = match chain {
            Chain::Track => &node.state.tracks[0].effects,
            Chain::Master => &node.state.master_chain,
            Chain::Bus => &node.state.bus_routing.buses[0].effects,
        };
        assert_eq!(stored.len(), 1, "{chain:?}");
        assert_eq!(stored[0].params, json!({ "gain_db": 4.9 }), "{chain:?}");

        // It renders, and as the 0 dB gain: the audio is unchanged.
        assert_eq!(s.render("after.wav"), before, "{chain:?}");
    }
}

/// The stale effect can be fixed and its neighbours edited: only the keys
/// of a new call are looked at, never what is already stored.
#[test]
fn a_stored_effect_with_an_unknown_name_can_still_be_edited() {
    for chain in CHAINS {
        let mut s = Session::new();
        s.plant(chain, stale_gain());

        // A new call that repeats the stale name is a new call: refused.
        let msg = err(s.call(
            "set_effect_params",
            chain.with(json!({ "effect_index": 0, "params": { "gain_db": 6.0 } })),
        ));
        assert!(names(&msg, "gain_db"), "{chain:?}: {msg}");

        // Merging the name the gain reads is fine, the stale key is left
        // where it was, and the effect now does what it says.
        let out = ok(s.call(
            "set_effect_params",
            chain.with(json!({ "effect_index": 0, "params": { "db": -12.0 } })),
        ));
        assert_eq!(out["params"]["db"], -12.0, "{chain:?}: {out}");

        // Replacing drops the stale key.
        let out = ok(s.call(
            "set_effect_params",
            chain.with(json!({
                "effect_index": 0, "params": { "db": -6.0 }, "replace": true,
            })),
        ));
        assert_eq!(out["params"], json!({ "db": -6.0 }), "{chain:?}: {out}");

        // Other tools on the chain are untouched by any of this.
        ok(s.call(
            "set_effect_bypassed",
            chain.with(json!({ "effect_index": 0, "bypassed": true })),
        ));
        ok(s.call(
            "set_effect_bypassed",
            chain.with(json!({ "effect_index": 0, "bypassed": false })),
        ));
        ok(s.call("remove_effect", chain.with(json!({ "effect_index": 0 }))));
        assert!(s.chain(chain).is_empty());
    }
}

/// A stored effect of a kind the renderer cannot run has no list of names to
/// check against (it renders nothing, and says so), and its parameters stay
/// editable as they were, so an old project's reverb can be tuned for the
/// day it streams.
#[test]
fn a_stored_effect_of_a_kind_with_no_names_to_check_keeps_its_params_editable() {
    let mut s = Session::new();
    s.plant(
        Chain::Track,
        EffectInstance {
            kind: "reverb".into(),
            params: json!({ "room_size": 0.5 }),
            bypassed: true,
        },
    );
    let out = ok(s.call(
        "set_effect_params",
        json!({ "track": 0, "effect_index": 0, "params": { "decay": 2.0 } }),
    ));
    assert_eq!(out["params"], json!({ "room_size": 0.5, "decay": 2.0 }));
}
