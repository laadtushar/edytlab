//! The agent can put an effect on a bus (#533).
//!
//! `create_bus` and `set_send` have existed since #111, and the renderer
//! runs a bus's chain on the sum of what its sends feed it, then adds the
//! result to the master. But nothing could write to `Bus::effects`: a bus
//! was a name with a chain nobody could fill, so "one shared reverb fed by
//! several tracks", which is what the bus is for, could not be built.
//!
//! The five chain tools (`add_effect`, `remove_effect`, `reorder_effects`,
//! `set_effect_params`, `set_effect_bypassed`) take `scope: "bus"` and
//! `bus: "<name>"` in place of a `track`. These tests drive them through
//! the dispatcher and check what the master tests check for the master
//! bus:
//!
//! * what reaches the **samples**: a gain on a bus changes what the bus
//!   adds to the mix by exactly that gain, a bypassed or removed effect
//!   renders byte-identically to none, and params and order are heard;
//! * that the **bus is found** by name (or by id, which tells apart two
//!   buses that share a name), and that one that is not there is refused
//!   with the buses the session has;
//! * that the **kind** is refused as it is on a track and on the master,
//!   and that a stored effect the renderer cannot run can be removed;
//! * that every edit is an ordinary **undoable node** and a bus-scope
//!   `DiffOp::Effect`, and that no chain reaches into another.

use std::path::{Path, PathBuf};

use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
use serde_json::{json, Value};
use session::{DiffOp, EffectInstance, EffectScope};
use tempfile::TempDir;
use tools::{DispatchError, ToolContext, ToolDispatcher, ToolResult};

const SAMPLE_RATE: u32 = 44_100;
const FRAMES: usize = 44_100;

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

/// A mono sine, `amp` of full scale.
fn write_tone(dir: &Path, name: &str, freq: f32, amp: f32) -> PathBuf {
    let path = dir.join(name);
    let spec = WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut w = WavWriter::create(&path, spec).expect("wav writer");
    for n in 0..FRAMES {
        let t = n as f32 / SAMPLE_RATE as f32;
        let s = (2.0 * std::f32::consts::PI * freq * t).sin() * amp;
        w.write_sample((s * 32_767.0) as i16).unwrap();
    }
    w.finalize().unwrap();
    path
}

/// A two-track session: 220 Hz and 330 Hz at 0.15 each. Quiet on purpose:
/// a bus adds to the mix, so the room the tests need is the headroom to
/// boost what a bus carries by 12 dB without the sum reaching full scale,
/// where the render's clamp would hide the level the tests measure.
struct Session {
    dir: TempDir,
    store: session::Store,
    engine: audio_engine::Engine,
    dispatcher: ToolDispatcher,
    clipboard: Option<tools::Clipboard>,
    renders: usize,
}

impl Session {
    fn two_quiet_tracks() -> Self {
        let dir = TempDir::new().expect("tempdir");
        let a = write_tone(dir.path(), "a.wav", 220.0, 0.15);
        let b = write_tone(dir.path(), "b.wav", 330.0, 0.15);
        let store = session::Store::open(dir.path()).expect("open store");
        let mut s = Self {
            dir,
            store,
            engine: audio_engine::Engine::new(),
            dispatcher: ToolDispatcher::default_dispatcher(),
            clipboard: None,
            renders: 0,
        };
        ok(s.call("load", json!({ "path": a.to_string_lossy() })));
        ok(s.call("load", json!({ "path": b.to_string_lossy() })));
        s
    }

    /// The same, with a bus called `name` that track 0 sends to at 0 dB.
    fn with_bus(name: &str) -> (Self, String) {
        let mut s = Self::two_quiet_tracks();
        let id = s.create_bus(name);
        s.send(0, &id, 0.0);
        (s, id)
    }

    /// The dispatcher's whole answer, including a schema rejection.
    fn try_call(&mut self, tool: &str, args: Value) -> Result<ToolResult, DispatchError> {
        let mut ctx = ToolContext {
            store: &mut self.store,
            engine: &mut self.engine,
            user_message: "",
            clipboard: &mut self.clipboard,
            allowed_tools: None,
        };
        self.dispatcher.invoke(tool, args, &mut ctx)
    }

    fn call(&mut self, tool: &str, args: Value) -> ToolResult {
        self.try_call(tool, args)
            .unwrap_or_else(|e| panic!("{tool} was rejected before it ran: {e}"))
    }

    fn head(&self) -> session::NodeId {
        self.store.head().expect("a head")
    }

    fn state(&self) -> session::SessionState {
        self.store.get(self.head()).expect("head node").state
    }

    fn node_count(&self) -> usize {
        self.store.list_nodes().expect("list nodes").len()
    }

