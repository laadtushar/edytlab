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
//! other project is only read, and nothing is deleted but its own staging
//! files. A copy goes to a temporary file in the folder it is for, keeps
//! the source's modification time, and is renamed into place only when it
//! is whole, and not over a file that appeared meanwhile. A file that
//! cannot be copied is counted and skipped; on a volume that has no rename
//! that refuses to replace a file (and no hard links), that is every file,
//! and the copy is left as it was.
//!
//! ## Room on the disk
//!
//! Before each file the repair asks the volume how much room it has, and
//! stops where a file would leave less than [`KEEP_FREE_BYTES`] (1 GiB), or
//! at the first copy the disk refuses as full. What it left is counted and logged, and the next open tries
//! again. The head's audio goes first, so a repair cut short has put back
//! what the session plays now.
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
//! from the nodes it has listed for the orphan sweep ([`plan_repair_listed`]:
//! a project that is not a copy costs one pass over its clips and no file
//! system call), and copies with the lock let go, holding a staged handle on
//! the store (`Store::stage`) until it is done. The handle writes nothing; it counts
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
    /// reach, a volume that cannot rename without replacing), skipped and
    /// logged.
    pub failed: usize,
    /// Files not copied because the disk was nearly full ([`KEEP_FREE_BYTES`]):
    /// the repair stopped there, and the next open tries again.
    pub left_for_space: usize,
    /// Their total size.
    pub left_bytes: u64,
}

/// What the repair leaves free on a volume: 1 GiB. Filling the disk to
/// bring a copy's audio home would stop the next edit, or the next save,
/// from being written; a gigabyte is room for many edits (a five-minute
/// stereo edit writes about 55 MB).
///
/// Not a share of the volume: a tenth of a 1 TB disk is 100 GB, which a
/// disk in daily use often does not have, and the repair would then never
/// run there, leaving the copy to lose its audio with the original, which
/// is what it is for.
pub const KEEP_FREE_BYTES: u64 = 1 << 30;

/// The bytes this process may still write on the volume holding `dir`.
pub fn available_space(dir: &Path) -> io::Result<u64> {
    Ok(fs4::statvfs(dir)?.available_space())
}

/// Decide what to copy into `store`'s project, as described in the module
/// docs, keeping `derived/` within `cap_bytes` for what the history sweep
/// could delete again. Reads the nodes, so the caller holds the store's
/// lock. A store whose nodes cannot be read plans nothing.
///
/// See [`plan_repair_listed`], which this lists the nodes for.
pub fn plan_repair(store: &session::Store, cap_bytes: u64) -> RepairPlan {
    match store.list_nodes() {
        Ok(nodes) => plan_repair_listed(store, &nodes, cap_bytes),
        Err(_) => RepairPlan::default(),
    }
}

/// [`plan_repair`] for nodes the caller has already listed from `store`:
/// opening a project lists them once for the orphan sweep and for this,
/// both under the store's lock.
///
/// A project that is not a copy costs one pass over the clips and no file
/// system call: a path into the project's own folders is told apart by
/// comparing it, and a file of the user's by its folder's name. Only when
/// some clip names another project's file that this one lacks does the
/// plan look further (the head, what can be rebuilt, the size of
/// `derived/`).
///
/// It also removes staging files a repair left behind when the app was
/// killed mid-copy, from `clipboard/` and `generated/` (the orphan sweep
/// removes those in `derived/`), but only while no staged handle on the
/// project is alive: a repair running now holds one, and its staging file
/// is not a leftover.
pub fn plan_repair_listed(
    store: &session::Store,
    nodes: &[session::SessionNode],
    cap_bytes: u64,
) -> RepairPlan {
    if store.staged_in_flight() == 0 {
        remove_leftovers(store.project_dir());
    }

    // What some clip names in another project, and this one lacks.
    let mut wanted: Vec<(RepairFile, SystemTime)> = Vec::new();
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
        let mtime = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        let file = RepairFile {
            from: from.clone(),
            to,
            bytes: meta.len(),
        };
        wanted.push((file, mtime));
    }
    if wanted.is_empty() {
        return RepairPlan::default();
    }

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
    let rebuildable = crate::reclaim::rebuildable_paths(nodes);
    let derived = derived_dir(store.project_dir());

    // The head's audio first, so a repair that stops for space has put
    // back what the session now plays; then what nothing could rebuild;
    // then, within the cap, history newest first.
    let mut heads = Vec::new();
    let mut always = Vec::new();
    let mut within_cap: Vec<(SystemTime, RepairFile)> = Vec::new();
    for (file, mtime) in wanted {
        if head_named.contains(&file.from) {
            heads.push(file);
        } else if file.to.parent() == Some(derived.as_path())
            && rebuildable.contains(&resolved(&file.from))
        {
            within_cap.push((mtime, file));
        } else {
            always.push(file);
        }
    }
    heads.append(&mut always);
    let mut files = heads;

    let mut total = folder_bytes(&derived)
        + files
            .iter()
            .filter(|f| f.to.parent() == Some(derived.as_path()))
            .map(|f| f.bytes)
            .sum::<u64>();
    // Newest first: the history sweep takes the oldest first.
    within_cap.sort_by(|a, b| b.0.cmp(&a.0));
    for (_, file) in within_cap {
        if total + file.bytes <= cap_bytes {
            total += file.bytes;
            files.push(file);
        }
    }
    RepairPlan { files }
}

