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
        let mut installed = Vec::new();
        let canon_plugin = plugin_dir
            .canonicalize()
            .map_err(|e| format!("canonicalize plugin dir: {e}"))?;
        for rel in &self.skills {
            let src = plugin_dir.join(rel);
            let canon_src = src
                .canonicalize()
                .map_err(|e| format!("resolve skill path `{rel}`: {e}"))?;
            if !canon_src.starts_with(&canon_plugin) {
                return Err(format!("skill path `{rel}` escapes plugin directory"));
            }
            let filename = canon_src
                .file_name()
                .ok_or_else(|| format!("skill path `{rel}` has no filename"))?;
            if !filename.to_string_lossy().ends_with(".md") {
                return Err(format!("skill `{rel}` is not a .md file"));
            }
            std::fs::create_dir_all(skills_dir).map_err(|e| format!("create skills dir: {e}"))?;
            let dst = skills_dir.join(filename);
            std::fs::copy(&canon_src, &dst).map_err(|e| format!("copy skill `{rel}`: {e}"))?;
            installed.push(dst);
        }
        Ok(installed)
    }

    /// Copy agent profile files from `plugin_dir` into `agents_dir`.
    pub fn install_agents(
        &self,
        plugin_dir: &Path,
        agents_dir: &Path,
    ) -> Result<Vec<PathBuf>, String> {
        if self.agents.is_empty() {
            return Ok(vec![]);
        }
        let canon_plugin = plugin_dir
            .canonicalize()
            .map_err(|e| format!("canonicalize plugin dir: {e}"))?;
        let mut installed = Vec::new();
        for rel in &self.agents {
            let src = plugin_dir.join(rel);
            let canon_src = src
                .canonicalize()
                .map_err(|e| format!("resolve agent path `{rel}`: {e}"))?;
            if !canon_src.starts_with(&canon_plugin) {
                return Err(format!("agent path `{rel}` escapes plugin directory"));
            }
            let filename = canon_src
                .file_name()
                .ok_or_else(|| format!("agent path `{rel}` has no filename"))?;
            if !filename.to_string_lossy().ends_with(".md") {
                return Err(format!("agent `{rel}` is not a .md file"));
            }
            std::fs::create_dir_all(agents_dir).map_err(|e| format!("create agents dir: {e}"))?;
            let dst = agents_dir.join(filename);
            std::fs::copy(&canon_src, &dst).map_err(|e| format!("copy agent `{rel}`: {e}"))?;
            installed.push(dst);
        }
        Ok(installed)
    }
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
}
