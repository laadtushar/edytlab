//! Recipes: the edit chain without the audio (#162).
//!
//! The claims worth testing are the ones a user would rely on:
//!
//! * the file contains no audio, and is legible;
//! * replaying it against the same source reproduces the **same bytes**,
//!   which the content-addressed store lets us check exactly rather than
//!   approximately;
//! * replaying against *different* audio produces a sensible result;
//! * a recipe that cannot keep its promise refuses before it starts,
//!   naming the step — not halfway through.

use std::path::{Path, PathBuf};

use hound::{SampleFormat, WavSpec, WavWriter};
use serde_json::{json, Value};
use tempfile::TempDir;
use tools::{ToolContext, ToolDispatcher, ToolResult};

const SAMPLE_RATE: u32 = 48_000;

fn write_sine(path: &Path, freq: f32) -> PathBuf {
    let spec = WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut w = WavWriter::create(path, spec).expect("wav writer");
    for n in 0..SAMPLE_RATE as usize {
        let t = n as f32 / SAMPLE_RATE as f32;
        let s = (2.0 * std::f32::consts::PI * freq * t).sin() * 0.25;
        w.write_sample((s * 32_767.0) as i16).unwrap();
    }
    w.finalize().unwrap();
    path.to_path_buf()
}

fn ok(result: ToolResult) -> Value {
    match result {
        ToolResult::Ok(v) => v,
        ToolResult::Error(msg) => panic!("expected Ok, got Error({msg})"),
    }
}

fn err(result: ToolResult) -> String {
    match result {
        ToolResult::Error(msg) => msg,
        ToolResult::Ok(v) => panic!("expected Error, got Ok({v})"),
    }
}

struct Session {
    dir: TempDir,
    store: session::Store,
    engine: audio_engine::Engine,
    dispatcher: ToolDispatcher,
    clipboard: Option<Vec<f32>>,
}

