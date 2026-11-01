//! A stored effect the renderer cannot run no longer fails a render, and
//! the tools that render say so (#520).
//!
//! `add_effect` refuses such a kind now, but an older project can hold one
//! already, on a track or on the master bus (`scope: "master"`, #516).
//! Until this, every render of that session failed. The renderer now
//! leaves the entry out and reports it, and what these tests pin is the
//! half a model or a user can see: each tool that renders puts the
//! warning in its result, a cached preview carries it as the first render
//! did, and `audition_effect` refuses an unrunnable kind up front rather
//! than rendering a no-op.
//!
//! Stored entries are written straight into a node, the way a project
//! saved by an older build holds them, never through a tool.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use tools::ToolResult;

#[path = "support/effects.rs"]
mod effects;
#[path = "support/harness.rs"]
mod harness;
use effects::{entry, NOT_YET_STREAMABLE, STREAMABLE};
use harness::{err, ok, write_sine, Session};

const SAMPLE_RATE: u32 = 48_000;

/// A session with `tracks` tracks, each a half-different sine.
fn session(tracks: usize) -> Session {
    let mut s = Session::empty();
    for i in 0..tracks {
        let src = s.dir.path().join(format!("in{i}.wav"));
        let freq = 220.0 + 110.0 * i as f32;
        write_sine(&src, SAMPLE_RATE, freq, 0.3, SAMPLE_RATE as usize);
        s.load(&src);
    }
    s
}

/// Both families a stored entry can be of: the eleven above, and a kind
/// that exists nowhere.
fn unrunnable_kinds() -> Vec<&'static str> {
    let mut kinds = NOT_YET_STREAMABLE.to_vec();
    kinds.push("wobbulator");
    kinds
}

impl Session {
    fn preview(&mut self) -> ToolResult {
        let node_id = self.head_hex();
        self.call("render_preview", json!({ "node_id": node_id }))
    }
}

fn warnings(v: &Value) -> Vec<String> {
    v["warnings"]
        .as_array()
        .unwrap_or_else(|| panic!("no `warnings` array in {v}"))
        .iter()
        .map(|w| w.as_str().expect("a warning is text").to_string())
        .collect()
}

fn file_bytes(v: &Value) -> Vec<u8> {
    std::fs::read(v["path"].as_str().expect("a path")).expect("the rendered file")
}

fn assert_names(w: &str, parts: &[&str]) {
    for part in parts {
        assert!(w.contains(part), "the warning should name `{part}`: {w}");
    }
}

// --- render_preview ---------------------------------------------------------

/// **Acceptance item 2, through the tool.** Each kind, planted on a track
/// and then on the master: the preview renders, carries one warning that
/// names the scope, the chain index and the kind, and the audio is the
/// audio of a session with no such entry.
#[test]
fn render_preview_renders_each_stored_kind_and_names_what_it_left_out() {
    let mut clean = session(1);
    let clean_bytes = file_bytes(&ok(clean.preview()));

    for kind in unrunnable_kinds() {
        let mut on_track = session(1);
        on_track.plant(0, entry(kind));
        let v = ok(on_track.preview());
        let w = warnings(&v);
        assert_eq!(w.len(), 1, "{kind} on a track: {w:?}");
        assert_names(&w[0], &["track 0", "effect 0", kind]);
        assert!(
            v["summary"].as_str().unwrap().contains(kind),
            "{kind}: the summary is what a model reads first, and it should carry the warning: {v}"
        );
        assert_eq!(
            file_bytes(&v),
            clean_bytes,
            "{kind} on a track: the render must equal the one without the entry"
        );

        let mut on_master = session(1);
        on_master.plant_master(entry(kind));
        let v = ok(on_master.preview());
        let w = warnings(&v);
        assert_eq!(w.len(), 1, "{kind} on the master: {w:?}");
        assert_names(&w[0], &["master", "effect 0", kind]);
        assert_eq!(
            file_bytes(&v),
            clean_bytes,
            "{kind} on the master: the render must equal the one without the entry"
        );
    }
}

/// One warning per entry, and the index is the entry's position in the
/// stored chain, the number `remove_effect` takes.
#[test]
fn every_stored_entry_gets_a_warning_with_its_position() {
    let mut s = session(1);
    s.plant(0, entry("reverb"));
    ok(s.call("add_effect", json!({ "track": 0, "kind": "gain" })));
    s.plant(0, entry("echo"));
    s.plant_master(entry("eq"));

    let w = warnings(&ok(s.preview()));
    assert_eq!(w.len(), 3, "{w:?}");
    assert_names(&w[0], &["track 0", "effect 0", "reverb"]);
    assert_names(&w[1], &["track 0", "effect 2", "echo"]);
    assert_names(&w[2], &["master", "effect 0", "eq"]);
}

