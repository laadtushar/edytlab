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
//! ## Writes are atomic, and leave nothing else behind
//!
//! A render goes to a staging file in the same directory and is renamed
//! into place. A crash mid-render therefore leaves no half-written file
//! that a later run would serve as a hit; the strongest thing this
//! module promises is that a hit is a real hit.
//!
//! The staging file is the cache's to remove, and it does, whichever way
//! the render ends: a failed render, a failed rename and a panic all delete
//! it. One thing cannot: a process killed mid-render. That file is in this
//! directory but is not an entry, so the cap does not count it; the next
//! render that misses sweeps any staging file older than
//! [`STALE_STAGING`], which no render still in progress can be.
//!
//! ## Every preview is an entry
//!
//! A caller that wants a rendered file gets it from here or does not get
//! one. There is no "render somewhere else and hand back the path" for a
//! case the cache does not fit: a file outside this directory is outside
//! the cap, outside the project, and nobody's to clean up (#535). That is
//! what lets `render_preview` and `audition_effect` run under Plan first
//! without approval (`Tool::mutates`). An excerpt of a mix is an entry like
//! any other, under a key that names its range ([`excerpt_key`]), so it can
//! never be served as the mix.
//!
//! ## An entry being read is held
//!
//! Most callers read an entry once, straight after asking for it. The A/B
//! compare does not: the bar loads a side from its file on every switch, for
//! as long as it is open, and a side evicted meanwhile is a switch that
//! fails (#575). Such a caller takes a [`Held`] for the key
//! ([`PreviewCache::hold`]) before asking for it, and eviction leaves the
//! entry alone until the hold is dropped.
//!
//! The cap is on the entries nobody holds. A held entry is outside it, so a
//! render while one is held evicts only unheld entries to fit, and is
//! refused only when it is bigger than the cap on its own. A long session
//! (an hour-long mix) can therefore be compared although its two sides do
//! not fit in the cap together. The one holder is the A/B compare, which
//! holds one pair at a time, so the folder is bounded by the cap plus that
//! pair. When the last hold on an entry is dropped it counts again, and the
//! cache is brought back under the cap there and then.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, SystemTime};

/// Directory under `.audiograph/` holding cached previews.
pub const CACHE_DIR: &str = "previews";

/// How much rendered preview audio to keep before evicting. One GiB is
/// roughly 18 renders of a five-minute stereo project — enough that
/// stepping back and forth through recent history always hits, without
/// letting a long session quietly consume the disk.
pub const DEFAULT_CAP_BYTES: u64 = 1024 * 1024 * 1024;

/// How old a staging file must be before the next render sweeps it.
///
/// A render writes its staging file as it goes, so one that has not been
/// touched for this long belongs to a process that is gone. An hour is far
/// past the longest render, and short enough that a few crashes do not add
/// up to a cache that is quietly larger than its cap.
pub const STALE_STAGING: Duration = Duration::from_secs(60 * 60);

/// Environment override for the cap, in bytes. Exists so tests can use
/// a cap measured in kilobytes instead of writing a gigabyte of audio.
pub const CAP_ENV: &str = "EDYTLAB_PREVIEW_CACHE_BYTES";

/// Every cache folder under `.audiograph/`, by name: the folders that hold
/// only renders the app makes again on demand, so losing one costs a render
/// and nothing else (#576).
///
/// The one list of them. Save As leaves these folders out of the copy (and
/// only these, and only directly under `.audiograph/`: a folder of the same
/// name anywhere else is someone's own), and the window's asset scope grants
/// each by name, as it has to for a folder inside `.audiograph/` (#402). A
/// [`PreviewCache`] is made in one of them ([`PreviewCache::in_dir`] checks,
/// in a debug build), so a new cache is added here, and Save As and the
/// asset scope follow without a change of their own.
///
/// `derived/` is not on it, though some of its files rebuild: it is the
/// project's own audio, and a node that no replay can make again has no
/// other copy of what it plays.
pub const CACHE_DIRS: [&str; 2] = [CACHE_DIR, crate::tool::audition::AUDITION_DIR];

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

/// The entries something is still reading, by path, each with the number
/// of [`Held`]s that name it.
///
/// Process-wide rather than per [`PreviewCache`]: every caller opens its own
/// `PreviewCache` for a project (the window's commands and the agent's tools
/// alike), and all of them have to see the same holds. Eviction runs with
/// this locked, so a hold is taken either before an eviction looks or after
/// it has finished. Nothing is done under it but listing a cache folder and
/// removing files from it, and no other lock is taken while it is held.
static HELD: Mutex<BTreeMap<PathBuf, usize>> = Mutex::new(BTreeMap::new());

