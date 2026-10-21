//! Making a copied or moved project play its own audio.
//!
//! Clip paths are absolute. That is deliberate — a clip can point at a
//! file anywhere, and the node id is the hash of the state including
//! those paths, so an edit's identity is the whole of what it says.
//!
//! It also means a project folder that is *copied* holds every byte it
//! needs and still points at the folder it came from. Save As (#156)
//! copies the whole folder, `.audiograph/derived/` and `clipboard/` and
//! `generated/` with it, precisely so the copy is self-contained, and
//! without this module it would not be: delete the original and the copy
//! goes silent while its own audio sits beside it, unreferenced.
//!
//! ## The folders, and why rebinding by file name is sound
//!
//! Three folders hold audio the project wrote under a name that is a hash
//! of the audio: `.audiograph/derived/` (destructive edits, and files
//! `load` copies in), `.audiograph/clipboard/` (the blobs a paste
//! depends on) and `generated/` at the project's root (`generate_tone`,
//! `generate_noise`, `mix_to_new_track`). Since #620 every writer but one
//! names a file by `tools::provenance::audio_hash`, the blake3 of the
//! samples *with their rate and channel count*; the exception, a
//! mixed-down track, is named by the blake3 of its WAV file, header
//! included, which holds the layout too. So a file of the same name holds
//! the same audio at the same rate and in the same number of channels, in
//! whichever of the folders it is, and whichever writer wrote it: an edit
//! whose output is the same audio as a `load` copy in `derived/` names that
//! very file, and one that equals a clipboard blob or a generated tone
//! names a file of the same name in another folder. (A file the project
//! writes as a WAV holds its samples as 16 bits, and the name is made from
//! them before that; two files of one name are the same audio to that.)
//!
//! That is the rule for what is written now. Before #620 an edit's output
//! and a generated tone or noise were named by the blake3 of their samples
//! alone, so two buffers of a different rate or channel count could share a
//! name, and the second was handed the first's file. Nothing is renamed, so
//! a project already on disk still has such files, and for those the name
//! does not say at what rate or in how many channels. This module never
//! looks inside a file and cannot tell them apart. For a copy that does not
//! matter, since its file of the name is a byte copy of the one named; and
//! the replay that rebuilds one (`tools::rederive`, the history sweep)
//! recognises the older name as well as the new one, and writes the file
//! under the name the node records. The caches beside them (`previews/`,
//! `auditions/`) are named by node or by request, not by content, and
//! nothing else in the tree is named by content, which is why nothing else
//! is rebound.
//!
//! ## The rules
//!
//! A read rebinds a clip only to a file of this project's that exists:
//!
//! 1. **A file in one of those folders of another project** is this
//!    project's file of the same name in the same folder, when this project
//!    has it. (`generated/` counts as a project's when the folder beside it
//!    is a store: `<other>/.audiograph/`.)
//! 2. **Any other path that does not exist** is this project's file of the
//!    same name in `derived/`, `clipboard/` or `generated/`, if it has one.
//!
//! Everything else is left as written: a file of another project that this
//! project does not hold, whether that file is there or not; every path
//! into this project's own folders however it is spelt; and every file of
//! the user's, wherever it is and whatever it is called. A user's file is
//! never rebound while it exists, so it keeps pointing where it is.
//!
//! A clip is never pointed at a file that is not there. An edit writes the
//! clips it read into its node, so a missing path a read made up would be
//! saved for good: a copy edited while its original was only out of reach
//! (a drive unplugged, a share down) would never reach it again. A file of
//! another project's `derived/` that is gone is rebuilt into this
//! project's `derived/` by name (`tools::rederive`), and rule 1 finds it
//! there from then on.
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
//! there. Opening such a copy copies those files back into it
//! (`tools::repair`, from the desktop app's open).
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
//! Reading costs a path comparison per clip for a project's own files. A
//! file of another project's costs a look at whether this project has it,
//! and once per folder for the life of the [`crate::Store`], the check of
//! whether that folder is this project's own under another spelling.
//!
//! ## Why it is invisible in a healthy project
//!
//! A project's own files, and the user's, are never touched, so in a
//! project that has not been copied or moved this does nothing at all and
//! `NodeId::from_state` still reproduces the id it was stored under. That
//! holds when the project is opened by another spelling of its folder (a
//! link, `..`): a path into its own folders is recognised by the folder it
//! resolves to, not by how it is written.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::state::SessionState;
use crate::store::STORE_DIR;

