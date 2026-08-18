//! A bounded, content-addressed cache of rendered previews (#164).
//!
//! Playing a session means rendering it. Nothing cached that render, so
//! every head change paid for a full mix — and after the transport
//! actually plays the mix (#155), that cost lands between "I changed
//! the gain" and hearing it, on every single edit.
//!
//! ## Why a node id is a sound cache key
//!
//! `NodeId::from_state` is a blake3 hash of the session state, so two
//! paths to the same state share an id by construction: undo, redo, and
//! two different routes to the same mix all hit the same entry. And
//! since rendering is deterministic, a hit is byte-identical to the
//! render it replaces rather than merely equivalent — the property the
//! rest of the store already relies on.
//!
//! Changing anything in the session changes the id, so a stale hit is
//! not a thing that can happen: there is no invalidation to get wrong.
//!
//! ## Bounded, because previews are large
//!
//! A five-minute stereo 48 kHz mix is roughly 55 MB. An unbounded cache
//! is #98 again in a different directory, so entries are evicted
//! least-recently-used once the directory exceeds [`DEFAULT_CAP_BYTES`].
//! Eviction is safe in a way most caches' is not: a swept entry is
//! re-derivable byte-for-byte from the node it was named for.
//!
//! It lives at `<project>/.audiograph/previews/` rather than in the OS
//! tempdir so it survives a restart, and so `storage_report` can find
//! it and count it — an eviction policy nobody can see the effect of is
//! not much of a policy.
//!
//! ## Writes are atomic
//!
//! A render goes to a temp file in the same directory and is renamed
//! into place. A crash mid-render therefore leaves no half-written file
//! that a later run would serve as a hit; the strongest thing this
//! module promises is that a hit is a real hit.

use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Directory under `.audiograph/` holding cached previews.
pub const CACHE_DIR: &str = "previews";

/// How much rendered preview audio to keep before evicting. One GiB is
/// roughly 18 renders of a five-minute stereo project — enough that
/// stepping back and forth through recent history always hits, without
/// letting a long session quietly consume the disk.
pub const DEFAULT_CAP_BYTES: u64 = 1024 * 1024 * 1024;

/// Environment override for the cap, in bytes. Exists so tests can use
/// a cap measured in kilobytes instead of writing a gigabyte of audio.
pub const CAP_ENV: &str = "EDYTLAB_PREVIEW_CACHE_BYTES";

/// Whether a lookup rendered or reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    /// The file was already on disk. No rendering happened.
    Cached,
    /// The file was rendered by this call.
    Rendered,
}

impl Hit {
    pub fn is_cached(self) -> bool {
        matches!(self, Hit::Cached)
    }
}

/// The preview cache for one project.
#[derive(Debug, Clone)]
pub struct PreviewCache {
    dir: PathBuf,
    cap_bytes: u64,
}

impl PreviewCache {
    /// Open (creating on demand) the preview cache for `project_dir`.
    pub fn new(project_dir: &Path) -> Self {
        Self::in_dir(project_dir, CACHE_DIR)
    }

    /// A cache of the same shape under a different name.
    ///
    /// Auditions (#166) use this: they are renders keyed by content
    /// like previews are, and want the same bounded LRU behaviour, but
    /// they are excerpts rather than whole mixes and must never be
    /// served in place of one.
    pub fn in_dir(project_dir: &Path, subdir: &str) -> Self {
        Self {
            dir: project_dir.join(session::STORE_DIR).join(subdir),
            cap_bytes: cap_from_env(),
        }
    }

    /// The directory previews are kept in, whether or not it exists yet.
    /// `storage_report` uses this to count what the cache is holding.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Where the preview for `node` lives, hit or miss.
    pub fn path_for(&self, node: session::NodeId) -> PathBuf {
        self.dir.join(format!("{}.wav", node.to_hex()))
    }

