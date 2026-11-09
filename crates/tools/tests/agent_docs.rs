//! `CLAUDE.md` stays an index, and its area guides stay reachable (#961).
//!
//! Every Claude session loads `CLAUDE.md` on every turn, so each line of it
//! costs on every turn whether or not the task needs it. #961 moved each
//! area's notes into a guide under `docs/agents/` and left `CLAUDE.md` with
//! what every task needs plus one pointer per guide. Two ways that can rot
//! without anyone noticing, each checked here:
//!
//! * notes are added back to `CLAUDE.md` instead of to a guide, and it grows
//!   until it is the 50 KB file #961 split up;
//! * a guide is added, renamed or removed without its pointer, so a guide no
//!   session is told to read, or a pointer to nothing.
//!
//! Text only, in the manner of `ml_workflow.rs` beside it.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// The most `CLAUDE.md` may hold, in bytes with `\n` line endings. It was
/// 14,379 B when this test was written (2026-11-08). Raise it only by what a
/// change measures it needs, and first ask whether that text belongs in a
/// guide under `docs/agents/` instead.
const CLAUDE_MD_MAX_BYTES: usize = 15_000;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Line endings as `\n`: on Windows git checks the repository out with
/// `\r\n`, which would count a byte per line more there than here.
fn read(relative: &str) -> String {
    let path = repo_root().join(relative);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
        .replace("\r\n", "\n")
}

/// The guides on disk, as `docs/agents/<name>.md`.
fn guides_on_disk() -> BTreeSet<String> {
    let dir = repo_root().join("docs/agents");
    std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("cannot list {}: {e}", dir.display()))
        .map(|entry| entry.expect("a directory entry").file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".md"))
        .map(|name| format!("docs/agents/{name}"))
        .collect()
}

/// Every `docs/agents/<name>.md` the text names in backticks.
fn guides_named(text: &str) -> BTreeSet<String> {
    text.split('`')
        .skip(1)
        .step_by(2)
        .filter(|span| span.starts_with("docs/agents/") && span.ends_with(".md"))
        .map(str::to_owned)
        .collect()
}

#[test]
fn claude_md_stays_within_its_budget() {
    let size = read("CLAUDE.md").len();
    assert!(
        size <= CLAUDE_MD_MAX_BYTES,
        "CLAUDE.md is {size} B, over its {CLAUDE_MD_MAX_BYTES} B budget. Every session loads it \
         on every turn: move area-specific notes to the guide under docs/agents/ that CLAUDE.md \
         points to for that area, or raise the budget by the measured amount if the text is \
         needed by every task."
    );
}

#[test]
fn every_guide_has_a_pointer_in_claude_md() {
    let named = guides_named(&read("CLAUDE.md"));
    let on_disk = guides_on_disk();
    let unpointed: Vec<_> = on_disk.difference(&named).collect();
    assert!(
        unpointed.is_empty(),
        "these guides are never named in CLAUDE.md, so no session is told to read them: \
         {unpointed:?}. Add a line to its \"Area guides\" list."
    );
}

#[test]
fn every_guide_claude_md_names_exists() {
    let named = guides_named(&read("CLAUDE.md"));
    let on_disk = guides_on_disk();
    let missing: Vec<_> = named.difference(&on_disk).collect();
    assert!(
        missing.is_empty(),
        "CLAUDE.md points to guides that do not exist: {missing:?}"
    );
    assert!(
        named.len() >= 10,
        "CLAUDE.md names only {} guides; the lookup above matched almost nothing",
        named.len()
    );
}
