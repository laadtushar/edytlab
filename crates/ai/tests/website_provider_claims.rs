//! The website's provider prose must match the provider registry (#261).
//!
//! The landing FAQ told visitors that Ollama was *"planned for v1 phase
//! 3"* and that local models got *"a simplified tool surface"*. Both
//! were untrue when it was written: Ollama shipped in #126, and no
//! provider-conditioned tool narrowing existed — the only whitelist was
//! keyed on the active agent profile. Meanwhile four other places on the
//! same site advertised Ollama as working, including the changelog entry
//! announcing it.
//!
//! Since #395 the second claim is true: Ollama is sent a smaller set of
//! tools (`LlmProvider::tool_set`), so "a simplified tool surface" is no
//! longer banned below. Only the "planned" phrases are. The local-model
//! page describes the real thing, and a test here ties that sentence to
//! the code, so the claim cannot outlive the behaviour.
//!
//! Someone evaluating the app for offline use reads the answer written
//! for exactly that question and is told the capability is unbuilt, on
//! the same scroll that advertises it. Whichever they believe, one is
//! wrong — and the stale one is the one that stops the download.
//!
//! This is the provider analogue of
//! `the_landing_page_claims_only_what_the_registry_can_do` in
//! `crates/tools/tests/website_tool_docs.rs`. It lives here rather than
//! there because `SUPPORTED_PROVIDER_IDS` is declared in this crate and
//! `tools` cannot see it — the dependency runs the other way.
//!
//! The repo has form for this drift: `9b3de78 docs(website): the app
//! supports five LLM providers, not three (#100)`.

use std::path::PathBuf;

use ai::SUPPORTED_PROVIDER_IDS;

fn read_website(relative: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../website")
        .join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

/// How each provider id is spelled in prose. A registry id is a slug;
/// the site writes a name.
///
/// Every id in `SUPPORTED_PROVIDER_IDS` must appear here or the test
/// below fails — that is how adding a seventh provider is made to
/// surface as a website task rather than being forgotten.
fn display_name(id: &str) -> Option<&'static str> {
    match id {
        "anthropic" => Some("Anthropic"),
        "openrouter" => Some("OpenRouter"),
        "openai" => Some("OpenAI"),
        "groq" => Some("Groq"),
        "gemini" => Some("Gemini"),
        "ollama" => Some("Ollama"),
        _ => None,
    }
}

fn faq() -> String {
    read_website("components/landing/faq.tsx")
}

/// The answer to one FAQ question, by a distinctive fragment of the
/// question itself.
///
/// Scoped rather than searching the whole file, and that is the point:
/// the first version of this test looked at the file as a whole and
/// passed against the exact drift #261 reports, because "Ollama"
/// appeared in the *stale* answer two questions further down. A guard
/// that the reported bug walks straight through is not a guard.
fn answer_to(question_fragment: &str) -> String {
    let src = faq();
    let q = src
        .find(question_fragment)
        .unwrap_or_else(|| panic!("no FAQ question containing {question_fragment:?}"));
    let a = src[q..]
        .find("a: ")
        .unwrap_or_else(|| panic!("question {question_fragment:?} has no answer"));
    let rest = &src[q + a..];
    // Answers are single-line string literals in the `faqs` array, so
    // the line is the answer.
    rest.lines().next().unwrap_or_default().to_string()
}

/// Every supported provider has a prose spelling here.
///
/// Asserted first and separately: without it, adding a provider whose
/// name nobody wrote down would make the test below silently skip it,
/// and a guard that skips the new case is worse than no guard.
#[test]
fn every_registered_provider_has_a_name_the_site_could_use() {
    let missing: Vec<_> = SUPPORTED_PROVIDER_IDS
        .iter()
        .filter(|id| display_name(id).is_none())
        .collect();
    assert!(
        missing.is_empty(),
        "these providers are registered but `display_name` above does not know how the website \
         would write them: {missing:?}. Add them — and then check the FAQ actually mentions them."
    );
}

