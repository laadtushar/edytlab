//! A chain effect the renderer cannot stream is refused where it is added
//! (#520).
//!
//! `add_effect` used to store any kind it was given. The renderer streams
//! a chain a chunk at a time, and eleven kinds that exist as destructive
//! tools (reverb, echo, eq, compressor...) cannot run that way yet, so a
//! chain holding one failed every render until it was removed. (A stored
//! one is now left out of the render with a warning instead, which
//! `stored_effect_render_tools.rs` pins; an effect that is left out does
//! nothing, so it is still refused here.) These tests
//! pin that the refusal comes at the edit, names the kinds that do work so
//! the model can choose, and leaves the session exactly as it was.
//!
//! The two lists of kinds are written out in `support/effects.rs` on
//! purpose. The engine's registry is what the tool consults; spelling the
//! kinds there is what makes a change to either one a visible edit to a
//! test rather than a silent one.

use serde_json::{json, Value};
use session::EffectInstance;
use tools::{ToolDispatcher, ToolResult};

#[path = "support/effects.rs"]
mod effects;
#[path = "support/harness.rs"]
mod harness;
use effects::{NOT_YET_STREAMABLE, STREAMABLE};
use harness::{err, ok, write_sine, Session};

const SAMPLE_RATE: u32 = 48_000;

/// Half a second of a 300 Hz sine, loaded.
fn session() -> Session {
    let mut s = Session::empty();
    let src = write_sine(
        &s.dir.path().join("in.wav"),
        SAMPLE_RATE,
        300.0,
        0.3,
        SAMPLE_RATE as usize / 2,
    );
    s.load(&src);
    s
}

impl Session {
    /// The session renders: the question a stored effect decides.
    fn renders(&mut self) -> Result<Value, String> {
        let node_id = self.head_hex();
        match self.call("render_preview", json!({ "node_id": node_id })) {
            ToolResult::Ok(v) => Ok(v),
            ToolResult::Error(m) => Err(m),
        }
    }
}

/// **The ticket.** Every kind the renderer cannot stream is refused, with
/// the kinds that work in the message, and nothing is written.
#[test]
fn add_effect_refuses_each_kind_the_renderer_cannot_stream() {
    let mut s = session();
    let head = s.head_hex();
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

        assert_eq!(
            s.head_hex(),
            head,
            "{kind}: a refusal must not move the head"
        );
        assert_eq!(
            s.node_count(),
            nodes,
            "{kind}: a refusal must write no node"
        );
        assert!(
            s.state().tracks[0].effects.is_empty(),
            "{kind}: the chain must stay empty"
        );
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
/// be left out of every render exactly as an unstreamable one is.
#[test]
fn add_effect_refuses_an_unknown_kind_and_lists_what_works() {
    let mut s = session();
    let head = s.head_hex();

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
        assert_eq!(
            s.head_hex(),
            head,
            "{kind:?}: a refusal must not move the head"
        );
    }
}

/// The refusal must not reach the kinds that work: each is accepted with no
/// parameters, and a chain of all of them renders. This is the property the
/// tool and the renderer have to share, checked end to end.
#[test]
fn every_kind_add_effect_accepts_renders() {
    let mut s = session();
    for kind in STREAMABLE {
        ok(s.call("add_effect", json!({ "track": 0, "kind": kind })));
    }
    assert_eq!(s.state().tracks[0].effects.len(), STREAMABLE.len());
    s.renders()
        .unwrap_or_else(|e| panic!("a chain of accepted kinds failed to render: {e}"));
}

/// Bypass is a way into the chain too. A bypassed effect of a kind the
/// renderer cannot stream (an old project's, or a hand-edited one) renders
/// fine, because the renderer skips bypassed entries; switching it back on
/// would make every render fail, so that is refused.
#[test]
fn a_stored_unstreamable_effect_cannot_be_switched_back_on() {
    let mut s = session();
    s.plant(
        0,
        EffectInstance {
            kind: "reverb".into(),
            params: json!({}),
            bypassed: true,
        },
    );
    s.renders()
        .expect("a bypassed effect is skipped, so the session renders");
    let head = s.head_hex();

    let msg = err(s.call(
        "set_effect_bypassed",
        json!({ "track": 0, "effect_index": 0, "bypassed": false }),
    ));
    assert!(msg.contains("reverb"), "should name the kind: {msg}");
    for works in STREAMABLE {
        assert!(msg.contains(works), "should list `{works}`: {msg}");
    }
    assert_eq!(s.head_hex(), head, "a refusal must not move the head");
    assert!(
        s.state().tracks[0].effects[0].bypassed,
        "the effect must stay bypassed"
    );
}

