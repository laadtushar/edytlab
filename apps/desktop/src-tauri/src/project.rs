//! A project as a real object (#156).
//!
//! Until now a project was a folder path and nothing else: `open_project`
//! took a directory, `Store::open` made `.audiograph/` inside it, and
//! that was the whole concept. Nothing carried a name, nothing recorded
//! that you had opened it, and reopening put you back at the folder
//! rather than where you were working.
//!
//! Three small files fix that, and they are deliberately separate:
//!
//! * **`<project>/project.json`** — what the project *is*: a name that
//!   is not its folder path, when it was created, notes. Beside
//!   `.audiograph/` rather than inside it, because the store is an
//!   implementation detail and this is the thing a person would open,
//!   copy or back up.
//! * **`<project>/.audiograph/view.json`** — where you were: head, zoom,
//!   selection, playhead. Inside the store directory, because unlike the
//!   name it is disposable — losing it costs a scroll, not work.
//! * **`~/.edytlab/recents.json`** — which projects exist. Outside every
//!   project, because a list of projects cannot live inside one of them.
//!
//! Nothing here is load-bearing for the audio. Every read tolerates a
//! missing or corrupt file by returning defaults: a project whose
//! `project.json` was hand-edited into nonsense must still open, because
//! the audio and its history are not in that file.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use session::Store;

/// View state, inside `.audiograph/`. Stays here: it is the app's own
/// idea of where you were looking, and nothing below the app reads it.
pub const VIEW_FILE: &str = "view.json";
/// How many recent projects to remember. Ten is about a screen.
pub const MAX_RECENTS: usize = 10;

/// `ProjectMeta` and its file now live in `session`, because `tools`
/// needs to read them and cannot depend on this crate — see that
/// module for why the type moved rather than being duplicated.
///
/// Re-exported here so every existing caller, and this module's own
/// view/recents code, keep referring to `project::ProjectMeta` as
/// before.
pub use session::meta::{meta_path, read_meta, write_meta, ProjectMeta, PROJECT_FILE};

/// Where the user was, so reopening is a resumption and not a restart.
///
/// Every field is optional because view state is a convenience: a
/// project that has never been closed cleanly, or one opened by a build
/// that did not write this, still opens — just at the top.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ViewState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zoom_px_per_sec: Option<f64>,
    /// `[start, end]` in session seconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection: Option<[f64; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub playhead_sec: Option<f64>,
}

/// One entry in the recents list.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RecentProject {
    pub path: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_opened_at: Option<String>,
}

fn view_path(project_dir: &Path) -> PathBuf {
    project_dir.join(session::STORE_DIR).join(VIEW_FILE)
}

/// Record that the project was opened now, creating `project.json` if
/// this is the first time. Returns the metadata as it now stands.
pub fn touch_opened(project_dir: &Path, now: &str) -> ProjectMeta {
    let mut meta = read_meta(project_dir);
    if meta.created_at.is_none() {
        meta.created_at = Some(now.to_string());
    }
    meta.last_opened_at = Some(now.to_string());
    // A metadata write that fails must not stop a project opening —
    // a read-only folder still has readable audio in it.
    let _ = write_meta(project_dir, &meta);
    meta
}

pub fn read_view(project_dir: &Path) -> ViewState {
    std::fs::read_to_string(view_path(project_dir))
        .ok()
        .and_then(|t| serde_json::from_str::<ViewState>(&t).ok())
        .unwrap_or_default()
}

pub fn write_view(project_dir: &Path, view: &ViewState) -> std::io::Result<()> {
    let path = view_path(project_dir);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(view)?)
}

