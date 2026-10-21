//! One chain across a folder (#169).
//!
//! Podcast work arrives as twelve files, not one. The claims worth
//! testing are the ones that make a batch usable unattended: every file
//! gets its **own** session, a failure on one does not abandon the
//! rest, and the report says which was which and why.

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

/// A recipe that loads a file and quietens it, exported from a throwaway
/// session — the same way a user would make one.
fn make_recipe(s: &mut Session) -> PathBuf {
    let src = write_sine(&s.dir.path().join("template.wav"), 440.0);
    ok(s.call("load", json!({ "path": src.to_string_lossy() })));
    ok(s.call("gain", json!({ "track": 0, "db": -6.0 })));
    let recipe = s.dir.path().join("chain.json");
    ok(s.call(
        "export_recipe",
        json!({ "out_path": recipe.to_string_lossy(), "name": "quieten" }),
    ));
    recipe
}

/// A folder of three takes.
fn make_folder(root: &Path) -> PathBuf {
    let dir = root.join("takes");
    std::fs::create_dir_all(&dir).unwrap();
    for (i, freq) in [220.0, 440.0, 880.0].into_iter().enumerate() {
        write_sine(&dir.join(format!("take-{i}.wav")), freq);
    }
    // Something that is not audio, which must be ignored rather than
    // reported as a failure.
    std::fs::write(dir.join("notes.txt"), "not audio").unwrap();
    dir
}

#[test]
fn a_chain_runs_across_every_file_in_a_folder() {
    let mut s = Session::new();
    let recipe = make_recipe(&mut s);
    let takes = make_folder(s.dir.path());
    let out = s.dir.path().join("out");

    let v = ok(s.call(
        "batch_apply",
        json!({
            "recipe_path": recipe.to_string_lossy(),
            "input_dir": takes.to_string_lossy(),
            "output_dir": out.to_string_lossy(),
        }),
    ));

    assert_eq!(v["files"], json!(3), "the .txt must not be counted: {v}");
    assert_eq!(v["succeeded"], json!(3), "{v}");
    assert_eq!(v["refused"], json!(0));
}

/// **Each file gets its own session and history.** Not one giant
/// session with three tracks: three projects, each with its own store.
#[test]
fn each_file_becomes_its_own_project() {
    let mut s = Session::new();
    let recipe = make_recipe(&mut s);
    let takes = make_folder(s.dir.path());
    let out = s.dir.path().join("out");

    ok(s.call(
        "batch_apply",
        json!({
            "recipe_path": recipe.to_string_lossy(),
            "input_dir": takes.to_string_lossy(),
            "output_dir": out.to_string_lossy(),
        }),
    ));

    for i in 0..3 {
        let project = out.join(format!("take-{i}"));
        assert!(
            project.join(".audiograph").join("nodes").is_dir(),
            "take-{i} has no history of its own"
        );
        let store = session::Store::open(&project).expect("open the batch project");
        assert!(store.head().is_some(), "take-{i} has no head");
        assert_eq!(
            store.list_nodes().expect("nodes").len(),
            2,
            "each project should hold its own two steps, not the others'"
        );
    }
}

/// **A failure on file 3 does not abandon files 4–12.** The whole point
/// of a batch is that it is unattended.
#[test]
fn one_bad_file_does_not_stop_the_rest() {
    let mut s = Session::new();
    let recipe = make_recipe(&mut s);
    let takes = make_folder(s.dir.path());
    // A file with the right extension and nothing decodable in it.
    std::fs::write(takes.join("broken.wav"), b"not a wav at all").unwrap();
    let out = s.dir.path().join("out");

    let v = ok(s.call(
        "batch_apply",
        json!({
            "recipe_path": recipe.to_string_lossy(),
            "input_dir": takes.to_string_lossy(),
            "output_dir": out.to_string_lossy(),
        }),
    ));

    assert_eq!(v["files"], json!(4));
    assert_eq!(
        v["succeeded"],
        json!(3),
        "the three good takes still ran: {v}"
    );
    assert_eq!(v["refused"], json!(1));

    let results = v["results"].as_array().expect("results");
    let bad = results
        .iter()
        .find(|r| r["file"].as_str().unwrap_or_default().contains("broken"))
        .expect("the bad file has a row of its own");
    assert_eq!(bad["ok"], json!(false));
    assert!(
        !bad["reason"].as_str().unwrap_or_default().is_empty(),
        "a refusal has to say why: {bad}"
    );
}

