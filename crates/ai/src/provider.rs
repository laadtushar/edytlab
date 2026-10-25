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

pub use crate::chatgpt::provider::{ChatGptProvider, ReasoningReplay};

use crate::anthropic::{
    ApiError, ContentBlock, ContentBlockDelta, ContentBlockStart, MessageMeta, MessagesRequest,
    Role, StreamEvent, ToolChoice,
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
/// Stable id for the ChatGPT plan provider (Sign in with ChatGPT, #474).
pub use crate::chatgpt::provider::CHATGPT_ID;

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

/// Default OpenAI chat model: `gpt-6-luna`, which OpenAI describes as "our
/// most efficient model for focused, high-volume tasks"
/// (<https://developers.openai.com/api/docs/models>). It is the cheapest
/// current model that takes tools on Chat Completions, which is the only API
/// edytlab speaks, and the agent loop dispatches many round-trips per turn,
/// so the per-call cost dominates. Its page lists $0.1 in and $0.5 out per 1M
/// tokens (<https://developers.openai.com/api/docs/models/gpt-6-luna>).
///
/// Not the larger models: `gpt-6.1-sol` takes tools only through the
/// Responses API ("Chat Completions is supported without tool calling"), and
/// `gpt-6-astra` costs 100 times as much and is one dropdown pick away.
///
/// It takes tools on Chat Completions only with `reasoning_effort` set to
/// `none`, which [`openai_reasoning_effort`] sends.
pub const OPENAI_DEFAULT_MODEL: &str = "gpt-6-luna";

/// Cheap classifier model on OpenAI. The same model as the main one: it is
/// already OpenAI's cost-optimised model, and the classifier prompt is one
/// short user message, so a separate model would not move the cost needle.
pub const OPENAI_CLASSIFIER_MODEL: &str = "gpt-6-luna";

/// OpenRouter's default model. OpenRouter's ids are its own, with a dot in
/// the version (`anthropic/claude-sonnet-5.5`, as its public
/// `/api/v1/models` list and
/// <https://openrouter.ai/docs/cookbook/evaluate-and-optimize/model-migrations/claude-5-5>
/// give it), so it is not the Anthropic id with a prefix. A bare Anthropic id
/// run through [`OpenRouterProvider::translate_model`] is not guaranteed to
/// resolve there: the dated form of the old classifier did not.
pub const OPENROUTER_DEFAULT_MODEL: &str = "anthropic/claude-sonnet-5.5";

/// OpenRouter's classifier model: the Haiku tier, which Anthropic's models
/// page describes as "for high-volume, latency-sensitive tasks such as
/// classification, extraction, and routing". OpenRouter allows thinking to be
/// disabled on it, which the classifier request does.
pub const OPENROUTER_CLASSIFIER_MODEL: &str = "anthropic/claude-haiku-5.5";

/// Groq's default model. Both of Groq's defaults are in the Production
/// table of <https://console.groq.com/docs/models> and take tools
/// (<https://console.groq.com/docs/tool-use/overview>).
pub const GROQ_DEFAULT_MODEL: &str = "llama-3.3-70b-versatile";

/// Groq's classifier model: its small, fast one.
pub const GROQ_CLASSIFIER_MODEL: &str = "llama-3.1-8b-instant";

/// Gemini's default model: Google's pick for new projects, "our most
/// intelligent Flash model" (<https://ai.google.dev/gemini-api/docs/models>).
/// `gemini-2.0-flash`, which this replaced, was shut down on 2026-06-01
/// (<https://ai.google.dev/gemini-api/docs/deprecations>).
pub const GEMINI_DEFAULT_MODEL: &str = "gemini-3.8-flash";

/// Gemini's classifier model: "our fastest, most cost-effective 3.5 model for
/// high-throughput execution", which thinks at `minimal` by default
/// (<https://ai.google.dev/gemini-api/docs/thinking>), what one word of
/// output wants.
pub const GEMINI_CLASSIFIER_MODEL: &str = "gemini-3.5-flash-lite";

/// The field that carries the output cap in a streamed chat-completions body
/// for `provider_id`.
///
/// `max_completion_tokens`, which OpenAI's newer models require, for every
/// provider but Ollama. Ollama's OpenAI-compatible API lists `max_tokens`
/// among the request fields it supports and not `max_completion_tokens`
/// (<https://docs.ollama.com/api/openai-compatibility>), so the cap went out
/// under a name its documentation does not list, which a server is free to
/// ignore, leaving a local model to run on without one.
/// The classifier and plan requests already sent `max_tokens` to every
/// provider but OpenAI (`one_shot_body`).
pub(crate) fn output_limit_field(provider_id: &str) -> &'static str {
    if provider_id == OLLAMA_ID {
        "max_tokens"
    } else {
        "max_completion_tokens"
    }
}

/// What to send as `reasoning_effort` to OpenAI's own endpoint for `model`,
/// if it needs one.
///
/// `gpt-6-luna` "supports function calling only with `reasoning_effort` set
/// to `none`" on Chat Completions
/// (<https://developers.openai.com/api/docs/models/gpt-6-luna>), and every
/// edit the agent makes is a tool call, so without this the default model
/// cannot edit.
///
/// It is a rule for that one model (and a dated snapshot of it) on purpose.
/// `none` is not an effort every model accepts, and a non-reasoning model
/// rejects the field altogether, so sending it to others would break models
/// that work. A successor needs its own line, and
/// `every_provider_defaults_to_a_current_model` makes the next default visible.
pub(crate) fn openai_reasoning_effort(model: &str) -> Option<&'static str> {
    (model == "gpt-6-luna" || model.starts_with("gpt-6-luna-")).then_some("none")
}

/// Whether a failed chat request means the provider has no such model
/// (retired, renamed, or never existed), as opposed to any other failure.
///
/// Providers retire models, and a model somebody chose keeps being sent
/// until they choose another. The answer is a 404 that, shown raw, says
/// nothing a person can act on; knowing it is the model lets the app say so
/// and offer the default. It is a claim ("it may have been retired"), so it is
/// made only where the answer is unambiguous, and everything else stays a
/// plain API error:
///
/// * Never for a provider that needs no key. Ollama's own message, "model
///   ... not found, try pulling it first", is the useful one, and "retired"
///   would be wrong for a model that was simply never pulled.
/// * A body that is not JSON is a wrong base path on a proxy, not a model.
///   Gemini wraps its errors in an array, so the first element is read too.
/// * `error.code == "model_not_found"`, which OpenAI and the providers that
///   copy its API use, at any status.
/// * Otherwise a **404** only, never a 400, which can name the model for other
///   reasons (a field it does not take): on Anthropic's wire format an
///   `error.type` of `not_found_error`, since `POST /v1/messages` names no
///   resource but the model; and on either format, a 404 whose body names
///   the model that was sent.
///
/// Anthropic's answer for a retired model is
/// `{"type":"error","error":{"type":"not_found_error","message":"model: <id>"}}`
/// with status 404, captured from the API (see the test). The other
/// providers' shapes are written from their documented fields and are not
/// captured; the rules key on those fields, not on wording.
pub(crate) fn is_model_unavailable(
    provider: &dyn LlmProvider,
    status: u16,
    body: &str,
    wire_model: &str,
) -> bool {
    if !provider.requires_api_key() {
        return false;
    }
    let Ok(json) = serde_json::from_str::<Value>(body) else {
        return false;
    };
    let error = match &json {
        Value::Array(items) => items.first().and_then(|item| item.get("error")),
        other => other.get("error"),
    };
    let Some(error) = error else {
        return false;
    };

    if error.get("code").and_then(Value::as_str) == Some("model_not_found") {
        return true;
    }
    if status != 404 {
        return false;
    }
    if provider.wire_format() == WireFormat::AnthropicMessages
        && error.get("type").and_then(Value::as_str) == Some("not_found_error")
    {
        return true;
    }
    !wire_model.is_empty() && body.contains(wire_model)
}

