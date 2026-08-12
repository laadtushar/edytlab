//! The chat badge's tool labels must cover exactly the registered tools.
//!
//! `ToolBadge.tsx` maps a dispatcher tool name to the words the user
//! reads while a tool runs. Nothing tied the two sides together, and the
//! map drifted in both directions at once: nine entries named tools that
//! had been renamed away (`cut` → `cut_range`, `copy` → `copy_region`,
//! `paste` → `paste_region`, `fade_in`/`fade_out` → `fade`, `fork` →
//! `fork_node`, `rename_node` → `name_node`, `set_volume` →
//! `set_track_gain`, `add_marker` → `label`), while 49 of the 69
//! registered tools had no entry at all and fell through to a fallback
//! that just strips underscores — so the badge read "de esser" and
//! "high pass filter".
//!
//! Neither half of that is visible from either side alone, which is why
//! the check lives here, in the crate that owns the tool list. The
//! sibling mapping on the Rust side (`category_for` in `commands.rs`)
//! stayed correct through the same renames precisely because it sits
//! next to the dispatcher.
//!
//! Reading the `.tsx` as text is deliberately crude. The alternative —
//! shipping labels from the backend — is a bigger change than the
//! problem warrants, and a regex over a hand-maintained literal is
//! enough to make the next rename fail loudly instead of silently.

use std::collections::BTreeSet;
use std::path::PathBuf;

use tools::ToolDispatcher;

/// `apps/desktop/src/components/ToolBadge.tsx`, relative to this crate.
fn tool_badge_source() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../apps/desktop/src/components/ToolBadge.tsx");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

/// Every key of the `TOOL_LABELS` object literal.
fn labelled_tools(src: &str) -> BTreeSet<String> {
    let start = src
        .find("const TOOL_LABELS")
        .expect("TOOL_LABELS was renamed or removed; update this test with it");
    let body = &src[start..];
    let end = body
        .find("\n};")
        .expect("TOOL_LABELS is not closed by a `};` at line start");
    body[..end]
        .lines()
        // `  some_tool: "Label",` — the leading spaces distinguish an
        // entry from the `const …` line and from comments.
        .filter_map(|line| {
            let line = line.trim();
            if line.starts_with("//") {
                return None;
            }
            let (key, rest) = line.split_once(':')?;
            if !rest.trim_start().starts_with('"') {
                return None;
            }
            let key = key.trim();
            if !key.is_empty()
                && key
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
            {
                Some(key.to_string())
            } else {
                None
            }
        })
        .collect()
}

fn registered_tools() -> BTreeSet<String> {
    let dispatcher = ToolDispatcher::default_dispatcher();
    let schemas = dispatcher.tool_schemas();
    schemas
        .as_array()
        .expect("tool_schemas returns an array")
        .iter()
        .filter_map(|s| s.get("name")?.as_str().map(str::to_string))
        .collect()
}

#[test]
fn every_registered_tool_has_a_badge_label() {
    let labelled = labelled_tools(&tool_badge_source());
    let registered = registered_tools();

    let missing: Vec<_> = registered.difference(&labelled).collect();
    assert!(
        missing.is_empty(),
        "{} tool(s) would render as raw snake_case in the chat badge; \
         add them to TOOL_LABELS in ToolBadge.tsx: {missing:?}",
        missing.len()
    );
}

#[test]
fn no_badge_label_names_a_tool_that_does_not_exist() {
    let labelled = labelled_tools(&tool_badge_source());
    let registered = registered_tools();

    let ghosts: Vec<_> = labelled.difference(&registered).collect();
    assert!(
        ghosts.is_empty(),
        "{} label(s) in ToolBadge.tsx name a tool the dispatcher does not \
         register — a rename left them behind: {ghosts:?}",
        ghosts.len()
    );
}

/// A guard on the guard: if the parse silently matched nothing, both
/// tests above would pass on an empty set and prove nothing.
#[test]
fn the_label_parser_actually_finds_labels() {
    let labelled = labelled_tools(&tool_badge_source());
    assert!(
        labelled.len() > 50,
        "only parsed {} labels out of ToolBadge.tsx — the object literal's \
         shape probably changed and this test is no longer reading it",
        labelled.len()
    );
    assert!(
        labelled.contains("load"),
        "parsed {} labels but not the `load` entry, which is the first one",
        labelled.len()
    );
}
