//! Plugin manifest format: `edytlab-plugin.json`
//!
//! A plugin is a directory containing an `edytlab-plugin.json` manifest
//! that enumerates skill files, optional MCP server entries, and optional
//! agent profile files. The installer copies skill and agent files into
//! the appropriate user directories.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
pub struct PluginManifest {
    /// Kebab-case plugin identifier, unique across installed plugins.
    pub name: String,
    /// Semver string.
    pub version: String,
    pub description: Option<String>,
    pub author: Option<String>,
    /// Relative paths to skill `.md` files inside the plugin directory.
    #[serde(default)]
    pub skills: Vec<String>,
    /// MCP server entries (same format as `~/.edytlab/mcp.json`).
    #[serde(default, rename = "mcpServers")]
    pub mcp_servers: HashMap<String, Value>,
    /// Relative paths to agent profile `.md` files.
    #[serde(default)]
    pub agents: Vec<String>,
}

impl PluginManifest {
    /// Load and parse `edytlab-plugin.json` from `manifest_path`.
    pub fn load(manifest_path: &Path) -> Result<Self, String> {
        let content = std::fs::read_to_string(manifest_path)
            .map_err(|e| format!("read {}: {e}", manifest_path.display()))?;
        serde_json::from_str(&content)
            .map_err(|e| format!("parse {}: {e}", manifest_path.display()))
    }

    /// Copy skill files from `plugin_dir` into `skills_dir`.
    /// Returns list of destination paths written.
    pub fn install_skills(
        &self,
        plugin_dir: &Path,
        skills_dir: &Path,
    ) -> Result<Vec<PathBuf>, String> {
        install_entries("skill", plugin_dir, skills_dir, &self.skills)
    }

    /// Copy agent profile files from `plugin_dir` into `agents_dir`.
    pub fn install_agents(
        &self,
        plugin_dir: &Path,
        agents_dir: &Path,
    ) -> Result<Vec<PathBuf>, String> {
        install_entries("agent", plugin_dir, agents_dir, &self.agents)
    }
}

/// Resolve one manifest-relative entry to `(source, destination)`.
///
/// The containment check runs on the *canonicalised* source against the
/// canonicalised plugin root, so it rejects `..` and symlinks alike. The
/// destination is built from `file_name()` of that canonical path, which
/// is necessarily a single path component — a manifest therefore cannot
/// influence the destination directory or smuggle separators into it.
fn resolve_entry(
    kind: &str,
    canon_plugin: &Path,
    plugin_dir: &Path,
    rel: &str,
    dest_dir: &Path,
) -> Result<(PathBuf, PathBuf), String> {
    let canon_src = plugin_dir
        .join(rel)
        .canonicalize()
        .map_err(|e| format!("resolve {kind} path `{rel}`: {e}"))?;
    if !canon_src.starts_with(canon_plugin) {
        return Err(format!("{kind} path `{rel}` escapes plugin directory"));
    }
    let filename = canon_src
        .file_name()
        .ok_or_else(|| format!("{kind} path `{rel}` has no filename"))?;
    let name = filename.to_string_lossy();
    // `.md` alone is a hidden file with an empty stem, not a skill: the
    // loader keys skills by filename stem, so it could never be named.
    if !name.ends_with(".md") || name == ".md" {
        return Err(format!("{kind} `{rel}` is not a .md file"));
    }
    let dst = dest_dir.join(filename);
    Ok((canon_src, dst))
}

