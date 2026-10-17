//! A project folder copied whole, with the folder it came from still on
//! disk (#612).
//!
//! Save As copies the folder and opens the copy; so does a user copying it
//! by hand before deleting the original, a common way to move a project.
//! The copy's node files are the original's, byte for byte, so their clips
//! name the original's absolute paths. These tests read the copy through
//! its own `Store`, as every reader of the copy does: the window, the
//! agent's tools, the renders and the sweeps.

use std::path::{Path, PathBuf};

use chrono::Utc;
use session::relocate::{clipboard_dir, derived_dir};
use session::{BusGraph, Clip, NodeId, SessionNode, SessionState, Store, TempoMap, Track, TrackId};
use tempfile::TempDir;
use uuid::Uuid;

fn clip(path: &Path) -> Clip {
    Clip {
        source_path: path.to_path_buf(),
        start_in_track: 0,
        source_offset: 0,
        length: 1_000,
        content_hash: None,
        time_stretch_factor: None,
        pitch_shift_semitones: None,
        beat_grid: None,
        volume_envelope: Vec::new(),
    }
}

fn state(paths: &[&Path]) -> SessionState {
    SessionState {
        tracks: vec![Track {
            id: TrackId(Uuid::from_u128(1)),
            name: "Vox".into(),
            clips: paths.iter().map(|p| clip(p)).collect(),
            gain_db: 0.0,
            pan: 0.0,
            muted: false,
            soloed: false,
            effects: Vec::new(),
            sends: Vec::new(),
        }],
        bus_routing: BusGraph::default(),
        master_chain: Vec::new(),
        tempo_map: TempoMap::default(),
        key_map: None,
        transcript: None,
        sample_rate: 48_000,
        length_samples: 1_000,
        annotations: Vec::new(),
        sync_lock: false,
    }
}

