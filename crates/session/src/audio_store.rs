//! The project's own audio files: where they live, what they are named, and
//! the one way they are written (#719, ADR 0008, stage 1).
//!
//! A project keeps audio under names made from the audio itself. This module
//! is the single home of the three things that make that so:
//!
//! * **The folders** ([`AudioFolder`]): `derived/` and `clipboard/` under
//!   `.audiograph/`, and `generated/` at the project's root.
//! * **The names** ([`audio_hash`], and the older [`legacy_sample_hash`] that
//!   only recognises names written before #620).
//! * **The write** ([`AudioStore::put_with`]): a file is put under its name
//!   whole or not at all, and a name already there is kept.
//!
//! Stage 1 moved the definitions here and changed nothing on disk. Stage 2
//! sends every writer through the store: names are [`AudioName`]s, which
//! only this module makes, and every file is put whole through
//! [`AudioStore::put_with`] (or, for a clipboard blob that can be checked
//! against its name, [`AudioStore::put_verified`], and for a mix-down named
//! by its bytes, [`AudioStore::put_named_by_content`]). Names and folders are
//! still what they were.
//!
//! Stage 3a sends every deleter through the store: the orphan sweep, the
//! history sweep, compaction and the removal of a killed repair's staging
//! files ask [`AudioStore::owns`] (or list a folder with
//! [`AudioStore::files`]) whether a file is the store's, and delete it only
//! through an [`AudioDeleter`], which [`Store::audio_deleter`] hands out only
//! while no handle on the project is staged. Which folder each of them works
//! in is still its own choice (the sweeps `derived/`, compaction `derived/`,
//! the leftovers `clipboard/` and `generated/`).
//!
//! Stage 3b sends the copiers and resolvers through the store:
//!
//! * **A replay's put-back** (`tools::rederive`): what a replay wrote is read
//!   as a [`Rebuilt`] ([`AudioStore::rebuilt`]), which answers to every name a
//!   node may record for a file, the one the file has and, for a node written
//!   before #620, the name its samples had then. [`Rebuilt::put_back`] puts
//!   the file into this project's `derived/` under the name the node records,
//!   through [`AudioStore::put_with`].
//! * **Repair's copies** (`tools::repair`): a clip naming another project's
//!   file is resolved to a [`Counterpart`] (`Store::counterpart`), this
//!   project's place for that name, and [`Counterpart::copy_in`] copies it
//!   there without ever replacing a file.
//! * **Save As's check** (the desktop's `project.rs`): which of the head's
//!   files the copy needs is [`AudioStore::owns`] in any folder, and where it
//!   is in the copy is [`OwnedFile::in_project`].
//! * **Which folder a path is in** ([`AudioStore::folder_of`]), for a rebuild
//!   that asks about a file that is not there.
//!
//! Stage 4 removes what a writer left behind: a put's staging file
//! (`.cas-*.wav.part`) in `clipboard/` or `generated/` whose writer was killed
//! mid-write goes once it is a day old, while nothing is staged
//! ([`AudioDeleter::remove_stale_put_leftovers`], run with the orphan sweep).
//! The store's cases are tested once, here; `tools`' `tests/audio_store/`
//! holds what only its callers (the sweeps, compaction, a rebuild, the repair)
//! can show.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::node::SessionNode;
use crate::state::Clip;
use crate::store::{Store, STORE_DIR};

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

/// One of the folders whose files are named by their content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioFolder {
    /// `<project>/.audiograph/derived/`: an edit's output and a `load` copy.
    Derived,
    /// `<project>/.audiograph/clipboard/`: clipboard blobs.
    Clipboard,
    /// `<project>/generated/`: generated tones, noise and mixed-down tracks.
    Generated,
}

impl AudioFolder {
    /// Every folder.
    pub const ALL: [AudioFolder; 3] = [
        AudioFolder::Derived,
        AudioFolder::Clipboard,
        AudioFolder::Generated,
    ];

    /// The folder's name, as written on disk.
    pub fn name(self) -> &'static str {
        match self {
            AudioFolder::Derived => DERIVED_DIR,
            AudioFolder::Clipboard => CLIPBOARD_DIR,
            AudioFolder::Generated => GENERATED_DIR,
        }
    }

    /// This folder of the project at `project_dir`.
    pub fn dir(self, project_dir: &Path) -> PathBuf {
        match self {
            AudioFolder::Derived => derived_dir(project_dir),
            AudioFolder::Clipboard => clipboard_dir(project_dir),
            AudioFolder::Generated => generated_dir(project_dir),
        }
    }
}

/// Content hash of decoded audio: rate, channels, then samples as
/// little-endian bytes.
///
/// Two files whose samples, rate and channel count all match *are* the
/// same audio for every purpose this store has, so they hash the same
/// (#160) — and two genuinely different ones cannot collide without
/// colliding blake3 itself.
///
/// **The name of every audio file the project writes from its samples**
/// (`<hash>.wav`), reached by writers as [`AudioName::of_samples`]: a writer
/// that hashed on its own could leave the rate or channel count out again
/// (#620), and `crates/tools/tests/audio_names_cover_layout.rs` fails a tool
/// or desktop source that calls blake3 itself.
pub fn audio_hash(samples: &[f32], sample_rate: u32, channels: u16) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&sample_rate.to_le_bytes());
    hasher.update(&(channels as u32).to_le_bytes());
    update_with_samples(&mut hasher, samples);
    hasher.finalize().to_hex().to_string()
}

/// Feed `samples` to `hasher` as little-endian bytes, a slice at a time so
/// that hashing a track's worth does not first copy it.
fn update_with_samples(hasher: &mut blake3::Hasher, samples: &[f32]) {
    let mut chunk = [0u8; 4 * 4096];
    for block in samples.chunks(4096) {
        for (bytes, s) in chunk.chunks_exact_mut(4).zip(block) {
            bytes.copy_from_slice(&s.to_le_bytes());
        }
        hasher.update(&chunk[..block.len() * 4]);
    }
}

/// The hash that named an edit's output and a generated tone or noise
/// before #620: blake3 of the interleaved samples, little-endian, and
/// nothing else — not the rate, not the channel count.
///
/// **Exists only to recognise names written before that change.** Files
/// already on disk and the nodes that name them keep those names (nothing
/// is renamed), and a replay of such a node now writes the new name
/// ([`audio_hash`]), so `rederive` and the history sweep ask whether this
/// would have been the recorded one. Nothing may name a new file with it:
/// two buffers of a different layout and the same samples share it, which
/// is the bug.
///
/// A writer stores it as the clip's `content_hash`, as the edits have
/// always done (the blake3 of the samples), which is how a replay finds it
/// again without recomputing anything from a file: the file holds the
/// samples as 16-bit PCM, not as they were hashed.
pub fn legacy_sample_hash(samples: &[f32]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    update_with_samples(&mut hasher, samples);
    *hasher.finalize().as_bytes()
}

/// [`legacy_sample_hash`] as the file stem it named, hex. Like it, only for
/// recognising a name written before #620.
pub fn legacy_sample_hash_name(samples: &[f32]) -> String {
    legacy_stem(&legacy_sample_hash(samples))
}

/// The file stem a [`legacy_sample_hash`] named, hex.
pub fn legacy_stem(hash: &[u8; 32]) -> String {
    blake3::Hash::from_bytes(*hash).to_hex().to_string()
}

/// A name in the project's audio store, made only here, from what it names
/// (#719 stage 2). [`AudioStore::put_with`] and [`AudioStore::put_verified`]
/// take one, so no writer can put a file under a name it made up: the rate
/// and channel count cannot be left out of an audio name again (#620), and a
/// writer cannot invent a fourth way of naming.
///
/// Reading is not restricted: a clip's file is found by the path its node
/// records, and [`AudioStore::path`] takes a plain stem for that.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AudioName(String);

impl AudioName {
    /// The name of audio made from its samples: [`audio_hash`] (rate,
    /// channel count, then the samples). A destructive edit's output, a
    /// `load` copy, a generated tone or noise, a clipboard blob.
    pub fn of_samples(samples: &[f32], sample_rate: u32, channels: u16) -> Self {
        Self(audio_hash(samples, sample_rate, channels))
    }

    /// The name of a file by its bytes, header included: a mixed-down track,
    /// whose WAV header holds the rate and channel count the samples alone
    /// would not. Private: only [`AudioStore::put_named_by_content`] names a
    /// file this way, from the file it has just written, so no writer can
    /// hand it bytes that leave the layout out.
    fn of_file_bytes(bytes: &[u8]) -> Self {
        Self(blake3::hash(bytes).to_hex().to_string())
    }

    /// The name of a lane copy: the audio a track's clips lay out on the
    /// timeline, keyed by the clips (`track-<hash>`), not their samples, so a
    /// listing finds it without decoding anything. No node names these
    /// files; they are a cache the timeline draws.
    pub fn of_lane(clips: &[Clip]) -> Self {
        let mut hasher = blake3::Hasher::new();
        for c in clips {
            hasher.update(c.source_path.to_string_lossy().as_bytes());
            hasher.update(&c.start_in_track.to_le_bytes());
            hasher.update(&c.source_offset.to_le_bytes());
            hasher.update(&c.length.to_le_bytes());
        }
        Self(format!("track-{}", hasher.finalize().to_hex()))
    }

    /// The name a node records as the file name `recorded` (`<stem>.wav`).
    /// Private: only a [`Rebuilt`] adopts a recorded name, and only once a
    /// file it wrote answers to it, so a put-back writes under the name the
    /// node reads and under no name the store did not give.
    fn of_recorded(recorded: &OsStr) -> Option<Self> {
        let stem = recorded.to_str()?.strip_suffix(".wav")?;
        (!stem.is_empty()).then(|| Self(stem.to_string()))
    }

