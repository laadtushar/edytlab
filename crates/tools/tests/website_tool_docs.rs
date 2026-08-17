//! The website's tool documentation must cover exactly the registered
//! tools.
//!
//! Two pages on the marketing site enumerate the toolbox: the docs
//! reference at `website/app/docs/tools/page.tsx`, and the landing
//! section at `website/components/landing/tool-catalogue.tsx`. Nothing
//! tied either to the dispatcher, and the reference had drifted — it
//! listed 69 of 75, missing exactly the six that shipped most recently
//! (`create_bus`, `set_send`, `remove_send`, `move_clip`,
//! `remove_clip`, `normalize_loudness`).
//!
//! That is the same failure `tool_badge_labels.rs` exists to stop, one
//! surface over. This is its sibling, and it is deliberately the same
//! crude shape: read the `.tsx` as text and compare name sets. Anything
//! cleaner — generating the pages from the registry, say — is a bigger
//! change than the problem warrants, and a regex over a hand-maintained
//! literal is enough to make the next new tool fail loudly instead of
//! quietly going undocumented.
//!
//! The two pages are held to different standards on purpose:
//!
//! * The **docs reference** must be exhaustive. It is the page a reader
//!   consults to find out whether something exists, so a missing entry
//!   is a wrong answer.
//! * The **landing catalogue** is a highlight reel and may omit tools,
//!   but may never *invent* one. It exists to make "75 tools" checkable,
//!   and a name on it that the agent cannot call would defeat that.
//!
//! Both directions matter. A name here that no longer exists in the
//! registry is as bad as a missing one: it promises a capability that
//! was renamed or removed.

use std::collections::BTreeSet;
use std::path::PathBuf;

use tools::ToolDispatcher;

fn read_website(relative: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../website")
        .join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

/// Every text file on the marketing site, as (relative path, contents).
///
/// Walked rather than listed: a claim on a page nobody thought to name
/// is exactly the failure this file exists to prevent.
fn website_sources() -> Vec<(String, String)> {
    fn walk(dir: &std::path::Path, root: &std::path::Path, out: &mut Vec<(String, String)>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if path.is_dir() {
                // Build output and dependencies are not authored copy.
                if name == "node_modules" || name == ".next" || name == "public" {
                    continue;
                }
                walk(&path, root, out);
            } else if matches!(
                path.extension().and_then(|e| e.to_str()),
                Some("tsx" | "ts" | "md" | "mdx")
            ) {
                if let Ok(text) = std::fs::read_to_string(&path) {
                    let rel = path
                        .strip_prefix(root)
                        .unwrap_or(&path)
                        .to_string_lossy()
                        .to_string();
                    out.push((rel, text));
                }
            }
        }
    }

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../website");
    let mut out = Vec::new();
    walk(&root, &root, &mut out);
    assert!(
        !out.is_empty(),
        "found no website sources under {} — the layout moved and these \
         guards are now reading nothing",
        root.display()
    );
    out
}

/// Every registered tool name, from the dispatcher itself.
fn registered() -> BTreeSet<String> {
    ToolDispatcher::default_dispatcher()
        .tool_schemas()
        .as_array()
        .expect("tool_schemas returns an array")
        .iter()
        .filter_map(|s| s["name"].as_str().map(str::to_owned))
        .collect()
}

/// Names quoted after a `name:` key — the shape both pages use for a
/// tool entry.
fn names_after_key(src: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for (i, _) in src.match_indices("name: \"") {
        let rest = &src[i + "name: \"".len()..];
        if let Some(end) = rest.find('"') {
            let name = &rest[..end];
            // Tool names are snake_case ASCII; the same key is used for
            // section titles and other prose on these pages, so the
            // shape is the filter.
            if !name.is_empty()
                && name
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
            {
                out.insert(name.to_owned());
            }
        }
    }
    out
}

/// Every string inside a `tools: [ ... ]` array literal.
fn names_in_tools_arrays(src: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut rest = src;
    while let Some(i) = rest.find("tools: [") {
        rest = &rest[i + "tools: [".len()..];
        let end = rest.find(']').expect("a `tools: [` is never closed");
        let block = &rest[..end];
        for (j, _) in block.match_indices('"') {
            let after = &block[j + 1..];
            if let Some(k) = after.find('"') {
                let name = &after[..k];
                if !name.is_empty()
                    && name
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
                {
                    out.insert(name.to_owned());
                }
            }
        }
        rest = &rest[end..];
    }
    out
}