/// The report names what happened per file, rather than a count that
/// hides which one was odd.
#[test]
fn the_report_names_each_file() {
    let mut s = Session::new();
    let recipe = make_recipe(&mut s);
    let takes = make_folder(s.dir.path());

    let v = ok(s.call(
        "batch_apply",
        json!({
            "recipe_path": recipe.to_string_lossy(),
            "input_dir": takes.to_string_lossy(),
            "output_dir": s.dir.path().join("out").to_string_lossy(),
        }),
    ));

    let results = v["results"].as_array().expect("results");
    assert_eq!(results.len(), 3);
    for r in results {
        assert!(r["file"].as_str().is_some(), "every row names its file");
        assert_eq!(r["steps_applied"], json!(2), "{r}");
    }
}

#[test]
fn it_can_render_each_result() {
    let mut s = Session::new();
    let recipe = make_recipe(&mut s);
    let takes = make_folder(s.dir.path());
    let out = s.dir.path().join("out");

    let v = ok(s.call(
        "batch_apply",
        json!({
            "recipe_path": recipe.to_string_lossy(),
            "input_dir": takes.to_string_lossy(),
            "output_dir": out.to_string_lossy(),
            "render_format": "wav",
        }),
    ));

    assert_eq!(v["succeeded"], json!(3), "{v}");
    for i in 0..3 {
        let rendered = out.join(format!("take-{i}.wav"));
        assert!(rendered.is_file(), "take-{i} was not rendered");
    }
}

/// A chain that cannot be replayed is refused **once, up front** —
/// it is a property of the chain, not of the twelve files.
#[test]
fn an_unreplayable_chain_is_refused_before_any_file() {
    let mut s = Session::new();
    let recipe = make_recipe(&mut s);
    let takes = make_folder(s.dir.path());
    let out = s.dir.path().join("out");

    let mut doc: Value = serde_json::from_str(&std::fs::read_to_string(&recipe).unwrap()).unwrap();
    doc["steps"].as_array_mut().unwrap().push(json!({
        "tool": "transcribe",
        "params": { "path": "in.wav" },
        "inputs": { "model": { "id": "whisper", "version": "base.onnx" } },
        "replayable": false,
    }));
    std::fs::write(&recipe, serde_json::to_string_pretty(&doc).unwrap()).unwrap();

    let msg = err(s.call(
        "batch_apply",
        json!({
            "recipe_path": recipe.to_string_lossy(),
            "input_dir": takes.to_string_lossy(),
            "output_dir": out.to_string_lossy(),
        }),
    ));
    assert!(msg.contains("transcribe"), "should name the step: {msg}");
    assert!(!out.exists(), "and touch nothing");
}

#[test]
fn an_empty_folder_says_so() {
    let mut s = Session::new();
    let recipe = make_recipe(&mut s);
    let empty = s.dir.path().join("empty");
    std::fs::create_dir_all(&empty).unwrap();

    let msg = err(s.call(
        "batch_apply",
        json!({
            "recipe_path": recipe.to_string_lossy(),
            "input_dir": empty.to_string_lossy(),
        }),
    ));
    assert!(msg.contains("no audio files"), "{msg}");
}