/// A failure a provider may know how to name better than the generic errors.
///
/// Handed to [`LlmProvider::classify_failure`] at the two places the agent
/// loop turns a provider's failure into an [`Error`](crate::Error): a request
/// answered with a non-2xx status, and an `error` inside a stream. Both carry
/// what the provider said, unaltered, so a provider whose codes call for
/// different handling (the ChatGPT plan's `subscription_sharing_*` codes) can
/// tell them apart instead of showing every one as the same text.
#[derive(Debug, Clone, Copy)]
pub enum ProviderFailure<'a> {
    /// The request was answered with `status` and `body`, before any stream.
    /// `request_id` is the response's `x-request-id` (or `openai-request-id`).
    Http {
        status: u16,
        body: &'a str,
        request_id: Option<&'a str>,
    },
    /// A stream carried an error event: the API's `code` (the error's
    /// `kind`), its message, and the request field it is about, if it named
    /// one; and the request ID of the response the stream came on.
    Stream {
        kind: &'a str,
        message: &'a str,
        param: Option<&'a str>,
        request_id: Option<&'a str>,
    },
}

/// What a provider changed in itself after its server refused a request, so
/// that the same request, built again, is a different one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Adaptation {
    /// The way the tools are laid out in the request, or how many of them
    /// are sent, changed ([`LlmProvider::tool_set`] may answer differently).
    /// Anything produced under the old layout is not valid under the new:
    /// the agent loop picks the tools again and drops the model's thinking.
    ToolLayout,
    /// The provider stopped asking for, and replaying, the model's
    /// reasoning.
    Reasoning,
}

/// What a provider authenticates with.
///
/// The default for a hosted provider is a key; [`Self::None`] is a local
/// daemon with nothing to be wrong; [`Self::ChatGptOAuth`] is the ChatGPT
/// plan, whose bearer token is fetched, and refreshed when it is about to
/// expire, for every request ([`crate::Credential::ChatGpt`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialKind {
    /// An API key, kept in the keychain slot `<provider id>_api_key`.
    ApiKey,
    /// Nothing: a local provider.
    None,
    /// Sign in with ChatGPT: an OAuth session ([`crate::chatgpt`]), not a key.
    ChatGptOAuth,
}

/// Provider-level error surfaced from `parse_stream_chunk`. We use a
/// thin `String` wrapper rather than re-exporting `crate::Error` to
/// avoid making the trait depend on the agent loop's error machinery.
#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("provider stream parse error: {0}")]
    Parse(String),
}

/// How a provider's one-shot requests are shaped and its replies read.
/// Streaming has its own per-provider hooks
/// ([`LlmProvider::serialize_request`], [`LlmProvider::parse_stream_chunk`]);
/// this is the other half, used by the classifier and the plan request.
/// Those are single JSON replies on the first two formats and a stream on
/// [`WireFormat::Responses`], which takes nothing else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WireFormat {
    /// Anthropic's Messages API (also OpenRouter's passthrough): a
    /// top-level `system`, replies in `content[].text`.
    AnthropicMessages,
    /// OpenAI's chat-completions API, which Groq, Gemini and Ollama
    /// also speak: the system prompt is the first message, replies in
    /// `choices[0].message.content`.
    ChatCompletions,
    /// OpenAI's Responses API ([`crate::responses`]): the system prompt is
    /// `instructions`, the conversation an array of `input` items, and the
    /// reply a stream of typed events that succeeds only at
    /// `response.completed`. It has no non-streaming mode here, so one-shot
    /// requests (the classifier, the plan) are streamed and read from the
    /// stream. The ChatGPT plan provider speaks it ([`ChatGptProvider`]).
    Responses,
}

impl WireFormat {
    /// Whether a stream on this format has an end marker of its own that a
    /// step must reach, so that running out of events without it is an error
    /// and not a shorter reply.
    ///
    /// Only the Responses format does: its turn succeeds at
    /// `response.completed` and nowhere else. The other two have always taken
    /// the end of the stream as the end of the step, and keep doing so.
    pub fn needs_terminal_event(self) -> bool {
        self == WireFormat::Responses
    }
}

