//! Provider abstraction for the LLM transport layer.
//!
//! This crate originally hardcoded Anthropic's Messages API. To support
//! OpenRouter and OpenAI we extract the per-provider differences behind
//! the [`LlmProvider`] trait.
//!
//! # What the trait owns
//!
//! 1. **Auth + headers** (`apply_auth`).
//! 2. **Model id translation** (`translate_model`).
//! 3. **Endpoint path** (`endpoint_path`) — Anthropic + OpenRouter both
//!    serve `/v1/messages`; OpenAI serves `/v1/chat/completions`.
//! 4. **Request serialisation** (`serialize_request`) — Anthropic +
//!    OpenRouter use the canonical [`MessagesRequest`] shape; OpenAI
//!    translates each request into chat-completions JSON.
//! 5. **Stream parsing** (`parse_stream_chunk`) — Anthropic + OpenRouter
//!    deserialise each SSE `data:` payload directly into the canonical
//!    [`StreamEvent`] enum; OpenAI translates `delta`/`tool_calls` chunks
//!    into the same enum so the agent loop's tool-calling state machine
//!    is provider-agnostic.
//!
//! Anything else stays in the agent loop.

use std::fmt::Debug;
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

use crate::anthropic::{
    ApiError, ContentBlock, ContentBlockDelta, ContentBlockStart, MessageMeta, MessagesRequest,
    Role, StreamEvent,
};

/// Stable id for the Anthropic provider.
pub const ANTHROPIC_ID: &str = "anthropic";
/// Stable id for the OpenRouter provider.
pub const OPENROUTER_ID: &str = "openrouter";
/// Stable id for the OpenAI provider.
pub const OPENAI_ID: &str = "openai";
/// Stable id for the Groq provider.
pub const GROQ_ID: &str = "groq";
/// Stable id for the Gemini provider.
pub const GEMINI_ID: &str = "gemini";
/// Stable id for the Ollama provider (local models, no API key).
pub const OLLAMA_ID: &str = "ollama";

/// Default base URL for Anthropic's Messages API.
pub const ANTHROPIC_DEFAULT_BASE_URL: &str = "https://api.anthropic.com";
/// Default base URL for OpenRouter (Anthropic-compatible Messages API
/// mounted under `/api/v1`).
pub const OPENROUTER_DEFAULT_BASE_URL: &str = "https://openrouter.ai/api";
/// Default base URL for OpenAI's Chat Completions API.
pub const OPENAI_DEFAULT_BASE_URL: &str = "https://api.openai.com";
/// Default base URL for Groq's OpenAI-compatible Chat Completions API.
pub const GROQ_DEFAULT_BASE_URL: &str = "https://api.groq.com/openai";
/// Default base URL for Google Gemini's OpenAI-compatible Chat Completions API.
pub const GEMINI_DEFAULT_BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta/openai";
/// Default base URL for a local Ollama daemon's OpenAI-compatible API.
///
/// Overridable: running Ollama on another machine over the LAN is a
/// normal setup, and it is the one case where a keyless provider is not
/// on localhost.
pub const OLLAMA_DEFAULT_BASE_URL: &str = "http://localhost:11434/v1";

/// `anthropic-version` header value sent on every Anthropic call.
pub use crate::prompt::ANTHROPIC_VERSION;

/// Default OpenAI chat model. We pick `gpt-4o-mini` because it has
/// solid tool-calling support, sub-second TTFT, and is the cheapest
/// `gpt-4o`-class model — the agent loop dispatches many round-trips
/// per turn so the per-call cost dominates.
pub const OPENAI_DEFAULT_MODEL: &str = "gpt-4o-mini";

/// Cheap classifier model on OpenAI. Re-uses the main model id today —
/// `gpt-4o-mini` is already at the low end of OpenAI's pricing curve and
/// the M27 classifier prompt is one short user message + one-token cap,
/// so swapping in a separate model would not move the cost needle.
pub const OPENAI_CLASSIFIER_MODEL: &str = "gpt-4o-mini";

/// Provider-level error surfaced from `parse_stream_chunk`. We use a
/// thin `String` wrapper rather than re-exporting `crate::Error` to
/// avoid making the trait depend on the agent loop's error machinery.
#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("provider stream parse error: {0}")]
    Parse(String),
}

/// Per-provider knobs the agent loop and validator need.
pub trait LlmProvider: Send + Sync + Debug {
    /// Stable identifier (`"anthropic"`, `"openrouter"`, `"openai"`).
    fn id(&self) -> &'static str;

    /// Default base URL (without trailing slash).
    fn base_url(&self) -> &str;

    /// Default model id when the user has not picked one.
    fn default_model(&self) -> &str;

    /// Cheap classifier model id.
    fn classifier_model(&self) -> &str;

    /// Translate a canonical model id into the shape the underlying
    /// provider expects.
    fn translate_model(&self, model: &str) -> String;

    /// Apply authentication + provider-specific headers.
    fn apply_auth(&self, req: reqwest::RequestBuilder, api_key: &str) -> reqwest::RequestBuilder;

    /// Endpoint path appended to [`base_url`]. Defaults to Anthropic's
    /// `/v1/messages`; OpenAI overrides to `/v1/chat/completions`.
    fn endpoint_path(&self) -> &str {
        "/v1/messages"
    }

    /// Whether this provider needs an API key to be usable.
    ///
    /// Every hosted provider does, so the default is `true` and no
    /// existing implementation changes. A local daemon does not, and
    /// before this existed there was no way to say so: the agent was
    /// only constructed when a key was present, saving a blank key was
    /// rejected, and first launch blocked behind a key prompt. Those
    /// checks now ask the provider instead of assuming.
    fn requires_api_key(&self) -> bool {
        true
    }