fn append(store: &mut Store, state: SessionState) -> NodeId {
    store
        .append(SessionNode {
            id: NodeId([0; 32]),
            parent: None,
            created_at: Utc::now(),
            label: None,
            reasoning: None,
            state,
            op: None,
        })
        .expect("append")
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

/// An original with two edits in its history: the first names an edit's
/// output and a pasted blob, the second another output and the user's own
/// take, which lives outside every project. Then a copy of the folder.
struct Copied {
    _tmp: TempDir,
    original: PathBuf,
    copy: PathBuf,
    take: PathBuf,
    first: NodeId,
    second: NodeId,
}

impl Copied {
    fn new() -> Self {
        let tmp = TempDir::new().unwrap();
        let original = tmp.path().join("original");
        let copy = tmp.path().join("copy");
        let media = tmp.path().join("media");
        std::fs::create_dir_all(derived_dir(&original)).unwrap();
        std::fs::create_dir_all(clipboard_dir(&original)).unwrap();
        std::fs::create_dir_all(&media).unwrap();
        let take = media.join("take.wav");
        std::fs::write(&take, b"the user's take").unwrap();
        let edit = derived_dir(&original).join("1111.wav");
        let blob = clipboard_dir(&original).join("2222.wav");
        let later = derived_dir(&original).join("3333.wav");
        for file in [&edit, &blob, &later] {
            std::fs::write(file, b"audio").unwrap();
        }

        let mut store = Store::open(&original).unwrap();
        let first = append(&mut store, state(&[&edit, &blob]));
        let second = append(&mut store, state(&[&later, &take]));
        drop(store);
        copy_folder(&original, &copy);

        Self {
            _tmp: tmp,
            original,
            copy,
            take,
            first,
            second,
        }
    }
}

/// Where every clip of every node points, read through `project`'s store.
fn named(project: &Path) -> Vec<PathBuf> {
    let store = Store::open(project).unwrap();
    let mut out: Vec<PathBuf> = store
        .list_nodes()
        .unwrap()
        .into_iter()
        .flat_map(|n| n.state.tracks)
        .flat_map(|t| t.clips)
        .map(|c| c.source_path)
        .collect();
    out.sort();
    out
}

/// `copy` names its own files and the user's take, and nothing else.
fn assert_self_contained(copy: &Path, take: &Path, named: &[PathBuf]) {
    for path in named {
        let own = path.starts_with(derived_dir(copy)) || path.starts_with(clipboard_dir(copy));
        assert!(
            own || path == take,
            "the copy names a file that is not its own: {}",
            path.display()
        );
        assert!(path.is_file(), "{} is not there", path.display());
    }
}

#[test]
fn a_copy_names_its_own_files_while_the_original_is_still_there() {
    let c = Copied::new();
    let paths = named(&c.copy);
    assert_eq!(paths.len(), 4);
    assert_self_contained(&c.copy, &c.take, &paths);

    // A node read on its own reads the same as in a listing.
    let store = Store::open(&c.copy).unwrap();
    let head = store.get(store.head().unwrap()).unwrap();
    assert_eq!(
        head.state.tracks[0].clips[0].source_path,
        derived_dir(&c.copy).join("3333.wav")
    );
    assert_eq!(head.id, c.second, "a node keeps the id it was stored under");
}

#[test]
fn the_original_still_names_its_own_files() {
    let c = Copied::new();
    for path in named(&c.original) {
        assert!(
            path.starts_with(&c.original) || path == c.take,
            "{}",
            path.display()
        );
    }
}

#[test]
fn a_copy_names_its_own_files_once_the_original_is_gone() {
    let c = Copied::new();
    std::fs::remove_dir_all(&c.original).unwrap();
    assert_self_contained(&c.copy, &c.take, &named(&c.copy));
}

/// Node ids hash the state, and the copy reads its nodes with its own
/// paths in them, so appending a copied node's state again is a different
/// id from the node's. Moving back to an earlier node must still land on
/// that node, as it does in the original, and not add a duplicate of it.
#[test]
fn revert_to_in_a_copy_lands_on_the_node() {
    let c = Copied::new();
    let mut store = Store::open(&c.copy).unwrap();
    let before = store.list_nodes().unwrap().len();

    assert_eq!(store.revert_to(c.first).unwrap(), c.first);
    assert_eq!(store.head(), Some(c.first));
    assert_eq!(store.list_nodes().unwrap().len(), before, "no node added");
}

/// The same once the original is gone, which is the project moved: its
/// paths were already rebound then, before the copy rule.
#[test]
fn revert_to_in_a_moved_project_lands_on_the_node() {
    let c = Copied::new();
    std::fs::remove_dir_all(&c.original).unwrap();
    let mut store = Store::open(&c.copy).unwrap();
    let before = store.list_nodes().unwrap().len();

    assert_eq!(store.revert_to(c.first).unwrap(), c.first);
    assert_eq!(store.head(), Some(c.first));
    assert_eq!(store.list_nodes().unwrap().len(), before, "no node added");
}

/// An edit in the copy reads its parent with the copy's paths, so what it
/// writes names the copy's files; and a copy of the copy names its own in
/// turn, whichever project a node was first written in.
#[test]
fn a_copy_of_a_copy_names_its_own_files() {
    let c = Copied::new();
    let mut store = Store::open(&c.copy).unwrap();
    let head = store.get(store.head().unwrap()).unwrap();
    let mut edited = head.state.clone();
    edited.tracks[0].clips[0].start_in_track = 500;
    append(&mut store, edited);
    drop(store);

    let again = c.copy.with_file_name("copy-of-copy");
    copy_folder(&c.copy, &again);
    let paths = named(&again);
    assert_eq!(paths.len(), 6);
    assert_self_contained(&again, &c.take, &paths);

    std::fs::remove_dir_all(&c.original).unwrap();
    std::fs::remove_dir_all(&c.copy).unwrap();
    assert_eq!(named(&again), paths, "and the same with both gone");
}

/// A copy whose own file of a name is missing (a copy saved by 0.6.0,
/// whose open swept its `derived/`), edited while the original is out of
/// reach: its drive unplugged, its share down. An edit writes the head's
/// clips as it read them, so a read that pointed them at the copy's missing
/// file would save that path into the new node, and when the drive came
/// back the edit would still not reach the original's file. Read as
/// written, the edit names the original's file, and plays again when it is
/// back.
#[test]
fn an_edit_made_while_the_original_is_offline_finds_it_again() {
    let c = Copied::new();
    std::fs::remove_file(derived_dir(&c.copy).join("3333.wav")).unwrap();
    let offline = c.original.with_file_name("unplugged");
    std::fs::rename(&c.original, &offline).unwrap();

    let mut store = Store::open(&c.copy).unwrap();
    let mut edited = store.get(c.second).unwrap().state;
    edited.tracks[0].gain_db = -3.0;
    let edit = append(&mut store, edited);
    drop(store);

    std::fs::rename(&offline, &c.original).unwrap();
    let store = Store::open(&c.copy).unwrap();
    let path = store.get(edit).unwrap().state.tracks[0].clips[0]
        .source_path
        .clone();
    assert_eq!(path, derived_dir(&c.original).join("3333.wav"));
    assert!(
        path.is_file(),
        "the edit lost its audio: {}",
        path.display()
    );
}

/// A staged handle may not move the shared head, so it may not revert
/// either: it is refused, and the handle is where it was.
#[test]
fn revert_to_on_a_staged_handle_is_refused() {
    let c = Copied::new();
    let store = Store::open(&c.original).unwrap();
    let mut staged = store.stage().unwrap();

    let err = staged.revert_to(c.first).unwrap_err();
    assert!(matches!(err, session::Error::Staged(_)), "{err:?}");
    assert_eq!(staged.head(), Some(c.second));
    drop(staged);
    assert_eq!(Store::open(&c.original).unwrap().head(), Some(c.second));
}
