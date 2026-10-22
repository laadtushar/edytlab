//! A render must never be written over audio the project reads (#653).
//!
//! `load` leaves a WAV where it is, so a clip reads the user's own file. A
//! writer that was then handed that path truncated it before the render had
//! read a byte: `batch_apply` with its default `output_dir` (the input
//! folder) and a `render_format` equal to the input's extension rendered
//! onto every input, and `render_final` did the same for an `out_path` that
//! was a loaded original. Even a write that read first would have changed
//! what every older node in the history plays, so undo would lie.
//!
//! These tests drive the tools the way the agent does and check the user's
//! bytes afterwards.

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

/// An MP3 of a sine, made by the app's own encoder.
fn write_mp3_sine(path: &Path, freq: f32) -> PathBuf {
    let samples: Vec<f32> = (0..SAMPLE_RATE as usize)
        .map(|n| {
            let t = n as f32 / SAMPLE_RATE as f32;
            (2.0 * std::f32::consts::PI * freq * t).sin() * 0.25
        })
        .collect();
    audio_engine::write_mp3(&samples, SAMPLE_RATE, 1, None, path).expect("mp3");
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
        let project = dir.path().join("project");
        std::fs::create_dir_all(&project).unwrap();
        let store = session::Store::open(&project).expect("open store");
        Self {
            dir,
            store,
            engine: audio_engine::Engine::new(),
            dispatcher: ToolDispatcher::default_dispatcher(),
            clipboard: None,
        }
    }

    /// Where the user's own audio lives: outside the project.
    fn user_dir(&self, name: &str) -> PathBuf {
        let dir = self.dir.path().join(name);
        std::fs::create_dir_all(&dir).unwrap();
        dir
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

    /// `load` the file in place and take 6 dB off the track, so that a render
    /// of it is *not* a byte copy of the file: a write that lands on the
    /// original then shows as a changed file, whichever way round it went.
    fn load_quietened(&mut self, path: &Path) {
        ok(self.call("load", json!({ "path": path.to_string_lossy() })));
        ok(self.call("gain", json!({ "track": 0, "db": -6.0 })));
    }

    fn head(&self) -> session::SessionNode {
        self.store.get(self.store.head().expect("head")).unwrap()
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

fn bytes(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// The user's file is exactly as it was. Asked before the tool's answer is
/// looked at, so a failure reports the damage and not only that a write
/// went ahead.
#[track_caller]
fn assert_unchanged(path: &Path, before: &[u8]) {
    let now = bytes(path);
    assert!(
        now == before,
        "{} was written over (it was {} bytes, it is {} now)",
        path.display(),
        before.len(),
        now.len()
    );
}

/// A recipe from a throwaway session with a `load` and a level change: the
/// way a user makes one (`export_recipe`). The session the test then drives
/// is another, so its head is its own.
fn make_recipe(s: &Session) -> PathBuf {
    let mut maker = Session::new();
    let template = write_sine(&maker.user_dir("templates").join("template.wav"), 440.0);
    ok(maker.call("load", json!({ "path": template.to_string_lossy() })));
    ok(maker.call("gain", json!({ "track": 0, "db": -6.0 })));
    let recipe = s.dir.path().join("level.recipe.json");
    ok(maker.call(
        "export_recipe",
        json!({ "out_path": recipe.to_string_lossy(), "name": "level" }),
    ));
    recipe
}

/// Three takes in a folder of the user's own.
fn three_wavs(dir: &Path) -> Vec<PathBuf> {
    [220.0, 440.0, 880.0]
        .into_iter()
        .enumerate()
        .map(|(i, f)| write_sine(&dir.join(format!("take{}.wav", i + 1)), f))
        .collect()
}

// =============================================================================
// batch_apply
// =============================================================================

/// **The #653 repro.** Three WAVs, a recipe, the default `output_dir` and a
/// WAV render: refused before any file is started, inputs byte-identical.
#[test]
fn a_batch_that_would_render_onto_its_inputs_is_refused_and_they_survive() {
    let mut s = Session::new();
    let recipe = make_recipe(&s);
    let input = s.user_dir("danger_in");
    let takes = three_wavs(&input);
    let before: Vec<Vec<u8>> = takes.iter().map(|p| bytes(p)).collect();

    let result = s.call(
        "batch_apply",
        json!({
            "recipe_path": recipe.to_string_lossy(),
            "input_dir": input.to_string_lossy(),
            "render_format": "wav",
        }),
    );
    for (take, before) in takes.iter().zip(&before) {
        assert_unchanged(take, before);
    }
    let msg = err(result);

    assert!(
        msg.contains("output_dir"),
        "the refusal has to say what to change: {msg}"
    );
    assert!(
        msg.contains("take1.wav"),
        "and name the first collision: {msg}"
    );
    assert!(
        !input.join("take1").exists(),
        "nothing is started: no project folder beside the inputs"
    );
}

/// The same collision on a lossy input: the load copy is a derived WAV, so
/// the render succeeded and replaced the user's MP3 with a re-encode.
#[test]
fn a_batch_will_not_re_encode_over_an_mp3_input() {
    let mut s = Session::new();
    let recipe = make_recipe(&s);
    let input = s.user_dir("mp3_in");
    let mp3 = write_mp3_sine(&input.join("a.mp3"), 440.0);
    let before = bytes(&mp3);

    let result = s.call(
        "batch_apply",
        json!({
            "recipe_path": recipe.to_string_lossy(),
            "input_dir": input.to_string_lossy(),
            "render_format": "mp3",
        }),
    );
    assert_unchanged(&mp3, &before);
    let msg = err(result);

    assert!(msg.contains("output_dir"), "{msg}");
}

/// Only some files collide, and not the first: the whole batch is still
/// refused, before the clean file is started.
#[test]
fn a_collision_on_a_later_file_stops_the_batch_before_the_first() {
    let mut s = Session::new();
    let recipe = make_recipe(&s);
    let input = s.user_dir("mixed_in");
    // `a.mp3` would render to `a.wav`, which is not there; `b.wav` would
    // render onto itself.
    write_mp3_sine(&input.join("a.mp3"), 330.0);
    let b = write_sine(&input.join("b.wav"), 440.0);
    let before = bytes(&b);

    let result = s.call(
        "batch_apply",
        json!({
            "recipe_path": recipe.to_string_lossy(),
            "input_dir": input.to_string_lossy(),
            "render_format": "wav",
        }),
    );
    assert_unchanged(&b, &before);
    let msg = err(result);

    assert!(msg.contains("b.wav"), "names the file that collides: {msg}");
    assert!(
        !input.join("a").exists() && !input.join("a.wav").exists(),
        "the clean file must not have been started either"
    );
}

/// A render that would land on a *different* input is as bad: `a.mp3`'s WAV
/// render is `a.wav`, which is another file in the folder.
#[test]
fn a_render_that_lands_on_another_input_is_refused() {
    let mut s = Session::new();
    let recipe = make_recipe(&s);
    let input = s.user_dir("pair_in");
    write_mp3_sine(&input.join("a.mp3"), 330.0);
    let wav = write_sine(&input.join("a.wav"), 440.0);
    let before = bytes(&wav);

    let result = s.call(
        "batch_apply",
        json!({
            "recipe_path": recipe.to_string_lossy(),
            "input_dir": input.to_string_lossy(),
            "render_format": "wav",
        }),
    );
    assert_unchanged(&wav, &before);
    let msg = err(result);

    assert!(msg.contains("output_dir"), "{msg}");
}

/// The fix must not turn a safe batch into a refused one: another
/// `output_dir`, the same inputs, the same files.
#[test]
fn a_batch_with_its_own_output_dir_still_renders_and_leaves_the_inputs_alone() {
    let mut s = Session::new();
    let recipe = make_recipe(&s);
    let input = s.user_dir("safe_in");
    let takes = three_wavs(&input);
    let before: Vec<Vec<u8>> = takes.iter().map(|p| bytes(p)).collect();
    let out = s.dir.path().join("safe_out");

    let v = ok(s.call(
        "batch_apply",
        json!({
            "recipe_path": recipe.to_string_lossy(),
            "input_dir": input.to_string_lossy(),
            "output_dir": out.to_string_lossy(),
            "render_format": "wav",
        }),
    ));

    assert_eq!(v["succeeded"], json!(3), "{v}");
    assert_eq!(v["refused"], json!(0), "{v}");
    for (i, (take, before)) in takes.iter().zip(&before).enumerate() {
        assert_unchanged(take, before);
        assert!(
            out.join(format!("take{}.wav", i + 1))
                .metadata()
                .unwrap()
                .len()
                > 44,
            "take{} was not rendered",
            i + 1
        );
    }
}

/// A batch that renders nothing (no `render_format`) writes only projects,
/// as before, whatever `output_dir` is.
#[test]
fn a_batch_with_no_render_format_is_not_in_the_way() {
    let mut s = Session::new();
    let recipe = make_recipe(&s);
    let input = s.user_dir("noformat_in");
    let takes = three_wavs(&input);
    let before: Vec<Vec<u8>> = takes.iter().map(|p| bytes(p)).collect();

    let v = ok(s.call(
        "batch_apply",
        json!({
            "recipe_path": recipe.to_string_lossy(),
            "input_dir": input.to_string_lossy(),
        }),
    ));

    assert_eq!(v["succeeded"], json!(3), "{v}");
    for (take, before) in takes.iter().zip(&before) {
        assert_unchanged(take, before);
    }
}

// =============================================================================
// render_final
// =============================================================================

/// `render_final` handed the path of a loaded original: refused, file
/// unchanged. Before, it truncated the file before the render read it.
#[test]
fn render_final_will_not_write_over_a_loaded_original() {
    let mut s = Session::new();
    let original = write_sine(&s.user_dir("music").join("take1.wav"), 440.0);
    let before = bytes(&original);
    s.load_quietened(&original);
    let head = s.store.head().unwrap().to_hex();

    let result = s.call(
        "render_final",
        json!({ "node_id": head, "format": "wav", "out_path": original.to_string_lossy() }),
    );
    assert_unchanged(&original, &before);
    let msg = err(result);

    assert!(
        msg.contains("take1.wav") && msg.contains("another"),
        "the refusal names the file and says to choose another path: {msg}"
    );
}

/// The head no longer names the original (a destructive edit moved the clip
/// to derived audio) but an older node does, and undo plays it.
#[test]
fn render_final_will_not_write_over_a_file_only_an_older_node_names() {
    let mut s = Session::new();
    let original = write_sine(&s.user_dir("music").join("take1.wav"), 440.0);
    let before = bytes(&original);
    ok(s.call("load", json!({ "path": original.to_string_lossy() })));
    ok(s.call("reverse", json!({ "track": 0 })));
    let head = s.head();
    assert_ne!(
        head.state.tracks[0].clips[0].source_path, original,
        "the premise: the head plays derived audio, not the original"
    );
    let head_id = s.store.head().unwrap().to_hex();

    let result = s.call(
        "render_final",
        json!({ "node_id": head_id, "format": "wav", "out_path": original.to_string_lossy() }),
    );
    assert_unchanged(&original, &before);
    let msg = err(result);

    assert!(msg.contains("take1.wav"), "{msg}");
}

/// The lossy original: the session plays a derived WAV copy of `a.mp3`, so
/// nothing in the graph breaks, but the file is still the user's.
#[test]
fn render_final_will_not_re_encode_over_the_mp3_that_was_loaded() {
    let mut s = Session::new();
    let mp3 = write_mp3_sine(&s.user_dir("music").join("a.mp3"), 440.0);
    let before = bytes(&mp3);
    s.load_quietened(&mp3);
    let head = s.store.head().unwrap().to_hex();

    let result = s.call(
        "render_final",
        json!({ "node_id": head, "format": "mp3", "out_path": mp3.to_string_lossy() }),
    );
    assert_unchanged(&mp3, &before);
    let msg = err(result);

    assert!(msg.contains("a.mp3"), "{msg}");
}

/// What still works: a fresh path, and an earlier export of the project's
/// own (a file that exists but is nobody's source).
#[test]
fn render_final_still_writes_a_fresh_path_and_over_an_earlier_export() {
    let mut s = Session::new();
    let original = write_sine(&s.user_dir("music").join("take1.wav"), 440.0);
    let before = bytes(&original);
    ok(s.call("load", json!({ "path": original.to_string_lossy() })));
    let head = s.store.head().unwrap().to_hex();
    let out = s.user_dir("exports").join("take1-final.wav");

    for round in 0..2 {
        let v = ok(s.call(
            "render_final",
            json!({ "node_id": head, "format": "wav", "out_path": out.to_string_lossy() }),
        ));
        assert!(
            v["frames_written"].as_u64().unwrap() > 0,
            "round {round}: {v}"
        );
    }
    assert!(out.metadata().unwrap().len() > 44);
    assert_unchanged(&original, &before);
}

/// A second name for the same file is the same file.
#[test]
fn render_final_sees_through_a_hard_link() {
    let mut s = Session::new();
    let original = write_sine(&s.user_dir("music").join("take1.wav"), 440.0);
    let before = bytes(&original);
    let alias = s.user_dir("elsewhere").join("renamed.wav");
    std::fs::hard_link(&original, &alias).expect("hard link");
    s.load_quietened(&original);
    let head = s.store.head().unwrap().to_hex();

    let result = s.call(
        "render_final",
        json!({ "node_id": head, "format": "wav", "out_path": alias.to_string_lossy() }),
    );
    assert_unchanged(&original, &before);
    let msg = err(result);

    assert!(msg.contains("renamed.wav"), "{msg}");
}

/// A path through a symlink is the file it points at.
#[cfg(unix)]
#[test]
fn render_final_sees_through_a_symlink() {
    let mut s = Session::new();
    let original = write_sine(&s.user_dir("music").join("take1.wav"), 440.0);
    let before = bytes(&original);
    let link = s.user_dir("elsewhere").join("shortcut.wav");
    std::os::unix::fs::symlink(&original, &link).expect("symlink");
    s.load_quietened(&original);
    let head = s.store.head().unwrap().to_hex();

    let result = s.call(
        "render_final",
        json!({ "node_id": head, "format": "wav", "out_path": link.to_string_lossy() }),
    );
    assert_unchanged(&original, &before);
    let msg = err(result);

    assert!(msg.contains("shortcut.wav"), "{msg}");
    assert!(
        std::fs::symlink_metadata(&link).unwrap().is_symlink(),
        "and the link is still a link"
    );
}

/// A directory reached through a symlink is the same folder: `out_path`
/// spelt through it names the original.
#[cfg(unix)]
#[test]
fn render_final_sees_through_a_symlinked_folder() {
    let mut s = Session::new();
    let music = s.user_dir("music");
    let original = write_sine(&music.join("take1.wav"), 440.0);
    let before = bytes(&original);
    let folder_link = s.dir.path().join("music-link");
    std::os::unix::fs::symlink(&music, &folder_link).expect("symlink");
    s.load_quietened(&original);
    let head = s.store.head().unwrap().to_hex();

    let result = s.call(
        "render_final",
        json!({
            "node_id": head,
            "format": "wav",
            "out_path": folder_link.join("take1.wav").to_string_lossy(),
        }),
    );
    assert_unchanged(&original, &before);
    let msg = err(result);

    assert!(msg.contains("take1.wav"), "{msg}");
}

/// `take1.WAV` is `take1.wav` on a case-insensitive disk. Only macOS and
/// Windows have one by default (Linux's ext4 does not fold case), so this
/// runs there; it also checks at run time that the volume does fold, since
/// a case-sensitive APFS volume does not.
#[cfg(any(target_os = "macos", windows))]
#[test]
fn render_final_sees_through_a_different_spelling_of_the_name() {
    let mut s = Session::new();
    let original = write_sine(&s.user_dir("music").join("take1.wav"), 440.0);
    let shouted = original.with_file_name("TAKE1.WAV");
    if !shouted.exists() {
        eprintln!("skipped: this volume is case-sensitive");
        return;
    }
    let before = bytes(&original);
    s.load_quietened(&original);
    let head = s.store.head().unwrap().to_hex();

    let result = s.call(
        "render_final",
        json!({ "node_id": head, "format": "wav", "out_path": shouted.to_string_lossy() }),
    );
    assert_unchanged(&original, &before);
    let msg = err(result);

    assert!(msg.to_lowercase().contains("take1.wav"), "{msg}");
}

// =============================================================================
// the other writers that take a path
// =============================================================================

/// `export_recipe` writes JSON to `out_path`; pointed at a loaded original
/// it replaced the audio with text.
#[test]
fn export_recipe_will_not_write_over_a_loaded_original() {
    let mut s = Session::new();
    let original = write_sine(&s.user_dir("music").join("take1.wav"), 440.0);
    let before = bytes(&original);
    ok(s.call("load", json!({ "path": original.to_string_lossy() })));

    let result = s.call(
        "export_recipe",
        json!({ "out_path": original.to_string_lossy() }),
    );
    assert_unchanged(&original, &before);
    let msg = err(result);

    assert!(msg.contains("take1.wav"), "{msg}");
}

/// `export_multiple` names its files `<index>_<track name>.wav` in a folder
/// under the project. A loaded file that already has such a name there is
/// not to be replaced.
#[test]
fn export_multiple_will_not_write_over_a_loaded_original() {
    let mut s = Session::new();
    let project = s.store.project_dir().to_path_buf();
    let folder = project.join("takes");
    std::fs::create_dir_all(&folder).unwrap();
    let original = write_sine(&folder.join("0_vox.wav"), 440.0);
    let before = bytes(&original);
    s.load_quietened(&original);
    ok(s.call("rename_track", json!({ "track": 0, "name": "vox" })));

    let result = s.call(
        "export_multiple",
        json!({ "track_indices": [0], "output_dir": "takes" }),
    );
    assert_unchanged(&original, &before);
    let msg = err(result);

    assert!(msg.contains("0_vox.wav"), "{msg}");
}