    /// Path used to probe the key via a GET models list (OpenAI-compatible
    /// providers). Defaults to `/v1/models`; Gemini overrides to `/models`
    /// because its compat base URL already includes the version segment.
    fn list_models_path(&self) -> &str {
        "/v1/models"
    }

    /// Serialise a canonical [`MessagesRequest`] into the provider's
    /// wire JSON. The default implementation pass-through-serialises the
    /// request via `serde_json::to_value`, which is correct for any
    /// Anthropic-Messages-compatible endpoint (Anthropic, OpenRouter).
    /// OpenAI overrides this to emit chat-completions JSON.
    fn serialize_request(&self, req: &MessagesRequest<'_>) -> Value {
        serde_json::to_value(req).expect("MessagesRequest serialisation must not fail")
    }

    /// Parse one SSE `data:` payload into zero or more canonical
    /// [`StreamEvent`]s. The default implementation deserialises the
    /// chunk as Anthropic-shape JSON; OpenAI overrides to translate
    /// chat-completions deltas into the same canonical events.
    ///
    /// Returns `Ok(vec![])` for chunks the provider does not surface
    /// (e.g. OpenAI's `[DONE]` sentinel).
    fn parse_stream_chunk(&self, raw: &str) -> Result<Vec<StreamEvent>, ProviderError> {
        let ev: StreamEvent =
            serde_json::from_str(raw).map_err(|e| ProviderError::Parse(e.to_string()))?;
        Ok(vec![ev])
    }

    /// Human-readable label for the Settings UI.
    fn label(&self) -> &str {
        self.id()
    }
}

// ---------------------------------------------------------------------
// Anthropic
// ---------------------------------------------------------------------

#[derive(Debug, Default, Clone, Copy)]
pub struct AnthropicProvider;

impl LlmProvider for AnthropicProvider {
    fn id(&self) -> &'static str {
        ANTHROPIC_ID
    }
    fn base_url(&self) -> &str {
        ANTHROPIC_DEFAULT_BASE_URL
    }
    fn default_model(&self) -> &str {
        crate::prompt::DEFAULT_MODEL
    }
    fn classifier_model(&self) -> &str {
        crate::CLASSIFIER_MODEL
    }
    fn translate_model(&self, model: &str) -> String {
        model.to_string()
    }
    fn apply_auth(&self, req: reqwest::RequestBuilder, api_key: &str) -> reqwest::RequestBuilder {
        req.header("x-api-key", api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json")
    }
}

// ---------------------------------------------------------------------
// OpenRouter
// ---------------------------------------------------------------------

#[derive(Debug, Default, Clone, Copy)]
pub struct OpenRouterProvider;

impl LlmProvider for OpenRouterProvider {
    fn id(&self) -> &'static str {
        OPENROUTER_ID
    }
    fn base_url(&self) -> &str {
        OPENROUTER_DEFAULT_BASE_URL
    }
    fn default_model(&self) -> &str {
        crate::prompt::DEFAULT_MODEL
    }
    fn classifier_model(&self) -> &str {
        crate::CLASSIFIER_MODEL
    }
    fn translate_model(&self, model: &str) -> String {
        if model.contains('/') {
            model.to_string()
        } else {
            format!("anthropic/{model}")
        }
    }
    fn apply_auth(&self, req: reqwest::RequestBuilder, api_key: &str) -> reqwest::RequestBuilder {
        req.header("authorization", format!("Bearer {api_key}"))
            .header("HTTP-Referer", "https://edytlab.app")
            .header("X-Title", "edytlab")
            .header("content-type", "application/json")
    }
}

// ---------------------------------------------------------------------
// OpenAI
// ---------------------------------------------------------------------

/// Native OpenAI Chat Completions provider. Translates the canonical
/// Anthropic-shaped [`MessagesRequest`] into chat-completions JSON, and
/// translates streamed `choices[0].delta` chunks back into canonical
/// [`StreamEvent`]s.
///
/// # Tool-call id synthesis
///
/// OpenAI's streamed `tool_calls` deltas use a dense `index` per tool
/// call, but the `id` field is only present on the *first* delta for
/// each index. We track per-stream state in a [`Mutex`] keyed by
/// `(message_id, index)` so subsequent deltas land on the same canonical
/// `tool_use` block. When we never see an explicit id (older snapshots,
/// some Azure OpenAI deployments) we synthesise one as
/// `"call_<message_id>_<index>"` so the agent loop's tool dispatch path
/// — which keys off the id — still functions.
///
/// # Stop reason mapping
///
/// | OpenAI `finish_reason` | Canonical `stop_reason`        |
/// |------------------------|--------------------------------|
/// | `stop`                 | `end_turn`                     |
/// | `tool_calls`           | `tool_use`                     |
/// | `length`               | `max_tokens`                   |
/// | `function_call`        | `tool_use` (legacy alias)      |
/// | `content_filter`       | `end_turn` (closest analogue)  |
/// | anything else / null   | passes through verbatim        |
#[derive(Debug, Default)]
pub struct OpenAIProvider {
    /// Per-call streaming state. Keyed by message id; cleared on
    /// [`StreamEvent::MessageStart`]. The `Vec<Option<usize>>` maps the
    /// `tool_calls[idx].index` -> the canonical content-block index we
    /// allocated for it, so subsequent argument deltas land on the same
    /// block.
    state: Mutex<OpenAIStreamState>,
}

#[derive(Debug, Default)]
struct OpenAIStreamState {
    /// Synthetic message id, set on the first chunk of each response.
    message_id: Option<String>,
    /// Number of canonical content blocks we've emitted so far. The
    /// next text or tool block is allocated at this index.
    next_block_index: u32,
    /// For each OpenAI `tool_calls[i].index`, the canonical block
    /// index we allocated for it (so subsequent deltas with the same
    /// `index` route to the same `tool_use` block).
    tool_block_for_index: Vec<Option<u32>>,
    /// Whether we've already started a text block at index 0. OpenAI
    /// streams text via `choices[0].delta.content`; we lazy-allocate
    /// the text block on the first non-empty content delta.
    text_block_index: Option<u32>,
    /// Whether we've already emitted `MessageStart`.
    started: bool,
}