/// **The cache.** A preview of an unchanged head is served from the cache,
/// and the cache holds the audio, not the report. The hit has to say what
/// the miss before it said, or the second call would look like the
/// problem had gone away.
#[test]
fn a_cached_preview_carries_the_same_warnings() {
    let mut s = session(1);
    s.plant(0, entry("reverb"));
    s.plant_master(entry("wobbulator"));

    let first = ok(s.preview());
    assert_eq!(first["cached"], json!(false), "{first}");
    let second = ok(s.preview());
    assert_eq!(second["cached"], json!(true), "{second}");

    assert_eq!(warnings(&second), warnings(&first));
    assert_eq!(warnings(&second).len(), 2);
    for w in warnings(&second) {
        assert!(
            second["summary"].as_str().unwrap().contains(&w),
            "the summary should carry the whole warning, not a pointer to it: {second}"
        );
    }
}

/// Once the entry is removed the next head has no warning, cache or not:
/// the warnings come from the state being rendered, not from the project.
#[test]
fn removing_the_stored_entry_clears_the_warning() {
    let mut s = session(1);
    s.plant_master(entry("compressor"));
    assert_eq!(warnings(&ok(s.preview())).len(), 1);

    ok(s.call(
        "remove_effect",
        json!({ "scope": "master", "effect_index": 0 }),
    ));
    let v = ok(s.preview());
    assert!(
        v.get("warnings").is_none(),
        "a session with nothing skipped keeps the result as it always was: {v}"
    );
}

/// A session with nothing wrong has no `warnings` key at all, so the
/// result a model reads on the ordinary path is unchanged.
#[test]
fn a_clean_preview_has_no_warnings_key() {
    let mut s = session(1);
    ok(s.call("add_effect", json!({ "track": 0, "kind": "gain" })));
    ok(s.call(
        "add_effect",
        json!({ "scope": "master", "kind": "limiter" }),
    ));
    let first = ok(s.preview());
    assert!(first.get("warnings").is_none(), "{first}");
    let hit = ok(s.preview());
    assert_eq!(hit["cached"], json!(true));
    assert!(hit.get("warnings").is_none(), "{hit}");
}

/// A bypassed entry is skipped by design and says nothing.
#[test]
fn a_bypassed_stored_entry_says_nothing() {
    let mut s = session(1);
    let mut node = s.store.get(s.store.head().unwrap()).unwrap();
    let mut off = entry("reverb");
    off.bypassed = true;
    node.state.master_chain.push(off);
    s.store.append(node).unwrap();

    let v = ok(s.preview());
    assert!(v.get("warnings").is_none(), "{v}");
}

/// A ranged preview is not cached, and warns the same way.
#[test]
fn a_ranged_preview_carries_the_warnings() {
    let mut s = session(1);
    s.plant_master(entry("phaser"));
    let node_id = s.head_hex();
    let v = ok(s.call(
        "render_preview",
        json!({ "node_id": node_id, "range": [0, 12_000] }),
    ));
    let w = warnings(&v);
    assert_eq!(w.len(), 1, "{w:?}");
    assert_names(&w[0], &["master", "phaser"]);
}

// --- render_final -----------------------------------------------------------

#[test]
fn render_final_carries_the_warnings_in_every_format() {
    for format in ["wav", "flac"] {
        let mut s = session(1);
        s.plant(0, entry("reverb"));
        s.plant_master(entry("eq"));
        let out = s.dir.path().join(format!("final.{format}"));
        let node_id = s.head_hex();

        let v = ok(s.call(
            "render_final",
            json!({
                "node_id": node_id,
                "format": format,
                "out_path": out.to_string_lossy(),
            }),
        ));
        let w = warnings(&v);
        assert_eq!(w.len(), 2, "{format}: {w:?}");
        assert_names(&w[0], &["track 0", "reverb"]);
        assert_names(&w[1], &["master", "eq"]);
        assert!(v["summary"].as_str().unwrap().contains("reverb"), "{v}");
        assert!(out.is_file(), "{format}: the export must exist");
    }
}

#[test]
fn a_clean_render_final_has_no_warnings_key() {
    let mut s = session(1);
    let out = s.dir.path().join("final.wav");
    let node_id = s.head_hex();
    let v = ok(s.call(
        "render_final",
        json!({ "node_id": node_id, "format": "wav", "out_path": out.to_string_lossy() }),
    ));
    assert!(v.get("warnings").is_none(), "{v}");
}

// --- mix_to_new_track and export_multiple -----------------------------------

