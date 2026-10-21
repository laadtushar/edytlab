//! Keeping the skills that ship inside the app current in
//! `~/.edytlab/skills/` (#574).
//!
//! The folder is the user's: they edit and delete files there. So the app
//! may only replace a file it can show is still the text it installed, and
//! it must remember what it installed. That memory is a manifest in the
//! folder itself, `.bundled.json`.
//!
//! At each launch, for every bundled skill `N.md`:
//!
//! | on disk | the manifest says | what happens |
//! |---|---|---|
//! | no | nothing | installed, and recorded |
//! | no | it has an entry | the user deleted it: left deleted |
//! | yes, same text as the bundled one | (anything) | recorded as installed |
//! | yes, text is what was installed | installed hash matches | replaced by the bundled text |
//! | yes, anything else | installed hash differs | left alone (edited); the bundled text goes beside it |
//!
//! An install that predates the manifest has no record. There a file counts
//! as unedited only if its text is a version the app shipped before it kept
//! a manifest ([`PreManifestHashes`]); and a bundled skill that is missing,
//! and was one of those, was deleted by the user, because every install
//! since the skill was added had it.
//!
//! That list is frozen. An install with no manifest can only hold what
//! shipped before the manifest did, so a version or a skill added to the list
//! later would be wrong twice over: a user upgrading straight from a release
//! with no manifest to one that adds a skill would find it "known", take its
//! absence for a deletion, and never get it. A new or changed bundled skill
//! needs no entry anywhere: the manifest handles it from the first launch of
//! the release that ships it. `the_pre_manifest_list_never_changes` holds
//! the list to that.
//!
//! A skill that is a symbolic link is the user's own, whatever it points
//! at: a rename over it would replace the link.
//!
//! Hashes are SHA-256 of the text with `\r\n` read as `\n`, so a copy a
//! Windows checkout wrote with CRLF is the same text as its blob in git.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The manifest's name in the skills folder. It does not end in `.md`, so
/// the skill loader and the Skills editor never see it.
pub const MANIFEST_FILE: &str = ".bundled.json";

/// Every version of every bundled skill shipped before the app kept
/// `.bundled.json`. Frozen: see the module's notes. `scripts/pre-manifest-skill-hashes.mjs --check`
/// verifies it against the git history.
const PRE_MANIFEST_JSON: &str = include_str!("pre_manifest_skills.json");

/// SHA-256 of a skill's text, as 64 lowercase hex digits, reading `\r\n` as
/// `\n`.
pub fn text_hash(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    let mut rest = bytes;
    while let Some(i) = rest.iter().position(|&b| b == b'\r') {
        hasher.update(&rest[..i]);
        if rest.get(i + 1) != Some(&b'\n') {
            hasher.update(b"\r");
        }
        rest = &rest[i + 1..];
    }
    hasher.update(rest);
    format!("{:x}", hasher.finalize())
}

/// The hashes of the versions of each bundled skill that shipped before the
/// app kept `.bundled.json`, by file name. It describes installs with no
/// manifest and nothing else.
#[derive(Debug, Default, Clone)]
pub struct PreManifestHashes(BTreeMap<String, BTreeSet<String>>);

#[derive(Deserialize)]
struct PreManifestFile {
    skills: BTreeMap<String, Vec<String>>,
}

impl PreManifestHashes {
    /// The list compiled into the app. A list that cannot be read is an
    /// empty one, which is the safe side: nothing without a manifest entry
    /// is then taken for unedited, or for deleted. A test parses the
    /// committed file.
    pub fn embedded() -> Self {
        Self::from_json(PRE_MANIFEST_JSON).unwrap_or_else(|e| {
            tracing::error!(error = %e, "pre_manifest_skills.json is unreadable");
            Self::default()
        })
    }

    pub fn from_json(json: &str) -> serde_json::Result<Self> {
        let file: PreManifestFile = serde_json::from_str(json)?;
        Ok(Self(
            file.skills
                .into_iter()
                .map(|(name, hashes)| (name, hashes.into_iter().collect()))
                .collect(),
        ))
    }

    /// Whether `file` was a bundled skill before the manifest.
    pub fn was_bundled(&self, file: &str) -> bool {
        self.0.contains_key(file)
    }

