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

use serde::Serialize;
use serde_json::{json, Value};

use crate::anthropic::{ContentBlock, Message, MessagesRequest, Role, ToolChoice};
use crate::provider::{
    AnthropicProvider, GeminiProvider, GroqProvider, LlmProvider, OpenAIProvider,
    OpenRouterProvider, GEMINI_ID, GROQ_ID, OLLAMA_ID, OPENAI_ID,
};

/// Providers that use an OpenAI-compatible `/models` probe rather than a
/// 1-token Anthropic Messages call. Validation hits GET `{base}/{models_path}`
/// which is auth-gated but costs zero tokens.
const MODELS_PROBE_IDS: &[&str] = &[OPENAI_ID, GROQ_ID, GEMINI_ID, OLLAMA_ID];

/// Name of the throwaway tool the capability probe offers the model. It
/// is deliberately namespaced so it cannot collide with a real edytlab
/// tool if it ever shows up in a log.
const PROBE_TOOL: &str = "edytlab_probe";

/// The outcome of a TEST press.
///
/// Reachability and tool support are different questions and TEST used
/// to answer only the first. Every edit in edytlab is a tool call, and
/// tool support is a property of the *model*, not the server: a local
/// model without it connects fine, tests green, fills the model
/// dropdown, and then fails on the first real request. That is a
/// confusing way to learn something the app could say up front.
///
/// `Err` from the probe still means "unreachable or rejected" and
/// carries the same `"<status> <body>"` text as before. This struct is
/// the success side, split in two.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeReport {
    /// The model id the capability probe ran against — the one the user
    /// picked, or the provider default when they have not picked one.
    /// Named in the UI, because it is the model and not the server that
    /// is at fault.
    pub model: String,
    /// Whether the model actually emitted a tool call when offered one.
    pub tools_ok: bool,
    /// Why the tool probe failed, when it did: the model's text reply,
    /// or the endpoint's `"<status> <body>"`. `None` when tools work.
    pub detail: Option<String>,
}

impl ProbeReport {
    fn ready(model: String) -> Self {
        Self {
            model,
            tools_ok: true,
            detail: None,
        }
    }
}

/// The one-tool schema the probe offers. Written in Anthropic shape;
/// `serialize_request` translates it to `functions` for the
/// OpenAI-compatible providers, so the probe exercises the exact
/// serialisation path a real turn uses.
fn probe_tools() -> Value {
    json!([{
        "name": PROBE_TOOL,
        "description": "Connectivity check. Call this tool once with ok = true.",
        "input_schema": {
            "type": "object",
            "properties": {
                "ok": { "type": "boolean", "description": "Always true." }
            },
            "required": ["ok"],
        },
    }])
}

/// Did this non-streaming response contain a tool call?
///
/// Two shapes, because we talk to two families of API: Anthropic puts a
/// `tool_use` block in `content[]`; OpenAI-compatible servers put a
/// `tool_calls` array on `choices[0].message`.
fn response_called_a_tool(body: &Value) -> bool {
    let anthropic = body
        .get("content")
        .and_then(Value::as_array)
        .is_some_and(|blocks| {
            blocks
                .iter()
                .any(|b| b.get("type").and_then(Value::as_str) == Some("tool_use"))
        });

    let openai = body
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|c| c.first())
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("tool_calls"))
        .and_then(Value::as_array)
        .is_some_and(|calls| !calls.is_empty());

    anthropic || openai
}

/// The text the model replied with instead of calling the tool, if any.
/// Used to make the warning concrete rather than abstract.
fn response_text(body: &Value) -> Option<String> {
    let anthropic = body
        .get("content")
        .and_then(Value::as_array)
        .and_then(|blocks| {
            blocks
                .iter()
                .find(|b| b.get("type").and_then(Value::as_str) == Some("text"))
                .and_then(|b| b.get("text"))
                .and_then(Value::as_str)
        });

    let openai = body
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|c| c.first())
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(Value::as_str);

    let text = anthropic.or(openai)?.trim();
    if text.is_empty() {
        return None;
    }
    Some(truncate(text, 200))
}

