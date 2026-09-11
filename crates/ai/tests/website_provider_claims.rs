//! The website's provider prose must match the provider registry (#261).
//!
//! The landing FAQ told visitors that Ollama was *"planned for v1 phase
//! 3"* and that local models got *"a simplified tool surface"*. Both
//! were untrue: Ollama shipped in #126, and no provider-conditioned
//! tool narrowing has ever existed — the only whitelist is keyed on the
//! active agent profile. Meanwhile four other places on the same site
//! advertised Ollama as working, including the changelog entry
//! announcing it.
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

    // The specific stale sentences from #261, and the general shapes
    // they belong to. "phase 3" is the one that was actually there;
    // the rest are the ways the same claim tends to get rewritten.
    for stale in [
        "planned for v1 phase 3",
        "simplified tool surface",
        "Ollama (Qwen, Llama 3, etc.) are planned",
    ] {
        assert!(
            !src.contains(stale),
            "the FAQ still says {stale:?}. Ollama is a registered provider and there is no \
             provider-conditioned tool filtering anywhere in the codebase — the only whitelist \
             is keyed on the agent profile."
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