// =============================================================================
// The capability whitelist is enforced at dispatch, not just in the
// schema list (#238)
// =============================================================================
//
// Trimming the `tools` array sent to the model is a hint: it tells a
// well-behaved model what to ask for. It was the *only* place the
// whitelist was applied, so it was not a control.
//
// Two ways past it. A model on an OpenAI-compatible or Ollama endpoint
// could simply name a filtered-out tool. And deterministically, without
// any model misbehaviour at all: `batch_apply` built a fresh
// `default_dispatcher()` that had never seen the whitelist, so
// unticking `render_final` still let it write files through
// `render_format` — at an unconstrained absolute `out_path`.

use std::collections::HashSet;

/// Run `tool` with only `allowed` permitted.
fn call_restricted(
    s: &mut Session,
    tool: &str,
    args: Value,
    allowed: &[&str],
) -> tools::Result<ToolResult> {
    let allowed: HashSet<String> = allowed.iter().map(|t| (*t).to_string()).collect();
    let mut ctx = ToolContext {
        store: &mut s.store,
        engine: &mut s.engine,
        user_message: "",
        clipboard: &mut s.clipboard,
        allowed_tools: Some(&allowed),
    };
    s.dispatcher.invoke(tool, args, &mut ctx)
}

#[test]
fn a_tool_outside_the_whitelist_is_refused_at_dispatch() {
    let mut s = Session::new();
    let src = write_sine(&s.dir.path().join("a.wav"), 440.0);

    let refused = call_restricted(
        &mut s,
        "load",
        json!({ "path": src.to_string_lossy() }),
        &["gain"],
    );
    match refused {
        Err(tools::DispatchError::NotPermitted(name)) => assert_eq!(name, "load"),
        other => panic!("expected NotPermitted, got {other:?}"),
    }
}

/// The refusal is distinct from "no such tool" on purpose: the agent
/// loop hands it back to the model, and calling it unknown would send
/// the model hunting for a spelling mistake.
#[test]
fn a_refusal_is_not_reported_as_an_unknown_tool() {
    let mut s = Session::new();
    let refused = call_restricted(&mut s, "gain", json!({ "track": 0, "db": -3.0 }), &["load"]);
    assert!(
        matches!(refused, Err(tools::DispatchError::NotPermitted(_))),
        "a permitted-but-disabled tool must not be reported as unknown"
    );
}

#[test]
fn a_whitelisted_tool_still_runs() {
    let mut s = Session::new();
    let src = write_sine(&s.dir.path().join("a.wav"), 440.0);
    let out = call_restricted(
        &mut s,
        "load",
        json!({ "path": src.to_string_lossy() }),
        &["load"],
    )
    .expect("dispatch");
    assert!(
        matches!(out, ToolResult::Ok(_)),
        "the whitelist must permit what it lists, not merely refuse everything"
    );
}

/// The deterministic bypass, and the reason the check lives on
/// `ToolContext` rather than the dispatcher: `batch_apply` builds its
/// own dispatcher, but it inherits the *context*, so the restriction
/// follows the nested dispatch.
#[test]
fn batch_apply_cannot_reach_a_tool_its_caller_may_not_use() {
    let mut s = Session::new();
    let recipe = make_recipe(&mut s);

    let input_dir = s.dir.path().join("inputs");
    std::fs::create_dir_all(&input_dir).unwrap();
    write_sine(&input_dir.join("one.wav"), 440.0);

    // `batch_apply` is allowed; the `gain` step inside the recipe is not.
    let out = call_restricted(
        &mut s,
        "batch_apply",
        json!({
            "input_dir": input_dir.to_string_lossy(),
            "recipe_path": recipe.to_string_lossy(),
        }),
        &["batch_apply", "load"],
    )
    .expect("batch_apply itself dispatches");

    let v = match out {
        ToolResult::Ok(v) => v,
        ToolResult::Error(m) => panic!("batch_apply errored outright: {m}"),
    };
    // Every file must be refused rather than quietly processed: the
    // chain cannot complete without a tool the caller may not run.
    assert_eq!(
        v["succeeded"], 0,
        "a step outside the whitelist was executed through batch_apply: {v}"
    );
    assert_eq!(
        v["refused"], 1,
        "the file should be refused, not skipped: {v}"
    );

    // And the reason must name the refusal, so a user who unticked a
    // capability can tell that is why the batch did nothing — rather
    // than reading it as an unexplained failure.
    let reason = v["results"][0]["reason"].as_str().unwrap_or_default();
    assert!(
        reason.contains("gain") && reason.contains("not enabled"),
        "the refusal reason should name the disabled tool, got: {reason}"
    );
}

