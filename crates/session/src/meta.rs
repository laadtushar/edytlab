//! What a project *is*, as opposed to where it is.
//!
//! This lived in the desktop crate, which is the natural home for it
//! right up until something below the app needs to read it. `#225 §5`
//! made `render_final` default its export tags from the project, and
//! `tools` cannot depend on `edytlab-desktop` — the dependency runs
//! the other way.
//!
//! Three ways out, and this is the least bad:
//!
//! * duplicate a partial struct in `tools` and pin the two together
//!   with a test. The repo does this for prose (`website_tool_docs`,
//!   `settings_provider_list`), but those pin *documentation* against
//!   code. Two definitions of one on-disk format is a different thing:
//!   a drift between them is a file that round-trips wrong, not a
//!   sentence that reads wrong.
//! * carry the defaults on `ToolContext`, which is where app-supplied
//!   policy already lives. Correct, and it costs 128 edits because
//!   every construction site must name the new field.
//! * move the type to a crate both sides already depend on. `Store`
//!   owns `project_dir` and everything written under it, so the file
//!   beside `.audiograph/` is within what this crate already knows
//!   about.
//!
//! `apps/desktop/src-tauri/src/project.rs` re-exports these, so every
//! existing caller keeps working unchanged and the app keeps its own
//! view/recents types next to them.
//!
//! Nothing here is load-bearing for audio. Every read tolerates a
//! missing or corrupt file by returning defaults: a `project.json`
//! hand-edited into nonsense must still open, because the audio and
//! its history are not in that file.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Metadata file name, beside `.audiograph/`.
pub const PROJECT_FILE: &str = "project.json";

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
    /// Tag defaults for exports (#225 §5).
    ///
    /// `render_final` writes `title`/`artist`/`album`/`year`/`comment`
    /// into the output file and took every one of them as a per-export
    /// argument — so the same answers had to be given again on every
    /// export, and only by asking the agent in a sentence. These are
    /// the ones that belong to the *project* rather than to one render:
    /// an episode's artist does not change between exports.
    ///
    /// `title` is deliberately absent. The project's `name` already is
    /// the title, and a second field holding the same thing is a pair
    /// that can disagree.
    ///
    /// All `serde(default)`, so a `project.json` written before this
    /// existed reads back with them empty rather than failing.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub artist: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub album: String,
    /// A string, not a number: this is written verbatim into a tag, and
    /// `render_final` already takes it as a string.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub year: String,
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
            artist: String::new(),
            album: String::new(),
            year: String::new(),
        }
    }

    /// Whether this project has any export tag to contribute.
    ///
    /// A project that has never been given one should not make
    /// `render_final` behave differently from before it could read
    /// this at all.
    pub fn has_tag_defaults(&self) -> bool {
        !self.artist.is_empty() || !self.album.is_empty() || !self.year.is_empty()
    }
}

pub fn meta_path(project_dir: &Path) -> PathBuf {
    project_dir.join(PROJECT_FILE)
}

/// Read `project.json`, falling back to a name derived from the folder.
///
/// Never fails: a missing or unparseable file is the same as one that
/// has never been written.
pub fn read_meta(project_dir: &Path) -> ProjectMeta {
    std::fs::read_to_string(meta_path(project_dir))
        .ok()
        .and_then(|t| serde_json::from_str::<ProjectMeta>(&t).ok())
        .unwrap_or_else(|| ProjectMeta::from_dir(project_dir))
}

/// The project's own metadata, or `None` if it has never been written.
///
/// [`read_meta`] cannot answer this: it synthesises a name from the
/// folder when the file is missing, which is right for *showing* a
/// project and wrong for *inheriting* from one. A directory called
/// `take3` is not a title anybody chose, and writing it into every
/// exported file would be an answer invented on the user's behalf.
///
/// So the distinction is explicit — "this project says nothing" is a
/// different fact from "this project is called take3" (#225 §5).
pub fn read_meta_if_present(project_dir: &Path) -> Option<ProjectMeta> {
    std::fs::read_to_string(meta_path(project_dir))
        .ok()
        .and_then(|t| serde_json::from_str::<ProjectMeta>(&t).ok())
}

pub fn write_meta(project_dir: &Path, meta: &ProjectMeta) -> std::io::Result<()> {
    let text = serde_json::to_string_pretty(meta)?;
    std::fs::write(meta_path(project_dir), text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_written_before_the_tag_fields_existed_still_reads() {
        // The back-compat that `serde(default)` buys. A project.json
        // from before #225 §5 has none of these keys.
        let old = r#"{"name":"Episode 4","notes":"rough cut"}"#;
        let meta: ProjectMeta = serde_json::from_str(old).expect("old project.json");
        assert_eq!(meta.name, "Episode 4");
        assert_eq!(meta.notes, "rough cut");
        assert_eq!(meta.artist, "");
        assert!(!meta.has_tag_defaults());
    }

    #[test]
    fn empty_tags_are_not_written_back() {
        // `skip_serializing_if` keeps a project that never set them
        // from growing three empty keys it has no use for.
        let meta = ProjectMeta::from_dir(Path::new("/tmp/Episode 4"));
        let text = serde_json::to_string(&meta).expect("serialise");
        assert!(!text.contains("artist"), "wrote an empty artist: {text}");
        assert!(!text.contains("album"));
        assert!(!text.contains("year"));
    }

    #[test]
    fn a_project_with_any_tag_has_defaults_to_contribute() {
        let mut meta = ProjectMeta::from_dir(Path::new("/tmp/x"));
        assert!(!meta.has_tag_defaults());
        meta.album = "Season 2".into();
        assert!(meta.has_tag_defaults());
    }

    #[test]
    fn nonsense_on_disk_still_opens() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(meta_path(dir.path()), "{ not json").expect("write");
        // The audio and its history are not in this file, so a corrupt
        // one must not stop the project opening.
        let meta = read_meta(dir.path());
        assert_eq!(meta.name, dir.path().file_name().unwrap().to_str().unwrap());
    }

    #[test]
    fn an_unwritten_project_says_nothing_rather_than_guessing() {
        let dir = tempfile::tempdir().expect("tempdir");
        // `read_meta` names it after the folder, which is right for
        // showing and wrong for inheriting.
        assert_eq!(
            read_meta(dir.path()).name,
            dir.path().file_name().unwrap().to_str().unwrap()
        );
        assert!(read_meta_if_present(dir.path()).is_none());
    }

    #[test]
    fn a_round_trip_keeps_the_tags() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut meta = ProjectMeta::from_dir(dir.path());
        meta.artist = "Ada Lovelace".into();
        meta.year = "2026".into();
        write_meta(dir.path(), &meta).expect("write");
        assert_eq!(read_meta(dir.path()), meta);
    }
}