    /// The file stem: the name without `.wav`.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The 32 bytes of a name that is one hash ([`of_samples`](Self::of_samples),
    /// [`of_file_bytes`](Self::of_file_bytes)), for a clip's `content_hash`;
    /// `None` for a lane copy's, which is not one hash.
    pub fn digest(&self) -> Option<[u8; 32]> {
        blake3::Hash::from_hex(&self.0).ok().map(|h| *h.as_bytes())
    }
}

impl std::fmt::Display for AudioName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Where a file under a store name is ([`AudioStore::path_of`]): shown in a
/// message, or read. It is deliberately not an `AsRef<Path>`, so it cannot be
/// handed to `std::fs::write`, `File::create` or a WAV writer by accident: a
/// write straight onto a trusted name could be cut short and stay there.
/// [`as_path`](Self::as_path) is for a reader that needs the path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorePath(PathBuf);

impl StorePath {
    /// The path, for a read (a listing, a lookup by path). Never write to it:
    /// put a file through [`AudioStore::put_with`].
    pub fn as_path(&self) -> &Path {
        &self.0
    }
}

impl std::fmt::Display for StorePath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.display().fmt(f)
    }
}

/// The audio files of one project.
#[derive(Debug, Clone)]
pub struct AudioStore {
    project_dir: PathBuf,
}

impl AudioStore {
    /// The audio store of the project at `project_dir`.
    pub fn new(project_dir: impl Into<PathBuf>) -> Self {
        Self {
            project_dir: project_dir.into(),
        }
    }

    /// Where the file named `stem` lives in `folder`: `<folder>/<stem>.wav`.
    /// Whether it exists is not asked. Crate-private: outside `session` a
    /// path is asked for by an [`AudioName`] ([`path_of`](Self::path_of)),
    /// so no caller can spell one for a name the store did not make.
    pub(crate) fn path(&self, folder: AudioFolder, stem: &str) -> PathBuf {
        folder.dir(&self.project_dir).join(format!("{stem}.wav"))
    }

    /// Where the file named `name` lives in `folder`, to name it in a
    /// message ([`StorePath`] is `Display`) or to read it
    /// ([`StorePath::as_path`]). Not a path to write to: a file under a store
    /// name is trusted on sight, so it is written only by the store's puts,
    /// which stage it and rename it into place.
    pub fn path_of(&self, folder: AudioFolder, name: &AudioName) -> StorePath {
        StorePath(self.path(folder, name.as_str()))
    }

    /// Put the file `fill` writes under `name` in `folder`, whole or not at
    /// all, unless a file is already there. Returns the file's path.
    ///
    /// The name is trusted on sight: an existing file under it is kept
    /// without a look (`fill` is not called), and a node names it as audio
    /// that exists. So a write cut short — a crash, a full disk, a `fill`
    /// that fails — must never be left under it, and neither may one that
    /// another writer is half-way through. `fill` is handed a temporary
    /// file in the same folder (`.cas-*.wav.part`, made if the folder is
    /// not there yet), and it is renamed into place once whole.
    ///
    /// A writer that loses a race to another writer of the same name is
    /// not an error: the name is the hash of the content, so what is there
    /// is what this would have put there, and on Windows replacing a file
    /// somebody has open fails.
    pub fn put_with(
        &self,
        folder: AudioFolder,
        name: &AudioName,
        fill: impl FnOnce(&Path) -> Result<(), String>,
    ) -> Result<PathBuf, String> {
        let path = self.path(folder, name.as_str());
        if path.exists() {
            return Ok(path);
        }
        let tmp = self.temp_in(folder)?;
        fill(&tmp)?;
        match tmp.persist(&path) {
            Ok(()) => Ok(path),
            // Another writer of the same name got there first.
            Err(_) if path.is_file() => Ok(path),
            Err(e) => Err(format!("could not put the file in place: {}", e.error)),
        }
    }

    /// [`put_with`](Self::put_with) for a folder whose files can be checked
    /// against their name: an existing file is kept only if `verify` says it
    /// holds what `name` names, and is otherwise replaced, whole, by what
    /// `fill` writes (a temporary file renamed over it).
    ///
    /// Only the clipboard can be checked this way: a blob is float and reads
    /// back to exactly the samples its name hashes, while a `derived/` file
    /// is 16-bit PCM and its name is never recomputed from it (#620). Blobs
    /// written as 16-bit before #377 are still on disk; a replay pastes what
    /// the blob holds, so one that does not read back is replaced.
    ///
    /// The replace is a rename over the old file, so the name is never
    /// missing or half written. On Windows a rename over a file another
    /// process has open fails; that is an error here (the old blob stays),
    /// where writing over it in place used to succeed.
    pub fn put_verified(
        &self,
        folder: AudioFolder,
        name: &AudioName,
        verify: impl FnOnce(&Path) -> bool,
        fill: impl FnOnce(&Path) -> Result<(), String>,
    ) -> Result<PathBuf, String> {
        let path = self.path(folder, name.as_str());
        if path.is_file() && verify(&path) {
            return Ok(path);
        }
        let tmp = self.temp_in(folder)?;
        fill(&tmp)?;
        tmp.persist(&path)
            .map_err(|e| format!("could not put the file in place: {}", e.error))?;
        Ok(path)
    }

    /// Put a file whose name is known only once it is written, named by its
    /// bytes, header included ([`AudioName::of_file_bytes`]): a mixed-down
    /// track, whose WAV header holds the rate and channel count. `fill`
    /// writes a temporary file in `folder`; it is read, named and renamed
    /// into place, unless a file is already under that name, which is kept
    /// (the name is the content's). Nothing is left under any name if a step
    /// fails.
    pub fn put_named_by_content(
        &self,
        folder: AudioFolder,
        fill: impl FnOnce(&Path) -> Result<(), String>,
    ) -> Result<(AudioName, PathBuf), String> {
        let tmp = self.temp_in(folder)?;
        fill(&tmp)?;
        let bytes =
            std::fs::read(&tmp).map_err(|e| format!("could not read what was written: {e}"))?;
        let name = AudioName::of_file_bytes(&bytes);
        let path = self.path(folder, name.as_str());
        if path.is_file() {
            return Ok((name, path));
        }
        match tmp.persist(&path) {
            Ok(()) => Ok((name, path)),
            Err(_) if path.is_file() => Ok((name, path)),
            Err(e) => Err(format!("could not put the file in place: {}", e.error)),
        }
    }

    /// The file at `path`, if it is one the store may delete: a regular file
    /// (not a link, not a folder) **directly** in one of the store's folders
    /// of this project. `None` for anything else.
    ///
    /// The folder is the store's own ([`AudioFolder::dir`] of this project),
    /// and `path`'s parent is compared with each folder **by identity**
    /// ([`crate::live::identify`]: device and inode, or volume serial number
    /// and file id), never by spelling, so a path that only reads as one of
    /// them (`derived/../../take.wav`) is not in it, and one spelled another
    /// way (a link to the project, a case-folded name, the copy of a project
    /// read through [`crate::relocate`]) is. Identity needs no canonical
    /// path, which some volumes cannot give (ImDisk, vboxsf, WebDAV); the
    /// path returned is the canonical one where there is one, and the path as
    /// spelled where canonicalising fails for any reason but the file not
    /// being there ([`resolved_or_spelled`]). The file itself is not
    /// followed: a link in `derived/` is not the store's, whether it points
    /// into the folder or out of it (a caller that resolves a path before it
    /// asks, as compaction does, asks about the link's target), and neither
    /// is a file in a folder below one of them. A node can name any file
    /// (`load` references the user's own recording in place), and this is
    /// what keeps a deleter from taking one (#589).
    ///
    /// The answer says which folder: a deleter that works in one (compaction
    /// in `derived/`) checks it.
    ///
    /// A caller asking about many paths at once takes the folders' identities
    /// once with [`folders`](Self::folders) and asks [`Folders::owns`].
    pub fn owns(&self, path: &Path) -> Option<OwnedFile> {
        self.folders().owns(path)
    }