/// Copy the project whose live store is `store` to `dest`, and say how
/// much was written.
///
/// Save As, now that a project folder actually contains its audio
/// (#156). Before the storage layout moved, this could not have worked:
/// derived files lived beside whichever source the user opened, so a
/// copy of the project folder captured the history and none of the
/// sound.
///
/// What travels:
///
/// * `project.json` — the copy is the same project, under a new name on
///   disk. Renaming it is a separate verb and doing it here silently
///   would be a surprise.
/// * `.audiograph/nodes/`, `head`, `trail.json`, `view.json` — the
///   history, the path taken through it (what undo follows, #453) and
///   where you were in it.
/// * `.audiograph/derived/` and `clipboard/` — the audio. Without these
///   the copy is a list of edits pointing at nothing.
/// * Anything else in the folder, the user's own files included.
///
/// What does not: the caches, the folders directly under `.audiograph/`
/// named in [`tools::preview_cache::CACHE_DIRS`] (`previews/` and
/// `auditions/`). Every entry in them re-derives byte-identically on
/// demand, and they are the largest things in the directory, so copying
/// them would make Save As slow in exchange for nothing. The list is the
/// cache code's, so a cache added there is left behind here with no
/// change to this function (#576). A folder of the same name anywhere
/// else is not a cache, and travels.
///
/// ## A snapshot, without stopping the app for the copy (#571)
///
/// Save As answers during a turn, and a copy can be gigabytes, so the
/// project can change while it is copied. Taken file by file with nothing
/// held, a copy could pair a `head` read after an edit with a `nodes/`
/// read before it, or miss audio a copied node plays, and then open and
/// fail on its first read. So it is made in two parts:
///
/// 1. **Under the store's lock**, the store's own state: `nodes/`, the
///    files directly in `.audiograph/` (`head`, `trail.json`, `view.json`)
///    and `project.json`. They are small. No edit is half-way through
///    while the lock is held: one that holds the store keeps the lock
///    until it is done, and one that runs off the lock takes it to commit.
///    So the copy's head is the head at this moment, and its node is
///    beside it. Before letting go, the copy stages a handle on the store
///    ([`Store::stage`]) and keeps it to the end. The handle writes
///    nothing. It counts as in flight ([`Store::staged_in_flight`]), which
///    every deleter of the project's files waits on: the orphan sweep, the
///    background reclaimer and compaction (rule 0 in `tools::reclaim`).
/// 2. **With no lock held**, the rest: the audio and anything else in the
///    folder. Audio is named by its content and, while the handle lives,
///    only added to, so every file a copied node names that was there in
///    part 1 is still there to copy. What an edit adds meanwhile may be
///    copied too. No copied node names it, and the copy's own sweep
///    removes it when the copy is opened.
///
/// So the window and the agent wait for the history to be copied, not
/// the audio. No other lock is taken, the engine's included.
///
/// The copy is checked before it is answered: its `head` is the head of
/// part 1, that node is in the copy, and so is every file the head plays
/// from `derived/` and `clipboard/` that was there in part 1. A copy that
/// fails the check is an error naming what is missing, and `dest` is left
/// as it is.
pub fn copy_project(store: &Mutex<Store>, dest: &Path) -> io::Result<CopyReport> {
    copy_project_with(store, dest, |_| {})
}

/// [`copy_project`], calling `before_each_file` with each file's path in
/// the project (relative to it) before the file is copied. The call is
/// where a test makes the project change during the copy.
pub(crate) fn copy_project_with(
    store: &Mutex<Store>,
    dest: &Path,
    mut before_each_file: impl FnMut(&Path),
) -> io::Result<CopyReport> {
    if dest.exists() && std::fs::read_dir(dest)?.next().is_some() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} already exists and is not empty", dest.display()),
        ));
    }

    // Part 1, under the lock.
    let (mut copy, reading, needed) = {
        let live = store
            .lock()
            .map_err(|_| io::Error::other("the project's store lock is poisoned"))?;
        let mut copy = ProjectCopy {
            src: live.project_dir().to_path_buf(),
            dest,
            report: CopyReport::default(),
            before_each_file: &mut before_each_file,
        };
        let reading = live.stage().map_err(|e| io::Error::other(e.to_string()))?;
        let needed = needed_by_head(&live);
        copy.state()?;
        (copy, reading, needed)
    };

    // Part 2, with no lock held: `reading` keeps the deleters away.
    copy.folder(Path::new(""))?;
    copy.check(reading.head(), &needed)?;
    drop(reading);
    Ok(copy.report)
}

/// What a copy moved, so the caller can say something true about it.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct CopyReport {
    pub files: usize,
    pub bytes: u64,
    /// Files in the project's caches (cached previews and auditions)
    /// deliberately left behind. They rebuild on demand.
    pub skipped_cache_files: usize,
}

/// Which part of a copy a path in the project belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Part {
    /// The store's own state, copied under the store's lock.
    State,
    /// A cache folder, left behind.
    Cache,
    /// Everything else, copied with no lock held.
    Rest,
}

/// The part `rel`, a path relative to the project, belongs to.
fn part_of(rel: &Path, is_dir: bool) -> Part {
    let name = rel.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    if rel.parent() == Some(Path::new(session::STORE_DIR)) {
        if !is_dir || name == session::store::NODES_DIR {
            return Part::State;
        }
        if tools::preview_cache::CACHE_DIRS.contains(&name) {
            return Part::Cache;
        }
    }
    if rel == Path::new(PROJECT_FILE) && !is_dir {
        return Part::State;
    }
    Part::Rest
}