    /// Whether `hash` is one of the versions of `file` shipped before the
    /// manifest.
    pub fn is_version_of(&self, file: &str, hash: &str) -> bool {
        self.0.get(file).is_some_and(|s| s.contains(hash))
    }
}

/// What the app remembers about one bundled skill.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Entry {
    /// Hash of the text the app put in the file. `None`: the file was the
    /// user's when the app first met it, so the app has never written it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    installed: Option<String>,
    /// Hash of the bundled text last written beside an edited copy, so a
    /// side file the user deletes stays deleted until the bundled text
    /// changes again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    offered: Option<String>,
}

/// `.bundled.json`: an entry for every bundled skill the app has dealt
/// with. An entry without a file on disk means the user deleted it.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Manifest {
    #[serde(default)]
    files: BTreeMap<String, Entry>,
}

/// What one pass did, for the log and the command's return value.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SyncReport {
    /// Skills copied into the folder for the first time.
    pub installed: usize,
    /// Unedited copies replaced by a newer bundled text.
    pub updated: usize,
    /// Edited copies left as they were, whose bundled text differs.
    pub kept_edited: usize,
    /// Side files (`<name>.bundled.md`) written beside an edited copy.
    pub side_copies: usize,
    /// Skills the user deleted, not brought back.
    pub kept_deleted: usize,
    /// Skills that could not be dealt with. Each is logged and skipped.
    pub failed: usize,
}

impl SyncReport {
    /// Files the pass put into the skills folder as skills: what
    /// `install_bundled_skills` returns.
    pub fn written(&self) -> usize {
        self.installed + self.updated
    }
}

/// Bring the skills folder up to date with the bundled skills, as the
/// module's table says. Never deletes anything.
///
/// An error on one skill is logged and counted in
/// [`SyncReport::failed`]; the others carry on. Only a bundled directory
/// that cannot be read, or a skills folder that cannot be made, is an `Err`.
/// A bundled directory that is not there (a dev build) is not an error:
/// there is nothing to do.
pub fn sync(
    bundled_dir: &Path,
    skills_dir: &Path,
    pre_manifest: &PreManifestHashes,
) -> io::Result<SyncReport> {
    let mut report = SyncReport::default();
    if !bundled_dir.is_dir() {
        return Ok(report);
    }
    let bundled = bundled_skill_files(bundled_dir)?;
    if bundled.is_empty() {
        return Ok(report);
    }
    fs::create_dir_all(skills_dir)?;

    let before = load_manifest(skills_dir);
    // No manifest and a folder that already holds skills: an app that kept
    // no manifest installed it. Of the bundled skills, those that shipped
    // then (`was_bundled`) and are missing were deleted by the user; any
    // other is new to this folder.
    let predates_manifest = before.is_none() && holds_skill_files(skills_dir)?;
    let mut manifest = before.clone().unwrap_or_default();

    for (name, src) in &bundled {
        if let Err(e) = sync_one(
            name,
            src,
            skills_dir,
            pre_manifest,
            predates_manifest,
            &mut manifest,
            &mut report,
        ) {
            tracing::warn!(skill = %name, error = %e, "bundled skill not updated; skipping it");
            report.failed += 1;
        }
    }

    if before.as_ref() != Some(&manifest) {
        // Not fatal: at the next launch every file this pass wrote equals
        // its bundled text, which is then recorded as installed.
        if let Err(e) = save_manifest(skills_dir, &manifest) {
            tracing::warn!(error = %e, "could not save {MANIFEST_FILE}");
        }
    }
    Ok(report)
}