impl Clone for OpenAIProvider {
    fn clone(&self) -> Self {
        Self {
            state: Mutex::new(OpenAIStreamState::default()),
        }
    }
}

impl LlmProvider for OpenAIProvider {
    fn id(&self) -> &'static str {
        OPENAI_ID
    }
    fn base_url(&self) -> &str {
        OPENAI_DEFAULT_BASE_URL
    }
    fn default_model(&self) -> &str {
        OPENAI_DEFAULT_MODEL
    }
    fn classifier_model(&self) -> &str {
        OPENAI_CLASSIFIER_MODEL
    }
    fn translate_model(&self, model: &str) -> String {
        // OpenAI accepts its own ids verbatim; no namespacing.
        model.to_string()
    }
    fn apply_auth(&self, req: reqwest::RequestBuilder, api_key: &str) -> reqwest::RequestBuilder {
        req.header("authorization", format!("Bearer {api_key}"))
            .header("content-type", "application/json")
    }
    fn endpoint_path(&self) -> &str {
        "/v1/chat/completions"
    }
    fn serialize_request(&self, req: &MessagesRequest<'_>) -> Value {
        // Reset per-stream state for the new turn iteration. The agent
        // loop constructs one request per round-trip, so resetting here
        // matches the lifetime of the SSE stream that follows.
        if let Ok(mut s) = self.state.lock() {
            *s = OpenAIStreamState::default();
        }

        let messages = translate_messages(req);
        let tools = translate_tools(req);

        let mut body = json!({
            "model": req.model,
            "messages": messages,
            "stream": req.stream,
        });

        // OpenAI's newer reasoning-class models reject `max_tokens` and
        // require `max_completion_tokens`. We send the latter only —
        // current `gpt-*` models accept it and it's forward-compatible
        // with `o1`/`o3`.
        if let Some(obj) = body.as_object_mut() {
            obj.insert(
                "max_completion_tokens".to_string(),
                Value::from(req.max_tokens),
            );
            if let Some(t) = tools {
                obj.insert("tools".to_string(), t);
                // `tool_choice: "auto"` is OpenAI's shape. We only set
                // it when tools are actually present (OpenAI rejects
                // `tool_choice` without a `tools` array).
                obj.insert("tool_choice".to_string(), Value::from("auto"));
            }
        }

        body
    }
    fn parse_stream_chunk(&self, raw: &str) -> Result<Vec<StreamEvent>, ProviderError> {
        // OpenAI's stream terminates with the literal `[DONE]` sentinel
        // outside any JSON envelope. Treat it as MessageStop.
        if raw.trim() == "[DONE]" {
            return Ok(vec![StreamEvent::MessageStop]);
        }

        let v: Value =
            serde_json::from_str(raw).map_err(|e| ProviderError::Parse(e.to_string()))?;

        // Surface OpenAI errors as canonical `Error` events. OpenAI
        // emits these inside the SSE body (rather than a non-2xx
        // status) for mid-stream failures.
        if let Some(err) = v.get("error") {
            let message = err
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("openai stream error")
                .to_string();
            let kind = err
                .get("type")
                .and_then(|t| t.as_str())
                .unwrap_or("error")
                .to_string();
            return Ok(vec![StreamEvent::Error {
                error: ApiError { kind, message },
            }]);
        }

        let mut out = Vec::new();
        let mut state = self
            .state
            .lock()
            .map_err(|_| ProviderError::Parse("openai state mutex poisoned".into()))?;

        // Emit a synthetic `MessageStart` on the first chunk we see for
        // this stream. The id is the chunk's `id` if present, else a
        // stable placeholder.
        if !state.started {
            let id = v
                .get("id")
                .and_then(|s| s.as_str())
                .unwrap_or("openai-msg")
                .to_string();
            let model = v
                .get("model")
                .and_then(|s| s.as_str())
                .unwrap_or("openai")
                .to_string();
            state.message_id = Some(id.clone());
            state.started = true;
            out.push(StreamEvent::MessageStart {
                message: MessageMeta { id, model },
            });
        }

        let choice = match v
            .get("choices")
            .and_then(|c| c.as_array())
            .and_then(|a| a.first())
        {
            Some(c) => c,
            // No choices array — keepalive or empty; nothing to emit.
            None => return Ok(out),
        };

        if let Some(delta) = choice.get("delta") {
            // Text delta: `delta.content` is a string.
            if let Some(text) = delta.get("content").and_then(|c| c.as_str()) {
                if !text.is_empty() {
                    let idx = match state.text_block_index {
                        Some(i) => i,
                        None => {
                            let i = state.next_block_index;
                            state.next_block_index += 1;
                            state.text_block_index = Some(i);
                            out.push(StreamEvent::ContentBlockStart {
                                index: i,
                                content_block: ContentBlockStart::Text {
                                    text: String::new(),
                                },
                            });
                            i
                        }
                    };
                    out.push(StreamEvent::ContentBlockDelta {
                        index: idx,
                        delta: ContentBlockDelta::TextDelta {
                            text: text.to_string(),
                        },
                    });
                }
            }

            // Tool call deltas: `delta.tool_calls` is an array of
            // partial calls keyed by `index`.
            if let Some(calls) = delta.get("tool_calls").and_then(|c| c.as_array()) {
                for call in calls {
                    let oai_idx = call.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as usize;

                    // Allocate a canonical block index for this OpenAI
                    // tool-call index, the first time we see it.
                    while state.tool_block_for_index.len() <= oai_idx {
                        state.tool_block_for_index.push(None);
                    }
                    let block_idx = match state.tool_block_for_index[oai_idx] {
                        Some(i) => i,
                        None => {
                            let i = state.next_block_index;
                            state.next_block_index += 1;
                            state.tool_block_for_index[oai_idx] = Some(i);

                            let id = call
                                .get("id")
                                .and_then(|s| s.as_str())
                                .map(str::to_string)
                                .unwrap_or_else(|| {
                                    // Synthesise a stable id when OpenAI
                                    // doesn't supply one.
                                    let m = state.message_id.as_deref().unwrap_or("msg");
                                    format!("call_{m}_{oai_idx}")
                                });
                            let name = call
                                .get("function")
                                .and_then(|f| f.get("name"))
                                .and_then(|s| s.as_str())
                                .unwrap_or("")
                                .to_string();

                            out.push(StreamEvent::ContentBlockStart {
                                index: i,
                                content_block: ContentBlockStart::ToolUse {
                                    id,
                                    name,
                                    input: json!({}),
                                },
                            });
                            i
                        }
                    };

                    // Argument fragments stream incrementally on
                    // `function.arguments` as opaque JSON-stringified
                    // text, identical in spirit to Anthropic's
                    // `input_json_delta`.
                    if let Some(args_chunk) = call
                        .get("function")
                        .and_then(|f| f.get("arguments"))
                        .and_then(|s| s.as_str())
                    {
                        if !args_chunk.is_empty() {
                            out.push(StreamEvent::ContentBlockDelta {
                                index: block_idx,
                                delta: ContentBlockDelta::InputJsonDelta {
                                    partial_json: args_chunk.to_string(),
                                },
                            });
                        }
                    }
                }
            }
        }

        // Final chunk: `finish_reason` is non-null. Emit
        // `ContentBlockStop` for every block we opened, then
        // `MessageDelta { stop_reason }` and `MessageStop`.
        if let Some(reason) = choice.get("finish_reason").and_then(|r| r.as_str()) {
            let canonical = map_finish_reason(reason);

            if let Some(i) = state.text_block_index.take() {
                out.push(StreamEvent::ContentBlockStop { index: i });
            }
            for i in state.tool_block_for_index.drain(..).flatten() {
                out.push(StreamEvent::ContentBlockStop { index: i });
            }

            out.push(StreamEvent::MessageDelta {
                delta: crate::anthropic::MessageDeltaInner {
                    stop_reason: Some(canonical.to_string()),
                },
            });
            out.push(StreamEvent::MessageStop);
        }

        Ok(out)
    }
}