/// Directory under `.audiograph/` holding derived audio.
pub const DERIVED_DIR: &str = "derived";

/// Directory under `.audiograph/` where clipboard blobs are kept.
pub const CLIPBOARD_DIR: &str = "clipboard";

/// Directory at a project's root, beside `.audiograph/`, where
/// `generate_tone`, `generate_noise` and `mix_to_new_track` write.
pub const GENERATED_DIR: &str = "generated";

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

/// Where generated audio lives for a project: `<project>/generated/`.
pub fn generated_dir(project_dir: &Path) -> PathBuf {
    project_dir.join(GENERATED_DIR)
}

/// Point each clip that names another project's audio, or a missing file,
/// at this project's own file of the same name, by the rules in the module
/// docs. Returns how many clips were rebound.
///
/// A [`crate::Store`] keeps a [`Rebinder`] for its project, which
/// remembers what it has worked out about each folder; this makes a new one.
pub fn rebind(state: &mut SessionState, project_dir: &Path) -> usize {
    Rebinder::new(project_dir).rebind(state)
}

/// Where this project keeps the file `path` names, if `path` is a file in
/// a content-named folder of *another* project, whether or not either
/// file exists. See [`Rebinder::counterpart`].
pub fn counterpart(project_dir: &Path, path: &Path) -> Option<PathBuf> {
    Rebinder::new(project_dir).counterpart(path)
}

/// One of the folders whose files are named by their content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kept {
    Derived,
    Clipboard,
    Generated,
}

impl Kept {
    const ALL: [Kept; 3] = [Kept::Derived, Kept::Clipboard, Kept::Generated];

    fn dir(self, project_dir: &Path) -> PathBuf {
        match self {
            Kept::Derived => derived_dir(project_dir),
            Kept::Clipboard => clipboard_dir(project_dir),
            Kept::Generated => generated_dir(project_dir),
        }
    }
}

/// Which content-named folder `path` is a file directly in, and the
/// project that folder belongs to, by how the path is written. Whether a
/// `generated/` is a project's at all is left to the caller to ask.
fn kept_folder(path: &Path) -> Option<(Kept, &Path)> {
    let folder = path.parent()?;
    let name = folder.file_name()?;
    if name == OsStr::new(GENERATED_DIR) {
        return Some((Kept::Generated, folder.parent()?));
    }
    let kept = if name == OsStr::new(DERIVED_DIR) {
        Kept::Derived
    } else if name == OsStr::new(CLIPBOARD_DIR) {
        Kept::Clipboard
    } else {
        return None;
    };
    let store = folder.parent()?;
    if store.file_name()? != OsStr::new(STORE_DIR) {
        return None;
    }
    Some((kept, store.parent()?))
}

/// Where a path a clip names is, for the rules.
enum Place {
    /// In one of this project's own content-named folders.
    Own,
    /// In one of another project's: where this project keeps that name.
    Theirs(PathBuf),
    /// Anywhere else.
    Elsewhere,
}

/// The rules in the module docs, for one project, remembering for each
/// folder a clip names whether it is this project's own under another
/// spelling. A [`crate::Store`] keeps one for as long as it is open, and
/// its staged handles share it.
#[derive(Debug)]
pub struct Rebinder {
    project_dir: PathBuf,
    /// For each content-named folder a clip has named, whether it is this
    /// project's own folder of that kind, resolved. Only answers made while
    /// this project's folder resolves are kept: one made before it existed
    /// would not hold once it does.
    own: Mutex<HashMap<PathBuf, bool>>,
}

impl Rebinder {
    pub fn new(project_dir: &Path) -> Self {
        Self {
            project_dir: project_dir.to_path_buf(),
            own: Mutex::new(HashMap::new()),
        }
    }