    /// The store's folders as they are now, identified once, for a caller
    /// that asks [`Folders::owns`] or [`Folders::folder_of`] about many
    /// paths in one go (the head's clips, a node's missing files). A folder
    /// made or removed after this is taken is not seen; take it again.
    pub fn folders(&self) -> Folders<'_> {
        Folders {
            store: self,
            ids: AudioFolder::ALL.map(|f| identity(&f.dir(&self.project_dir))),
        }
    }

    /// Every file the store may delete in `folder` ([`owns`](Self::owns)):
    /// the regular files directly in it, in no particular order. Empty when
    /// the folder is not there. A file that goes while it is listed is left
    /// out.
    pub fn files(&self, folder: AudioFolder) -> std::io::Result<Vec<OwnedFile>> {
        let spelled = folder.dir(&self.project_dir);
        let Some(real) = resolved_or_spelled(&spelled, std::fs::canonicalize(&spelled)) else {
            return Ok(Vec::new());
        };
        if !real.is_dir() {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        for entry in std::fs::read_dir(&real)? {
            if let Some(file) = owned_entry(folder, real.join(entry?.file_name())) {
                out.push(file);
            }
        }
        Ok(out)
    }

    /// Which of the store's folders `path` is directly in, whether or not a
    /// file is there. `path`'s parent is compared with each folder by
    /// identity ([`crate::live::identify`]), as [`owns`](Self::owns)
    /// compares, and by spelling only when neither of the two can be
    /// identified (a folder not made yet, or removed: there, the same
    /// spelling is the same place). It asks nothing about the file, so it is
    /// for a rebuild asking where a gone file belongs; a delete asks
    /// [`owns`](Self::owns).
    pub fn folder_of(&self, path: &Path) -> Option<AudioFolder> {
        self.folders().folder_of(path)
    }

    /// Which of the store's folders the folder `dir` is, by identity; `None`
    /// for any other folder, and for one that cannot be read. Save As asks it
    /// once per folder it copies.
    pub fn folder_at(&self, dir: &Path) -> Option<AudioFolder> {
        let id = identity(dir)?;
        AudioFolder::ALL
            .into_iter()
            .find(|f| identity(&f.dir(&self.project_dir)) == Some(id))
    }

    /// What a replay wrote into this store's `derived/` (a scratch project's),
    /// by every name a node may record for each file ([`Rebuilt`]). `nodes`
    /// are the scratch project's own: a clip of one that names a file this
    /// store wrote in `derived/` says, by its `content_hash`, what that file
    /// was called before #620. A folder that cannot be listed wrote nothing.
    pub fn rebuilt(&self, nodes: &[SessionNode]) -> Rebuilt {
        let produced: BTreeSet<OsString> = self
            .files(AudioFolder::Derived)
            .unwrap_or_default()
            .iter()
            .map(|f| f.name().to_os_string())
            .collect();
        let folders = self.folders();
        let mut legacy = BTreeMap::new();
        for clip in nodes
            .iter()
            .flat_map(|n| &n.state.tracks)
            .flat_map(|t| &t.clips)
        {
            let (Some(hash), Some(written)) = (clip.content_hash, clip.source_path.file_name())
            else {
                continue;
            };
            // Only what this store wrote: the step read the real project's
            // audio, and a clip naming that is not what the replay made.
            if !produced.contains(written)
                || folders.folder_of(&clip.source_path) != Some(AudioFolder::Derived)
            {
                continue;
            }
            let older = OsString::from(format!("{}.wav", legacy_stem(&hash)));
            if older != written {
                legacy.insert(older, written.to_os_string());
            }
        }
        Rebuilt {
            dir: AudioFolder::Derived.dir(&self.project_dir),
            produced,
            legacy,
        }
    }

    /// A temporary file in `folder` (`.cas-*.wav.part`), making the folder if
    /// it is not there yet, removed on drop unless it is persisted.
    fn temp_in(&self, folder: AudioFolder) -> Result<tempfile::TempPath, String> {
        let dir = folder.dir(&self.project_dir);
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("failed to create {}: {e}", dir.display()))?;
        Ok(tempfile::Builder::new()
            .prefix(PUT_STAGING_PREFIX)
            .suffix(STAGING_SUFFIX)
            .tempfile_in(&dir)
            .map_err(|e| format!("failed to create a file in {}: {e}", dir.display()))?
            .into_temp_path())
    }
}

/// The identity of the folder `dir`, if it can be read.
fn identity(dir: &Path) -> Option<crate::live::FolderId> {
    crate::live::identify(dir).ok().map(|i| i.id())
}

/// `path`'s folder, `.` for a bare file name.
fn parent_of(path: &Path) -> &Path {
    match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    }
}

/// A store's folders with their identities, read once
/// ([`AudioStore::folders`]).
#[derive(Debug)]
pub struct Folders<'a> {
    store: &'a AudioStore,
    /// Each of [`AudioFolder::ALL`]'s identity, `None` where it cannot be
    /// read (not made yet).
    ids: [Option<crate::live::FolderId>; 3],
}

impl Folders<'_> {
    /// [`AudioStore::owns`], against these identities.
    pub fn owns(&self, path: &Path) -> Option<OwnedFile> {
        let name = path.file_name()?;
        let parent = parent_of(path);
        let parent_id = identity(parent)?;
        let folder = AudioFolder::ALL
            .into_iter()
            .zip(self.ids)
            .find(|(_, id)| *id == Some(parent_id))?
            .0;
        let dir = resolved_or_spelled(parent, std::fs::canonicalize(parent))?;
        owned_entry(folder, dir.join(name))
    }

    /// [`AudioStore::folder_of`], against these identities.
    pub fn folder_of(&self, path: &Path) -> Option<AudioFolder> {
        path.file_name()?;
        let parent = parent_of(path);
        let parent_id = identity(parent);
        AudioFolder::ALL
            .into_iter()
            .zip(self.ids)
            .find(|(f, id)| match (parent_id, id) {
                (Some(a), Some(b)) => a == *b,
                (None, None) => parent == f.dir(&self.store.project_dir),
                _ => false,
            })
            .map(|(f, _)| f)
    }
}

/// What a path to delete is spelled as, from what canonicalising `spelled`
/// gave: the canonical path; `None` when nothing is there; and `spelled`
/// itself for any other failure, because some volumes cannot give a
/// canonical path for a file that is there (ImDisk, vboxsf and WebDAV, where
/// `realpath` or `GetFinalPathNameByHandleW` fails). Which folder a file is in
/// is decided by identity ([`AudioStore::owns`]), not by this path.
fn resolved_or_spelled(spelled: &Path, canonical: std::io::Result<PathBuf>) -> Option<PathBuf> {
    match canonical {
        Ok(path) => Some(path),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => Some(spelled.to_path_buf()),
    }
}

/// What a replay wrote into its scratch project's `derived/`, answering to
/// every name a node may record for each file (#620), made by
/// [`AudioStore::rebuilt`].
///
/// A file answers to the name it has, and, when the clip that names it
/// records the blake3 of its samples alone as its `content_hash` (what every
/// edit has always stored there), to the name those samples had before #620,
/// which is the name a node written then records. Names are compared, never
/// worked out from a file: a `derived/` file holds 16-bit PCM, not the
/// samples as they were hashed. Nothing on disk is renamed and no stored op
/// says which scheme named a file, so this older name is recognised for as
/// long as a project written before #620 can be opened.
#[derive(Debug, Clone, Default)]
pub struct Rebuilt {
    /// The scratch project's `derived/`.
    dir: PathBuf,
    /// The files the replay wrote, by the names it gave them.
    produced: BTreeSet<OsString>,
    /// For a file in `produced` that an older build would have called
    /// something else: that older name, to the file's name.
    legacy: BTreeMap<OsString, OsString>,
}

impl Rebuilt {
    /// The files the replay wrote, by the names it gave them.
    pub fn produced(&self) -> &BTreeSet<OsString> {
        &self.produced
    }

    /// Every name a node may record for what the replay wrote: the names it
    /// gave the files, and the names an older build would have given them.
    /// A recorded name in this list is a name the replay rebuilt.
    pub fn names(&self) -> impl Iterator<Item = &OsString> {
        self.produced.iter().chain(self.legacy.keys())
    }

    /// The file that answers to the recorded file name `recorded`, if one
    /// does: the file of that name, or the one an older build would have
    /// called that.
    fn answering(&self, recorded: &OsStr) -> Option<PathBuf> {
        let written = if self.produced.contains(recorded) {
            recorded
        } else {
            self.legacy.get(recorded)?.as_os_str()
        };
        Some(self.dir.join(written))
    }

    /// Put the file that answers to `recorded` (a file name a node records)
    /// into `into`'s `derived/` under that recorded name, through
    /// [`AudioStore::put_with`]: whole or not at all, and a file already
    /// there is kept. The recorded name is the one the node reads, so a node
    /// written before #620 has its file back under its older name. `Ok(None)`
    /// when no file the replay wrote answers to it: the replay did not
    /// reproduce what the node names.
    pub fn put_back(&self, recorded: &OsStr, into: &AudioStore) -> Result<Option<PathBuf>, String> {
        let (Some(from), Some(name)) = (self.answering(recorded), AudioName::of_recorded(recorded))
        else {
            return Ok(None);
        };
        let place = into.path(AudioFolder::Derived, name.as_str());
        // `put_with` keeps whatever is under the name; something there that
        // is not a file (a folder, a link to one) would be kept too, and the
        // rebuild would then report an edit that is not deterministic.
        if place.exists() && !place.is_file() {
            return Err(format!(
                "cannot put {} back: something that is not a file is there",
                place.display()
            ));
        }
        into.put_with(AudioFolder::Derived, &name, |tmp| {
            copy_whole(&from, tmp)
                .map(drop)
                .map_err(|e| format!("could not put {} back: {e}", place.display()))
        })
        .map(Some)
    }
}

/// Copy `from` onto `to`, whole and synced to the disk; how many bytes.
fn copy_whole(from: &Path, to: &Path) -> io::Result<u64> {
    let mut src = std::fs::File::open(from)?;
    let mut dst = std::fs::File::create(to)?;
    let bytes = io::copy(&mut src, &mut dst)?;
    dst.sync_all()?;
    Ok(bytes)
}

/// How a copy into the project ([`Counterpart::copy_in`]) names its staging
/// file: `.repair-*.wav.part`. Its own prefix, because a killed copy's
/// staging files in `clipboard/` and `generated/` are removed by that prefix
/// ([`AudioDeleter::remove_copy_leftovers`]), which a writer's `.cas-` one
/// is not: a writer may hold no handle on the project (a punch-in's take).
const COPY_STAGING_PREFIX: &str = ".repair-";
const STAGING_SUFFIX: &str = ".wav.part";

/// How a put ([`AudioStore::put_with`] and the others) names its staging file:
/// `.cas-*.wav.part`, removed on drop unless it is renamed into place, so one
/// is left only by a writer that was killed mid-write.
const PUT_STAGING_PREFIX: &str = ".cas-";

/// How old a put's staging file in `clipboard/` or `generated/` must be before
/// it counts as left behind ([`AudioDeleter::remove_stale_put_leftovers`]): a
/// day. A writer there may hold no handle on the project (a punch-in stores its
/// take with `store_clipboard_blob` while holding none), so the staged check
/// cannot tell a file being written from a leftover; age can, since no write
/// there lasts more than a few minutes (a punch-in's region is at most 300 s,
/// with at most 30 s each side).
pub const STALE_PUT_AGE: Duration = Duration::from_secs(24 * 60 * 60);