/// The docs reference is the page a reader consults to find out whether
/// a capability exists. A missing entry is a wrong answer, so this is
/// exhaustive in both directions.
#[test]
fn the_docs_reference_covers_every_registered_tool() {
    let registered = registered();
    let documented = names_after_key(&read_website("app/docs/tools/page.tsx"));

    let missing: Vec<_> = registered.difference(&documented).cloned().collect();
    assert!(
        missing.is_empty(),
        "{} registered tool(s) are undocumented on the website's tools \
         reference; add them to website/app/docs/tools/page.tsx: {missing:?}",
        missing.len()
    );

    let invented: Vec<_> = documented.difference(&registered).cloned().collect();
    assert!(
        invented.is_empty(),
        "{} tool(s) documented on the website are not registered — they \
         were renamed or removed, and the page now promises something the \
         agent cannot do: {invented:?}",
        invented.len()
    );
}

/// The landing catalogue is a highlight reel: it may omit, but it may
/// never invent. Its whole job is making the tool count checkable.
#[test]
fn the_landing_catalogue_invents_no_tools() {
    let registered = registered();
    let listed = names_in_tools_arrays(&read_website("components/landing/tool-catalogue.tsx"));

    assert!(
        !listed.is_empty(),
        "found no tool names in tool-catalogue.tsx — the `tools: [...]` \
         shape changed and this test is no longer reading anything"
    );

    let invented: Vec<_> = listed.difference(&registered).cloned().collect();
    assert!(
        invented.is_empty(),
        "the landing page advertises {} tool(s) the agent cannot call: \
         {invented:?}",
        invented.len()
    );
}

/// **Every** number the site quotes has to be the real one.
///
/// This used to check two named files, and six stale claims lived
/// happily outside them: four pages still said "69 tools" and the
/// catalogue's own module comment said "75", while the registry had 81.
/// A guard that names its files can only ever cover the files someone
/// remembered to name.
///
/// So this scans the whole site instead. Any file that quotes a count is
/// covered the moment it is written, including pages that do not exist
/// yet — which is the only version of this test that stays true.
///
/// The changelog is exempt: its older entries describe what a past
/// release shipped, and rewriting history to match today would be a
/// different kind of lie.
#[test]
fn no_page_quotes_a_stale_tool_count() {
    let n = registered().len();
    let mut wrong: Vec<String> = Vec::new();

    for (rel, src) in website_sources() {
        if rel.contains("changelog") {
            continue;
        }
        for (i, _) in src.match_indices(" tools") {
            // Walk back over the digits immediately before " tools".
            let head = &src[..i];
            let digits: String = head
                .chars()
                .rev()
                .take_while(char::is_ascii_digit)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            if digits.is_empty() {
                continue;
            }
            if digits != n.to_string() {
                wrong.push(format!("{rel}: says \"{digits} tools\""));
            }
        }
    }

    assert!(
        wrong.is_empty(),
        "the registry has {n} tools; these claims disagree and would ship \
         as marketing copy that is simply false:\n  {}",
        wrong.join("\n  ")
    );
}

/// The stats strip shows the count as a bare figure rather than as
/// "N tools", so it needs its own check.
#[test]
fn the_stats_strip_shows_the_real_count() {
    let n = registered().len();
    let stats = read_website("components/landing/stats-strip.tsx");
    assert!(
        stats.contains(&format!("\"{n}\"")),
        "the stats strip does not show {n}; the registry has {n} \
         registered tools"
    );
}

/// A guard on the guards: if either page stops matching the shape these
/// tests parse, they would pass by reading nothing at all.
#[test]
fn the_parsers_still_find_something() {
    let documented = names_after_key(&read_website("app/docs/tools/page.tsx"));
    assert!(
        documented.len() > 50,
        "only found {} tool names in the docs reference — the `name: \"…\"` \
         shape changed and this test is reading almost nothing",
        documented.len()
    );
}