    /// Render the head the way the app does, and return its PCM.
    fn render(&mut self) -> Vec<i16> {
        self.renders += 1;
        let out = self.dir.path().join(format!("out-{}.wav", self.renders));
        audio_engine::render_state_to_wav(&self.state(), &out, None).expect("render");
        WavReader::open(&out)
            .expect("open render")
            .samples::<i16>()
            .map(|r| r.expect("sample"))
            .collect()
    }

    /// Create a bus; returns its id.
    fn create_bus(&mut self, name: &str) -> String {
        let v = ok(self.call("create_bus", json!({ "name": name })));
        v["bus_id"].as_str().expect("a bus id").to_string()
    }

    fn send(&mut self, track: usize, bus_id: &str, level_db: f64) {
        ok(self.call(
            "set_send",
            json!({ "track": track, "bus_id": bus_id, "level_db": level_db }),
        ));
    }

    fn add_bus(&mut self, bus: &str, kind: &str, params: Value) -> Value {
        ok(self.call(
            "add_effect",
            json!({ "scope": "bus", "bus": bus, "kind": kind, "params": params }),
        ))
    }

    /// The chain of the bus called `name` at the head.
    fn bus_chain(&self, name: &str) -> Vec<EffectInstance> {
        self.state()
            .bus_routing
            .buses
            .iter()
            .find(|b| b.name == name)
            .unwrap_or_else(|| panic!("no bus called {name}"))
            .effects
            .clone()
    }

