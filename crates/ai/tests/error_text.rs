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
