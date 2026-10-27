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
//!
//! ## An entry exists whole or not at all
//!
//! A separation takes minutes, and the stems are written as it goes. They are
//! written into a sibling directory, `<hex>.tmp-<pid>-<n>` ([`Pending`]), and
//! that directory is renamed to `<hex>` only when all four files are complete.
//! A run that is cancelled, fails, or is killed leaves no `<hex>`, so a later
//! call can never find three stems and a half and take it for a hit; the
//! leftover of a killed run is a `.tmp-` directory nothing reads, which
//! [`prune`] removes once it is an hour old.
//!
//! ## A bounded cache
//!
//! Stems are large (about 40 MB a minute of stereo, four stems), and each
//! different input is a new entry. [`prune`] keeps the directory under
//! [`STEM_CACHE_CAP_BYTES`] by removing the entries used longest ago, never
//! the one it was asked to keep. This is what `load.rs` means by the
//! stem cache being a cache the store may delete from: loading a stem as a
//! track copies it into the project's own `derived/`, so removing it here
//! never breaks a clip.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use crate::Result;

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

/// How much stem audio a project's `stem-cache/` keeps: 1 GiB, about five
/// songs of five minutes (16-bit stereo, four stems each).
pub const STEM_CACHE_CAP_BYTES: u64 = 1 << 30;

/// A `.tmp-` directory this much older than now is the leftover of a run
/// that did not finish, and is removed by [`prune`].
const STALE_TEMP: Duration = Duration::from_secs(60 * 60);

/// The infix of a temporary directory's name.
const TEMP_INFIX: &str = ".tmp-";

/// A stem directory being written. Nothing is visible at the final name until
/// [`Pending::publish`]; dropping it without publishing removes what was
/// written.
#[derive(Debug)]
pub struct Pending {
    temp: PathBuf,
    dest: PathBuf,
    published: bool,
}

impl Pending {
    /// Start writing the entry that will be called `dest`. Creates the
    /// temporary directory (and `dest`'s parent) now.
    pub fn create(dest: &Path) -> Result<Self> {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let name = dest
            .file_name()
            .ok_or_else(|| std::io::Error::other("a stem directory needs a name"))?
            .to_string_lossy()
            .into_owned();
        let temp = dest.with_file_name(format!(
            "{name}{TEMP_INFIX}{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&temp)?;
        Ok(Self {
            temp,
            dest: dest.to_path_buf(),
            published: false,
        })
    }

    /// Where the four files are written.
    pub fn dir(&self) -> &Path {
        &self.temp
    }

    /// Make the entry visible, if all four files are there. If another run
    /// finished the same entry first, theirs stays and this one is discarded:
    /// the two are the same audio by construction (the name is the input's
    /// and the model's).
    pub fn publish(mut self) -> Result<StemPaths> {
        let written = StemPaths::for_dir(&self.temp);
        if !written.all_exist() {
            return Err(crate::DemucsError::Pipeline(
                "a stem directory was published before all four stems were written".into(),
            ));
        }
        let paths = StemPaths::for_dir(&self.dest);
        if paths.all_exist() {
            // Dropping removes ours.
            return Ok(paths);
        }
        if self.dest.exists() {
            // Not a whole entry: the leftover of something that is not this
            // code. It cannot be a hit, and it is in the way.
            std::fs::remove_dir_all(&self.dest)?;
        }
        std::fs::rename(&self.temp, &self.dest)?;
        self.published = true;
        Ok(paths)
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        if !self.published {
            let _ = std::fs::remove_dir_all(&self.temp);
        }
    }
}

/// Mark `dir` as used just now, so [`prune`] keeps it longer. Best effort.
pub fn touch(dir: &Path) {
    let now = SystemTime::now();
    for path in StemPaths::for_dir(dir).iter() {
        if let Ok(file) = std::fs::OpenOptions::new().write(true).open(path) {
            let _ = file.set_modified(now);
        }
    }
}

/// What [`prune`] removed.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Pruned {
    pub entries: usize,
    pub bytes: u64,
}

/// Total size of the files directly inside `dir`, and the latest time any of
/// them was modified.
fn measure(dir: &Path) -> (u64, SystemTime) {
    let mut bytes = 0;
    let mut latest = SystemTime::UNIX_EPOCH;
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            if let Ok(meta) = entry.metadata() {
                bytes += meta.len();
                if let Ok(modified) = meta.modified() {
                    latest = latest.max(modified);
                }
            }
        }
    }
    (bytes, latest)
}

