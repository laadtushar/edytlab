//! Provider abstraction for the LLM transport layer.
//!
//! This crate originally hardcoded Anthropic's Messages API. To support
//! OpenRouter (and, later, other Anthropic-Messages-compatible gateways)
//! we extract the per-provider differences behind the [`LlmProvider`]
//! trait.
//!
//! Only the *transport* layer differs between providers — the agent
//! tool-calling loop, SSE parser, request body shape, and conversation
//! representation are unchanged. OpenRouter exposes an Anthropic-shaped
//! `/v1/messages` endpoint with the same SSE event names and JSON
//! payloads, so the only concrete deltas are:
//!
//! 1. Authentication headers (`x-api-key` vs `Authorization: Bearer`).
//! 2. Optional provider-specific headers (`HTTP-Referer`, `X-Title`).
//! 3. Model id translation (Anthropic uses bare ids like
//!    `claude-sonnet-4-6`, OpenRouter prefixes them with `anthropic/`).
//!
//! Anything else stays in the agent loop.

use std::fmt::Debug;
use std::sync::Arc;

/// Stable id for the Anthropic provider. Used as the keychain account
/// suffix and the value the frontend sends as the active provider.
pub const ANTHROPIC_ID: &str = "anthropic";

/// Stable id for the OpenRouter provider.
pub const OPENROUTER_ID: &str = "openrouter";

/// Default base URL for Anthropic's Messages API.
pub const ANTHROPIC_DEFAULT_BASE_URL: &str = "https://api.anthropic.com";

/// Default base URL for OpenRouter. OpenRouter mounts the
/// Anthropic-compatible Messages API under `/api/v1` (the trailing
/// `/v1/messages` is appended by the agent loop, matching Anthropic).
pub const OPENROUTER_DEFAULT_BASE_URL: &str = "https://openrouter.ai/api";

/// `anthropic-version` header value sent on every Anthropic call. Both
/// Anthropic and OpenRouter accept this; OpenRouter forwards it to
/// upstream Anthropic. Re-exported from [`crate::prompt::ANTHROPIC_VERSION`].
pub use crate::prompt::ANTHROPIC_VERSION;

/// Per-provider knobs the agent loop and validator need.
///
/// Implementations must be cheap to clone via `Arc`. The trait is
/// deliberately stateless w.r.t. credentials — the API key is passed
/// per-request to [`LlmProvider::apply_auth`] so the same provider
/// instance can be shared between callers without leaking the secret
/// into trait state.
pub trait LlmProvider: Send + Sync + Debug {
    /// Stable identifier (`"anthropic"` or `"openrouter"`). Used as the
    /// keychain slot suffix and the value the frontend sends as the
    /// "active provider".
    fn id(&self) -> &'static str;

    /// Default base URL (without trailing slash). The agent loop appends
    /// `/v1/messages`.
    fn base_url(&self) -> &str;

    /// Default model id when the user has not picked one.
    fn default_model(&self) -> &str;

    /// Cheap classifier model id.
    fn classifier_model(&self) -> &str;

    /// Translate a model id from the canonical Anthropic shape into the
    /// shape the underlying provider expects. For Anthropic this is the
    /// identity; for OpenRouter we prepend `anthropic/` if the id is not
    /// already prefixed.
    fn translate_model(&self, model: &str) -> String;

    /// Apply authentication + provider-specific headers to a request
    /// builder. The agent loop calls this just before `.json(&body)
    /// .send()`.
    fn apply_auth(&self, req: reqwest::RequestBuilder, api_key: &str) -> reqwest::RequestBuilder;

    /// Human-readable label for the Settings UI. Not currently consumed
    /// by Rust code (the frontend has its own labels) but exposed for
    /// completeness.
    fn label(&self) -> &str {
        self.id()
    }
}

// ---------------------------------------------------------------------
// Anthropic
// ---------------------------------------------------------------------