/// What a copy of the project cannot be without, relative to it: the
/// head's node file, and the files the head plays from `derived/` and
/// `clipboard/`. Only what is there now counts, so a copy is never
/// refused for a file the project had already lost (history audio a sweep
/// removed, which a replay puts back). Read under the store's lock.
fn needed_by_head(store: &Store) -> Vec<PathBuf> {
    let Some(head) = store.head() else {
        return Vec::new();
    };
    let src = store.project_dir();
    let mut needed = vec![session::store::node_path(src, &head.to_hex())];
    if let Ok(node) = store.get(head) {
        let audio = [
            session::relocate::derived_dir(src),
            session::relocate::clipboard_dir(src),
        ];
        needed.extend(
            node.state
                .tracks
                .iter()
                .flat_map(|t| &t.clips)
                .map(|c| c.source_path.clone())
                .filter(|path| audio.iter().any(|dir| path.starts_with(dir))),
        );
    }
    needed
        .into_iter()
        .filter(|path| path.is_file())
        .filter_map(|path| path.strip_prefix(src).ok().map(Path::to_path_buf))
        .collect()
}

/// One copy in progress, from the project at `src` to `dest`.
struct ProjectCopy<'a> {
    src: PathBuf,
    dest: &'a Path,
    report: CopyReport,
    before_each_file: &'a mut dyn FnMut(&Path),
}

impl ProjectCopy<'_> {
    /// Part 1: what [`part_of`] calls the store's own state.
    fn state(&mut self) -> io::Result<()> {
        std::fs::create_dir_all(self.dest)?;
        let meta = Path::new(PROJECT_FILE);
        if self.src.join(meta).is_file() {
            self.file(meta)?;
        }
        let store = Path::new(session::STORE_DIR);
        std::fs::create_dir_all(self.dest.join(store))?;
        for rel in self.list(store)? {
            let from = self.src.join(&rel);
            let is_dir = from.is_dir();
            if part_of(&rel, is_dir) != Part::State {
                continue;
            }
            if is_dir {
                self.folder(&rel)?;
            } else if from.is_file() {
                self.file(&rel)?;
            }
        }
        Ok(())
    }

    /// The folder `rel` and everything in it, but the store's state, which
    /// part 1 copied, and the caches, which are counted and left.
    fn folder(&mut self, rel: &Path) -> io::Result<()> {
        std::fs::create_dir_all(self.dest.join(rel))?;
        for rel in self.list(rel)? {
            let from = self.src.join(&rel);
            // A copy made inside the project is not copied into itself.
            if from == self.dest {
                continue;
            }
            let is_dir = from.is_dir();
            match part_of(&rel, is_dir) {
                Part::State => {}
                Part::Cache => self.report.skipped_cache_files += count_files(&from),
                Part::Rest if is_dir => self.folder(&rel)?,
                Part::Rest if from.is_file() => self.file(&rel)?,
                Part::Rest => {}
            }
        }
        Ok(())
    }

    /// What is in the folder `rel`, as paths relative to the project. A
    /// folder that has gone is empty.
    fn list(&self, rel: &Path) -> io::Result<Vec<PathBuf>> {
        match std::fs::read_dir(self.src.join(rel)) {
            Ok(entries) => entries
                .map(|entry| entry.map(|e| rel.join(e.file_name())))
                .collect(),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(e),
        }
    }

    /// Copy the file `rel`, unless it has gone since it was listed: a
    /// staging file renamed into place, most likely. What the copy cannot
    /// be without is checked at the end.
    fn file(&mut self, rel: &Path) -> io::Result<()> {
        (self.before_each_file)(rel);
        let from = self.src.join(rel);
        match std::fs::copy(&from, self.dest.join(rel)) {
            Ok(bytes) => {
                self.report.files += 1;
                self.report.bytes += bytes;
                Ok(())
            }
            Err(_) if !from.exists() => Ok(()),
            Err(e) => Err(e),
        }
    }

    /// That the copy holds `head` as its head, and every file in `needed`.
    fn check(&self, head: Option<session::NodeId>, needed: &[PathBuf]) -> io::Result<()> {
        let not_whole = |what: String| {
            io::Error::other(format!(
                "the copy at {} is not whole: {what}. It was left as it is",
                self.dest.display()
            ))
        };
        if let Some(head) = head {
            let hex = head.to_hex();
            let file = self
                .dest
                .join(session::STORE_DIR)
                .join(session::store::HEAD_FILE);
            let copied = std::fs::read_to_string(file).unwrap_or_default();
            if copied.trim() != hex {
                return Err(not_whole(format!(
                    "its head is `{}`, where the project's was {hex}",
                    copied.trim()
                )));
            }
        }
        for rel in needed {
            if !self.dest.join(rel).is_file() {
                return Err(not_whole(format!(
                    "it has no {}, which its head needs",
                    rel.display()
                )));
            }
        }
        Ok(())
    }
}

/// How many files are under `dir`, at any depth. What cannot be read
/// counts as none.
fn count_files(dir: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| {
            let path = entry.path();
            if path.is_dir() {
                count_files(&path)
            } else {
                usize::from(path.is_file())
            }
        })
        .sum()
}

/// Where the recents list lives — outside every project, since a list
/// of projects cannot live inside one of them.
pub fn recents_path(home: &Path) -> PathBuf {
    home.join(".edytlab").join("recents.json")
}

