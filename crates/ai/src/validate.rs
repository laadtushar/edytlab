//! Lightweight API-key validation against the Anthropic Messages API.
//!
//! M13's Settings UI exposes a "Test" button that needs to tell the user
//! whether the key they just typed actually works, *before* they save it
//! and try a real chat turn. We intentionally do this server-side (in
//! Rust) rather than in the renderer so the proposed key never has to
//! leave the trusted process.
//!
//! The shape of the request is kept as small as possible to minimise
//! cost when the user mashes the button: `claude-haiku-4-5`, a single
//! one-character user message, and `max_tokens = 1`. We rely on the API
//! returning a 401/403 for an invalid key *before* spending tokens; if
//! the network is unreachable the call surfaces an HTTP error that the
//! Settings panel can display verbatim.
//!
//! The function only inspects the HTTP status. A 200 means the key is
//! authenticated and the account has Messages-API access. Any non-2xx
//! response yields `Err(VALIDATION_ERROR)` whose `Display` is the
//! `"<status> <body>"` string that M13 acceptance criterion #2 mandates
//! ("`401 invalid x-api-key`").

use serde_json::json;

use crate::prompt::{ANTHROPIC_VERSION, DEFAULT_BASE_URL};

/// Cheap probe model. Haiku is the smallest currently-available model on
/// the Anthropic API; using it minimises the latency of the round-trip.
const PROBE_MODEL: &str = "claude-haiku-4-5";

/// Validate an Anthropic API key by issuing a one-token Messages call.
///
/// Returns `Ok(())` on HTTP 200 and `Err(message)` otherwise, where
/// `message` is `"<status> <body-text>"` so the caller can surface a
/// developer-readable reason. Network errors are converted to `Err` with
/// the error's `Display` text — they are still surfacable to the user
/// even though there is no HTTP status to report.
///
/// `base_url` is parameterised for tests; production callers pass
/// [`DEFAULT_BASE_URL`].
pub async fn test_api_key_against(api_key: &str, base_url: &str) -> Result<(), String> {
    if api_key.trim().is_empty() {
        return Err("api key must not be empty".to_string());
    }

    let body = json!({
        "model": PROBE_MODEL,
        "max_tokens": 1,
        "messages": [{
            "role": "user",
            "content": "ping",
        }],
    });

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{base_url}/v1/messages"))
        .header("x-api-key", api_key)
        .header("anthropic-version", ANTHROPIC_VERSION)
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    let status = resp.status();
    if status.is_success() {
        return Ok(());
    }

    // Try to read the body for context. If we can't, fall back to the
    // canonical reason phrase (e.g. "Unauthorized") so the user still
    // sees something more useful than "non-2xx".
    let text = resp.text().await.unwrap_or_default();
    let trimmed = text.trim();
    let detail = if trimmed.is_empty() {
        status.canonical_reason().unwrap_or("error").to_string()
    } else {
        trimmed.to_string()
    };
    Err(format!("{} {}", status.as_u16(), detail))
}

/// Convenience wrapper for production callers that always hit
/// [`DEFAULT_BASE_URL`].
pub async fn test_api_key(api_key: &str) -> Result<(), String> {
    test_api_key_against(api_key, DEFAULT_BASE_URL).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn empty_key_is_rejected_without_a_network_call() {
        // No mock server registered: if we accidentally make a request,
        // it would fail with a connection error, not the empty-key error.
        let err = test_api_key_against("", "http://127.0.0.1:1")
            .await
            .expect_err("empty key");
        assert!(err.contains("must not be empty"), "got {err}");
    }

    #[tokio::test]
    async fn ok_status_is_success() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .and(header("x-api-key", "good-key"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "msg_1",
                "type": "message",
                "role": "assistant",
                "model": "claude-haiku-4-5",
                "content": [{"type": "text", "text": "p"}],
                "stop_reason": "end_turn"
            })))
            .mount(&server)
            .await;

        test_api_key_against("good-key", &server.uri())
            .await
            .expect("should be ok");
    }

    #[tokio::test]
    async fn unauthorized_response_is_surfaced_with_status_and_body() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(ResponseTemplate::new(401).set_body_string("invalid x-api-key"))
            .mount(&server)
            .await;

        let err = test_api_key_against("bad-key", &server.uri())
            .await
            .expect_err("should be err");
        assert!(err.contains("401"), "missing status: {err}");
        assert!(err.contains("invalid x-api-key"), "missing body: {err}");
    }
}
