//! Nothing downloads a model except the user's click (#383).
//!
//! A model is about 139 MB and the request goes to a third party. The
//! rules (CLAUDE.md, and the plan on #383, finding 3) are that no tool, no
//! agent path and no startup path downloads anything: the one way in is
//! the `download_ml_model` command, which the Settings row calls when the
//! user presses Download. The downloader holds no store, engine, agent or
//! dispatcher lock, and a tool runs holding all four, so a download from a
//! tool would also freeze the app for minutes.
//!
//! This reads the production source of every crate and app and fails when:
//!
//! - anything outside `ml-pipeline` names the downloader (`ModelStore`,
//!   `fetched_model_path`, `FetchObserver`): a tool or the agent loop
//!   would need one of them to fetch a file;
//! - anything but the desktop `commands.rs` names `ModelDownloads`, the
//!   manager the commands drive;
//! - `start_download`, the one method that begins a transfer, is called
//!   anywhere but inside the body of `download_ml_model`.
//!
//! It is a lexical check, in the manner of `session`'s
//! `no_unregistered_open.rs`: line comments are cut, and test modules are
//! not special-cased, because a production file with a test that downloads
//! would be a path too. It cannot see a call made through a name it does
//! not match, so a new way to start a transfer has to be named here.

use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Every `.rs` file under `dir`, as `(path relative to the repo with '/',
/// contents with line comments cut)`.
fn rust_sources(dir: &Path) -> Vec<(String, String)> {
    fn walk(dir: &Path, root: &Path, out: &mut Vec<(String, String)>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if matches!(
                name.as_str(),
                "node_modules" | "target" | ".git" | ".next" | "tests"
            ) {
                continue;
            }
            if path.is_dir() {
                walk(&path, root, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                if let Ok(text) = std::fs::read_to_string(&path) {
                    let rel = path
                        .strip_prefix(root)
                        .unwrap_or(&path)
                        .components()
                        .map(|c| c.as_os_str().to_string_lossy().to_string())
                        .collect::<Vec<_>>()
                        .join("/");
                    out.push((rel, strip_line_comments(&text)));
                }
            }
        }
    }
    let root = repo_root();
    let mut out = Vec::new();
    walk(dir, &root, &mut out);
    out
}

fn strip_line_comments(src: &str) -> String {
    src.lines()
        .map(|l| match l.find("//") {
            Some(i) => &l[..i],
            None => l,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// All production sources outside `crates/ml-pipeline`.
fn other_sources() -> Vec<(String, String)> {
    let root = repo_root();
    let mut all = rust_sources(&root.join("crates"));
    all.extend(rust_sources(&root.join("apps")));
    all.retain(|(path, _)| !path.starts_with("crates/ml-pipeline/"));
    all
}

/// Whether `word` appears in `text` as a whole identifier.
fn mentions(text: &str, word: &str) -> bool {
    let is_ident = |c: char| c.is_alphanumeric() || c == '_';
    text.match_indices(word).any(|(i, _)| {
        let before = text[..i].chars().next_back();
        let after = text[i + word.len()..].chars().next();
        !before.is_some_and(is_ident) && !after.is_some_and(is_ident)
    })
}

#[test]
fn the_scan_sees_the_sources_it_is_meant_to_guard() {
    let all = other_sources();
    for expected in [
        "apps/desktop/src-tauri/src/commands.rs",
        "crates/tools/src/lib.rs",
        "crates/ai/src/lib.rs",
    ] {
        assert!(
            all.iter().any(|(p, _)| p == expected),
            "{expected} was not scanned: {:?}",
            all.iter().map(|(p, _)| p).take(8).collect::<Vec<_>>()
        );
    }
}

#[test]
fn nothing_outside_ml_pipeline_names_the_downloader() {
    for (path, text) in other_sources() {
        for word in ["ModelStore", "fetched_model_path", "FetchObserver"] {
            assert!(
                !mentions(&text, word),
                "{path} names `{word}`. Only the user's click downloads a model, through \
                 `download_ml_model`; no tool, agent or startup code may fetch one \
                 (#383, CLAUDE.md)."
            );
        }
    }
}

#[test]
fn only_the_desktop_commands_drive_the_manager() {
    for (path, text) in other_sources() {
        if path == "apps/desktop/src-tauri/src/commands.rs" {
            continue;
        }
        assert!(
            !mentions(&text, "ModelDownloads"),
            "{path} names `ModelDownloads`; only the desktop commands may drive a model download"
        );
        assert!(
            !mentions(&text, "start_download"),
            "{path} calls `start_download`; only `download_ml_model` may"
        );
    }
}

/// The text of the body of `fn <name>` in `src`, braces matched, or `None`.
fn body_of(src: &str, name: &str) -> Option<(usize, usize)> {
    let needle = format!("fn {name}");
    let at = src
        .match_indices(&needle)
        .find(|(i, _)| {
            !src[i + needle.len()..]
                .chars()
                .next()
                .is_some_and(|c| c.is_alphanumeric() || c == '_')
        })?
        .0;
    let open = at + src[at..].find('{')?;
    let mut depth = 0usize;
    for (i, c) in src[open..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some((open, open + i + 1));
                }
            }
            _ => {}
        }
    }
    None
}

