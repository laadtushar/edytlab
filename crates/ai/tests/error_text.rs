//! What a person reads when the model provider fails (#405).
//!
//! The same two errors are returned for every provider, and both used to
//! begin "anthropic", so a person on Ollama or Groq was told that Anthropic
//! had failed.

use ai::Error;

fn words(e: Error) -> String {
    e.to_string()
}

#[test]
fn an_api_error_names_no_provider_but_keeps_the_status_and_the_servers_message() {
    let text = words(Error::Api {
        status: 500,
        message: "the model fell over".into(),
    });
    assert!(!text.to_lowercase().contains("anthropic"), "{text}");
    assert!(text.contains("500"), "{text}");
    assert!(text.contains("the model fell over"), "{text}");
}

#[test]
fn a_stream_error_names_no_provider_but_keeps_the_servers_message() {
    let text = words(Error::ApiStream("overloaded".into()));
    assert!(!text.to_lowercase().contains("anthropic"), "{text}");
    assert!(text.contains("overloaded"), "{text}");
}

/// A request that did not fit the model's context says what to change, not
/// which provider failed (#395). It also must not read as a key problem:
/// the chat offers Settings for those, and no key fixes this.
#[test]
fn a_context_error_names_no_provider_and_says_what_to_change() {
    let text = words(Error::ContextTooSmall {
        needed: Some(15157),
        available: Some(8192),
        message: "the server's own body".into(),
    });
    let lower = text.to_lowercase();
    assert!(lower.contains("context"), "{text}");
    assert!(!lower.contains("anthropic"), "{text}");
    assert!(!lower.contains("api key"), "{text}");
    assert!(text.contains("15157") && text.contains("8192"), "{text}");
    assert!(
        !text.contains("the server's own body"),
        "the server's raw body belongs in the log, not the chat: {text}"
    );
}

/// A model the provider no longer has is named, with the default to switch
/// to, in words that name no provider (#405) and show none of its JSON. The
/// chat's banner reads the two ids out of the backticks (see `chat-errors.ts`),
/// so the shape is part of the contract.
#[test]
fn a_model_unavailable_error_names_both_models_and_no_provider() {
    let text = words(Error::ModelUnavailable {
        model: "claude-opus-4-1-20250805".into(),
        default_model: "claude-sonnet-5-5".into(),
        status: 404,
    });
    let lower = text.to_lowercase();
    assert!(lower.contains("model unavailable"), "{text}");
    assert!(text.contains("`claude-opus-4-1-20250805`"), "{text}");
    assert!(text.contains("`claude-sonnet-5-5`"), "{text}");
    assert!(text.contains("Settings"), "{text}");
    assert!(text.contains("404"), "{text}");
    assert!(!lower.contains("anthropic"), "{text}");
    assert!(!lower.contains("not_found_error"), "{text}");
    assert!(!text.contains('{'), "{text}");
    // Not a key problem: the chat offers Settings for those by the words
    // "api key", and no key fixes this.
    assert!(!lower.contains("api key"), "{text}");
}

/// ChatGPT plan failures are told in their own words (#474): not as a refused
/// key, which the chat answers by offering Settings to paste one in, and not as
/// a provider's JSON.
#[test]
fn a_plan_usage_error_names_no_provider_and_is_not_a_key_problem() {
    use ai::chatgpt::{PlanUsageError, PlanUsageKind};
    for kind in [
        PlanUsageKind::UsageLimit,
        PlanUsageKind::NotEligible,
        PlanUsageKind::PlanDisabled,
        PlanUsageKind::SessionEnded { detail: None },
        PlanUsageKind::Unavailable {
            detail: "HTTP 503".into(),
        },
    ] {
        let error = PlanUsageError::new(kind).with_request_id(Some("req_1"));
        let text = words(Error::PlanUsage(error));
        let lower = text.to_lowercase();
        assert!(!lower.contains("anthropic"), "{text}");
        assert!(!lower.contains("api key"), "{text}");
        assert!(!lower.contains("authenticat"), "{text}");
        assert!(!text.contains('{'), "{text}");
        assert!(text.contains("ChatGPT"), "{text}");
        assert!(text.contains("req_1"), "{text}");
    }
}