// =============================================================================
// A batch never writes into a project that is open (#566)
// =============================================================================
//
// Each file's project is `<output_dir>/<file stem>`, and `output_dir`
// defaults to the folder the audio is in. So a folder holding `podcast.wav`
// next to a project folder named `podcast/` was enough for the batch to open
// a second `Store` on a project the app had open, and append its whole chain
// to it: the open one's in-memory head knew nothing of the new nodes, and its
// next edit would write a head that skipped them.

/// Run `tool` against `store` as the agent does: with the project's store as
/// the context's.
fn call_on(store: &mut session::Store, tool: &str, args: Value) -> ToolResult {
    let mut engine = audio_engine::Engine::new();
    let mut clipboard: Option<tools::Clipboard> = None;
    let mut ctx = ToolContext {
        store,
        engine: &mut engine,
        user_message: "",
        clipboard: &mut clipboard,
        allowed_tools: None,
    };
    ToolDispatcher::default_dispatcher()
        .invoke(tool, args, &mut ctx)
        .unwrap()
}

/// The project the app has open: `dir`, opened as the app opens it (shared,
/// through the registry), with one edit in it.
fn open_project(dir: &Path, audio: &Path) -> session::LiveStore {
    let live = session::Store::open_shared(dir).expect("open the project");
    ok(call_on(
        &mut live.lock().unwrap(),
        "load",
        json!({ "path": audio.to_string_lossy() }),
    ));
    live
}

/// What the open project holds: its head and how many nodes it has, read
/// from the live store and from the folder.
fn history(live: &session::LiveStore, project: &Path) -> (Option<session::NodeId>, usize, usize) {
    let held = live.lock().unwrap();
    let on_disk = session::Store::open(project).unwrap();
    assert_eq!(
        on_disk.head(),
        held.head(),
        "the folder and the store differ"
    );
    (
        held.head(),
        held.list_nodes().unwrap().len(),
        on_disk.list_nodes().unwrap().len(),
    )
}

#[test]
fn a_batch_will_not_write_into_the_project_that_is_open() {
    let mut s = Session::new();
    let recipe = make_recipe(&mut s);
    let takes = make_folder(s.dir.path());
    // The open project is `takes/take-0`, which is where the default
    // `output_dir` (the audio's own folder) puts the project of `take-0.wav`.
    let project = takes.join("take-0");
    let open = open_project(&project, &takes.join("take-0.wav"));
    let before = history(&open, &project);

    // The turn that runs the batch holds the project's store, as the agent does.
    let msg = err(call_on(
        &mut open.lock().unwrap(),
        "batch_apply",
        json!({
            "recipe_path": recipe.to_string_lossy(),
            "input_dir": takes.to_string_lossy(),
        }),
    ));

    assert!(msg.contains("take-0"), "name the folder: {msg}");
    assert!(msg.contains("output_dir"), "ask for another: {msg}");
    assert_eq!(
        history(&open, &project),
        before,
        "a node was appended to the open project"
    );
    // Refused up front, before any file: nothing was started.
    assert!(!takes.join("take-1").exists(), "{msg}");
}

/// The window's project, not the turn's own: the context's store is another
/// folder's, and the open project is only open.
#[test]
fn a_batch_will_not_write_into_a_project_the_window_has_open_either() {
    let mut s = Session::new();
    let recipe = make_recipe(&mut s);
    let takes = make_folder(s.dir.path());
    let project = takes.join("take-1");
    let open = open_project(&project, &takes.join("take-1.wav"));
    let before = history(&open, &project);

    let msg = err(s.call(
        "batch_apply",
        json!({
            "recipe_path": recipe.to_string_lossy(),
            "input_dir": takes.to_string_lossy(),
        }),
    ));

    assert!(msg.contains("take-1"), "{msg}");
    assert_eq!(history(&open, &project), before);
    assert!(!takes.join("take-0").exists(), "nothing was started");
}