/// The FAQ's "Which LLM models work?" answer lists every provider that
/// works.
///
/// Omission is the failure mode this catches, and it is the quiet one:
/// the answer read perfectly well while leaving out the provider the
/// next question was about.
#[test]
fn the_faq_lists_every_supported_provider() {
    let answer = answer_to("Which LLM models work");
    let missing: Vec<_> = SUPPORTED_PROVIDER_IDS
        .iter()
        .filter_map(|id| display_name(id))
        .filter(|name| !answer.contains(name))
        .collect();
    assert!(
        missing.is_empty(),
        "the FAQ answer to \"Which LLM models work?\" omits {missing:?}, but {} are registered \
         providers a user can select in settings. That answer is the provider list someone \
         evaluating the app reads.\nanswer was: {answer}",
        missing.len()
    );
}

/// Ollama is described as shipped, not planned.
///
/// Pinned to the registry rather than asserted flat: if Ollama is ever
/// removed as a provider, this stops demanding the site claim it.
#[test]
fn the_faq_does_not_call_a_shipped_provider_unbuilt() {
    if !SUPPORTED_PROVIDER_IDS.contains(&"ollama") {
        return; // Removed as a provider; the site is free to say so.
    }
    let src = faq();

    // The stale "planned" sentences from #261, and the general shape they
    // belong to. "phase 3" is the one that was actually there; the rest is
    // the way the same claim tends to get rewritten.
    for stale in [
        "planned for v1 phase 3",
        "Ollama (Qwen, Llama 3, etc.) are planned",
    ] {
        assert!(
            !src.contains(stale),
            "the FAQ still says {stale:?}. Ollama is a registered provider, shipped since #126; \
             the tool narrowing it gets is described on the local-model page, not promised here."
        );
    }
}

/// The claim that Ollama needs no key is the one a privacy-motivated
/// visitor acts on, so it is pinned to the code that makes it true.
///
/// `requires_api_key()` returning false is the whole of it; if that
/// ever flips, the FAQ sentence becomes a lie about what the app will
/// ask them for.
#[test]
fn the_keyless_claim_matches_the_provider() {
    // Gated like the test above, and for the same reason: with Ollama
    // removed from the registry `provider_from_id` falls back to
    // Anthropic, so this would demand a claim the site is by then free
    // to drop — failing over copy that had become correct. Caught in
    // review on #316.
    if !SUPPORTED_PROVIDER_IDS.contains(&"ollama") {
        return;
    }
    let ollama = ai::provider::provider_from_id("ollama");
    assert_eq!(ollama.id(), "ollama", "provider_from_id fell back");
    assert!(
        !ollama.requires_api_key(),
        "Ollama now requires an API key, but the FAQ tells visitors it does not"
    );
    assert!(
        faq().contains("no API key"),
        "the FAQ no longer says Ollama is keyless — that is allowed, but this test has to be \
         told, or it silently stops guarding the claim"
    );
}

/// The local-model page says Ollama is sent a smaller set of tools, and
/// that is only true while the provider asks for one.
///
/// Pinned both ways, like the keyless claim above: if `tool_set()` is
/// ever changed back to the full list, the page must stop saying so, and
/// if the page's sentence is removed this test has to be told rather than
/// silently guarding nothing.
#[test]
fn the_local_model_page_describes_the_slim_tool_set_only_while_ollama_has_one() {
    if !SUPPORTED_PROVIDER_IDS.contains(&"ollama") {
        return; // Removed as a provider; the page is free to say so.
    }
    let ollama = ai::provider::provider_from_id("ollama");
    assert_eq!(ollama.id(), "ollama", "provider_from_id fell back");
    let page = read_website("app/use-cases/local-ai-audio-editor/page.tsx");
    // The page wraps its prose across lines, so compare on words.
    let words = page.split_whitespace().collect::<Vec<_>>().join(" ");
    let claims_it = words.contains("smaller set of tools");
    let has_it = ollama.tool_set() == ai::ToolSet::Slim;
    assert_eq!(
        claims_it,
        has_it,
        "the local-model page {} a smaller set of tools for Ollama, but OllamaProvider::tool_set() \
         is {:?}",
        if claims_it {
            "claims"
        } else {
            "does not mention"
        },
        ollama.tool_set()
    );
}
