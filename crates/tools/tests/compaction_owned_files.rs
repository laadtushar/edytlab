//! `compact_session` deletes only files the project owns (#589).
//!
//! `load` references a streamable WAV the user picked *in place*, so the
//! node that loaded it names `~/Music/take.wav` as a clip source. Once
//! that node is pruned, the original appears only among the pruned
//! nodes' sources — and compaction used to delete every such file. The
//! user's own recording went with the history.
//!
//! A project owns what is under its `.audiograph/derived/`. Nothing else
//! a node names is the project's to delete: not the user's file, not a
//! clipboard blob, not a cached preview, and not a path that only
//! *looks* like it is in `derived/` (`..`, a symlink).

use std::path::{Path, PathBuf};

use hound::{SampleFormat, WavWriter};
use serde_json::{json, Value};
use tempfile::TempDir;
use tools::reclaim::{apply_compaction, plan_compaction, CompactionPlan};
use tools::{ToolContext, ToolDispatcher, ToolResult};

const SR: u32 = 48_000;

fn write_tone(path: &Path, seconds: f64, hz: f32) -> PathBuf {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: SR,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut w = WavWriter::create(path, spec).expect("wav writer");
    for n in 0..((SR as f64 * seconds) as usize) {
        let t = n as f32 / SR as f32;
        w.write_sample(((2.0 * std::f32::consts::PI * hz * t).sin() * 8000.0) as i16)
            .unwrap();
    }
    w.finalize().unwrap();
    path.to_path_buf()
}

fn ok(r: ToolResult) -> Value {
    match r {
        ToolResult::Ok(v) => v,
        ToolResult::Error(m) => panic!("expected Ok, got Error({m})"),
    }
}