    /// Put `effect` on the bus called `name` the way a project saved by an
    /// older build holds one: written straight into a node, not through a
    /// tool.
    fn plant_bus(&mut self, name: &str, effect: EffectInstance) {
        let mut node = self.store.get(self.head()).unwrap();
        node.state
            .bus_routing
            .buses
            .iter_mut()
            .find(|b| b.name == name)
            .expect("a bus to plant on")
            .effects
            .push(effect);
        self.store.append(node).expect("append");
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

/// What `mix` has that `dry` does not, sample by sample: the bus's
/// contribution, since a bus adds to the mix rather than replacing it.
fn added_to(mix: &[i16], dry: &[i16]) -> Vec<f32> {
    assert_eq!(mix.len(), dry.len(), "the two renders differ in length");
    mix.iter()
        .zip(dry)
        .map(|(m, d)| *m as f32 - *d as f32)
        .collect()
}

fn peak_f(v: &[f32]) -> f32 {
    v.iter().map(|x| x.abs()).fold(0.0, f32::max)
}

/// Full-scale sample value of a dBFS level.
fn level(db: f32) -> f32 {
    10.0f32.powf(db / 20.0) * 32_767.0
}

/// Assert that `got` is `want` scaled by `factor`, to within a few steps of
/// 16-bit rounding (three renders are involved, each rounded).
fn assert_scaled(got: &[f32], want: &[f32], factor: f32, what: &str) {
    assert_eq!(got.len(), want.len());
    let worst = got
        .iter()
        .zip(want)
        .map(|(g, w)| (g - factor * w).abs())
        .fold(0.0, f32::max);
    assert!(
        worst <= 4.0,
        "{what}: the bus's contribution is not the unprocessed one x {factor}: off by up to \
         {worst} steps"
    );
}

const DB6: f32 = 1.995_262_3;

// ─── the audio ────────────────────────────────────────────────────────

/// The ticket: a gain on a bus is a gain on what the bus carries. Track 0
/// sends to the bus at 0 dB, so with no bus effect the mix is the dry mix
/// plus a copy of track 0; with +6 dB on the bus it is the dry mix plus
/// that copy at twice the amplitude. Both are measured against the same
/// dry mix, sample by sample.
#[test]
fn a_gain_added_to_a_bus_is_heard_at_that_level() {
    let (mut s, _) = Session::with_bus("Drums");
    // The same session with the send taken out: the dry mix.
    let mut dry_session = Session::two_quiet_tracks();
    let dry = dry_session.render();

    let routed = s.render();
    let carried = added_to(&routed, &dry);
    assert!(
        peak_f(&carried) > level(-20.0),
        "the premise failed: the send put nothing on the bus (peak {})",
        peak_f(&carried)
    );

    let v = s.add_bus("Drums", "gain", json!({ "db": 6.0 }));
    assert_eq!(v["effect_count"], 1, "{v}");
    let boosted = s.render();

    assert_ne!(
        boosted, routed,
        "the render is unchanged: the bus chain was ignored"
    );
    assert_scaled(&added_to(&boosted, &dry), &carried, DB6, "+6 dB on the bus");
}

/// A bus effect is part of the signal path from a send to the master, and
/// only that. A bus nothing sends to carries nothing, so an effect on it
/// is stored and not heard. The tool's description says so; this pins it.
#[test]
fn an_effect_on_a_bus_nothing_sends_to_is_not_heard() {
    let mut s = Session::two_quiet_tracks();
    let dry = s.render();
    s.create_bus("Idle");
    s.add_bus("Idle", "gain", json!({ "db": 12.0 }));
    assert_eq!(s.bus_chain("Idle").len(), 1);
    assert_eq!(s.render(), dry, "an unfed bus changed the mix");
}

/// A bypassed effect renders as though it were absent, byte for byte, and
/// so does a removed one.
#[test]
fn bypassing_or_removing_a_bus_effect_renders_the_unprocessed_bus_exactly() {
    let (mut s, _) = Session::with_bus("Drums");
    let routed = s.render();
    s.add_bus("Drums", "gain", json!({ "db": 6.0 }));
    assert_ne!(s.render(), routed);

    ok(s.call(
        "set_effect_bypassed",
        json!({ "scope": "bus", "bus": "Drums", "effect_index": 0, "bypassed": true }),
    ));
    assert_eq!(s.render(), routed, "a bypassed bus effect changed the mix");

    ok(s.call(
        "set_effect_bypassed",
        json!({ "scope": "bus", "bus": "Drums", "effect_index": 0, "bypassed": false }),
    ));
    assert_ne!(s.render(), routed, "re-enabling it should bring it back");

    ok(s.call(
        "remove_effect",
        json!({ "scope": "bus", "bus": "Drums", "effect_index": 0 }),
    ));
    assert_eq!(s.render(), routed, "a removed bus effect changed the mix");
    assert!(s.bus_chain("Drums").is_empty());
}

/// Editing a bus effect's parameters is heard, and merges rather than
/// replaces, as it does on a track.
#[test]
fn bus_effect_params_are_editable_in_place() {
    let (mut s, _) = Session::with_bus("Drums");
    let mut dry_session = Session::two_quiet_tracks();
    let dry = dry_session.render();
    let carried = added_to(&s.render(), &dry);

    // A limiter whose ceiling is below the signal it carries, so there is
    // a second parameter (release) for the merge to keep.
    s.add_bus(
        "Drums",
        "limiter",
        json!({ "ceiling_db": -6.0, "release_ms": 80.0 }),
    );
    let v = ok(s.call(
        "set_effect_params",
        json!({
            "scope": "bus", "bus": "Drums", "effect_index": 0,
            "params": { "ceiling_db": -12.0 }
        }),
    ));
    assert_eq!(v["params"]["ceiling_db"], -12.0, "{v}");
    assert_eq!(
        v["params"]["release_ms"], 80.0,
        "a merge must keep the keys it was not given: {v}"
    );
    assert_eq!(s.bus_chain("Drums")[0].params["ceiling_db"], -12.0);

    // Track 0 peaks near -16.5 dBFS, so a -12 dB ceiling leaves it alone
    // and a -20 dB one does not: the edit is heard.
    let untouched = added_to(&s.render(), &dry);
    assert_scaled(&untouched, &carried, 1.0, "a ceiling above the signal");
    ok(s.call(
        "set_effect_params",
        json!({
            "scope": "bus", "bus": "Drums", "effect_index": 0,
            "params": { "ceiling_db": -20.0 }
        }),
    ));
    let limited = added_to(&s.render(), &dry);
    assert!(
        peak_f(&limited) <= level(-20.0) + 2.0 && peak_f(&limited) < peak_f(&carried) - 1_000.0,
        "lowering the ceiling to -20 dBFS should lower what the bus adds: {} -> {}",
        peak_f(&carried),
        peak_f(&limited)
    );

    // `replace` swaps the object wholesale, so the release is gone.
    let replaced = ok(s.call(
        "set_effect_params",
        json!({
            "scope": "bus", "bus": "Drums", "effect_index": 0,
            "params": { "ceiling_db": -3.0 }, "replace": true
        }),
    ));
    assert!(replaced["params"].get("release_ms").is_none(), "{replaced}");
}

/// Order is part of the sound: boosting into a limiter is not limiting and
/// then boosting.
#[test]
fn reordering_a_bus_chain_changes_the_mix() {
    let (mut s, _) = Session::with_bus("Drums");
    let mut dry_session = Session::two_quiet_tracks();
    let dry = dry_session.render();

    s.add_bus("Drums", "gain", json!({ "db": 12.0 }));
    s.add_bus("Drums", "limiter", json!({ "ceiling_db": -6.0 }));
    let gain_then_limit = added_to(&s.render(), &dry);
    assert!(
        peak_f(&gain_then_limit) <= level(-6.0) + 2.0,
        "boost then limit should stay under the ceiling: {}",
        peak_f(&gain_then_limit)
    );

    ok(s.call(
        "reorder_effects",
        json!({ "scope": "bus", "bus": "Drums", "order": [1, 0] }),
    ));
    let limit_then_gain = added_to(&s.render(), &dry);
    assert!(
        peak_f(&limit_then_gain) > level(-6.0) + 1_000.0,
        "a gain after the limiter should push the bus back over its ceiling: {}",
        peak_f(&limit_then_gain)
    );
    let kinds: Vec<String> = s
        .bus_chain("Drums")
        .iter()
        .map(|e| e.kind.clone())
        .collect();
    assert_eq!(kinds, ["limiter", "gain"]);
}

/// Position puts the effect where asked, as on a track.
#[test]
fn position_inserts_into_the_bus_chain() {
    let (mut s, _) = Session::with_bus("Drums");
    s.add_bus("Drums", "gain", json!({ "db": -1.0 }));
    s.add_bus("Drums", "limiter", json!({}));
    let v = ok(s.call(
        "add_effect",
        json!({ "scope": "bus", "bus": "Drums", "kind": "high_pass_filter", "position": 0 }),
    ));
    assert_eq!(v["position"], 0, "{v}");
    assert_eq!(v["effect_count"], 3, "{v}");
    let kinds: Vec<String> = s
        .bus_chain("Drums")
        .iter()
        .map(|e| e.kind.clone())
        .collect();
    assert_eq!(kinds, ["high_pass_filter", "gain", "limiter"]);
}

// ─── finding the bus ──────────────────────────────────────────────────

/// A bus is found by the name it was created with, and only that bus's
/// chain changes.
#[test]
fn the_named_bus_is_the_one_edited() {
    let mut s = Session::two_quiet_tracks();
    s.create_bus("Drums");
    s.create_bus("Room");

    s.add_bus("Room", "gain", json!({ "db": -3.0 }));
    assert!(s.bus_chain("Drums").is_empty());
    assert_eq!(s.bus_chain("Room").len(), 1);

    s.add_bus("Drums", "limiter", json!({}));
    assert_eq!(s.bus_chain("Drums")[0].kind, "limiter");
    assert_eq!(s.bus_chain("Room")[0].kind, "gain");
}

/// Every chain tool says which bus it edited, in words.
#[test]
fn the_summaries_name_the_bus() {
    let (mut s, _) = Session::with_bus("Drums");
    let added = s.add_bus("Drums", "limiter", json!({ "ceiling_db": -1.0 }));
    let set = ok(s.call(
        "set_effect_params",
        json!({
            "scope": "bus", "bus": "Drums", "effect_index": 0,
            "params": { "ceiling_db": -2.0 }
        }),
    ));
    let bypassed = ok(s.call(
        "set_effect_bypassed",
        json!({ "scope": "bus", "bus": "Drums", "effect_index": 0, "bypassed": true }),
    ));
    let reordered = ok(s.call(
        "reorder_effects",
        json!({ "scope": "bus", "bus": "Drums", "order": [0] }),
    ));
    let removed = ok(s.call(
        "remove_effect",
        json!({ "scope": "bus", "bus": "Drums", "effect_index": 0 }),
    ));
    for (tool, v) in [
        ("add_effect", added),
        ("set_effect_params", set),
        ("set_effect_bypassed", bypassed),
        ("reorder_effects", reordered),
        ("remove_effect", removed),
    ] {
        let summary = v["summary"].as_str().unwrap();
        assert!(
            summary.contains("bus") && summary.contains("Drums"),
            "{tool}: the summary should name the bus: {summary}"
        );
        assert!(
            !summary.contains("track"),
            "{tool}: a bus edit should not read as a track edit: {summary}"
        );
    }
}

/// A name that is not a bus is refused, with the buses the session has so
/// the call can be repeated right. For each of the five tools, because
/// each resolves its own arguments.
#[test]
fn an_unknown_bus_is_refused_and_the_buses_are_listed() {
    let mut s = Session::two_quiet_tracks();
    let drums = s.create_bus("Drums");
    s.create_bus("Room");
    s.add_bus("Drums", "gain", json!({}));
    let nodes = s.node_count();
    let head = s.head();

    let calls: [(&str, Value); 5] = [
        ("add_effect", json!({ "kind": "limiter" })),
        ("remove_effect", json!({ "effect_index": 0 })),
        ("reorder_effects", json!({ "order": [0] })),
        (
            "set_effect_params",
            json!({ "effect_index": 0, "params": {} }),
        ),
        (
            "set_effect_bypassed",
            json!({ "effect_index": 0, "bypassed": true }),
        ),
    ];
    for (tool, mut args) in calls {
        args["scope"] = json!("bus");
        args["bus"] = json!("Reverb");
        let msg = err(s.call(tool, args));
        assert!(
            msg.contains("Reverb"),
            "{tool}: should say which name was not found: {msg}"
        );
        assert!(
            msg.contains("Drums") && msg.contains("Room") && msg.contains(&drums),
            "{tool}: should list the session's buses, with their ids: {msg}"
        );
    }
    assert_eq!(s.node_count(), nodes, "a refused call appended a node");
    assert_eq!(s.head(), head);
}

/// A session with no bus at all is told to make one, not shown an empty
/// list.
#[test]
fn a_session_with_no_buses_says_to_create_one() {
    let mut s = Session::two_quiet_tracks();
    let msg = err(s.call(
        "add_effect",
        json!({ "scope": "bus", "bus": "Drums", "kind": "gain" }),
    ));
    assert!(
        msg.contains("no buses") && msg.contains("create_bus"),
        "{msg}"
    );
}

/// Bus names are matched exactly, as kinds are: a near miss is an unknown
/// bus, and the list shows the right spelling.
#[test]
fn a_bus_name_is_matched_exactly() {
    let mut s = Session::two_quiet_tracks();
    s.create_bus("Drums");
    for wrong in ["drums", "Drums ", "Drum", ""] {
        let msg = err(s.call(
            "add_effect",
            json!({ "scope": "bus", "bus": wrong, "kind": "gain" }),
        ));
        assert!(msg.contains("\"Drums\""), "{wrong:?}: {msg}");
    }
    assert!(s.bus_chain("Drums").is_empty());
}

/// Names are not unique: `create_bus` takes any name. Two buses with one
/// name cannot be told apart by it, so the call is refused rather than
/// guessed, and the id `create_bus` returned names the one meant.
#[test]
fn two_buses_with_one_name_are_told_apart_by_id() {
    let mut s = Session::two_quiet_tracks();
    let first = s.create_bus("Verb");
    let second = s.create_bus("Verb");
    let nodes = s.node_count();

    let msg = err(s.call(
        "add_effect",
        json!({ "scope": "bus", "bus": "Verb", "kind": "gain" }),
    ));
    assert!(
        msg.contains("2 buses") && msg.contains(&first) && msg.contains(&second),
        "an ambiguous name should list both ids: {msg}"
    );
    assert_eq!(s.node_count(), nodes);

    ok(s.call(
        "add_effect",
        json!({ "scope": "bus", "bus": second, "kind": "limiter" }),
    ));
    let buses = s.state().bus_routing.buses;
    assert!(buses[0].effects.is_empty(), "the first bus was edited");
    assert_eq!(buses[1].effects.len(), 1, "the second bus was not edited");

    // An id also finds a bus whose name is unique.
    let drums = s.create_bus("Drums");
    ok(s.call(
        "add_effect",
        json!({ "scope": "bus", "bus": drums, "kind": "gain" }),
    ));
    assert_eq!(s.bus_chain("Drums").len(), 1);
}

// ─── the arguments ────────────────────────────────────────────────────

/// `scope`, `track` and `bus` name one chain between them. Every
/// combination that does not is refused with what to pass instead, and
/// none is resolved by guessing.
#[test]
fn a_target_that_is_not_one_chain_is_refused_with_what_to_pass() {
    let mut s = Session::two_quiet_tracks();
    s.create_bus("Drums");
    let nodes = s.node_count();

    let cases: [(Value, &[&str]); 6] = [
        // Nothing named: every way to name a chain is offered.
        (json!({ "kind": "gain" }), &["track", "master", "bus"]),
        // A bus scope with no bus.
        (
            json!({ "scope": "bus", "kind": "gain" }),
            &["needs `bus`", "create_bus"],
        ),
        // A bus has no track index.
        (
            json!({ "scope": "bus", "bus": "Drums", "track": 0, "kind": "gain" }),
            &["no track index"],
        ),
        // The master bus is not a named bus.
        (
            json!({ "scope": "master", "bus": "Drums", "kind": "gain" }),
            &["master", "drop `bus`"],
        ),
        // A track's chain is not a bus's.
        (
            json!({ "scope": "track", "track": 0, "bus": "Drums", "kind": "gain" }),
            &["scope \"track\"", "bus"],
        ),
        // Both a track and a bus, no scope to say which.
        (
            json!({ "track": 0, "bus": "Drums", "kind": "gain" }),
            &["either", "track", "bus"],
        ),
    ];
    for (args, wants) in cases {
        let msg = err(s.call("add_effect", args.clone()));
        for want in wants {
            assert!(msg.contains(want), "{args}: expected {want:?} in: {msg}");
        }
    }
    assert_eq!(s.node_count(), nodes, "a refused call appended a node");
    assert!(s.bus_chain("Drums").is_empty());
    assert!(s.state().tracks[0].effects.is_empty());
    assert!(s.state().master_chain.is_empty());
}

/// `bus` alone is a bus, as `track` alone is a track: it can mean nothing
/// else.
#[test]
fn a_bus_alone_is_that_buses_chain() {
    let mut s = Session::two_quiet_tracks();
    s.create_bus("Drums");
    ok(s.call("add_effect", json!({ "bus": "Drums", "kind": "gain" })));
    assert_eq!(s.bus_chain("Drums").len(), 1);
}

/// The scope enum takes `bus` now, and still turns a scope the tools do
/// not have (a typo) away before any tool runs.
#[test]
fn the_scope_enum_knows_bus_and_nothing_made_up() {
    let mut s = Session::two_quiet_tracks();
    s.create_bus("Drums");
    let nodes = s.node_count();
    for scope in ["Bus", "buses", "mix", ""] {
        match s.try_call(
            "add_effect",
            json!({ "scope": scope, "bus": "Drums", "kind": "gain" }),
        ) {
            Err(DispatchError::SchemaValidation(m)) => {
                assert!(m.contains("scope"), "scope {scope:?}: {m}")
            }
            other => panic!("scope {scope:?} should fail the schema, got {other:?}"),
        }
    }
    assert_eq!(s.node_count(), nodes);
}

/// An index past the end of a bus chain is refused with the range, and an
/// empty chain reads properly.
#[test]
fn an_effect_index_past_a_bus_chain_names_the_range() {
    let mut s = Session::two_quiet_tracks();
    s.create_bus("Drums");

    let msg = err(s.call(
        "remove_effect",
        json!({ "scope": "bus", "bus": "Drums", "effect_index": 0 }),
    ));
    assert!(
        msg.contains("Drums") && msg.contains("0 effects"),
        "an empty chain should say so: {msg}"
    );

    s.add_bus("Drums", "limiter", json!({}));
    let nodes = s.node_count();
    let calls: [(&str, Value); 3] = [
        (
            "remove_effect",
            json!({ "scope": "bus", "bus": "Drums", "effect_index": 3 }),
        ),
        (
            "set_effect_params",
            json!({
                "scope": "bus", "bus": "Drums", "effect_index": 3,
                "params": { "ceiling_db": -1 }
            }),
        ),
        (
            "set_effect_bypassed",
            json!({ "scope": "bus", "bus": "Drums", "effect_index": 3, "bypassed": true }),
        ),
    ];
    for (tool, args) in calls {
        let msg = err(s.call(tool, args));
        assert!(
            msg.contains("index 3") && msg.contains("Drums") && msg.contains("1 effect"),
            "{tool}: {msg}"
        );
    }
    assert_eq!(s.node_count(), nodes);
}

/// The permutation check is the track tool's, applied to a bus chain.
#[test]
fn a_bus_reorder_must_be_a_permutation() {
    let mut s = Session::two_quiet_tracks();
    s.create_bus("Drums");
    s.add_bus("Drums", "gain", json!({ "db": -1.0 }));
    s.add_bus("Drums", "limiter", json!({}));
    let nodes = s.node_count();

    let short = err(s.call(
        "reorder_effects",
        json!({ "scope": "bus", "bus": "Drums", "order": [0] }),
    ));
    assert!(short.contains("all 2 effects"), "{short}");
    let repeated = err(s.call(
        "reorder_effects",
        json!({ "scope": "bus", "bus": "Drums", "order": [1, 1] }),
    ));
    assert!(repeated.contains("more than once"), "{repeated}");
    let out_of_range = err(s.call(
        "reorder_effects",
        json!({ "scope": "bus", "bus": "Drums", "order": [0, 7] }),
    ));
    assert!(out_of_range.contains("out of range"), "{out_of_range}");
    assert_eq!(s.node_count(), nodes);
}

// ─── the kind ─────────────────────────────────────────────────────────

/// A kind the renderer cannot stream is refused on a bus as it is on a
/// track and on the master, with the kinds that work in the message, and
/// nothing is written.
#[test]
fn add_effect_on_a_bus_refuses_each_kind_the_renderer_cannot_stream() {
    let (mut s, _) = Session::with_bus("Drums");
    let head = s.head();
    let nodes = s.node_count();

    for kind in NOT_YET_STREAMABLE {
        let msg = err(s.call(
            "add_effect",
            json!({ "scope": "bus", "bus": "Drums", "kind": kind }),
        ));
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
        assert!(
            s.bus_chain("Drums").is_empty(),
            "{kind}: chain must stay empty"
        );
    }

    // The session still renders.
    s.render();
}

#[test]
fn add_effect_on_a_bus_refuses_an_unknown_kind_and_lists_what_works() {
    let (mut s, _) = Session::with_bus("Drums");
    let nodes = s.node_count();
    for kind in ["wobbulator", "Reverb", "GAIN", ""] {
        let msg = err(s.call(
            "add_effect",
            json!({ "scope": "bus", "bus": "Drums", "kind": kind }),
        ));
        assert!(msg.contains("unknown effect kind"), "{kind:?}: {msg}");
        for works in STREAMABLE {
            assert!(msg.contains(works), "{kind:?}: {msg}");
        }
    }
    assert_eq!(s.node_count(), nodes);
    assert!(s.bus_chain("Drums").is_empty());
}

/// The refusal does not reach what works: every kind the renderer can run
/// is accepted on a bus with no parameters, and a chain of all of them
/// renders.
#[test]
fn every_working_kind_is_accepted_on_a_bus_and_renders() {
    let (mut s, _) = Session::with_bus("Drums");
    for kind in STREAMABLE {
        s.add_bus("Drums", kind, json!({}));
    }
    assert_eq!(s.bus_chain("Drums").len(), STREAMABLE.len());
    assert_eq!(
        audio_engine::effect_chain::streamable_kinds().count(),
        STREAMABLE.len(),
        "the renderer gained a kind this test does not know"
    );
    s.render();
}

/// A project from before the kind was checked can hold a bypassed effect
/// the renderer cannot stream on a bus. It cannot be switched back on (it
/// would be left out of the render and do nothing), but it can be
/// bypassed and removed.
#[test]
fn a_stored_unrunnable_bus_effect_cannot_be_switched_on() {
    let (mut s, _) = Session::with_bus("Drums");
    s.plant_bus(
        "Drums",
        EffectInstance {
            kind: "reverb".into(),
            params: json!({}),
            bypassed: true,
        },
    );
    let nodes = s.node_count();
    let head = s.head();

    let msg = err(s.call(
        "set_effect_bypassed",
        json!({ "scope": "bus", "bus": "Drums", "effect_index": 0, "bypassed": false }),
    ));
    assert!(
        msg.contains("reverb") && msg.contains("stays bypassed"),
        "{msg}"
    );
    assert_eq!(s.head(), head);
    assert_eq!(s.node_count(), nodes);
    assert!(s.bus_chain("Drums")[0].bypassed);

    // Bypassing stays allowed.
    ok(s.call(
        "set_effect_bypassed",
        json!({ "scope": "bus", "bus": "Drums", "effect_index": 0, "bypassed": true }),
    ));
}

/// The way out the render warning offers ("remove or bypass the effect to
/// clear this warning") now exists for a bus: the warning names the bus
/// and the stored index, and `remove_effect` on that bus clears it.
#[test]
fn a_stored_unrunnable_bus_effect_is_named_by_the_render_and_removable() {
    let (mut s, _) = Session::with_bus("Drums");
    s.add_bus("Drums", "gain", json!({ "db": 0.0 }));
    s.plant_bus(
        "Drums",
        EffectInstance {
            kind: "reverb".into(),
            params: json!({}),
            bypassed: false,
        },
    );

    let preview = ok(s.call("render_preview", json!({ "node_id": s.head().to_hex() })));
    let warnings = preview["warnings"].as_array().expect("a warning");
    assert_eq!(warnings.len(), 1, "{preview}");
    let w = warnings[0].as_str().unwrap();
    assert!(
        w.contains("bus \"Drums\"") && w.contains("effect 1") && w.contains("reverb"),
        "{w}"
    );

    ok(s.call(
        "remove_effect",
        json!({ "scope": "bus", "bus": "Drums", "effect_index": 1 }),
    ));
    let preview = ok(s.call("render_preview", json!({ "node_id": s.head().to_hex() })));
    assert!(
        preview.get("warnings").is_none(),
        "removing the entry should clear the warning: {preview}"
    );
    assert_eq!(s.bus_chain("Drums").len(), 1);
}

// ─── undo ─────────────────────────────────────────────────────────────

/// An edit to a bus chain is a node like any other: a new head, parented
/// on the one before, so undo (moving the head back) and redo work and the
/// mix follows.
#[test]
fn a_bus_edit_is_an_undoable_node() {
    let (mut s, _) = Session::with_bus("Drums");
    let before = s.head();
    let routed = s.render();

    let v = s.add_bus("Drums", "gain", json!({ "db": 6.0 }));
    let after = s.head();
    assert_ne!(after, before, "the edit did not move the head");
    assert_eq!(v["node_id"], after.to_hex());
    assert_eq!(
        s.store.get(after).unwrap().parent,
        Some(before),
        "the new node should hang off the head it was made from"
    );
    let boosted = s.render();
    assert_ne!(boosted, routed);

    s.store.set_head(before).expect("undo");
    assert!(s.bus_chain("Drums").is_empty());
    assert_eq!(s.render(), routed, "undo left the effect on the mix");

    s.store.set_head(after).expect("redo");
    assert_eq!(s.bus_chain("Drums").len(), 1);
    assert_eq!(s.render(), boosted);

    ok(s.call("revert_to", json!({ "target": before.to_hex() })));
    assert_eq!(s.head(), before);
    assert!(s.bus_chain("Drums").is_empty());
}

/// Each of the five tools appends exactly one node.
#[test]
fn every_bus_chain_tool_appends_one_node() {
    let (mut s, _) = Session::with_bus("Drums");
    let steps: [(&str, Value); 5] = [
        (
            "add_effect",
            json!({ "scope": "bus", "bus": "Drums", "kind": "gain", "params": { "db": -3.0 } }),
        ),
        (
            "add_effect",
            json!({
                "scope": "bus", "bus": "Drums", "kind": "limiter",
                "params": { "ceiling_db": -1.0 }
            }),
        ),
        (
            "set_effect_params",
            json!({
                "scope": "bus", "bus": "Drums", "effect_index": 1,
                "params": { "ceiling_db": -2.0 }
            }),
        ),
        (
            "reorder_effects",
            json!({ "scope": "bus", "bus": "Drums", "order": [1, 0] }),
        ),
        (
            "set_effect_bypassed",
            json!({ "scope": "bus", "bus": "Drums", "effect_index": 0, "bypassed": true }),
        ),
    ];
    for (tool, args) in steps {
        let nodes = s.node_count();
        let prior = s.head();
        ok(s.call(tool, args));
        assert_eq!(s.node_count(), nodes + 1, "{tool} should append one node");
        assert_ne!(s.head(), prior, "{tool} should move the head");
    }
    // Node ids hash the state alone, so a step that lands on a state the
    // session has already been in adds no node; index 1 leaves a bypassed
    // gain after the limiter, which is new.
    let nodes = s.node_count();
    ok(s.call(
        "remove_effect",
        json!({ "scope": "bus", "bus": "Drums", "effect_index": 1 }),
    ));
    assert_eq!(s.node_count(), nodes + 1);
}

/// The session layer sees the edit as a bus-scope effect change, so
/// `compare_nodes`, `apply_diff` and merges treat it as they treat a track
/// effect.
#[test]
fn the_edit_shows_up_as_a_bus_scope_effect_in_the_diff() {
    let (mut s, bus_id) = Session::with_bus("Drums");
    let before = s.head();
    s.add_bus("Drums", "limiter", json!({ "ceiling_db": -1.0 }));

    let diff = s.store.diff(before, s.head()).expect("diff");

    assert_eq!(diff.added.len(), 1, "{diff:?}");
    match &diff.added[0] {
        DiffOp::Effect {
            scope,
            index,
            effect,
        } => {
            match scope {
                EffectScope::Bus { bus_id: id } => assert_eq!(id.to_string(), bus_id),
                other => panic!("expected a bus scope, got {other:?}"),
            }
            assert_eq!(*index, 0);
            assert_eq!(effect.kind, "limiter");
        }
        other => panic!("expected a bus-scope effect, got {other:?}"),
    }
    assert!(
        diff.removed.is_empty() && diff.modified.is_empty(),
        "{diff:?}"
    );
}

// ─── the chains are separate ──────────────────────────────────────────

/// A track's chain and the master's work exactly as before, and no chain
/// reaches into another.
#[test]
fn track_master_and_bus_chains_are_independent() {
    let (mut s, _) = Session::with_bus("Drums");

    ok(s.call(
        "add_effect",
        json!({ "track": 1, "kind": "gain", "params": { "db": -3.0 } }),
    ));
    ok(s.call(
        "add_effect",
        json!({ "scope": "master", "kind": "limiter" }),
    ));
    assert!(
        s.bus_chain("Drums").is_empty(),
        "a track or master edit reached a bus"
    );

    // A bus edit leaves the tracks and the master alone.
    let tracks_before = serde_json::to_value(s.state().tracks).unwrap();
    let master_before = serde_json::to_value(s.state().master_chain).unwrap();
    s.add_bus("Drums", "gain", json!({ "db": 3.0 }));
    assert_eq!(
        serde_json::to_value(s.state().tracks).unwrap(),
        tracks_before,
        "a bus edit changed a track"
    );
    assert_eq!(
        serde_json::to_value(s.state().master_chain).unwrap(),
        master_before,
        "a bus edit changed the master chain"
    );

    // And removing from the bus leaves the other two.
    ok(s.call(
        "remove_effect",
        json!({ "scope": "bus", "bus": "Drums", "effect_index": 0 }),
    ));
    assert_eq!(s.state().master_chain.len(), 1);
    assert_eq!(s.state().tracks[1].effects.len(), 1);

    // A track index past the end is still refused as before.
    let msg = err(s.call("add_effect", json!({ "track": 9, "kind": "gain" })));
    assert!(msg.contains("track index 9 out of range"), "{msg}");
}