/// Map an OpenAI `finish_reason` to the canonical Anthropic-shaped
/// `stop_reason` string.
fn map_finish_reason(r: &str) -> &str {
    match r {
        "stop" => "end_turn",
        "tool_calls" | "function_call" => "tool_use",
        "length" => "max_tokens",
        "content_filter" => "end_turn",
        other => other,
    }
}

/// Translate a canonical conversation into OpenAI `messages` JSON.
///
/// Rules:
/// * The system blocks are concatenated and emitted as one
///   `{role: "system"}` message.
/// * Anthropic's `user` messages with `tool_result` blocks become one
///   `{role: "tool", tool_call_id, content}` per block — OpenAI
///   represents tool outputs as top-level messages, not nested blocks.
/// * Anthropic's `assistant` messages combine `text` and `tool_use`
///   blocks into a single `{role: "assistant", content, tool_calls}`
///   message.
fn translate_messages(req: &MessagesRequest<'_>) -> Vec<Value> {
    let mut out = Vec::with_capacity(req.messages.len() + 1);

    // System: concatenate every Anthropic system block's text.
    let system_text: String = req
        .system
        .iter()
        .map(|b| b.text)
        .collect::<Vec<_>>()
        .join("\n\n");
    if !system_text.is_empty() {
        out.push(json!({ "role": "system", "content": system_text }));
    }

    for msg in req.messages.iter() {
        match msg.role {
            Role::User => {
                // Split into (text-and-image content) and tool_results.
                let mut text_parts: Vec<&str> = Vec::new();
                let mut tool_results: Vec<(&str, &str, Option<bool>)> = Vec::new();
                for block in &msg.content {
                    match block {
                        ContentBlock::Text { text } => text_parts.push(text.as_str()),
                        ContentBlock::ToolResult {
                            tool_use_id,
                            content,
                            is_error,
                        } => tool_results.push((tool_use_id.as_str(), content.as_str(), *is_error)),
                        // tool_use blocks shouldn't appear on user
                        // messages; if they do, ignore them — the model
                        // should never see what looks like its own
                        // tool_use originating from the user.
                        ContentBlock::ToolUse { .. } => {}
                    }
                }

                // Tool results come *first* so the assistant turn that
                // generated the tool_calls precedes them in conversation
                // order. (We rely on the agent loop having pushed the
                // tool_result user turn directly after the assistant
                // tool_use turn, so the relative ordering is already
                // correct here.)
                for (id, body, is_error) in tool_results {
                    let content = if matches!(is_error, Some(true)) {
                        format!("error: {body}")
                    } else {
                        body.to_string()
                    };
                    out.push(json!({
                        "role": "tool",
                        "tool_call_id": id,
                        "content": content,
                    }));
                }

                if !text_parts.is_empty() {
                    out.push(json!({
                        "role": "user",
                        "content": text_parts.join(""),
                    }));
                }
            }
            Role::Assistant => {
                let mut text_parts: Vec<&str> = Vec::new();
                let mut tool_calls: Vec<Value> = Vec::new();
                for block in &msg.content {
                    match block {
                        ContentBlock::Text { text } => text_parts.push(text.as_str()),
                        ContentBlock::ToolUse { id, name, input } => {
                            tool_calls.push(json!({
                                "id": id,
                                "type": "function",
                                "function": {
                                    "name": name,
                                    "arguments": serde_json::to_string(input)
                                        .unwrap_or_else(|_| "{}".to_string()),
                                }
                            }));
                        }
                        ContentBlock::ToolResult { .. } => {
                            // Out of place on an assistant turn; skip.
                        }
                    }
                }

                let mut entry = serde_json::Map::new();
                entry.insert("role".to_string(), Value::from("assistant"));
                if text_parts.is_empty() {
                    // OpenAI requires `content` to be present on
                    // assistant messages even when only tool_calls are
                    // emitted. Use null so the JSON is well-formed.
                    entry.insert("content".to_string(), Value::Null);
                } else {
                    entry.insert("content".to_string(), Value::from(text_parts.join("")));
                }
                if !tool_calls.is_empty() {
                    entry.insert("tool_calls".to_string(), Value::from(tool_calls));
                }
                out.push(Value::Object(entry));
            }
        }
    }

    out
}

