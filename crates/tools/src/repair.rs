//! Bringing a copied project's audio home (#612).
//!
//! A project copied by Save As, or by hand, holds its own copy of every
//! file in `.audiograph/derived/`, `clipboard/` and `generated/`, and
//! reads its nodes with its own files (`session::relocate`). A copy saved
//! by 0.6.0 does not: opening it swept its whole `derived/`, because its
//! nodes named the original's files, and it has played from the original's
//! folder since. Delete the original and it keeps only what a replay can
//! rebuild.
//!
//! This puts such a copy right while the original is still there. For
//! every clip that names a file in another project's content-named folder
//! that exists there and is missing here, the file is copied into this
//! project's folder of the same kind, under the same name. Names are hashes
//! of the audio, so the copy is the file the clip names, and from then on
//! a read finds it here (`session::relocate`, rule 1).
//!
//! It only adds: a file already here under a name is never replaced, the
//! other project is only read, and nothing is deleted. A copy goes to a
//! temporary file in the folder it is for and is renamed into place only
//! when it is whole, and not over a file that appeared meanwhile. A file
//! that cannot be copied is counted and skipped.
//!
//! ## Within the history sweep's cap
//!
//! Past its cap, the history sweep (`reclaim::sweep`) deletes `derived/`
//! audio that only older nodes name and a replay rebuilds. A copy whose
//! sweep has done that names the original's file again (a read finds no
//! file here), and copying every such file back on each open would undo
//! the sweep, which would delete them again, and so on. So the files no
//! sweep of this project ever deletes are always copied: the head's,
//! what no replay can rebuild (`reclaim::rebuildable_paths`), and the
//! blobs and generated audio, which nothing sweeps. Other history is
//! copied newest first while `derived/` stays within the cap. What is left
//! out a replay rebuilds into this project when it is wanted
//! (`rederive::materialize`), the original there or not.
//!
//! ## When it runs
//!
//! The desktop app plans it as a project opens, under the store's lock,
//! and copies with the lock let go, holding a staged handle on the store
//! (`Store::stage`) until it is done. The handle writes nothing; it counts
//! as in flight, so the orphan sweep, the background reclaimer and
//! compaction wait (rule 0 in [`crate::reclaim`]), and none of them can
//! take a half-copied file, or a whole one, for an orphan.

use std::collections::{BTreeSet, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::provenance::derived_dir;

/// One file to copy in.
#[derive(Debug, Clone)]
pub struct RepairFile {
    from: PathBuf,
    to: PathBuf,
    bytes: u64,
}

impl RepairFile {
    /// The other project's file a clip names.
    pub fn from(&self) -> &Path {
        &self.from
    }

    /// Where this project keeps a file of that name.
    pub fn to(&self) -> &Path {
        &self.to
    }

    pub fn bytes(&self) -> u64 {
        self.bytes
    }
}

/// What a repair would copy, decided without copying anything.
#[derive(Debug, Clone, Default)]
pub struct RepairPlan {
    files: Vec<RepairFile>,
}

impl RepairPlan {
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    pub fn files(&self) -> &[RepairFile] {
        &self.files
    }
}

/// What a repair did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RepairReport {
    /// Files copied in.
    pub copied: usize,
    /// Their total size.
    pub bytes: u64,
    /// Files that were here by the time their turn came (another open's
    /// repair, or an edit that wrote the same audio), left as they were.
    pub already_there: usize,
    /// Files that could not be copied (the other project went out of
    /// reach, the disk filled), skipped and logged.
    pub failed: usize,
}

