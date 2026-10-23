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
//! is history only, the second's is the head's), a copy and paste onto the
//! second, which no replay can redo, and a generated tone, which lives in
//! `<project>/generated/` rather than the store. The third take is
//! untouched and plays from where the user keeps it.

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
            run(
                "generate_tone",
                json!({ "frequency_hz": 660.0, "duration_sec": 0.5 }),
            );
        }
        let reference = render(&store);
        drop(store);
        assert_eq!(
            files(&derived(&original)).len(),
            3,
            "two silences and a paste each wrote a file"
        );
        assert_eq!(
            files(&original.join("generated")).len(),
            1,
            "the tone wrote one"
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
                        || path.starts_with(self.copy.join("generated"))
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
    // History the sweep took comes back, into the copy, when it is wanted.
    for node in &w.nodes {
        tools::rederive::materialize(&store, *node)
            .unwrap_or_else(|e| panic!("node {} cannot be rebuilt: {e}", node.to_hex()));
    }
    w.assert_self_contained();
}

/// History the copy's own sweep took comes back into the copy once the
/// original is gone: the copied node names the original's file, which no
/// longer exists, and a rebuild puts the file in the copy's `derived/`
/// under its name, where a read then finds it.
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

/// The generated tone is the copy's own once the original is gone, as the
/// store's audio is: `generated/` is named by content and travels with
/// the folder, and no replay writes it, so there is no other way back.
#[test]
fn a_generated_track_plays_once_the_original_is_deleted() {
    let w = World::new();
    tools::reclaim::sweep_orphans(&w.copy_store()).unwrap();
    std::fs::remove_dir_all(&w.original).unwrap();

    let store = w.copy_store();
    let tone = store.get(w.head()).unwrap().state.tracks[3].clips[0]
        .source_path
        .clone();
    assert!(
        tone.starts_with(w.copy.join("generated")) && tone.is_file(),
        "the copy's tone is gone with the original: {}",
        tone.display()
    );
    assert!(render(&store) == w.reference);
}

/// The original is out of reach (its drive unplugged), and the copy's own
/// file of a name is gone (its history sweep took it). A read names what
/// the node says, the original's file; a rebuild puts the file back in the
/// copy's `derived/` by name, never in the original's folder; and a read
/// then finds the copy's file, before and after the drive comes back.
#[test]
fn a_gone_file_of_another_project_is_looked_for_here() {
    let w = World::new();
    {
        let store = w.copy_store();
        let report = tools::reclaim::sweep(&store, 0).unwrap();
        assert_eq!(
            report.removed_files, 1,
            "the first silence went: {report:?}"
        );
    }
    let offline = w.original.with_file_name("unplugged");
    std::fs::rename(&w.original, &offline).unwrap();
    let older = w.nodes[3];
    let store = w.copy_store();
    let missing = tools::rederive::missing_paths(&store, older);
    assert_eq!(missing.len(), 1, "{missing:?}");
    assert!(
        missing[0].starts_with(derived(&w.original)),
        "a read points at a file that does not exist: {}",
        missing[0].display()
    );

    let rebuilt = tools::rederive::materialize(&store, older).expect("rebuilt");

    assert_eq!(
        rebuilt,
        vec![derived(&w.copy).join(missing[0].file_name().unwrap())]
    );
    assert!(
        !w.original.exists(),
        "nothing was written where the original was"
    );
    let read = |store: &session::Store| {
        store.get(older).unwrap().state.tracks[0].clips[0]
            .source_path
            .clone()
    };
    assert_eq!(read(&store), rebuilt[0]);
    std::fs::rename(&offline, &w.original).unwrap();
    assert_eq!(read(&w.copy_store()), rebuilt[0]);
}

impl World {
    /// What opening the copy did in 0.6.0: its orphan sweep took every
    /// file in its `derived/`.
    fn swept_by_0_6_0(&self) {
        for name in files(&derived(&self.copy)).keys() {
            std::fs::remove_file(derived(&self.copy).join(name)).unwrap();
        }
    }
}