/// Copy every listed `.md` entry into `dest_dir`.
///
/// Refuses to overwrite an existing file. Skills and agent profiles are
/// fed to the model verbatim, so a plugin allowed to replace, say,
/// `podcast-cleanup.md` could substitute its own instructions for ones
/// the user already trusts — a quiet way to change what the agent does.
/// Replacing an installed file is therefore a deliberate act (delete it
/// first) rather than a side effect of installing something else.
///
/// Everything is resolved and checked before anything is written, so a
/// conflict on the last entry cannot leave the earlier ones installed.
fn install_entries(
    kind: &str,
    plugin_dir: &Path,
    dest_dir: &Path,
    rels: &[String],
) -> Result<Vec<PathBuf>, String> {
    if rels.is_empty() {
        return Ok(vec![]);
    }
    let canon_plugin = plugin_dir
        .canonicalize()
        .map_err(|e| format!("canonicalize plugin dir: {e}"))?;

    let mut planned: Vec<(PathBuf, PathBuf)> = Vec::with_capacity(rels.len());
    for rel in rels {
        let (src, dst) = resolve_entry(kind, &canon_plugin, plugin_dir, rel, dest_dir)?;
        if dst.exists() {
            return Err(format!(
                "{kind} `{}` already exists — remove {} first if you mean to replace it",
                rel,
                dst.display()
            ));
        }
        // Two manifest entries with the same basename would otherwise
        // clobber each other mid-install, last one winning silently.
        if planned.iter().any(|(_, planned_dst)| planned_dst == &dst) {
            return Err(format!(
                "{kind} `{rel}` collides with another entry of the same filename"
            ));
        }
        planned.push((src, dst));
    }

    std::fs::create_dir_all(dest_dir).map_err(|e| format!("create {kind} dir: {e}"))?;
    let mut installed = Vec::with_capacity(planned.len());
    for (src, dst) in planned {
        std::fs::copy(&src, &dst).map_err(|e| format!("copy {kind} `{}`: {e}", src.display()))?;
        installed.push(dst);
    }
    Ok(installed)
}

#[cfg(test)]
mod tests {
    use super::PluginManifest;
    use std::fs;
    use tempfile::TempDir;

    fn write_manifest(dir: &TempDir, content: &str) -> std::path::PathBuf {
        let path = dir.path().join("edytlab-plugin.json");
        fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn parse_minimal_manifest() {
        let dir = TempDir::new().unwrap();
        let path = write_manifest(&dir, r#"{"name":"test-plugin","version":"1.0.0"}"#);
        let m = PluginManifest::load(&path).unwrap();
        assert_eq!(m.name, "test-plugin");
        assert_eq!(m.version, "1.0.0");
        assert!(m.skills.is_empty());
        assert!(m.agents.is_empty());
        assert!(m.mcp_servers.is_empty());
    }

    #[test]
    fn parse_full_manifest() {
        let dir = TempDir::new().unwrap();
        let path = write_manifest(
            &dir,
            r#"{
                "name": "podcast-toolkit",
                "version": "2.1.0",
                "description": "Podcast production skills",
                "skills": ["skills/podcast-cleanup.md"],
                "mcpServers": {"whisper": {"command": "npx", "args": ["whisper-mcp"]}},
                "agents": ["agents/podcast-producer.md"]
            }"#,
        );
        let m = PluginManifest::load(&path).unwrap();
        assert_eq!(m.name, "podcast-toolkit");
        assert_eq!(m.version, "2.1.0");
        assert_eq!(m.skills.len(), 1);
        assert_eq!(m.mcp_servers.len(), 1);
        assert_eq!(m.agents.len(), 1);
    }

    #[test]
    fn install_skills_copies_files() {
        let plugin_dir = TempDir::new().unwrap();
        let skills_src = plugin_dir.path().join("skills");
        fs::create_dir_all(&skills_src).unwrap();
        fs::write(
            skills_src.join("my-skill.md"),
            "---\nname: my-skill\n---\nbody",
        )
        .unwrap();

        let manifest = PluginManifest {
            name: "test".into(),
            version: "1.0.0".into(),
            description: None,
            author: None,
            skills: vec!["skills/my-skill.md".into()],
            mcp_servers: Default::default(),
            agents: vec![],
        };

        let dst_dir = TempDir::new().unwrap();
        let installed = manifest
            .install_skills(plugin_dir.path(), dst_dir.path())
            .unwrap();
        assert_eq!(installed.len(), 1);
        assert!(dst_dir.path().join("my-skill.md").exists());
    }

    #[test]
    fn install_agents_skips_when_empty() {
        let plugin_dir = TempDir::new().unwrap();
        let manifest = PluginManifest {
            name: "test".into(),
            version: "1.0.0".into(),
            description: None,
            author: None,
            skills: vec![],
            mcp_servers: Default::default(),
            agents: vec![],
        };
        let dst_dir = TempDir::new().unwrap();
        // Should not create agents dir or error.
        let installed = manifest
            .install_agents(plugin_dir.path(), dst_dir.path())
            .unwrap();
        assert!(installed.is_empty());
    }

    // -----------------------------------------------------------------
    // Containment + overwrite protection
    // -----------------------------------------------------------------

    /// Build a plugin dir containing `skills/<name>` with given content.
    fn plugin_with_skill(name: &str, body: &str) -> TempDir {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join("skills")).unwrap();
        fs::write(dir.path().join("skills").join(name), body).unwrap();
        dir
    }