/// Copy in what `plan` lists, as described in the module docs, while the
/// disk keeps [`KEEP_FREE_BYTES`] free. Takes no lock; the caller keeps
/// the deleters away with a staged handle.
pub fn apply_repair(plan: &RepairPlan) -> RepairReport {
    apply_repair_with(plan, available_space)
}

/// [`apply_repair`], asking `space` how many bytes the volume has left
/// before each file. A file that would leave less than [`KEEP_FREE_BYTES`],
/// or a copy the disk refuses as full, stops the repair there; what is left
/// is counted, logged, and tried again at the next open. A volume that
/// cannot be asked is not a reason to stop: a full disk still stops it.
pub fn apply_repair_with(
    plan: &RepairPlan,
    mut space: impl FnMut(&Path) -> io::Result<u64>,
) -> RepairReport {
    let mut report = RepairReport::default();
    for (i, file) in plan.files.iter().enumerate() {
        if file.to.exists() {
            report.already_there += 1;
            continue;
        }
        let room = existing_ancestor(&file.to).map(&mut space);
        let full = match room {
            Some(Ok(available)) => available < file.bytes.saturating_add(KEEP_FREE_BYTES),
            _ => false,
        };
        let copied = if full {
            Err(io::Error::from(io::ErrorKind::StorageFull))
        } else {
            copy_in(&file.from, &file.to)
        };
        match copied {
            Ok(Some(bytes)) => {
                report.copied += 1;
                report.bytes += bytes;
            }
            Ok(None) => report.already_there += 1,
            Err(e) if e.kind() == io::ErrorKind::StorageFull => {
                let left = &plan.files[i..];
                report.left_for_space = left.len();
                report.left_bytes = left.iter().map(|f| f.bytes).sum();
                tracing::warn!(
                    left = report.left_for_space,
                    left_bytes = report.left_bytes,
                    "stopped copying audio into this project: the disk is nearly full"
                );
                break;
            }
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
/// `Ok(None)` when something was. The copy keeps the source's modification
/// time, which is what the history sweep takes the oldest by.
///
/// The rename is `tempfile`'s no-replace rename, and a hard link where the
/// volume refuses that; on a volume that allows neither (some FAT and
/// network volumes) it is an error, and nothing is written.
fn copy_in(from: &Path, to: &Path) -> io::Result<Option<u64>> {
    let parent = to
        .parent()
        .ok_or_else(|| io::Error::other("the destination has no folder"))?;
    std::fs::create_dir_all(parent)?;
    let mut part = tempfile::Builder::new()
        .prefix(STAGING_PREFIX)
        .suffix(STAGING_SUFFIX)
        .tempfile_in(parent)?;
    let mut src = std::fs::File::open(from)?;
    let bytes = io::copy(&mut src, part.as_file_mut())?;
    if let Ok(mtime) = src.metadata().and_then(|m| m.modified()) {
        let _ = part.as_file().set_modified(mtime);
    }
    part.as_file().sync_all()?;
    match part.persist_noclobber(to) {
        Ok(_) => Ok(Some(bytes)),
        Err(e) if e.error.kind() == io::ErrorKind::AlreadyExists => Ok(None),
        Err(e) => Err(e.error),
    }
}

/// How a repair names its staging files.
const STAGING_PREFIX: &str = ".repair-";
const STAGING_SUFFIX: &str = ".wav.part";

/// Remove the staging files a killed repair left in `clipboard/` and
/// `generated/`. A file that cannot be removed is left for the next open.
fn remove_leftovers(project_dir: &Path) {
    for dir in [
        session::relocate::clipboard_dir(project_dir),
        session::relocate::generated_dir(project_dir),
    ] {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with(STAGING_PREFIX) && name.ends_with(STAGING_SUFFIX) {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
}

/// The nearest folder of `path` that exists, to ask its volume for room.
fn existing_ancestor(path: &Path) -> Option<&Path> {
    path.ancestors().skip(1).find(|p| p.is_dir())
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// The rename never replaces a file: one that is at `to` by the time
    /// the copy is whole stays as it was, and no staging file is left.
    #[test]
    fn copy_in_never_replaces_a_file() {
        let tmp = TempDir::new().unwrap();
        let from = tmp.path().join("from.wav");
        std::fs::write(&from, b"the original's").unwrap();
        let dir = tmp.path().join("derived");
        std::fs::create_dir_all(&dir).unwrap();
        let to = dir.join("abc.wav");
        std::fs::write(&to, b"already here").unwrap();

        assert_eq!(copy_in(&from, &to).unwrap(), None);
        assert_eq!(std::fs::read(&to).unwrap(), b"already here");
        let names: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name())
            .collect();
        assert_eq!(names, vec![std::ffi::OsString::from("abc.wav")]);
    }

    /// The copy keeps the source's modification time: the history sweep
    /// takes the oldest first, and a repaired file stamped with the time of
    /// the repair would read as the newest history there is.
    #[test]
    fn copy_in_keeps_the_sources_time() {
        let tmp = TempDir::new().unwrap();
        let from = tmp.path().join("from.wav");
        std::fs::write(&from, b"audio").unwrap();
        let then = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_600_000_000);
        std::fs::File::options()
            .write(true)
            .open(&from)
            .unwrap()
            .set_modified(then)
            .unwrap();
        let to = tmp.path().join("derived").join("abc.wav");

        assert_eq!(copy_in(&from, &to).unwrap(), Some(5));
        assert_eq!(std::fs::metadata(&to).unwrap().modified().unwrap(), then);
    }

    /// A plan of `n` files of `bytes` each, from one folder to another.
    fn plan_of(tmp: &TempDir, n: usize, bytes: usize) -> RepairPlan {
        let src = tmp.path().join("original");
        let dest = tmp.path().join("copy");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir_all(&dest).unwrap();
        let files = (0..n)
            .map(|i| {
                let from = src.join(format!("{i}.wav"));
                std::fs::write(&from, vec![0u8; bytes]).unwrap();
                RepairFile {
                    from,
                    to: dest.join(format!("{i}.wav")),
                    bytes: bytes as u64,
                }
            })
            .collect();
        RepairPlan { files }
    }

    /// With the disk nearly full the repair copies nothing, and says what
    /// it left.
    #[test]
    fn a_nearly_full_disk_stops_the_repair() {
        let tmp = TempDir::new().unwrap();
        let plan = plan_of(&tmp, 3, 100);

        let report = apply_repair_with(&plan, |_| Ok(KEEP_FREE_BYTES + 50));

        assert_eq!(report.copied, 0, "{report:?}");
        assert_eq!(report.left_for_space, 3, "{report:?}");
        assert_eq!(report.left_bytes, 300, "{report:?}");
        assert!(!plan.files()[0].to().exists());
    }

    /// It copies while the room lasts and stops at the first file that
    /// would take the disk under its margin.
    #[test]
    fn the_repair_stops_where_the_room_runs_out() {
        let tmp = TempDir::new().unwrap();
        let plan = plan_of(&tmp, 3, 100);
        let mut available = KEEP_FREE_BYTES + 250;

        let report = apply_repair_with(&plan, |_| {
            let room = available;
            available -= 100;
            Ok(room)
        });

        assert_eq!(report.copied, 2, "{report:?}");
        assert_eq!(report.left_for_space, 1, "{report:?}");
        assert!(plan.files()[1].to().is_file());
        assert!(!plan.files()[2].to().exists());
    }

    /// A volume that cannot be asked does not stop the repair.
    #[test]
    fn a_volume_that_cannot_be_asked_does_not_stop_it() {
        let tmp = TempDir::new().unwrap();
        let plan = plan_of(&tmp, 2, 10);

        let report = apply_repair_with(&plan, |_| Err(io::Error::other("no statvfs here")));

        assert_eq!(report.copied, 2, "{report:?}");
    }

    /// A killed repair's staging files in `clipboard/` and `generated/` go
    /// when the next repair is planned, unless a repair is running; other
    /// files stay.
    #[test]
    fn a_killed_repairs_staging_files_go_at_the_next_plan() {
        let tmp = TempDir::new().unwrap();
        let project = tmp.path().join("p");
        let store = session::Store::open(&project).unwrap();
        let dirs = [
            session::relocate::clipboard_dir(&project),
            session::relocate::generated_dir(&project),
        ];
        let put = || {
            for dir in &dirs {
                std::fs::create_dir_all(dir).unwrap();
                std::fs::write(dir.join(".repair-KILLED.wav.part"), b"half").unwrap();
                std::fs::write(dir.join("keep.wav"), b"audio").unwrap();
            }
        };
        put();

        let running = store.stage().unwrap();
        plan_repair(&store, u64::MAX);
        for dir in &dirs {
            assert!(
                dir.join(".repair-KILLED.wav.part").exists(),
                "a running repair's file went"
            );
        }
        drop(running);

        plan_repair(&store, u64::MAX);
        for dir in &dirs {
            assert!(
                !dir.join(".repair-KILLED.wav.part").exists(),
                "{}",
                dir.display()
            );
            assert!(dir.join("keep.wav").is_file());
        }
    }
}
