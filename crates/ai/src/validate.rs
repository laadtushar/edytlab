//! Lightweight API-key validation against an Anthropic-shaped Messages
//! API.
//!
//! M13's Settings UI exposes a "Test" button that needs to tell the user
//! whether the key they just typed actually works, *before* they save it
//! and try a real chat turn. We intentionally do this server-side (in
//! Rust) rather than in the renderer so the proposed key never has to
//! leave the trusted process.
//!
//! The shape of the request is kept as small as possible to minimise
//! cost when the user mashes the button: the provider's classifier
//! model, a single one-character user message, and `max_tokens = 1`. We
//! rely on the API returning a 401/403 for an invalid key *before*
//! spending tokens; if the network is unreachable the call surfaces an
//! HTTP error that the Settings panel can display verbatim.
//!
//! The function only inspects the HTTP status. A 200 means the key is
//! authenticated and the account has Messages-API access. Any non-2xx
//! response yields `Err(VALIDATION_ERROR)` whose `Display` is the
//! `"<status> <body>"` string that M13 acceptance criterion #2 mandates
//! ("`401 invalid x-api-key`").
//!
//! # Multi-provider
//!
//! The validator takes a [`LlmProvider`] (rather than hardcoding
//! Anthropic headers) so the same code path works for OpenRouter — the
//! request body is identical (Anthropic-shape) and only the auth
//! headers differ, which is exactly what
//! [`LlmProvider::apply_auth`] encapsulates.

use std::sync::Arc;

use serde_json::json;

use crate::provider::{
    AnthropicProvider, GeminiProvider, GroqProvider, LlmProvider, OpenAIProvider,
    OpenRouterProvider, GEMINI_ID, GROQ_ID, OPENAI_ID,
};

/// Providers that use an OpenAI-compatible `/models` probe rather than a
/// 1-token Anthropic Messages call. Validation hits GET `{base}/{models_path}`
/// which is auth-gated but costs zero tokens.
const MODELS_PROBE_IDS: &[&str] = &[OPENAI_ID, GROQ_ID, GEMINI_ID];

