//! No code that ships opens a store behind the registry's back (#566).
//!
//! `Store::open` (and `Registry::open_unregistered`, which it calls) is the
//! raw open: it does not look for a live store for the folder, does not
//! become one and holds no claim, so two writers on one project folder is
//! exactly what it allows. It is there for a folder only the caller can reach
//! (a test's temporary folder). The opens that protect a project are
//! `Store::open_shared` (the project the window has, the agent works in and
//! every command reaches), `Store::open_private` (a folder of the code's own)
//! and `Store::open_read_only` (a reader).
//!
//! ## What keeps the raw open out of shipped code
//!
//! Two things, and neither is the whole of it.
//!
//! 1. **The `raw-open` feature of `session`.** The raw open exists only in
//!    `session`'s own test build and with that feature, and the crates that
//!    use it in tests turn it on in their `[dev-dependencies]` only. So a
//!    plain build (`cargo build`, `cargo check`, every release build) of a
//!    crate that calls it fails to compile. But Cargo's resolver unifies a
//!    dev-dependency's features into the normal build whenever the
//!    dev-dependency is being built, and `cargo test` and `cargo clippy
//!    --all-targets` build it. The CI steps that run are those, so the
//!    feature alone would not fail them, and the first place it fails is a
//!    release build.
//! 2. **This test.** It reads the source of every crate and app that ships
//!    and fails on a use of the raw open outside test code. It lexes the
//!    source (comments, string and char literals are not code) and removes
//!    every item marked `#[cfg(test)]` or `#[cfg(all(test, ..))]` by matching
//!    its braces, so a `#[cfg(test)]` helper at the top of a file hides
//!    that helper and nothing after it. It catches `Store::open(`,
//!    `Store :: open (`, `.map(Store::open)`, `<Store>::open`, `use
//!    ...::Store::open`, `use ...::Store::{open, ..}`, an alias (`use
//!    session::Store as S; S::open(..)`, `type S = session::Store;`), and
//!    `open_unregistered` and `with_identity` (a registry of one's own).
//!
//! What it does not see: a raw open reached through something that is not
//! spelled `Store` (a macro that builds the path, a trait object, a generic).
//! `tests/` folders and `session`'s own `src` (which defines the open) are
//! not scanned. A file that a `#[cfg(test)] mod name;` pulls in *is* scanned,
//! like any other `src` file: the declaration is removed but the file is not
//! known to be test-only. There is none today; one that used the raw open
//! would fail this test, loudly, and would be a reason to teach it.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

#[path = "../../../test-support/rust_source_scan.rs"]
mod rust_source_scan;
use rust_source_scan::{is_punct, lex, past_group, without_test_items, Tok};

// =============================================================================
// Finding the raw open
// =============================================================================

/// Where `source` uses the raw open outside test code, as (line, what).
fn unregistered_opens(source: &str) -> Vec<(usize, String)> {
    let tokens = without_test_items(&lex(source));
    let ident = |i: usize| match tokens.get(i).map(|t| &t.tok) {
        Some(Tok::Ident(n)) => Some(n.as_str()),
        _ => None,
    };
    let punct = |i: usize, c: char| tokens.get(i).is_some_and(|t| is_punct(t, c));
    let path_sep = |i: usize| punct(i, ':') && punct(i + 1, ':');

    // Names that mean `Store`: itself, and what it is renamed or aliased to.
    let mut names: HashSet<String> = HashSet::from(["Store".to_string()]);
    for i in 0..tokens.len() {
        // `use ...::Store as Name`
        if ident(i) == Some("Store") && ident(i + 1) == Some("as") {
            if let Some(alias) = ident(i + 2) {
                names.insert(alias.to_string());
            }
        }
        // `type Name = ...Store;`
        if ident(i) == Some("type") && punct(i + 2, '=') {
            let mut j = i + 3;
            while j < tokens.len() && !punct(j, ';') {
                if ident(j) == Some("Store") {
                    if let Some(alias) = ident(i + 1) {
                        names.insert(alias.to_string());
                    }
                }
                j += 1;
            }
        }
    }

    let mut found = Vec::new();
    for i in 0..tokens.len() {
        let line = tokens[i].line;
        match ident(i) {
            Some(raw @ ("open_unregistered" | "with_identity")) => {
                found.push((line, raw.to_string()));
            }
            Some(name) if names.contains(name) => {
                // `Store::open`, and `<Store>::open`.
                let after = if punct(i + 1, '>') { i + 2 } else { i + 1 };
                if path_sep(after) && ident(after + 2) == Some("open") {
                    found.push((line, format!("{name}::open")));
                }
                // `Store::{open, ..}`
                if path_sep(i + 1) && punct(i + 3, '{') {
                    let end = past_group(&tokens, i + 3);
                    if (i + 4..end).any(|j| ident(j) == Some("open")) {
                        found.push((line, format!("{name}::{{open}}")));
                    }
                }
            }
            _ => {}
        }
    }
    found
}

// =============================================================================
// Which files
// =============================================================================

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