/// A project another writer holds privately (a batch already running into
/// it) is as much in use as a shared one, and every open one is named.
#[test]
fn every_project_in_use_is_named() {
    let mut s = Session::new();
    let recipe = make_recipe(&mut s);
    let takes = make_folder(s.dir.path());
    let shared = open_project(&takes.join("take-0"), &takes.join("take-0.wav"));
    let _private = session::Store::open_private(&takes.join("take-2")).unwrap();

    let msg = err(s.call(
        "batch_apply",
        json!({
            "recipe_path": recipe.to_string_lossy(),
            "input_dir": takes.to_string_lossy(),
        }),
    ));

    assert!(msg.contains("take-0") && msg.contains("take-2"), "{msg}");
    assert!(!msg.contains("take-1"), "take-1 is not open: {msg}");
    drop(shared);
}

/// A project open at a path whose folder has since been replaced still
/// writes through that path. The batch's refusal is its own: the caller is
/// the turn, so "wait for the turn to end" would be no answer.
#[test]
fn a_per_file_project_folder_replaced_under_an_open_project_is_refused_without_telling_the_turn_to_wait(
) {
    let mut s = Session::new();
    let recipe = make_recipe(&mut s);
    let takes = make_folder(s.dir.path());
    let project = takes.join("take-0");
    let open = open_project(&project, &takes.join("take-0.wav"));
    // The folder is put back from a backup: another folder at the same path.
    std::fs::rename(&project, takes.join("take-0.old")).unwrap();
    std::fs::create_dir(&project).unwrap();

    let v = ok(s.call(
        "batch_apply",
        json!({
            "recipe_path": recipe.to_string_lossy(),
            "input_dir": takes.to_string_lossy(),
        }),
    ));

    assert_eq!(v["succeeded"], json!(2), "the other two files ran: {v}");
    let refused = v["results"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["file"].as_str().unwrap().ends_with("take-0.wav"))
        .expect("a row for take-0");
    assert_eq!(refused["ok"], json!(false), "{refused}");
    let why = refused["reason"].as_str().unwrap();
    assert!(why.contains("output_dir"), "ask for another: {why}");
    assert!(!why.contains("wait for"), "the caller is the turn: {why}");
    assert!(!why.contains("open the project again"), "{why}");
    assert!(
        !project.join(".audiograph").join("head").exists(),
        "a history was written into the replaced folder"
    );
    drop(open);
}

/// Only the folders in use are refused: with the open project left alone, a
/// different `output_dir` runs as ever and does not touch it.
#[test]
fn a_batch_into_another_folder_still_runs_beside_an_open_project() {
    let mut s = Session::new();
    let recipe = make_recipe(&mut s);
    let takes = make_folder(s.dir.path());
    let project = takes.join("take-0");
    let open = open_project(&project, &takes.join("take-0.wav"));
    let before = history(&open, &project);
    let out = s.dir.path().join("out");

    let v = ok(call_on(
        &mut open.lock().unwrap(),
        "batch_apply",
        json!({
            "recipe_path": recipe.to_string_lossy(),
            "input_dir": takes.to_string_lossy(),
            "output_dir": out.to_string_lossy(),
        }),
    ));

    assert_eq!(v["succeeded"], json!(3), "{v}");
    assert_eq!(history(&open, &project), before, "the open project changed");
    for i in 0..3 {
        let batch_project = session::Store::open(&out.join(format!("take-{i}"))).unwrap();
        assert_eq!(batch_project.list_nodes().unwrap().len(), 2);
    }
    // The batch's projects are free again once it is done.
    assert!(!session::Store::is_open(&out.join("take-0")));
}