fn sync_one(
    name: &str,
    src: &Path,
    skills_dir: &Path,
    pre_manifest: &PreManifestHashes,
    predates_manifest: bool,
    manifest: &mut Manifest,
    report: &mut SyncReport,
) -> io::Result<()> {
    let bundled = fs::read(src)?;
    let bundled_hash = text_hash(&bundled);
    let dst = skills_dir.join(name);
    let mut entry = manifest.files.get(name).cloned();

    // A link the user made to a skill kept elsewhere: renaming a file over
    // it would replace the link itself, so it is theirs, like an edited one.
    if fs::symlink_metadata(&dst).is_ok_and(|m| m.file_type().is_symlink()) {
        manifest.files.entry(name.to_owned()).or_default();
        report.kept_edited += 1;
        return Ok(());
    }

    let disk = match fs::read(&dst) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            if entry.is_some() || (predates_manifest && pre_manifest.was_bundled(name)) {
                // The user deleted it. Remember that, so the decision
                // outlives the migration.
                manifest.files.entry(name.to_owned()).or_default();
                report.kept_deleted += 1;
            } else {
                write_atomic(&dst, &bundled, false)?;
                manifest.files.insert(
                    name.to_owned(),
                    Entry {
                        installed: Some(bundled_hash),
                        offered: None,
                    },
                );
                report.installed += 1;
            }
            return Ok(());
        }
        Err(e) => return Err(e),
    };

    let disk_hash = text_hash(&disk);
    let mut e = entry.take().unwrap_or_default();
    if disk_hash == bundled_hash {
        // Already current, whoever put it there.
        e.installed = Some(bundled_hash);
        manifest.files.insert(name.to_owned(), e);
        return Ok(());
    }

    let unedited = match &e.installed {
        Some(installed) => *installed == disk_hash,
        None => pre_manifest.is_version_of(name, &disk_hash),
    };
    if unedited {
        write_atomic(&dst, &bundled, true)?;
        e.installed = Some(bundled_hash);
        manifest.files.insert(name.to_owned(), e);
        report.updated += 1;
        return Ok(());
    }

    // Edited: it stays as it is. Put the new text beside it, once per
    // bundled version.
    report.kept_edited += 1;
    if e.offered.as_deref() != Some(bundled_hash.as_str()) {
        match write_atomic(&side_copy_path(skills_dir, name), &bundled, true) {
            Ok(()) => {
                e.offered = Some(bundled_hash);
                report.side_copies += 1;
            }
            Err(err) => {
                tracing::warn!(skill = %name, error = %err, "could not write the bundled copy beside an edited skill");
            }
        }
    }
    manifest.files.insert(name.to_owned(), e);
    Ok(())
}

/// `<stem>.bundled.md`, which `skills::SkillLibrary` skips.
fn side_copy_path(skills_dir: &Path, name: &str) -> PathBuf {
    let stem = name.strip_suffix(".md").unwrap_or(name);
    skills_dir.join(format!("{stem}{}.md", skills::BUNDLED_COPY_SUFFIX))
}

/// The `.md` files directly in the bundled directory, by name.
fn bundled_skill_files(dir: &Path) -> io::Result<Vec<(String, PathBuf)>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("md") || !path.is_file() {
            continue;
        }
        if let Some(name) = entry.file_name().to_str() {
            files.push((name.to_owned(), path));
        }
    }
    files.sort();
    Ok(files)
}

/// Whether the folder holds any `.md` file: the old app's test for "skills
/// are already installed".
fn holds_skill_files(dir: &Path) -> io::Result<bool> {
    for entry in fs::read_dir(dir)? {
        if entry?.path().extension().and_then(|s| s.to_str()) == Some("md") {
            return Ok(true);
        }
    }
    Ok(false)
}

/// The manifest, or `None` when there is none or it cannot be read; either
/// way the pre-manifest rules apply, which only ever touch a file whose text
/// is a version that shipped before the manifest.
///
/// So when a manifest is lost or corrupted after the app has been updating
/// the folder, a file holding a later text is not in the frozen list: it
/// reads as edited, is left alone and gets a side copy, never an overwrite.
/// The cost is that such a skill stops being updated, and that a bundled
/// skill the user deleted which is not one of the pre-manifest skills comes
/// back once, since nothing says it was deleted.
fn load_manifest(skills_dir: &Path) -> Option<Manifest> {
    let raw = match fs::read(skills_dir.join(MANIFEST_FILE)) {
        Ok(raw) => raw,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return None,
        Err(e) => {
            tracing::warn!(error = %e, "could not read {MANIFEST_FILE}");
            return None;
        }
    };
    match serde_json::from_slice(&raw) {
        Ok(m) => Some(m),
        Err(e) => {
            tracing::warn!(error = %e, "{MANIFEST_FILE} is not valid; treating the folder as installed by an older app");
            None
        }
    }
}

fn save_manifest(skills_dir: &Path, manifest: &Manifest) -> io::Result<()> {
    let mut json = serde_json::to_string_pretty(manifest).map_err(io::Error::other)?;
    json.push('\n');
    write_atomic(&skills_dir.join(MANIFEST_FILE), json.as_bytes(), true)
}