/// A copy saved by 0.6.0, opened with the original still there: the repair
/// copies in every file of the original's that the copy's nodes name and
/// the copy has lost, and the original can then be deleted.
#[test]
fn the_repair_brings_a_0_6_0_copys_audio_home() {
    let w = World::new();
    w.swept_by_0_6_0();
    let original = tree(&w.original);

    let plan =
        tools::repair::plan_repair(&w.copy_store(), tools::reclaim::DEFAULT_DERIVED_CAP_BYTES);
    let report = tools::repair::apply_repair(&plan);

    assert_eq!(report.copied, 3, "{report:?}");
    assert_eq!(report.failed, 0, "{report:?}");
    assert_eq!(files(&derived(&w.copy)), files(&derived(&w.original)));
    assert_eq!(tree(&w.original), original, "the original is untouched");
    w.assert_self_contained();
    assert!(
        tools::repair::plan_repair(&w.copy_store(), tools::reclaim::DEFAULT_DERIVED_CAP_BYTES)
            .is_empty(),
        "a second open has nothing to copy"
    );

    std::fs::remove_dir_all(&w.original).unwrap();
    let store = w.copy_store();
    for node in &w.nodes {
        assert_eq!(
            tools::rederive::missing_paths(&store, *node),
            Vec::<PathBuf>::new(),
            "node {} lost audio",
            node.to_hex()
        );
    }
    assert!(render(&store) == w.reference);
}

/// Past the history sweep's cap the repair copies only what that sweep
/// never deletes: the head's audio, and audio no replay can rebuild (the
/// paste). Copying the rest would push the copy over its cap, the sweep
/// would delete it again, and the next open would copy it back. Left out,
/// it is rebuilt into the copy when it is wanted.
#[test]
fn past_the_cap_the_repair_leaves_what_a_replay_rebuilds() {
    let w = World::new();
    w.swept_by_0_6_0();

    let report = tools::repair::apply_repair(&tools::repair::plan_repair(&w.copy_store(), 0));
    assert_eq!(report.copied, 2, "the head's two edits: {report:?}");

    std::fs::remove_dir_all(&w.original).unwrap();
    let older = w.nodes[3];
    let store = w.copy_store();
    assert_eq!(
        tools::rederive::missing_paths(&store, w.head()),
        Vec::<PathBuf>::new()
    );
    let rebuilt = tools::rederive::materialize(&store, older).expect("rebuilt");
    assert_eq!(rebuilt.len(), 1);
    assert!(rebuilt[0].starts_with(derived(&w.copy)));
}

/// The repair adds files and never replaces one: a file that appears under
/// a planned name before the copy lands (another open's repair, an edit
/// that wrote the same audio) is left as it is.
#[test]
fn the_repair_never_replaces_a_file() {
    let w = World::new();
    w.swept_by_0_6_0();
    let plan =
        tools::repair::plan_repair(&w.copy_store(), tools::reclaim::DEFAULT_DERIVED_CAP_BYTES);
    let first = plan.files()[0].to().to_path_buf();
    std::fs::write(&first, b"already here").unwrap();

    let report = tools::repair::apply_repair(&plan);

    assert_eq!(report.copied, 2, "{report:?}");
    assert_eq!(report.already_there, 1, "{report:?}");
    assert_eq!(std::fs::read(&first).unwrap(), b"already here");
    let leftovers: Vec<_> = files(&derived(&w.copy))
        .into_keys()
        .filter(|n| !n.ends_with(".wav") || n.starts_with('.'))
        .collect();
    assert!(
        leftovers.is_empty(),
        "staging files left behind: {leftovers:?}"
    );
}