    /// A manifest must not be able to reach outside its own directory,
    /// even though `..` is a perfectly ordinary relative path.
    #[test]
    fn skill_path_escaping_the_plugin_dir_is_rejected() {
        let outside = TempDir::new().unwrap();
        fs::write(outside.path().join("secret.md"), "not yours").unwrap();

        let plugin = TempDir::new().unwrap();
        let manifest = PluginManifest {
            name: "evil".into(),
            version: "1".into(),
            description: None,
            author: None,
            skills: vec![format!(
                "../{}/secret.md",
                outside.path().file_name().unwrap().to_string_lossy()
            )],
            mcp_servers: Default::default(),
            agents: vec![],
        };
        let dest = TempDir::new().unwrap();
        let err = manifest
            .install_skills(plugin.path(), dest.path())
            .expect_err("must reject a path outside the plugin dir");
        assert!(err.contains("escapes plugin directory"), "got: {err}");
    }

    /// A symlink pointing outside is resolved by canonicalisation and
    /// then caught by the same containment check.
    #[cfg(unix)]
    #[test]
    fn symlinked_skill_pointing_outside_is_rejected() {
        let outside = TempDir::new().unwrap();
        let target = outside.path().join("secret.md");
        fs::write(&target, "not yours").unwrap();

        let plugin = TempDir::new().unwrap();
        fs::create_dir_all(plugin.path().join("skills")).unwrap();
        std::os::unix::fs::symlink(&target, plugin.path().join("skills/link.md")).unwrap();

        let manifest = PluginManifest {
            name: "evil".into(),
            version: "1".into(),
            description: None,
            author: None,
            skills: vec!["skills/link.md".into()],
            mcp_servers: Default::default(),
            agents: vec![],
        };
        let dest = TempDir::new().unwrap();
        let err = manifest
            .install_skills(plugin.path(), dest.path())
            .expect_err("must reject a symlink leaving the plugin dir");
        assert!(err.contains("escapes plugin directory"), "got: {err}");
    }

    /// The core of the overwrite protection: skills are fed to the model
    /// verbatim, so a plugin that could replace one the user already
    /// trusts would be rewriting the agent's instructions.
    #[test]
    fn installing_over_an_existing_skill_is_refused() {
        let plugin = plugin_with_skill("podcast-cleanup.md", "malicious instructions");
        let dest = TempDir::new().unwrap();
        fs::create_dir_all(dest.path()).unwrap();
        let victim = dest.path().join("podcast-cleanup.md");
        fs::write(&victim, "trusted instructions").unwrap();

        let manifest = PluginManifest {
            name: "evil".into(),
            version: "1".into(),
            description: None,
            author: None,
            skills: vec!["skills/podcast-cleanup.md".into()],
            mcp_servers: Default::default(),
            agents: vec![],
        };
        let err = manifest
            .install_skills(plugin.path(), dest.path())
            .expect_err("must not clobber an installed skill");
        assert!(err.contains("already exists"), "got: {err}");
        assert_eq!(
            fs::read_to_string(&victim).unwrap(),
            "trusted instructions",
            "the existing skill must be left untouched"
        );
    }

