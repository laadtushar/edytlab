//! Stem-cache layout helpers.
//!
//! `<project>/.audiograph/stem-cache/<hex>/{vocals,drums,bass,other}.wav`
//!
//! Why a directory rather than a single archive: users routinely want
//! to drag one stem out and play it. A flat `.wav`-per-stem layout is
//! also trivially inspectable from a file manager, which matters for
//! demos. The trade-off (4 fs entries per cache hit instead of 1) is
//! noise.
//!
//! The hex segment matches the inference cache hex used by
//! `ml_pipeline::ContentHash` so a project's stem cache and inference
//! cache can sit side-by-side under `.audiograph/` without naming
//! collisions.

use std::path::{Path, PathBuf};

use hound::{SampleFormat, WavSpec, WavWriter};

use crate::{DemucsError, Result, StemBuffer};

/// The four stem names Demucs emits, in canonical order.
pub const STEM_NAMES: [&str; 4] = ["vocals", "drums", "bass", "other"];

/// Filenames under each `<hex>/` cache directory, in the same order
/// as [`STEM_NAMES`]. Lives alongside [`STEM_NAMES`] so the two can't
/// drift.
pub const STEM_FILENAMES: [&str; 4] = ["vocals.wav", "drums.wav", "bass.wav", "other.wav"];

/// Paths to a four-stem set on disk.
///
/// Always references the canonical filenames inside a single
/// `<hex>/` cache directory — see [`StemPaths::for_dir`]. The fields
/// are public so the tool layer can serialize them straight into its
/// `ToolResult::Ok` JSON.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StemPaths {
    pub vocals: PathBuf,
    pub drums: PathBuf,
    pub bass: PathBuf,
    pub other: PathBuf,
}

impl StemPaths {
    /// Build a [`StemPaths`] from the canonical `<hex>/` cache dir.
    /// Does not check existence — pair with [`Self::all_exist`] for
    /// cache-hit detection.
    pub fn for_dir(dir: &Path) -> Self {
        Self {
            vocals: dir.join("vocals.wav"),
            drums: dir.join("drums.wav"),
            bass: dir.join("bass.wav"),
            other: dir.join("other.wav"),
        }
    }

    /// Iterate over the four paths in canonical [`STEM_NAMES`] order.
    pub fn iter(&self) -> impl Iterator<Item = &Path> {
        [
            self.vocals.as_path(),
            self.drums.as_path(),
            self.bass.as_path(),
            self.other.as_path(),
        ]
        .into_iter()
    }

    /// True if all four stem files are present on disk. Used as the
    /// cache-hit gate.
    pub fn all_exist(&self) -> bool {
        self.iter().all(|p| p.exists())
    }
}

/// Resolve the cache directory for a given content-hash hex. Creates
/// neither the parent nor the directory itself — call sites do that
/// when they're about to write.
pub fn stem_dir_for(stem_cache_root: &Path, key_hex: &str) -> PathBuf {
    stem_cache_root.join(key_hex)
}

/// Write four stem buffers to `<dir>/{name}.wav`, returning the
/// [`StemPaths`].
///
/// The stems must be in the canonical order ([`STEM_NAMES`]). Errors
/// from the WAV writer surface as [`DemucsError::Wav`]; I/O failures
/// propagate as [`DemucsError::Io`].
///
/// Atomicity: writes each stem to a `*.wav.tmp` then renames into
/// place. A crashed writer leaves at most one stale `.tmp` file
/// behind; the cache-hit gate ([`StemPaths::all_exist`]) only sees
/// real files, so a partial write doesn't poison subsequent reads.
pub fn write_stems_to_dir(dir: &Path, stems: &[StemBuffer; 4]) -> Result<StemPaths> {
    std::fs::create_dir_all(dir)?;

    // Validate the canonical ordering up front so a misordered slice
    // produces a clear error rather than a corrupted stem directory.
    for (i, stem) in stems.iter().enumerate() {
        if stem.name != STEM_NAMES[i] {
            return Err(DemucsError::Wav(format!(
                "stem {i} has name {:?}, expected {:?}",
                stem.name, STEM_NAMES[i]
            )));
        }
    }

    let paths = StemPaths::for_dir(dir);
    let final_paths = [&paths.vocals, &paths.drums, &paths.bass, &paths.other];
    for (stem, final_path) in stems.iter().zip(final_paths.iter()) {
        write_stem_to_wav(final_path, stem)?;
    }
    Ok(paths)
}