/// Decide what to copy into `store`'s project, as described in the module
/// docs, keeping `derived/` within `cap_bytes` for what the history sweep
/// could delete again. Reads the nodes, so the caller holds the store's
/// lock. A store whose nodes cannot be read plans nothing.
pub fn plan_repair(store: &session::Store, cap_bytes: u64) -> RepairPlan {
    let Ok(nodes) = store.list_nodes() else {
        return RepairPlan::default();
    };
    let head_named: BTreeSet<PathBuf> = store
        .head()
        .and_then(|head| store.get(head).ok())
        .map(|head| {
            head.state
                .tracks
                .iter()
                .flat_map(|t| &t.clips)
                .map(|c| c.source_path.clone())
                .collect()
        })
        .unwrap_or_default();
    let rebuildable = crate::reclaim::rebuildable_paths(&nodes);
    let derived = derived_dir(store.project_dir());

    let mut always = Vec::new();
    let mut within_cap: Vec<(SystemTime, RepairFile)> = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    for clip in nodes
        .iter()
        .flat_map(|n| &n.state.tracks)
        .flat_map(|t| &t.clips)
    {
        let from = &clip.source_path;
        let Some(to) = store.counterpart(from) else {
            continue;
        };
        if !seen.insert(to.clone()) || to.exists() {
            continue;
        }
        let Ok(meta) = std::fs::metadata(from) else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        let file = RepairFile {
            from: from.clone(),
            to,
            bytes: meta.len(),
        };
        let sweepable = file.to.parent() == Some(derived.as_path())
            && !head_named.contains(from)
            && rebuildable.contains(&resolved(from));
        if sweepable {
            within_cap.push((meta.modified().unwrap_or(SystemTime::UNIX_EPOCH), file));
        } else {
            always.push(file);
        }
    }

    let mut total = folder_bytes(&derived)
        + always
            .iter()
            .filter(|f| f.to.parent() == Some(derived.as_path()))
            .map(|f| f.bytes)
            .sum::<u64>();
    // Newest first: the history sweep takes the oldest first.
    within_cap.sort_by(|a, b| b.0.cmp(&a.0));
    for (_, file) in within_cap {
        if total + file.bytes <= cap_bytes {
            total += file.bytes;
            always.push(file);
        }
    }
    RepairPlan { files: always }
}

/// Copy in what `plan` lists, as described in the module docs. Takes no
/// lock; the caller keeps the deleters away with a staged handle.
pub fn apply_repair(plan: &RepairPlan) -> RepairReport {
    let mut report = RepairReport::default();
    for file in &plan.files {
        if file.to.exists() {
            report.already_there += 1;
            continue;
        }
        match copy_in(&file.from, &file.to) {
            Ok(Some(bytes)) => {
                report.copied += 1;
                report.bytes += bytes;
            }
            Ok(None) => report.already_there += 1,
            Err(e) => {
                tracing::warn!(
                    from = %file.from.display(),
                    to = %file.to.display(),
                    error = %e,
                    "could not copy a file this project names into it"
                );
                report.failed += 1;
            }
        }
    }
    report
}

/// Copy `from` to `to` by way of a temporary file beside `to`, renamed
/// into place only when whole and only if nothing is at `to` by then.
/// `Ok(None)` when something was.
fn copy_in(from: &Path, to: &Path) -> io::Result<Option<u64>> {
    let parent = to
        .parent()
        .ok_or_else(|| io::Error::other("the destination has no folder"))?;
    std::fs::create_dir_all(parent)?;
    let mut part = tempfile::Builder::new()
        .prefix(".repair-")
        .suffix(".wav.part")
        .tempfile_in(parent)?;
    let mut src = std::fs::File::open(from)?;
    let bytes = io::copy(&mut src, part.as_file_mut())?;
    part.as_file().sync_all()?;
    match part.persist_noclobber(to) {
        Ok(_) => Ok(Some(bytes)),
        Err(e) if e.error.kind() == io::ErrorKind::AlreadyExists => Ok(None),
        Err(e) => Err(e.error),
    }
}

/// `path` as the file system names it, as `reclaim` keys paths.
fn resolved(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// The total size of the files directly in `dir`.
fn folder_bytes(dir: &Path) -> u64 {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|e| e.metadata().ok())
                .filter(|m| m.is_file())
                .map(|m| m.len())
                .sum()
        })
        .unwrap_or(0)
}
