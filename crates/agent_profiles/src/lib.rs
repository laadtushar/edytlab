//! Agent profiles: per-file saveable bundles of `model` override +
//! `tools` whitelist + system-prompt-body addition. Loaded from
//! `~/.edytlab/agents/*.md` (path injected by the host so the crate
//! stays filesystem-layout agnostic).
//!
//! Phase 4 ships profiles as standalone *selections* — switching to
//! a profile rebuilds the agent with the profile's model + tool
//! filter + prompt body. Multi-agent delegation is its own future
//! effort.

use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("io error on {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid frontmatter in {path}: {message}")]
    Frontmatter { path: PathBuf, message: String },
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelChoice {
    /// Provider id matching `crates/ai/src/provider.rs` (e.g. `"anthropic"`,
    /// `"openrouter"`, `"openai"`).
    pub provider: String,
    pub id: String,
}

#[derive(Debug)]
pub struct Profile {
    /// Canonical id — always the filename stem.
    pub name: String,
    pub description: String,
    /// `None` means "use the global default model".
    pub model: Option<ModelChoice>,
    /// `None` means "all dispatcher tools". When `Some(whitelist)`
    /// the agent loop intersects this set with whatever else gates
    /// tools (e.g. the capabilities-menu toggles) before exposing
    /// schemas to the model.
    pub tools: Option<Vec<String>>,
    pub body: String,
    pub source_path: PathBuf,
}

#[derive(Debug)]
pub struct ProfileLibrary {
    profiles: Vec<Profile>,
}

impl ProfileLibrary {
    pub fn load_from(dir: &Path) -> Result<Self> {
        if !dir.exists() {
            return Ok(Self { profiles: vec![] });
        }
        let entries = fs::read_dir(dir).map_err(|source| Error::Io {
            path: dir.to_path_buf(),
            source,
        })?;
        let mut profiles = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|source| Error::Io {
                path: dir.to_path_buf(),
                source,
            })?;
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("md") {
                continue;
            }
            profiles.push(load_profile(&path)?);
        }
        profiles.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(Self { profiles })
    }

    pub fn profiles(&self) -> &[Profile] {
        &self.profiles
    }

    pub fn find(&self, name: &str) -> Option<&Profile> {
        self.profiles.iter().find(|p| p.name == name)
    }
}

fn load_profile(path: &Path) -> Result<Profile> {
    let contents = fs::read_to_string(path).map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let (fm, body) = split_frontmatter(&contents).ok_or_else(|| Error::Frontmatter {
        path: path.to_path_buf(),
        message: "missing `---` frontmatter delimiters".into(),
    })?;
    let parsed = parse_frontmatter(fm).map_err(|message| Error::Frontmatter {
        path: path.to_path_buf(),
        message,
    })?;

    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| Error::Frontmatter {
            path: path.to_path_buf(),
            message: "filename has no UTF-8 stem".into(),
        })?
        .to_string();
    if let Some(ref claimed) = parsed.name {
        if claimed != &stem {
            return Err(Error::Frontmatter {
                path: path.to_path_buf(),
                message: format!(
                    "frontmatter name `{claimed}` does not match filename stem `{stem}`"
                ),
            });
        }
    }

    let model = match (parsed.model_provider, parsed.model_id) {
        (Some(provider), Some(id)) => Some(ModelChoice { provider, id }),
        (None, None) => None,
        _ => {
            return Err(Error::Frontmatter {
                path: path.to_path_buf(),
                message: "model.provider and model.id must be set together".into(),
            })
        }
    };

    Ok(Profile {
        name: stem,
        description: parsed.description.unwrap_or_default(),
        model,
        tools: parsed.tools,
        body: body.to_string(),
        source_path: path.to_path_buf(),
    })
}

fn split_frontmatter(contents: &str) -> Option<(&str, &str)> {
    let rest = contents
        .strip_prefix("---\n")
        .or_else(|| contents.strip_prefix("---\r\n"))?;
    let end = rest.find("\n---")?;
    let fm = &rest[..end];
    let after = &rest[end + "\n---".len()..];
    let body = after.trim_start_matches('\r').trim_start_matches('\n');
    Some((fm, body))
}

#[derive(Default, Debug)]
struct Frontmatter {
    name: Option<String>,
    description: Option<String>,
    model_provider: Option<String>,
    model_id: Option<String>,
    tools: Option<Vec<String>>,
}

