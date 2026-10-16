//! Making a copied or moved project play its own audio.
//!
//! Clip paths are absolute. That is deliberate — a clip can point at a
//! file anywhere, and the node id is the hash of the state including
//! those paths, so an edit's identity is the whole of what it says.
//!
//! It also means a project folder that is *copied* holds every byte it
//! needs and still points at the folder it came from. Save As (#156)
//! copies `.audiograph/derived/` and `clipboard/` precisely so the copy
//! is self-contained, and without this module it would not be: delete
//! the original and the copy goes silent while its own audio sits
//! beside it, unreferenced.
//!
//! ## Why rebinding by file name is sound
//!
//! Derived files and clipboard blobs are named by content — the file
//! name *is* the blake3 of the samples (`tools::provenance::audio_hash`).
//! So finding `9f3c….wav` in this project's `derived/` is not a guess
//! that some nearby file might do; it is finding the same bytes the
//! other path named. Nothing else in the tree is content-addressed,
//! which is why nothing else is rebound.
//!
//! ## The rules
//!
//! 1. **A file in another project's store** (`<other>/.audiograph/derived/`
//!    or `clipboard/`) is this project's file of the same name, in the
//!    same folder of its own store, when this project has that file. It
//!    is also this project's when the named file is gone, even if this
//!    project has lost it too: that is where a rebuild puts it
//!    (`tools::rederive` rebuilds only into the project's own
//!    `derived/`), and where a read reports it missing.
//! 2. **Any other path that does not exist** is this project's file of the
//!    same name in `derived/` or `clipboard/`, if it has one.
//!
//! Everything else is left as written: a file of another project's store
//! that this project does not hold (a WAV loaded from that folder: there is
//! nothing here to play instead), every path into this project's own
//! store however it is spelt, and every file of the user's, wherever it
//! is and whatever it is called. A user's file is never rebound while it
//! exists, so it keeps pointing where it is.
//!
//! ## Why a copy is rebound while the original is still there (#612)
//!
//! Rule 2 alone fires only once the original has gone. Until then the
//! copy's nodes name the original's files, and they exist. Save As opens
//! the copy straight away, and every open runs the orphan sweep, which
//! deletes each file in `derived/` that no node names; the copy's nodes
//! named the original's files, so the copy's own were named by nothing
//! and went. The copy then played from the original's folder, and
//! deleting the original (the second half of moving a project, by Save As
//! or by copying the folder by hand) cost it every edit no replay can
//! redo. Rule 1 makes a copy read its own files from the moment it
//! exists, and the sweeps, which count what the store reads back, keep
//! exactly what the copy plays. It is one rule, applied wherever a node is
//! read, so what is played and what is kept cannot drift apart.
//!
//! ## Why it happens on read rather than on copy
//!
//! Rewriting the paths in the copied node files would change every node
//! id, and the ids are the file names, the parent pointers and the head.
//! A copy would arrive with a broken DAG. Rebinding on the way out of
//! the store leaves what is on disk exactly as written. It also covers
//! what no copy routine sees: a folder copied by hand, and a project moved.
//!
//! What it cannot do is put back audio a copy has already lost. A copy
//! opened by a build without rule 1 had its `derived/` swept then; its
//! clips name the original's files, which it plays while the original is
//! there (the original's file exists and this project has none of that
//! name), and nothing here copies them into it.
//!
//! ## What it costs
//!
//! A rebound node reads with paths its id was not hashed from, so
//! appending its state again is a new node with another id, not the node
//! itself. `revert_to` therefore moves the head to the node it names
//! rather than appending its state. An edit that changes nothing, made
//! on a copied node, still adds a node, as it already did in a moved
//! project; once an edit has been made in the copy, its nodes name the
//! copy's files and this no longer arises.
//!
//! ## Why it is invisible in a healthy project
//!
//! A project's own files, and the user's, are never touched, so in a
//! project that has not been copied or moved this does nothing at all and
//! `NodeId::from_state` still reproduces the id it was stored under. That
//! holds when the project is opened by another spelling of its folder (a
//! link, `..`): a path into its own store is recognised by the folder it
//! resolves to, not by how it is written.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use crate::state::SessionState;
use crate::store::STORE_DIR;

/// Directory under `.audiograph/` holding derived audio.
pub const DERIVED_DIR: &str = "derived";

/// Directory under `.audiograph/` where clipboard blobs are kept.
pub const CLIPBOARD_DIR: &str = "clipboard";

/// Where derived audio lives: `<project>/.audiograph/derived/`.
///
/// Inside the project rather than beside the source the user opened, so
/// that a project folder is something you can copy, move or back up
/// (#190).
pub fn derived_dir(project_dir: &Path) -> PathBuf {
    project_dir.join(STORE_DIR).join(DERIVED_DIR)
}