impl Session {
    fn new() -> Self {
        let dir = TempDir::new().expect("tempdir");
        let store = session::Store::open(dir.path()).expect("open store");
        Self {
            dir,
            store,
            engine: audio_engine::Engine::new(),
            dispatcher: ToolDispatcher::default_dispatcher(),
            clipboard: None,
        }
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

    /// Load `in.wav` and run two destructive edits, so the chain has
    /// something in it worth replaying.
    fn build_chain(&mut self) -> PathBuf {
        let src = write_sine(&self.dir.path().join("in.wav"), 440.0);
        ok(self.call("load", json!({ "path": src.to_string_lossy() })));
        ok(self.call(
            "silence_region",
            json!({ "track": 0, "start_sec": 0.0, "end_sec": 0.2 }),
        ));
        ok(self.call("gain", json!({ "track": 0, "db": -3.0 })));
        src
    }
}

/// The file is the edit chain and nothing else — a few KB of readable
/// JSON with the tool names visible in it.
#[test]
fn a_recipe_contains_the_chain_and_no_audio() {
    let mut s = Session::new();
    s.build_chain();
    let out = s.dir.path().join("chain.json");

    let v = ok(s.call(
        "export_recipe",
        json!({ "out_path": out.to_string_lossy(), "name": "podcast chain" }),
    ));
    assert_eq!(v["steps"], json!(3));
    assert!(v["blockers"].as_array().unwrap().is_empty(), "{v}");

    let text = std::fs::read_to_string(&out).expect("recipe on disk");
    assert!(text.contains("\"load\""), "{text}");
    assert!(text.contains("\"silence_region\""), "{text}");
    assert!(text.contains("\"gain\""), "{text}");
    assert!(text.contains("podcast chain"), "{text}");
    // A second of mono 48 kHz audio is ~96 KB; a chain of three steps
    // is a couple of KB. The point of the ticket is that the audio is
    // not in here.
    assert!(
        text.len() < 8_000,
        "recipe is {} bytes — is audio leaking into it?",
        text.len()
    );
}

/// The audio a track points at after the last step, and its bytes.
fn final_audio(s: &Session) -> (String, Vec<u8>) {
    let head = s.store.head().expect("a head");
    let node = s.store.get(head).expect("head node");
    let clip = &node.state.tracks[0].clips[0];
    let name = clip
        .source_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_string();
    let bytes = std::fs::read(&clip.source_path).expect("derived audio on disk");
    (name, bytes)
}

/// **The acceptance criterion.** Replaying against the same source
/// reproduces the original bytes exactly.
///
/// Checked two ways, because one alone would be weaker than it looks: a
/// derived file is named `blake3(its own samples)`, so an equal *name*
/// already means equal samples — and the bytes are compared as well, so
/// the test does not rest entirely on that invariant holding.
///
/// Note what is deliberately *not* asserted: the node id. `Track::id`
/// is a fresh UUID on every `load`, so two sessions that performed
/// identical edits on identical audio carry different session state and
/// therefore different node ids. The audio is reproducible; the state
/// around it is not, and #162's premise of "no RNG in the edit path"
/// does not hold for track identity.
#[test]
fn replaying_the_same_source_reproduces_the_original_bytes() {
    let mut original = Session::new();
    let src = original.build_chain();
    let (expected_name, expected_bytes) = final_audio(&original);
    let recipe = original.dir.path().join("chain.json");
    ok(original.call(
        "export_recipe",
        json!({ "out_path": recipe.to_string_lossy() }),
    ));

    // A fresh session, same source file, nothing else in common.
    let mut replay = Session::new();
    let v = ok(replay.call("apply_recipe", json!({ "path": recipe.to_string_lossy() })));
    assert_eq!(v["steps_applied"], json!(3), "{v}");

    let (name, bytes) = final_audio(&replay);
    assert_eq!(
        name, expected_name,
        "the replay produced differently-named audio, so its samples differ"
    );
    assert_eq!(
        bytes, expected_bytes,
        "the replayed audio is not byte-identical"
    );
    assert!(src.exists(), "the replay must not disturb the source");
}

/// The other side of the same coin, stated on its own so it cannot be
/// mistaken for an accident: identical work on identical audio produces
/// different node ids, because every `load` mints a random track id.
/// Anything that wants to compare two chains has to compare the audio,
/// not the id.
#[test]
fn a_replay_lands_on_a_different_node_id_despite_identical_audio() {
    let mut original = Session::new();
    original.build_chain();
    let recipe = original.dir.path().join("chain.json");
    ok(original.call(
        "export_recipe",
        json!({ "out_path": recipe.to_string_lossy() }),
    ));

    let mut replay = Session::new();
    ok(replay.call("apply_recipe", json!({ "path": recipe.to_string_lossy() })));

    assert_eq!(
        final_audio(&replay).0,
        final_audio(&original).0,
        "same audio"
    );
    assert_ne!(
        replay.head(),
        original.head(),
        "if this ever passes, track ids became deterministic and the recipe \
         comparison could be simplified to the node id"
    );
}

/// Applying to different audio is the main reason to have a recipe. The
/// result is a sensible one: the chain runs, and lands somewhere else.
#[test]
fn replaying_against_different_audio_runs_and_lands_elsewhere() {
    let mut original = Session::new();
    original.build_chain();
    let expected_audio = final_audio(&original).0;
    let recipe = original.dir.path().join("chain.json");
    ok(original.call(
        "export_recipe",
        json!({ "out_path": recipe.to_string_lossy() }),
    ));

    let mut replay = Session::new();
    let other = write_sine(&replay.dir.path().join("other.wav"), 880.0);
    let v = ok(replay.call(
        "apply_recipe",
        json!({ "path": recipe.to_string_lossy(), "source": other.to_string_lossy() }),
    ));

    assert_eq!(v["steps_applied"], json!(3), "{v}");
    assert_eq!(v["retargeted_loads"], json!(1), "{v}");
    // Compared by the audio's own name rather than by node id: ids
    // differ even for identical work (see the test below), so they
    // cannot tell "different audio" from "different run".
    assert_ne!(
        final_audio(&replay).0,
        expected_audio,
        "different source audio must not produce the same derived audio"
    );
}

/// A source that is not there is refused before anything runs, rather
/// than failing inside step 1 with a decode error.
#[test]
fn a_missing_substitute_source_is_refused_up_front() {
    let mut s = Session::new();
    s.build_chain();
    let recipe = s.dir.path().join("chain.json");
    ok(s.call(
        "export_recipe",
        json!({ "out_path": recipe.to_string_lossy() }),
    ));

    let msg = err(s.call(
        "apply_recipe",
        json!({ "path": recipe.to_string_lossy(), "source": "/nope/missing.wav" }),
    ));
    assert!(msg.contains("missing.wav"), "{msg}");
}

/// **Refuse before running, not halfway through.** A chain containing a
/// step that cannot be replayed is rejected as a whole, naming the
/// step — a session left half-edited is a state nobody chose.
#[test]
fn a_non_replayable_step_refuses_the_whole_recipe_and_names_it() {
    let mut s = Session::new();
    s.build_chain();
    let recipe = s.dir.path().join("chain.json");
    ok(s.call(
        "export_recipe",
        json!({ "out_path": recipe.to_string_lossy() }),
    ));

    // Splice in a model step, which is what `transcribe` records.
    let mut doc: Value = serde_json::from_str(&std::fs::read_to_string(&recipe).unwrap()).unwrap();
    doc["steps"].as_array_mut().unwrap().push(json!({
        "tool": "transcribe",
        "params": { "path": "in.wav" },
        "inputs": { "model": { "id": "whisper", "version": "base.onnx" }, "replayable": false },
        "replayable": false,
    }));
    std::fs::write(&recipe, serde_json::to_string_pretty(&doc).unwrap()).unwrap();

    let mut replay = Session::new();
    let head_before = replay.store.head();
    let msg = err(replay.call("apply_recipe", json!({ "path": recipe.to_string_lossy() })));

    assert!(msg.contains("transcribe"), "should name the step: {msg}");
    assert!(msg.contains("whisper"), "should say why: {msg}");
    assert_eq!(
        replay.store.head(),
        head_before,
        "nothing may run when the recipe is refused"
    );
}

/// A recipe that contains `apply_recipe` would recurse. Refused by
/// name rather than left to blow the stack.
#[test]
fn a_self_referential_recipe_is_refused() {
    let mut s = Session::new();
    s.build_chain();
    let recipe = s.dir.path().join("chain.json");
    ok(s.call(
        "export_recipe",
        json!({ "out_path": recipe.to_string_lossy() }),
    ));

    let mut doc: Value = serde_json::from_str(&std::fs::read_to_string(&recipe).unwrap()).unwrap();
    doc["steps"].as_array_mut().unwrap().push(json!({
        "tool": "apply_recipe",
        "params": { "path": "chain.json" },
        "replayable": true,
    }));
    std::fs::write(&recipe, serde_json::to_string_pretty(&doc).unwrap()).unwrap();

    let msg = err(s.call("apply_recipe", json!({ "path": recipe.to_string_lossy() })));
    assert!(msg.contains("recurse"), "{msg}");
}

/// Reviewable before it executes: a dry run reports the plan and
/// changes nothing.
#[test]
fn a_dry_run_reports_the_plan_and_touches_nothing() {
    let mut s = Session::new();
    s.build_chain();
    let recipe = s.dir.path().join("chain.json");
    ok(s.call(
        "export_recipe",
        json!({ "out_path": recipe.to_string_lossy() }),
    ));

    let mut replay = Session::new();
    let v = ok(replay.call(
        "apply_recipe",
        json!({ "path": recipe.to_string_lossy(), "dry_run": true }),
    ));

    assert_eq!(v["dry_run"], json!(true));
    assert_eq!(v["steps"], json!(3));
    assert!(
        v["summary"]
            .as_str()
            .unwrap()
            .contains("load → silence_region → gain"),
        "the plan should read in order: {v}"
    );
    assert!(
        replay.store.head().is_none(),
        "a dry run must not touch the session"
    );
}

/// A file that is not a recipe fails as a message, not a panic.
#[test]
fn an_unreadable_recipe_is_a_message() {
    let mut s = Session::new();
    let junk = s.dir.path().join("junk.json");
    std::fs::write(&junk, "{ this is not json").unwrap();
    let msg = err(s.call("apply_recipe", json!({ "path": junk.to_string_lossy() })));
    assert!(msg.contains("not a readable recipe"), "{msg}");
}
