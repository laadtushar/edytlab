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