/// Translate the canonical tool schema array (Anthropic shape) into
/// OpenAI chat-completions `tools`. Returns `None` when there are no
/// tools (OpenAI rejects an empty array).
fn translate_tools(req: &MessagesRequest<'_>) -> Option<Value> {
    let arr = req.tools.as_ref()?.as_array()?;
    if arr.is_empty() {
        return None;
    }
    let translated: Vec<Value> = arr
        .iter()
        .map(|t| {
            let name = t.get("name").cloned().unwrap_or(Value::Null);
            let description = t.get("description").cloned().unwrap_or(Value::Null);
            let parameters = t
                .get("input_schema")
                .cloned()
                .unwrap_or_else(|| json!({"type":"object","properties":{}}));
            json!({
                "type": "function",
                "function": {
                    "name": name,
                    "description": description,
                    "parameters": parameters,
                }
            })
        })
        .collect();
    Some(Value::from(translated))
}

// ---------------------------------------------------------------------
// Groq
// ---------------------------------------------------------------------

/// Groq Cloud provider. Uses Groq's OpenAI-compatible Chat Completions
/// endpoint. Delegates stream parsing and request serialisation to the
/// shared [`OpenAIProvider`] state machine — only the base URL, auth
/// header, and model defaults differ.
#[derive(Debug, Default)]
pub struct GroqProvider {
    inner: OpenAIProvider,
}

impl Clone for GroqProvider {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl LlmProvider for GroqProvider {
    fn id(&self) -> &'static str {
        GROQ_ID
    }
    fn base_url(&self) -> &str {
        GROQ_DEFAULT_BASE_URL
    }
    fn default_model(&self) -> &str {
        "llama-3.3-70b-versatile"
    }
    fn classifier_model(&self) -> &str {
        "llama-3.1-8b-instant"
    }
    fn translate_model(&self, model: &str) -> String {
        model.to_string()
    }
    fn apply_auth(&self, req: reqwest::RequestBuilder, api_key: &str) -> reqwest::RequestBuilder {
        req.header("authorization", format!("Bearer {api_key}"))
            .header("content-type", "application/json")
    }
    fn endpoint_path(&self) -> &str {
        "/v1/chat/completions"
    }
    fn serialize_request(&self, req: &MessagesRequest<'_>) -> Value {
        self.inner.serialize_request(req)
    }
    fn parse_stream_chunk(&self, raw: &str) -> Result<Vec<StreamEvent>, ProviderError> {
        self.inner.parse_stream_chunk(raw)
    }
    fn label(&self) -> &str {
        "Groq"
    }
}

// ---------------------------------------------------------------------
// Gemini
// ---------------------------------------------------------------------

/// Google Gemini provider via the OpenAI-compatible endpoint at
/// `generativelanguage.googleapis.com/v1beta/openai`. Delegates stream
/// parsing and request serialisation to the shared [`OpenAIProvider`]
/// state machine — only the base URL, auth header, and model defaults
/// differ.
#[derive(Debug, Default)]
pub struct GeminiProvider {
    inner: OpenAIProvider,
}

impl Clone for GeminiProvider {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl LlmProvider for GeminiProvider {
    fn id(&self) -> &'static str {
        GEMINI_ID
    }
    fn base_url(&self) -> &str {
        GEMINI_DEFAULT_BASE_URL
    }
    fn default_model(&self) -> &str {
        "gemini-2.0-flash"
    }
    fn classifier_model(&self) -> &str {
        "gemini-2.0-flash"
    }
    fn translate_model(&self, model: &str) -> String {
        model.to_string()
    }
    fn apply_auth(&self, req: reqwest::RequestBuilder, api_key: &str) -> reqwest::RequestBuilder {
        req.header("authorization", format!("Bearer {api_key}"))
            .header("content-type", "application/json")
    }
    fn endpoint_path(&self) -> &str {
        "/chat/completions"
    }
    fn list_models_path(&self) -> &str {
        "/models"
    }
    fn serialize_request(&self, req: &MessagesRequest<'_>) -> Value {
        self.inner.serialize_request(req)
    }
    fn parse_stream_chunk(&self, raw: &str) -> Result<Vec<StreamEvent>, ProviderError> {
        self.inner.parse_stream_chunk(raw)
    }
    fn label(&self) -> &str {
        "Google Gemini"
    }
}

// ---------------------------------------------------------------------
// Ollama (local)
// ---------------------------------------------------------------------

/// Local models via Ollama's OpenAI-compatible API.
///
/// Same wrapper shape as [`GroqProvider`] and [`GeminiProvider`] —
/// request serialisation and stream parsing delegate to the shared
/// [`OpenAIProvider`] state machine.
///
/// What is different is that there is no key. `apply_auth` ignores the
/// one it is handed rather than sending an empty `Bearer`, which some
/// proxies in front of a daemon will reject outright.
///
/// `default_model` is a guess in a way the hosted providers' are not:
/// Ollama serves whatever the user has pulled, and there is no model
/// that is guaranteed present. `llama3.2` is the common default, and if
/// it is absent the daemon says so clearly — which is a better failure
/// than an empty dropdown.
#[derive(Debug, Default)]
pub struct OllamaProvider {
    inner: OpenAIProvider,
}