#[test]
fn nothing_that_ships_calls_the_raw_open() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    let session_src = repo.join("crates").join("session").join("src");
    let mut offenders = Vec::new();
    let mut scanned = 0;
    let mut commands_rs = false;
    for root in source_roots(&repo) {
        let mut files = Vec::new();
        rust_files(&root, &mut files);
        for file in files {
            if file.starts_with(&session_src) {
                continue;
            }
            scanned += 1;
            commands_rs |= file.ends_with("src-tauri/src/commands.rs");
            let source = fs::read_to_string(&file).unwrap();
            for (line, what) in unregistered_opens(&source) {
                let rel = file.strip_prefix(&repo).unwrap_or(&file);
                offenders.push(format!("{}:{line}: {what}", rel.display()));
            }
        }
    }
    assert!(
        scanned > 50,
        "the scan found only {scanned} files: wrong roots?"
    );
    assert!(
        commands_rs,
        "the desktop app's commands.rs, the largest file, was not scanned: wrong roots?"
    );
    assert!(
        offenders.is_empty(),
        "the raw open consults no registry, so it can make a second writer on a project folder \
         (#566). Use `Store::open_shared` for the project the app has open, \
         `Store::open_private` for a folder of your own, `Store::open_read_only` for a reader:\n{}",
        offenders.join("\n")
    );
}

// =============================================================================
// The scan itself
// =============================================================================

fn lines_of(source: &str) -> Vec<usize> {
    unregistered_opens(source)
        .into_iter()
        .map(|(line, _)| line)
        .collect()
}

#[test]
fn the_scan_finds_a_call_and_skips_tests_comments_and_strings() {
    let source = "\
//! `Store::open(dir)` in a doc comment is fine.
fn a() {
    // Store::open(dir) in a comment too.
    /* and /* nested */ Store::open(dir) in a block comment */
    let msg = \"Store::open(dir) in a string\";
    let raw = r#\"Store::open(dir) \" in a raw string\"#;
    let s = session::Store::open(dir).unwrap();
}

#[cfg(test)]
mod tests {
    fn b() { Store::open(dir).unwrap(); }
}
";
    assert_eq!(lines_of(source), vec![7]);
    assert!(unregistered_opens("fn c() { Store::open_private(d) }").is_empty());
    assert!(unregistered_opens("fn c() { Store::open_shared(d) }").is_empty());
}

/// The hole the first version of this scan had: it stopped reading a file at
/// the first `#[cfg(test)]`, and a `#[cfg(test)]` on a helper, a field or a
/// `fn` near the top of a file hid everything after it.
#[test]
fn a_cfg_test_item_hides_that_item_and_nothing_after_it() {
    let source = "\
use std::path::Path;

#[cfg(test)]
fn helper() { let _ = Store::open(p); }

pub struct State {
    #[cfg(test)]
    pub probe: Option<Store>,
    pub store: Option<u8>,
}

#[cfg(test)]
use session::Store as Hidden;

#[cfg(all(test, unix))]
const FIXTURE: Fixture = Fixture { store: Store::open(p) };

#[cfg_attr(test, derive(Debug))]
pub fn production() {
    let _ = Store::open(p);
}
";
    assert_eq!(lines_of(source), vec![20]);
}

/// A comma inside generic arguments, a tuple type or a `where` clause is not
/// the end of the item: what follows it is still the item's.
#[test]
fn commas_in_generics_and_where_clauses_do_not_end_a_test_only_item() {
    let source = "\
struct S {
    #[cfg(test)]
    probe: HashMap<String, Vec<u8>>,
    #[cfg(test)]
    pair: Option<(u8, u8)>,
    live: u8,
}

#[cfg(test)]
fn helper() -> Result<(Store, u8), E> { Store::open(p) }

#[cfg(test)]
fn helper2<T, U>(a: T) -> U where T: A, U: B { Store::open(p) }

fn production() { Store::open(p); }
";
    assert_eq!(lines_of(source), vec![15]);
}

#[test]
fn cfg_not_test_and_cfg_any_test_are_production_code() {
    let source = "\
#[cfg(not(test))]
fn a() { Store::open(p); }
#[cfg(any(test, feature = \"x\"))]
fn b() { Store::open(p); }
";
    assert_eq!(lines_of(source), vec![2, 4]);
}

#[test]
fn a_whole_file_marked_cfg_test_is_skipped() {
    assert!(unregistered_opens("#![cfg(test)]\nfn a() { Store::open(p); }").is_empty());
}

#[test]
fn other_spellings_of_the_raw_open_are_found() {
    for (what, source) in [
        ("spaced", "fn a() { Store :: open (p); }"),
        (
            "a function value",
            "fn a() { let _ = paths.map(Store::open); }",
        ),
        (
            "a qualified path",
            "fn a() { session::Store::open(p).unwrap(); }",
        ),
        ("an angle path", "fn a() { <session::Store>::open(p); }"),
        ("a use", "use session::Store::open;"),
        ("a use list", "use session::Store::{open, open_shared};"),
        (
            "an alias",
            "use session::Store as S;\nfn a() { S::open(p); }",
        ),
        (
            "a type alias",
            "type S = session::Store;\nfn a() { S::open(p); }",
        ),
        (
            "the registry's",
            "fn a() { Registry::global().open_unregistered(p); }",
        ),
        (
            "a registry of its own",
            "fn a() { Registry::with_identity(identify).open_shared(p); }",
        ),
    ] {
        assert_eq!(
            unregistered_opens(source).len(),
            1,
            "{what} was not found in: {source}"
        );
    }
}

#[test]
fn a_lifetime_is_not_a_char_literal() {
    let source = "\
fn a<'a>(x: &'a str) -> &'a str { x }
fn b() { let c = 'x'; let d = '\\n'; Store::open(p); }
";
    assert_eq!(lines_of(source), vec![2]);
}