/// How much of the tool list a provider is sent with each request (#395).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolSet {
    /// Every tool that is permitted this turn, with its full description
    /// and schema, in the order the dispatcher lists them.
    Full,
    /// A core of common edits plus the tools the conversation points at,
    /// with shortened descriptions, within a fixed size
    /// ([`crate::tool_selection`]). For a model whose context window is
    /// small enough that the whole list does not fit.
    Slim,
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
    ///
    /// Never `Content-Type`: every body goes out with `.json()`, which sets
    /// it once, and `RequestBuilder::header` appends, so a second one is sent
    /// as `application/json, application/json`. Most APIs read past that;
    /// the ChatGPT plan's route answers 400 "Unsupported content type".
    /// `every_request_carries_one_content_type` holds it.
    fn apply_auth(&self, req: reqwest::RequestBuilder, api_key: &str) -> reqwest::RequestBuilder;

    /// Endpoint path appended to [`base_url`]. Defaults to Anthropic's
    /// `/v1/messages`; OpenAI overrides to `/v1/chat/completions`.
    fn endpoint_path(&self) -> &str {
        "/v1/messages"
    }

    /// The shape of this provider's one-shot requests and replies.
    /// Anthropic's, unless a provider speaks chat-completions — which
    /// every provider that does must say, or its classifier and plan
    /// requests go out in a shape its server ignores (#399).
    fn wire_format(&self) -> WireFormat {
        WireFormat::AnthropicMessages
    }

    /// What this provider authenticates with. [`CredentialKind::ApiKey`]
    /// unless a provider says otherwise, so a provider added later is
    /// keyed until someone decides it is not.
    fn credential_kind(&self) -> CredentialKind {
        CredentialKind::ApiKey
    }

    /// Whether this provider needs an API key to be usable.
    ///
    /// Derived from [`Self::credential_kind`]: every hosted provider
    /// takes a key, so no existing implementation changes. A local daemon
    /// does not, and before this existed there was no way to say so: the
    /// agent was only constructed when a key was present, saving a blank
    /// key was rejected, and first launch blocked behind a key prompt.
    /// Those checks now ask the provider instead of assuming. The ChatGPT
    /// plan has no key either; it has a session.
    fn requires_api_key(&self) -> bool {
        self.credential_kind() == CredentialKind::ApiKey
    }

    /// Whether a base URL the user (or a stored setting) names replaces
    /// [`Self::base_url`]. Yes, unless the provider holds a credential that
    /// must reach one host only: the ChatGPT plan's bearer token goes to
    /// `api.openai.com` and nowhere else, so that provider says no and
    /// [`crate::LlmConfig::base_url`] then ignores any override.
    fn honours_base_url_override(&self) -> bool {
        true
    }

    /// Whether the per-turn mode classifier (a model call of its own, before
    /// the request the user asked for) is skipped, and every request taken
    /// as [`general`](crate::agent_loop). For a provider where each call is
    /// spent from the user's own allowance rather than billed per token.
    fn skips_classifier(&self) -> bool {
        false
    }

    /// Whether the model's thinking is kept in the history, within a turn, to
    /// be replayed with the tool results that follow it.
    ///
    /// Whatever the provider issues with effort (Anthropic's `thinking`
    /// blocks) by default. A provider that returns reasoning without an
    /// effort setting (the Responses API's `reasoning` items) says so here.
    /// Either way [`crate::agent_loop`] drops it all at the start of the next
    /// turn, because it is bound to the `system` and `tools` it was produced
    /// under.
    fn replays_thinking(&self) -> bool {
        self.supports_effort()
    }

    /// Name a failure better than the generic errors do, or `None` to leave
    /// it to them. See [`ProviderFailure`]. The default knows nothing.
    fn classify_failure(&self, _failure: &ProviderFailure<'_>) -> Option<crate::Error> {
        None
    }

    /// After the server refused a request: change something in this provider
    /// so that building the request again gives a different one, and say what.
    /// `None` leaves the failure to [`Self::classify_failure`]. A provider
    /// must never answer `Some` twice for what is the same request, or the
    /// loop would send one body over and over: each answer moves it to a
    /// state it was not in, and it runs out. The default never adapts.
    fn adapt_after_refusal(&self, _failure: &ProviderFailure<'_>) -> Option<Adaptation> {
        None
    }

    /// Whether a failure that arrives in a stream, before anything of the
    /// response has been shown, is tried again once with the same request.
    /// For a provider where each try spends from an allowance: only the codes
    /// that say the service, not the request, was at fault.
    fn retries_stream_failure(&self, _code: &str) -> bool {
        false
    }

    /// How long to wait before that second try.
    fn stream_retry_delay(&self) -> std::time::Duration {
        std::time::Duration::ZERO
    }

    /// Whether a `401` with this body is answered by renewing the credential
    /// and sending the request again (once). Yes unless the body says the
    /// credential was not the problem.
    fn renews_on_401(&self, _body: &str) -> bool {
        true
    }

    /// The reasoning effort to send for `model`, given the one the person
    /// chose: the same, unless the provider knows the model does not take it.
    fn effort_for_model(
        &self,
        _model: &str,
        effort: Option<crate::Effort>,
    ) -> Option<crate::Effort> {
        effort
    }

    /// Told the models a catalogue fetch returned, for a provider whose
    /// requests depend on what a model supports.
    fn note_catalogue(&self, _models: &[crate::ModelInfo]) {}

    /// Whether the endpoint accepts `output_config.effort`, and so
    /// whether the reasoning-effort setting applies.
    ///
    /// Anthropic's own API does. OpenRouter carries the same wire shape
    /// but is a gateway to models that may not know the field, and the
    /// chat-completions servers would reject it as an unknown one, so
    /// the default is `false` and a provider has to opt in. The same
    /// answer governs whether the model's `thinking` blocks are kept in
    /// the history: they come with effort and carry a signature only
    /// the server that issued them will take back.
    fn supports_effort(&self) -> bool {
        false
    }

    /// How much of the tool list each request carries.
    ///
    /// [`ToolSet::Full`] unless a provider says otherwise, and a provider
    /// should only say [`ToolSet::Slim`] for models with a small context
    /// window, which is what local models usually run with. Anthropic
    /// must stay `Full`: its prompt cache keys on the whole tools-plus-
    /// system prefix, so a tool list that varied with the message would
    /// turn every message into a cache miss.
    fn tool_set(&self) -> ToolSet {
        ToolSet::Full
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
    fn supports_effort(&self) -> bool {
        true
    }
    fn apply_auth(&self, req: reqwest::RequestBuilder, api_key: &str) -> reqwest::RequestBuilder {
        req.header("x-api-key", api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
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
        OPENROUTER_DEFAULT_MODEL
    }
    fn classifier_model(&self) -> &str {
        OPENROUTER_CLASSIFIER_MODEL
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

impl OpenAIProvider {
    /// The chat-completions body for `req`, for the provider `provider_id`
    /// that is serving it: OpenAI itself, or Groq, Gemini or Ollama, which
    /// delegate here and say who they are.
    ///
    /// The caller's `max_tokens` is lowered to the output limit of the model
    /// if [`crate::models::max_output_tokens`] knows one for that provider.
    /// OpenAI is reported to reject a request above a model's limit rather
    /// than lower it (see [`crate::prompt::DEFAULT_MAX_TOKENS`]), and the agent
    /// asks every provider for the same number. This
    /// is the one serializer behind every streamed step and the Settings
    /// probe, so no caller has to remember to clamp.
    fn serialize_chat_completions(&self, provider_id: &str, req: &MessagesRequest<'_>) -> Value {
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
        // with `o1`/`o3`. Ollama is the exception (see
        // [`output_limit_field`]).
        if let Some(obj) = body.as_object_mut() {
            obj.insert(
                output_limit_field(provider_id).to_string(),
                Value::from(crate::models::clamp_max_tokens(
                    provider_id,
                    req.model,
                    req.max_tokens,
                )),
            );
            // Only OpenAI's own endpoint: Groq, Gemini and Ollama serialize
            // through here too and would not know the field.
            if provider_id == OPENAI_ID {
                if let Some(effort) = openai_reasoning_effort(req.model) {
                    obj.insert("reasoning_effort".to_string(), Value::from(effort));
                }
            }
            if let Some(t) = tools {
                obj.insert("tools".to_string(), t);
                // `tool_choice: "auto"` is OpenAI's shape, and so is
                // `"none"`, which the loop asks for on the one request
                // after the tool budget is spent. We only set it when
                // tools are actually present (OpenAI rejects
                // `tool_choice` without a `tools` array).
                let choice = if req.tool_choice == Some(ToolChoice::NONE) {
                    "none"
                } else {
                    "auto"
                };
                obj.insert("tool_choice".to_string(), Value::from(choice));
            }
        }

        body
    }
}

impl LlmProvider for OpenAIProvider {
    fn wire_format(&self) -> WireFormat {
        WireFormat::ChatCompletions
    }
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
    }
    fn endpoint_path(&self) -> &str {
        "/v1/chat/completions"
    }
    fn serialize_request(&self, req: &MessagesRequest<'_>) -> Value {
        self.serialize_chat_completions(OPENAI_ID, req)
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
                error: ApiError {
                    kind,
                    message,
                    param: None,
                },
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
                        // Thinking is the assistant's, and OpenAI has
                        // nowhere to put it.
                        ContentBlock::Thinking { .. } | ContentBlock::RedactedThinking { .. } => {}
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
                        // Chat completions has no thinking block, and a
                        // signature only Anthropic can verify is of no
                        // use to it.
                        ContentBlock::Thinking { .. } | ContentBlock::RedactedThinking { .. } => {}
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
    fn wire_format(&self) -> WireFormat {
        WireFormat::ChatCompletions
    }
    fn id(&self) -> &'static str {
        GROQ_ID
    }
    fn base_url(&self) -> &str {
        GROQ_DEFAULT_BASE_URL
    }
    fn default_model(&self) -> &str {
        GROQ_DEFAULT_MODEL
    }
    fn classifier_model(&self) -> &str {
        GROQ_CLASSIFIER_MODEL
    }
    fn translate_model(&self, model: &str) -> String {
        model.to_string()
    }
    fn apply_auth(&self, req: reqwest::RequestBuilder, api_key: &str) -> reqwest::RequestBuilder {
        req.header("authorization", format!("Bearer {api_key}"))
    }
    fn endpoint_path(&self) -> &str {
        "/v1/chat/completions"
    }
    fn serialize_request(&self, req: &MessagesRequest<'_>) -> Value {
        self.inner.serialize_chat_completions(GROQ_ID, req)
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
    fn wire_format(&self) -> WireFormat {
        WireFormat::ChatCompletions
    }
    fn id(&self) -> &'static str {
        GEMINI_ID
    }
    fn base_url(&self) -> &str {
        GEMINI_DEFAULT_BASE_URL
    }
    fn default_model(&self) -> &str {
        GEMINI_DEFAULT_MODEL
    }
    fn classifier_model(&self) -> &str {
        GEMINI_CLASSIFIER_MODEL
    }
    fn translate_model(&self, model: &str) -> String {
        model.to_string()
    }
    fn apply_auth(&self, req: reqwest::RequestBuilder, api_key: &str) -> reqwest::RequestBuilder {
        req.header("authorization", format!("Bearer {api_key}"))
    }
    fn endpoint_path(&self) -> &str {
        "/chat/completions"
    }
    fn list_models_path(&self) -> &str {
        "/models"
    }
    fn serialize_request(&self, req: &MessagesRequest<'_>) -> Value {
        self.inner.serialize_chat_completions(GEMINI_ID, req)
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
    fn wire_format(&self) -> WireFormat {
        WireFormat::ChatCompletions
    }
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
        req
    }
    fn credential_kind(&self) -> CredentialKind {
        CredentialKind::None
    }
    fn tool_set(&self) -> ToolSet {
        // Local models usually run with a 4k to 8k context, and the whole
        // tool list alone is several times that (#395).
        ToolSet::Slim
    }
    fn endpoint_path(&self) -> &str {
        "/chat/completions"
    }
    fn list_models_path(&self) -> &str {
        // The base URL already carries `/v1`.
        "/models"
    }
    fn serialize_request(&self, req: &MessagesRequest<'_>) -> Value {
        self.inner.serialize_chat_completions(OLLAMA_ID, req)
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
        CHATGPT_ID => Arc::new(ChatGptProvider::default()),
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
    CHATGPT_ID,
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anthropic::{Message, SystemBlock};

    #[test]
    fn anthropic_translate_model_is_identity() {
        let p = AnthropicProvider;
        assert_eq!(p.translate_model("claude-sonnet-5-5"), "claude-sonnet-5-5");
    }

    #[test]
    fn openrouter_translate_model_prepends_anthropic_namespace() {
        let p = OpenRouterProvider;
        assert_eq!(
            p.translate_model("claude-sonnet-5-5"),
            "anthropic/claude-sonnet-5-5"
        );
    }

    #[test]
    fn openrouter_translate_model_passes_through_already_qualified_ids() {
        let p = OpenRouterProvider;
        assert_eq!(
            p.translate_model("anthropic/claude-sonnet-5.5"),
            "anthropic/claude-sonnet-5.5"
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

    /// A chat request is `post(url).json(body)` and then `apply_auth`
    /// (`LlmConfig::post_json`); the probe is `apply_auth` and then `.json`.
    /// Either way exactly one `Content-Type` goes out. Two (one from `.json`,
    /// one appended by `apply_auth`) were sent by every provider, and the
    /// ChatGPT plan's route refused them: 400 `{"detail":"Unsupported content
    /// type"}`.
    #[test]
    fn every_request_carries_one_content_type() {
        let body = serde_json::json!({ "model": "m" });
        for id in SUPPORTED_PROVIDER_IDS {
            let p = provider_from_id(id);
            let url = format!("https://example.invalid{}", p.endpoint_path());
            let client = reqwest::Client::new();
            let chat = p
                .apply_auth(client.post(&url).json(&body), "k")
                .build()
                .expect("request builds");
            let probe = p
                .apply_auth(client.post(&url), "k")
                .json(&body)
                .build()
                .expect("request builds");
            for (how, req) in [("chat", &chat), ("probe", &probe)] {
                let values: Vec<_> = req.headers().get_all("content-type").iter().collect();
                assert_eq!(values, ["application/json"], "{id} {how}: {values:?}");
            }
            let models = p
                .apply_auth(
                    client.get(format!("https://example.invalid{}", p.list_models_path())),
                    "k",
                )
                .build()
                .expect("request builds");
            assert!(
                models.headers().get("content-type").is_none(),
                "{id}: a GET has no body, so no Content-Type"
            );
        }
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
        assert_eq!(p.translate_model("gemini-3.8-flash"), "gemini-3.8-flash");
    }

    #[test]
    fn provider_from_id_returns_groq_and_gemini() {
        assert_eq!(provider_from_id("groq").id(), "groq");
        assert_eq!(provider_from_id("gemini").id(), "gemini");
    }

    /// Only Ollama is sent the smaller tool set (#395).
    ///
    /// Anthropic must not be: its prompt cache keys on the whole tool
    /// list, so a list that varied with the message would turn every
    /// message into a cache miss. Hosted providers take the whole list,
    /// and changing any of them is a decision to make on its own, so a
    /// provider added later is `Full` until someone says otherwise.
    #[test]
    fn only_ollama_sends_the_slim_tool_set() {
        for id in SUPPORTED_PROVIDER_IDS {
            let provider = provider_from_id(id);
            assert_eq!(provider.id(), *id, "provider_from_id fell back for {id}");
            let expected = if *id == OLLAMA_ID {
                ToolSet::Slim
            } else {
                ToolSet::Full
            };
            assert_eq!(provider.tool_set(), expected, "for {id}");
        }
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
            model: "claude-sonnet-5-5",
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
            output_config: None,
        };
        let body = AnthropicProvider.serialize_request(&req);
        assert_eq!(body["model"], "claude-sonnet-5-5");
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
            output_config: None,
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

    /// The last request of a turn that spent its tool budget asks the
    /// model not to call a tool, in each wire format (#439). The tools
    /// stay in the request: the conversation holds tool calls, and
    /// Anthropic rejects that without tool definitions.
    #[test]
    fn a_tool_less_request_says_so_in_both_wire_formats() {
        let msgs = vec![Message {
            role: Role::User,
            content: vec![ContentBlock::Text {
                text: "hello".into(),
            }],
        }];
        let req = MessagesRequest {
            model: "m",
            max_tokens: 16,
            system: vec![SystemBlock {
                kind: "text",
                text: "s",
                cache_control: None,
            }],
            messages: &msgs,
            tools: Some(json!([{
                "name": "gain",
                "description": "Gain",
                "input_schema": {"type": "object", "properties": {}}
            }])),
            tool_choice: Some(ToolChoice::NONE),
            stream: true,
            output_config: None,
        };

        let anthropic = AnthropicProvider.serialize_request(&req);
        assert_eq!(anthropic["tool_choice"], json!({ "type": "none" }));
        assert_eq!(anthropic["tools"][0]["name"], "gain");

        let openai = OpenAIProvider::default().serialize_request(&req);
        assert_eq!(openai["tool_choice"], "none");
        assert_eq!(openai["tools"][0]["function"]["name"], "gain");
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

    // ------------------------------------------------------------------
    // Reasoning effort
    // ------------------------------------------------------------------

    fn effort_request<'a>(
        msgs: &'a [Message],
        output_config: Option<crate::anthropic::OutputConfig>,
    ) -> MessagesRequest<'a> {
        MessagesRequest {
            model: "claude-sonnet-5-5",
            max_tokens: 16,
            system: vec![SystemBlock {
                kind: "text",
                text: "be helpful",
                cache_control: None,
            }],
            messages: msgs,
            tools: None,
            tool_choice: None,
            stream: true,
            output_config,
        }
    }

    fn one_user_message() -> Vec<Message> {
        vec![Message {
            role: Role::User,
            content: vec![ContentBlock::Text { text: "hi".into() }],
        }]
    }

    #[test]
    fn anthropic_sends_effort_as_output_config_with_the_exact_shape() {
        use crate::anthropic::{Effort, OutputConfig};
        let msgs = one_user_message();
        for effort in Effort::ALL {
            let req = effort_request(&msgs, Some(OutputConfig::effort(effort)));
            let body = AnthropicProvider.serialize_request(&req);
            assert_eq!(
                body["output_config"],
                json!({ "effort": effort.as_str() }),
                "{effort:?}"
            );
            assert!(body.get("effort").is_none(), "no top-level effort");
        }
    }

    #[test]
    fn anthropic_body_with_no_effort_has_no_output_config() {
        let msgs = one_user_message();
        let body = AnthropicProvider.serialize_request(&effort_request(&msgs, None));
        assert!(body.get("output_config").is_none());
        assert_eq!(
            body,
            json!({
                "model": "claude-sonnet-5-5",
                "max_tokens": 16,
                "system": [{ "type": "text", "text": "be helpful" }],
                "messages": [{ "role": "user", "content": [{ "type": "text", "text": "hi" }] }],
                "stream": true
            })
        );
    }

    /// Only Anthropic's own endpoint and the ChatGPT plan's take an effort.
    /// OpenRouter shares Anthropic's wire shape but fronts models that may
    /// not, and the chat-completions servers reject an unknown field
    /// outright. The plan's is the Responses API's own `reasoning.effort`,
    /// held to the levels a model lists.
    #[test]
    fn only_anthropic_and_the_chatgpt_plan_support_effort() {
        for id in SUPPORTED_PROVIDER_IDS {
            assert_eq!(
                provider_from_id(id).supports_effort(),
                *id == ANTHROPIC_ID || *id == CHATGPT_ID,
                "{id}"
            );
        }
    }

    /// The chat-completions providers build their bodies from scratch,
    /// so even a request that somehow carried an effort cannot put one
    /// on their wire.
    #[test]
    fn chat_completions_bodies_never_carry_an_effort() {
        use crate::anthropic::{Effort, OutputConfig};
        let msgs = one_user_message();
        let req = effort_request(&msgs, Some(OutputConfig::effort(Effort::Max)));
        for id in [OPENAI_ID, GROQ_ID, GEMINI_ID, OLLAMA_ID] {
            let text = provider_from_id(id).serialize_request(&req).to_string();
            assert!(!text.contains("output_config"), "{id}: {text}");
            assert!(!text.contains("effort"), "{id}: {text}");
        }
    }

    /// `LlmConfig::effective_effort` is what every builder reads, so a
    /// level chosen under Anthropic and left behind after a switch
    /// cannot follow the user to a server that would reject it.
    #[test]
    fn effective_effort_is_dropped_for_every_provider_that_cannot_take_it() {
        use crate::anthropic::Effort;
        for id in SUPPORTED_PROVIDER_IDS {
            let cfg = crate::LlmConfig::new(provider_from_id(id), "k").with_effort(Effort::High);
            let expected = (*id == ANTHROPIC_ID || *id == CHATGPT_ID).then_some(Effort::High);
            assert_eq!(cfg.effective_effort(), expected, "{id}");
        }
        let unset = crate::LlmConfig::new_anthropic("k");
        assert_eq!(unset.effective_effort(), None);
    }

    // ------------------------------------------------------------------
    // Stream parsing: what a higher effort streams
    // ------------------------------------------------------------------

    fn parse_all(p: &dyn LlmProvider, chunks: &[&str]) -> Vec<StreamEvent> {
        chunks
            .iter()
            .flat_map(|c| {
                p.parse_stream_chunk(c)
                    .unwrap_or_else(|e| panic!("chunk must parse: {c}: {e}"))
            })
            .collect()
    }

    /// The stream from a real `max`-effort tool-use exchange, reduced:
    /// a `thinking` block with its deltas, then text, then a tool call
    /// carrying the `caller` field the parser has no use for.
    #[test]
    fn thinking_blocks_and_their_deltas_parse() {
        let events = parse_all(
            &AnthropicProvider,
            &[
                r#"{"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"","signature":""}}"#,
                r#"{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"let me see"}}"#,
                r#"{"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"EqQB"}}"#,
                r#"{"type":"content_block_stop","index":0}"#,
                r#"{"type":"content_block_start","index":1,"content_block":{"type":"redacted_thinking","data":"OPAQUE"}}"#,
                r#"{"type":"content_block_start","index":2,"content_block":{"type":"tool_use","id":"toolu_1","name":"get_bpm","input":{},"caller":{"type":"direct"}}}"#,
            ],
        );
        assert!(matches!(
            &events[0],
            StreamEvent::ContentBlockStart {
                content_block: ContentBlockStart::Thinking { thinking, signature },
                ..
            } if thinking.is_empty() && signature.is_empty()
        ));
        assert!(matches!(
            &events[1],
            StreamEvent::ContentBlockDelta {
                delta: ContentBlockDelta::ThinkingDelta { thinking },
                ..
            } if thinking == "let me see"
        ));
        assert!(matches!(
            &events[2],
            StreamEvent::ContentBlockDelta {
                delta: ContentBlockDelta::SignatureDelta { signature },
                ..
            } if signature == "EqQB"
        ));
        assert!(matches!(
            &events[4],
            StreamEvent::ContentBlockStart {
                content_block: ContentBlockStart::RedactedThinking { data },
                ..
            } if data == "OPAQUE"
        ));
        assert!(matches!(
            &events[5],
            StreamEvent::ContentBlockStart {
                content_block: ContentBlockStart::ToolUse { name, .. },
                ..
            } if name == "get_bpm"
        ));
    }

    /// A block type, delta type or event type this client has never
    /// heard of is skipped, not an error: an error here ends the whole
    /// turn.
    #[test]
    fn unknown_block_delta_and_event_types_are_skipped_not_errors() {
        let events = parse_all(
            &AnthropicProvider,
            &[
                r#"{"type":"content_block_start","index":0,"content_block":{"type":"server_tool_use","id":"srv_1","name":"web_search","input":{}}}"#,
                r#"{"type":"content_block_start","index":1,"content_block":{"type":"web_search_tool_result","tool_use_id":"srv_1","content":[{"type":"web_search_result","url":"https://example.com"}]}}"#,
                r#"{"type":"content_block_start","index":2,"content_block":{"type":"some_future_block","anything":[1,2,3]}}"#,
                r#"{"type":"content_block_delta","index":2,"delta":{"type":"citations_delta","citation":{"cited_text":"x"}}}"#,
                r#"{"type":"content_block_delta","index":2,"delta":{"type":"some_future_delta","payload":{"a":1}}}"#,
                r#"{"type":"some_future_event","index":2}"#,
                r#"{"type":"ping"}"#,
            ],
        );
        assert!(matches!(
            &events[0],
            StreamEvent::ContentBlockStart {
                content_block: ContentBlockStart::Other,
                ..
            }
        ));
        assert!(matches!(
            &events[1],
            StreamEvent::ContentBlockStart {
                content_block: ContentBlockStart::Other,
                ..
            }
        ));
        assert!(matches!(
            &events[2],
            StreamEvent::ContentBlockStart {
                content_block: ContentBlockStart::Other,
                ..
            }
        ));
        assert!(matches!(
            &events[3],
            StreamEvent::ContentBlockDelta {
                delta: ContentBlockDelta::Other,
                ..
            }
        ));
        assert!(matches!(
            &events[4],
            StreamEvent::ContentBlockDelta {
                delta: ContentBlockDelta::Other,
                ..
            }
        ));
        assert!(matches!(&events[5], StreamEvent::Other));
        assert!(matches!(&events[6], StreamEvent::Ping));
    }

    /// A thinking block missing a field the server usually sends still
    /// parses; every one defaults.
    #[test]
    fn a_thinking_block_missing_fields_still_parses() {
        let events = parse_all(
            &AnthropicProvider,
            &[
                r#"{"type":"content_block_start","index":0,"content_block":{"type":"thinking"}}"#,
                r#"{"type":"content_block_start","index":1,"content_block":{"type":"redacted_thinking"}}"#,
                r#"{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta"}}"#,
                r#"{"type":"content_block_delta","index":0,"delta":{"type":"signature_delta"}}"#,
            ],
        );
        assert_eq!(events.len(), 4);
    }

    /// History holding thinking (from an Anthropic turn) must not break
    /// a chat-completions request: those servers have nowhere to put it.
    #[test]
    fn chat_completions_translation_skips_thinking_blocks() {
        let msgs = vec![
            Message {
                role: Role::User,
                content: vec![ContentBlock::Text { text: "hi".into() }],
            },
            Message {
                role: Role::Assistant,
                content: vec![
                    ContentBlock::Thinking {
                        thinking: "private".into(),
                        signature: "SIG".into(),
                    },
                    ContentBlock::RedactedThinking {
                        data: "DATA".into(),
                    },
                    ContentBlock::Text {
                        text: "hello".into(),
                    },
                ],
            },
        ];
        let req = effort_request(&msgs, None);
        let text = OpenAIProvider::default()
            .serialize_request(&req)
            .to_string();
        assert!(text.contains("hello"));
        assert!(!text.contains("private"), "{text}");
        assert!(!text.contains("SIG"), "{text}");
        assert!(!text.contains("DATA"), "{text}");
    }

    // ------------------------------------------------------------------
    // Per-model output limit
    // ------------------------------------------------------------------

    /// A streamed step as the loop builds it: `max_tokens` on `model`, with
    /// tools attached.
    fn step_for<'a>(model: &'a str, msgs: &'a [Message], max_tokens: u32) -> MessagesRequest<'a> {
        MessagesRequest {
            model,
            max_tokens,
            system: vec![SystemBlock {
                kind: "text",
                text: "sys",
                cache_control: None,
            }],
            messages: msgs,
            tools: Some(json!([{
                "name": "t",
                "description": "d",
                "input_schema": {"type": "object"}
            }])),
            tool_choice: Some(ToolChoice::AUTO),
            stream: true,
            output_config: None,
        }
    }

    /// The defect: the 8192 every step asks for went to OpenAI's older
    /// models, which are reported to reject a request above their limit.
    /// Each of them now carries the limit OpenAI's page gives.
    #[test]
    fn an_openai_request_for_a_legacy_model_carries_its_output_limit() {
        let msgs = one_user_message();
        for (model, limit) in [
            ("gpt-3.5-turbo", 4_096),
            ("gpt-3.5-turbo-0125", 4_096),
            ("gpt-4-turbo", 4_096),
            ("gpt-4-turbo-2024-04-09", 4_096),
            ("gpt-4-turbo-preview", 4_096),
            ("gpt-4-0125-preview", 4_096),
            ("gpt-4-1106-preview", 4_096),
            ("gpt-4", 8_192),
            ("gpt-4-0613", 8_192),
        ] {
            let req = step_for(model, &msgs, crate::prompt::DEFAULT_MAX_TOKENS);
            let body = OpenAIProvider::default().serialize_request(&req);
            assert_eq!(body["max_completion_tokens"], limit, "{model}");
            assert!(body.get("max_tokens").is_none(), "{model}");
        }
    }

    /// The clamp is for the models that need it. A current model keeps the
    /// 8192, and so does a model nobody has a limit for.
    #[test]
    fn an_openai_request_for_a_current_or_unknown_model_keeps_the_cap() {
        let msgs = one_user_message();
        for model in [
            "gpt-4o-mini",
            "gpt-4o",
            "gpt-4.1",
            "o3",
            "gpt-5",
            "gpt-4o-2024-05-13",
            "a-model-nobody-has-heard-of",
        ] {
            let req = step_for(model, &msgs, crate::prompt::DEFAULT_MAX_TOKENS);
            let body = OpenAIProvider::default().serialize_request(&req);
            assert_eq!(body["max_completion_tokens"], 8192, "{model}");
        }
    }

    /// It lowers and does not raise: the Settings probe asks for 256 and
    /// has to get 256, even on a model whose limit is far above it.
    #[test]
    fn a_request_below_the_limit_is_left_as_it_is() {
        let msgs = one_user_message();
        for model in ["gpt-4-turbo", "gpt-3.5-turbo", "gpt-4o-mini"] {
            let body = OpenAIProvider::default().serialize_request(&step_for(model, &msgs, 256));
            assert_eq!(body["max_completion_tokens"], 256, "{model}");
        }
    }

    /// Groq, Gemini and Ollama serialize through OpenAI's, and each says
    /// who it is, so each gets its own provider's limits and not OpenAI's.
    #[test]
    fn the_wrappers_use_their_own_providers_limits() {
        let msgs = one_user_message();
        let cap = crate::prompt::DEFAULT_MAX_TOKENS;

        // Their own documented limits are applied.
        let groq = GroqProvider::default().serialize_request(&step_for(
            "meta-llama/llama-prompt-guard-2-22m",
            &msgs,
            cap,
        ));
        assert_eq!(groq["max_completion_tokens"], 512);
        let gemini = GeminiProvider::default().serialize_request(&step_for(
            "gemini-3.1-flash-lite-image",
            &msgs,
            cap,
        ));
        assert_eq!(gemini["max_completion_tokens"], 4_096);

        // OpenAI's are not: the same ids on another provider are not
        // OpenAI's models.
        for id in [GROQ_ID, GEMINI_ID, OLLAMA_ID] {
            let body = provider_from_id(id).serialize_request(&step_for("gpt-4-turbo", &msgs, cap));
            assert_eq!(output_cap(&body), 8192, "{id}");
        }
        // And the defaults of each stay at the cap.
        for (id, model) in [
            (GROQ_ID, "llama-3.3-70b-versatile"),
            (GEMINI_ID, "gemini-3.8-flash"),
            (GEMINI_ID, "gemini-3.5-flash-lite"),
            (OLLAMA_ID, "llama3.2"),
        ] {
            let body = provider_from_id(id).serialize_request(&step_for(model, &msgs, cap));
            assert_eq!(output_cap(&body), 8192, "{id} {model}");
        }
    }

    /// The output cap of a chat-completions body, under whichever field the
    /// provider reads it from.
    fn output_cap(body: &Value) -> &Value {
        body.get("max_completion_tokens")
            .or_else(|| body.get("max_tokens"))
            .expect("the body carries an output cap")
    }

    /// Ollama's OpenAI-compatible API lists `max_tokens` among the request
    /// fields it supports and not `max_completion_tokens`
    /// (<https://docs.ollama.com/api/openai-compatibility>). A field the server
    /// does not read leaves the step uncapped, and a local model that runs on
    /// is the thing the cap exists to stop. The classifier and plan requests
    /// have always sent `max_tokens` to it; the streamed steps now do too.
    #[test]
    fn ollama_is_sent_the_output_cap_field_it_reads() {
        let msgs = one_user_message();
        let cap = crate::prompt::DEFAULT_MAX_TOKENS;

        let body = provider_from_id(OLLAMA_ID).serialize_request(&step_for("llama3.2", &msgs, cap));
        assert_eq!(body["max_tokens"], 8192, "{body}");
        assert!(body.get("max_completion_tokens").is_none(), "{body}");

        // The others keep the field they were sent.
        for id in [OPENAI_ID, GROQ_ID, GEMINI_ID] {
            let body = provider_from_id(id).serialize_request(&step_for("a-model", &msgs, cap));
            assert_eq!(body["max_completion_tokens"], 8192, "{id}");
            assert!(body.get("max_tokens").is_none(), "{id}");
        }
    }

    /// The Anthropic wire format is serialized as it is: its models allow
    /// far more than the cap.
    #[test]
    fn the_anthropic_wire_format_is_not_clamped() {
        let msgs = one_user_message();
        for provider in [
            provider_from_id(ANTHROPIC_ID),
            provider_from_id(OPENROUTER_ID),
        ] {
            let body = provider.serialize_request(&step_for("gpt-4-turbo", &msgs, 8192));
            assert_eq!(body["max_tokens"], 8192, "{}", provider.id());
        }
    }

    // ------------------------------------------------------------------
    // Current defaults
    // ------------------------------------------------------------------

    /// The model each provider uses when the user has not chosen one, and
    /// the one it classifies with. Each id was checked against its
    /// provider's own page, listed in the PR that set it; a new generation
    /// updates this test with the constant, which is the point of it.
    ///
    /// The ids are exact on purpose. OpenRouter's are dotted
    /// (`anthropic/claude-sonnet-5.5`, not `-5-5`), which its public
    /// `/api/v1/models` list and its docs both show.
    #[test]
    fn every_provider_defaults_to_a_current_model() {
        for (id, default, classifier) in [
            (ANTHROPIC_ID, "claude-sonnet-5-5", "claude-haiku-5-5"),
            (
                OPENROUTER_ID,
                "anthropic/claude-sonnet-5.5",
                "anthropic/claude-haiku-5.5",
            ),
            (OPENAI_ID, "gpt-6-luna", "gpt-6-luna"),
            (GROQ_ID, "llama-3.3-70b-versatile", "llama-3.1-8b-instant"),
            (GEMINI_ID, "gemini-3.8-flash", "gemini-3.5-flash-lite"),
            (OLLAMA_ID, "llama3.2", "llama3.2"),
            // Never asked (the classifier is skipped): the default's id.
            (CHATGPT_ID, "gpt-6.1-sol", "gpt-6.1-sol"),
        ] {
            let p = provider_from_id(id);
            assert_eq!(p.default_model(), default, "{id} default");
            assert_eq!(p.classifier_model(), classifier, "{id} classifier");
        }
        // Every provider is covered, so a new one cannot skip this.
        assert_eq!(SUPPORTED_PROVIDER_IDS.len(), 7);
    }

    /// OpenRouter's ids are already its own, so they go out as they are
    /// rather than being prefixed a second time.
    #[test]
    fn openrouter_sends_its_defaults_unchanged() {
        let p = OpenRouterProvider;
        assert_eq!(
            p.translate_model(p.default_model()),
            "anthropic/claude-sonnet-5.5"
        );
        assert_eq!(
            p.translate_model(p.classifier_model()),
            "anthropic/claude-haiku-5.5"
        );
    }

    /// `gpt-6-luna` "supports function calling only with `reasoning_effort`
    /// set to `none`" on Chat Completions
    /// (<https://developers.openai.com/api/docs/models/gpt-6-luna>), and
    /// every edit the agent makes is a tool call. The rule is for that model
    /// alone: `none` is not offered by every model, and a non-reasoning model
    /// does not take the field.
    #[test]
    fn luna_is_sent_reasoning_effort_none_and_nothing_else_is() {
        let msgs = one_user_message();
        let cap = crate::prompt::DEFAULT_MAX_TOKENS;

        let luna = OpenAIProvider::default().serialize_request(&step_for("gpt-6-luna", &msgs, cap));
        assert_eq!(luna["reasoning_effort"], "none", "{luna}");
        assert!(luna.get("tools").is_some(), "the step carries its tools");

        for model in ["gpt-6-astra", "gpt-6.1-sol", "gpt-4o-mini", "o3", "gpt-5"] {
            let body = OpenAIProvider::default().serialize_request(&step_for(model, &msgs, cap));
            assert!(body.get("reasoning_effort").is_none(), "{model}: {body}");
        }
        for id in [GROQ_ID, GEMINI_ID, OLLAMA_ID] {
            for model in ["llama-3.3-70b-versatile", "gemini-3.8-flash", "gpt-6-luna"] {
                let body = provider_from_id(id).serialize_request(&step_for(model, &msgs, cap));
                assert!(
                    body.get("reasoning_effort").is_none(),
                    "{id} {model}: only OpenAI's own endpoint is sent it: {body}"
                );
            }
        }
        // The Anthropic wire format has no such field.
        let body = AnthropicProvider.serialize_request(&step_for("gpt-6-luna", &msgs, cap));
        assert!(body.get("reasoning_effort").is_none());
    }

    #[test]
    fn openai_reasoning_effort_is_a_single_model_rule() {
        assert_eq!(openai_reasoning_effort("gpt-6-luna"), Some("none"));
        // A dated snapshot of it, should OpenAI publish one.
        assert_eq!(openai_reasoning_effort("gpt-6-luna-snapshot"), Some("none"));
        for model in ["gpt-6-lunar", "gpt-6", "gpt-6-astra", "gpt-4o-mini", ""] {
            assert_eq!(openai_reasoning_effort(model), None, "{model:?}");
        }
    }

    // ------------------------------------------------------------------
    // A model the provider no longer has
    // ------------------------------------------------------------------

    fn unavailable(provider_id: &str, status: u16, body: &str, wire_model: &str) -> bool {
        is_model_unavailable(
            provider_from_id(provider_id).as_ref(),
            status,
            body,
            wire_model,
        )
    }

    /// What a request naming a retired model is answered with, per wire
    /// format, and what is not that: any other failure must stay the plain
    /// API error it was, because "it may have been retired" is a claim.
    ///
    /// The Anthropic body is the real one: the answer to a `POST
    /// /v1/messages` naming `claude-opus-4-1-20250805`, retired on
    /// 2026-08-05 (<https://platform.claude.com/docs/en/about-claude/model-deprecations>),
    /// captured from the API on 2026-10-10. The other providers' bodies are
    /// illustrative: they are written from the documented fields
    /// (`error.code == "model_not_found"`, a 404 that names the model), not
    /// captured, and the rule keys on those fields and not on the wording.
    #[test]
    fn model_unavailable_is_recognised_per_wire_format() {
        const ANTHROPIC_RETIRED: &str = r#"{"type":"error","error":{"type":"not_found_error","message":"model: claude-opus-4-1-20250805"},"request_id":"req_011Cftywc6xsQRBrAxHK2RgM"}"#;
        let retired = "claude-opus-4-1-20250805";

        // Anthropic: a 404 `not_found_error` from POST /v1/messages names no
        // resource but the model.
        assert!(unavailable(ANTHROPIC_ID, 404, ANTHROPIC_RETIRED, retired));
        // A 404 that is not JSON is a wrong base path on a proxy.
        assert!(!unavailable(
            ANTHROPIC_ID,
            404,
            "<html><body>404 Not Found</body></html>",
            retired
        ));
        // A 400 can name the model for other reasons.
        let bad_request = r#"{"type":"error","error":{"type":"invalid_request_error","message":"'claude-opus-5-5' does not support this request"}}"#;
        assert!(!unavailable(
            ANTHROPIC_ID,
            400,
            bad_request,
            "claude-opus-5-5"
        ));
        // Other statuses are other problems, whatever they say.
        for (status, body) in [
            (
                401,
                r#"{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}"#,
            ),
            (
                429,
                r#"{"type":"error","error":{"type":"rate_limit_error","message":"slow down"}}"#,
            ),
            (
                500,
                r#"{"type":"error","error":{"type":"api_error","message":"Internal server error"}}"#,
            ),
            (
                529,
                r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#,
            ),
        ] {
            assert!(
                !unavailable(ANTHROPIC_ID, status, body, retired),
                "{status}"
            );
        }

        // OpenRouter speaks Anthropic's wire format and answers its own way
        // for a model it no longer serves: a 404 that names it.
        let openrouter_gone = r#"{"error":{"message":"No endpoints found for anthropic/claude-sonnet-4.","code":404}}"#;
        assert!(unavailable(
            OPENROUTER_ID,
            404,
            openrouter_gone,
            "anthropic/claude-sonnet-4"
        ));
        assert!(!unavailable(
            OPENROUTER_ID,
            404,
            openrouter_gone,
            "anthropic/claude-sonnet-5.5"
        ));

        // OpenAI and the providers that copy its API: `model_not_found`.
        let openai_gone = r#"{"error":{"message":"The model `gpt-4o-2024-05-13` does not exist or you do not have access to it.","type":"invalid_request_error","param":null,"code":"model_not_found"}}"#;
        assert!(unavailable(
            OPENAI_ID,
            404,
            openai_gone,
            "gpt-4o-2024-05-13"
        ));
        assert!(unavailable(
            GROQ_ID,
            404,
            openai_gone,
            "llama-3.3-70b-versatile"
        ));
        // The code decides, whatever the status.
        assert!(unavailable(
            OPENAI_ID,
            400,
            openai_gone,
            "gpt-4o-2024-05-13"
        ));
        // Gemini wraps its errors in an array, and has no such code: a 404
        // that names the model is the model.
        let gemini_gone = r#"[{"error":{"code":404,"message":"models/gemini-2.0-flash is not found for API version v1beta, or is not supported for generateContent.","status":"NOT_FOUND"}}]"#;
        assert!(unavailable(GEMINI_ID, 404, gemini_gone, "gemini-2.0-flash"));
        assert!(!unavailable(
            GEMINI_ID,
            404,
            gemini_gone,
            "gemini-3.8-flash"
        ));
        // A 400 that names the model is not that.
        let gemini_bad = r#"[{"error":{"code":400,"message":"gemini-3.8-flash: unsupported field","status":"INVALID_ARGUMENT"}}]"#;
        assert!(!unavailable(GEMINI_ID, 400, gemini_bad, "gemini-3.8-flash"));
        // Not found for another reason, with no model in it.
        let other_404 = r#"{"error":{"message":"Unknown request URL: POST /v1/chat/completionz","type":"invalid_request_error"}}"#;
        assert!(!unavailable(OPENAI_ID, 404, other_404, "gpt-6-luna"));
        // A key problem and a server fault are not it.
        let bad_key = r#"{"error":{"message":"Incorrect API key provided","type":"invalid_request_error","code":"invalid_api_key"}}"#;
        assert!(!unavailable(OPENAI_ID, 401, bad_key, "gpt-6-luna"));
        assert!(!unavailable(
            OPENAI_ID,
            500,
            "upstream fell over",
            "gpt-6-luna"
        ));

        // Ollama says it in its own words, and they are the useful ones:
        // "try pulling it first" is the fix, "retired" would be wrong.
        let ollama_missing = r#"{"error":{"message":"model \"llama3.2\" not found, try pulling it first","type":"api_error","param":null,"code":null}}"#;
        assert!(!unavailable(OLLAMA_ID, 404, ollama_missing, "llama3.2"));
        assert!(!unavailable(OLLAMA_ID, 404, openai_gone, "llama3.2"));
    }
}
