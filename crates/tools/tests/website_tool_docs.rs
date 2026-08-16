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

/// The headline count on the landing page has to be the real one.
///
/// "75 tools" is the claim the catalogue below it exists to substantiate;
/// if the registry grows and the number does not, the page undersells,
/// and if the registry shrinks it lies.
#[test]
fn the_advertised_tool_count_matches_the_registry() {
    let n = registered().len();
    let catalogue = read_website("components/landing/tool-catalogue.tsx");
    let stats = read_website("components/landing/stats-strip.tsx");

    assert!(
        catalogue.contains(&format!("{n} tools")),
        "tool-catalogue.tsx does not say \"{n} tools\"; the registry has \
         {n} registered tools and the heading has drifted"
    );
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
