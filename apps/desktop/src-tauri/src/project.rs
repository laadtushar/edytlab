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

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Metadata file name, beside `.audiograph/`.
pub const PROJECT_FILE: &str = "project.json";
/// View state, inside `.audiograph/`.
pub const VIEW_FILE: &str = "view.json";
/// How many recent projects to remember. Ten is about a screen.
pub const MAX_RECENTS: usize = 10;

/// What a project is, as opposed to where it is.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProjectMeta {
    /// Human name. Defaults to the folder's name, which is a decent
    /// first guess and a terrible permanent answer.
    pub name: String,
    /// ISO 8601. Absent in files written before this existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_opened_at: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
}

impl ProjectMeta {
    /// A project that has never been named takes its folder's name.
    pub fn from_dir(dir: &Path) -> Self {
        Self {
            name: dir
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("Untitled")
                .to_string(),
            created_at: None,
            last_opened_at: None,
            notes: String::new(),
        }
    }
}

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

fn meta_path(project_dir: &Path) -> PathBuf {
    project_dir.join(PROJECT_FILE)
}

fn view_path(project_dir: &Path) -> PathBuf {
    project_dir.join(session::STORE_DIR).join(VIEW_FILE)
}

/// Read `project.json`, falling back to a folder-named default.
///
/// A missing file is the normal case for a project created before this
/// existed. A corrupt one is treated the same way: the name is not
/// worth refusing to open a session over.
pub fn read_meta(project_dir: &Path) -> ProjectMeta {
    std::fs::read_to_string(meta_path(project_dir))
        .ok()
        .and_then(|t| serde_json::from_str::<ProjectMeta>(&t).ok())
        .unwrap_or_else(|| ProjectMeta::from_dir(project_dir))
}

pub fn write_meta(project_dir: &Path, meta: &ProjectMeta) -> std::io::Result<()> {
    let text = serde_json::to_string_pretty(meta)?;
    std::fs::write(meta_path(project_dir), text)
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

/// Copy a project to `dest`, and say how much was written.
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
/// * `.audiograph/nodes/`, `head`, `view.json` — the history and where
///   you were in it.
/// * `.audiograph/derived/` and `clipboard/` — the audio. Without these
///   the copy is a list of edits pointing at nothing.
///
/// What does not: `.audiograph/previews/`. It is a cache keyed by node
/// id, every entry re-derives byte-identically on demand, and it is the
/// largest thing in the directory. Copying it would make Save As slow
/// in exchange for nothing.
pub fn copy_project(src: &Path, dest: &Path) -> std::io::Result<CopyReport> {
    if dest.exists() && std::fs::read_dir(dest)?.next().is_some() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!("{} already exists and is not empty", dest.display()),
        ));
    }
    let mut report = CopyReport::default();
    copy_dir(src, dest, &mut report)?;
    Ok(report)
}

/// What a copy moved, so the caller can say something true about it.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct CopyReport {
    pub files: usize,
    pub bytes: u64,
    /// Preview-cache files deliberately left behind.
    pub skipped_previews: usize,
}

fn copy_dir(src: &Path, dest: &Path, report: &mut CopyReport) -> std::io::Result<()> {
    std::fs::create_dir_all(dest)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let name = entry.file_name();
        let to = dest.join(&name);

        if from.is_dir() {
            // The preview cache is the one thing worth leaving: it is a
            // cache, it is the biggest directory, and every entry
            // rebuilds byte-identically from the node it is named for.
            if name == std::ffi::OsStr::new(tools::preview_cache::CACHE_DIR) {
                report.skipped_previews += std::fs::read_dir(&from)
                    .map(|d| d.flatten().filter(|e| e.path().is_file()).count())
                    .unwrap_or(0);
                continue;
            }
            copy_dir(&from, &to, report)?;
        } else if from.is_file() {
            let bytes = std::fs::copy(&from, &to)?;
            report.files += 1;
            report.bytes += bytes;
        }
    }
    Ok(())
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
        std::fs::write(src.join(PROJECT_FILE), "{\"name\":\"Episode 12\"}").unwrap();
        std::fs::write(store.join("head"), "abc").unwrap();
        std::fs::write(store.join("nodes").join("a.json"), "{}").unwrap();
        std::fs::write(store.join("derived").join("edit.wav"), vec![0u8; 2048]).unwrap();
        std::fs::write(store.join("clipboard").join("c.wav"), vec![0u8; 512]).unwrap();
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

        let dest = tmp.path().join("copy");
        let report = copy_project(&src, &dest).expect("copy");

        assert!(dest.join(PROJECT_FILE).is_file(), "the project itself");
        assert!(
            dest.join(session::STORE_DIR)
                .join("nodes")
                .join("a.json")
                .is_file(),
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

        // The cache is the one thing left behind: it is the biggest
        // directory and every entry re-derives on demand.
        assert!(
            !dest
                .join(session::STORE_DIR)
                .join(tools::preview_cache::CACHE_DIR)
                .exists(),
            "the preview cache must not be copied"
        );
        assert_eq!(report.skipped_previews, 2);
        assert_eq!(report.files, 5);
        assert!(report.bytes >= 2048 + 512);
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

        let err = copy_project(&src, &dest).unwrap_err();
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
        copy_project(&src, &dest).expect("copy into a path that does not exist yet");
        assert!(dest.join(PROJECT_FILE).is_file());
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