/// The mix bakes the audio into a new track, so a skipped effect is gone
/// from that track for good and has to be said. The tool renders only the
/// tracks asked for, which makes their position in the render differ from
/// their position in the session: the warning must name the session's
/// index, the one the model knows the track by.
#[test]
fn mix_to_new_track_warns_with_the_sessions_track_numbers() {
    let mut s = session(3);
    s.plant(2, entry("reverb"));
    s.plant(0, entry("echo"));
    s.plant_master(entry("eq"));

    let v = ok(s.call("mix_to_new_track", json!({ "track_indices": [2, 1] })));
    let w = warnings(&v);
    assert_eq!(
        w.len(),
        2,
        "track 0 was not mixed, so its echo is not this mix's: {w:?}"
    );
    assert_names(&w[0], &["track 2", "reverb"]);
    assert_names(&w[1], &["master", "eq"]);
    assert!(
        w.iter().all(|m| !m.contains("echo")),
        "an entry on a track that was not mixed is not reported: {w:?}"
    );
    assert!(v["summary"].as_str().unwrap().contains("reverb"), "{v}");

    // The new track is the mix without the skipped entries.
    let mut clean = session(3);
    let c = ok(clean.call("mix_to_new_track", json!({ "track_indices": [2, 1] })));
    let mixed = |s: &Session, v: &Value| {
        let idx = v["track_index"].as_u64().unwrap() as usize;
        std::fs::read(&s.state().tracks[idx].clips[0].source_path).unwrap()
    };
    assert_eq!(mixed(&s, &v), mixed(&clean, &c));
    assert!(c.get("warnings").is_none(), "{c}");
}

#[test]
fn export_multiple_warns_for_the_exported_tracks_and_the_master_once() {
    let mut s = session(3);
    s.plant(0, entry("reverb"));
    s.plant(1, entry("echo"));
    s.plant(2, entry("phaser"));
    s.plant_master(entry("eq"));

    let v = ok(s.call(
        "export_multiple",
        json!({ "track_indices": [0, 2], "output_dir": "stems" }),
    ));
    let w = warnings(&v);
    assert_eq!(w.len(), 3, "{w:?}");
    assert_names(&w[0], &["track 0", "reverb"]);
    assert_names(&w[1], &["track 2", "phaser"]);
    assert_names(&w[2], &["master", "eq"]);
    assert_eq!(v["count"], json!(2), "{v}");
}

// --- audition_effect ----------------------------------------------------------

/// An audition renders the session as it would sound with the effect in the
/// chain. With a kind the renderer skips, the "audition" would be the
/// session unchanged and a silent no-op, so it is refused at the door with
/// the kinds that do work, and nothing is rendered or cached.
#[test]
fn audition_effect_refuses_each_unrunnable_kind_up_front() {
    let mut s = session(1);
    let head = s.head_hex();
    let nodes = s.node_count();

    for kind in unrunnable_kinds() {
        let msg = err(s.call("audition_effect", json!({ "track": 0, "kind": kind })));
        assert!(msg.contains(kind), "{kind}: should name the kind: {msg}");
        for works in STREAMABLE {
            assert!(
                msg.contains(works),
                "{kind}: should list `{works}` among the kinds that work: {msg}"
            );
        }
    }

    // Kinds that read as the same word, matched exactly like the renderer.
    for kind in ["Reverb", "GAIN", " gain", ""] {
        let msg = err(s.call("audition_effect", json!({ "track": 0, "kind": kind })));
        assert!(msg.contains("unknown effect kind"), "{kind:?}: {msg}");
    }

    assert_eq!(s.head_hex(), head);
    assert_eq!(s.node_count(), nodes);
    let cache: PathBuf = s.dir.path().join(".audiograph").join("auditions");
    let cached = std::fs::read_dir(&cache).map(|d| d.count()).unwrap_or(0);
    assert_eq!(cached, 0, "a refused audition must render nothing");
}

/// The refusal comes before the arguments that need the session, so it
/// reads the same whatever else is wrong with the call.
#[test]
fn the_kind_is_refused_even_when_the_track_is_wrong_too() {
    let mut s = session(1);
    let msg = err(s.call("audition_effect", json!({ "track": 9, "kind": "reverb" })));
    assert!(
        msg.contains("reverb") && msg.contains("gain"),
        "the kind is the more useful thing to learn first: {msg}"
    );
}

#[test]
fn audition_effect_still_auditions_every_kind_that_works() {
    let mut s = session(1);
    for kind in STREAMABLE {
        let v = ok(s.call("audition_effect", json!({ "track": 0, "kind": kind })));
        assert!(Path::new(v["path"].as_str().unwrap()).is_file(), "{kind}");
        assert!(v.get("warnings").is_none(), "{kind}: {v}");
    }
}

/// The audition renders the whole session around the effect, so a stored
/// entry elsewhere in it is skipped there too, and the audition must not
/// claim to sound like the result without saying the result is missing
/// something.
#[test]
fn an_audition_carries_the_warnings_of_the_session_it_renders() {
    let mut s = session(1);
    s.plant_master(entry("reverb"));
    let args = json!({ "track": 0, "kind": "low_pass_filter", "params": { "cutoff_hz": 1000 } });

    let first = ok(s.call("audition_effect", args.clone()));
    assert_eq!(first["cached"], json!(false), "{first}");
    let w = warnings(&first);
    assert_eq!(w.len(), 1, "{w:?}");
    assert_names(&w[0], &["master", "reverb"]);

    let second = ok(s.call("audition_effect", args));
    assert_eq!(second["cached"], json!(true), "{second}");
    assert_eq!(warnings(&second), w);
}
