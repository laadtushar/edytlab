//! Markdown-with-frontmatter helpers shared by `skills` and
//! `agent_profiles` (both keep one `<name>.md` file per item).

use std::fs;
use std::path::{Path, PathBuf};

/// The `*.md` files directly under `dir`, in directory order.
pub fn md_files(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().and_then(|s| s.to_str()) == Some("md") {
            out.push(path);
        }
    }
    Ok(out)
}

/// The file stem of `path`, which must equal the frontmatter `name`
/// when one is given. The error is the message for `Error::Frontmatter`.
pub fn checked_stem(path: &Path, claimed: Option<&str>) -> Result<String, String> {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| "filename has no UTF-8 stem".to_string())?
        .to_string();
    if let Some(claimed) = claimed {
        if claimed != stem {
            return Err(format!(
                "frontmatter name `{claimed}` does not match filename stem `{stem}`"
            ));
        }
    }
    Ok(stem)
}

/// Split `---\n…fm…\n---\n…body…` into (`fm`, `body`). Returns `None`
/// if either delimiter is missing.
pub fn split_frontmatter(contents: &str) -> Option<(&str, &str)> {
    let rest = contents.strip_prefix("---\n").or_else(|| {
        // Tolerate CRLF for files authored on Windows.
        contents.strip_prefix("---\r\n")
    })?;
    let end = rest.find("\n---")?;
    let fm = &rest[..end];
    // Skip past the closing delimiter and one optional newline so the
    // body starts on a clean line.
    let after = &rest[end + "\n---".len()..];
    let body = after.trim_start_matches('\r').trim_start_matches('\n');
    Some((fm, body))
}

pub fn strip_quotes(s: &str) -> &str {
    let s = s.trim();
    if (s.starts_with('"') && s.ends_with('"') && s.len() >= 2)
        || (s.starts_with('\'') && s.ends_with('\'') && s.len() >= 2)
    {
        &s[1..s.len() - 1]
    } else {
        s
    }
}