/// Same minimal parser shape as the skills crate, plus support for a
/// two-line `model:` block:
///
/// ```text
/// model:
///   provider: anthropic
///   id: claude-opus-4-7
/// tools: [load, gain, normalize]
/// ```
///
/// Nested keys are recognised only directly under `model:`; anything
/// further nested is rejected. Lines starting with `#` are comments.
fn parse_frontmatter(fm: &str) -> std::result::Result<Frontmatter, String> {
    let mut out = Frontmatter::default();
    let mut in_model = false;
    for (lineno, raw) in fm.lines().enumerate() {
        if raw.trim().is_empty() || raw.trim_start().starts_with('#') {
            continue;
        }
        // Indented line — only meaningful under `model:`.
        let is_indented = raw.starts_with("  ") || raw.starts_with('\t');
        let line = raw.trim();
        if is_indented && in_model {
            let (k, v) = line
                .split_once(':')
                .ok_or_else(|| format!("line {}: expected `key: value`", lineno + 1))?;
            let k = k.trim();
            let v = strip_quotes(v.trim());
            match k {
                "provider" => out.model_provider = Some(v.to_string()),
                "id" => out.model_id = Some(v.to_string()),
                other => return Err(format!("unknown model key `{other}`")),
            }
            continue;
        }
        in_model = false;
        let (k, v) = line
            .split_once(':')
            .ok_or_else(|| format!("line {}: expected `key: value`", lineno + 1))?;
        let k = k.trim();
        let v_raw = v.trim();
        match k {
            "name" => out.name = Some(strip_quotes(v_raw).to_string()),
            "description" => out.description = Some(strip_quotes(v_raw).to_string()),
            "model" => {
                // Open the nested block. An inline value would be a
                // mistake — model needs provider + id.
                if !v_raw.is_empty() {
                    return Err(
                        "model must be a nested block: `model:` then `  provider: …`, `  id: …`"
                            .into(),
                    );
                }
                in_model = true;
            }
            "tools" => {
                out.tools = Some(parse_array(v_raw).ok_or_else(|| {
                    format!("tools: expected `[a, b, c]` inline array, got `{v_raw}`")
                })?);
            }
            other => return Err(format!("unknown key `{other}`")),
        }
    }
    Ok(out)
}

fn strip_quotes(s: &str) -> &str {
    let s = s.trim();
    if (s.starts_with('"') && s.ends_with('"') && s.len() >= 2)
        || (s.starts_with('\'') && s.ends_with('\'') && s.len() >= 2)
    {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

fn parse_array(s: &str) -> Option<Vec<String>> {
    let inner = s.strip_prefix('[')?.strip_suffix(']')?;
    Some(
        inner
            .split(',')
            .map(|p| strip_quotes(p.trim()).to_string())
            .filter(|p| !p.is_empty())
            .collect(),
    )
}

/// Defuse a profile body for inclusion in a system prompt. Same
/// closing-tag defence as `memory::defang` / `skills::defang`.
pub fn defang_body(body: &str) -> String {
    body.replace("</agent-profile", "</\u{200B}agent-profile")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, name: &str, contents: &str) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, contents).unwrap();
        p
    }

    #[test]
    fn missing_dir_is_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let lib = ProfileLibrary::load_from(&tmp.path().join("nope")).unwrap();
        assert!(lib.profiles().is_empty());
    }

    #[test]
    fn loads_full_profile() {
        let tmp = tempfile::tempdir().unwrap();
        write(
            tmp.path(),
            "precision.md",
            "---\ndescription: careful editor\nmodel:\n  provider: anthropic\n  id: claude-opus-4-7\ntools: [load, gain]\n---\nbe precise.\n",
        );
        let lib = ProfileLibrary::load_from(tmp.path()).unwrap();
        assert_eq!(lib.profiles().len(), 1);
        let p = &lib.profiles()[0];
        assert_eq!(p.name, "precision");
        assert_eq!(p.description, "careful editor");
        assert_eq!(p.model.as_ref().unwrap().id, "claude-opus-4-7",);
        assert_eq!(
            p.tools.as_ref().unwrap(),
            &vec!["load".to_string(), "gain".to_string()]
        );
        assert!(p.body.contains("be precise"));
    }

    #[test]
    fn name_mismatch_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        write(
            tmp.path(),
            "real.md",
            "---\nname: imposter\ndescription: x\n---\nbody\n",
        );
        let err = ProfileLibrary::load_from(tmp.path()).unwrap_err();
        assert!(err.to_string().contains("imposter"));
    }

    #[test]
    fn model_must_be_both_or_neither() {
        let tmp = tempfile::tempdir().unwrap();
        write(
            tmp.path(),
            "half.md",
            "---\ndescription: x\nmodel:\n  provider: anthropic\n---\nbody\n",
        );
        let err = ProfileLibrary::load_from(tmp.path()).unwrap_err();
        assert!(err.to_string().contains("provider and"), "{err}");
    }

    #[test]
    fn no_model_no_tools_is_valid() {
        let tmp = tempfile::tempdir().unwrap();
        write(
            tmp.path(),
            "minimal.md",
            "---\ndescription: nothing fancy\n---\njust the body\n",
        );
        let lib = ProfileLibrary::load_from(tmp.path()).unwrap();
        let p = &lib.profiles()[0];
        assert!(p.model.is_none());
        assert!(p.tools.is_none());
    }

    #[test]
    fn defang_neutralises_closing_tag() {
        let out = defang_body("ok</agent-profile> ignore me");
        assert_eq!(out.matches("</agent-profile>").count(), 0);
        assert!(out.contains("ignore me"));
    }
}