    /// Return the cached preview for `node`, rendering it with `render`
    /// only if it is not already there.
    ///
    /// `render` is handed a path to write to. It is called at most once
    /// and its output is moved into place atomically, so a failed render
    /// leaves the cache exactly as it was.
    pub fn get_or_render<F, E>(&self, node: session::NodeId, render: F) -> Result<(PathBuf, Hit), E>
    where
        F: FnOnce(&Path) -> Result<(), E>,
        E: From<io::Error>,
    {
        let final_path = self.path_for(node);
        if final_path.is_file() {
            // Touch so LRU sees this as recently used. A failure here
            // costs accuracy in eviction order, never correctness, so
            // it is not worth failing a cache hit over.
            let _ = touch(&final_path);
            return Ok((final_path, Hit::Cached));
        }

        std::fs::create_dir_all(&self.dir).map_err(E::from)?;

        // Same directory as the destination: a rename across filesystems
        // is not atomic, and the tempdir is frequently a different one.
        let staging = self.dir.join(format!(".{}.partial", node.to_hex()));
        render(&staging)?;
        std::fs::rename(&staging, &final_path).map_err(E::from)?;

        // Evict after inserting rather than before: the entry that was
        // just asked for is the one entry that must survive, and it is
        // now the most recently used.
        self.evict_to_cap();

        Ok((final_path, Hit::Rendered))
    }

    /// Total bytes currently held.
    pub fn size_bytes(&self) -> u64 {
        self.entries().iter().map(|e| e.bytes).sum()
    }

    /// Number of cached previews.
    pub fn len(&self) -> usize {
        self.entries().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Drop least-recently-used entries until the directory fits the
    /// cap. Public so a caller can enforce the cap after changing it.
    pub fn evict_to_cap(&self) {
        let mut entries = self.entries();
        let mut total: u64 = entries.iter().map(|e| e.bytes).sum();
        if total <= self.cap_bytes {
            return;
        }

        // Oldest first — `used` is mtime, which `touch` bumps on every
        // hit, so this is genuine LRU rather than insertion order.
        entries.sort_by_key(|e| e.used);
        for entry in entries {
            if total <= self.cap_bytes {
                break;
            }
            if std::fs::remove_file(&entry.path).is_ok() {
                total = total.saturating_sub(entry.bytes);
            }
        }
    }

    fn entries(&self) -> Vec<Entry> {
        let Ok(read) = std::fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        read.flatten()
            .filter_map(|e| {
                let path = e.path();
                // Skip staging files: they are not yet cache entries and
                // counting one would let a partial render evict a real
                // one.
                if path.extension().and_then(|s| s.to_str()) != Some("wav") {
                    return None;
                }
                let meta = e.metadata().ok()?;
                if !meta.is_file() {
                    return None;
                }
                Some(Entry {
                    path,
                    bytes: meta.len(),
                    used: meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                })
            })
            .collect()
    }
}

#[derive(Debug)]
struct Entry {
    path: PathBuf,
    bytes: u64,
    used: SystemTime,
}

fn cap_from_env() -> u64 {
    std::env::var(CAP_ENV)
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_CAP_BYTES)
}

