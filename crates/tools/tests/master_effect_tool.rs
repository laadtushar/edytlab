//! The agent can put an effect on the master bus (#460).
//!
//! `SessionState::master_chain` has been in the session, in the diff
//! layer (`EffectScope::Master`) and in the renderer since #110, and
//! `master_chain_render.rs` proves a limiter on it reaches the audio.
//! But no tool wrote to it, so asked for "a limiter on the master at
//! -1 dB" the agent rendered every track into a new "Master" track,
//! muted the sources and limited that. It works, and it is baked: a later
//! edit to a source track is silently absent from the "Master" until
//! someone re-renders it, which is the opposite of a non-destructive
//! session.
//!
//! The five chain tools (`add_effect`, `remove_effect`, `reorder_effects`,
//! `set_effect_params`, `set_effect_bypassed`) take `scope: "master"` in
//! place of a `track`. These tests drive them through the dispatcher and
//! check four things:
//!
//! * what reaches the **samples**: the limiter bounds the rendered mix, a
//!   bypassed or removed one renders byte-identically to none, and a gain
//!   change on a source track after the limiter was added is limited too,
//!   which is the thing a baked master track gets wrong;
//! * that the **arguments are validated**, with errors that say what to
//!   pass instead, and that a refused call appends nothing;
//! * that every edit is an ordinary **undoable node**;
//! * that a track's chain is untouched by a master edit, and the other
//!   way round.

use std::path::{Path, PathBuf};

use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
use serde_json::{json, Value};
use session::{DiffOp, EffectScope};
use tempfile::TempDir;
use tools::{DispatchError, ToolContext, ToolDispatcher, ToolResult};

const SAMPLE_RATE: u32 = 44_100;
const FRAMES: usize = 44_100;

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

/// A two-track session: 220 Hz and 330 Hz at 0.6 each. Their peaks line
/// up every 1/110 s, so the sum reaches about 1.2 and the mix clips,
/// which is the situation a limiter on the master is asked for in.
struct Session {
    dir: TempDir,
    store: session::Store,
    engine: audio_engine::Engine,
    dispatcher: ToolDispatcher,
    clipboard: Option<tools::Clipboard>,
    renders: usize,
}