#[test]
fn start_download_is_called_in_one_place_and_it_is_the_download_command() {
    let (_, text) = other_sources()
        .into_iter()
        .find(|(p, _)| p == "apps/desktop/src-tauri/src/commands.rs")
        .expect("commands.rs");

    let (open, close) =
        body_of(&text, "download_ml_model").expect("the download_ml_model command exists");
    let calls: Vec<usize> = text
        .match_indices("start_download")
        .map(|(i, _)| i)
        .collect();
    assert_eq!(
        calls.len(),
        1,
        "`start_download` must be called once, by `download_ml_model`; found {} mentions",
        calls.len()
    );
    assert!(
        calls[0] > open && calls[0] < close,
        "`start_download` is called outside the body of `download_ml_model`"
    );
}

/// Byte offsets of every whole-identifier occurrence of `word` in `text`.
fn mention_offsets(text: &str, word: &str) -> Vec<usize> {
    let is_ident = |c: char| c.is_alphanumeric() || c == '_';
    text.match_indices(word)
        .map(|(i, _)| i)
        .filter(|&i| {
            let before = text[..i].chars().next_back();
            let after = text[i + word.len()..].chars().next();
            !before.is_some_and(is_ident) && !after.is_some_and(is_ident)
        })
        .collect()
}

/// The byte range of the delimited text that starts at `open_at` (which
/// holds `open`), up to and including its matching `close`.
fn delimited(text: &str, open_at: usize, open: char, close: char) -> (usize, usize) {
    let mut depth = 0usize;
    for (i, c) in text[open_at..].char_indices() {
        if c == open {
            depth += 1;
        } else if c == close {
            depth -= 1;
            if depth == 0 {
                return (open_at, open_at + i + close.len_utf8());
            }
        }
    }
    panic!("no matching {close:?} for the {open:?} at byte {open_at}");
}

/// The command function is never *called*, only defined and registered.
///
/// `download_ml_model` is an ordinary `async fn`, so any code in the desktop
/// crate could call it (`download_ml_model(state, "whisper-base".into())`
/// from another command, or `crate::commands::download_ml_model(..)` from
/// another module) and start a transfer without a click. So the name may
/// appear in exactly two kinds of place: its own `fn` in `commands.rs`, and
/// in `lib.rs`, the `use crate::commands::{..}` list and the
/// `generate_handler![..]` list that registers it. Anywhere else, in any
/// crate or app, is a path to a download that no click asked for.
#[test]
fn the_download_command_is_defined_and_registered_and_never_called() {
    const COMMAND: &str = "download_ml_model";
    let sources = other_sources();

    for (path, text) in &sources {
        let hits = mention_offsets(text, COMMAND);
        match path.as_str() {
            "apps/desktop/src-tauri/src/commands.rs" => {
                assert_eq!(
                    hits.len(),
                    1,
                    "{path} must mention `{COMMAND}` once, in its `fn`: it is called by no code"
                );
                assert!(
                    text[..hits[0]].trim_end().ends_with("fn"),
                    "{path} mentions `{COMMAND}` somewhere that is not its `fn` definition"
                );
            }
            "apps/desktop/src-tauri/src/lib.rs" => {
                let use_at = text
                    .find("use crate::commands::{")
                    .expect("lib.rs imports the commands");
                let use_open = use_at + "use crate::commands::".len();
                let imports = delimited(text, use_open, '{', '}');
                let handler_at = text
                    .find("generate_handler![")
                    .expect("lib.rs registers the commands");
                let handlers = delimited(text, handler_at + "generate_handler!".len(), '[', ']');
                for at in hits {
                    let registered = [imports, handlers]
                        .iter()
                        .any(|&(from, to)| at > from && at < to);
                    assert!(
                        registered,
                        "{path} mentions `{COMMAND}` outside the `use` list and \
                         `generate_handler!`: a launch must never begin a model download"
                    );
                    assert!(
                        !text[at + COMMAND.len()..].trim_start().starts_with('('),
                        "{path} calls `{COMMAND}`"
                    );
                }
            }
            _ => assert!(
                hits.is_empty(),
                "{path} mentions `{COMMAND}`. Only the user's click may reach it, through the \
                 window's IPC: no tool, agent path or other command may call it (#383)."
            ),
        }
    }
}

#[test]
fn startup_and_the_setup_hook_do_not_download() {
    // `lib.rs` builds the app: its setup runs at every launch. It may
    // register the commands and manage the state they use, and nothing
    // more. (That `download_ml_model` is not called from it is checked
    // above.)
    let (_, lib) = other_sources()
        .into_iter()
        .find(|(p, _)| p == "apps/desktop/src-tauri/src/lib.rs")
        .expect("lib.rs");
    for word in ["start_download", "ModelDownloads"] {
        assert!(
            !lib.contains(word),
            "lib.rs mentions `{word}`: a launch must never begin a model download"
        );
    }
}
