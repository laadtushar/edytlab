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
//! Stage 1 moves the definitions here and changes nothing on disk: every
//! name and folder is what it was, and the callers that wrote a file with
//! their own copy of the temp-and-rename now go through
//! [`AudioStore::put_with`]. The sweeps, compaction, repair, `rederive`, the
//! generators, `mix_to_new_track`, lane copies and clipboard blobs are
//! untouched.

use std::path::{Path, PathBuf};

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
/// (`<hash>.wav`), and the only function to name one with: a writer that
/// hashed on its own could leave the rate or channel count out again
/// (#620), and `crates/tools/tests/audio_names_cover_layout.rs` fails a tool source that
/// calls blake3 itself.
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

    /// Put the file `fill` writes under the name `stem` in `folder`, whole
    /// or not at all, unless a file is already there. Returns the file's
    /// path.
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
        stem: &str,
        fill: impl FnOnce(&Path) -> Result<(), String>,
    ) -> Result<PathBuf, String> {
        let path = self.path(folder, stem);
        if path.exists() {
            return Ok(path);
        }
        let dir = folder.dir(&self.project_dir);
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("failed to create {}: {e}", dir.display()))?;
        let tmp = tempfile::Builder::new()
            .prefix(".cas-")
            .suffix(".wav.part")
            .tempfile_in(&dir)
            .map_err(|e| format!("failed to create a file in {}: {e}", dir.display()))?
            .into_temp_path();
        fill(&tmp)?;
        match tmp.persist(&path) {
            Ok(()) => Ok(path),
            // Another writer of the same name got there first.
            Err(_) if path.is_file() => Ok(path),
            Err(e) => Err(e.to_string()),
        }
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
            .put_with(AudioFolder::Derived, "x", |_| {
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
            .put_with(AudioFolder::Derived, "x", |tmp| {
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
            .put_with(AudioFolder::Derived, "x", |tmp| {
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
            .put_with(AudioFolder::Generated, "x", |tmp| {
                std::fs::write(tmp, b"audio").map_err(|e| e.to_string())
            })
            .unwrap();
        assert_eq!(got, dir.path().join("generated").join("x.wav"));
        assert_eq!(std::fs::read(&got).unwrap(), b"audio");
    }
}
