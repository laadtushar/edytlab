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
//!
//! The stale-count guard reaches past the website into `docs/` as well.
//! It was website-only by construction — rooted at `../../website` —
//! which is how the *marketing* copy came to be the accurate one while
//! `docs/README.md` and `docs/architecture.md` said 28.

use std::collections::BTreeSet;
use std::path::PathBuf;

use tools::ToolDispatcher;

fn read_website(relative: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../website")
        .join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

/// Walk a directory for authored text, as `(path relative to `label`,
/// contents)`.
///
/// Walked rather than listed: a claim on a page nobody thought to name
/// is exactly the failure this file exists to prevent.
fn text_sources(dir: &str, label: &str) -> Vec<(String, String)> {
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

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(dir);
    let mut out = Vec::new();
    walk(&root, &root, &mut out);
    assert!(
        !out.is_empty(),
        "found no sources under {} — the layout moved and these guards are \
         now reading nothing",
        root.display()
    );
    out.into_iter()
        .map(|(rel, text)| (format!("{label}/{rel}"), text))
        .collect()
}

/// Every text file on the marketing site.
fn website_sources() -> Vec<(String, String)> {
    text_sources("website", "website")
}

/// Every authored doc in `docs/`.
///
/// The checked-in docs were outside every guard here — `website_sources`
/// roots at `../../website`, so the site was covered by construction and
/// the repo's own documentation was not. The result was that the
/// *marketing* copy was the accurate one and `docs/` understated the
/// toolbox by 4.5×, which is backwards.
fn repo_doc_sources() -> Vec<(String, String)> {
    text_sources("docs", "docs")
        .into_iter()
        // Dated plan and spec records describe what was intended at the
        // time. Rewriting them to match today would be a different kind
        // of lie, the same reason the changelog is exempt below.
        // HANDOVER.md is the same case in a file of its own: its §4 is
        // headed "Build phasing (recap)" and its §5 is a table of
        // contents for the spec, so both of its counts describe
        // documents rather than the product.
        .filter(|(rel, _)| {
            !rel.contains("superpowers/plans")
                && !rel.contains("specs/")
                && !rel.ends_with("HANDOVER.md")
        })
        .collect()
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

/// Tool names inside the scroll story's `const TOOLS = [...]`.
///
/// A separate reader from `names_in_tools_arrays` because the story
/// holds a bare array rather than the catalogue's `tools: [...]` inside
/// a group object. Matching on the shape each file actually has beats
/// contorting one of them to suit a parser.
fn names_in_story_tools(src: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let Some(i) = src.find("const TOOLS = [") else {
        return out;
    };
    let rest = &src[i + "const TOOLS = [".len()..];
    let end = rest.find(']').expect("`const TOOLS = [` is never closed");
    for part in rest[..end].split('"').skip(1).step_by(2) {
        if !part.is_empty()
            && part
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        {
            out.insert(part.to_owned());
        }
    }
    out
}

/// The scroll story names the tools it shows running, and a name there
/// is a promise like any other.
///
/// This exists because it already went wrong: the story shipped a chip
/// reading `duck_under_speech` while that tool was still sitting in an
/// unmerged branch. The catalogue guard below did not catch it — that
/// one reads a single named file and this was a different file. Same
/// failure the count walk was widened to prevent, one surface over.
#[test]
fn the_scroll_story_invents_no_tools() {
    let registered = registered();
    let listed = names_in_story_tools(&read_website("components/story/scroll-story.tsx"));

    assert!(
        !listed.is_empty(),
        "found no tool names in scroll-story.tsx — the `const TOOLS = [...]` \
         shape changed and this test is no longer reading anything"
    );

    let invented: Vec<_> = listed.difference(&registered).cloned().collect();
    assert!(
        invented.is_empty(),
        "the scroll story shows {} tool(s) running that the agent cannot \
         call: {invented:?}",
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

    for (rel, src) in website_sources().into_iter().chain(repo_doc_sources()) {
        if rel.contains("changelog") {
            continue;
        }
        for (count, phrase) in tool_counts_claimed(&src) {
            if count != n.to_string() {
                wrong.push(format!("{rel}: says \"{phrase}\""));
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

/// Every "N … tools" claim in `src`, as `(count, phrase as written)`.
///
/// The count and the word are usually separated: "All 85 audio-editing
/// tools", "all 85 agent-callable audio tools". This walked back over
/// *immediately preceding* digits, so both of those — the meta and
/// OpenGraph descriptions of `/docs/tools`, which are what search
/// results and link previews render — sat two lines above a body that
/// said 93 while every test reported green.
///
/// So it now steps back over whole words. The limit of three is what
/// separates a description of the toolbox from a number that happens to
/// share a sentence with it, and any non-word character ends the walk,
/// which keeps a claim from reaching across a clause boundary into an
/// unrelated figure.
fn tool_counts_claimed(src: &str) -> Vec<(String, String)> {
    /// A word is what a count can be separated from "tools" by:
    /// `agent-callable`, `audio-editing`, `built_in`.
    fn is_word(c: char) -> bool {
        c.is_ascii_alphanumeric() || c == '-' || c == '_'
    }

    let chars: Vec<char> = src.chars().collect();
    let mut found = Vec::new();

    for (byte_i, _) in src.match_indices(" tools") {
        // "tools" must end the word: "toolset" is not a count.
        let after = byte_i + " tools".len();
        if src[after..].chars().next().is_some_and(is_word) {
            continue;
        }
        // `match_indices` gives byte offsets; the walk is over chars.
        // Start just past the space the match began with, so the first
        // step has a separator to consume like every other step.
        let mut i = src[..byte_i].chars().count() + 1;

        // Up to three intervening words, then give up.
        let mut words: Vec<String> = Vec::new();
        for _ in 0..4 {
            // The separating whitespace. Its absence means the previous
            // token ran straight into this one.
            let before = i;
            while i > 0 && chars[i - 1].is_whitespace() {
                i -= 1;
            }
            if i == before {
                break;
            }
            let end = i;
            while i > 0 && is_word(chars[i - 1]) {
                i -= 1;
            }
            if i == end {
                // Punctuation, a quote, a `·` — not part of the claim.
                break;
            }
            let token: String = chars[i..end].iter().collect();
            if token.chars().all(|c| c.is_ascii_digit()) {
                // A count is written after a space, a bracket or a
                // quote. A digit run glued to anything else is part of
                // something larger: `v0.1.0`, `SOME_VAR=1`, `tools/93`.
                let starts_a_word = i == 0
                    || chars[i - 1].is_whitespace()
                    || matches!(chars[i - 1], '(' | '[' | '"' | '\'' | '“' | '~');
                if !starts_a_word {
                    break;
                }
                words.insert(0, token.clone());
                found.push((token, format!("{} tools", words.join(" "))));
                break;
            }
            if token.chars().any(|c| c.is_ascii_digit()) {
                // `M26`, `x86_64` — not a count either.
                break;
            }
            words.insert(0, token);
        }
    }

    found
}

#[cfg(test)]
mod count_claim_tests {
    use super::tool_counts_claimed;

    /// The two forms that shipped stale in the `/docs/tools` metadata.
    #[test]
    fn reads_a_count_separated_from_the_word_by_other_words() {
        assert_eq!(
            tool_counts_claimed("All 85 audio-editing tools available to the agent"),
            [("85".to_string(), "85 audio-editing tools".to_string())]
        );
        assert_eq!(
            tool_counts_claimed("Complete reference for all 85 agent-callable audio tools."),
            [(
                "85".to_string(),
                "85 agent-callable audio tools".to_string()
            )]
        );
    }

    #[test]
    fn still_reads_the_plain_form() {
        assert_eq!(
            tool_counts_claimed("93 tools the agent can reach"),
            [("93".to_string(), "93 tools".to_string())]
        );
    }

    /// A number in a neighbouring clause is not a claim about the
    /// toolbox. Stopping at punctuation is what keeps this apart.
    #[test]
    fn does_not_reach_across_punctuation() {
        assert!(tool_counts_claimed("// ── 3 · The tools run ──").is_empty());
        assert!(tool_counts_claimed("Step 4. The tools then run").is_empty());
    }

    #[test]
    fn ignores_versions_and_unrelated_prose() {
        assert!(tool_counts_claimed("the v0.1.0 release tools").is_empty());
        assert!(tool_counts_claimed("the tools the agent can call").is_empty());
    }

    /// Three words is the limit; past that a number is not describing
    /// the noun any more.
    #[test]
    fn gives_up_after_three_intervening_words() {
        assert!(
            tool_counts_claimed("85 of the very many audio tools").is_empty(),
            "walked too far back to still be reading one claim"
        );
    }
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

/// The landing page's headline claims must outlive nothing.
///
/// The tool lists above are checked by name, which catches a tool being
/// renamed away. It does not catch the other direction: prose that
/// describes a *capability* staying on the site after the capability is
/// gone. That copy is the most damaging kind to get wrong, because it
/// is what someone decides to download on.
///
/// So each claim here is pinned to the tool that makes it true. The
/// pairing is the point — remove the tool and the sentence fails, which
/// is the only moment anyone would think to reword it.
#[test]
fn the_landing_page_claims_only_what_the_registry_can_do() {
    let registry = registered();
    let grid = read_website("components/landing/feature-grid.tsx");

    // (a phrase on the site, the tool that has to exist for it to be true)
    let claims: &[(&str, &str)] = &[
        // Text-based editing (#157): the transcript is the editor, and
        // deleting words cuts the audio.
        ("Edit the words, not the waveform", "cut_words"),
        ("the transcript becomes the editor", "transcribe"),
        // Stem separation and the phase vocoder.
        ("Demucs stem separation", "separate_stems"),
        ("Time-stretch", "time_stretch"),
        ("pitch-shift", "pitch_shift"),
        ("Warp a performance onto a beat grid", "align_to_beat"),
        // Export claims.
        ("Loudness-normalise", "normalize_loudness"),
        // History.
        ("Fork, A/B compare, and revert", "fork_node"),
    ];

    let mut broken = Vec::new();
    for (phrase, tool) in claims {
        if !grid.contains(phrase) {
            // The copy was reworded. That is allowed — but this test
            // has to be told, or it silently stops guarding anything.
            broken.push(format!(
                "the feature grid no longer says {phrase:?}; update this test \
                 to match the new wording (or drop the claim)"
            ));
        } else if !registry.contains(*tool) {
            broken.push(format!(
                "the feature grid claims {phrase:?} but `{tool}` is not \
                 registered — that is marketing copy for something the agent \
                 cannot do"
            ));
        }
    }

    assert!(broken.is_empty(), "{}", broken.join("\n  "));
}