/// The path with `..` and symlinks resolved; the path itself once the
/// file is gone, so an assertion about a deleted file fails on its own
/// message rather than here.
fn canon(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

/// A project folder, and — somewhere else entirely — the user's own
/// recording, loaded from where it is.
struct World {
    project: TempDir,
    /// Stands in for `~/Music`.
    _user: TempDir,
    /// The user's file, outside the project.
    outside: PathBuf,
    store: session::Store,
    engine: audio_engine::Engine,
    dispatcher: ToolDispatcher,
    clipboard: Option<tools::Clipboard>,
}

impl World {
    fn new() -> Self {
        let project = TempDir::new().expect("project dir");
        let user = TempDir::new().expect("user dir");
        let outside = write_tone(&user.path().join("take.wav"), 4.0, 440.0);
        let store = session::Store::open(project.path()).expect("open store");
        let mut w = Self {
            project,
            _user: user,
            outside,
            store,
            engine: audio_engine::Engine::new(),
            dispatcher: ToolDispatcher::default_dispatcher(),
            clipboard: None,
        };
        let loaded = w.call("load", json!({ "path": w.outside.to_string_lossy() }));
        ok(loaded);
        w
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

    /// Four *destructive* edits, so `derived/` has history in it.
    fn make_history(&mut self) {
        for start in [0.0, 1.0, 2.0, 3.0] {
            ok(self.call(
                "silence_region",
                json!({ "track": 0, "start_sec": start, "end_sec": start + 0.2 }),
            ));
        }
    }

    /// Append a node whose clip names `file`, the way a paste, a preview
    /// or a hand-edited project would. Moves the head onto it.
    fn append_naming(&mut self, file: &Path, label: &str) {
        let head = self.store.head().expect("head");
        let mut state = self.store.get(head).expect("head node").state;
        state.tracks[0].clips[0].source_path = file.to_path_buf();
        self.store
            .append(session::SessionNode {
                id: session::NodeId([0u8; 32]),
                parent: None,
                created_at: chrono::Utc::now(),
                label: Some(label.into()),
                reasoning: None,
                state,
                op: None,
            })
            .expect("append");
    }

    fn derived(&self) -> PathBuf {
        tools::provenance::derived_dir(self.project.path())
    }

    fn derived_files(&self) -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = std::fs::read_dir(self.derived())
            .map(|d| {
                d.flatten()
                    .map(|e| e.path())
                    .filter(|p| p.is_file())
                    .collect()
            })
            .unwrap_or_default();
        v.sort();
        v
    }

    fn derived_bytes(&self) -> u64 {
        self.derived_files()
            .iter()
            .filter_map(|p| std::fs::metadata(p).ok())
            .map(|m| m.len())
            .sum()
    }

    /// A tone written into `.audiograph/<dir>/<name>`, as long as the
    /// loaded file so the edits that follow still change it.
    fn cache_file(&self, dir: &str, name: &str, hz: f32) -> PathBuf {
        let d = self.project.path().join(".audiograph").join(dir);
        std::fs::create_dir_all(&d).unwrap();
        write_tone(&d.join(name), 4.0, hz)
    }

    /// How many nodes name `file` as a clip source.
    fn nodes_naming(&self, file: &Path) -> usize {
        let want = canon(file);
        self.store
            .list_nodes()
            .unwrap()
            .iter()
            .filter(|n| {
                n.state
                    .tracks
                    .iter()
                    .any(|t| t.clips.iter().any(|c| canon(&c.source_path) == want))
            })
            .count()
    }

    fn head(&self) -> session::NodeId {
        self.store.head().expect("head")
    }
}

/// The ticket's sequence, exactly: load a file from outside the project,
/// make destructive edits, compact with `keep_last: 1`.
#[test]
fn a_user_file_outside_the_project_survives_compaction() {
    let mut w = World::new();
    w.make_history();
    let before = std::fs::read(&w.outside).unwrap();
    assert_eq!(
        w.nodes_naming(&w.outside.clone()),
        1,
        "the load node names the user's file in place; without that this test proves nothing"
    );

    let v = ok(w.call("compact_session", json!({ "keep_last": 1, "apply": true })));

    assert_eq!(v["applied"], json!(true), "{v}");
    assert!(
        v["removed_nodes"].as_u64().unwrap_or(0) > 0,
        "the load node is among the pruned: {v}"
    );
    assert_eq!(
        w.nodes_naming(&w.outside.clone()),
        0,
        "and with it gone only pruned history named the file"
    );
    assert!(
        w.outside.is_file(),
        "the user's own recording was deleted from outside the project"
    );
    assert_eq!(std::fs::read(&w.outside).unwrap(), before);
}

/// A clipboard blob and a cached preview are the project's, but they are
/// not derived audio: other things (a paste, the preview cache's own
/// bound) manage them. Compaction removes neither.
#[test]
fn clipboard_blobs_and_cached_previews_are_not_removed() {
    let mut w = World::new();
    let blob = w.cache_file("clipboard", "blob.wav", 330.0);
    let preview = w.cache_file("previews", "mix.wav", 550.0);
    w.append_naming(&blob, "names a clipboard blob");
    w.append_naming(&preview, "names a cached preview");
    w.make_history();
    let (blob_bytes, preview_bytes) = (
        std::fs::read(&blob).unwrap(),
        std::fs::read(&preview).unwrap(),
    );
    assert_eq!(
        w.nodes_naming(&blob.clone()),
        1,
        "sanity: only pruned history names it"
    );
    assert_eq!(
        w.nodes_naming(&preview.clone()),
        1,
        "sanity: only pruned history names it"
    );

    ok(w.call("compact_session", json!({ "keep_last": 1, "apply": true })));

    assert!(blob.is_file(), "a clipboard blob was removed by compaction");
    assert!(
        preview.is_file(),
        "a cached preview was removed by compaction"
    );
    assert_eq!(std::fs::read(&blob).unwrap(), blob_bytes);
    assert_eq!(std::fs::read(&preview).unwrap(), preview_bytes);
    assert!(w.outside.is_file());
}

/// The feature keeps working: derived audio only pruned nodes name is
/// still removed, and what is reported is what left the disk.
#[test]
fn derived_audio_only_pruned_nodes_name_is_still_removed() {
    let mut w = World::new();
    w.make_history();
    let before_files = w.derived_files();
    let before_bytes = w.derived_bytes();
    assert!(
        before_files.len() >= 4,
        "four destructive edits wrote audio"
    );

    let v = ok(w.call("compact_session", json!({ "keep_last": 1, "apply": true })));

    let after_files = w.derived_files();
    let gone: Vec<&PathBuf> = before_files
        .iter()
        .filter(|f| !after_files.contains(f))
        .collect();
    assert!(!gone.is_empty(), "compaction freed nothing: {v}");
    assert_eq!(
        after_files.len(),
        1,
        "only the head's own audio is left: {after_files:?}"
    );
    assert_eq!(v["removed_files"], json!(gone.len()), "{v}");
    assert_eq!(
        v["freed_bytes"],
        json!(before_bytes - w.derived_bytes()),
        "the bytes reported are the bytes that left the disk: {v}"
    );
    // What the head names is still there and still opens.
    for track in &w.store.get(w.head()).unwrap().state.tracks {
        for clip in &track.clips {
            assert!(clip.source_path.is_file(), "{}", clip.source_path.display());
        }
    }
}

/// The preview lists, and counts, only what `apply` then removes — with
/// the user's file, a clipboard blob and a preview all named by pruned
/// nodes, none of them in it.
#[test]
fn the_preview_lists_only_what_apply_removes() {
    let mut w = World::new();
    let blob = w.cache_file("clipboard", "blob.wav", 330.0);
    let preview = w.cache_file("previews", "mix.wav", 550.0);
    w.append_naming(&blob, "names a clipboard blob");
    w.append_naming(&preview, "names a cached preview");
    w.make_history();
    let derived = canon(&w.derived());

    // The plan, directly: every file it lists is in `derived/`, and the
    // byte count is those files' size.
    let plan = plan_compaction(&w.store, w.head(), 1).expect("plan");
    assert!(!plan.files.is_empty(), "there is derived audio to reclaim");
    for f in &plan.files {
        assert!(
            canon(f).starts_with(&derived),
            "the plan lists a file outside derived/: {}",
            f.display()
        );
    }
    for protected in [&w.outside, &blob, &preview] {
        assert!(
            !plan.files.iter().any(|f| canon(f) == canon(protected)),
            "the plan lists {}",
            protected.display()
        );
    }
    let listed: u64 = plan
        .files
        .iter()
        .map(|f| std::fs::metadata(f).unwrap().len())
        .sum();
    assert_eq!(plan.bytes, listed);

    // The tool's preview, then the real thing.
    let before = w.derived_bytes();
    let preview_v = ok(w.call("compact_session", json!({ "keep_last": 1 })));
    assert_eq!(preview_v["applied"], json!(false), "{preview_v}");
    assert_eq!(w.derived_bytes(), before, "a preview removes nothing");
    let applied = ok(w.call("compact_session", json!({ "keep_last": 1, "apply": true })));

    assert_eq!(
        preview_v["reclaimable_bytes"], applied["freed_bytes"],
        "the preview promised {preview_v} and apply freed {applied}"
    );
    assert_eq!(
        preview_v["reclaimable_files"], applied["removed_files"],
        "{preview_v} vs {applied}"
    );
    assert_eq!(
        applied["freed_bytes"],
        json!(before - w.derived_bytes()),
        "and that is what left the disk"
    );
}

fn plan_for(files: Vec<PathBuf>) -> CompactionPlan {
    CompactionPlan {
        nodes: Vec::new(),
        new_root: None,
        bytes: files
            .iter()
            .filter_map(|f| std::fs::metadata(f).ok())
            .map(|m| m.len())
            .sum(),
        files,
        total_nodes: 0,
    }
}

/// A plan built somewhere else, naming whatever it likes: the second
/// guard. The outside file survives; the `derived/` file in the same
/// plan still goes; the refusal is counted rather than failing the
/// compaction.
#[test]
fn apply_refuses_a_hand_built_plan_that_names_outside_files() {
    let mut w = World::new();
    w.make_history();
    let blob = w.cache_file("clipboard", "blob.wav", 330.0);
    let preview = w.cache_file("previews", "mix.wav", 550.0);
    let derived_file = w
        .derived_files()
        .into_iter()
        .next()
        .expect("a derived file");
    let derived_len = std::fs::metadata(&derived_file).unwrap().len();
    let outside_bytes = std::fs::read(&w.outside).unwrap();

    let plan = plan_for(vec![
        w.outside.clone(),
        blob.clone(),
        derived_file.clone(),
        preview.clone(),
    ]);
    let report = apply_compaction(&mut w.store, &plan).expect("a refused path is not an error");

    assert!(
        w.outside.is_file(),
        "an outside file was deleted from a plan"
    );
    assert_eq!(std::fs::read(&w.outside).unwrap(), outside_bytes);
    assert!(blob.is_file(), "a clipboard blob was deleted from a plan");
    assert!(
        preview.is_file(),
        "a cached preview was deleted from a plan"
    );
    assert!(!derived_file.exists(), "the derived file in the plan goes");
    assert_eq!(report.removed_files, 1, "{report:?}");
    assert_eq!(report.freed_bytes, derived_len, "{report:?}");
    assert_eq!(
        report.kept_files, 3,
        "refusals are counted as kept: {report:?}"
    );
}

/// A path that reads as inside `derived/` and resolves elsewhere — `..`,
/// or a symlink — is not inside it. Compared canonically, in the plan and
/// again at the delete.
#[test]
fn a_path_that_only_looks_like_derived_is_not_derived() {
    let mut w = World::new();
    // A file in the project folder but not in derived/: the shape of a
    // user dropping `take.wav` next to their project.
    let beside = write_tone(&w.project.path().join("beside.wav"), 1.0, 220.0);
    // `..` only resolves through a directory that exists.
    std::fs::create_dir_all(w.derived()).unwrap();
    let dotdot = w.derived().join("..").join("..").join("beside.wav");
    assert!(dotdot.is_file(), "the test's own path must resolve");

    let escapes = {
        // `mut` is used only by the unix-only symlink case below.
        #[cfg_attr(not(unix), allow(unused_mut))]
        let mut escapes = vec![(dotdot, beside.clone())];
        #[cfg(unix)]
        {
            let link = w.derived().join("link.wav");
            std::os::unix::fs::symlink(&w.outside, &link).unwrap();
            escapes.push((link, w.outside.clone()));
        }
        escapes
    };

    for (spelled, real) in escapes {
        let bytes = std::fs::read(&real).unwrap();

        // Named by a node that will be pruned: the plan leaves it out.
        w.append_naming(&spelled, "names a path that is not derived");
        w.make_history();
        let plan = plan_compaction(&w.store, w.head(), 1).expect("plan");
        assert!(
            !plan.files.iter().any(|f| canon(f) == canon(&real)),
            "the plan lists {} through {}",
            real.display(),
            spelled.display()
        );

        // Named by a plan built elsewhere: apply leaves it alone.
        let report = apply_compaction(&mut w.store, &plan_for(vec![spelled.clone()])).unwrap();
        assert_eq!(report.removed_files, 0, "{report:?}");
        assert_eq!(report.kept_files, 1, "{report:?}");
        assert_eq!(
            std::fs::read(&real).unwrap(),
            bytes,
            "{} was removed or changed through {}",
            real.display(),
            spelled.display()
        );
    }
}