/// Write a single stem to `path` atomically.
fn write_stem_to_wav(path: &Path, stem: &StemBuffer) -> Result<()> {
    if stem.sample_rate == 0 || stem.channels == 0 {
        return Err(DemucsError::Wav(format!(
            "stem {:?}: invalid header sample_rate={} channels={}",
            stem.name, stem.sample_rate, stem.channels
        )));
    }
    let frames = stem.channels as usize;
    if !stem.samples.len().is_multiple_of(frames) {
        return Err(DemucsError::Wav(format!(
            "stem {:?}: interleaved buffer length {} not divisible by channels {}",
            stem.name,
            stem.samples.len(),
            frames
        )));
    }

    let tmp_path = path.with_extension("wav.tmp");
    {
        let spec = WavSpec {
            channels: stem.channels,
            sample_rate: stem.sample_rate,
            bits_per_sample: 32,
            sample_format: SampleFormat::Float,
        };
        let mut writer =
            WavWriter::create(&tmp_path, spec).map_err(|e| DemucsError::Wav(e.to_string()))?;
        for s in &stem.samples {
            writer
                .write_sample(*s)
                .map_err(|e| DemucsError::Wav(e.to_string()))?;
        }
        writer
            .finalize()
            .map_err(|e| DemucsError::Wav(e.to_string()))?;
    }
    std::fs::rename(&tmp_path, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buf(name: &'static str) -> StemBuffer {
        StemBuffer {
            name,
            samples: vec![0.0, 0.0, 0.1, -0.1],
            sample_rate: 44_100,
            channels: 2,
        }
    }

    #[test]
    fn for_dir_uses_canonical_filenames() {
        let dir = Path::new("/tmp/example/aabb");
        let p = StemPaths::for_dir(dir);
        assert_eq!(p.vocals, dir.join("vocals.wav"));
        assert_eq!(p.drums, dir.join("drums.wav"));
        assert_eq!(p.bass, dir.join("bass.wav"));
        assert_eq!(p.other, dir.join("other.wav"));
    }

    #[test]
    fn all_exist_false_when_dir_missing() {
        let dir = Path::new("/tmp/definitely-not-a-real-stem-cache-dir-xyzzy");
        assert!(!StemPaths::for_dir(dir).all_exist());
    }

    #[test]
    fn write_stems_round_trips() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("hex");
        let stems = [buf("vocals"), buf("drums"), buf("bass"), buf("other")];
        let paths = write_stems_to_dir(&dir, &stems).unwrap();
        assert!(paths.all_exist());
        // No stale tempfiles.
        for entry in std::fs::read_dir(&dir).unwrap() {
            let name = entry.unwrap().file_name().into_string().unwrap();
            assert!(
                STEM_FILENAMES.contains(&name.as_str()),
                "unexpected file in stem cache dir: {name}"
            );
        }
    }

    #[test]
    fn write_stems_rejects_misordered_input() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("hex");
        let stems = [buf("drums"), buf("vocals"), buf("bass"), buf("other")];
        let err = write_stems_to_dir(&dir, &stems).unwrap_err();
        assert!(matches!(err, DemucsError::Wav(_)));
    }

    #[test]
    fn stem_dir_for_joins_hex() {
        let root = Path::new("/proj/.audiograph/stem-cache");
        let dir = stem_dir_for(root, "deadbeef");
        assert_eq!(dir, root.join("deadbeef"));
    }
}