/// Validate an API key by issuing a one-token Messages call against
/// `provider`'s endpoint. `base_url` is parameterised for tests;
/// production callers pass the provider's default base URL.
///
/// Returns `Ok(())` on HTTP 200 and `Err(message)` otherwise, where
/// `message` is `"<status> <body-text>"` so the caller can surface a
/// developer-readable reason. Network errors are converted to `Err` with
/// the error's `Display` text — they are still surfaceable to the user
/// even though there is no HTTP status to report.
pub async fn test_api_key_with(
    provider: &dyn LlmProvider,
    api_key: &str,
    base_url: &str,
) -> Result<(), String> {
    if api_key.trim().is_empty() {
        return Err("api key must not be empty".to_string());
    }

    let client = reqwest::Client::new();

    // OpenAI-compatible providers (OpenAI, Groq, Gemini) don't expose
    // `/v1/messages`; probe their models catalogue endpoint instead. It's
    // auth-gated, costs zero tokens, and returns 200/401 clearly.
    let resp = if MODELS_PROBE_IDS.contains(&provider.id()) {
        let req = client.get(format!("{base_url}{}", provider.list_models_path()));
        let req = provider.apply_auth(req, api_key);
        req.send().await.map_err(|e| e.to_string())?
    } else {
        let body = json!({
            "model": provider.translate_model(provider.classifier_model()),
            "max_tokens": 1,
            "messages": [{
                "role": "user",
                "content": "ping",
            }],
        });
        let req = client.post(format!("{base_url}/v1/messages"));
        let req = provider.apply_auth(req, api_key);
        req.json(&body).send().await.map_err(|e| e.to_string())?
    };

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

/// Convenience wrapper for production callers that always hit the
/// provider's default base URL. Resolves the provider from its stable
/// id and dispatches to [`test_api_key_with`].
pub async fn test_api_key_for(provider_id: &str, api_key: &str) -> Result<(), String> {
    let provider = crate::provider::provider_from_id(provider_id);
    let base_url = provider.base_url().to_string();
    test_api_key_with(provider.as_ref(), api_key, &base_url).await
}

/// Back-compat shim: validate against Anthropic. Kept so the historical
/// callers (`commands::test_api_key`) compile without changes; the
/// updated commands surface uses [`test_api_key_for`] explicitly.
pub async fn test_api_key(api_key: &str) -> Result<(), String> {
    let provider = AnthropicProvider;
    let base_url = provider.base_url().to_string();
    test_api_key_with(&provider, api_key, &base_url).await
}

/// Test-only helper: validate against a wiremock URI. Forwards to
/// [`test_api_key_with`] with an [`AnthropicProvider`] so existing
/// tests keep their current behaviour.
pub async fn test_api_key_against(api_key: &str, base_url: &str) -> Result<(), String> {
    test_api_key_with(&AnthropicProvider, api_key, base_url).await
}

/// Test-only: explicit OpenRouter validation against a custom base URL.
/// The frontend reaches this through [`test_api_key_for`]; the test
/// suite uses this to assert the OpenRouter auth headers without going
/// through provider id resolution.
pub async fn test_openrouter_key_against(api_key: &str, base_url: &str) -> Result<(), String> {
    test_api_key_with(&OpenRouterProvider, api_key, base_url).await
}

/// Test-only: explicit OpenAI validation against a custom base URL.
/// The frontend reaches this through [`test_api_key_for`]; the test
/// suite uses this to assert the OpenAI Bearer header path.
pub async fn test_openai_key_against(api_key: &str, base_url: &str) -> Result<(), String> {
    test_api_key_with(&OpenAIProvider::default(), api_key, base_url).await
}

/// Test-only: explicit Groq validation against a custom base URL.
pub async fn test_groq_key_against(api_key: &str, base_url: &str) -> Result<(), String> {
    test_api_key_with(&GroqProvider::default(), api_key, base_url).await
}

/// Test-only: explicit Gemini validation against a custom base URL.
pub async fn test_gemini_key_against(api_key: &str, base_url: &str) -> Result<(), String> {
    test_api_key_with(&GeminiProvider::default(), api_key, base_url).await
}

/// Resolve a provider trait object from a stable id. Re-exported here
/// so the desktop `commands` layer can avoid pulling `provider::*`
/// directly.
pub fn provider_for(id: &str) -> Arc<dyn LlmProvider> {
    crate::provider::provider_from_id(id)
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

    /// OpenRouter validation must use `Authorization: Bearer <key>` (and
    /// not the Anthropic-style `x-api-key`). The wiremock matcher
    /// asserts the header is present and correctly formed.
    #[tokio::test]
    async fn openrouter_validation_uses_bearer_authorization_header() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .and(header("authorization", "Bearer or-test-key"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "msg_1",
                "type": "message",
                "role": "assistant",
                "model": "anthropic/claude-haiku-4-5-20251001",
                "content": [{"type": "text", "text": "p"}],
                "stop_reason": "end_turn"
            })))
            .mount(&server)
            .await;

        test_openrouter_key_against("or-test-key", &server.uri())
            .await
            .expect("should be ok");
    }

    /// OpenAI validation must hit `/v1/models` with a Bearer header,
    /// and never send the Anthropic-style `x-api-key`. The wiremock
    /// matcher pins both the path AND the auth header.
    #[tokio::test]
    async fn openai_validation_hits_models_endpoint_with_bearer() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .and(header("authorization", "Bearer sk-test"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "object": "list",
                "data": [{"id": "gpt-4o-mini", "object": "model"}]
            })))
            .mount(&server)
            .await;

        test_openai_key_against("sk-test", &server.uri())
            .await
            .expect("should be ok");

        let received = server.received_requests().await.unwrap();
        assert_eq!(received.len(), 1);
        assert!(
            received[0].headers.get("x-api-key").is_none(),
            "OpenAI request unexpectedly carried x-api-key"
        );
        // No anthropic-version header should appear either.
        assert!(
            received[0].headers.get("anthropic-version").is_none(),
            "OpenAI request unexpectedly carried anthropic-version"
        );
    }

    /// Confirm OpenRouter validation does *not* send Anthropic's
    /// `x-api-key` header — the request would still succeed against the
    /// real OpenRouter API, but cross-leaking the wrong header is a sign
    /// the abstraction has regressed.
    #[tokio::test]
    async fn openrouter_validation_does_not_send_x_api_key() {
        let server = MockServer::start().await;
        // Match only requests that DO carry the Bearer header AND lack
        // any x-api-key header. We can't easily assert "header absent"
        // with wiremock 0.6's matchers, so we rely on the Bearer match
        // being the only mock and check `received_requests` after.
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .and(header("authorization", "Bearer or-key-2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "msg_1",
                "type": "message",
                "role": "assistant",
                "model": "anthropic/claude-haiku-4-5-20251001",
                "content": [{"type": "text", "text": "p"}],
                "stop_reason": "end_turn"
            })))
            .mount(&server)
            .await;

        test_openrouter_key_against("or-key-2", &server.uri())
            .await
            .expect("should be ok");

        let received = server.received_requests().await.unwrap();
        assert_eq!(received.len(), 1);
        // No x-api-key header should appear on an OpenRouter request.
        assert!(
            received[0].headers.get("x-api-key").is_none(),
            "OpenRouter request unexpectedly carried x-api-key"
        );
        // HTTP-Referer + X-Title attribution headers should be present.
        assert_eq!(
            received[0]
                .headers
                .get("HTTP-Referer")
                .and_then(|v| v.to_str().ok()),
            Some("https://edytlab.app")
        );
        assert_eq!(
            received[0]
                .headers
                .get("X-Title")
                .and_then(|v| v.to_str().ok()),
            Some("edytlab")
        );
    }
}