impl Clone for OllamaProvider {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl LlmProvider for OllamaProvider {
    fn id(&self) -> &'static str {
        OLLAMA_ID
    }
    fn base_url(&self) -> &str {
        OLLAMA_DEFAULT_BASE_URL
    }
    fn default_model(&self) -> &str {
        "llama3.2"
    }
    fn classifier_model(&self) -> &str {
        "llama3.2"
    }
    fn translate_model(&self, model: &str) -> String {
        model.to_string()
    }
    fn apply_auth(&self, req: reqwest::RequestBuilder, _api_key: &str) -> reqwest::RequestBuilder {
        // No Authorization header at all. An empty `Bearer ` is worse
        // than none: it looks like a malformed credential rather than
        // an absent one.
        req.header("content-type", "application/json")
    }
    fn requires_api_key(&self) -> bool {
        false
    }
    fn endpoint_path(&self) -> &str {
        "/chat/completions"
    }
    fn list_models_path(&self) -> &str {
        // The base URL already carries `/v1`.
        "/models"
    }
    fn serialize_request(&self, req: &MessagesRequest<'_>) -> Value {
        self.inner.serialize_request(req)
    }
    fn parse_stream_chunk(&self, raw: &str) -> Result<Vec<StreamEvent>, ProviderError> {
        self.inner.parse_stream_chunk(raw)
    }
    fn label(&self) -> &str {
        "Ollama (local)"
    }
}

// ---------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------

/// Build a provider from its stable id. Unknown ids fall back to
/// Anthropic.
pub fn provider_from_id(id: &str) -> Arc<dyn LlmProvider> {
    match id {
        OPENROUTER_ID => Arc::new(OpenRouterProvider),
        OPENAI_ID => Arc::new(OpenAIProvider::default()),
        GROQ_ID => Arc::new(GroqProvider::default()),
        GEMINI_ID => Arc::new(GeminiProvider::default()),
        OLLAMA_ID => Arc::new(OllamaProvider::default()),
        _ => Arc::new(AnthropicProvider),
    }
}