pub fn read_recents(home: &Path) -> Vec<RecentProject> {
    std::fs::read_to_string(recents_path(home))
        .ok()
        .and_then(|t| serde_json::from_str::<Vec<RecentProject>>(&t).ok())
        .unwrap_or_default()
}

/// Put `entry` at the front, deduplicating by path and capping the
/// list.
///
/// Most-recent-first, and one entry per project: reopening a project
/// moves it to the top rather than adding a second row for the same
/// folder, which is what makes the list usable after a week.
pub fn push_recent(home: &Path, entry: RecentProject) -> std::io::Result<Vec<RecentProject>> {
    let mut list = read_recents(home);
    list.retain(|r| r.path != entry.path);
    list.insert(0, entry);
    list.truncate(MAX_RECENTS);

    let path = recents_path(home);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, serde_json::to_string_pretty(&list)?)?;
    Ok(list)
}

/// Drop an entry — for a project the user moved or deleted, where the
/// row is now a dead link.
pub fn forget_recent(home: &Path, project_path: &str) -> std::io::Result<Vec<RecentProject>> {
    let mut list = read_recents(home);
    list.retain(|r| r.path != project_path);
    let path = recents_path(home);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, serde_json::to_string_pretty(&list)?)?;
    Ok(list)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn a_project_with_no_metadata_takes_its_folder_name() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("My Podcast");
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(read_meta(&dir).name, "My Podcast");
    }

    /// The name is a project's own, not its folder's, once set — that
    /// is the whole point of having the file.
    #[test]
    fn a_name_survives_a_round_trip() {
        let tmp = TempDir::new().unwrap();
        let mut meta = ProjectMeta::from_dir(tmp.path());
        meta.name = "Episode 12 — mixdown".to_string();
        meta.notes = "needs a re-record at 4:10".to_string();
        write_meta(tmp.path(), &meta).unwrap();

        let read = read_meta(tmp.path());
        assert_eq!(read.name, "Episode 12 — mixdown");
        assert_eq!(read.notes, "needs a re-record at 4:10");
    }

    /// A hand-edited `project.json` must not stop a project opening.
    /// The audio and its history are not in that file.
    #[test]
    fn a_corrupt_metadata_file_falls_back_rather_than_failing() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join(PROJECT_FILE), "{ not json").unwrap();
        let meta = read_meta(tmp.path());
        assert!(!meta.name.is_empty());
    }

    #[test]
    fn opening_records_created_once_and_opened_every_time() {
        let tmp = TempDir::new().unwrap();
        let first = touch_opened(tmp.path(), "2026-01-01T00:00:00Z");
        assert_eq!(first.created_at.as_deref(), Some("2026-01-01T00:00:00Z"));

        let second = touch_opened(tmp.path(), "2026-02-02T00:00:00Z");
        assert_eq!(
            second.created_at.as_deref(),
            Some("2026-01-01T00:00:00Z"),
            "created_at must not move"
        );
        assert_eq!(
            second.last_opened_at.as_deref(),
            Some("2026-02-02T00:00:00Z")
        );
    }

    #[test]
    fn view_state_round_trips_and_defaults_when_absent() {
        let tmp = TempDir::new().unwrap();
        assert_eq!(read_view(tmp.path()), ViewState::default());

        let view = ViewState {
            head: Some("abc".into()),
            zoom_px_per_sec: Some(120.0),
            selection: Some([1.5, 4.25]),
            playhead_sec: Some(2.0),
        };
        write_view(tmp.path(), &view).unwrap();
        assert_eq!(read_view(tmp.path()), view);
    }

    fn recent(path: &str, name: &str) -> RecentProject {
        RecentProject {
            path: path.to_string(),
            name: name.to_string(),
            last_opened_at: None,
        }
    }

    /// Most recent first, one row per project. Reopening moves a
    /// project up rather than adding a duplicate.
    #[test]
    fn recents_are_most_recent_first_and_deduplicated() {
        let home = TempDir::new().unwrap();
        push_recent(home.path(), recent("/a", "A")).unwrap();
        push_recent(home.path(), recent("/b", "B")).unwrap();
        let list = push_recent(home.path(), recent("/a", "A renamed")).unwrap();

        assert_eq!(list.len(), 2, "reopening must not add a second row");
        assert_eq!(list[0].path, "/a");
        assert_eq!(list[0].name, "A renamed", "the newer name wins");
        assert_eq!(list[1].path, "/b");
    }

    #[test]
    fn recents_are_capped() {
        let home = TempDir::new().unwrap();
        for i in 0..MAX_RECENTS + 5 {
            push_recent(home.path(), recent(&format!("/p{i}"), "p")).unwrap();
        }
        let list = read_recents(home.path());
        assert_eq!(list.len(), MAX_RECENTS);
        assert_eq!(
            list[0].path,
            format!("/p{}", MAX_RECENTS + 4),
            "the newest must survive the cap"
        );
    }

    #[test]
    fn a_recent_can_be_forgotten() {
        let home = TempDir::new().unwrap();
        push_recent(home.path(), recent("/a", "A")).unwrap();
        push_recent(home.path(), recent("/b", "B")).unwrap();
        let list = forget_recent(home.path(), "/a").unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].path, "/b");
    }

    /// Copy the project at `src` to `dest` as Save As does, through a live
    /// store on it.
    fn copy_of(src: &Path, dest: &Path) -> io::Result<CopyReport> {
        let store = Mutex::new(session::Store::open(src).expect("open the original"));
        copy_project(&store, dest)
    }

    /// Save As only works because a project contains its own audio
    /// now. The copy has to carry the history *and* the sound.
    #[test]
    fn a_copy_takes_the_history_and_the_audio() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("original");
        let store = src.join(session::STORE_DIR);
        std::fs::create_dir_all(store.join("nodes")).unwrap();
        std::fs::create_dir_all(store.join("derived")).unwrap();
        std::fs::create_dir_all(store.join("clipboard")).unwrap();
        std::fs::create_dir_all(store.join(tools::preview_cache::CACHE_DIR)).unwrap();
        std::fs::create_dir_all(store.join(tools::tool::audition::AUDITION_DIR)).unwrap();
        std::fs::write(src.join(PROJECT_FILE), "{\"name\":\"Episode 12\"}").unwrap();
        let edit = store.join("derived").join("edit.wav");
        let blob = store.join("clipboard").join("c.wav");
        std::fs::write(&edit, vec![0u8; 2048]).unwrap();
        std::fs::write(&blob, vec![0u8; 512]).unwrap();
        let head = session::Store::open(&src)
            .unwrap()
            .append(playing(&[edit, blob], 1))
            .unwrap()
            .to_hex();
        // Two cached previews, which must not travel.
        std::fs::write(
            store.join(tools::preview_cache::CACHE_DIR).join("p1.wav"),
            vec![0u8; 4096],
        )
        .unwrap();
        std::fs::write(
            store.join(tools::preview_cache::CACHE_DIR).join("p2.wav"),
            vec![0u8; 4096],
        )
        .unwrap();
        // And two auditions, a cache of the same kind (#576).
        for name in ["a1.wav", "a2.wav"] {
            std::fs::write(
                store.join(tools::tool::audition::AUDITION_DIR).join(name),
                vec![0u8; 4096],
            )
            .unwrap();
        }

        let dest = tmp.path().join("copy");
        let report = copy_of(&src, &dest).expect("copy");

        assert!(dest.join(PROJECT_FILE).is_file(), "the project itself");
        assert!(
            session::store::node_path(&dest, &head).is_file(),
            "its history"
        );
        assert!(
            dest.join(session::STORE_DIR)
                .join("derived")
                .join("edit.wav")
                .is_file(),
            "and the audio that history points at"
        );
        assert!(
            dest.join(session::STORE_DIR)
                .join("clipboard")
                .join("c.wav")
                .is_file(),
            "including the clipboard blobs a paste depends on"
        );

        // The caches are what is left behind: they are the biggest
        // directories and every entry re-derives on demand.
        assert!(
            !dest
                .join(session::STORE_DIR)
                .join(tools::preview_cache::CACHE_DIR)
                .exists(),
            "the preview cache must not be copied"
        );
        assert!(
            !dest
                .join(session::STORE_DIR)
                .join(tools::tool::audition::AUDITION_DIR)
                .exists(),
            "the audition cache must not be copied"
        );
        assert_eq!(report.skipped_cache_files, 4);
        assert_eq!(report.files, 5);
        assert!(report.bytes >= 2048 + 512);
    }

    /// The promise Save As actually makes: the copy is a project, not a
    /// second window onto the first one.
    ///
    /// The bytes travel (the test above), but the nodes name the
    /// *original's* absolute paths — so until `session::relocate`, a
    /// copy played only while the folder it came from still existed,
    /// and the audio sitting in its own `derived/` was never reached.
    /// Deleting the original is the honest test of "self-contained".
    #[test]
    fn a_copy_still_plays_after_the_original_is_deleted() {
        use session::{Clip, SessionNode, SessionState, Store, Track, TrackId};

        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("original");
        std::fs::create_dir_all(&src).unwrap();

        // A derived file where the storage layout now puts it (#190),
        // named by content as the CAS writes it.
        let derived = session::relocate::derived_dir(&src);
        std::fs::create_dir_all(&derived).unwrap();
        let audio = derived.join("9f3c0000.wav");
        std::fs::write(&audio, vec![0u8; 1024]).unwrap();

        let mut store = Store::open(&src).unwrap();
        store
            .append(SessionNode {
                id: session::NodeId([0u8; 32]),
                parent: None,
                created_at: chrono::Utc::now(),
                label: Some("an edit".into()),
                reasoning: None,
                state: SessionState {
                    tracks: vec![Track {
                        id: TrackId(uuid::Uuid::new_v4()),
                        name: "Vox".into(),
                        clips: vec![Clip {
                            source_path: audio.clone(),
                            start_in_track: 0,
                            source_offset: 0,
                            length: 256,
                            content_hash: None,
                            time_stretch_factor: None,
                            pitch_shift_semitones: None,
                            beat_grid: None,
                            volume_envelope: Vec::new(),
                        }],
                        gain_db: 0.0,
                        pan: 0.0,
                        muted: false,
                        soloed: false,
                        effects: Vec::new(),
                        sends: Vec::new(),
                    }],
                    bus_routing: Default::default(),
                    master_chain: Vec::new(),
                    tempo_map: Default::default(),
                    key_map: None,
                    transcript: None,
                    sample_rate: 48_000,
                    length_samples: 256,
                    annotations: Vec::new(),
                    sync_lock: false,
                },
                op: None,
            })
            .unwrap();
        drop(store);

        let dest = tmp.path().join("copy");
        copy_of(&src, &dest).expect("copy");
        std::fs::remove_dir_all(&src).expect("the original is gone");

        let copy = Store::open(&dest).unwrap();
        let head = copy.head().expect("the copy has a head");
        let node = copy.get(head).expect("and the node reads back");
        let path = &node.state.tracks[0].clips[0].source_path;

        assert!(
            path.is_file(),
            "the copy must reach its own audio, not the deleted original's: {}",
            path.display()
        );
        assert!(
            path.starts_with(&dest),
            "and it must be the copy's file, not something else on disk: {}",
            path.display()
        );
    }

    /// Undo follows the path taken (#453), so a copy that carried the
    /// history and not the path would undo differently from the project it
    /// was made from. `trail.json` is in `.audiograph/`, which the copy
    /// takes whole.
    #[test]
    fn a_copy_keeps_the_path_taken() {
        use session::trail::{read_trail, write_trail, HeadTrail};
        use session::{SessionNode, SessionState, Store};

        let node = |length_samples: u64| SessionNode {
            id: session::NodeId([0u8; 32]),
            parent: None,
            created_at: chrono::Utc::now(),
            label: None,
            reasoning: None,
            state: SessionState {
                tracks: Vec::new(),
                bus_routing: Default::default(),
                master_chain: Vec::new(),
                tempo_map: Default::default(),
                key_map: None,
                transcript: None,
                sample_rate: 48_000,
                length_samples,
                annotations: Vec::new(),
                sync_lock: false,
            },
            op: None,
        };

        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("original");
        let mut store = Store::open(&src).unwrap();
        let a = store.append(node(1_000)).unwrap().to_hex();
        let b = store.append(node(2_000)).unwrap().to_hex();
        // Unmuted: back on `a`, having been on `b`.
        let saved = HeadTrail {
            head: Some(a.clone()),
            trail: vec![a.clone(), b.clone()],
            redo: vec![b],
        };
        assert!(write_trail(&src, &saved).unwrap());
        drop(store);

        let dest = tmp.path().join("copy");
        copy_of(&src, &dest).expect("copy");
        std::fs::remove_dir_all(&src).expect("the original is gone");

        assert_eq!(read_trail(&dest), saved);
    }

    /// Copying into somebody's existing folder would merge two projects
    /// into one and corrupt both.
    #[test]
    fn a_copy_refuses_a_destination_that_is_not_empty() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("original");
        std::fs::create_dir_all(&src).unwrap();
        let dest = tmp.path().join("occupied");
        std::fs::create_dir_all(&dest).unwrap();
        std::fs::write(dest.join("someone-elses.txt"), "hello").unwrap();

        let err = copy_of(&src, &dest).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
        assert!(
            dest.join("someone-elses.txt").is_file(),
            "and touched nothing"
        );
    }

    #[test]
    fn a_copy_into_a_fresh_path_creates_it() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("original");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join(PROJECT_FILE), "{}").unwrap();
        let dest = tmp.path().join("nested").join("copy");
        copy_of(&src, &dest).expect("copy into a path that does not exist yet");
        assert!(dest.join(PROJECT_FILE).is_file());
    }

    /// A folder is a cache because of where it is, not what it is called
    /// (#576): only `.audiograph/previews/` and `.audiograph/auditions/` are
    /// the app's caches. A `previews` folder of the user's own in the project
    /// folder, or one under `derived/`, is the project's and travels.
    #[test]
    fn a_folder_named_like_a_cache_elsewhere_is_copied() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("original");
        let store = src.join(session::STORE_DIR);
        let derived = session::relocate::derived_dir(&src);
        let elsewhere = [
            src.join("previews").join("my-mixdown.wav"),
            src.join("auditions").join("take-3.wav"),
            derived.join("previews").join("kept.wav"),
            derived.join("auditions").join("kept.wav"),
        ];
        for file in &elsewhere {
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, vec![0u8; 256]).unwrap();
        }
        std::fs::create_dir_all(store.join(tools::preview_cache::CACHE_DIR)).unwrap();
        std::fs::write(
            store.join(tools::preview_cache::CACHE_DIR).join("p.wav"),
            vec![0u8; 256],
        )
        .unwrap();

        let dest = tmp.path().join("copy");
        copy_of(&src, &dest).expect("copy");

        for file in &elsewhere {
            let copied = dest.join(file.strip_prefix(&src).unwrap());
            assert!(
                copied.is_file(),
                "{} is not a cache and was left out of the copy",
                copied.display()
            );
        }
        assert!(!dest
            .join(session::STORE_DIR)
            .join(tools::preview_cache::CACHE_DIR)
            .exists());
    }

    /// A node with one track playing `audio`, one clip per file, and a length
    /// that gives it an id of its own.
    fn playing(audio: &[PathBuf], length_samples: u64) -> session::SessionNode {
        use session::{Clip, SessionNode, SessionState, Track, TrackId};
        SessionNode {
            id: session::NodeId([0u8; 32]),
            parent: None,
            created_at: chrono::Utc::now(),
            label: None,
            reasoning: None,
            state: SessionState {
                tracks: vec![Track {
                    id: TrackId(uuid::Uuid::nil()),
                    name: "Vox".into(),
                    clips: audio
                        .iter()
                        .map(|path| Clip {
                            source_path: path.clone(),
                            start_in_track: 0,
                            source_offset: 0,
                            length: 256,
                            content_hash: None,
                            time_stretch_factor: None,
                            pitch_shift_semitones: None,
                            beat_grid: None,
                            volume_envelope: Vec::new(),
                        })
                        .collect(),
                    gain_db: 0.0,
                    pan: 0.0,
                    muted: false,
                    soloed: false,
                    effects: Vec::new(),
                    sends: Vec::new(),
                }],
                bus_routing: Default::default(),
                master_chain: Vec::new(),
                tempo_map: Default::default(),
                key_map: None,
                transcript: None,
                sample_rate: 48_000,
                length_samples,
                annotations: Vec::new(),
                sync_lock: false,
            },
            op: None,
        }
    }

    /// A project whose head plays a file in `derived/` and a clipboard
    /// blob, as its live store; and that head.
    struct Original {
        dir: PathBuf,
        store: std::sync::Mutex<session::Store>,
        head: session::NodeId,
        /// The audio the head plays from `derived/`.
        audio: PathBuf,
    }

    fn original(tmp: &TempDir) -> Original {
        let dir = tmp.path().join("original");
        let derived = session::relocate::derived_dir(&dir);
        let clipboard = session::relocate::clipboard_dir(&dir);
        std::fs::create_dir_all(&derived).unwrap();
        std::fs::create_dir_all(&clipboard).unwrap();
        let audio = derived.join("first.wav");
        let blob = clipboard.join("blob.wav");
        std::fs::write(&audio, vec![1u8; 1024]).unwrap();
        std::fs::write(&blob, vec![2u8; 512]).unwrap();
        let mut store = session::Store::open(&dir).unwrap();
        let head = store.append(playing(&[audio.clone(), blob], 1)).unwrap();
        Original {
            dir,
            store: std::sync::Mutex::new(store),
            head,
            audio,
        }
    }

    /// `path` inside the project's store directory, as the copy names the
    /// files it is about to copy.
    fn in_store(path: &str) -> PathBuf {
        Path::new(session::STORE_DIR).join(path)
    }

    /// Save As during an edit (#571). The copy used to be taken file by
    /// file with nothing held, so an edit that landed part way could leave a
    /// copy whose head names a node the copy does not have, or a node whose
    /// audio it does not have; it opened, and failed on its first read.
    ///
    /// Here an edit lands just before the copy reaches `head` and another
    /// just after it, each made the way a tool that runs off the store's
    /// lock makes one (staged, its audio written with no lock held, then
    /// committed) and each followed by what deletes from `derived/`: the
    /// orphan sweep, and a compaction down to the newest node. An edit that
    /// finds the store's lock taken waits for it, as every writer does.
    ///
    /// The copy is the project as it was when the copy started, and it
    /// plays on its own once the original is gone. The audio is copied with
    /// the store's lock free, so a long copy does not stop every command.
    #[test]
    fn a_copy_is_the_project_as_it_was_when_the_copy_started() {
        use session::{Commit, Store};
        let tmp = TempDir::new().unwrap();
        let p = original(&tmp);
        let derived = session::relocate::derived_dir(&p.dir);

        let head_file = in_store("head");
        let mut edits_due = 0;
        let mut edits_landed = 0u64;
        let (mut reached_head, mut past_head) = (false, false);
        let mut audio_copied_under_the_lock = Vec::new();
        let dest = tmp.path().join("copy");
        copy_project_with(&p.store, &dest, |file| {
            if file == head_file {
                reached_head = true;
                edits_due += 1;
            } else if reached_head && !past_head {
                past_head = true;
                edits_due += 1;
            }
            if file.starts_with(in_store("derived")) && p.store.try_lock().is_err() {
                audio_copied_under_the_lock.push(file.to_path_buf());
            }
            while edits_due > 0 {
                let Ok(live) = p.store.try_lock() else {
                    break;
                };
                edits_landed += 1;
                let mut run = live.stage().unwrap();
                drop(live);
                let audio = derived.join(format!("edit-{edits_landed}.wav"));
                std::fs::write(&audio, vec![3u8; 1024]).unwrap();
                run.append(playing(&[audio], 1 + edits_landed)).unwrap();
                let mut live = p.store.lock().unwrap();
                assert_eq!(live.commit(run).unwrap(), Commit::Published);
                let _ = tools::reclaim::sweep_orphans(&live);
                let head = live.head().unwrap();
                let plan = tools::reclaim::plan_compaction(&live, head, 1).unwrap();
                let _ = tools::reclaim::apply_compaction(&mut live, &plan);
                edits_due -= 1;
            }
        })
        .expect("copy");

        drop(p.store);
        std::fs::remove_dir_all(&p.dir).expect("the original is gone");
        let copy = Store::open(&dest).unwrap();
        let head = copy.head().expect("the copy has a head");
        if let Err(e) = copy.get(head) {
            panic!(
                "the copy's head {} is not a node in the copy: {e}",
                head.to_hex()
            );
        }
        assert_eq!(
            head, p.head,
            "the copy's head is not the head the project had when the copy started"
        );
        for node in copy.list_nodes().unwrap() {
            for clip in node.state.tracks.iter().flat_map(|t| &t.clips) {
                assert!(
                    clip.source_path.is_file() && clip.source_path.starts_with(&dest),
                    "node {} in the copy names audio the copy does not have: {}",
                    node.id.to_hex(),
                    clip.source_path.display()
                );
            }
        }
        assert_eq!(
            edits_landed, 2,
            "the edits did not land during the copy, so this shows nothing"
        );
        assert!(
            audio_copied_under_the_lock.is_empty(),
            "the audio was copied holding the store's lock, which stops every command \
             for as long as the copy takes: {audio_copied_under_the_lock:?}"
        );
    }

    /// The copy is checked before it is opened: a copy whose head names a
    /// node it does not have is an error, not a project that opens and
    /// fails on its first read (#571).
    #[test]
    fn a_copy_without_its_head_node_is_an_error() {
        let tmp = TempDir::new().unwrap();
        let p = original(&tmp);
        let hex = p.head.to_hex();
        let dest = tmp.path().join("copy");
        let copied_node = dest
            .join(session::STORE_DIR)
            .join("nodes")
            .join(&hex[0..2])
            .join(format!("{hex}.json"));

        // Lost from the copy once the audio is being copied, whatever took it.
        let result = copy_project_with(&p.store, &dest, |file| {
            if file.starts_with(in_store("derived")) && copied_node.is_file() {
                std::fs::remove_file(&copied_node).unwrap();
            }
        });

        let err = result.expect_err("a copy without its head node was answered as a project");
        assert!(err.to_string().contains(&hex), "{err}");
    }

    /// Likewise a copy without audio its head plays, which was there when
    /// the copy started and is gone before it was copied: an error naming
    /// the file, not a copy that plays silence there (#571).
    #[test]
    fn a_copy_without_audio_its_head_plays_is_an_error() {
        let tmp = TempDir::new().unwrap();
        let p = original(&tmp);
        let name = in_store("derived").join("first.wav");
        let dest = tmp.path().join("copy");

        let result = copy_project_with(&p.store, &dest, |file| {
            if file == name {
                std::fs::remove_file(&p.audio).unwrap();
            }
        });

        let err = result.expect_err("a copy without its head's audio was answered as a project");
        assert!(err.to_string().contains("first.wav"), "{err}");
    }

    #[test]
    fn a_corrupt_recents_file_reads_as_empty() {
        let home = TempDir::new().unwrap();
        let p = recents_path(home.path());
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, "[[[").unwrap();
        assert!(read_recents(home.path()).is_empty());
        // And writing over it recovers rather than compounding.
        let list = push_recent(home.path(), recent("/a", "A")).unwrap();
        assert_eq!(list.len(), 1);
    }
}