/// Keep a surfaced server/model string short enough to read in a
/// settings panel. Cuts on a char boundary — some of these bodies are
/// not ASCII.
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let cut: String = s.chars().take(max).collect();
    format!("{cut}…")
}

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
    probe_provider(provider, api_key, base_url, None)
        .await
        .map(|_| ())
}

/// Reachability + credentials + tool support, in that order.
///
/// The first two are the historical TEST: `Err` still carries
/// `"<status> <body>"` and still means "do not save this". The third is
/// new, costs exactly one extra request, and only runs once the first
/// two passed — so an unreachable endpoint reports the same error it
/// always did rather than a confusing tool-support message.
///
/// `model` is the id the user selected; `None` falls back to the
/// provider's default. It matters which one we probe: tool support
/// varies per model on the same server.
pub async fn probe_provider(
    provider: &dyn LlmProvider,
    api_key: &str,
    base_url: &str,
    model: Option<&str>,
) -> Result<ProbeReport, String> {
    reachability_probe(provider, api_key, base_url).await?;

    let model = model
        .map(str::trim)
        .filter(|m| !m.is_empty())
        .unwrap_or_else(|| provider.default_model())
        .to_string();

    Ok(tool_probe(provider, api_key, base_url, &model).await)
}

/// Offer the model one trivial tool and see whether it calls it.
///
/// Never returns `Err`: a failure here is not "your endpoint is wrong",
/// it is "this model will not be able to edit", which is a warning the
/// user can act on while still saving a working key.
async fn tool_probe(
    provider: &dyn LlmProvider,
    api_key: &str,
    base_url: &str,
    model: &str,
) -> ProbeReport {
    let wire_model = provider.translate_model(model);
    let messages = vec![Message {
        role: Role::User,
        content: vec![ContentBlock::Text {
            text: format!("Call the {PROBE_TOOL} tool with ok set to true. Do not reply in text."),
        }],
    }];
    let req = MessagesRequest {
        model: &wire_model,
        // Enough headroom for a tool call's arguments. A model that
        // truncates mid-call would otherwise look like one that cannot
        // call tools at all.
        max_tokens: 256,
        system: Vec::new(),
        messages: &messages,
        tools: Some(probe_tools()),
        tool_choice: Some(ToolChoice::AUTO),
        // Non-streaming: we want the whole reply in one JSON body, and
        // the probe has no UI to stream into.
        stream: false,
    };
    let body = provider.serialize_request(&req);

    let client = reqwest::Client::new();
    let request = provider.apply_auth(
        client.post(format!("{base_url}{}", provider.endpoint_path())),
        api_key,
    );
    let resp = match request.json(&body).send().await {
        Ok(r) => r,
        Err(e) => {
            return ProbeReport {
                model: model.to_string(),
                tools_ok: false,
                detail: Some(e.to_string()),
            }
        }
    };

    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();

    // A non-2xx here is nearly always the server saying it does not
    // accept a `tools` array — llama.cpp and vLLM both answer 400 for
    // that. Reachability already passed, so this is a tool problem.
    if !status.is_success() {
        let trimmed = text.trim();
        let detail = if trimmed.is_empty() {
            status.canonical_reason().unwrap_or("error").to_string()
        } else {
            truncate(trimmed, 200)
        };
        return ProbeReport {
            model: model.to_string(),
            tools_ok: false,
            detail: Some(format!("{} {}", status.as_u16(), detail)),
        };
    }

    let parsed: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    if response_called_a_tool(&parsed) {
        return ProbeReport::ready(model.to_string());
    }

    ProbeReport {
        model: model.to_string(),
        tools_ok: false,
        detail: response_text(&parsed),
    }
}

