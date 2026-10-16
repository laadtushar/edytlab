//! The sweeps on a copy of a project, with the original still on disk
//! (#612).
//!
//! Save As copies the project folder and opens the copy, and every open
//! runs the orphan sweep; a folder copied by hand is the same once it is
//! opened. The copy's node files are the original's, so their clips name
//! the original's `derived/` and `clipboard/` files, which still exist.
//! The sweeps decide what to keep from what the nodes name, so a copy that
//! read its nodes with the original's paths took its own audio for
//! orphans and deleted it, and was left playing from the original's files.
//! Deleting the original, the second half of moving a project, then left
//! it with whatever a replay could rebuild: an edit no replay can redo (a
//! paste, here) was gone.
//!
//! The history here is the agent's tools: three takes loaded from outside
//! the project, two destructive edits on the first (the first edit's audio
//! is history only, the second's is the head's), and a copy and paste
//! onto the second, which no replay can redo. The third take is untouched
//! and plays from where the user keeps it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use hound::{SampleFormat, WavWriter};
use serde_json::{json, Value};
use tempfile::TempDir;
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

/// Copy a folder the way a file manager does: every file, nothing left out.
fn copy_folder(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_folder(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// Every file in `dir`, by name, with its bytes.
fn files(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    std::fs::read_dir(dir)
        .map(|d| {
            d.flatten()
                .filter(|e| e.path().is_file())
                .map(|e| {
                    (
                        e.file_name().to_string_lossy().into_owned(),
                        std::fs::read(e.path()).unwrap(),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Every file under `dir`, at any depth, by path relative to it, with its
/// bytes.
fn tree(dir: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                out.insert(
                    path.strip_prefix(root).unwrap().to_path_buf(),
                    std::fs::read(&path).unwrap(),
                );
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

fn derived(project: &Path) -> PathBuf {
    tools::derived_dir(project)
}

struct World {
    _tmp: TempDir,
    original: PathBuf,
    copy: PathBuf,
    /// The takes, where the user keeps them.
    takes: Vec<PathBuf>,
    /// Oldest first.
    nodes: Vec<session::NodeId>,
    /// The original's head, rendered before anything was copied.
    reference: Vec<u8>,
}

impl World {
    fn new() -> Self {
        let tmp = TempDir::new().unwrap();
        let original = tmp.path().join("original");
        let copy = tmp.path().join("copy");
        let music = tmp.path().join("music");
        std::fs::create_dir_all(&music).unwrap();
        let takes: Vec<PathBuf> = [220.0, 330.0, 440.0]
            .iter()
            .enumerate()
            .map(|(i, hz)| write_tone(&music.join(format!("take-{i}.wav")), 2.0, *hz))
            .collect();

        let mut store = session::Store::open(&original).unwrap();
        let mut engine = audio_engine::Engine::new();
        let mut clipboard = None;
        let dispatcher = ToolDispatcher::default_dispatcher();
        let mut nodes = Vec::new();
        {
            let mut ctx = ToolContext {
                store: &mut store,
                engine: &mut engine,
                user_message: "",
                clipboard: &mut clipboard,
                allowed_tools: None,
            };
            let mut run = |tool: &str, args: Value| {
                match dispatcher.invoke(tool, args, &mut ctx).expect("invoke") {
                    ToolResult::Ok(_) => {}
                    ToolResult::Error(m) => panic!("{tool} failed: {m}"),
                }
                let head = ctx.store.head().expect("a head");
                if nodes.last() != Some(&head) {
                    nodes.push(head);
                }
            };
            for take in &takes {
                run("load", json!({ "path": take }));
            }
            run(
                "silence_region",
                json!({ "track": 0, "start_sec": 0.2, "end_sec": 0.4 }),
            );
            run(
                "copy_region",
                json!({ "track": 0, "range": { "start_sec": 0.5, "end_sec": 0.9 } }),
            );
            run("paste_region", json!({ "track": 1, "at": 1.0 }));
            run(
                "silence_region",
                json!({ "track": 0, "start_sec": 1.2, "end_sec": 1.4 }),
            );
        }
        let reference = render(&store);
        drop(store);
        assert_eq!(
            files(&derived(&original)).len(),
            3,
            "two silences and a paste each wrote a file"
        );
        copy_folder(&original, &copy);

        Self {
            _tmp: tmp,
            original,
            copy,
            takes,
            nodes,
            reference,
        }
    }

    fn head(&self) -> session::NodeId {
        *self.nodes.last().unwrap()
    }

    /// The copy as every reader of it sees it.
    fn copy_store(&self) -> session::Store {
        session::Store::open(&self.copy).unwrap()
    }

    /// The copy names its own files and the user's takes, and nothing of
    /// the original's, in every node.
    fn assert_self_contained(&self) {
        let store = self.copy_store();
        for node in store.list_nodes().unwrap() {
            for clip in node.state.tracks.iter().flat_map(|t| &t.clips) {
                let path = &clip.source_path;
                assert!(
                    path.starts_with(self.copy.join(session::STORE_DIR))
                        || self.takes.contains(path),
                    "node {} names a file that is not the copy's: {}",
                    node.id.to_hex(),
                    path.display()
                );
            }
        }
    }
}

/// The head of `store`, rendered, as the bytes it came out as.
fn render(store: &session::Store) -> Vec<u8> {
    let state = store.get(store.head().unwrap()).unwrap().state;
    let out = tempfile::NamedTempFile::new().unwrap();
    audio_engine::render_state_to_wav(&state, out.path(), None)
        .unwrap_or_else(|e| panic!("the head does not render: {e}"));
    std::fs::read(out.path()).unwrap()
}

/// The open sweep, which Save As runs on the copy as it opens it.
#[test]
fn opening_a_copy_sweeps_none_of_its_audio() {
    let w = World::new();
    let before = files(&derived(&w.copy));
    let original = tree(&w.original);

    let report = tools::reclaim::sweep_orphans(&w.copy_store()).unwrap();

    assert_eq!(
        report.removed_orphans, 0,
        "the copy's own audio was taken for orphans: {report:?}"
    );
    assert_eq!(files(&derived(&w.copy)), before);
    assert_eq!(tree(&w.original), original, "the original is untouched");
    w.assert_self_contained();
}

/// The background reclaimer, over its cap: the head's files stay, and
/// none of the copy's named audio is an orphan. Only history a replay has
/// rebuilt may go.
#[test]
fn the_history_sweep_over_its_cap_takes_none_of_a_copys_audio_for_orphans() {
    let w = World::new();
    let original = tree(&w.original);
    let store = w.copy_store();

    let plan = tools::reclaim::plan_sweep(&store, 0).unwrap();
    let report = tools::reclaim::apply_sweep(&store, &plan).unwrap();

    assert_eq!(report.removed_orphans, 0, "{report:?}");
    assert_eq!(
        tools::rederive::missing_paths(&store, w.head()),
        Vec::<PathBuf>::new(),
        "the head's audio is all there"
    );
    assert_eq!(tree(&w.original), original, "the original is untouched");
    assert!(
        report.removed_files <= 1,
        "only the first silence's audio is history a replay can rebuild: {report:?}"
    );
}

/// The whole of #612: Save As opens the copy (the open sweep), the
/// reclaimer runs on it, and then the original is deleted. The copy's head
/// still plays every clip, from the copy's own files, with no replay: the
/// paste on its second track is one no replay can redo.
#[test]
fn a_copy_plays_after_its_sweeps_and_the_original_is_deleted() {
    let w = World::new();
    {
        let store = w.copy_store();
        tools::reclaim::sweep_orphans(&store).unwrap();
        tools::reclaim::sweep(&store, 0).unwrap();
    }
    std::fs::remove_dir_all(&w.original).unwrap();

    let store = w.copy_store();
    assert_eq!(
        tools::rederive::missing_paths(&store, w.head()),
        Vec::<PathBuf>::new(),
        "the copy's head lost audio with the original"
    );
    assert!(
        render(&store) == w.reference,
        "the copy's head renders differently"
    );
    w.assert_self_contained();
}

/// History the copy's own sweep took comes back into the copy once the
/// original is gone: the copied node names the original's file, which no
/// longer exists, so it reads as the copy's file of that name, which is
/// where a rebuild puts it.
#[test]
fn undo_in_a_copy_rebuilds_its_swept_history_into_the_copy() {
    let w = World::new();
    {
        let store = w.copy_store();
        let report = tools::reclaim::sweep(&store, 0).unwrap();
        assert_eq!(
            report.removed_files, 1,
            "the first silence went: {report:?}"
        );
    }
    std::fs::remove_dir_all(&w.original).unwrap();
    // The first silence, before the copy and paste.
    let older = w.nodes[3];
    let store = w.copy_store();
    assert!(!tools::rederive::missing_paths(&store, older).is_empty());

    let rebuilt = tools::rederive::materialize(&store, older).expect("rebuilt");

    assert_eq!(rebuilt.len(), 1, "{rebuilt:?}");
    assert!(
        rebuilt[0].starts_with(derived(&w.copy)),
        "{}",
        rebuilt[0].display()
    );
    assert_eq!(
        tools::rederive::missing_paths(&store, older),
        Vec::<PathBuf>::new()
    );
    assert!(
        !w.original.exists(),
        "the rebuild wrote nothing into the deleted original's folder"
    );
}

/// Compaction of a copy frees the copy's files only, never the
/// original's, which are another project's (#593).
#[test]
fn compacting_a_copy_frees_its_own_files_and_none_of_the_originals() {
    let w = World::new();
    let original = tree(&w.original);
    let mut store = w.copy_store();

    let plan = tools::reclaim::plan_compaction(&store, w.head(), 1).unwrap();
    for file in &plan.files {
        assert!(
            file.starts_with(std::fs::canonicalize(derived(&w.copy)).unwrap()),
            "compaction of the copy plans to delete {}",
            file.display()
        );
    }
    let report = tools::reclaim::apply_compaction(&mut store, &plan).unwrap();

    assert_eq!(tree(&w.original), original, "the original is untouched");
    assert_eq!(
        report.removed_files, 1,
        "the first silence's audio, which only pruned nodes name: {report:?}"
    );
    assert_eq!(report.kept_files, 0, "{report:?}");
    assert!(render(&store) == w.reference);
}