/// Where clipboard blobs live for a project.
pub fn clipboard_dir(project_dir: &Path) -> PathBuf {
    project_dir.join(STORE_DIR).join(CLIPBOARD_DIR)
}

/// Point each clip that names another project's store, or a missing file,
/// at this project's own file of the same name, by the rules in the module
/// docs. Returns how many clips were rebound.
pub fn rebind(state: &mut SessionState, project_dir: &Path) -> usize {
    Rebinder::new(project_dir).rebind(state)
}

/// [`rebind`] for many nodes of one project, asking once per folder
/// whether a folder a clip names is this project's own.
pub(crate) struct Rebinder<'a> {
    project_dir: &'a Path,
    /// For each store folder a clip has named, whether it is this
    /// project's own under another spelling.
    own: HashMap<PathBuf, bool>,
}

impl<'a> Rebinder<'a> {
    pub(crate) fn new(project_dir: &'a Path) -> Self {
        Self {
            project_dir,
            own: HashMap::new(),
        }
    }

    pub(crate) fn rebind(&mut self, state: &mut SessionState) -> usize {
        let mut rebound = 0;
        for track in &mut state.tracks {
            for clip in &mut track.clips {
                if let Some(local) = self.local_for(&clip.source_path) {
                    clip.source_path = local;
                    rebound += 1;
                }
            }
        }
        rebound
    }

    /// Where a clip naming `path` should read instead, if anywhere.
    fn local_for(&mut self, path: &Path) -> Option<PathBuf> {
        let name = path.file_name()?;
        if let Some(store_dir) = store_folder(path) {
            // Rule 1. The usual case, this project's own file as written,
            // costs a comparison and no file system call.
            let local = self.project_dir.join(STORE_DIR).join(store_dir).join(name);
            if local == path || self.is_own(path.parent()?, store_dir) {
                return None;
            }
            if local.is_file() {
                return Some(local);
            }
            if path.exists() {
                return None;
            }
            return Some(self.by_name(name).unwrap_or(local));
        }
        // Rule 2.
        if path.exists() {
            return None;
        }
        self.by_name(name)
    }

    /// This project's file called `name`, in `derived/` or `clipboard/`.
    fn by_name(&self, name: &OsStr) -> Option<PathBuf> {
        [
            derived_dir(self.project_dir),
            clipboard_dir(self.project_dir),
        ]
        .into_iter()
        .map(|dir| dir.join(name))
        .find(|p| p.is_file())
    }

    /// Whether `folder`, a store folder called `store_dir`, is this
    /// project's own: the same folder, resolved. A folder that cannot be
    /// resolved (gone) is another project's.
    fn is_own(&mut self, folder: &Path, store_dir: &str) -> bool {
        if let Some(&own) = self.own.get(folder) {
            return own;
        }
        let local = self.project_dir.join(STORE_DIR).join(store_dir);
        let own = match (std::fs::canonicalize(folder), std::fs::canonicalize(local)) {
            (Ok(a), Ok(b)) => a == b,
            _ => false,
        };
        self.own.insert(folder.to_path_buf(), own);
        own
    }
}