/// Stable ids for the providers shipped today.
pub const SUPPORTED_PROVIDER_IDS: &[&str] = &[
    ANTHROPIC_ID,
    OPENROUTER_ID,
    OPENAI_ID,
    GROQ_ID,
    GEMINI_ID,
    OLLAMA_ID,
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anthropic::{Message, SystemBlock};

    #[test]
    fn anthropic_translate_model_is_identity() {
        let p = AnthropicProvider;
        assert_eq!(p.translate_model("claude-sonnet-4-6"), "claude-sonnet-4-6");
    }

    #[test]
    fn openrouter_translate_model_prepends_anthropic_namespace() {
        let p = OpenRouterProvider;
        assert_eq!(
            p.translate_model("claude-sonnet-4-6"),
            "anthropic/claude-sonnet-4-6"
        );
    }

    #[test]
    fn openrouter_translate_model_passes_through_already_qualified_ids() {
        let p = OpenRouterProvider;
        assert_eq!(
            p.translate_model("anthropic/claude-sonnet-4-6"),
            "anthropic/claude-sonnet-4-6"
        );
        assert_eq!(
            p.translate_model("openai/gpt-4o-mini"),
            "openai/gpt-4o-mini"
        );
    }

    #[test]
    fn openai_translate_model_is_identity() {
        let p = OpenAIProvider::default();
        assert_eq!(p.translate_model("gpt-4o-mini"), "gpt-4o-mini");
        assert_eq!(p.translate_model("o3-mini"), "o3-mini");
        // No namespacing — OpenAI accepts its own ids verbatim.
        assert!(!p.translate_model("gpt-4o").contains('/'));
    }

    #[test]
    fn endpoint_paths_are_correct_per_provider() {
        assert_eq!(AnthropicProvider.endpoint_path(), "/v1/messages");
        assert_eq!(OpenRouterProvider.endpoint_path(), "/v1/messages");
        assert_eq!(
            OpenAIProvider::default().endpoint_path(),
            "/v1/chat/completions"
        );
    }

    #[test]
    fn provider_ids_are_stable() {
        assert_eq!(AnthropicProvider.id(), "anthropic");
        assert_eq!(OpenRouterProvider.id(), "openrouter");
        assert_eq!(OpenAIProvider::default().id(), "openai");
    }

    #[test]
    fn provider_from_id_falls_back_to_anthropic_for_unknown() {
        let p = provider_from_id("");
        assert_eq!(p.id(), "anthropic");
        let p = provider_from_id("unknown-provider");
        assert_eq!(p.id(), "anthropic");
    }

    #[test]
    fn provider_from_id_returns_openai() {
        let p = provider_from_id("openai");
        assert_eq!(p.id(), "openai");
    }

    #[test]
    fn supported_provider_ids_contains_openai() {
        assert!(SUPPORTED_PROVIDER_IDS.contains(&"openai"));
    }

    /// The default has to stay `true`, or adding a provider silently
    /// makes it keyless and the key prompt disappears for it.
    #[test]
    fn every_hosted_provider_still_requires_a_key() {
        for id in [ANTHROPIC_ID, OPENROUTER_ID, OPENAI_ID, GROQ_ID, GEMINI_ID] {
            assert!(
                provider_from_id(id).requires_api_key(),
                "{id} must still require a key"
            );
        }
    }

    #[test]
    fn ollama_is_keyless_and_registered() {
        let p = provider_from_id(OLLAMA_ID);
        assert_eq!(p.id(), OLLAMA_ID, "the factory fell through to Anthropic");
        assert!(!p.requires_api_key());
        assert!(SUPPORTED_PROVIDER_IDS.contains(&OLLAMA_ID));
    }

    /// An empty `Bearer` reads as a malformed credential rather than an
    /// absent one, and some proxies in front of a daemon reject it.
    #[test]
    fn ollama_sends_no_authorization_header() {
        let p = provider_from_id(OLLAMA_ID);
        let req = reqwest::Client::new().post("http://localhost:11434/v1/chat/completions");
        let built = p.apply_auth(req, "").build().expect("request builds");
        assert!(
            built.headers().get("authorization").is_none(),
            "ollama must not send an Authorization header"
        );
    }

    #[test]
    fn supported_provider_ids_contains_groq_and_gemini() {
        assert!(SUPPORTED_PROVIDER_IDS.contains(&"groq"));
        assert!(SUPPORTED_PROVIDER_IDS.contains(&"gemini"));
    }

    #[test]
    fn groq_provider_attributes() {
        let p = GroqProvider::default();
        assert_eq!(p.id(), "groq");
        assert_eq!(p.endpoint_path(), "/v1/chat/completions");
        assert_eq!(p.list_models_path(), "/v1/models");
        assert!(p.base_url().contains("groq.com"));
        assert_eq!(
            p.translate_model("llama-3.3-70b-versatile"),
            "llama-3.3-70b-versatile"
        );
    }

    #[test]
    fn gemini_provider_attributes() {
        let p = GeminiProvider::default();
        assert_eq!(p.id(), "gemini");
        assert_eq!(p.endpoint_path(), "/chat/completions");
        assert_eq!(p.list_models_path(), "/models");
        assert!(p.base_url().contains("generativelanguage.googleapis.com"));
        assert_eq!(p.translate_model("gemini-2.0-flash"), "gemini-2.0-flash");
    }

    #[test]
    fn provider_from_id_returns_groq_and_gemini() {
        assert_eq!(provider_from_id("groq").id(), "groq");
        assert_eq!(provider_from_id("gemini").id(), "gemini");
    }

    #[test]
    fn anthropic_serialize_request_is_canonical_messages_shape() {
        // Sanity: Anthropic's default trait serialization must produce
        // the legacy `/v1/messages` body so the OpenRouter pass-through
        // doesn't regress.
        let msgs: Vec<Message> = vec![Message {
            role: Role::User,
            content: vec![ContentBlock::Text { text: "hi".into() }],
        }];
        let req = MessagesRequest {
            model: "claude-sonnet-4-6",
            max_tokens: 16,
            system: vec![SystemBlock {
                kind: "text",
                text: "be helpful",
                cache_control: None,
            }],
            messages: &msgs,
            tools: None,
            tool_choice: None,
            stream: true,
        };
        let body = AnthropicProvider.serialize_request(&req);
        assert_eq!(body["model"], "claude-sonnet-4-6");
        assert_eq!(body["max_tokens"], 16);
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["messages"][0]["content"][0]["type"], "text");
    }

    #[test]
    fn openai_serialize_request_translates_to_chat_completions_shape() {
        let msgs: Vec<Message> = vec![
            Message {
                role: Role::User,
                content: vec![ContentBlock::Text {
                    text: "hello".into(),
                }],
            },
            Message {
                role: Role::Assistant,
                content: vec![
                    ContentBlock::Text {
                        text: "ok, calling the tool".into(),
                    },
                    ContentBlock::ToolUse {
                        id: "call_1".into(),
                        name: "load_audio".into(),
                        input: json!({"path": "/tmp/x.wav"}),
                    },
                ],
            },
            Message {
                role: Role::User,
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "call_1".into(),
                    content: "{\"node_id\":\"abcd\"}".into(),
                    is_error: None,
                }],
            },
        ];
        let req = MessagesRequest {
            model: "gpt-4o-mini",
            max_tokens: 4096,
            system: vec![SystemBlock {
                kind: "text",
                text: "be a DAW",
                cache_control: None,
            }],
            messages: &msgs,
            tools: Some(json!([{
                "name": "load_audio",
                "description": "Load a WAV file",
                "input_schema": {"type": "object", "properties": {"path": {"type": "string"}}}
            }])),
            tool_choice: None,
            stream: true,
        };

        let p = OpenAIProvider::default();
        let body = p.serialize_request(&req);

        assert_eq!(body["model"], "gpt-4o-mini");
        assert_eq!(body["stream"], true);
        // OpenAI's newer field name.
        assert_eq!(body["max_completion_tokens"], 4096);
        assert!(
            body.get("max_tokens").is_none(),
            "should NOT send max_tokens"
        );

        // System message comes first.
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][0]["content"], "be a DAW");

        // User text.
        assert_eq!(body["messages"][1]["role"], "user");
        assert_eq!(body["messages"][1]["content"], "hello");

        // Assistant with text + tool_calls combined.
        assert_eq!(body["messages"][2]["role"], "assistant");
        assert_eq!(body["messages"][2]["content"], "ok, calling the tool");
        assert_eq!(body["messages"][2]["tool_calls"][0]["id"], "call_1");
        assert_eq!(body["messages"][2]["tool_calls"][0]["type"], "function");
        assert_eq!(
            body["messages"][2]["tool_calls"][0]["function"]["name"],
            "load_audio"
        );
        // Arguments must be a stringified JSON object.
        let args_str = body["messages"][2]["tool_calls"][0]["function"]["arguments"]
            .as_str()
            .unwrap();
        let parsed: Value = serde_json::from_str(args_str).unwrap();
        assert_eq!(parsed["path"], "/tmp/x.wav");

        // Tool result becomes a top-level `tool` message.
        assert_eq!(body["messages"][3]["role"], "tool");
        assert_eq!(body["messages"][3]["tool_call_id"], "call_1");
        assert!(body["messages"][3]["content"]
            .as_str()
            .unwrap()
            .contains("node_id"));

        // Tools translated to function-shaped entries.
        assert_eq!(body["tools"][0]["type"], "function");
        assert_eq!(body["tools"][0]["function"]["name"], "load_audio");
        assert_eq!(body["tool_choice"], "auto");
    }

    #[test]
    fn openai_parse_stream_chunk_emits_text_then_stop() {
        let p = OpenAIProvider::default();
        let chunks = vec![
            // First delta: text.
            r#"{"id":"chatcmpl-1","model":"gpt-4o-mini","choices":[{"index":0,"delta":{"role":"assistant","content":"hello"}}]}"#,
            // Second delta: more text.
            r#"{"id":"chatcmpl-1","model":"gpt-4o-mini","choices":[{"index":0,"delta":{"content":" world"}}]}"#,
            // Final: finish_reason=stop.
            r#"{"id":"chatcmpl-1","model":"gpt-4o-mini","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}"#,
            "[DONE]",
        ];

        let mut events = Vec::new();
        for c in chunks {
            events.extend(p.parse_stream_chunk(c).unwrap());
        }

        // First chunk -> MessageStart + ContentBlockStart + delta
        assert!(matches!(events[0], StreamEvent::MessageStart { .. }));
        assert!(matches!(events[1], StreamEvent::ContentBlockStart { .. }));
        assert!(matches!(
            events[2],
            StreamEvent::ContentBlockDelta {
                delta: ContentBlockDelta::TextDelta { .. },
                ..
            }
        ));
        // Second chunk -> another text delta
        assert!(matches!(
            events[3],
            StreamEvent::ContentBlockDelta {
                delta: ContentBlockDelta::TextDelta { .. },
                ..
            }
        ));
        // Final chunk -> ContentBlockStop, MessageDelta(stop_reason=end_turn), MessageStop
        let stop_idx = events.iter().position(|e| matches!(e, StreamEvent::MessageDelta{ delta } if delta.stop_reason.as_deref() == Some("end_turn"))).expect("stop reason");
        assert!(stop_idx > 0);
        assert!(matches!(events.last().unwrap(), StreamEvent::MessageStop));
    }

    #[test]
    fn openai_parse_stream_chunk_emits_tool_use_blocks() {
        let p = OpenAIProvider::default();
        let chunks = vec![
            // First chunk: tool_calls with id + name.
            r#"{"id":"chatcmpl-2","model":"gpt-4o-mini","choices":[{"index":0,"delta":{"role":"assistant","tool_calls":[{"index":0,"id":"call_xyz","type":"function","function":{"name":"load_audio","arguments":""}}]}}]}"#,
            // Second chunk: argument fragment.
            r#"{"id":"chatcmpl-2","model":"gpt-4o-mini","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"path\":"}}]}}]}"#,
            // Third chunk: more args.
            r#"{"id":"chatcmpl-2","model":"gpt-4o-mini","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"/x.wav\"}"}}]}}]}"#,
            // Final chunk.
            r#"{"id":"chatcmpl-2","model":"gpt-4o-mini","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}"#,
        ];

        let mut events = Vec::new();
        for c in chunks {
            events.extend(p.parse_stream_chunk(c).unwrap());
        }

        // Find the ToolUse block start.
        let tool_start = events
            .iter()
            .find(|e| {
                matches!(
                    e,
                    StreamEvent::ContentBlockStart {
                        content_block: ContentBlockStart::ToolUse { .. },
                        ..
                    }
                )
            })
            .expect("tool_use ContentBlockStart");
        if let StreamEvent::ContentBlockStart {
            content_block: ContentBlockStart::ToolUse { id, name, .. },
            ..
        } = tool_start
        {
            assert_eq!(id, "call_xyz");
            assert_eq!(name, "load_audio");
        }

        // Argument deltas are emitted in order.
        let mut accum = String::new();
        for ev in &events {
            if let StreamEvent::ContentBlockDelta {
                delta: ContentBlockDelta::InputJsonDelta { partial_json },
                ..
            } = ev
            {
                accum.push_str(partial_json);
            }
        }
        let parsed: Value = serde_json::from_str(&accum).unwrap();
        assert_eq!(parsed["path"], "/x.wav");

        // Stop reason mapped to tool_use.
        assert!(events.iter().any(
            |e| matches!(e, StreamEvent::MessageDelta{delta} if delta.stop_reason.as_deref() == Some("tool_use"))
        ));
    }

    #[test]
    fn openai_finish_reason_mapping() {
        assert_eq!(map_finish_reason("stop"), "end_turn");
        assert_eq!(map_finish_reason("tool_calls"), "tool_use");
        assert_eq!(map_finish_reason("function_call"), "tool_use");
        assert_eq!(map_finish_reason("length"), "max_tokens");
        assert_eq!(map_finish_reason("content_filter"), "end_turn");
        assert_eq!(map_finish_reason("custom"), "custom");
    }

    #[test]
    fn openai_synthesises_tool_call_id_when_absent() {
        // Some Azure OpenAI deployments omit the `id` field. We must
        // still produce a stable id so the agent loop can correlate
        // tool_use -> tool_result.
        let p = OpenAIProvider::default();
        let events = p
            .parse_stream_chunk(
                r#"{"id":"chatcmpl-z","model":"gpt-4o-mini","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"type":"function","function":{"name":"foo","arguments":""}}]}}]}"#,
            )
            .unwrap();
        let id = events
            .iter()
            .find_map(|e| match e {
                StreamEvent::ContentBlockStart {
                    content_block: ContentBlockStart::ToolUse { id, .. },
                    ..
                } => Some(id.clone()),
                _ => None,
            })
            .unwrap();
        assert!(
            id.starts_with("call_"),
            "synthesised id should start with call_, got {id}"
        );
        assert!(id.contains("chatcmpl-z"), "id should embed message id");
    }
}