/// Put what `fill` writes at `to`, by way of a staging file beside it
/// (`.repair-*.wav.part`) synced and renamed over `to` once whole. Nothing is
/// left at `to`, nor a staging file, when `fill` or the rename fails.
fn put_staged(
    to: &Path,
    fill: impl FnOnce(&mut std::fs::File) -> io::Result<u64>,
) -> io::Result<u64> {
    let parent = to
        .parent()
        .ok_or_else(|| io::Error::other("the destination has no folder"))?;
    std::fs::create_dir_all(parent)?;
    let mut part = tempfile::Builder::new()
        .prefix(COPY_STAGING_PREFIX)
        .suffix(STAGING_SUFFIX)
        .tempfile_in(parent)?;
    let bytes = fill(part.as_file_mut())?;
    part.as_file().sync_all()?;
    part.persist(to).map_err(|e| e.error)?;
    Ok(bytes)
}

/// Where this project keeps a file a clip names in another project's store
/// folder: the same name, in this project's folder of the same kind
/// (`session::relocate`, rule 1), whether or not either file is there. Made
/// only inside `session`, by `Store::counterpart` (the store's `Rebinder`), for
/// the store's own project, so a copy into the project goes to a place the
/// store worked out, never one a caller spelled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Counterpart {
    folder: AudioFolder,
    path: PathBuf,
}

impl Counterpart {
    pub(crate) fn new(folder: AudioFolder, path: PathBuf) -> Self {
        Self { folder, path }
    }

    /// The store's folder it is in.
    pub fn folder(&self) -> AudioFolder {
        self.folder
    }

    /// Where it is: this project's folder as the store spells it, and the
    /// name.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Copy `from`, the other project's file of this name, here (#612): by
    /// way of a staging file beside it, renamed into place only when whole
    /// and only if nothing is here by then, keeping `from`'s modification
    /// time (which the history sweep takes the oldest by). How many bytes;
    /// `Ok(None)` when a file was here first, which is kept: the name is the
    /// content's, so it is the file the clip names.
    ///
    /// The rename is `tempfile`'s no-replace rename, and a hard link where
    /// the volume refuses that; on a volume that allows neither (some FAT and
    /// network volumes) it is an error, and nothing is written.
    pub fn copy_in(&self, from: &Path) -> io::Result<Option<u64>> {
        let parent = self
            .path
            .parent()
            .ok_or_else(|| io::Error::other("the destination has no folder"))?;
        std::fs::create_dir_all(parent)?;
        let mut part = tempfile::Builder::new()
            .prefix(COPY_STAGING_PREFIX)
            .suffix(STAGING_SUFFIX)
            .tempfile_in(parent)?;
        let mut src = std::fs::File::open(from)?;
        let bytes = io::copy(&mut src, part.as_file_mut())?;
        if let Ok(mtime) = src.metadata().and_then(|m| m.modified()) {
            let _ = part.as_file().set_modified(mtime);
        }
        part.as_file().sync_all()?;
        match part.persist_noclobber(&self.path) {
            Ok(_) => Ok(Some(bytes)),
            Err(e) if e.error.kind() == io::ErrorKind::AlreadyExists => Ok(None),
            Err(e) => Err(e.error),
        }
    }
}

/// `path` as an [`OwnedFile`] of `folder`, if it is a regular file.
fn owned_entry(folder: AudioFolder, path: PathBuf) -> Option<OwnedFile> {
    let meta = std::fs::symlink_metadata(&path).ok()?;
    meta.file_type().is_file().then(|| OwnedFile {
        folder,
        bytes: meta.len(),
        modified: meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
        path,
    })
}

/// A file the store may delete, as [`AudioStore::owns`] or
/// [`AudioStore::files`] found it. Only they make one, so
/// [`AudioDeleter::remove`] is never handed a path somebody spelled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedFile {
    folder: AudioFolder,
    path: PathBuf,
    bytes: u64,
    modified: SystemTime,
}

impl OwnedFile {
    /// The store's folder it is in.
    pub fn folder(&self) -> AudioFolder {
        self.folder
    }

    /// Where it is: the folder as the file system resolves it (as spelled on
    /// a volume that cannot resolve it), and the file's name.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Its file name.
    pub fn name(&self) -> &OsStr {
        self.path.file_name().unwrap_or_default()
    }

    /// Where it is relative to its project's folder
    /// (`.audiograph/derived/<name>`, `generated/<name>`, ...): where a copy
    /// of the project holds it.
    pub fn in_project(&self) -> PathBuf {
        self.folder.dir(Path::new("")).join(self.name())
    }

    /// Copy this file into `into`'s folder of the same kind, under the same
    /// name: Save As copying the project's audio into the new folder (#571).
    /// By way of a staging file in that folder (`.repair-*.wav.part`, which
    /// the copy's own sweeps remove if this is cut short), renamed into place
    /// once whole and synced, so a copy that is killed or runs out of disk
    /// never leaves part of a file under a name that is trusted on sight (a
    /// rebuild would take it for the audio and never write it again). The
    /// file's permissions go with it, as `std::fs::copy` takes them. How many
    /// bytes.
    ///
    /// The rename replaces: the destination is a new project that only this
    /// copy writes (Save As refuses a folder that is not empty), and a rename
    /// that replaces works on every volume, where a no-replace one does not.
    pub fn copy_into(&self, into: &AudioStore) -> io::Result<u64> {
        let to = self.folder.dir(&into.project_dir).join(self.name());
        let mut src = std::fs::File::open(&self.path)?;
        let permissions = src.metadata()?.permissions();
        put_staged(&to, |part| {
            let bytes = io::copy(&mut src, part)?;
            part.set_permissions(permissions)?;
            Ok(bytes)
        })
    }

    /// Its size when it was found.
    pub fn bytes(&self) -> u64 {
        self.bytes
    }

    /// When it was last written, as it was found; the epoch if the file
    /// system does not say.
    pub fn modified(&self) -> SystemTime {
        self.modified
    }
}

/// Leave to delete the project's audio (#719 stage 3a, #421, #566): handed
/// out by [`Store::audio_deleter`] only while no handle on the project is
/// staged, and asked again at every delete.
///
/// A staged handle is an edit running off the store's lock (it has written
/// its output and no node names it yet, so a sweep would take it for an
/// orphan), Save As copying the project's audio (#571), or a repair copying
/// audio in from the project this one was copied from (#612). The count is
/// the folder's, shared by every `Store` on it ([`Store::staged_in_flight`]),
/// and it cannot go up from the store this was made from while its lock is
/// held; asking again before each delete also covers a stage begun from
/// another `Store` on the folder meanwhile.
pub struct AudioDeleter<'a> {
    store: &'a Store,
    audio: AudioStore,
}

impl std::fmt::Debug for AudioDeleter<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AudioDeleter")
            .field("audio", &self.audio)
            .finish_non_exhaustive()
    }
}

impl AudioDeleter<'_> {
    /// The store whose files this deletes.
    pub fn audio(&self) -> &AudioStore {
        &self.audio
    }

    /// Delete `file`, if it is still this project's
    /// ([`AudioStore::owns`], in the same folder) and no handle on the
    /// project has been staged since. Whether it was deleted; a file that is
    /// not, for any reason, is left as it is.
    pub fn remove(&self, file: &OwnedFile) -> bool {
        self.remove_if(file, |_| true)
    }

    /// [`remove`](Self::remove), when `still` holds for the file as it is at
    /// the delete (its size and time read again, not as `file` found them).
    fn remove_if(&self, file: &OwnedFile, still: impl FnOnce(&OwnedFile) -> bool) -> bool {
        if self.store.staged_in_flight() > 0 {
            return false;
        }
        match self.audio.owns(&file.path) {
            Some(now) if now.folder == file.folder && still(&now) => {
                std::fs::remove_file(&now.path).is_ok()
            }
            _ => false,
        }
    }

    /// Remove the staging files a copy into the project
    /// ([`Counterpart::copy_in`]) left in `clipboard/` and `generated/` when
    /// the app was killed mid-copy (the orphan sweep removes those in
    /// `derived/`); how many went. A copy running now holds a staged handle,
    /// so while it runs this deleter is not handed out, or refuses at the
    /// delete. A file that cannot be removed is left for the next open.
    pub fn remove_copy_leftovers(&self) -> usize {
        let mut removed = 0;
        for folder in [AudioFolder::Clipboard, AudioFolder::Generated] {
            let Ok(files) = self.audio.files(folder) else {
                continue;
            };
            for file in files {
                let name = file.name().to_string_lossy();
                if name.starts_with(COPY_STAGING_PREFIX)
                    && name.ends_with(STAGING_SUFFIX)
                    && self.remove(&file)
                {
                    removed += 1;
                }
            }
        }
        removed
    }

    /// Remove the staging files a put ([`AudioStore::put_with`],
    /// `.cas-*.wav.part`) left in `clipboard/` and `generated/` when its writer
    /// was killed mid-write, once they are [`STALE_PUT_AGE`] old at `now`; how
    /// many went. Run with the orphan sweep as a project opens (which removes
    /// those in `derived/`, at any age, since no node names them).
    ///
    /// Age is the test because a writer there may hold no staged handle (a
    /// punch-in's take), so a young file may be one being written now. Each
    /// delete is [`remove`](Self::remove)'s, with the age read again from the
    /// file at the delete: nothing goes while a handle is staged, and nothing
    /// that is not this project's file in that folder. A file whose time is
    /// after `now` (a clock set back) is not old, and is kept.
    pub fn remove_stale_put_leftovers(&self, now: SystemTime) -> usize {
        let stale = |file: &OwnedFile| {
            now.duration_since(file.modified())
                .is_ok_and(|age| age >= STALE_PUT_AGE)
        };
        let mut removed = 0;
        for folder in [AudioFolder::Clipboard, AudioFolder::Generated] {
            let Ok(files) = self.audio.files(folder) else {
                continue;
            };
            for file in files {
                let name = file.name().to_string_lossy();
                if name.starts_with(PUT_STAGING_PREFIX)
                    && name.ends_with(STAGING_SUFFIX)
                    && stale(&file)
                    && self.remove_if(&file, stale)
                {
                    removed += 1;
                }
            }
        }
        removed
    }
}