/// The original TEST: can we reach this endpoint with these
/// credentials? Unchanged in behaviour, extracted so the tool probe can
/// run after it.
async fn reachability_probe(
    provider: &dyn LlmProvider,
    api_key: &str,
    base_url: &str,
) -> Result<(), String> {
    // A keyless provider is validated by reaching it at all — there is
    // no credential to be wrong. Rejecting the empty string here would
    // make a local daemon unconfigurable.
    if provider.requires_api_key() && api_key.trim().is_empty() {
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
    probe_provider_for(provider_id, api_key, None, None)
        .await
        .map(|_| ())
}

/// Production entry point for the Settings TEST button.
///
/// `base_url` and `model` are the values on screen, not the saved ones:
/// testing an endpoint you have typed but not yet saved is the whole
/// point of the button, and probing the default endpoint instead would
/// report on a server the user is not about to use.
pub async fn probe_provider_for(
    provider_id: &str,
    api_key: &str,
    base_url: Option<&str>,
    model: Option<&str>,
) -> Result<ProbeReport, String> {
    let provider = crate::provider::provider_from_id(provider_id);
    let base_url = base_url
        .map(str::trim)
        .filter(|u| !u.is_empty())
        .map(|u| u.trim_end_matches('/').to_string())
        .unwrap_or_else(|| provider.base_url().to_string());
    probe_provider(provider.as_ref(), api_key, &base_url, model).await
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

    /// The empty-key rejection must not apply to a provider that has no
    /// key. It used to be unconditional, which made a local daemon
    /// impossible to validate and therefore impossible to configure.
    #[tokio::test]
    async fn a_keyless_provider_validates_with_an_empty_key() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/models"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "object": "list", "data": [] })),
            )
            .mount(&server)
            .await;

        let provider = crate::provider::provider_from_id(OLLAMA_ID);
        test_api_key_with(provider.as_ref(), "", &server.uri())
            .await
            .expect("a keyless provider must validate on reachability alone");
    }

    /// And the rejection must still apply to everyone else.
    #[tokio::test]
    async fn a_hosted_provider_still_rejects_an_empty_key() {
        let provider = crate::provider::provider_from_id(OPENAI_ID);
        let err = test_api_key_with(provider.as_ref(), "  ", "http://127.0.0.1:1")
            .await
            .unwrap_err();
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
        // The models GET is the reachability probe; the tool probe that
        // follows is a POST the mock server does not answer, and its
        // failure is a warning rather than an error.
        assert_eq!(received[0].url.path(), "/v1/models");
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

    // -----------------------------------------------------------------
    // Tool-capability probe
    //
    // The point of these: reachability and tool support are different
    // questions, and every edit in edytlab is a tool call. A model that
    // answers but ignores the `tools` array connects, tests green under
    // the old probe, and then fails on the first real edit.
    // -----------------------------------------------------------------

    /// An OpenAI-compatible server whose model returns a `tool_calls`
    /// array is the ready state.
    #[tokio::test]
    async fn a_model_that_calls_the_tool_reports_ready() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": []})))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{
                    "message": {
                        "role": "assistant",
                        "content": null,
                        "tool_calls": [{
                            "id": "call_1",
                            "type": "function",
                            "function": {"name": PROBE_TOOL, "arguments": "{\"ok\":true}"}
                        }]
                    },
                    "finish_reason": "tool_calls"
                }]
            })))
            .mount(&server)
            .await;

        let provider = OpenAIProvider::default();
        let report = probe_provider(&provider, "sk-test", &server.uri(), Some("qwen2.5-7b"))
            .await
            .expect("reachable");
        assert!(report.tools_ok, "expected ready, got {report:?}");
        assert_eq!(report.model, "qwen2.5-7b");
        assert_eq!(report.detail, None);
    }

    /// The middle state: the server answers, the model replies in prose,
    /// and no tool is called. This is the LM Studio failure — it must
    /// not read as success.
    #[tokio::test]
    async fn a_model_that_answers_in_text_is_not_ready() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": []})))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{
                    "message": {"role": "assistant", "content": "Sure! ok = true."},
                    "finish_reason": "stop"
                }]
            })))
            .mount(&server)
            .await;

        let provider = OpenAIProvider::default();
        let report = probe_provider(&provider, "sk-test", &server.uri(), Some("no-tools-7b"))
            .await
            .expect("the endpoint itself is fine");
        assert!(!report.tools_ok, "text-only reply must not report ready");
        assert_eq!(report.model, "no-tools-7b");
        assert_eq!(report.detail.as_deref(), Some("Sure! ok = true."));
    }

    /// A server that rejects the `tools` array outright — llama.cpp and
    /// vLLM both answer 400 — is the same middle state, not a
    /// connection failure, because reachability already passed.
    #[tokio::test]
    async fn a_server_that_rejects_tools_is_a_warning_not_an_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": []})))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(400).set_body_string("tools are not supported"))
            .mount(&server)
            .await;

        let provider = OpenAIProvider::default();
        let report = probe_provider(&provider, "sk-test", &server.uri(), Some("plain-7b"))
            .await
            .expect("reachability passed, so this is not an Err");
        assert!(!report.tools_ok);
        let detail = report.detail.expect("a reason");
        assert!(detail.contains("400"), "missing status: {detail}");
        assert!(detail.contains("tools are not supported"), "got {detail}");
    }

    /// Anthropic's shape is a `tool_use` block in `content[]`, not a
    /// `tool_calls` array. Both count.
    #[tokio::test]
    async fn anthropic_tool_use_blocks_count_as_ready() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "msg_1",
                "type": "message",
                "role": "assistant",
                "model": "claude-sonnet-4-6",
                "content": [
                    {"type": "tool_use", "id": "tu_1", "name": PROBE_TOOL, "input": {"ok": true}}
                ],
                "stop_reason": "tool_use"
            })))
            .mount(&server)
            .await;

        let report = probe_provider(&AnthropicProvider, "k", &server.uri(), None)
            .await
            .expect("reachable");
        assert!(report.tools_ok);
        // No model was passed, so the provider default is what ran — and
        // the report has to name it, since that is what the warning
        // would blame.
        assert_eq!(report.model, AnthropicProvider.default_model());
    }

    /// An unreachable endpoint reports the existing error and never
    /// reaches the tool probe — a tool-support warning would be a
    /// misleading way to say "wrong URL".
    #[tokio::test]
    async fn an_unreachable_endpoint_still_errors() {
        let err = probe_provider(&AnthropicProvider, "k", "http://127.0.0.1:1", None)
            .await
            .expect_err("connection refused");
        assert!(!err.contains("tool"), "should not mention tools: {err}");
    }

    /// A rejected key errors before the tool probe spends a request.
    #[tokio::test]
    async fn a_rejected_key_errors_before_the_tool_probe() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(401).set_body_string("bad key"))
            .mount(&server)
            .await;

        let provider = OpenAIProvider::default();
        let err = probe_provider(&provider, "sk-bad", &server.uri(), None)
            .await
            .expect_err("401");
        assert!(err.contains("401"), "got {err}");
        let received = server.received_requests().await.unwrap();
        assert_eq!(received.len(), 1, "the tool probe must not have run");
    }

    /// The probe request must carry the tool schema in the shape the
    /// provider actually uses — this is the whole mechanism, and a
    /// silently-dropped `tools` array would make every model look
    /// incapable.
    #[tokio::test]
    async fn the_probe_request_carries_one_tool() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": []})))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"choices": []})))
            .mount(&server)
            .await;

        let provider = OpenAIProvider::default();
        let _ = probe_provider(&provider, "sk-test", &server.uri(), Some("m")).await;

        let received = server.received_requests().await.unwrap();
        let post = received
            .iter()
            .find(|r| r.url.path() == "/v1/chat/completions")
            .expect("the tool probe ran");
        let body: Value = serde_json::from_slice(&post.body).unwrap();
        let tools = body["tools"].as_array().expect("a tools array");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["function"]["name"], PROBE_TOOL);
        assert_eq!(body["model"], "m");
        assert_eq!(body["stream"], false);
    }

    /// A typed-but-unsaved base URL is what TEST must probe. It used to
    /// probe the provider's default, which reported on a server the user
    /// was not about to use.
    #[tokio::test]
    async fn the_probe_honours_a_custom_base_url() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": []})))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{"message": {"role": "assistant", "tool_calls": [
                    {"id": "c", "type": "function",
                     "function": {"name": PROBE_TOOL, "arguments": "{}"}}
                ]}}]
            })))
            .mount(&server)
            .await;

        // Trailing slash included deliberately: users paste it.
        let base = format!("{}/", server.uri());
        let report = probe_provider_for(OPENAI_ID, "sk-test", Some(&base), Some("local-model"))
            .await
            .expect("the local server answered");
        assert!(report.tools_ok);
        assert_eq!(report.model, "local-model");
        assert!(
            !server.received_requests().await.unwrap().is_empty(),
            "nothing reached the custom endpoint"
        );
    }
}
