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
//! the leftovers `clipboard/` and `generated/`). Repair's copies, Save As and
//! `rederive`'s put-back are stage 3b.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

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

    /// Where the file named `name` lives in `folder`.
    pub fn path_of(&self, folder: AudioFolder, name: &AudioName) -> PathBuf {
        self.path(folder, name.as_str())
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
        let path = self.path_of(folder, name);
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
        let path = self.path_of(folder, name);
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
        let path = self.path_of(folder, &name);
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
    /// and the two are compared as the file system resolves them: `path`'s
    /// parent and each folder are canonicalised, so a path that only reads as
    /// one of them (`derived/../../take.wav`) is not in it, and one spelled
    /// another way (a link to the project, the copy of a project read through
    /// [`crate::relocate`]) is. The file itself is not followed: a link in
    /// `derived/` is not the store's, whether it points into the folder or
    /// out of it, and neither is a file in a folder below one of them. A
    /// node can name any file (`load` references the user's own recording in
    /// place), and this is what keeps a deleter from taking one (#589).
    ///
    /// The answer says which folder: a deleter that works in one (compaction
    /// in `derived/`) checks it.
    pub fn owns(&self, path: &Path) -> Option<OwnedFile> {
        let name = path.file_name()?;
        let parent = match path.parent() {
            Some(p) if !p.as_os_str().is_empty() => p,
            _ => Path::new("."),
        };
        let real_parent = std::fs::canonicalize(parent).ok()?;
        let folder = AudioFolder::ALL.into_iter().find(|f| {
            std::fs::canonicalize(f.dir(&self.project_dir)).is_ok_and(|dir| dir == real_parent)
        })?;
        owned_entry(folder, real_parent.join(name))
    }

    /// Every file the store may delete in `folder` ([`owns`](Self::owns)):
    /// the regular files directly in it, in no particular order. Empty when
    /// the folder is not there. A file that goes while it is listed is left
    /// out.
    pub fn files(&self, folder: AudioFolder) -> std::io::Result<Vec<OwnedFile>> {
        let real = match std::fs::canonicalize(folder.dir(&self.project_dir)) {
            Ok(dir) if dir.is_dir() => dir,
            Ok(_) => return Ok(Vec::new()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };
        let mut out = Vec::new();
        for entry in std::fs::read_dir(&real)? {
            if let Some(file) = owned_entry(folder, real.join(entry?.file_name())) {
                out.push(file);
            }
        }
        Ok(out)
    }

    /// A temporary file in `folder` (`.cas-*.wav.part`), making the folder if
    /// it is not there yet, removed on drop unless it is persisted.
    fn temp_in(&self, folder: AudioFolder) -> Result<tempfile::TempPath, String> {
        let dir = folder.dir(&self.project_dir);
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("failed to create {}: {e}", dir.display()))?;
        Ok(tempfile::Builder::new()
            .prefix(".cas-")
            .suffix(".wav.part")
            .tempfile_in(&dir)
            .map_err(|e| format!("failed to create a file in {}: {e}", dir.display()))?
            .into_temp_path())
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

    /// Where it is: the folder as the file system resolves it, and the
    /// file's name.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Its file name.
    pub fn name(&self) -> &OsStr {
        self.path.file_name().unwrap_or_default()
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
        if self.store.staged_in_flight() > 0 {
            return false;
        }
        match self.audio.owns(&file.path) {
            Some(now) if now.folder == file.folder => std::fs::remove_file(&now.path).is_ok(),
            _ => false,
        }
    }
}

impl Store {
    /// This project's audio store.
    pub fn audio_store(&self) -> AudioStore {
        AudioStore::new(self.project_dir())
    }

    /// Leave to delete this project's audio, or `None` while a handle on the
    /// project is staged ([`AudioDeleter`]). Every deleter of the project's
    /// audio starts here: the sweeps and compaction in `tools::reclaim`, and
    /// a repair's leftovers.
    pub fn audio_deleter(&self) -> Option<AudioDeleter<'_>> {
        (self.staged_in_flight() == 0).then(|| AudioDeleter {
            store: self,
            audio: self.audio_store(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            volume_envelope_frames: Default::default(),
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
        assert_eq!(path, store.path_of(AudioFolder::Generated, &name));
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
