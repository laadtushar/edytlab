//! Projects written before #620 keep working after it.
//!
//! Until #620 a destructive edit's output was named by the blake3 of its
//! samples alone, and now it is named by `provenance::audio_hash` (the
//! samples with their rate and channel count). Nothing on disk is renamed,
//! so a project that is already there names files of the older kind, and
//! two things compare a name to a replay's: the history sweep's `Verifier`
//! (#356) and `rederive::materialize`. Names are compared, never recomputed
//! from a file, so a replay of such a node has to be recognised by the name
//! its samples would have had.
//!
//! The older history is built here the way the older build wrote it: every
//! derived file under its samples-only name, the node naming it, and the
//! clip's `content_hash` (which always was that same hash).

use std::path::{Path, PathBuf};

use hound::{SampleFormat, WavSpec, WavWriter};
use serde_json::{json, Value};
use session::{NodeId, SessionNode, Store};
use tempfile::TempDir;
use tools::provenance::{audio_hash, derived_dir, legacy_sample_hash_name, legacy_stem};
use tools::{ToolContext, ToolDispatcher, ToolResult};

const SR: u32 = 48_000;

/// Edits at these seconds, each a tenth of a second long.
const EDITS: [f64; 3] = [0.0, 0.3, 0.6];

fn write_take(path: &Path) {
    let spec = WavSpec {
        channels: 1,
        sample_rate: SR,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut w = WavWriter::create(path, spec).expect("wav writer");
    for n in 0..SR as usize {
        let t = n as f32 / SR as f32;
        w.write_sample(((2.0 * std::f32::consts::PI * 440.0 * t).sin() * 8000.0) as i16)
            .unwrap();
    }
    w.finalize().unwrap();
}

struct Session {
    dir: TempDir,
    store: Store,
    engine: audio_engine::Engine,
    dispatcher: ToolDispatcher,
    clipboard: Option<tools::Clipboard>,
}

impl Session {
    fn new() -> Self {
        let dir = TempDir::new().expect("tempdir");
        let store = Store::open(dir.path()).expect("store");
        Self {
            dir,
            store,
            engine: audio_engine::Engine::new(),
            dispatcher: ToolDispatcher::default_dispatcher(),
            clipboard: None,
        }
    }

    fn call(&mut self, tool: &str, args: Value) -> Value {
        let mut ctx = ToolContext {
            store: &mut self.store,
            engine: &mut self.engine,
            user_message: "",
            clipboard: &mut self.clipboard,
            allowed_tools: None,
        };
        match self
            .dispatcher
            .invoke(tool, args, &mut ctx)
            .expect("invoke")
        {
            ToolResult::Ok(v) => v,
            ToolResult::Error(m) => panic!("`{tool}` failed: {m}"),
        }
    }

    fn silence(&mut self, start_sec: f64) {
        self.call(
            "silence_region",
            json!({ "track": 0, "start_sec": start_sec, "end_sec": start_sec + 0.1 }),
        );
    }

    fn head(&self) -> NodeId {
        self.store.head().expect("head")
    }

    fn node(&self, id: NodeId) -> SessionNode {
        self.store.get(id).expect("node")
    }

    /// The nodes from the root to the head.
    fn chain(&self) -> Vec<SessionNode> {
        let mut out = Vec::new();
        let mut cursor = Some(self.head());
        while let Some(id) = cursor {
            let n = self.node(id);
            cursor = n.parent;
            out.push(n);
        }
        out.reverse();
        out
    }

    /// The file node `id`'s first track plays.
    fn file_of(&self, id: NodeId) -> PathBuf {
        self.node(id).state.tracks[0].clips[0].source_path.clone()
    }

    /// The names in this project's `derived/`, sorted.
    fn derived_names(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(derived_dir(self.dir.path()))
            .map(|d| {
                d.flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }
}

/// A project that only the build before #620 could have written: a `load`
/// of `take` and an edit for each of [`EDITS`], every edit's file under the
/// name its samples alone had.
///
/// It is made by running the real tools in a project of their own and
/// writing down what the older build would have: the same node, op and
/// bytes, the file under the name in the clip's `content_hash` (the
/// blake3 of the samples, which is what the older build named it by).
/// `corrupt` gives the first edit a name its samples never had.
fn project_from_before(take: &Path, corrupt: bool) -> Session {
    let mut now = Session::new();
    now.call("load", json!({ "path": take }));
    for start in EDITS {
        now.silence(start);
    }

    let mut old = Session::new();
    let mut first_edit = true;
    for node in now.chain() {
        let mut state = node.state.clone();
        for clip in state.tracks.iter_mut().flat_map(|t| t.clips.iter_mut()) {
            if clip.source_path.parent() != Some(derived_dir(now.dir.path()).as_path()) {
                continue;
            }
            let mut stem = legacy_stem(&clip.content_hash.expect("an edit records its samples"));
            if corrupt && first_edit {
                // A hash of other samples: nothing a replay writes.
                stem = legacy_sample_hash_name(&[0.5, -0.5]);
                first_edit = false;
            }
            let older = derived_dir(old.dir.path()).join(format!("{stem}.wav"));
            std::fs::create_dir_all(older.parent().unwrap()).unwrap();
            std::fs::copy(&clip.source_path, &older).expect("write the older file");
            clip.source_path = older;
        }
        let id = old
            .store
            .append(SessionNode {
                id: NodeId([0; 32]),
                parent: None,
                created_at: node.created_at,
                label: node.label.clone(),
                reasoning: None,
                state,
                op: None,
            })
            .expect("append");
        if let Some(op) = node.op {
            old.store.set_op(id, op).expect("op");
        }
    }
    old
}

/// What `files` hold, to compare with what a rebuild puts back.
fn bytes_of(files: &[PathBuf]) -> Vec<Vec<u8>> {
    files
        .iter()
        .map(|p| std::fs::read(p).unwrap_or_else(|e| panic!("read {}: {e}", p.display())))
        .collect()
}

/// A project of the older kind, to have its edits swept.
struct Old {
    /// Holds the take that the first node loads, which a replay reads.
    _tmp: TempDir,
    project: Session,
}

impl Old {
    fn new(corrupt: bool) -> Self {
        let tmp = TempDir::new().expect("tempdir");
        let take = tmp.path().join("take.wav");
        write_take(&take);
        let project = project_from_before(&take, corrupt);
        Self { _tmp: tmp, project }
    }

    /// The edits' files, oldest first.
    fn edit_files(&self) -> Vec<PathBuf> {
        self.project
            .chain()
            .iter()
            .skip(1)
            .map(|n| self.project.file_of(n.id))
            .collect()
    }
}

#[test]
fn the_fixture_is_what_the_older_build_wrote() {
    let old = Old::new(false);
    let edits = old.edit_files();
    assert_eq!(edits.len(), EDITS.len());
    for (file, node) in edits.iter().zip(old.project.chain().iter().skip(1)) {
        let clip = &node.state.tracks[0].clips[0];
        // The name is the blake3 of the samples alone, the clip's own hash.
        assert_eq!(
            file.file_stem().unwrap().to_string_lossy(),
            legacy_stem(&clip.content_hash.unwrap())
        );
        assert!(file.is_file());
        assert!(node.op.as_ref().is_some_and(|op| op.reproducible));
    }
    assert_eq!(old.project.derived_names().len(), EDITS.len());
}

/// **A brand-new edit is named with the new scheme**, and its clip still
/// records the samples-only hash the older name came from. The first edit
/// is worked out here from the take, independently of the tool: the
/// silenced decode, hashed both ways.
#[test]
fn a_new_edit_is_named_by_audio_hash_and_records_the_older_hash() {
    let tmp = TempDir::new().expect("tempdir");
    let take = tmp.path().join("take.wav");
    write_take(&take);
    let mut s = Session::new();
    s.call("load", json!({ "path": take }));
    s.silence(EDITS[0]);

    let mut samples = audio_decoder::decode_file(&take).expect("decode").samples;
    for v in &mut samples[..(0.1 * SR as f64) as usize] {
        *v = 0.0;
    }
    let clip = &s.node(s.head()).state.tracks[0].clips[0];
    assert_eq!(
        clip.source_path.file_stem().unwrap().to_string_lossy(),
        audio_hash(&samples, SR, 1),
        "named for the samples with their rate and channel count"
    );
    assert_ne!(
        audio_hash(&samples, SR, 1),
        legacy_sample_hash_name(&samples),
        "and not the older name"
    );
    assert_eq!(
        legacy_stem(&clip.content_hash.expect("recorded")),
        legacy_sample_hash_name(&samples),
        "the clip still holds the blake3 of the samples"
    );
}

/// **The sweep still verifies older history.** Every file the head does not
/// name goes, as it did before, with nothing kept for want of a proof.
#[test]
fn the_sweep_still_verifies_and_removes_files_named_the_older_way() {
    let old = Old::new(false);
    let report = tools::reclaim::sweep(&old.project.store, 0).expect("sweep");

    assert_eq!(report.kept_unverified, 0, "{report:?}");
    assert_eq!(report.kept_unrebuildable, 0, "{report:?}");
    assert_eq!(report.removed_files, EDITS.len() - 1, "{report:?}");
    let files = old.edit_files();
    let (head, gone) = files.split_last().unwrap();
    assert!(head.is_file(), "the head's audio is never swept");
    assert!(gone.iter().all(|f| !f.exists()), "history's audio went");
}

/// **A sweep made before still rebuilds after.** The files are removed by
/// the sweep, and `materialize` puts each back under the name the node
/// records, with the bytes it had. No file of the new kind appears.
#[test]
fn materialize_rebuilds_a_swept_file_under_its_older_name() {
    let old = Old::new(false);
    let files = old.edit_files();
    let before = bytes_of(&files);
    let names = old.project.derived_names();
    tools::reclaim::sweep(&old.project.store, 0).expect("sweep");

    // The head's parent: its file and its own parent's are both gone.
    let chain = old.project.chain();
    let target = chain[chain.len() - 2].id;
    assert!(!tools::rederive::missing_paths(&old.project.store, target).is_empty());

    let rebuilt = tools::rederive::materialize(&old.project.store, target).expect("materialize");
    assert_eq!(rebuilt, vec![files[EDITS.len() - 2].clone()]);
    for (file, bytes) in files.iter().zip(&before).take(EDITS.len() - 1) {
        // The parent's file too: the replay started from the audio before.
        assert_eq!(
            &std::fs::read(file).unwrap_or_else(|_| panic!("{} is back", file.display())),
            bytes,
            "{} came back different",
            file.display()
        );
    }
    assert_eq!(
        old.project.derived_names(),
        names,
        "the same names as before the sweep, and none of the new kind"
    );
}

/// The sweep that removed the files may be an earlier build's, which named
/// its replays' files the older way. The files are gone all the same, and
/// `materialize` has to put them back without that sweep being run again:
/// a replay now writes the new name, and the node records the older one.
#[test]
fn materialize_rebuilds_files_an_earlier_builds_sweep_removed() {
    let old = Old::new(false);
    let files = old.edit_files();
    let before = bytes_of(&files);
    for gone in &files[..EDITS.len() - 1] {
        std::fs::remove_file(gone).expect("remove, as the earlier sweep did");
    }

    let chain = old.project.chain();
    let target = chain[chain.len() - 2].id;
    let rebuilt = tools::rederive::materialize(&old.project.store, target)
        .expect("a node written before #620 is still rebuilt");

    assert_eq!(rebuilt, vec![files[EDITS.len() - 2].clone()]);
    for (file, bytes) in files.iter().zip(&before) {
        assert_eq!(&std::fs::read(file).expect("back"), bytes);
    }
    assert_eq!(old.project.derived_names().len(), EDITS.len());
}

/// Every node of the older history can be made to play again, one at a time.
#[test]
fn every_swept_node_can_be_materialized() {
    let old = Old::new(false);
    let files = old.edit_files();
    let before = bytes_of(&files);
    tools::reclaim::sweep(&old.project.store, 0).expect("sweep");

    for (node, (file, bytes)) in old
        .project
        .chain()
        .iter()
        .skip(1)
        .zip(files.iter().zip(&before))
    {
        tools::rederive::materialize(&old.project.store, node.id).expect("materialize");
        assert_eq!(&std::fs::read(file).expect("back"), bytes);
    }
}

/// A new edit on top of older history: the head is named the new way, the
/// files below it the older way, and both still verify and rebuild.
#[test]
fn older_history_under_a_new_edit_still_sweeps_and_rebuilds() {
    let mut old = Old::new(false);
    let older = old.edit_files();
    let before = bytes_of(&older);
    old.project.silence(0.8);
    let head_file = old.project.file_of(old.project.head());
    let head_clip = old.project.node(old.project.head()).state.tracks[0].clips[0].clone();
    assert_ne!(
        head_file.file_stem().unwrap().to_string_lossy(),
        legacy_stem(&head_clip.content_hash.expect("recorded")),
        "named the new way, not as the older files are"
    );

    let report = tools::reclaim::sweep(&old.project.store, 0).expect("sweep");
    assert_eq!(report.kept_unverified, 0, "{report:?}");
    assert_eq!(report.removed_files, EDITS.len(), "{report:?}");
    assert!(head_file.is_file());

    let chain = old.project.chain();
    let target = chain[chain.len() - 2].id;
    tools::rederive::materialize(&old.project.store, target).expect("materialize");
    assert_eq!(
        &std::fs::read(&older[EDITS.len() - 1]).unwrap(),
        &before[EDITS.len() - 1]
    );
}

/// A name no replay writes under either scheme is a mismatch, as it was:
/// the sweep keeps the file, and a rebuild is refused rather than made up.
#[test]
fn a_name_the_replay_does_not_produce_is_still_a_mismatch() {
    let old = Old::new(true);
    let files = old.edit_files();

    let report = tools::reclaim::sweep(&old.project.store, 0).expect("sweep");
    assert_eq!(report.kept_unverified, 1, "{report:?}");
    assert!(files[0].is_file(), "kept: nothing proves it can come back");

    std::fs::remove_file(&files[0]).unwrap();
    let chain = old.project.chain();
    let err = tools::rederive::materialize(&old.project.store, chain[1].id)
        .expect_err("cannot be rebuilt");
    let name = files[0].file_name().unwrap().to_string_lossy().to_string();
    assert!(err.contains(&name), "names the file: {err}");
    assert!(
        !files[0].exists(),
        "and writes nothing under a name it did not make"
    );
}