/// `derived` or `clipboard`, if `path` is a file directly in that folder
/// of some project's store (`<project>/.audiograph/<folder>/<file>`).
fn store_folder(path: &Path) -> Option<&'static str> {
    let folder = path.parent()?;
    let store = folder.parent()?;
    if store.file_name()? != OsStr::new(STORE_DIR) {
        return None;
    }
    let folder = folder.file_name()?;
    [DERIVED_DIR, CLIPBOARD_DIR]
        .into_iter()
        .find(|dir| folder == OsStr::new(dir))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{Clip, SessionState, Track, TrackId};
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn state_with(paths: &[PathBuf]) -> SessionState {
        SessionState {
            tracks: vec![Track {
                id: TrackId(uuid::Uuid::new_v4()),
                name: "Vox".into(),
                clips: paths
                    .iter()
                    .map(|p| Clip {
                        source_path: p.clone(),
                        start_in_track: 0,
                        source_offset: 0,
                        length: 100,
                        content_hash: None,
                        time_stretch_factor: None,
                        pitch_shift_semitones: None,
                        beat_grid: None,
                        volume_envelope: Vec::new(),
                    })
                    .collect(),
                gain_db: 0.0,
                pan: 0.0,
                muted: false,
                soloed: false,
                effects: Vec::new(),
                sends: Vec::new(),
            }],
            bus_routing: crate::state::BusGraph::default(),
            master_chain: Vec::new(),
            tempo_map: crate::state::TempoMap::default(),
            key_map: None,
            transcript: None,
            sample_rate: 48_000,
            length_samples: 100,
            annotations: Vec::new(),
            sync_lock: false,
        }
    }

    /// A copied project has the audio; the nodes name the original's
    /// path. Without the rebind, deleting the original makes the copy
    /// silent while its own audio sits beside it.
    #[test]
    fn a_moved_project_finds_its_own_copy_of_the_file() {
        let tmp = TempDir::new().unwrap();
        let copy = tmp.path().join("copy");
        std::fs::create_dir_all(derived_dir(&copy)).unwrap();
        std::fs::write(derived_dir(&copy).join("abc.wav"), b"audio").unwrap();

        // The path the node was written with — a folder that is gone.
        let gone = tmp.path().join("original").join(STORE_DIR).join("derived");
        let mut state = state_with(&[gone.join("abc.wav")]);

        assert_eq!(rebind(&mut state, &copy), 1);
        assert_eq!(
            state.tracks[0].clips[0].source_path,
            derived_dir(&copy).join("abc.wav"),
        );
    }

    /// The clipboard blobs a paste depends on are content-addressed too.
    #[test]
    fn clipboard_blobs_are_rebound_as_well() {
        let tmp = TempDir::new().unwrap();
        let copy = tmp.path().join("copy");
        std::fs::create_dir_all(clipboard_dir(&copy)).unwrap();
        std::fs::write(clipboard_dir(&copy).join("def.wav"), b"audio").unwrap();

        let mut state = state_with(&[PathBuf::from("/nowhere/def.wav")]);
        assert_eq!(rebind(&mut state, &copy), 1);
        assert_eq!(
            state.tracks[0].clips[0].source_path,
            clipboard_dir(&copy).join("def.wav"),
        );
    }

    /// The invariant everything else rests on: in a project that has
    /// not moved this is a no-op, so `NodeId::from_state` still
    /// reproduces the id the node was stored under.
    #[test]
    fn a_path_that_exists_is_never_touched() {
        let tmp = TempDir::new().unwrap();
        let project = tmp.path().join("p");
        std::fs::create_dir_all(derived_dir(&project)).unwrap();
        // Same file name in the project's derived dir — and the clip's
        // own path is fine, so it must win.
        std::fs::write(derived_dir(&project).join("abc.wav"), b"local").unwrap();
        let elsewhere = tmp.path().join("elsewhere");
        std::fs::create_dir_all(&elsewhere).unwrap();
        std::fs::write(elsewhere.join("abc.wav"), b"the real one").unwrap();

        let mut state = state_with(&[elsewhere.join("abc.wav")]);
        assert_eq!(rebind(&mut state, &project), 0);
        assert_eq!(
            state.tracks[0].clips[0].source_path,
            elsewhere.join("abc.wav"),
            "an existing path is the truth, even when a name matches"
        );
    }

    /// A genuinely missing file stays missing. Rebinding is only ever a
    /// lookup by content hash — there is nothing to substitute for a
    /// source the user imported and then deleted, and pretending
    /// otherwise would put the wrong audio in the timeline.
    #[test]
    fn a_missing_file_with_no_local_copy_is_left_alone() {
        let tmp = TempDir::new().unwrap();
        let project = tmp.path().join("p");
        std::fs::create_dir_all(derived_dir(&project)).unwrap();

        let mut state = state_with(&[PathBuf::from("/nowhere/gone.wav")]);
        assert_eq!(rebind(&mut state, &project), 0);
        assert_eq!(
            state.tracks[0].clips[0].source_path,
            PathBuf::from("/nowhere/gone.wav"),
        );
    }

    /// `original` and `copy`, each holding `name` in `dir_of(project)`, as
    /// Save As (or a folder copied by hand) leaves them.
    fn original_and_copy(dir_of: fn(&Path) -> PathBuf, name: &str) -> (TempDir, PathBuf, PathBuf) {
        let tmp = TempDir::new().unwrap();
        let original = tmp.path().join("original");
        let copy = tmp.path().join("copy");
        for project in [&original, &copy] {
            std::fs::create_dir_all(dir_of(project)).unwrap();
            std::fs::write(dir_of(project).join(name), b"audio").unwrap();
        }
        (tmp, original, copy)
    }

    /// The copy right after Save As (#612): its nodes name the original's
    /// files, and the original is still there. The copy holds the same
    /// bytes under the same name, so it plays its own, and what it names is
    /// what its sweeps keep.
    #[test]
    fn a_copy_plays_its_own_file_while_the_original_is_still_there() {
        let (_tmp, original, copy) = original_and_copy(derived_dir, "abc.wav");
        let mut state = state_with(&[derived_dir(&original).join("abc.wav")]);

        assert_eq!(rebind(&mut state, &copy), 1);
        assert_eq!(
            state.tracks[0].clips[0].source_path,
            derived_dir(&copy).join("abc.wav"),
        );
    }

    #[test]
    fn a_copy_plays_its_own_clipboard_blob_while_the_original_is_still_there() {
        let (_tmp, original, copy) = original_and_copy(clipboard_dir, "def.wav");
        let mut state = state_with(&[clipboard_dir(&original).join("def.wav")]);

        assert_eq!(rebind(&mut state, &copy), 1);
        assert_eq!(
            state.tracks[0].clips[0].source_path,
            clipboard_dir(&copy).join("def.wav"),
        );
    }

    /// The original, read as itself, is left exactly as written: its
    /// files are its own.
    #[test]
    fn the_original_keeps_its_own_paths() {
        let (_tmp, original, _copy) = original_and_copy(derived_dir, "abc.wav");
        let mut state = state_with(&[derived_dir(&original).join("abc.wav")]);

        assert_eq!(rebind(&mut state, &original), 0);
        assert_eq!(
            state.tracks[0].clips[0].source_path,
            derived_dir(&original).join("abc.wav"),
        );
    }

    /// A file in another project's store that this one does not hold is
    /// still the one to play: a node may name it (a WAV loaded from that
    /// folder), and there is nothing here to put in its place.
    #[test]
    fn another_projects_file_this_one_does_not_hold_is_left_where_it_is() {
        let (_tmp, original, copy) = original_and_copy(derived_dir, "abc.wav");
        std::fs::write(derived_dir(&original).join("only-there.wav"), b"audio").unwrap();
        let mut state = state_with(&[derived_dir(&original).join("only-there.wav")]);

        assert_eq!(rebind(&mut state, &copy), 0);
        assert_eq!(
            state.tracks[0].clips[0].source_path,
            derived_dir(&original).join("only-there.wav"),
        );
    }

    /// The original is gone, and so is this project's file of that name
    /// (the history sweep took it, having proved a replay rebuilds it). The
    /// clip names this project's file, so the rebuild puts it here and a
    /// read reports it missing here: a path in a folder that no longer
    /// exists is one nothing would ever rebuild.
    #[test]
    fn a_gone_file_of_another_project_is_looked_for_here() {
        let (_tmp, original, copy) = original_and_copy(derived_dir, "abc.wav");
        std::fs::remove_dir_all(&original).unwrap();
        std::fs::remove_file(derived_dir(&copy).join("abc.wav")).unwrap();
        let mut state = state_with(&[derived_dir(&original).join("abc.wav")]);

        assert_eq!(rebind(&mut state, &copy), 1);
        assert_eq!(
            state.tracks[0].clips[0].source_path,
            derived_dir(&copy).join("abc.wav"),
        );
    }

    /// Only a project's store is content-addressed. A folder of the user's
    /// that happens to be called `derived` holds their own files, which
    /// keep pointing where they are whatever this project holds.
    #[test]
    fn a_user_folder_named_derived_is_not_a_store() {
        let tmp = TempDir::new().unwrap();
        let project = tmp.path().join("p");
        std::fs::create_dir_all(derived_dir(&project)).unwrap();
        std::fs::write(derived_dir(&project).join("take.wav"), b"local").unwrap();
        let mine = tmp.path().join("music").join(DERIVED_DIR);
        std::fs::create_dir_all(&mine).unwrap();
        std::fs::write(mine.join("take.wav"), b"the user's own").unwrap();

        let mut state = state_with(&[mine.join("take.wav")]);
        assert_eq!(rebind(&mut state, &project), 0);
        assert_eq!(state.tracks[0].clips[0].source_path, mine.join("take.wav"));
    }

    /// A project opened through another spelling of its folder (here a
    /// link) reads its own files as written. Rewriting them to the
    /// spelling it was opened by would change every node's state, and
    /// with it what `NodeId::from_state` gives, for a project that has
    /// not moved at all.
    #[cfg(unix)]
    #[test]
    fn a_project_opened_through_a_link_keeps_its_paths() {
        let tmp = TempDir::new().unwrap();
        let project = tmp.path().join("p");
        std::fs::create_dir_all(derived_dir(&project)).unwrap();
        std::fs::write(derived_dir(&project).join("abc.wav"), b"audio").unwrap();
        let link = tmp.path().join("link");
        std::os::unix::fs::symlink(&project, &link).unwrap();

        let mut state = state_with(&[derived_dir(&project).join("abc.wav")]);
        assert_eq!(rebind(&mut state, &link), 0);
        assert_eq!(
            state.tracks[0].clips[0].source_path,
            derived_dir(&project).join("abc.wav"),
        );
    }
}