/// Bypassing, and switching a streamable effect on and off, are untouched.
#[test]
fn bypass_still_works_for_every_kind_that_can_be_bypassed() {
    let mut s = session();
    s.plant(
        0,
        EffectInstance {
            kind: "reverb".into(),
            params: json!({}),
            bypassed: true,
        },
    );
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
/// A `kind` key in `params` is not a parameter of any effect (#579), so it is
/// refused outright, which is how a good effect stays a good effect; a
/// patch of real parameters beside it is not applied either.
#[test]
fn set_effect_params_cannot_change_an_effects_kind() {
    let mut s = session();
    ok(s.call(
        "add_effect",
        json!({ "track": 0, "kind": "gain", "params": { "db": -3.0 } }),
    ));
    let head = s.head_hex();

    for replace in [false, true] {
        let msg = err(s.call(
            "set_effect_params",
            json!({
                "track": 0,
                "effect_index": 0,
                "params": { "kind": "reverb", "db": -6.0 },
                "replace": replace,
            }),
        ));
        assert!(msg.contains("'kind'"), "replace={replace}: {msg}");
        assert_eq!(s.head_hex(), head, "replace={replace}: no node");
        assert_eq!(
            s.state().tracks[0].effects[0].kind,
            "gain",
            "replace={replace}"
        );
        assert_eq!(
            s.state().tracks[0].effects[0].params,
            json!({ "db": -3.0 }),
            "replace={replace}"
        );
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

// --- the master bus ----------------------------------------------------------
//
// The master bus (`scope: "master"`, #516) is where one unrunnable effect
// used to break the whole mix, preview, playback and export included. The
// refusal is the same call on both targets; these tests drive it there.

/// **Acceptance item 1, on the master.** Every kind the renderer cannot
/// stream is refused with `scope: "master"`, with the kinds that work in
/// the message, and nothing is written.
#[test]
fn add_effect_on_the_master_refuses_each_kind_the_renderer_cannot_stream() {
    let mut s = session();
    let head = s.head_hex();
    let nodes = s.node_count();

    for kind in NOT_YET_STREAMABLE {
        let msg = err(s.call("add_effect", json!({ "scope": "master", "kind": kind })));

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

        assert_eq!(
            s.head_hex(),
            head,
            "{kind}: a refusal must not move the head"
        );
        assert_eq!(
            s.node_count(),
            nodes,
            "{kind}: a refusal must write no node"
        );
        assert!(
            s.state().master_chain.is_empty(),
            "{kind}: the master chain must stay empty"
        );
        assert!(
            s.state().tracks[0].effects.is_empty(),
            "{kind}: nor may a track's chain gain it"
        );
    }

    s.renders()
        .expect("a refused master effect leaves a session that renders");
}

/// A kind that exists nowhere is refused on the master as unknown, with
/// the kinds that work, and is matched exactly as the renderer matches.
#[test]
fn add_effect_on_the_master_refuses_an_unknown_kind_and_lists_what_works() {
    let mut s = session();
    let head = s.head_hex();

    for kind in ["wobbulator", "Reverb", "GAIN", " gain", "gain ", ""] {
        let msg = err(s.call("add_effect", json!({ "scope": "master", "kind": kind })));
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
        assert_eq!(
            s.head_hex(),
            head,
            "{kind:?}: a refusal must not move the head"
        );
        assert!(s.state().master_chain.is_empty(), "{kind:?}");
    }
}

/// The guard on the other side: the refusal does not reach what works. Each
/// working kind is accepted on the master, and the chain of all of them
/// renders.
#[test]
fn every_kind_add_effect_accepts_on_the_master_renders() {
    let mut s = session();
    for kind in STREAMABLE {
        ok(s.call("add_effect", json!({ "scope": "master", "kind": kind })));
    }
    assert_eq!(s.state().master_chain.len(), STREAMABLE.len());
    s.renders()
        .unwrap_or_else(|e| panic!("a master chain of accepted kinds failed to render: {e}"));
}

/// **A stored master reverb cannot be switched on.** A project from before
/// the refusal can hold a bypassed one, which renders only because bypassed
/// entries are skipped. Switching it back on is a way into the chain, so it
/// is refused on the master as it is on a track, and the entry stays
/// bypassed. Bypassing it, or leaving it bypassed, still works.
#[test]
fn a_stored_master_unstreamable_effect_cannot_be_switched_back_on() {
    for kind in NOT_YET_STREAMABLE.iter().copied().chain(["wobbulator"]) {
        let mut s = session();
        s.plant_master(EffectInstance {
            kind: kind.into(),
            params: json!({}),
            bypassed: true,
        });
        s.renders()
            .expect("a bypassed master effect is skipped, so the session renders");
        let head = s.head_hex();
        let nodes = s.node_count();

        let msg = err(s.call(
            "set_effect_bypassed",
            json!({ "scope": "master", "effect_index": 0, "bypassed": false }),
        ));
        assert!(msg.contains(kind), "{kind}: should name the kind: {msg}");
        for works in STREAMABLE {
            assert!(msg.contains(works), "{kind}: should list `{works}`: {msg}");
        }
        assert_eq!(
            s.head_hex(),
            head,
            "{kind}: a refusal must not move the head"
        );
        assert_eq!(
            s.node_count(),
            nodes,
            "{kind}: a refusal must write no node"
        );
        assert!(
            s.state().master_chain[0].bypassed,
            "{kind}: the effect must stay bypassed"
        );

        // Bypassing what is already bypassed changes nothing the renderer
        // sees, and is allowed.
        ok(s.call(
            "set_effect_bypassed",
            json!({ "scope": "master", "effect_index": 0, "bypassed": true }),
        ));
        s.renders().expect("still renders");
    }
}

/// Bypass stays a two-way switch for what can be bypassed: a streamable
/// master effect goes off and on, next to a stored bypassed reverb.
#[test]
fn bypass_on_the_master_still_works_for_a_kind_that_can_be_bypassed() {
    let mut s = session();
    s.plant_master(EffectInstance {
        kind: "reverb".into(),
        params: json!({}),
        bypassed: true,
    });
    ok(s.call(
        "add_effect",
        json!({ "scope": "master", "kind": "limiter" }),
    ));

    ok(s.call(
        "set_effect_bypassed",
        json!({ "scope": "master", "effect_index": 1, "bypassed": true }),
    ));
    ok(s.call(
        "set_effect_bypassed",
        json!({ "scope": "master", "effect_index": 1, "bypassed": false }),
    ));
    let chain = s.state().master_chain;
    assert!(chain[0].bypassed && !chain[1].bypassed, "{chain:?}");
    s.renders().expect("renders");
}