/// Only `derived/` audio is rebuilt: it is all a replay writes. A copy
/// whose node names another project's clipboard blob and generated file,
/// both gone with the original and missing from the copy, has nothing to
/// rebuild: `materialize` writes nothing into the copy's `clipboard/` or
/// `generated/` (a replay's output under those names would not be the
/// blob or the tone) and leaves the read to report them missing.
#[test]
fn only_derived_audio_is_a_rebuild_target() {
    let tmp = TempDir::new().unwrap();
    let original = tmp.path().join("original");
    let copy = tmp.path().join("copy");
    let blob = session::relocate::clipboard_dir(&original).join("1111.wav");
    let tone = session::relocate::generated_dir(&original).join("2222.wav");
    for file in [&blob, &tone] {
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        write_tone(file, 0.1, 440.0);
    }
    let node = {
        let mut store = session::Store::open(&original).unwrap();
        let mut state = session::SessionState {
            tracks: Vec::new(),
            bus_routing: Default::default(),
            master_chain: Vec::new(),
            tempo_map: Default::default(),
            key_map: None,
            transcript: None,
            sample_rate: SR,
            length_samples: 4_800,
            annotations: Vec::new(),
            sync_lock: false,
        };
        for path in [&blob, &tone] {
            state.tracks.push(session::Track {
                id: session::TrackId(uuid::Uuid::new_v4()),
                name: "t".into(),
                clips: vec![session::Clip {
                    source_path: path.to_path_buf(),
                    start_in_track: 0,
                    source_offset: 0,
                    length: 4_800,
                    content_hash: None,
                    time_stretch_factor: None,
                    pitch_shift_semitones: None,
                    beat_grid: None,
                    volume_envelope: Vec::new(),
                    volume_envelope_frames: Default::default(),
                }],
                gain_db: 0.0,
                pan: 0.0,
                muted: false,
                soloed: false,
                effects: Vec::new(),
                sends: Vec::new(),
            });
        }
        store
            .append(session::SessionNode {
                id: session::NodeId([0; 32]),
                parent: None,
                created_at: chrono::Utc::now(),
                label: None,
                reasoning: None,
                state,
                op: None,
            })
            .unwrap()
    };
    copy_folder(&original, &copy);
    std::fs::remove_dir_all(&original).unwrap();
    for dir in [
        session::relocate::clipboard_dir(&copy),
        session::relocate::generated_dir(&copy),
    ] {
        std::fs::remove_dir_all(dir).unwrap();
    }
    let store = session::Store::open(&copy).unwrap();

    assert_eq!(
        tools::rederive::materialize(&store, node),
        Ok(Vec::new()),
        "nothing to rebuild"
    );
    assert_eq!(
        tools::rederive::missing_paths(&store, node),
        vec![blob, tone]
    );
    assert!(!session::relocate::clipboard_dir(&copy).exists());
    assert!(!session::relocate::generated_dir(&copy).exists());
}

/// A repair cut short by a nearly full disk has put back the head's audio
/// first, so the copy plays as it stands even if the original goes before
/// the next open finishes the job.
#[test]
fn a_repair_cut_short_for_space_puts_back_the_heads_audio_first() {
    let w = World::new();
    w.swept_by_0_6_0();
    let plan =
        tools::repair::plan_repair(&w.copy_store(), tools::reclaim::DEFAULT_DERIVED_CAP_BYTES);
    assert_eq!(plan.files().len(), 3);
    let mut asked = 0;

    let report = tools::repair::apply_repair_with(&plan, |_| {
        asked += 1;
        Ok(if asked <= 2 { u64::MAX / 2 } else { 0 })
    });

    assert_eq!(report.copied, 2, "{report:?}");
    assert_eq!(report.left_for_space, 1, "{report:?}");
    std::fs::remove_dir_all(&w.original).unwrap();
    let store = w.copy_store();
    assert_eq!(
        tools::rederive::missing_paths(&store, w.head()),
        Vec::<PathBuf>::new(),
        "the head's audio came home first"
    );
    assert!(render(&store) == w.reference);
}