fn held() -> MutexGuard<'static, BTreeMap<PathBuf, usize>> {
    // A panic elsewhere cannot leave the map half-written: every change is
    // one insert or one remove.
    HELD.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// An entry kept out of eviction, and out of the cap, for as long as this
/// is alive; see [`PreviewCache::hold`].
///
/// Dropping the last hold on an entry evicts that entry's cache down to its
/// cap, the released entry included, so the cache is back under its cap as
/// soon as nothing holds it over.
#[derive(Debug)]
#[must_use = "the entry is held only while this is alive"]
pub struct Held {
    path: PathBuf,
    /// The cap of the cache the hold was taken in, which applies again once
    /// the hold is gone.
    cap_bytes: u64,
}

impl Held {
    /// The entry held, whether or not it is on disk.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Held {
    fn drop(&mut self) {
        let mut held = held();
        let Some(count) = held.get_mut(&self.path) else {
            return;
        };
        *count -= 1;
        if *count > 0 {
            return;
        }
        held.remove(&self.path);
        // The entry counts against the cap again. Evict now rather than at
        // the next render, which may be long after (or never, before the app
        // quits), so the cache is over its cap only while something holds it
        // there.
        if let Some(dir) = self.path.parent() {
            let cache = PreviewCache {
                dir: dir.to_path_buf(),
                cap_bytes: self.cap_bytes,
            };
            cache.evict(&held, None);
        }
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
    ///
    /// `subdir` is one of [`CACHE_DIRS`]: a cache folder not on that list
    /// would go into every Save As copy and be out of the webview's reach.
    pub fn in_dir(project_dir: &Path, subdir: &str) -> Self {
        debug_assert!(
            CACHE_DIRS.contains(&subdir),
            "`{subdir}` is not in CACHE_DIRS: add it there"
        );
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

    /// Keep the entry for `key` out of eviction for as long as the returned
    /// [`Held`] is alive, whether or not it has been rendered yet.
    ///
    /// For a caller that hands the path to something that reads the file
    /// again later, as the A/B bar does on every switch (#575). Take the hold
    /// before [`Self::get_or_render`]: an eviction that has already looked
    /// at the holds finishes before this returns, so the lookup after it sees
    /// what that eviction left, and no later eviction takes the entry.
    ///
    /// A held entry is outside the cap: renders made while it is held evict
    /// only unheld entries to fit, so the folder can be over the cap by what
    /// is held. It is not exempt from the rule that an entry bigger than the
    /// cap on its own is refused. Two holds on one key are two holds; the
    /// entry is held until both are dropped, and dropping the last one
    /// brings the cache back under its cap.
    pub fn hold(&self, key: session::NodeId) -> Held {
        let path = self.path_for(key);
        *held().entry(path.clone()).or_insert(0) += 1;
        Held {
            path,
            cap_bytes: self.cap_bytes,
        }
    }

    /// The cached entry for `key`, if there is one, marked as just used.
    pub fn lookup(&self, key: session::NodeId) -> Option<PathBuf> {
        let path = self.path_for(key);
        if !path.is_file() {
            return None;
        }
        // Touch so LRU sees this as recently used. A failure here costs
        // accuracy in eviction order, never correctness, so it is not
        // worth failing a cache hit over.
        let _ = touch(&path);
        Some(path)
    }

    /// Return the cached preview for `node`, rendering it with `render`
    /// only if it is not already there.
    ///
    /// `render` is handed a path to write to. It is called at most once
    /// and its output is moved into place atomically, so a failed render
    /// leaves the cache exactly as it was: the staging file is removed
    /// whether `render` fails, panics, or cannot be moved into place.
    ///
    /// An entry that is larger than the cap cannot be kept, and a path to a
    /// file that was evicted as it was written is no use to a caller, so
    /// that is an error rather than an `Ok`. It is refused before anything
    /// is evicted: the cache is left as it was. Entries that are
    /// [held](Self::hold) are outside the cap and are never evicted, so they
    /// play no part in whether a new entry fits.
    pub fn get_or_render<F, E>(&self, node: session::NodeId, render: F) -> Result<(PathBuf, Hit), E>
    where
        F: FnOnce(&Path) -> Result<(), E>,
        E: From<io::Error>,
    {
        if let Some(path) = self.lookup(node) {
            return Ok((path, Hit::Cached));
        }

        std::fs::create_dir_all(&self.dir).map_err(E::from)?;
        self.sweep_stale_staging();

        let final_path = self.path_for(node);
        // Same directory as the destination: a rename across filesystems
        // is not atomic, and the tempdir is frequently a different one.
        let staging = Staging(self.staging_path(node));
        render(&staging.0)?;
        match std::fs::rename(&staging.0, &final_path) {
            Ok(()) => {}
            // Another render of this key finished first. The name is the
            // content, so what is there is what this would have put there,
            // and a platform whose rename will not replace a file (a Windows
            // volume without POSIX rename) reports a race it lost as an
            // error. The staging file is removed on the way out.
            Err(_) if final_path.is_file() => {
                let _ = touch(&final_path);
                return Ok((final_path, Hit::Cached));
            }
            Err(e) => return Err(E::from(e)),
        }

        // An entry bigger than the whole cache cannot be kept. Finding that
        // out by evicting would cost the cache everything older first, so
        // look before evicting. Held entries are outside the cap and play no
        // part in this.
        let bytes = std::fs::metadata(&final_path).map(|m| m.len()).unwrap_or(0);
        if bytes > self.cap_bytes {
            let _ = std::fs::remove_file(&final_path);
            return Err(E::from(io::Error::other(format!(
                "the rendered preview ({bytes} bytes) does not fit in the preview cache, \
                 whose cap is {} bytes",
                self.cap_bytes
            ))));
        }

        // `rename` keeps the render's modified time, which is when the file
        // was written and not when it entered the cache, and a coarse clock
        // (exFAT keeps two seconds) can make that no newer than entries
        // already here. The entry just asked for is the one entry that must
        // survive, and it is the most recently used: say so first, and
        // evict around it.
        // The holds are read under their lock, which stays taken until the
        // eviction is done (see `HELD`).
        let _ = touch(&final_path);
        self.evict(&held(), Some(&final_path));

        if !final_path.is_file() {
            // Something else removed it as it was written.
            return Err(E::from(io::Error::other(
                "the rendered preview was removed from the preview cache as it was written",
            )));
        }

        Ok((final_path, Hit::Rendered))
    }

    /// A staging path that no other render, in this process or another,
    /// is using: two renders of one key must not write the same file, or
    /// one's cleanup would delete the other's work.
    fn staging_path(&self, node: session::NodeId) -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        self.dir.join(format!(
            ".{}.{}-{n}.partial",
            node.to_hex(),
            std::process::id()
        ))
    }

    /// Remove staging files too old to belong to a render in progress; see
    /// [`STALE_STAGING`].
    fn sweep_stale_staging(&self) {
        let Ok(read) = std::fs::read_dir(&self.dir) else {
            return;
        };
        let now = SystemTime::now();
        for entry in read.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("partial") {
                continue;
            }
            let stale = entry
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|modified| now.duration_since(modified).ok())
                .is_some_and(|age| age >= STALE_STAGING);
            if stale {
                let _ = std::fs::remove_file(&path);
            }
        }
    }

    /// Total bytes on disk, held entries included.
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

    /// Drop least-recently-used entries until the entries nobody
    /// [holds](Self::hold) fit the cap. Public so a caller can enforce the
    /// cap after changing it. Held entries are outside the cap and are never
    /// dropped.
    pub fn evict_to_cap(&self) {
        self.evict(&held(), None);
    }

    /// [`Self::evict_to_cap`], never taking `keep`. The cap is on the
    /// entries not in `held`, which are neither counted nor taken. An
    /// unheld `keep` that fits the cap on its own is always safe to keep:
    /// once every other unheld entry is gone it is all the cap holds.
    fn evict(&self, held: &BTreeMap<PathBuf, usize>, keep: Option<&Path>) {
        let mut entries: Vec<Entry> = self
            .entries()
            .into_iter()
            .filter(|e| !held.contains_key(&e.path))
            .collect();
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
            if keep == Some(entry.path.as_path()) {
                continue;
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

/// A staging file, removed when this goes out of scope.
///
/// After a successful rename the path is gone and the removal does
/// nothing, so the same line covers a failed render, a failed rename and a
/// panic.
struct Staging(PathBuf);

impl Drop for Staging {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// The key of the excerpt `[start, end)` (in frames) of what `base`
/// renders to.
///
/// `base` names a session state: a node, or for an audition the amended
/// state that never becomes one. Hashing the range in as well is what keeps
/// two excerpts of one state apart, and an excerpt apart from the whole
/// mix, which is stored under `base` itself. Equal inputs give equal keys,
/// so asking for an excerpt twice is a hit.
pub fn excerpt_key(base: session::NodeId, start: u64, end: u64) -> session::NodeId {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&base.0);
    hasher.update(&start.to_le_bytes());
    hasher.update(&end.to_le_bytes());
    session::NodeId(*hasher.finalize().as_bytes())
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

    /// Everything in the cache folder that is not a finished entry.
    fn staging_files(cache: &PreviewCache) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(cache.dir())
            .map(|read| {
                read.flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .filter(|n| !n.ends_with(".wav"))
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }

    /// The half-written file of a render that failed used to stay where it
    /// was: uncounted by the cap, and never looked at again.
    #[test]
    fn a_failed_render_leaves_no_staging_file() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = PreviewCache::new(tmp.path());

        cache
            .get_or_render::<_, io::Error>(node_id(20), |p| {
                write(p, 32)?;
                Err(io::Error::other("render blew up halfway"))
            })
            .unwrap_err();

        assert_eq!(staging_files(&cache), Vec::<String>::new());
    }

    /// A panic in the renderer unwinds through the cache the same way.
    #[test]
    fn a_render_that_panics_leaves_no_staging_file() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = PreviewCache::new(tmp.path());

        let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = cache.get_or_render::<_, io::Error>(node_id(21), |p| {
                write(p, 32)?;
                panic!("the renderer panicked halfway");
            });
        }));
        assert!(unwound.is_err(), "the panic should reach the caller");

        assert_eq!(staging_files(&cache), Vec::<String>::new());
    }

    /// The render succeeded and moving it into place did not: the file is
    /// still not the cache's to keep.
    #[test]
    fn a_render_that_cannot_be_moved_into_place_leaves_no_staging_file() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = PreviewCache::new(tmp.path());
        let id = node_id(22);
        // A folder with something in it where the entry belongs: a file
        // cannot be renamed over it.
        std::fs::create_dir_all(cache.path_for(id)).unwrap();
        std::fs::write(cache.path_for(id).join("in-the-way"), b"x").unwrap();

        cache
            .get_or_render::<_, io::Error>(id, |p| write(p, 32))
            .unwrap_err();

        assert_eq!(staging_files(&cache), Vec::<String>::new());
    }

    /// A render killed with the process (a crash, a power cut) cannot
    /// clean up after itself. Its file is in the cache folder but is not
    /// an entry, so nothing counts it against the cap; the next render
    /// sweeps it once it is too old to be one still being written.
    #[test]
    fn a_staging_file_left_by_a_crashed_render_is_swept() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = PreviewCache::new(tmp.path());
        std::fs::create_dir_all(cache.dir()).unwrap();

        let old = cache.dir().join(".dead.partial");
        let recent = cache.dir().join(".alive.partial");
        write(&old, 64).unwrap();
        write(&recent, 64).unwrap();
        let long_ago = SystemTime::now() - STALE_STAGING * 2;
        std::fs::File::options()
            .write(true)
            .open(&old)
            .unwrap()
            .set_modified(long_ago)
            .unwrap();

        cache
            .get_or_render::<_, io::Error>(node_id(23), |p| write(p, 64))
            .unwrap();

        assert!(!old.exists(), "a stale staging file was kept");
        assert!(
            recent.exists(),
            "a staging file that may still be being written was removed"
        );
    }

    /// An entry too big for the cap cannot be kept without breaking the
    /// cap, and a path to a file that was just evicted is no use to the
    /// caller. Say so instead.
    #[test]
    fn an_entry_larger_than_the_cap_is_an_error_not_a_dangling_path() {
        let tmp = tempfile::tempdir().unwrap();
        let mut cache = PreviewCache::new(tmp.path());
        cache.cap_bytes = 50;
        let id = node_id(24);

        let e = cache
            .get_or_render::<_, io::Error>(id, |p| write(p, 100))
            .unwrap_err();

        assert!(e.to_string().contains("cap"), "unhelpful error: {e}");
        assert!(!cache.path_for(id).exists());
        assert_eq!(cache.size_bytes(), 0);
    }

    /// A preview too big for the cap cannot be kept, and used to cost the
    /// cache everything else before that was found out: eviction ran with
    /// the new entry in place, oldest first, and took every older entry
    /// before it reached the one that could never fit.
    #[test]
    fn a_preview_over_the_cap_leaves_the_cache_as_it_was() {
        let tmp = tempfile::tempdir().unwrap();
        let mut cache = PreviewCache::new(tmp.path());
        cache.cap_bytes = 100;
        let (a, b) = (node_id(40), node_id(41));
        for id in [a, b] {
            cache
                .get_or_render::<_, io::Error>(id, |p| write(p, 40))
                .unwrap();
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        let big = node_id(42);
        let e = cache
            .get_or_render::<_, io::Error>(big, |p| write(p, 200))
            .unwrap_err();

        assert!(e.to_string().contains("cap"), "unhelpful error: {e}");
        assert!(!cache.path_for(big).exists());
        assert!(
            cache.path_for(a).is_file() && cache.path_for(b).is_file(),
            "an entry that fit was evicted to make room for one that could not"
        );
        assert_eq!(cache.size_bytes(), 80);
        assert_eq!(staging_files(&cache), Vec::<String>::new());
    }

    /// `rename` keeps the render's modified time, which is when the file
    /// was last written, not when it entered the cache. On a filesystem
    /// whose clock is coarse (exFAT keeps two seconds), or after a copy
    /// that preserves times, that can be older than entries already there;
    /// eviction would then take the entry just asked for and report an
    /// error for a preview that fits.
    #[test]
    fn a_new_entry_is_not_evicted_for_having_an_old_modified_time() {
        let tmp = tempfile::tempdir().unwrap();
        let mut cache = PreviewCache::new(tmp.path());
        cache.cap_bytes = 250;
        let (oldest, newer, fresh) = (node_id(43), node_id(44), node_id(45));
        for id in [oldest, newer] {
            cache
                .get_or_render::<_, io::Error>(id, |p| write(p, 100))
                .unwrap();
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        cache
            .get_or_render::<_, io::Error>(fresh, |p| {
                write(p, 100)?;
                std::fs::File::options()
                    .write(true)
                    .open(p)?
                    .set_modified(SystemTime::now() - Duration::from_secs(3600))
            })
            .expect("a 100-byte preview fits a 250-byte cache");

        assert!(cache.path_for(fresh).is_file(), "the new entry was evicted");
        assert!(cache.path_for(newer).is_file());
        assert!(
            !cache.path_for(oldest).exists(),
            "the entry evicted should be the least recently used one"
        );
        assert!(cache.size_bytes() <= 250);
    }

    /// Another render of the same key can finish between this one's lookup
    /// and its rename. On Unix the rename just replaces it; where it cannot
    /// (a Windows volume without POSIX rename) it is an error, but the
    /// entry is there and is the same bytes, so that is a hit, not a
    /// failure. The seam: a render that leaves no staging file makes the
    /// rename fail on every platform (a folder does not: Windows renames
    /// one over a file), while the winner's entry is already in place.
    #[test]
    fn a_rename_that_loses_a_race_is_a_hit_on_the_winner() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = PreviewCache::new(tmp.path());
        let id = node_id(46);

        let (path, hit) = cache
            .get_or_render::<_, io::Error>(id, |_staging| write(&cache.path_for(id), 64))
            .expect("the entry is there, so this is not an error");

        assert_eq!(hit, Hit::Cached);
        assert_eq!(path, cache.path_for(id));
        assert_eq!(std::fs::metadata(&path).unwrap().len(), 64);
    }

    /// Two renders of one key must not share a staging file: when one
    /// fails and cleans up, the other's half-finished work has to survive
    /// it. With one fixed name per key, the failure deleted the file the
    /// other was still writing.
    #[test]
    fn a_failed_render_does_not_delete_a_concurrent_renders_staging_file() {
        use std::sync::mpsc::channel;
        use std::sync::Arc;

        let tmp = tempfile::tempdir().unwrap();
        let cache = Arc::new(PreviewCache::new(tmp.path()));
        let id = node_id(25);
        let wait = Duration::from_secs(10);

        let (a_started, a_started_rx) = channel::<()>();
        let (b_done, b_done_rx) = channel::<()>();

        let a = {
            let cache = Arc::clone(&cache);
            std::thread::spawn(move || {
                cache.get_or_render::<_, io::Error>(id, |p| {
                    write(p, 64)?;
                    a_started.send(()).unwrap();
                    // Not finished until the other render has failed.
                    b_done_rx.recv_timeout(wait).unwrap();
                    Ok(())
                })
            })
        };

        a_started_rx.recv_timeout(wait).unwrap();
        let failed = cache.get_or_render::<_, io::Error>(id, |p| {
            write(p, 32)?;
            Err(io::Error::other("the second render failed"))
        });
        assert!(failed.is_err());
        b_done.send(()).unwrap();

        let (path, hit) = a
            .join()
            .unwrap()
            .expect("the first render was not disturbed");
        assert_eq!(hit, Hit::Rendered);
        assert_eq!(std::fs::metadata(path).unwrap().len(), 64);
        assert_eq!(staging_files(&cache), Vec::<String>::new());
    }

    /// An excerpt is its own entry: not the mix it was cut from, and not
    /// another cut of the same mix.
    #[test]
    fn an_excerpt_has_a_key_of_its_own() {
        let mix = node_id(30);
        let key = excerpt_key(mix, 0, 1_000);

        assert_ne!(key, mix, "an excerpt would be served as the mix");
        assert_eq!(key, excerpt_key(mix, 0, 1_000), "the key is not stable");
        assert_ne!(key, excerpt_key(mix, 0, 1_001));
        assert_ne!(key, excerpt_key(mix, 1, 1_000));
        assert_ne!(key, excerpt_key(node_id(31), 0, 1_000));
        // (0, 1000) is not (1000, 0), and a swap must not collide.
        assert_ne!(excerpt_key(mix, 0, 1_000), excerpt_key(mix, 1_000, 0));
    }

    // -----------------------------------------------------------------
    // Holds (#575)
    // -----------------------------------------------------------------

    /// Bytes on disk for the entries of `ids` that are there.
    fn bytes_of(cache: &PreviewCache, ids: &[session::NodeId]) -> u64 {
        ids.iter()
            .filter_map(|id| std::fs::metadata(cache.path_for(*id)).ok())
            .map(|m| m.len())
            .sum()
    }

    /// The A/B bar reads a side again on every switch. Here the held entry
    /// is the least recently used one, so without the hold it would be the
    /// first to go.
    #[test]
    fn a_held_entry_outlives_an_eviction_that_would_take_it() {
        let tmp = tempfile::tempdir().unwrap();
        let mut cache = PreviewCache::new(tmp.path());
        cache.cap_bytes = 150;
        let (old, middle, new) = (node_id(50), node_id(51), node_id(52));

        let _held = cache.hold(old);
        for id in [old, middle, new] {
            cache
                .get_or_render::<_, io::Error>(id, |p| write(p, 100))
                .unwrap();
            std::thread::sleep(Duration::from_millis(10));
        }

        assert!(cache.path_for(old).is_file(), "a held entry was evicted");
        assert!(
            !cache.path_for(middle).exists(),
            "the oldest entry not held should have gone instead"
        );
        assert!(cache.path_for(new).is_file());
        assert!(
            bytes_of(&cache, &[middle, new]) <= 150,
            "the entries nobody holds are over the cap"
        );
    }

    /// A held entry is outside the cap, so what is rendered beside it is
    /// judged on its own: a pair too big for the cap together can be
    /// compared, and a render beside it evicts only unheld entries. The
    /// folder is over the cap by what is held, and no more.
    #[test]
    fn held_entries_are_outside_the_cap() {
        let tmp = tempfile::tempdir().unwrap();
        let mut cache = PreviewCache::new(tmp.path());
        cache.cap_bytes = 150;
        let (a, b, c, d) = (node_id(53), node_id(54), node_id(55), node_id(56));

        let _holds = [cache.hold(a), cache.hold(b)];
        for id in [a, b, c] {
            cache
                .get_or_render::<_, io::Error>(id, |p| write(p, 100))
                .expect("each fits the cap on its own");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            [a, b, c].iter().all(|id| cache.path_for(*id).is_file()),
            "an entry was evicted or refused for the held pair's sake"
        );
        assert_eq!(cache.size_bytes(), 300, "over the cap by the held pair");

        // The next unheld render evicts the unheld one, not the pair.
        cache
            .get_or_render::<_, io::Error>(d, |p| write(p, 100))
            .unwrap();
        assert!(!cache.path_for(c).exists());
        assert!(cache.path_for(a).is_file() && cache.path_for(b).is_file());
        assert_eq!(cache.size_bytes(), 300);
        assert_eq!(staging_files(&cache), Vec::<String>::new());
    }

    /// Holding does not get round the one refusal there is: an entry bigger
    /// than the cap on its own is still an error.
    #[test]
    fn a_held_entry_bigger_than_the_cap_is_still_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let mut cache = PreviewCache::new(tmp.path());
        cache.cap_bytes = 50;
        let id = node_id(57);
        let _held = cache.hold(id);

        let e = cache
            .get_or_render::<_, io::Error>(id, |p| write(p, 100))
            .unwrap_err();

        assert!(e.to_string().contains("cap"), "unhelpful error: {e}");
        assert!(!cache.path_for(id).exists());
    }

    /// Dropping the holds puts the pair back under the cap, at once: the
    /// cache does not wait for a render that may never come.
    #[test]
    fn dropping_the_holds_brings_the_cache_back_under_its_cap() {
        let tmp = tempfile::tempdir().unwrap();
        let mut cache = PreviewCache::new(tmp.path());
        cache.cap_bytes = 150;
        let (a, b, newer) = (node_id(58), node_id(59), node_id(60));
        let holds = [cache.hold(a), cache.hold(b)];
        for id in [a, b, newer] {
            cache
                .get_or_render::<_, io::Error>(id, |p| write(p, 100))
                .unwrap();
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(cache.size_bytes(), 300);

        drop(holds);

        assert!(cache.size_bytes() <= 150, "still over the cap");
        assert!(
            cache.path_for(newer).is_file(),
            "the eviction should take the least recently used entries, the pair"
        );
        assert!(!cache.path_for(a).exists() && !cache.path_for(b).exists());
    }

    /// A hold ends when it is dropped: the entry is an ordinary one again,
    /// and goes first when it is the least recently used.
    #[test]
    fn a_dropped_hold_leaves_the_entry_to_eviction() {
        let tmp = tempfile::tempdir().unwrap();
        let mut cache = PreviewCache::new(tmp.path());
        cache.cap_bytes = 250;
        let (old, middle, new) = (node_id(61), node_id(62), node_id(63));

        let held = cache.hold(old);
        for id in [old, middle] {
            cache
                .get_or_render::<_, io::Error>(id, |p| write(p, 100))
                .unwrap();
            std::thread::sleep(Duration::from_millis(10));
        }
        drop(held);
        cache
            .get_or_render::<_, io::Error>(new, |p| write(p, 100))
            .unwrap();

        assert!(!cache.path_for(old).exists(), "a released entry was kept");
        assert!(cache.path_for(middle).is_file());
    }

    /// Two holds on one key (a compare of a node with itself, or two
    /// callers) keep the entry until both are gone.
    #[test]
    fn an_entry_held_twice_is_held_until_both_holds_are_dropped() {
        let tmp = tempfile::tempdir().unwrap();
        let mut cache = PreviewCache::new(tmp.path());
        cache.cap_bytes = 150;
        let (twice, next) = (node_id(64), node_id(65));
        let first = cache.hold(twice);
        let second = cache.hold(twice);
        assert_eq!(first.path(), cache.path_for(twice));
        for id in [twice, next] {
            cache
                .get_or_render::<_, io::Error>(id, |p| write(p, 100))
                .unwrap();
            std::thread::sleep(Duration::from_millis(10));
        }

        drop(first);
        assert!(
            cache.path_for(twice).is_file(),
            "released while a hold on it was left"
        );
        assert_eq!(cache.size_bytes(), 200);

        drop(second);
        assert!(!cache.path_for(twice).exists(), "kept after the last hold");
        assert!(cache.path_for(next).is_file());
    }

    /// `evict_to_cap` after a cap is lowered leaves a held entry too.
    #[test]
    fn evicting_to_a_lowered_cap_leaves_held_entries() {
        let tmp = tempfile::tempdir().unwrap();
        let mut cache = PreviewCache::new(tmp.path());
        let (held_id, free) = (node_id(66), node_id(67));
        let _held = cache.hold(held_id);
        for id in [held_id, free] {
            cache
                .get_or_render::<_, io::Error>(id, |p| write(p, 100))
                .unwrap();
            std::thread::sleep(Duration::from_millis(10));
        }

        cache.cap_bytes = 50;
        cache.evict_to_cap();

        assert!(cache.path_for(held_id).is_file());
        assert!(!cache.path_for(free).exists());
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
