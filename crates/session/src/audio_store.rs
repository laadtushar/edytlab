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
//! against its name, [`AudioStore::put_verified`]). Names and folders are
//! still what they were. The sweeps, compaction, repair and `rederive` are
//! stage 3.

use std::path::{Path, PathBuf};

use crate::state::Clip;
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
    /// would not.
    pub fn of_file_bytes(bytes: &[u8]) -> Self {
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
    /// Whether it exists is not asked.
    pub fn path(&self, folder: AudioFolder, stem: &str) -> PathBuf {
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
            Err(e) => Err(format!("failed to write {}: {}", path.display(), e.error)),
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
            .map_err(|e| format!("failed to write {}: {}", path.display(), e.error))?;
        Ok(path)
    }

    /// Put a file whose name is known only once it is written: `fill` writes a
    /// temporary file in `folder`, `name` reads it and says what it is called
    /// (a mixed-down track is named by its bytes, header included), and it is
    /// renamed into place, unless a file is already under that name, which
    /// is then kept (the name is the content's, so it holds the same thing).
    /// Nothing is left under any name if either step fails.
    pub fn put_then_name(
        &self,
        folder: AudioFolder,
        fill: impl FnOnce(&Path) -> Result<(), String>,
        name: impl FnOnce(&Path) -> Result<AudioName, String>,
    ) -> Result<(AudioName, PathBuf), String> {
        let tmp = self.temp_in(folder)?;
        fill(&tmp)?;
        let name = name(&tmp)?;
        let path = self.path_of(folder, &name);
        if path.is_file() {
            return Ok((name, path));
        }
        match tmp.persist(&path) {
            Ok(()) => Ok((name, path)),
            Err(_) if path.is_file() => Ok((name, path)),
            Err(e) => Err(format!("failed to write {}: {}", path.display(), e.error)),
        }
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
    fn put_then_name_names_the_file_after_it_is_written() {
        let dir = tempfile::tempdir().unwrap();
        let store = AudioStore::new(dir.path());
        let fill = |tmp: &Path| std::fs::write(tmp, b"mixdown").map_err(|e| e.to_string());
        let name_it = |tmp: &Path| Ok(AudioName::of_file_bytes(&std::fs::read(tmp).unwrap()));
        let (name, path) = store
            .put_then_name(AudioFolder::Generated, fill, name_it)
            .unwrap();
        assert_eq!(name, AudioName::of_file_bytes(b"mixdown"));
        assert_eq!(path, store.path_of(AudioFolder::Generated, &name));
        assert_eq!(std::fs::read(&path).unwrap(), b"mixdown");
        // The same content again: the file there is kept, nothing left over.
        let (again, _) = store
            .put_then_name(AudioFolder::Generated, fill, name_it)
            .unwrap();
        assert_eq!(again, name);
        let names: Vec<_> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .flatten()
            .map(|e| e.file_name())
            .collect();
        assert_eq!(names.len(), 1, "no .part left behind: {names:?}");
    }
}