/// Direct Anthropic Messages API. Sends `x-api-key` +
/// `anthropic-version`.
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
        // Anthropic accepts its own ids as-is.
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

/// OpenRouter's Anthropic-Messages-compatible endpoint. Auth is
/// `Authorization: Bearer <key>`; OpenRouter also recommends
/// `HTTP-Referer` and `X-Title` so traffic is attributable on their
/// dashboard.
///
/// **Model translation:** OpenRouter's catalogue uses
/// `anthropic/<bare-id>` (the bare id is identical to Anthropic's, e.g.
/// `claude-sonnet-4-6`). We accept either shape from the user — if the
/// id already contains a `/` we treat it as a fully-qualified
/// OpenRouter id (e.g. the user may want to point at
/// `openai/gpt-4o-mini` once we surface that catalogue), otherwise we
/// prepend `anthropic/`.
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
        // The agent's tool-calling loop is exercised only against
        // Anthropic-flavoured models on OpenRouter today. The translate
        // step adds the `anthropic/` prefix at request build time, so we
        // keep the canonical Anthropic id here.
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
            // OpenRouter best-practice attribution headers.
            .header("HTTP-Referer", "https://edytlab.app")
            .header("X-Title", "edytlab")
            .header("content-type", "application/json")
    }
}

// ---------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------

/// Build a provider from its stable id. Unknown ids fall back to
/// Anthropic — the historical behaviour, which keeps existing keychain
/// entries working on first launch.
pub fn provider_from_id(id: &str) -> Arc<dyn LlmProvider> {
    match id {
        OPENROUTER_ID => Arc::new(OpenRouterProvider),
        // Anthropic and any unknown value both yield the Anthropic
        // provider. We deliberately don't error: the only place a stale
        // id can appear is in user settings persisted before a migration,
        // and silent fallback is friendlier than a hard crash.
        _ => Arc::new(AnthropicProvider),
    }
}

/// Stable ids for the providers shipped today. The frontend mirrors this
/// list so the Settings picker stays in sync.
pub const SUPPORTED_PROVIDER_IDS: &[&str] = &[ANTHROPIC_ID, OPENROUTER_ID];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anthropic_translate_model_is_identity() {
        let p = AnthropicProvider;
        assert_eq!(p.translate_model("claude-sonnet-4-6"), "claude-sonnet-4-6");
        assert_eq!(p.translate_model("claude-haiku-4-5"), "claude-haiku-4-5");
    }

    #[test]
    fn openrouter_translate_model_prepends_anthropic_namespace() {
        let p = OpenRouterProvider;
        assert_eq!(
            p.translate_model("claude-sonnet-4-6"),
            "anthropic/claude-sonnet-4-6"
        );
        assert_eq!(
            p.translate_model("claude-haiku-4-5-20251001"),
            "anthropic/claude-haiku-4-5-20251001"
        );
    }

    #[test]
    fn openrouter_translate_model_passes_through_already_qualified_ids() {
        let p = OpenRouterProvider;
        // Already namespaced — don't double-prefix.
        assert_eq!(
            p.translate_model("anthropic/claude-sonnet-4-6"),
            "anthropic/claude-sonnet-4-6"
        );
        // Other namespaces also pass through; OpenRouter's catalogue is
        // not Anthropic-only.
        assert_eq!(
            p.translate_model("openai/gpt-4o-mini"),
            "openai/gpt-4o-mini"
        );
    }

    #[test]
    fn provider_ids_are_stable() {
        assert_eq!(AnthropicProvider.id(), "anthropic");
        assert_eq!(OpenRouterProvider.id(), "openrouter");
    }

    #[test]
    fn provider_from_id_falls_back_to_anthropic_for_unknown() {
        // Used by the back-compat path: an empty/legacy active-provider
        // setting should produce the Anthropic provider.
        let p = provider_from_id("");
        assert_eq!(p.id(), "anthropic");
        let p = provider_from_id("unknown-provider");
        assert_eq!(p.id(), "anthropic");
    }
}