/// Keep `root` under `cap` bytes: remove whole entries, the one last used
/// longest ago first, until it fits. `keep` (the entry just made, or just
/// read) is never removed, even when it alone is over the cap. Stale `.tmp-`
/// directories (see the module docs) go too. A missing `root` is nothing to
/// do.
pub fn prune(root: &Path, cap: u64, keep: &Path) -> Pruned {
    let mut removed = Pruned::default();
    let Ok(listing) = std::fs::read_dir(root) else {
        return removed;
    };
    let now = SystemTime::now();
    let mut entries: Vec<(PathBuf, u64, SystemTime)> = Vec::new();
    for entry in listing.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let (bytes, latest) = measure(&path);
        let is_temp = path
            .file_name()
            .is_some_and(|n| n.to_string_lossy().contains(TEMP_INFIX));
        if is_temp {
            let age = now.duration_since(latest).unwrap_or_default();
            // An empty one has no time: its own is the directory's.
            let age = if bytes == 0 {
                entry
                    .metadata()
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|m| now.duration_since(m).ok())
                    .unwrap_or(age)
            } else {
                age
            };
            if age >= STALE_TEMP && std::fs::remove_dir_all(&path).is_ok() {
                removed.entries += 1;
                removed.bytes += bytes;
            }
            continue;
        }
        entries.push((path, bytes, latest));
    }
    let mut total: u64 = entries.iter().map(|e| e.1).sum();
    entries.sort_by_key(|e| e.2);
    for (path, bytes, _) in entries {
        if total <= cap {
            break;
        }
        if path == keep {
            continue;
        }
        if std::fs::remove_dir_all(&path).is_ok() {
            total -= bytes;
            removed.entries += 1;
            removed.bytes += bytes;
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_entry(dir: &Path, bytes: usize) {
        std::fs::create_dir_all(dir).unwrap();
        for p in StemPaths::for_dir(dir).iter() {
            std::fs::write(p, vec![0u8; bytes]).unwrap();
        }
    }

    fn age(dir: &Path, by: Duration) {
        let then = SystemTime::now() - by;
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let file = std::fs::OpenOptions::new()
                .write(true)
                .open(entry.path())
                .unwrap();
            file.set_modified(then).unwrap();
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
    fn stem_dir_for_joins_hex() {
        let root = Path::new("/proj/.audiograph/stem-cache");
        let dir = stem_dir_for(root, "deadbeef");
        assert_eq!(dir, root.join("deadbeef"));
    }

    // -- an entry exists whole or not at all --------------------------------

    #[test]
    fn nothing_is_at_the_final_name_until_published() {
        let root = tempfile::tempdir().unwrap();
        let dest = root.path().join("abc123");
        let pending = Pending::create(&dest).unwrap();
        write_entry(pending.dir(), 10);
        assert!(!dest.exists(), "written, but not yet an entry");
        assert!(read_back(&dest).is_none());
        let paths = pending.publish().unwrap();
        assert_eq!(paths, StemPaths::for_dir(&dest));
        assert!(paths.all_exist());
        // Only the entry is left: no temporary directory.
        let names: Vec<_> = std::fs::read_dir(root.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(names, ["abc123"]);
    }

    fn read_back(dir: &Path) -> Option<StemPaths> {
        crate::read_cached_stems(dir)
    }

    #[test]
    fn dropping_without_publishing_leaves_nothing() {
        let root = tempfile::tempdir().unwrap();
        let dest = root.path().join("abc123");
        {
            let pending = Pending::create(&dest).unwrap();
            write_entry(pending.dir(), 10);
        }
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    }

    #[test]
    fn three_stems_are_not_an_entry() {
        let root = tempfile::tempdir().unwrap();
        let dest = root.path().join("abc123");
        let pending = Pending::create(&dest).unwrap();
        write_entry(pending.dir(), 10);
        std::fs::remove_file(pending.dir().join("other.wav")).unwrap();
        let err = pending.publish().unwrap_err();
        assert!(matches!(err, crate::DemucsError::Pipeline(_)), "{err:?}");
        assert!(!dest.exists());
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    }

    #[test]
    fn an_entry_made_meanwhile_by_another_run_is_kept() {
        let root = tempfile::tempdir().unwrap();
        let dest = root.path().join("abc123");
        let pending = Pending::create(&dest).unwrap();
        write_entry(pending.dir(), 10);
        write_entry(&dest, 99);
        let paths = pending.publish().unwrap();
        assert_eq!(std::fs::metadata(&paths.vocals).unwrap().len(), 99);
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
    }

    #[test]
    fn something_in_the_way_that_is_not_an_entry_is_replaced() {
        let root = tempfile::tempdir().unwrap();
        let dest = root.path().join("abc123");
        std::fs::create_dir_all(&dest).unwrap();
        std::fs::write(dest.join("vocals.wav"), b"half").unwrap();
        let pending = Pending::create(&dest).unwrap();
        write_entry(pending.dir(), 10);
        let paths = pending.publish().unwrap();
        assert!(paths.all_exist());
        assert_eq!(std::fs::metadata(&paths.vocals).unwrap().len(), 10);
    }

    #[test]
    fn two_runs_on_one_entry_do_not_share_a_directory() {
        let root = tempfile::tempdir().unwrap();
        let dest = root.path().join("abc123");
        let a = Pending::create(&dest).unwrap();
        let b = Pending::create(&dest).unwrap();
        assert_ne!(a.dir(), b.dir());
    }

    // -- the cap ------------------------------------------------------------

    #[test]
    fn prune_removes_the_entries_used_longest_ago_first() {
        let root = tempfile::tempdir().unwrap();
        for (name, days) in [("old", 3), ("older", 5), ("new", 1)] {
            let dir = root.path().join(name);
            write_entry(&dir, 100);
            age(&dir, Duration::from_secs(days * 86_400));
        }
        // 3 entries of 400 bytes; room for two.
        let removed = prune(root.path(), 900, &root.path().join("new"));
        assert_eq!(
            removed,
            Pruned {
                entries: 1,
                bytes: 400
            }
        );
        assert!(!root.path().join("older").exists());
        assert!(root.path().join("old").exists() && root.path().join("new").exists());
    }

    #[test]
    fn prune_never_removes_the_entry_it_was_told_to_keep() {
        let root = tempfile::tempdir().unwrap();
        let keep = root.path().join("keep");
        write_entry(&keep, 1000);
        age(&keep, Duration::from_secs(9 * 86_400));
        let other = root.path().join("other");
        write_entry(&other, 10);
        // Over the cap on its own, and the oldest: it stays.
        prune(root.path(), 100, &keep);
        assert!(keep.exists());
        assert!(!other.exists());
    }

    #[test]
    fn touching_an_entry_makes_it_the_newest() {
        let root = tempfile::tempdir().unwrap();
        for name in ["a", "b"] {
            let dir = root.path().join(name);
            write_entry(&dir, 100);
            age(&dir, Duration::from_secs(5 * 86_400));
        }
        touch(&root.path().join("a"));
        prune(root.path(), 400, &root.path().join("none"));
        assert!(root.path().join("a").exists());
        assert!(!root.path().join("b").exists());
    }

    #[test]
    fn prune_removes_stale_temporary_directories_and_spares_fresh_ones() {
        let root = tempfile::tempdir().unwrap();
        let stale = root.path().join("abc.tmp-1-0");
        let fresh = root.path().join("def.tmp-2-0");
        for dir in [&stale, &fresh] {
            std::fs::create_dir_all(dir).unwrap();
            std::fs::write(dir.join("vocals.wav"), b"partial").unwrap();
        }
        age(&stale, Duration::from_secs(2 * 3600));
        let removed = prune(root.path(), u64::MAX, &root.path().join("none"));
        assert_eq!(removed.entries, 1);
        assert!(!stale.exists() && fresh.exists());
    }

    #[test]
    fn prune_of_a_folder_that_is_not_there_is_nothing() {
        let removed = prune(Path::new("/definitely/not/here"), 0, Path::new("/x"));
        assert_eq!(removed, Pruned::default());
    }
}
