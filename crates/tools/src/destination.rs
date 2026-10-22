//! Where a write may land: never on audio the project reads (#653).
//!
//! `load` leaves a WAV where it is, so a clip names the user's own file, and
//! the nodes of an older edit name it long after the head has moved on. A
//! writer handed that path destroys it: `File::create` truncates the file
//! before a render has read a sample, and a write that did read first would
//! still change what every node naming the file plays, so undo would lie.
//! `batch_apply` did exactly this by default, since its `output_dir` is the
//! input folder and a `render_format` equal to an input's extension lands the
//! render on that input.
//!
//! So a writer that takes a path from its caller asks here first. A
//! destination is refused when it is the same file as any audio the open
//! project reads:
//!
//! * a clip source named by **any** node in the project's graph, not only the
//!   head's, because overwriting what an older node names breaks undo; and
//! * the file a `load` read, even when the session plays a copy of it (a
//!   lossy source is transcoded to `derived/`), because that file is the
//!   user's recording and re-encoding over it is as final.
//!
//! ## The same file, not the same spelling
//!
//! Paths are compared by what they are, not by what they are called: the
//! device and inode on unix, the volume serial number and file id on Windows
//! (the same call that tells project folders apart, [`session::live::identify`]
//! — it opens a file as readily as a folder). `take1.WAV` is `take1.wav` on a
//! case-insensitive disk, a path through a symlink is its target, and a hard
//! link is a second name for one file; none of those is a different file. A
//! file that cannot be opened to ask falls back to its resolved path.
//!
//! A destination that does not exist cannot be a source, so a fresh path costs
//! one `stat` and the project's history is read only when something is
//! already there to be replaced. A file that exists and is nobody's source
//! (an earlier export, say) is written over as it always was.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use serde_json::Value;
use session::live::identify;
use session::FolderId;

/// A set of files, looked up by which file they are rather than by name.
///
/// Built once and asked many times, so `n` inputs against `n` renders is `n`
/// opens and not `n²`. A path that is not a regular file that exists is not
/// in it and is never found in it: a file that is not there is nobody's
/// source, and a render cannot be written onto it.
pub struct FileSet<T> {
    by_id: HashMap<FolderId, T>,
    by_path: HashMap<PathBuf, T>,
}

impl<T: Clone> FileSet<T> {
    pub fn new() -> Self {
        Self {
            by_id: HashMap::new(),
            by_path: HashMap::new(),
        }
    }

    /// Add `path`, keeping the first value given for a file. Ignored when
    /// `path` is not a regular file that exists.
    pub fn insert(&mut self, path: &Path, value: T) {
        if !path.is_file() {
            return;
        }
        if let Ok(identity) = identify(path) {
            self.by_id
                .entry(identity.id())
                .or_insert_with(|| value.clone());
        }
        // Both, so a file whose identity cannot be read is still found by
        // where it resolves to, and the other way round.
        if let Ok(resolved) = std::fs::canonicalize(path) {
            self.by_path.entry(resolved).or_insert(value);
        }
    }

    /// The value stored for the file `path` is, if it is one of them.
    pub fn find(&self, path: &Path) -> Option<&T> {
        if self.by_id.is_empty() && self.by_path.is_empty() {
            return None;
        }
        if !path.is_file() {
            return None;
        }
        if let Some(found) = identify(path)
            .ok()
            .and_then(|identity| self.by_id.get(&identity.id()))
        {
            return Some(found);
        }
        std::fs::canonicalize(path)
            .ok()
            .and_then(|resolved| self.by_path.get(&resolved))
    }
}

impl<T: Clone> Default for FileSet<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// For each of `planned` that is the same file as one of `protected`, the
/// pair of positions `(planned, protected)`, in the order of `planned`.
///
/// `planned` paths that do not exist are skipped, whatever they are called.
pub fn overlaps(planned: &[PathBuf], protected: &[PathBuf]) -> Vec<(usize, usize)> {
    let mut files = FileSet::new();
    for (index, path) in protected.iter().enumerate() {
        files.insert(path, index);
    }
    planned
        .iter()
        .enumerate()
        .filter_map(|(i, path)| files.find(path).map(|&j| (i, j)))
        .collect()
}

/// How the project reads a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reads {
    /// A clip's source, in some node.
    Plays,
    /// The file a `load` imported, which the session may only play a copy of.
    LoadedFrom,
}

/// Every audio file the project's nodes name, by path as the nodes say it.
///
/// A clip source wins over a `load` that read the same file.
fn named_audio(nodes: &[session::SessionNode]) -> Vec<(PathBuf, Reads)> {
    let mut named: BTreeMap<PathBuf, Reads> = BTreeMap::new();
    let mut note = |path: PathBuf, reads: Reads| {
        named
            .entry(path)
            .and_modify(|seen| {
                if reads == Reads::Plays {
                    *seen = Reads::Plays;
                }
            })
            .or_insert(reads);
    };
    for node in nodes {
        for track in &node.state.tracks {
            for clip in &track.clips {
                note(clip.source_path.clone(), Reads::Plays);
            }
        }
        if let Some(op) = node.op.as_ref().filter(|op| op.tool == "load") {
            if let Some(Value::String(path)) = op.inputs.get("source").and_then(|s| s.get("path")) {
                note(PathBuf::from(path), Reads::LoadedFrom);
            }
        }
    }
    named.into_iter().collect()
}