/// Write `bytes` to a temporary file in `path`'s folder and rename it into
/// place, so a reader sees the old file or the new one and a crash leaves
/// no half-written skill. With `replace` false the rename refuses to
/// overwrite a file that appeared in the meantime.
fn write_atomic(path: &Path, bytes: &[u8], replace: bool) -> io::Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no folder"))?;
    // `.tmp`, not `.md`: nothing lists a temporary file as a skill, and the
    // guard removes it if anything below fails.
    let mut tmp = tempfile::Builder::new()
        .prefix(".bundled-")
        .suffix(".tmp")
        .tempfile_in(dir)?;
    tmp.write_all(bytes)?;
    tmp.as_file().sync_all()?;
    if replace {
        tmp.persist(path).map_err(|e| e.error)?;
    } else {
        tmp.persist_noclobber(path).map_err(|e| e.error)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// A temporary home with the app's bundled skills and the user's
    /// skills folder beside each other.
    struct Home {
        _tmp: TempDir,
        bundled: PathBuf,
        skills: PathBuf,
    }

    impl Home {
        fn new() -> Self {
            let tmp = TempDir::new().unwrap();
            let bundled = tmp.path().join("app").join("bundled-skills");
            let skills = tmp.path().join("home").join(".edytlab").join("skills");
            fs::create_dir_all(&bundled).unwrap();
            Self {
                _tmp: tmp,
                bundled,
                skills,
            }
        }

        /// What the next release of the app ships: exactly these files.
        fn ship(&self, files: &[(&str, &str)]) {
            for e in fs::read_dir(&self.bundled).unwrap() {
                fs::remove_file(e.unwrap().path()).unwrap();
            }
            for (name, text) in files {
                fs::write(self.bundled.join(name), text).unwrap();
            }
        }

        /// A file the user (or an older app) left in the skills folder.
        fn put(&self, name: &str, text: &str) {
            fs::create_dir_all(&self.skills).unwrap();
            fs::write(self.skills.join(name), text).unwrap();
        }

        fn read(&self, name: &str) -> String {
            fs::read_to_string(self.skills.join(name)).unwrap()
        }

        fn exists(&self, name: &str) -> bool {
            self.skills.join(name).exists()
        }

        fn launch(&self) -> SyncReport {
            self.launch_with(&PreManifestHashes::default())
        }

        fn launch_with(&self, pre_manifest: &PreManifestHashes) -> SyncReport {
            sync(&self.bundled, &self.skills, pre_manifest).unwrap()
        }

        /// The manifest as written: `files.<name>.installed`.
        fn recorded(&self, name: &str) -> Option<String> {
            let raw = fs::read_to_string(self.skills.join(MANIFEST_FILE)).ok()?;
            let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
            v["files"][name]["installed"].as_str().map(str::to_owned)
        }

        /// Every file name in the skills folder, sorted.
        fn names(&self) -> Vec<String> {
            let mut names: Vec<String> = fs::read_dir(&self.skills)
                .unwrap()
                .map(|e| e.unwrap().file_name().into_string().unwrap())
                .collect();
            names.sort();
            names
        }

        /// Whether the manifest has an entry for `name` at all.
        fn has_entry(&self, name: &str) -> bool {
            let raw = fs::read_to_string(self.skills.join(MANIFEST_FILE)).unwrap();
            let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
            v["files"].get(name).is_some()
        }
    }

    /// A pre-manifest list that says `file` once had these texts.
    fn pre_manifest(file: &str, texts: &[&str]) -> PreManifestHashes {
        let hashes: Vec<String> = texts.iter().map(|t| text_hash(t.as_bytes())).collect();
        let json = serde_json::json!({ "skills": { file: hashes } });
        PreManifestHashes::from_json(&json.to_string()).unwrap()
    }

    #[test]
    fn a_fresh_install_copies_every_skill_and_records_them() {
        let h = Home::new();
        h.ship(&[("a.md", "A1"), ("b.md", "B1")]);

        let report = h.launch();

        assert_eq!(report.installed, 2);
        assert_eq!(report.written(), 2);
        assert_eq!(h.read("a.md"), "A1");
        assert_eq!(h.read("b.md"), "B1");
        assert_eq!(h.recorded("a.md"), Some(text_hash(b"A1")));
        assert_eq!(h.recorded("b.md"), Some(text_hash(b"B1")));
        // Nothing else is left behind: no temporary file, no side copy.
        assert_eq!(h.names(), [".bundled.json", "a.md", "b.md"]);
    }

    #[test]
    fn an_unedited_old_copy_is_updated_when_the_bundled_text_changes() {
        let h = Home::new();
        h.ship(&[("a.md", "A1")]);
        h.launch();

        h.ship(&[("a.md", "A2")]);
        let report = h.launch();

        assert_eq!(report.updated, 1);
        assert_eq!(h.read("a.md"), "A2");
        assert_eq!(h.recorded("a.md"), Some(text_hash(b"A2")));
    }

    #[test]
    fn an_edited_copy_is_left_alone() {
        let h = Home::new();
        h.ship(&[("a.md", "A1")]);
        h.launch();
        h.put("a.md", "my own version");

        h.ship(&[("a.md", "A2")]);
        let report = h.launch();

        assert_eq!(report.updated, 0);
        assert_eq!(report.kept_edited, 1);
        assert_eq!(h.read("a.md"), "my own version");
        // Still the baseline the edit is measured against.
        assert_eq!(h.recorded("a.md"), Some(text_hash(b"A1")));
    }

    #[test]
    fn a_deleted_skill_is_not_reinstalled() {
        let h = Home::new();
        h.ship(&[("a.md", "A1"), ("b.md", "B1")]);
        h.launch();
        fs::remove_file(h.skills.join("a.md")).unwrap();

        // Neither on the next launch nor after the bundled text changes.
        let report = h.launch();
        assert_eq!(report.written(), 0);
        assert_eq!(report.kept_deleted, 1);
        assert!(!h.exists("a.md"));

        h.ship(&[("a.md", "A2"), ("b.md", "B1")]);
        assert_eq!(h.launch().written(), 0);
        assert!(!h.exists("a.md"));
    }

    #[test]
    fn a_new_bundled_skill_is_added_to_an_existing_install() {
        let h = Home::new();
        h.ship(&[("a.md", "A1")]);
        h.launch();
        h.put("mine.md", "something the user wrote");

        h.ship(&[("a.md", "A1"), ("c.md", "C1")]);
        let report = h.launch();

        assert_eq!(report.installed, 1);
        assert_eq!(h.read("c.md"), "C1");
        assert_eq!(h.recorded("c.md"), Some(text_hash(b"C1")));
        assert_eq!(h.read("mine.md"), "something the user wrote");
    }

    #[test]
    fn before_the_manifest_an_unedited_copy_of_an_older_shipped_version_is_updated() {
        let h = Home::new();
        // What the old app left: a populated folder and no manifest.
        h.put("a.md", "A1");
        h.ship(&[("a.md", "A2")]);

        let report = h.launch_with(&pre_manifest("a.md", &["A1", "A2"]));

        assert_eq!(report.updated, 1);
        assert_eq!(h.read("a.md"), "A2");
        assert_eq!(h.recorded("a.md"), Some(text_hash(b"A2")));
    }

    #[test]
    fn before_the_manifest_an_edited_copy_is_not_updated() {
        let h = Home::new();
        h.put("a.md", "A1 with my own changes");
        h.ship(&[("a.md", "A2")]);

        let report = h.launch_with(&pre_manifest("a.md", &["A1", "A2"]));

        assert_eq!(report.updated, 0);
        assert_eq!(report.kept_edited, 1);
        assert_eq!(h.read("a.md"), "A1 with my own changes");
        // Nothing is recorded as installed for a file the app never wrote.
        assert_eq!(h.recorded("a.md"), None);
    }
    #[cfg(unix)]
    #[test]
    fn a_skill_the_user_linked_elsewhere_is_left_alone() {
        let h = Home::new();
        h.ship(&[("a.md", "A1")]);
        h.launch();
        // The user keeps their skills in a dotfiles repo and links to it,
        // the file being the text the app installed.
        let kept = h.skills.parent().unwrap().join("dotfiles-a.md");
        fs::write(&kept, "A1").unwrap();
        fs::remove_file(h.skills.join("a.md")).unwrap();
        std::os::unix::fs::symlink(&kept, h.skills.join("a.md")).unwrap();

        h.ship(&[("a.md", "A2")]);
        let report = h.launch();

        assert_eq!(report.updated, 0);
        assert!(fs::symlink_metadata(h.skills.join("a.md"))
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(fs::read_to_string(&kept).unwrap(), "A1");
    }

    #[test]
    fn a_launch_with_nothing_new_changes_nothing() {
        let h = Home::new();
        h.ship(&[("a.md", "A1")]);
        h.launch();
        let manifest = fs::read(h.skills.join(MANIFEST_FILE)).unwrap();

        assert_eq!(h.launch(), SyncReport::default());
        assert_eq!(fs::read(h.skills.join(MANIFEST_FILE)).unwrap(), manifest);
        assert_eq!(h.read("a.md"), "A1");
    }

    #[test]
    fn without_a_bundled_directory_nothing_happens() {
        // A dev build has no `bundled-skills/` beside it.
        let h = Home::new();
        fs::remove_dir(&h.bundled).unwrap();

        assert_eq!(h.launch(), SyncReport::default());
        assert!(!h.skills.exists());
    }

    #[test]
    fn a_copy_edited_into_the_new_bundled_text_is_recorded_as_current() {
        let h = Home::new();
        h.ship(&[("a.md", "A1")]);
        h.launch();
        h.put("a.md", "A2");

        h.ship(&[("a.md", "A2")]);
        let report = h.launch();

        assert_eq!(report.written(), 0);
        assert_eq!(report.kept_edited, 0);
        assert_eq!(h.recorded("a.md"), Some(text_hash(b"A2")));
        assert_eq!(h.names(), [".bundled.json", "a.md"]);
    }

    #[test]
    fn before_the_manifest_the_current_text_is_recorded_without_a_write() {
        let h = Home::new();
        h.put("a.md", "A1");
        h.ship(&[("a.md", "A1")]);

        let report = h.launch_with(&pre_manifest("a.md", &["A1"]));

        assert_eq!(report.written(), 0);
        assert_eq!(h.recorded("a.md"), Some(text_hash(b"A1")));
        // So the next release can update it.
        h.ship(&[("a.md", "A2")]);
        assert_eq!(h.launch().updated, 1);
        assert_eq!(h.read("a.md"), "A2");
    }

    #[test]
    fn before_the_manifest_a_skill_the_user_deleted_stays_deleted() {
        let h = Home::new();
        // The old app put both in; the user deleted `b.md`.
        h.put("a.md", "A1");
        h.ship(&[("a.md", "A1"), ("b.md", "B1")]);
        let list = PreManifestHashes::from_json(
            &serde_json::json!({
                "skills": {
                    "a.md": [text_hash(b"A1")],
                    "b.md": [text_hash(b"B1")],
                }
            })
            .to_string(),
        )
        .unwrap();

        let report = h.launch_with(&list);
        assert_eq!(report.kept_deleted, 1);
        assert!(!h.exists("b.md"));

        // Once a manifest exists, the pre-manifest list is no longer what says
        // so: the manifest has an entry for it.
        assert!(h.has_entry("b.md"));
        assert_eq!(h.launch().written(), 0);
        assert!(!h.exists("b.md"));
    }

    #[test]
    fn before_the_manifest_a_skill_that_is_not_in_the_list_is_added() {
        let h = Home::new();
        h.put("a.md", "A1");
        h.ship(&[("a.md", "A1"), ("c.md", "C1")]);

        let report = h.launch_with(&pre_manifest("a.md", &["A1"]));

        assert_eq!(report.installed, 1);
        assert_eq!(h.read("c.md"), "C1");
    }

    #[test]
    fn a_skill_added_after_the_manifest_reaches_an_install_that_predates_it() {
        // A user on a release from before the manifest (a populated folder,
        // no `.bundled.json`) upgrades straight to one that adds
        // `later-skill.md`, against the list the app ships. The list knows
        // only what shipped before the manifest, so the new skill is new to
        // this install and is installed, not taken for one the user deleted.
        let h = Home::new();
        h.put("vocal-chain.md", "whatever the user has");
        h.ship(&[
            ("vocal-chain.md", "V2"),
            ("later-skill.md", "L1"),
            ("silence-cleaner.md", "S1"),
        ]);

        let report = h.launch_with(&PreManifestHashes::embedded());

        assert_eq!(report.installed, 1);
        assert_eq!(h.read("later-skill.md"), "L1");
        assert_eq!(h.recorded("later-skill.md"), Some(text_hash(b"L1")));
        // `silence-cleaner.md` did ship before the manifest, and the folder
        // lacks it: that one the user deleted.
        assert_eq!(report.kept_deleted, 1);
        assert!(!h.exists("silence-cleaner.md"));
    }

    #[test]
    fn a_deleted_edited_copy_stays_deleted_too() {
        // An edited file the app never wrote has no installed hash, but the
        // manifest still has its entry, so deleting it is remembered.
        let h = Home::new();
        h.put("a.md", "my version");
        h.ship(&[("a.md", "A2")]);
        h.launch_with(&pre_manifest("a.md", &["A1"]));
        fs::remove_file(h.skills.join("a.md")).unwrap();

        assert_eq!(h.launch().written(), 0);
        assert!(!h.exists("a.md"));
    }

    #[test]
    fn a_copy_with_windows_line_endings_is_the_same_text() {
        assert_eq!(text_hash(b"a\r\nb\r\n"), text_hash(b"a\nb\n"));
        // Only a CRLF pair is a line ending; a lone CR is text.
        assert_ne!(text_hash(b"a\rb"), text_hash(b"a\nb"));
        assert_ne!(text_hash(b"a\r"), text_hash(b"a"));

        // An old pre-manifest version a Windows checkout wrote with CRLF is
        // unedited, and an update replaces it.
        let h = Home::new();
        h.put("a.md", "line one\r\nline two\r\n");
        h.ship(&[("a.md", "line one\nline 2\n")]);
        let report = h.launch_with(&pre_manifest("a.md", &["line one\nline two\n"]));
        assert_eq!(report.updated, 1);
        assert_eq!(h.read("a.md"), "line one\nline 2\n");
    }

    #[test]
    fn a_manifest_that_cannot_be_read_counts_as_none() {
        let h = Home::new();
        h.put(MANIFEST_FILE, "{ this is not json");
        h.put("a.md", "A1");
        h.put("b.md", "B1 edited");
        h.ship(&[("a.md", "A2"), ("b.md", "B2")]);
        let list = PreManifestHashes::from_json(
            &serde_json::json!({
                "skills": {
                    "a.md": [text_hash(b"A1")],
                    "b.md": [text_hash(b"B1")],
                }
            })
            .to_string(),
        )
        .unwrap();

        let report = h.launch_with(&list);

        // Only the pre-manifest version is replaced, and the manifest is made
        // anew.
        assert_eq!(h.read("a.md"), "A2");
        assert_eq!(h.read("b.md"), "B1 edited");
        assert_eq!(report.updated, 1);
        assert_eq!(h.recorded("a.md"), Some(text_hash(b"A2")));
    }

    #[test]
    fn a_lost_manifest_leaves_a_text_the_app_installed_later_alone() {
        // v1 is a version that shipped before the manifest; v2 and v3 are
        // later ones. The app installs v1, updates to v2 under a manifest,
        // then the manifest is damaged.
        let h = Home::new();
        let list = pre_manifest("a.md", &["v1"]);
        h.ship(&[("a.md", "v1")]);
        h.launch_with(&list);
        h.ship(&[("a.md", "v2")]);
        assert_eq!(h.launch_with(&list).updated, 1);
        fs::write(h.skills.join(MANIFEST_FILE), "{ damaged").unwrap();

        // v2 is not a pre-manifest text, so without a record it is taken
        // for the user's: kept, with v3 beside it, and the manifest is made
        // anew.
        h.ship(&[("a.md", "v3")]);
        let report = h.launch_with(&list);

        assert_eq!(report.updated, 0);
        assert_eq!(report.kept_edited, 1);
        assert_eq!(h.read("a.md"), "v2");
        assert_eq!(h.read("a.bundled.md"), "v3");
        assert_eq!(h.recorded("a.md"), None);
    }

    #[test]
    fn a_skill_that_cannot_be_updated_does_not_stop_the_others() {
        let h = Home::new();
        // `a.md` is a directory, so it cannot be read as a skill.
        fs::create_dir_all(h.skills.join("a.md")).unwrap();
        h.ship(&[("a.md", "A1"), ("b.md", "B1")]);

        let report = h.launch();

        assert_eq!(report.failed, 1);
        assert_eq!(report.installed, 1);
        assert_eq!(h.read("b.md"), "B1");
        assert!(h.skills.join("a.md").is_dir());
    }

    #[test]
    fn an_edited_copy_gets_the_new_text_beside_it_once_per_bundled_version() {
        let skill =
            |body: &str| format!("---\nname: a\ndescription: d\ntrigger: always\n---\n{body}\n");
        let h = Home::new();
        h.ship(&[("a.md", &skill("v1"))]);
        h.launch();
        h.put("a.md", &skill("mine"));

        h.ship(&[("a.md", &skill("v2"))]);
        let report = h.launch();
        assert_eq!(report.side_copies, 1);
        assert_eq!(h.read("a.bundled.md"), skill("v2"));
        assert_eq!(h.read("a.md"), skill("mine"));

        // The agent and the Skills editor list one skill, the user's.
        let library = skills::SkillLibrary::load_from(&h.skills).unwrap();
        let listed: Vec<&str> = library.skills().iter().map(|s| s.name.as_str()).collect();
        assert_eq!(listed, ["a"]);

        // Deleted by the user, it stays deleted for this bundled version...
        fs::remove_file(h.skills.join("a.bundled.md")).unwrap();
        assert_eq!(h.launch().side_copies, 0);
        assert!(!h.exists("a.bundled.md"));

        // ...and comes back with the next one.
        h.ship(&[("a.md", &skill("v3"))]);
        assert_eq!(h.launch().side_copies, 1);
        assert_eq!(h.read("a.bundled.md"), skill("v3"));
        assert_eq!(h.read("a.md"), skill("mine"));
    }

    #[test]
    fn the_pre_manifest_list_is_valid() {
        let list: PreManifestFile = serde_json::from_str(PRE_MANIFEST_JSON).unwrap();
        assert!(!list.skills.is_empty());
        for (name, hashes) in &list.skills {
            assert!(name.ends_with(".md"), "{name}");
            assert!(!hashes.is_empty(), "{name} has no versions");
            for hash in hashes {
                assert!(
                    hash.len() == 64
                        && hash.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')),
                    "{name}: `{hash}` is not 64 lowercase hex digits"
                );
            }
        }
    }

    /// `pre_manifest_skills.json` is frozen. An install with no
    /// `.bundled.json` can hold only the versions that shipped before the
    /// manifest did, and `sync` takes a missing skill the list names for one
    /// the user deleted. So a skill or a version added to the list later is
    /// wrong: a user upgrading straight from a release with no manifest to
    /// one that adds the skill would be taken to have deleted it, and never
    /// get it. A new or changed bundled skill needs no entry here.
    #[test]
    fn the_pre_manifest_list_never_changes() {
        let list: PreManifestFile = serde_json::from_str(PRE_MANIFEST_JSON).unwrap();
        let versions: usize = list.skills.values().map(Vec::len).sum();
        let names: Vec<&str> = list.skills.keys().map(String::as_str).collect();
        assert_eq!(
            (names.len(), versions),
            (8, 12),
            "src/pre_manifest_skills.json lists {names:?} ({versions} versions). It is frozen at the \
             eight skills and twelve versions that shipped before the app kept .bundled.json, and must \
             not grow: a skill added to it would be taken, on an install with no manifest, for one the \
             user deleted, and never installed. A new or changed bundled skill needs no entry."
        );
        assert_eq!(
            text_hash(PRE_MANIFEST_JSON.as_bytes()),
            "526f212fb422794e472247306e0f21151ca80ac19b5dcde5cb7d923d49a40c8d",
            "src/pre_manifest_skills.json changed. It is frozen: it lists the bundled-skill versions \
             that shipped before the app kept .bundled.json, and an install with no manifest can hold \
             nothing else. Adding a later version or skill makes a user who upgrades straight from such \
             an install to a release that adds a skill treat it as deleted, so it is never installed. \
             A new or changed bundled skill needs no entry. If the file really was wrong, check it \
             against the history with `node scripts/pre-manifest-skill-hashes.mjs --check`, fix it, \
             and update this hash deliberately."
        );
    }
}
