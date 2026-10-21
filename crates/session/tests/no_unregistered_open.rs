//! No code that ships opens a store behind the registry's back (#566).
//!
//! `Store::open` is the raw open: it does not look for a live store for the
//! folder, does not become one and holds no claim, so two writers on one
//! project folder is exactly what it allows. It is there for a folder only
//! the caller can reach (a test's temporary folder), and the opens that
//! protect a project are `Store::open_shared` (the project the window has,
//! the agent works in and every command reaches), `Store::open_private` (a
//! folder of the code's own) and `Store::open_read_only` (a reader).
//!
//! Nothing in the type system stops a call to `Store::open` in a command or
//! a tool, so this does: a `Store::open(` in the non-test source of any crate
//! or app fails here. The next one to need a store opens it through the
//! registry, and one that really does have a folder to itself says so by
//! `open_private`, which costs nothing.
//!
//! Test code is exempt. By this repo's convention a file's tests come after
//! its first `#[cfg(test)]`, so the scan stops there; `tests/` folders and
//! `session`'s own `src` (which defines the open) are not scanned.

use std::fs;
use std::path::{Path, PathBuf};

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// The `src` folders that ship: every crate's, every app's, and the desktop
/// app's Rust backend.
fn source_roots(repo: &Path) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for group in ["crates", "apps"] {
        let Ok(entries) = fs::read_dir(repo.join(group)) else {
            continue;
        };
        for entry in entries.flatten() {
            let member = entry.path();
            roots.push(member.join("src"));
            roots.push(member.join("src-tauri").join("src"));
        }
    }
    roots
}

/// Lines of `source` that call `Store::open(` outside a comment, before the
/// first `#[cfg(test)]`.
fn unregistered_opens(source: &str) -> Vec<(usize, &str)> {
    let mut found = Vec::new();
    for (n, line) in source.lines().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("#[cfg(test)]") {
            break;
        }
        if trimmed.starts_with("//") {
            continue;
        }
        if line.contains("Store::open(") {
            found.push((n + 1, line.trim()));
        }
    }
    found
}

#[test]
fn nothing_that_ships_calls_the_raw_open() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    let session_src = repo.join("crates").join("session").join("src");
    let mut offenders = Vec::new();
    let mut scanned = 0;
    for root in source_roots(&repo) {
        let mut files = Vec::new();
        rust_files(&root, &mut files);
        for file in files {
            if file.starts_with(&session_src) {
                continue;
            }
            scanned += 1;
            let source = fs::read_to_string(&file).unwrap();
            for (line, text) in unregistered_opens(&source) {
                let rel = file.strip_prefix(&repo).unwrap_or(&file);
                offenders.push(format!("{}:{line}: {text}", rel.display()));
            }
        }
    }
    assert!(
        scanned > 50,
        "the scan found only {scanned} files: wrong roots?"
    );
    assert!(
        offenders.is_empty(),
        "`Store::open` does not consult the registry, so it can make a second writer on a project \
         folder (#566). Use `Store::open_shared` for the project the app has open, \
         `Store::open_private` for a folder of your own, `Store::open_read_only` for a reader:\n{}",
        offenders.join("\n")
    );
}

#[test]
fn the_scan_finds_a_call_and_skips_tests_and_comments() {
    let source = "\
//! `Store::open(dir)` in a doc comment is fine.
fn a() {
    // Store::open(dir) in a comment too.
    let s = session::Store::open(dir).unwrap();
}

#[cfg(test)]
mod tests {
    fn b() { Store::open(dir).unwrap(); }
}
";
    assert_eq!(
        unregistered_opens(source),
        vec![(4, "let s = session::Store::open(dir).unwrap();")]
    );
    assert!(unregistered_opens("fn c() { Store::open_private(d) }").is_empty());
}