/// Refuse a write to any of `destinations` that is the same file as audio the
/// project reads (see the module documentation). The message is for the
/// caller of the tool, who has to choose another path: it names the
/// destination, the file it is, and what to do.
///
/// Reads the store's whole history, but only when a destination exists to be
/// replaced. If the history cannot be read, an existing file is not written:
/// nothing is known to be safe.
pub fn refuse_project_audio<I, P>(store: &session::Store, destinations: I) -> Result<(), String>
where
    I: IntoIterator<Item = P>,
    P: AsRef<Path>,
{
    let existing: Vec<PathBuf> = destinations
        .into_iter()
        .map(|d| d.as_ref().to_path_buf())
        .filter(|d| d.is_file())
        .collect();
    let Some(first) = existing.first() else {
        return Ok(());
    };

    let nodes = store.list_nodes().map_err(|e| {
        format!(
            "not written: {} already exists, and the project's history could not be read to check \
             that it is not audio the project plays ({e}). Choose a path that does not exist yet.",
            first.display()
        )
    })?;
    let mut files: FileSet<(PathBuf, Reads)> = FileSet::new();
    for (path, reads) in named_audio(&nodes) {
        files.insert(&path, (path.clone(), reads));
    }
    for destination in &existing {
        if let Some((source, reads)) = files.find(destination) {
            return Err(refusal(destination, source, *reads));
        }
    }
    Ok(())
}

fn refusal(destination: &Path, source: &Path, reads: Reads) -> String {
    let which = if destination == source {
        format!("{}", destination.display())
    } else {
        format!(
            "{} (the same file as {})",
            destination.display(),
            source.display()
        )
    };
    let what = match reads {
        Reads::Plays => {
            "audio this project plays from, in its current state or in an earlier step of its \
             history, so writing to it would replace what those steps play and undo would no \
             longer return to what was there"
        }
        Reads::LoadedFrom => {
            "the file this project was loaded from, so writing to it would replace the original \
             recording"
        }
    };
    format!("not written: {which} is {what}. Choose another file name or folder for the output.")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn file(dir: &Path, name: &str, text: &str) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, text).unwrap();
        path
    }

    #[test]
    fn a_path_that_is_not_there_is_nobodys_file() {
        let dir = TempDir::new().unwrap();
        let mut set = FileSet::new();
        set.insert(&dir.path().join("missing.wav"), 1);
        assert!(set.find(&dir.path().join("missing.wav")).is_none());
    }

    #[test]
    fn a_folder_is_not_a_file() {
        let dir = TempDir::new().unwrap();
        let mut set = FileSet::new();
        set.insert(dir.path(), 1);
        assert!(set.find(dir.path()).is_none());
    }

    #[test]
    fn two_files_with_the_same_bytes_are_two_files() {
        let dir = TempDir::new().unwrap();
        let a = file(dir.path(), "a.wav", "same");
        let b = file(dir.path(), "b.wav", "same");
        let mut set = FileSet::new();
        set.insert(&a, "a");
        assert_eq!(set.find(&a), Some(&"a"));
        assert_eq!(set.find(&b), None);
    }

    #[test]
    fn a_hard_link_is_the_same_file() {
        let dir = TempDir::new().unwrap();
        let a = file(dir.path(), "a.wav", "x");
        let alias = dir.path().join("alias.wav");
        fs::hard_link(&a, &alias).unwrap();
        let mut set = FileSet::new();
        set.insert(&a, 7);
        assert_eq!(set.find(&alias), Some(&7));
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_and_a_path_through_a_linked_folder_are_the_same_file() {
        let dir = TempDir::new().unwrap();
        let real = dir.path().join("real");
        fs::create_dir(&real).unwrap();
        let a = file(&real, "a.wav", "x");
        let link = dir.path().join("link.wav");
        std::os::unix::fs::symlink(&a, &link).unwrap();
        let folder = dir.path().join("folder");
        std::os::unix::fs::symlink(&real, &folder).unwrap();
        let mut set = FileSet::new();
        set.insert(&a, ());
        assert!(set.find(&link).is_some());
        assert!(set.find(&folder.join("a.wav")).is_some());
    }

    #[cfg(unix)]
    #[test]
    fn a_dangling_symlink_is_not_a_file() {
        let dir = TempDir::new().unwrap();
        let a = file(dir.path(), "a.wav", "x");
        let dangling = dir.path().join("dangling.wav");
        std::os::unix::fs::symlink(dir.path().join("gone.wav"), &dangling).unwrap();
        let mut set = FileSet::new();
        set.insert(&a, ());
        assert!(set.find(&dangling).is_none());
    }

    #[test]
    fn overlaps_pairs_each_planned_path_with_the_protected_file_it_is() {
        let dir = TempDir::new().unwrap();
        let a = file(dir.path(), "a.wav", "x");
        let b = file(dir.path(), "b.wav", "y");
        let planned = vec![
            dir.path().join("fresh.wav"),
            dir.path().join(".").join("b.wav"),
            a.clone(),
        ];
        assert_eq!(overlaps(&planned, &[a, b]), vec![(1, 1), (2, 0)]);
    }

    #[test]
    fn overlaps_with_nothing_protected_finds_nothing() {
        let dir = TempDir::new().unwrap();
        let a = file(dir.path(), "a.wav", "x");
        assert!(overlaps(&[a], &[]).is_empty());
    }
}