    /// Rebind every clip of `state`; how many were rebound.
    pub fn rebind(&self, state: &mut SessionState) -> usize {
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

    /// Where this project keeps the file `path` names, if `path` is a file
    /// in another project's `.audiograph/derived/` or `clipboard/`, or its
    /// `generated/`: the same name in this project's folder of that kind.
    /// `None` for a path into this project's own folders (however spelt),
    /// for a `generated/` with no store beside it (not a project's), and
    /// for anything else.
    ///
    /// Neither file need exist: the rebuild of a gone file asks this for
    /// where to put it, and the repair of a copy for where to copy to.
    pub fn counterpart(&self, path: &Path) -> Option<PathBuf> {
        match self.place(path) {
            Place::Theirs(local) => Some(local),
            Place::Own | Place::Elsewhere => None,
        }
    }

    /// Where `path` is, for the rules.
    fn place(&self, path: &Path) -> Place {
        let Some(name) = path.file_name() else {
            return Place::Elsewhere;
        };
        let Some((kept, project)) = kept_folder(path) else {
            return Place::Elsewhere;
        };
        let local = kept.dir(&self.project_dir).join(name);
        // The usual case, this project's own file as written, costs a
        // comparison and no file system call.
        if local == path || path.parent().is_some_and(|f| self.is_own(f, kept)) {
            return Place::Own;
        }
        if kept == Kept::Generated && !project.join(STORE_DIR).is_dir() {
            return Place::Elsewhere;
        }
        Place::Theirs(local)
    }

    /// Where a clip naming `path` should read instead, if anywhere: always
    /// a file that exists.
    fn local_for(&self, path: &Path) -> Option<PathBuf> {
        match self.place(path) {
            // This project's own file, as written, whether it is there or
            // not: a gone one is rebuilt where it is named.
            Place::Own => None,
            // Rule 1.
            Place::Theirs(local) if local.is_file() => Some(local),
            // Rule 2.
            Place::Theirs(_) | Place::Elsewhere => {
                if path.exists() {
                    return None;
                }
                self.by_name(path.file_name()?)
            }
        }
    }

    /// This project's file called `name`, in `derived/`, `clipboard/` or
    /// `generated/`.
    fn by_name(&self, name: &OsStr) -> Option<PathBuf> {
        Kept::ALL
            .into_iter()
            .map(|kept| kept.dir(&self.project_dir).join(name))
            .find(|p| p.is_file())
    }

    /// Whether `folder`, a content-named folder of kind `kept`, is this
    /// project's own: the same folder, resolved. A folder that cannot be
    /// resolved (gone) is another project's.
    fn is_own(&self, folder: &Path, kept: Kept) -> bool {
        let lock = || self.own.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(&own) = lock().get(folder) {
            return own;
        }
        let Ok(here) = std::fs::canonicalize(kept.dir(&self.project_dir)) else {
            return false;
        };
        let own = std::fs::canonicalize(folder).is_ok_and(|there| there == here);
        lock().insert(folder.to_path_buf(), own);
        own
    }
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
    fn original_and_copy(
        dir_of: impl Fn(&Path) -> PathBuf,
        name: &str,
    ) -> (TempDir, PathBuf, PathBuf) {
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
    /// clip is left naming the original's file: a read rebinds only to a
    /// file that is here. Pointing it at a file that does not exist would be
    /// saved into every node an edit writes from it, and a folder that is
    /// only out of reach (a drive unplugged, a share down) would then never
    /// be reached again (see `tests/copied_project.rs`). Putting the file
    /// back here is the rebuild's job (`tools::rederive`), and a read finds
    /// it once it is back.
    #[test]
    fn a_gone_file_of_another_project_stays_as_written() {
        let (_tmp, original, copy) = original_and_copy(derived_dir, "abc.wav");
        std::fs::remove_dir_all(&original).unwrap();
        std::fs::remove_file(derived_dir(&copy).join("abc.wav")).unwrap();
        let mut state = state_with(&[derived_dir(&original).join("abc.wav")]);

        assert_eq!(rebind(&mut state, &copy), 0);
        assert_eq!(
            state.tracks[0].clips[0].source_path,
            derived_dir(&original).join("abc.wav"),
        );
    }

    /// `<project>/generated/`, where `generate_tone`, `generate_noise` and
    /// `mix_to_new_track` write, outside the store.
    fn generated(project: &Path) -> PathBuf {
        project.join("generated")
    }

    /// A project, by the folder that makes it one.
    fn make_project(project: &Path) {
        std::fs::create_dir_all(project.join(STORE_DIR)).unwrap();
    }

    /// Generated audio is named by its content too, and Save As copies the
    /// folder with the rest of the project, so a copy plays its own.
    #[test]
    fn a_copy_plays_its_own_generated_file_while_the_original_is_still_there() {
        let (_tmp, original, copy) = original_and_copy(generated, "abc.wav");
        make_project(&original);
        make_project(&copy);
        let mut state = state_with(&[generated(&original).join("abc.wav")]);

        assert_eq!(rebind(&mut state, &copy), 1);
        assert_eq!(
            state.tracks[0].clips[0].source_path,
            generated(&copy).join("abc.wav"),
        );
    }

    /// And once the original is gone, a missing path finds the copy's
    /// generated file by name, as it finds a derived file or a blob.
    #[test]
    fn a_moved_project_finds_its_own_generated_file() {
        let (_tmp, original, copy) = original_and_copy(generated, "abc.wav");
        make_project(&copy);
        std::fs::remove_dir_all(&original).unwrap();
        let mut state = state_with(&[generated(&original).join("abc.wav")]);

        assert_eq!(rebind(&mut state, &copy), 1);
        assert_eq!(
            state.tracks[0].clips[0].source_path,
            generated(&copy).join("abc.wav"),
        );
    }

    /// A folder called `generated` in a folder that is not a project is the
    /// user's, and its files keep pointing where they are.
    #[test]
    fn a_user_folder_named_generated_is_not_a_project() {
        let (_tmp, music, copy) = original_and_copy(generated, "take.wav");
        make_project(&copy);
        let mut state = state_with(&[generated(&music).join("take.wav")]);

        assert_eq!(rebind(&mut state, &copy), 0);
        assert_eq!(
            state.tracks[0].clips[0].source_path,
            generated(&music).join("take.wav"),
        );
    }

    /// Only the folders whose files are named by their content are
    /// rebound. The caches under `.audiograph/` are named by node or by
    /// request, so a file of the same name in this project's cache is not
    /// the same audio, whether the named file is there or gone.
    #[test]
    fn the_caches_are_not_rebound() {
        for cache in ["previews", "auditions"] {
            let in_cache = |p: &Path| p.join(STORE_DIR).join(cache);
            let (_tmp, original, copy) = original_and_copy(in_cache, "abc.wav");
            let named = in_cache(&original).join("abc.wav");
            // And the same name in each of the copy's own content folders,
            // so that taking the cache for any of them would show.
            let own = [derived_dir(&copy), clipboard_dir(&copy), generated(&copy)];
            for dir in &own {
                std::fs::create_dir_all(dir).unwrap();
                std::fs::write(dir.join("abc.wav"), b"audio").unwrap();
            }

            let mut state = state_with(&[named.clone()]);
            assert_eq!(rebind(&mut state, &copy), 0, "{cache}");
            assert_eq!(state.tracks[0].clips[0].source_path, named, "{cache}");

            // Gone, a path is looked for by name in those folders (rule 2),
            // so they are emptied first; the copy's own cache still has it.
            for dir in &own {
                std::fs::remove_file(dir.join("abc.wav")).unwrap();
            }
            std::fs::remove_dir_all(&original).unwrap();
            let mut state = state_with(&[named.clone()]);
            assert_eq!(rebind(&mut state, &copy), 0, "{cache}, gone");
            assert_eq!(state.tracks[0].clips[0].source_path, named, "{cache}, gone");
        }
    }

    /// A copy opened through a link, whose nodes name both its own files by
    /// its real folder and the original's. Each folder is judged by where it
    /// resolves, not by its name: the copy's own keep their spelling, and
    /// the original's are rebound, in whichever order the clips come.
    #[cfg(unix)]
    #[test]
    fn a_copy_opened_through_a_link_tells_its_own_folder_from_the_originals() {
        let (tmp, original, copy) = original_and_copy(derived_dir, "abc.wav");
        std::fs::write(derived_dir(&copy).join("own.wav"), b"audio").unwrap();
        let link = tmp.path().join("link");
        std::os::unix::fs::symlink(&copy, &link).unwrap();
        let own = derived_dir(&copy).join("own.wav");
        let theirs = derived_dir(&original).join("abc.wav");

        for clips in [[own.clone(), theirs.clone()], [theirs.clone(), own.clone()]] {
            let mut state = state_with(&clips);
            assert_eq!(rebind(&mut state, &link), 1, "{clips:?}");
            let read: Vec<_> = state.tracks[0]
                .clips
                .iter()
                .map(|c| c.source_path.clone())
                .collect();
            let expect: Vec<_> = clips
                .iter()
                .map(|p| {
                    if *p == theirs {
                        derived_dir(&link).join("abc.wav")
                    } else {
                        own.clone()
                    }
                })
                .collect();
            assert_eq!(read, expect);
        }
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