impl Store {
    /// This project's audio store.
    pub fn audio_store(&self) -> AudioStore {
        AudioStore::new(self.project_dir())
    }

    /// Leave to delete this project's audio, or `None` while a handle on the
    /// project is staged ([`AudioDeleter`]), and always `None` for a store
    /// that only reads ([`Store::open_read_only`]): a reader works beside the
    /// live store and does not hold its lock, so nothing it read is known to
    /// still hold. Every deleter of the project's audio starts here: the
    /// sweeps and compaction in `tools::reclaim`, and a repair's leftovers.
    pub fn audio_deleter(&self) -> Option<AudioDeleter<'_>> {
        (!self.is_read_only() && self.staged_in_flight() == 0).then(|| AudioDeleter {
            store: self,
            audio: self.audio_store(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::EnvelopeFrames;

    /// 4,800 samples of `sin(i * 0.01)`, the buffer the golden names below
    /// were taken from on main before this module existed.
    fn samples() -> Vec<f32> {
        // Exact IEEE arithmetic, so every platform builds the same bits
        // (`f32::sin` goes to the platform's libm, which rounds differently).
        (0..4800u32)
            .map(|i| ((i * 37) % 20_000) as f32 / 10_000.0 - 1.0)
            .collect()
    }

    /// A name for the store's own tests, which put files that are not audio.
    fn x() -> AudioName {
        AudioName("x".to_string())
    }

    const MONO_48K: &str = "ccbf40d3cc7e60382546ede040961f2320fd34a66db88b5b8e836723d6eb20f5";
    const STEREO_24K: &str = "2294fd7465418a3893b8c08bb357ca25296aeca43b67a5d86e34515792ddd553";
    const LEGACY: &str = "40dd2e135154bdb28cfa008dea44526caf12c5f92cf4f7e1ba88098c99372efe";

    #[test]
    fn audio_hash_names_are_unchanged() {
        let s = samples();
        assert_eq!(audio_hash(&s, 48_000, 1), MONO_48K);
        assert_eq!(audio_hash(&s, 24_000, 2), STEREO_24K);
        assert_ne!(MONO_48K, STEREO_24K, "the layout is part of the name");
    }

    #[test]
    fn legacy_names_are_unchanged() {
        let s = samples();
        assert_eq!(legacy_sample_hash_name(&s), LEGACY);
        assert_eq!(legacy_stem(&legacy_sample_hash(&s)), LEGACY);
    }

    #[test]
    fn legacy_name_ignores_layout() {
        let s = samples();
        // The same samples at two layouts get two names now; the older name
        // takes the samples alone (no rate, no channel count), so both
        // layouts shared it: the bug #620 fixed, and the reason nothing may
        // write one.
        assert_ne!(audio_hash(&s, 48_000, 1), audio_hash(&s, 24_000, 2));
        assert_ne!(legacy_sample_hash_name(&s), audio_hash(&s, 48_000, 1));
        assert_ne!(legacy_sample_hash_name(&s), audio_hash(&s, 24_000, 2));
    }

    #[test]
    fn folders_are_where_they_were() {
        let project = Path::new("proj");
        assert_eq!(
            AudioFolder::Derived.dir(project),
            project.join(".audiograph").join("derived")
        );
        assert_eq!(
            AudioFolder::Clipboard.dir(project),
            project.join(".audiograph").join("clipboard")
        );
        assert_eq!(
            AudioFolder::Generated.dir(project),
            project.join("generated")
        );
        let names: Vec<_> = AudioFolder::ALL.iter().map(|f| f.name()).collect();
        assert_eq!(names, ["derived", "clipboard", "generated"]);
        let store = AudioStore::new(project);
        assert_eq!(
            store.path(AudioFolder::Derived, "abc"),
            project.join(".audiograph").join("derived").join("abc.wav")
        );
    }

    #[test]
    fn put_keeps_a_file_already_there() {
        let dir = tempfile::tempdir().unwrap();
        let store = AudioStore::new(dir.path());
        let path = store.path(AudioFolder::Derived, "x");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"first").unwrap();
        let got = store
            .put_with(AudioFolder::Derived, &x(), |_| {
                panic!("fill must not run for a name that is there")
            })
            .unwrap();
        assert_eq!(got, path);
        assert_eq!(std::fs::read(&path).unwrap(), b"first");
    }

    #[test]
    fn a_fill_that_fails_leaves_nothing_under_the_name() {
        let dir = tempfile::tempdir().unwrap();
        let store = AudioStore::new(dir.path());
        let err = store
            .put_with(AudioFolder::Derived, &x(), |tmp| {
                std::fs::write(tmp, b"half").unwrap();
                Err("disk full".to_string())
            })
            .unwrap_err();
        assert_eq!(err, "disk full");
        assert!(!store.path(AudioFolder::Derived, "x").exists());
        let left: Vec<_> = std::fs::read_dir(AudioFolder::Derived.dir(dir.path()))
            .unwrap()
            .flatten()
            .collect();
        assert!(left.is_empty(), "no .part left behind: {left:?}");
    }

    #[test]
    fn a_lost_race_is_ok() {
        let dir = tempfile::tempdir().unwrap();
        let store = AudioStore::new(dir.path());
        let path = store.path(AudioFolder::Derived, "x");
        // Another writer puts the same name there while this one fills.
        let got = store
            .put_with(AudioFolder::Derived, &x(), |tmp| {
                std::fs::write(&path, b"whole").unwrap();
                std::fs::write(tmp, b"whole").unwrap();
                Ok(())
            })
            .unwrap();
        assert_eq!(got, path);
        assert_eq!(std::fs::read(&path).unwrap(), b"whole");
        let names: Vec<_> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .flatten()
            .map(|e| e.file_name())
            .collect();
        assert_eq!(names.len(), 1, "no .part left behind: {names:?}");
    }

    #[test]
    fn put_creates_the_folder() {
        let dir = tempfile::tempdir().unwrap();
        let store = AudioStore::new(dir.path());
        assert!(!AudioFolder::Generated.dir(dir.path()).exists());
        let got = store
            .put_with(AudioFolder::Generated, &x(), |tmp| {
                std::fs::write(tmp, b"audio").map_err(|e| e.to_string())
            })
            .unwrap();
        assert_eq!(got, dir.path().join("generated").join("x.wav"));
        assert_eq!(std::fs::read(&got).unwrap(), b"audio");
    }

    #[test]
    fn names_are_unchanged() {
        let s = samples();
        assert_eq!(AudioName::of_samples(&s, 48_000, 1).as_str(), MONO_48K);
        assert_eq!(AudioName::of_samples(&s, 24_000, 2).as_str(), STEREO_24K);
        // A mixed-down track: the blake3 of the WAV's bytes, header included.
        let bytes = b"RIFF....WAVEfmt the whole file";
        assert_eq!(
            AudioName::of_file_bytes(bytes).as_str(),
            blake3::hash(bytes).to_hex().as_str()
        );
        // A lane copy: `track-` and the blake3 of the clip descriptors, as
        // `tools::util::flattened_track_path` named it before stage 2.
        let clip = Clip {
            source_path: PathBuf::from("take.wav"),
            start_in_track: 10,
            source_offset: 20,
            length: 30,
            content_hash: None,
            time_stretch_factor: None,
            pitch_shift_semitones: None,
            beat_grid: None,
            volume_envelope: Vec::new(),
            volume_envelope_frames: EnvelopeFrames::default(),
        };
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"take.wav");
        hasher.update(&10u64.to_le_bytes());
        hasher.update(&20u64.to_le_bytes());
        hasher.update(&30u64.to_le_bytes());
        assert_eq!(
            AudioName::of_lane(std::slice::from_ref(&clip)).as_str(),
            format!("track-{}", hasher.finalize().to_hex())
        );
    }

    #[test]
    fn put_verified_keeps_a_file_that_checks_out() {
        let dir = tempfile::tempdir().unwrap();
        let store = AudioStore::new(dir.path());
        let path = store.path(AudioFolder::Clipboard, "x");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"good").unwrap();
        let got = store
            .put_verified(
                AudioFolder::Clipboard,
                &x(),
                |p| std::fs::read(p).unwrap() == b"good",
                |_| panic!("fill must not run for a file that checks out"),
            )
            .unwrap();
        assert_eq!(got, path);
        assert_eq!(std::fs::read(&path).unwrap(), b"good");
    }