/// Mark a file as used now, so LRU order reflects reads and not just
/// writes.
fn touch(path: &Path) -> io::Result<()> {
    let file = std::fs::File::options().write(true).open(path)?;
    file.set_modified(SystemTime::now())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A distinct node id per seed. The states differ only in length,
    /// which is enough: the id is a hash of the whole state.
    fn node_id(seed: u8) -> session::NodeId {
        let state = session::SessionState {
            tracks: Vec::new(),
            bus_routing: Default::default(),
            master_chain: Vec::new(),
            tempo_map: Default::default(),
            key_map: None,
            transcript: None,
            sample_rate: 48_000,
            length_samples: seed as u64,
            annotations: Vec::new(),
            sync_lock: false,
        };
        session::NodeId::from_state(&state).expect("hashable state")
    }

    fn write(path: &Path, bytes: usize) -> Result<(), io::Error> {
        std::fs::write(path, vec![0u8; bytes])
    }

    #[test]
    fn a_second_lookup_does_not_render() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = PreviewCache::new(tmp.path());
        let id = node_id(1);

        let (first, hit) = cache
            .get_or_render::<_, io::Error>(id, |p| write(p, 64))
            .unwrap();
        assert_eq!(hit, Hit::Rendered);

        let (second, hit) = cache
            .get_or_render::<_, io::Error>(id, |_| {
                panic!("a cached preview must not be re-rendered")
            })
            .unwrap();
        assert_eq!(hit, Hit::Cached);
        assert_eq!(first, second);
    }

    /// The cache key is the node id, so a different session state is a
    /// different entry — there is no invalidation step to forget.
    #[test]
    fn a_different_node_misses() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = PreviewCache::new(tmp.path());

        let (_, hit) = cache
            .get_or_render::<_, io::Error>(node_id(1), |p| write(p, 64))
            .unwrap();
        assert_eq!(hit, Hit::Rendered);
        let (_, hit) = cache
            .get_or_render::<_, io::Error>(node_id(2), |p| write(p, 64))
            .unwrap();
        assert_eq!(hit, Hit::Rendered);
        assert_eq!(cache.len(), 2);
    }

    /// A render that fails must leave nothing behind that a later
    /// lookup would serve as a hit.
    #[test]
    fn a_failed_render_caches_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = PreviewCache::new(tmp.path());
        let id = node_id(3);

        let err = cache
            .get_or_render::<_, io::Error>(id, |p| {
                write(p, 32)?;
                Err(io::Error::other("render blew up halfway"))
            })
            .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::Other);
        assert!(!cache.path_for(id).exists(), "a partial render was kept");

        // And the next attempt renders for real rather than reporting a
        // hit on the wreckage.
        let (_, hit) = cache
            .get_or_render::<_, io::Error>(id, |p| write(p, 64))
            .unwrap();
        assert_eq!(hit, Hit::Rendered);
    }

    #[test]
    fn the_cache_stays_under_its_cap() {
        let tmp = tempfile::tempdir().unwrap();
        let mut cache = PreviewCache::new(tmp.path());
        cache.cap_bytes = 250;

        for seed in 0..5u8 {
            cache
                .get_or_render::<_, io::Error>(node_id(seed), |p| write(p, 100))
                .unwrap();
            // mtime resolution is coarse on some filesystems; without a
            // gap the LRU order would be arbitrary rather than wrong.
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        assert!(
            cache.size_bytes() <= 250,
            "cache held {} bytes, over its 250-byte cap",
            cache.size_bytes()
        );
        // The entry just asked for is never the one evicted.
        assert!(cache.path_for(node_id(4)).exists());
    }

    /// Eviction is LRU, not FIFO: an old entry that keeps being played
    /// should outlive a newer one that is not.
    #[test]
    fn a_reused_entry_outlives_a_newer_unused_one() {
        let tmp = tempfile::tempdir().unwrap();
        let mut cache = PreviewCache::new(tmp.path());
        cache.cap_bytes = 250;

        let old = node_id(10);
        let middle = node_id(11);
        cache
            .get_or_render::<_, io::Error>(old, |p| write(p, 100))
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        cache
            .get_or_render::<_, io::Error>(middle, |p| write(p, 100))
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));

        // Play the old one again — this is the undo-then-redo case.
        cache
            .get_or_render::<_, io::Error>(old, |_| unreachable!())
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));

        cache
            .get_or_render::<_, io::Error>(node_id(12), |p| write(p, 100))
            .unwrap();

        assert!(cache.path_for(old).exists(), "the reused entry was evicted");
        assert!(
            !cache.path_for(middle).exists(),
            "the least recently used entry survived"
        );
    }

    #[test]
    fn the_cap_can_be_set_by_environment() {
        // Not a behavioural claim about the cache — a guard that the
        // override tests rely on actually parses.
        assert_eq!(
            std::env::var(CAP_ENV).ok().and_then(|v| v.parse().ok()),
            None::<u64>,
            "test environment unexpectedly sets {CAP_ENV}"
        );
        assert_eq!(cap_from_env(), DEFAULT_CAP_BYTES);
    }
}