impl Session {
    fn two_loud_tracks() -> Self {
        let dir = TempDir::new().expect("tempdir");
        let a = write_tone(dir.path(), "a.wav", 220.0, 0.6);
        let b = write_tone(dir.path(), "b.wav", 330.0, 0.6);
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

    fn add_master(&mut self, kind: &str, params: Value) -> Value {
        ok(self.call(
            "add_effect",
            json!({ "scope": "master", "kind": kind, "params": params }),
        ))
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

fn peak(pcm: &[i16]) -> f32 {
    pcm.iter().map(|v| (*v as f32).abs()).fold(0.0, f32::max)
}

fn rms(pcm: &[i16]) -> f32 {
    (pcm.iter().map(|v| (*v as f32).powi(2)).sum::<f32>() / pcm.len() as f32).sqrt()
}

/// Full-scale sample value of a dBFS level.
fn level(db: f32) -> f32 {
    10.0f32.powf(db / 20.0) * 32_767.0
}

// ─── the audio ────────────────────────────────────────────────────────

/// The ticket: "a limiter on the master" is one call, and the mix that
/// comes out is limited. Compared against the same session rendered
/// before the call, sample by sample.
#[test]
fn a_limiter_added_to_the_master_bounds_the_rendered_mix() {
    let mut s = Session::two_loud_tracks();
    let dry = s.render();
    assert!(
        peak(&dry) > level(-3.0),
        "the premise failed: the dry mix peaks at {} and never needs limiting",
        peak(&dry)
    );

    let v = s.add_master("limiter", json!({ "ceiling_db": -6.0 }));
    assert_eq!(v["effect_count"], 1, "{v}");
    let limited = s.render();

    assert_eq!(limited.len(), dry.len());
    let ceiling = level(-6.0);
    assert!(
        peak(&limited) <= ceiling + 2.0,
        "peak {} exceeds the -6 dBFS ceiling {ceiling}: the master effect did not reach the audio",
        peak(&limited)
    );
    assert!(
        peak(&limited) >= ceiling - 4.0,
        "peak {} is well under the ceiling {ceiling}: the limiter over-reduced",
        peak(&limited)
    );
    // A limiter turns the gain down and adds nothing: no sample may be
    // louder than the mix it came from.
    let louder = limited
        .iter()
        .zip(&dry)
        .filter(|(l, d)| (**l as i32).abs() > (**d as i32).abs() + 1)
        .count();
    assert_eq!(
        louder, 0,
        "{louder} samples came out louder than they went in"
    );
    assert_ne!(
        limited, dry,
        "the render is unchanged: the chain was ignored"
    );
}

/// The defect the baked "Master" track had. The limiter is on the bus,
/// not on a snapshot of the tracks, so a track edited afterwards is
/// still limited. Rendering the same session without the effect shows
/// the change really did make the mix too loud.
#[test]
fn a_gain_change_on_a_source_track_after_the_limiter_is_still_limited() {
    let mut s = Session::two_loud_tracks();
    s.add_master("limiter", json!({ "ceiling_db": -6.0 }));
    let before = s.render();

    ok(s.call("set_track_gain", json!({ "track": 0, "db": 6.0 })));
    let after = s.render();

    let ceiling = level(-6.0);
    assert!(
        peak(&after) <= ceiling + 2.0,
        "peak {} exceeds the ceiling {ceiling} after a source track was turned up: \
         the master is a snapshot, not a bus",
        peak(&after)
    );
    assert_ne!(after, before, "the gain change never reached the mix");
    assert!(
        rms(&after) > rms(&before),
        "turning a track up should make the limited mix denser: {} vs {}",
        rms(&after),
        rms(&before)
    );

    // Control: the same session with the chain removed is far over the
    // ceiling, so the bound above is the limiter's doing.
    ok(s.call(
        "remove_effect",
        json!({ "scope": "master", "effect_index": 0 }),
    ));
    assert!(
        peak(&s.render()) > ceiling * 1.5,
        "without the limiter the louder mix should be well over the ceiling"
    );
}

/// A bypassed effect renders as though it were absent, byte for byte,
/// and so does a removed one.
#[test]
fn bypassing_or_removing_a_master_effect_renders_the_dry_mix_exactly() {
    let mut s = Session::two_loud_tracks();
    let dry = s.render();
    s.add_master("limiter", json!({ "ceiling_db": -6.0 }));
    assert_ne!(s.render(), dry);

    ok(s.call(
        "set_effect_bypassed",
        json!({ "scope": "master", "effect_index": 0, "bypassed": true }),
    ));
    assert_eq!(s.render(), dry, "a bypassed master effect changed the mix");

    ok(s.call(
        "set_effect_bypassed",
        json!({ "scope": "master", "effect_index": 0, "bypassed": false }),
    ));
    assert_ne!(
        s.render(),
        dry,
        "re-enabling it should bring the limiter back"
    );

    ok(s.call(
        "remove_effect",
        json!({ "scope": "master", "effect_index": 0 }),
    ));
    assert_eq!(s.render(), dry, "a removed master effect changed the mix");
    assert!(s.state().master_chain.is_empty());
}

/// Editing a master effect's parameters is heard, and merges rather than
/// replaces, as it does on a track.
#[test]
fn master_effect_params_are_editable_in_place() {
    let mut s = Session::two_loud_tracks();
    s.add_master("limiter", json!({ "ceiling_db": -6.0, "release_ms": 80.0 }));
    let at_six = peak(&s.render());

    let v = ok(s.call(
        "set_effect_params",
        json!({ "scope": "master", "effect_index": 0, "params": { "ceiling_db": -12.0 } }),
    ));
    assert_eq!(v["params"]["ceiling_db"], -12.0, "{v}");
    assert_eq!(
        v["params"]["release_ms"], 80.0,
        "a merge must keep the keys it was not given: {v}"
    );

    let at_twelve = peak(&s.render());
    assert!(
        at_twelve <= level(-12.0) + 2.0 && at_twelve < at_six - 1_000.0,
        "lowering the ceiling from -6 to -12 dBFS should lower the peak: {at_six} -> {at_twelve}"
    );
}

/// Order is part of the sound: boosting into a limiter is not limiting
/// and then boosting.
#[test]
fn reordering_the_master_chain_changes_the_mix() {
    let mut s = Session::two_loud_tracks();
    s.add_master("gain", json!({ "db": 6.0 }));
    s.add_master("limiter", json!({ "ceiling_db": -6.0 }));
    let gain_then_limit = s.render();
    assert!(peak(&gain_then_limit) <= level(-6.0) + 2.0);

    ok(s.call(
        "reorder_effects",
        json!({ "scope": "master", "order": [1, 0] }),
    ));
    let limit_then_gain = s.render();

    assert_ne!(gain_then_limit, limit_then_gain);
    assert!(
        peak(&limit_then_gain) > level(-6.0) + 1_000.0,
        "a gain after the limiter should push the mix back over its ceiling"
    );
    let kinds: Vec<String> = s
        .state()
        .master_chain
        .iter()
        .map(|e| e.kind.clone())
        .collect();
    assert_eq!(kinds, ["limiter", "gain"]);
}

// ─── the arguments ────────────────────────────────────────────────────

/// Position puts the effect where asked, as on a track.
#[test]
fn position_inserts_into_the_master_chain() {
    let mut s = Session::two_loud_tracks();
    s.add_master("gain", json!({ "db": -1.0 }));
    s.add_master("limiter", json!({}));
    let v = ok(s.call(
        "add_effect",
        json!({ "scope": "master", "kind": "high_pass_filter", "position": 0 }),
    ));
    assert_eq!(v["position"], 0, "{v}");
    assert_eq!(v["effect_count"], 3, "{v}");
    let kinds: Vec<String> = s
        .state()
        .master_chain
        .iter()
        .map(|e| e.kind.clone())
        .collect();
    assert_eq!(kinds, ["high_pass_filter", "gain", "limiter"]);
}

/// Every chain tool says what it edited, in words, so the transcript
/// and the summary the model reads do not read as a track edit.
#[test]
fn the_summaries_name_the_master_bus() {
    let mut s = Session::two_loud_tracks();
    let added = s.add_master("limiter", json!({ "ceiling_db": -1.0 }));
    assert!(
        added["summary"].as_str().unwrap().contains("master"),
        "{added}"
    );
    let set = ok(s.call(
        "set_effect_params",
        json!({ "scope": "master", "effect_index": 0, "params": { "ceiling_db": -2.0 } }),
    ));
    assert!(set["summary"].as_str().unwrap().contains("master"), "{set}");
    let bypassed = ok(s.call(
        "set_effect_bypassed",
        json!({ "scope": "master", "effect_index": 0, "bypassed": true }),
    ));
    assert!(
        bypassed["summary"].as_str().unwrap().contains("master"),
        "{bypassed}"
    );
    let removed = ok(s.call(
        "remove_effect",
        json!({ "scope": "master", "effect_index": 0 }),
    ));
    assert!(
        removed["summary"].as_str().unwrap().contains("master"),
        "{removed}"
    );
}

/// A call that names no target is refused with what to pass, for each of
/// the five tools. Each is checked, because each resolves its own
/// arguments.
#[test]
fn a_call_that_names_no_chain_says_how_to_name_one() {
    let mut s = Session::two_loud_tracks();
    s.add_master("limiter", json!({}));
    let nodes = s.node_count();

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
    for (tool, args) in calls {
        let msg = err(s.call(tool, args));
        assert!(
            msg.contains("scope") && msg.contains("master") && msg.contains("track"),
            "{tool}: the error should say to pass `track` or scope \"master\": {msg}"
        );
    }
    assert_eq!(s.node_count(), nodes, "a refused call appended a node");
}

/// `track` is a track's index and the master bus has none, so giving
/// both is a mistake the call would otherwise resolve by guessing.
#[test]
fn a_track_index_with_scope_master_is_refused() {
    let mut s = Session::two_loud_tracks();
    let nodes = s.node_count();
    let msg = err(s.call(
        "add_effect",
        json!({ "scope": "master", "track": 0, "kind": "limiter" }),
    ));
    assert!(
        msg.contains("master") && msg.contains("track"),
        "the error should explain the conflict: {msg}"
    );
    assert_eq!(s.node_count(), nodes);
    assert!(s.state().master_chain.is_empty());
    assert!(s.state().tracks[0].effects.is_empty());
}

#[test]
fn scope_track_needs_a_track() {
    let mut s = Session::two_loud_tracks();
    let msg = err(s.call("add_effect", json!({ "scope": "track", "kind": "limiter" })));
    assert!(msg.contains("track"), "{msg}");
}

/// The scope is an enum in the schema, so a scope the tools do not have
/// (a bus, a typo) is turned away by the dispatcher before any tool runs.
#[test]
fn an_unknown_scope_is_rejected_by_the_schema() {
    let mut s = Session::two_loud_tracks();
    let nodes = s.node_count();
    for scope in ["bus", "Master", "mix", ""] {
        match s.try_call("add_effect", json!({ "scope": scope, "kind": "limiter" })) {
            Err(DispatchError::SchemaValidation(m)) => {
                assert!(m.contains("scope"), "scope {scope:?}: {m}")
            }
            other => panic!("scope {scope:?} should fail the schema, got {other:?}"),
        }
    }
    assert_eq!(s.node_count(), nodes);
}

/// An index past the end of the master chain is refused with the range,
/// and an empty chain reads properly.
#[test]
fn an_effect_index_past_the_master_chain_names_the_range() {
    let mut s = Session::two_loud_tracks();

    let msg = err(s.call(
        "remove_effect",
        json!({ "scope": "master", "effect_index": 0 }),
    ));
    assert!(
        msg.contains("master") && msg.contains("0 effects"),
        "an empty chain should say so: {msg}"
    );

    s.add_master("limiter", json!({}));
    let nodes = s.node_count();
    let calls: [(&str, Value); 3] = [
        (
            "remove_effect",
            json!({ "scope": "master", "effect_index": 3 }),
        ),
        (
            "set_effect_params",
            json!({ "scope": "master", "effect_index": 3, "params": { "ceiling_db": -1 } }),
        ),
        (
            "set_effect_bypassed",
            json!({ "scope": "master", "effect_index": 3, "bypassed": true }),
        ),
    ];
    for (tool, args) in calls {
        let msg = err(s.call(tool, args));
        assert!(
            msg.contains("index 3") && msg.contains("master") && msg.contains("1 effect"),
            "{tool}: {msg}"
        );
    }
    assert_eq!(s.node_count(), nodes);
}

/// `params` is an object in the schema, so anything else is turned away
/// by the dispatcher before the tool runs, on the master as on a track.
#[test]
fn set_effect_params_on_the_master_needs_an_object() {
    let mut s = Session::two_loud_tracks();
    s.add_master("limiter", json!({}));
    let nodes = s.node_count();
    match s.try_call(
        "set_effect_params",
        json!({ "scope": "master", "effect_index": 0, "params": 5 }),
    ) {
        Err(DispatchError::SchemaValidation(m)) => assert!(m.contains("params"), "{m}"),
        other => panic!("a non-object params should fail the schema, got {other:?}"),
    }
    assert_eq!(s.node_count(), nodes);
}

/// The permutation check is the track tool's, applied to the master
/// chain: a partial or repeated order would silently drop or duplicate
/// an effect.
#[test]
fn a_master_reorder_must_be_a_permutation() {
    let mut s = Session::two_loud_tracks();
    s.add_master("gain", json!({ "db": -1.0 }));
    s.add_master("limiter", json!({}));
    let nodes = s.node_count();

    let short = err(s.call(
        "reorder_effects",
        json!({ "scope": "master", "order": [0] }),
    ));
    assert!(short.contains("all 2 effects"), "{short}");
    let repeated = err(s.call(
        "reorder_effects",
        json!({ "scope": "master", "order": [1, 1] }),
    ));
    assert!(repeated.contains("more than once"), "{repeated}");
    let out_of_range = err(s.call(
        "reorder_effects",
        json!({ "scope": "master", "order": [0, 7] }),
    ));
    assert!(out_of_range.contains("out of range"), "{out_of_range}");
    assert_eq!(s.node_count(), nodes);
}

// ─── undo ─────────────────────────────────────────────────────────────

/// An edit to the master chain is a node like any other: a new head,
/// parented on the one before, so undo (moving the head back) and redo
/// work and the mix follows.
#[test]
fn a_master_edit_is_an_undoable_node() {
    let mut s = Session::two_loud_tracks();
    let before = s.head();
    let dry = s.render();

    let v = s.add_master("limiter", json!({ "ceiling_db": -6.0 }));
    let after = s.head();
    assert_ne!(after, before, "the edit did not move the head");
    assert_eq!(v["node_id"], after.to_hex());
    assert_eq!(
        s.store.get(after).unwrap().parent,
        Some(before),
        "the new node should hang off the head it was made from"
    );
    assert_eq!(s.state().master_chain.len(), 1);
    let limited = s.render();
    assert_ne!(limited, dry);

    // Undo: the head goes back, and the mix with it.
    s.store.set_head(before).expect("undo");
    assert!(s.state().master_chain.is_empty());
    assert_eq!(s.render(), dry, "undo left the limiter on the mix");

    // Redo.
    s.store.set_head(after).expect("redo");
    assert_eq!(s.state().master_chain.len(), 1);
    assert_eq!(s.render(), limited);

    // And the agent's own undo, `revert_to`, lands on the same node.
    ok(s.call("revert_to", json!({ "target": before.to_hex() })));
    assert_eq!(s.head(), before);
    assert!(s.state().master_chain.is_empty());
}

/// Each of the five tools appends exactly one node.
#[test]
fn every_master_chain_tool_appends_one_node() {
    let mut s = Session::two_loud_tracks();
    let steps: [(&str, Value); 5] = [
        (
            "add_effect",
            json!({ "scope": "master", "kind": "gain", "params": { "db": -3.0 } }),
        ),
        (
            "add_effect",
            json!({ "scope": "master", "kind": "limiter", "params": { "ceiling_db": -1.0 } }),
        ),
        (
            "set_effect_params",
            json!({ "scope": "master", "effect_index": 1, "params": { "ceiling_db": -2.0 } }),
        ),
        (
            "reorder_effects",
            json!({ "scope": "master", "order": [1, 0] }),
        ),
        (
            "set_effect_bypassed",
            json!({ "scope": "master", "effect_index": 0, "bypassed": true }),
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
    // session has already been in adds no node: removing index 0 here
    // would give back `[gain]`, which the first step made. Index 1 leaves
    // a bypassed limiter, which is new.
    let nodes = s.node_count();
    ok(s.call(
        "remove_effect",
        json!({ "scope": "master", "effect_index": 1 }),
    ));
    assert_eq!(s.node_count(), nodes + 1);
}

/// The session layer sees the edit as a master-scope effect change, so
/// `compare_nodes`, `apply_diff` and merges treat it as they treat a
/// track effect.
#[test]
fn the_edit_shows_up_as_a_master_scope_effect_in_the_diff() {
    let mut s = Session::two_loud_tracks();
    let before = s.head();
    s.add_master("limiter", json!({ "ceiling_db": -1.0 }));

    let diff = s.store.diff(before, s.head()).expect("diff");

    assert_eq!(diff.added.len(), 1, "{diff:?}");
    match &diff.added[0] {
        DiffOp::Effect {
            scope,
            index,
            effect,
        } => {
            assert_eq!(*scope, EffectScope::Master);
            assert_eq!(*index, 0);
            assert_eq!(effect.kind, "limiter");
        }
        other => panic!("expected a master-scope effect, got {other:?}"),
    }
    assert!(
        diff.removed.is_empty() && diff.modified.is_empty(),
        "{diff:?}"
    );
}

// ─── the two chains are separate ──────────────────────────────────────

/// A track's chain still works exactly as before, and neither chain
/// reaches into the other.
#[test]
fn track_and_master_chains_are_independent() {
    let mut s = Session::two_loud_tracks();

    // A call with only `track`, as every call written before the scope
    // existed.
    let v = ok(s.call(
        "add_effect",
        json!({ "track": 1, "kind": "gain", "params": { "db": -3.0 } }),
    ));
    assert!(
        v["summary"].as_str().unwrap().contains("track 1"),
        "a track edit should still say which track: {v}"
    );
    assert!(s.state().master_chain.is_empty());
    assert_eq!(s.state().tracks[1].effects.len(), 1);

    // `scope: "track"` spelled out is the same thing.
    ok(s.call(
        "add_effect",
        json!({ "scope": "track", "track": 0, "kind": "gain" }),
    ));
    assert_eq!(s.state().tracks[0].effects.len(), 1);

    // A master edit leaves every track alone.
    let tracks_before = serde_json::to_value(&s.state().tracks).unwrap();
    s.add_master("limiter", json!({}));
    assert_eq!(
        serde_json::to_value(&s.state().tracks).unwrap(),
        tracks_before,
        "a master edit changed a track"
    );
    assert_eq!(s.state().master_chain.len(), 1);

    // And removing from a track leaves the master chain.
    ok(s.call("remove_effect", json!({ "track": 1, "effect_index": 0 })));
    assert_eq!(s.state().master_chain.len(), 1);
    assert!(s.state().tracks[1].effects.is_empty());

    // A track index past the end is still refused as before.
    let msg = err(s.call("add_effect", json!({ "track": 9, "kind": "gain" })));
    assert!(msg.contains("track index 9 out of range"), "{msg}");
}