    /// A conflict on a later entry must not leave earlier ones written —
    /// a half-installed plugin is worse than a refused one.
    #[test]
    fn a_conflict_installs_nothing_at_all() {
        let plugin = TempDir::new().unwrap();
        fs::create_dir_all(plugin.path().join("skills")).unwrap();
        fs::write(plugin.path().join("skills/fresh.md"), "new").unwrap();
        fs::write(plugin.path().join("skills/taken.md"), "new").unwrap();

        let dest = TempDir::new().unwrap();
        fs::write(dest.path().join("taken.md"), "existing").unwrap();

        let manifest = PluginManifest {
            name: "p".into(),
            version: "1".into(),
            description: None,
            author: None,
            skills: vec!["skills/fresh.md".into(), "skills/taken.md".into()],
            mcp_servers: Default::default(),
            agents: vec![],
        };
        manifest
            .install_skills(plugin.path(), dest.path())
            .expect_err("must refuse the batch");
        assert!(
            !dest.path().join("fresh.md").exists(),
            "the first entry must not survive a failed install"
        );
    }

    /// Two entries with the same basename would silently overwrite one
    /// another, last write winning.
    #[test]
    fn duplicate_basenames_in_one_manifest_are_rejected() {
        let plugin = TempDir::new().unwrap();
        fs::create_dir_all(plugin.path().join("a")).unwrap();
        fs::create_dir_all(plugin.path().join("b")).unwrap();
        fs::write(plugin.path().join("a/dup.md"), "one").unwrap();
        fs::write(plugin.path().join("b/dup.md"), "two").unwrap();

        let manifest = PluginManifest {
            name: "p".into(),
            version: "1".into(),
            description: None,
            author: None,
            skills: vec!["a/dup.md".into(), "b/dup.md".into()],
            mcp_servers: Default::default(),
            agents: vec![],
        };
        let dest = TempDir::new().unwrap();
        let err = manifest
            .install_skills(plugin.path(), dest.path())
            .expect_err("must reject duplicate basenames");
        assert!(err.contains("collides"), "got: {err}");
    }

    /// Only markdown is installable, and `.md` alone is a hidden file
    /// with an empty stem — the loader keys skills by stem, so it could
    /// never be referenced.
    #[test]
    fn non_markdown_and_bare_dot_md_are_rejected() {
        for name in ["payload.sh", ".md"] {
            let plugin = plugin_with_skill(name, "x");
            let manifest = PluginManifest {
                name: "p".into(),
                version: "1".into(),
                description: None,
                author: None,
                skills: vec![format!("skills/{name}")],
                mcp_servers: Default::default(),
                agents: vec![],
            };
            let dest = TempDir::new().unwrap();
            let err = manifest
                .install_skills(plugin.path(), dest.path())
                .expect_err(&format!("`{name}` must not be installable"));
            assert!(err.contains("not a .md file"), "for {name}, got: {err}");
            assert!(
                !dest.path().join(name).exists(),
                "`{name}` must not have been written"
            );
        }
    }

    /// A malformed manifest surfaces as an error, never a panic.
    #[test]
    fn malformed_manifest_is_an_error_not_a_panic() {
        let dir = TempDir::new().unwrap();
        let path = write_manifest(&dir, "{ not json");
        let err = PluginManifest::load(&path).expect_err("must fail to parse");
        assert!(err.contains("parse"), "got: {err}");

        let path = write_manifest(&dir, r#"{"version":"1.0.0"}"#);
        PluginManifest::load(&path).expect_err("missing `name` must fail");
    }
}
