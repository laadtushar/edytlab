//! Config file (`~/.edytlab/mcp.json`) on-disk format.
//!
//! Compatible with Claude Code's `.mcp.json` shape closely enough to
//! lift configs between the two. Secrets in `env` use the
//! `<keychain:slot>` placeholder; the value is fetched from the OS
//! keychain at server-launch time so secrets never live in plaintext
//! on disk.

use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum McpError {
    #[error("io error on {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("parse error in {path}: {message}")]
    Parse { path: PathBuf, message: String },
    #[error("invalid server config: {0}")]
    InvalidConfig(String),
}

pub type Result<T> = std::result::Result<T, McpError>;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct McpConfig {
    /// Keyed by server id (the user-facing name). Order is preserved
    /// in the serialised output as an alphabetised key list.
    #[serde(default)]
    pub servers: HashMap<String, McpServerConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum McpServerConfig {
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        /// Env vars to pass to the child process. Values may use the
        /// `<keychain:slot>` placeholder to substitute a secret at
        /// launch time. Unresolved placeholders error out before the
        /// child is spawned.
        #[serde(default)]
        env: HashMap<String, String>,
        #[serde(default = "default_enabled")]
        enabled: bool,
    },
    Sse {
        url: String,
        #[serde(default)]
        headers: HashMap<String, String>,
        #[serde(default = "default_enabled")]
        enabled: bool,
    },
}

fn default_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Copy)]
pub enum McpTransport {
    Stdio,
    Sse,
}

impl McpServerConfig {
    pub fn transport(&self) -> McpTransport {
        match self {
            McpServerConfig::Stdio { .. } => McpTransport::Stdio,
            McpServerConfig::Sse { .. } => McpTransport::Sse,
        }
    }

    pub fn enabled(&self) -> bool {
        match self {
            McpServerConfig::Stdio { enabled, .. } => *enabled,
            McpServerConfig::Sse { enabled, .. } => *enabled,
        }
    }
}

/// A `<keychain:slot>` placeholder pointing at an OS keychain entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretRef<'a>(pub &'a str);

impl<'a> SecretRef<'a> {
    pub fn parse(s: &'a str) -> Option<Self> {
        let inner = s.strip_prefix("<keychain:")?.strip_suffix('>')?;
        if inner.is_empty() {
            return None;
        }
        Some(Self(inner))
    }
}

pub fn load_config(path: &Path) -> Result<McpConfig> {
    match fs::read_to_string(path) {
        Ok(s) => serde_json::from_str(&s).map_err(|e| McpError::Parse {
            path: path.to_path_buf(),
            message: e.to_string(),
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(McpConfig::default()),
        Err(source) => Err(McpError::Io {
            path: path.to_path_buf(),
            source,
        }),
    }
}

pub fn save_config(path: &Path, cfg: &McpConfig) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|source| McpError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
    }
    let serialised = serde_json::to_string_pretty(cfg).map_err(|e| McpError::Parse {
        path: path.to_path_buf(),
        message: e.to_string(),
    })?;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(parent).map_err(|source| McpError::Io {
        path: parent.to_path_buf(),
        source,
    })?;
    tmp.write_all(serialised.as_bytes())
        .and_then(|_| tmp.flush())
        .map_err(|source| McpError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    tmp.persist(path).map_err(|e| McpError::Io {
        path: path.to_path_buf(),
        source: e.error,
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_loads_as_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = load_config(&tmp.path().join("nope.json")).unwrap();
        assert!(cfg.servers.is_empty());
    }

    #[test]
    fn round_trip_stdio() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("mcp.json");
        let mut cfg = McpConfig::default();
        cfg.servers.insert(
            "github".into(),
            McpServerConfig::Stdio {
                command: "npx".into(),
                args: vec!["-y".into(), "@modelcontextprotocol/server-github".into()],
                env: {
                    let mut e = HashMap::new();
                    e.insert("GITHUB_TOKEN".into(), "<keychain:github_token>".into());
                    e
                },
                enabled: true,
            },
        );
        save_config(&path, &cfg).unwrap();
        let loaded = load_config(&path).unwrap();
        assert_eq!(loaded.servers.len(), 1);
    }

    #[test]
    fn secret_ref_parse() {
        assert_eq!(SecretRef::parse("<keychain:slot>"), Some(SecretRef("slot")));
        assert_eq!(SecretRef::parse("plain"), None);
        assert_eq!(SecretRef::parse("<keychain:>"), None);
    }
}