    #[test]
    fn put_verified_replaces_a_file_that_does_not() {
        let dir = tempfile::tempdir().unwrap();
        let store = AudioStore::new(dir.path());
        let path = store.path(AudioFolder::Clipboard, "x");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"lossy").unwrap();
        store
            .put_verified(
                AudioFolder::Clipboard,
                &x(),
                |p| std::fs::read(p).unwrap() == b"good",
                |tmp| std::fs::write(tmp, b"good").map_err(|e| e.to_string()),
            )
            .unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"good");
        let names: Vec<_> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .flatten()
            .map(|e| e.file_name())
            .collect();
        assert_eq!(names.len(), 1, "no .part left behind: {names:?}");
    }

    #[test]
    fn put_verified_keeps_the_old_file_when_the_new_one_cannot_be_written() {
        let dir = tempfile::tempdir().unwrap();
        let store = AudioStore::new(dir.path());
        let path = store.path(AudioFolder::Clipboard, "x");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"lossy").unwrap();
        let err = store
            .put_verified(
                AudioFolder::Clipboard,
                &x(),
                |_| false,
                |tmp| {
                    std::fs::write(tmp, b"ha").unwrap();
                    Err("disk full".to_string())
                },
            )
            .unwrap_err();
        assert_eq!(err, "disk full");
        assert_eq!(
            std::fs::read(&path).unwrap(),
            b"lossy",
            "never half replaced"
        );
    }

    #[test]
    fn put_named_by_content_names_the_file_after_it_is_written() {
        let dir = tempfile::tempdir().unwrap();
        let store = AudioStore::new(dir.path());
        let fill = |tmp: &Path| std::fs::write(tmp, b"mixdown").map_err(|e| e.to_string());
        let (name, path) = store
            .put_named_by_content(AudioFolder::Generated, fill)
            .unwrap();
        assert_eq!(name, AudioName::of_file_bytes(b"mixdown"));
        assert_eq!(path, store.path_of(AudioFolder::Generated, &name).as_path());
        assert_eq!(std::fs::read(&path).unwrap(), b"mixdown");
        // The same content again: the file there is kept, nothing left over.
        let (again, _) = store
            .put_named_by_content(AudioFolder::Generated, fill)
            .unwrap();
        assert_eq!(again, name);
        let names: Vec<_> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .flatten()
            .map(|e| e.file_name())
            .collect();
        assert_eq!(names.len(), 1, "no .part left behind: {names:?}");
    }

    // ---- What the store may delete (stage 3a) ----

    /// `<dir>/<name>` with `bytes` in it, making the folder.
    fn file_at(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn owns_a_file_directly_in_each_folder() {
        let dir = tempfile::tempdir().unwrap();
        let store = AudioStore::new(dir.path());
        for folder in AudioFolder::ALL {
            let path = file_at(&folder.dir(dir.path()), "a.wav", b"four");
            let owned = store.owns(&path).unwrap_or_else(|| panic!("{folder:?}"));
            assert_eq!(owned.folder(), folder);
            assert_eq!(owned.bytes(), 4);
            assert_eq!(owned.name(), "a.wav");
            assert_eq!(
                std::fs::canonicalize(owned.path()).unwrap(),
                std::fs::canonicalize(&path).unwrap()
            );
        }
        // Whatever the file is called: a killed writer's `.part` is the
        // store's too, which is how the orphan sweep removes one.
        let part = file_at(&derived_dir(dir.path()), ".cas-x.wav.part", b"");
        assert!(store.owns(&part).is_some());
    }

    #[test]
    fn owns_nothing_else() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project");
        let store = AudioStore::new(&project);
        let derived = derived_dir(&project);
        // Beside the project's folders, in the store folder's own parent,
        // in a folder below one, missing, or a folder.
        let beside = file_at(&project, "take.wav", b"user");
        let store_dir = file_at(&project.join(STORE_DIR), "head", b"id");
        let nested = file_at(&derived.join("sub"), "a.wav", b"deep");
        assert!(store.owns(&beside).is_none());
        assert!(store.owns(&store_dir).is_none());
        assert!(store.owns(&nested).is_none());
        assert!(store.owns(&derived.join("gone.wav")).is_none());
        assert!(store.owns(&derived.join("sub")).is_none());
        // A path that only reads as `derived/`.
        let dotdot = derived.join("..").join("..").join("take.wav");
        assert!(dotdot.is_file(), "the test's own path resolves");
        assert!(store.owns(&dotdot).is_none());
        // Another project's `derived/`.
        let other = file_at(&derived_dir(&dir.path().join("other")), "a.wav", b"x");
        assert!(store.owns(&other).is_none());
        // Nothing for a path with no file name.
        assert!(store.owns(&derived.join("..")).is_none());
    }

    #[test]
    fn a_path_spelled_another_way_is_resolved() {
        let dir = tempfile::tempdir().unwrap();
        let store = AudioStore::new(dir.path());
        let path = file_at(&derived_dir(dir.path()), "a.wav", b"x");
        let roundabout = derived_dir(dir.path())
            .join("..")
            .join(DERIVED_DIR)
            .join("a.wav");
        let owned = store.owns(&roundabout).expect("the same file");
        assert_eq!(
            owned.path(),
            std::fs::canonicalize(&path).unwrap().as_path()
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_link_in_a_folder_is_not_the_stores() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project");
        let store = AudioStore::new(&project);
        let derived = derived_dir(&project);
        let outside = file_at(dir.path(), "take.wav", b"user");
        let inside = file_at(&derived, "a.wav", b"x");
        let out_link = derived.join("out.wav");
        let in_link = derived.join("in.wav");
        std::os::unix::fs::symlink(&outside, &out_link).unwrap();
        std::os::unix::fs::symlink(&inside, &in_link).unwrap();
        assert!(store.owns(&out_link).is_none(), "points out of the folder");
        assert!(store.owns(&in_link).is_none(), "a link, even into it");
        let names: Vec<_> = store
            .files(AudioFolder::Derived)
            .unwrap()
            .into_iter()
            .map(|f| f.name().to_owned())
            .collect();
        assert_eq!(names, ["a.wav"], "files() lists no link either");

        // A project reached through a link is the same project.
        let linked = dir.path().join("linked");
        std::os::unix::fs::symlink(&project, &linked).unwrap();
        let through = AudioStore::new(&linked);
        assert!(through.owns(&inside).is_some());
        assert!(store.owns(&derived_dir(&linked).join("a.wav")).is_some());
    }

    /// Compared by identity: a project reached through a link to its folder,
    /// or through a link to one of its store folders, is the same folders,
    /// whichever spelling the store and the path use.
    #[cfg(unix)]
    #[test]
    fn folders_are_compared_by_identity_not_spelling() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project");
        let file = file_at(&derived_dir(&project), "a.wav", b"x");
        let linked = dir.path().join("linked");
        std::os::unix::fs::symlink(&project, &linked).unwrap();
        // A link straight to `derived/`, from outside the project.
        let to_derived = dir.path().join("to-derived");
        std::os::unix::fs::symlink(derived_dir(&project), &to_derived).unwrap();

        for (store_at, path) in [
            (&project, derived_dir(&linked).join("a.wav")),
            (&linked, file.clone()),
            (&linked, to_derived.join("a.wav")),
            (&project, to_derived.join("a.wav")),
        ] {
            let owned = AudioStore::new(store_at)
                .owns(&path)
                .unwrap_or_else(|| panic!("{} through {}", path.display(), store_at.display()));
            assert_eq!(owned.folder(), AudioFolder::Derived);
            assert_eq!(
                std::fs::canonicalize(owned.path()).unwrap(),
                std::fs::canonicalize(&file).unwrap()
            );
        }
        let listed = AudioStore::new(&linked)
            .files(AudioFolder::Derived)
            .unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name(), "a.wav");
    }

    #[test]
    fn a_path_that_cannot_be_canonicalised_is_used_as_spelled() {
        let spelled = Path::new("vboxsf").join("derived");
        let canonical = PathBuf::from("/real/derived");
        assert_eq!(
            resolved_or_spelled(&spelled, Ok(canonical.clone())),
            Some(canonical),
            "the canonical path where there is one"
        );
        let missing = std::io::Error::from(std::io::ErrorKind::NotFound);
        assert_eq!(
            resolved_or_spelled(&spelled, Err(missing)),
            None,
            "nothing there"
        );
        for kind in [
            std::io::ErrorKind::Unsupported,
            std::io::ErrorKind::PermissionDenied,
            std::io::ErrorKind::Other,
        ] {
            assert_eq!(
                resolved_or_spelled(&spelled, Err(kind.into())),
                Some(spelled.clone()),
                "{kind:?}: as spelled"
            );
        }
    }

    #[test]
    fn files_lists_the_regular_files_directly_in_a_folder() {
        let dir = tempfile::tempdir().unwrap();
        let store = AudioStore::new(dir.path());
        assert!(
            store.files(AudioFolder::Derived).unwrap().is_empty(),
            "no folder"
        );
        let clipboard = clipboard_dir(dir.path());
        file_at(&clipboard, "a.wav", b"aa");
        file_at(&clipboard, "b.wav", b"bbb");
        file_at(&clipboard.join("sub"), "c.wav", b"c");
        let mut files = store.files(AudioFolder::Clipboard).unwrap();
        files.sort_by(|a, b| a.name().cmp(b.name()));
        let got: Vec<_> = files
            .iter()
            .map(|f| (f.name().to_owned(), f.bytes(), f.folder()))
            .collect();
        assert_eq!(
            got,
            [
                ("a.wav".into(), 2, AudioFolder::Clipboard),
                ("b.wav".into(), 3, AudioFolder::Clipboard)
            ]
        );
        for f in &files {
            assert_eq!(store.owns(f.path()).as_ref(), Some(f));
        }
    }

    // ---- Copiers and resolvers (stage 3b) ----

    /// A copy into the project never replaces a file: one that is there by
    /// the time the copy is whole stays as it was, and no staging file is
    /// left.
    #[test]
    fn a_copy_in_never_replaces_a_file() {
        let tmp = tempfile::tempdir().unwrap();
        let from = file_at(tmp.path(), "from.wav", b"the original's");
        let to = file_at(
            &derived_dir(&tmp.path().join("p")),
            "abc.wav",
            b"already here",
        );
        let here = Counterpart::new(AudioFolder::Derived, to.clone());

        assert_eq!(here.copy_in(&from).unwrap(), None);
        assert_eq!(std::fs::read(&to).unwrap(), b"already here");
        let names: Vec<_> = std::fs::read_dir(to.parent().unwrap())
            .unwrap()
            .flatten()
            .map(|e| e.file_name())
            .collect();
        assert_eq!(names, vec![OsString::from("abc.wav")]);
    }

    /// The copy keeps the source's modification time: the history sweep
    /// takes the oldest first, and a copied file stamped with the time of
    /// the copy would read as the newest history there is.
    #[test]
    fn a_copy_in_keeps_the_sources_time() {
        let tmp = tempfile::tempdir().unwrap();
        let from = file_at(tmp.path(), "from.wav", b"audio");
        let then = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_600_000_000);
        std::fs::File::options()
            .write(true)
            .open(&from)
            .unwrap()
            .set_modified(then)
            .unwrap();
        let to = derived_dir(&tmp.path().join("p")).join("abc.wav");
        let here = Counterpart::new(AudioFolder::Derived, to.clone());

        assert_eq!(here.copy_in(&from).unwrap(), Some(5));
        assert_eq!(std::fs::metadata(&to).unwrap().modified().unwrap(), then);
    }

    /// Where another project's file goes: this project's folder of the same
    /// kind, under the same name, for each folder; nothing for this
    /// project's own files or anything else.
    #[test]
    fn a_counterpart_is_the_same_name_in_this_projects_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let (here, there) = (tmp.path().join("here"), tmp.path().join("there"));
        std::fs::create_dir_all(there.join(STORE_DIR)).unwrap();
        for folder in AudioFolder::ALL {
            let theirs = folder.dir(&there).join("a.wav");
            let got = crate::relocate::counterpart(&here, &theirs).expect("theirs");
            assert_eq!(got.folder(), folder);
            assert_eq!(got.path(), folder.dir(&here).join("a.wav"));
            assert!(
                crate::relocate::counterpart(&here, &folder.dir(&here).join("a.wav")).is_none()
            );
        }
        assert!(crate::relocate::counterpart(&here, &there.join("take.wav")).is_none());
    }

    /// A killed copy's staging files go from `clipboard/` and `generated/`;
    /// a writer's `.cas-` staging file and every other file stay, and nothing
    /// goes while a handle is staged.
    #[test]
    fn a_killed_copys_leftovers_go_and_nothing_else() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        for folder in [AudioFolder::Clipboard, AudioFolder::Generated] {
            let d = folder.dir(dir.path());
            file_at(&d, ".repair-KILLED.wav.part", b"half");
            file_at(&d, ".cas-WRITING.wav.part", b"half");
            file_at(&d, "keep.wav", b"audio");
        }
        file_at(&derived_dir(dir.path()), ".repair-X.wav.part", b"half");

        let deleter = store.audio_deleter().unwrap();
        let staged = store.stage().unwrap();
        assert_eq!(deleter.remove_copy_leftovers(), 0, "not while staged");
        drop(staged);
        assert_eq!(deleter.remove_copy_leftovers(), 2);
        for folder in [AudioFolder::Clipboard, AudioFolder::Generated] {
            let d = folder.dir(dir.path());
            assert!(!d.join(".repair-KILLED.wav.part").exists());
            assert!(d.join(".cas-WRITING.wav.part").is_file());
            assert!(d.join("keep.wav").is_file());
        }
        assert!(
            derived_dir(dir.path()).join(".repair-X.wav.part").is_file(),
            "the orphan sweep's, in `derived/`"
        );
    }

    /// `path`, written `age` before `now`.
    fn aged(path: PathBuf, now: SystemTime, age: Duration) -> PathBuf {
        let file = std::fs::File::options().write(true).open(&path).unwrap();
        file.set_modified(now - age).unwrap();
        path
    }

    const HOUR: Duration = Duration::from_secs(60 * 60);

    /// A put's staging file in `clipboard/` or `generated/` goes once it is
    /// a day old (#719 stage 4): a fresh one, which may be a punch-in's take
    /// being written with no handle staged, stays, as does one 23 hours old
    /// and one dated after `now`. Nothing else goes: not a file under a name,
    /// not a copy's staging file (that is `remove_copy_leftovers`'), and not
    /// one in `derived/` (the orphan sweep's).
    #[test]
    fn a_writers_staging_file_goes_once_it_is_a_day_old() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let now = SystemTime::now();
        for folder in [AudioFolder::Clipboard, AudioFolder::Generated] {
            let d = folder.dir(dir.path());
            aged(file_at(&d, ".cas-OLD.wav.part", b"half"), now, 25 * HOUR);
            file_at(&d, ".cas-FRESH.wav.part", b"half");
            aged(file_at(&d, ".cas-DAYISH.wav.part", b"half"), now, 23 * HOUR);
            let ahead = file_at(&d, ".cas-AHEAD.wav.part", b"half");
            std::fs::File::options()
                .write(true)
                .open(&ahead)
                .unwrap()
                .set_modified(now + HOUR)
                .unwrap();
            aged(file_at(&d, "keep.wav", b"audio"), now, 25 * HOUR);
            aged(file_at(&d, ".repair-X.wav.part", b"half"), now, 25 * HOUR);
        }
        let in_derived = aged(
            file_at(&derived_dir(dir.path()), ".cas-OLD.wav.part", b"half"),
            now,
            25 * HOUR,
        );

        let deleter = store.audio_deleter().unwrap();
        assert_eq!(deleter.remove_stale_put_leftovers(now), 2);
        for folder in [AudioFolder::Clipboard, AudioFolder::Generated] {
            let d = folder.dir(dir.path());
            assert!(!d.join(".cas-OLD.wav.part").exists(), "{}", d.display());
            for kept in [
                ".cas-FRESH.wav.part",
                ".cas-DAYISH.wav.part",
                ".cas-AHEAD.wav.part",
                "keep.wav",
                ".repair-X.wav.part",
            ] {
                assert!(d.join(kept).is_file(), "{kept} in {}", d.display());
            }
        }
        assert!(in_derived.is_file(), "the orphan sweep's, in `derived/`");
        assert_eq!(deleter.remove_stale_put_leftovers(now), 0, "already gone");
    }

    /// Nothing goes while a handle on the project is staged, a stage begun
    /// after the deleter was handed out included, since the delete is
    /// `remove`'s.
    #[test]
    fn no_stale_staging_file_goes_while_a_handle_is_staged() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let now = SystemTime::now();
        let old: Vec<PathBuf> = [AudioFolder::Clipboard, AudioFolder::Generated]
            .into_iter()
            .map(|f| {
                aged(
                    file_at(&f.dir(dir.path()), ".cas-OLD.wav.part", b"half"),
                    now,
                    25 * HOUR,
                )
            })
            .collect();

        let staged = store.stage().unwrap();
        assert!(store.audio_deleter().is_none(), "no leave while staged");
        drop(staged);
        let deleter = store.audio_deleter().unwrap();
        let staged = store.stage().unwrap();
        assert_eq!(
            deleter.remove_stale_put_leftovers(now),
            0,
            "not while staged"
        );
        assert!(old.iter().all(|p| p.is_file()));
        drop(staged);
        assert_eq!(deleter.remove_stale_put_leftovers(now), 2);
        assert!(old.iter().all(|p| !p.exists()));
    }

    /// A put's staging file is the one the leftovers rule looks for.
    #[test]
    fn a_put_stages_under_the_prefix_the_leftovers_rule_matches() {
        let dir = tempfile::tempdir().unwrap();
        let audio = AudioStore::new(dir.path());
        audio
            .put_with(AudioFolder::Clipboard, &x(), |tmp| {
                let name = tmp.file_name().unwrap().to_string_lossy().into_owned();
                assert!(name.starts_with(PUT_STAGING_PREFIX), "{name}");
                assert!(name.ends_with(STAGING_SUFFIX), "{name}");
                std::fs::write(tmp, b"whole").map_err(|e| e.to_string())
            })
            .unwrap();
    }

    /// A Save As copy of a store file that fails part-way (a full disk, a
    /// read error) leaves nothing under the name, and no staging file.
    #[test]
    fn a_staged_copy_that_fails_part_way_leaves_nothing_under_the_name() {
        let dir = tempfile::tempdir().unwrap();
        let to = derived_dir(dir.path()).join("abc.wav");
        let err = put_staged(&to, |part| {
            io::Write::write_all(part, b"the first half").unwrap();
            Err(io::Error::from(io::ErrorKind::StorageFull))
        })
        .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::StorageFull);
        assert!(!to.exists(), "a part under a trusted name");
        let left: Vec<_> = std::fs::read_dir(to.parent().unwrap())
            .unwrap()
            .flatten()
            .collect();
        assert!(left.is_empty(), "no staging file: {left:?}");
    }

    #[test]
    fn copy_into_puts_the_file_in_the_same_folder_of_the_other_project() {
        let dir = tempfile::tempdir().unwrap();
        let (src, dest) = (dir.path().join("src"), dir.path().join("dest"));
        for folder in AudioFolder::ALL {
            let path = file_at(&folder.dir(&src), "a.wav", b"audio");
            let owned = AudioStore::new(&src).owns(&path).unwrap();
            assert_eq!(owned.copy_into(&AudioStore::new(&dest)).unwrap(), 5);
            let copied = folder.dir(&dest).join("a.wav");
            assert_eq!(std::fs::read(&copied).unwrap(), b"audio");
            assert_eq!(
                std::fs::metadata(&copied).unwrap().permissions(),
                std::fs::metadata(&path).unwrap().permissions()
            );
            assert_eq!(std::fs::read_dir(folder.dir(&dest)).unwrap().count(), 1);
        }
    }

    #[test]
    fn folder_at_is_by_identity() {
        let dir = tempfile::tempdir().unwrap();
        let store = AudioStore::new(dir.path());
        for folder in AudioFolder::ALL {
            std::fs::create_dir_all(folder.dir(dir.path())).unwrap();
            assert_eq!(store.folder_at(&folder.dir(dir.path())), Some(folder));
        }
        let roundabout = derived_dir(dir.path()).join("..").join(DERIVED_DIR);
        assert_eq!(store.folder_at(&roundabout), Some(AudioFolder::Derived));
        assert_eq!(store.folder_at(dir.path()), None);
        assert_eq!(store.folder_at(&dir.path().join("gone")), None);
    }

    /// Something that is not a file under the name is an error naming it,
    /// not a put that "did not reproduce" the file.
    #[test]
    fn a_put_back_onto_a_folder_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let scratch = AudioStore::new(dir.path().join("scratch"));
        let name = AudioName("x".into());
        scratch
            .put_with(AudioFolder::Derived, &name, |tmp| {
                std::fs::write(tmp, b"rebuilt").map_err(|e| e.to_string())
            })
            .unwrap();
        let rebuilt = scratch.rebuilt(&[]);
        let real = AudioStore::new(dir.path().join("real"));
        let blocking = derived_dir(&dir.path().join("real")).join("x.wav");
        std::fs::create_dir_all(&blocking).unwrap();
        let err = rebuilt.put_back(OsStr::new("x.wav"), &real).unwrap_err();
        assert!(err.contains(&blocking.display().to_string()), "{err}");
        assert!(blocking.is_dir());
    }

    #[test]
    fn folder_of_needs_no_file() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project");
        let store = AudioStore::new(&project);
        // Before any folder is made: by spelling.
        for folder in AudioFolder::ALL {
            assert_eq!(
                store.folder_of(&folder.dir(&project).join("gone.wav")),
                Some(folder)
            );
        }
        assert_eq!(store.folder_of(&project.join("take.wav")), None);
        // Once made: by identity, so another spelling is the same folder,
        // and a path that only reads as one is not.
        std::fs::create_dir_all(derived_dir(&project)).unwrap();
        let roundabout = derived_dir(&project)
            .join("..")
            .join(DERIVED_DIR)
            .join("gone.wav");
        assert_eq!(store.folder_of(&roundabout), Some(AudioFolder::Derived));
        let other = derived_dir(&dir.path().join("other")).join("gone.wav");
        std::fs::create_dir_all(other.parent().unwrap()).unwrap();
        assert_eq!(store.folder_of(&other), None, "another project's");
        let nested = derived_dir(&project).join("sub").join("gone.wav");
        assert_eq!(store.folder_of(&nested), None);
        assert_eq!(store.folder_of(&derived_dir(&project).join("..")), None);
    }

    #[test]
    fn an_owned_file_says_where_it_is_in_its_project() {
        let dir = tempfile::tempdir().unwrap();
        let store = AudioStore::new(dir.path());
        for folder in AudioFolder::ALL {
            let path = file_at(&folder.dir(dir.path()), "a.wav", b"x");
            let rel = store.owns(&path).unwrap().in_project();
            assert_eq!(rel, folder.dir(Path::new("")).join("a.wav"));
            assert_eq!(dir.path().join(&rel), path);
        }
    }

    /// A clip whose `content_hash` is the blake3 of the samples alone.
    fn clip_of(path: PathBuf, hash: Option<[u8; 32]>) -> Clip {
        Clip {
            source_path: path,
            start_in_track: 0,
            source_offset: 0,
            length: 1,
            content_hash: hash,
            time_stretch_factor: None,
            pitch_shift_semitones: None,
            beat_grid: None,
            volume_envelope: Vec::new(),
            volume_envelope_frames: EnvelopeFrames::default(),
        }
    }

    fn node_with(clips: Vec<Clip>) -> SessionNode {
        use crate::state::{BusGraph, SessionState, TempoMap, Track, TrackId};
        let state = SessionState {
            tracks: vec![Track {
                id: TrackId(uuid::Uuid::nil()),
                name: "Vox".into(),
                clips,
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
            length_samples: 1,
            annotations: Vec::new(),
            sync_lock: false,
        };
        SessionNode {
            id: crate::NodeId([0; 32]),
            parent: None,
            created_at: chrono::Utc::now(),
            label: None,
            reasoning: None,
            state,
            op: None,
        }
    }

    /// The names a replay's file answers to (#620): its own, and the name
    /// its samples had before #620 when a clip of the scratch project names
    /// it with that `content_hash`; a clip of another project's audio (what
    /// the step read) teaches it nothing.
    #[test]
    fn a_rebuilt_file_answers_to_its_name_and_the_older_one() {
        let dir = tempfile::tempdir().unwrap();
        let scratch = dir.path().join("scratch");
        let s = samples();
        let name = AudioName::of_samples(&s, 48_000, 1);
        let store = AudioStore::new(&scratch);
        let written = store
            .put_with(AudioFolder::Derived, &name, |tmp| {
                std::fs::write(tmp, b"rebuilt").map_err(|e| e.to_string())
            })
            .unwrap();
        // The step read a file of the real project, which has a hash too.
        let read = file_at(&derived_dir(&dir.path().join("real")), "read.wav", b"r");
        let nodes = [node_with(vec![
            clip_of(written.clone(), Some(legacy_sample_hash(&s))),
            clip_of(read, Some([7; 32])),
        ])];
        let rebuilt = store.rebuilt(&nodes);

        let own = OsString::from(format!("{MONO_48K}.wav"));
        let older = OsString::from(format!("{LEGACY}.wav"));
        assert_eq!(rebuilt.produced().iter().collect::<Vec<_>>(), [&own]);
        let mut names: Vec<_> = rebuilt.names().cloned().collect();
        names.sort();
        let mut want = vec![own.clone(), older.clone()];
        want.sort();
        assert_eq!(names, want, "the step's input is not among them");

        // Put back under each recorded name, into the real project.
        let real = AudioStore::new(dir.path().join("real"));
        for recorded in [&own, &older] {
            let put = rebuilt.put_back(recorded, &real).unwrap().expect("answers");
            assert_eq!(put, derived_dir(&dir.path().join("real")).join(recorded));
            assert_eq!(std::fs::read(&put).unwrap(), b"rebuilt");
        }
        assert_eq!(
            rebuilt.put_back(OsStr::new("other.wav"), &real).unwrap(),
            None,
            "a name nothing answers to"
        );
        // A file already there is kept.
        let kept = derived_dir(&dir.path().join("real")).join(&own);
        std::fs::write(&kept, b"there first").unwrap();
        rebuilt.put_back(&own, &real).unwrap();
        assert_eq!(std::fs::read(&kept).unwrap(), b"there first");
        let left: Vec<_> = std::fs::read_dir(kept.parent().unwrap())
            .unwrap()
            .flatten()
            .map(|e| e.file_name())
            .filter(|n| n.to_string_lossy().ends_with(".part"))
            .collect();
        assert!(left.is_empty(), "no staging file left: {left:?}");
    }

    /// A clip with no `content_hash`, or one equal to its file's own name,
    /// adds no older name.
    #[test]
    fn a_rebuilt_file_with_no_older_name_answers_to_its_own() {
        let dir = tempfile::tempdir().unwrap();
        let store = AudioStore::new(dir.path());
        let a = file_at(&derived_dir(dir.path()), "a.wav", b"a");
        let b = file_at(&derived_dir(dir.path()), &format!("{LEGACY}.wav"), b"b");
        let hash = *blake3::Hash::from_hex(LEGACY).unwrap().as_bytes();
        let rebuilt = store.rebuilt(&[node_with(vec![clip_of(a, None), clip_of(b, Some(hash))])]);
        assert_eq!(rebuilt.names().count(), 2);
        assert_eq!(rebuilt.produced().len(), 2);
    }

    #[test]
    fn the_deleter_removes_only_while_nothing_is_staged() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let path = file_at(&derived_dir(dir.path()), "a.wav", b"x");
        let file = store.audio_store().owns(&path).unwrap();

        let staged = store.stage().unwrap();
        assert!(store.audio_deleter().is_none(), "refused while staged");
        assert!(staged.audio_deleter().is_none(), "the staged handle too");
        drop(staged);

        let deleter = store.audio_deleter().expect("nothing staged");
        // A stage begun after the leave was given is seen at the delete.
        let staged = store.stage().unwrap();
        assert!(!deleter.remove(&file));
        assert!(path.is_file(), "kept while staged");
        drop(staged);
        assert!(deleter.remove(&file));
        assert!(!path.exists());
        assert!(!deleter.remove(&file), "already gone");
    }

    #[test]
    fn a_store_that_only_reads_gets_no_deleter() {
        let dir = tempfile::tempdir().unwrap();
        let live = Store::open(dir.path()).unwrap();
        let reader = Store::open_read_only(dir.path()).unwrap();
        assert!(reader.is_read_only());
        assert!(reader.audio_deleter().is_none(), "a reader deletes nothing");
        assert!(live.audio_deleter().is_some(), "the writer may");
    }

    #[test]
    fn the_deleter_removes_only_its_own_projects_files() {
        let dir = tempfile::tempdir().unwrap();
        let a = Store::open(&dir.path().join("a")).unwrap();
        let b = Store::open(&dir.path().join("b")).unwrap();
        let path = file_at(&derived_dir(&dir.path().join("b")), "x.wav", b"x");
        let bs = b.audio_store().owns(&path).unwrap();
        assert!(
            !a.audio_deleter().unwrap().remove(&bs),
            "b's file, a's leave"
        );
        assert!(path.is_file());
        assert!(b.audio_deleter().unwrap().remove(&bs));
    }
}
