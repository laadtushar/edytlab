//! The `chatgpt` provider: the agent on a ChatGPT Plus or Pro plan (#474, PR 4).
//!
//! It speaks the Responses API ([`crate::responses`]) to
//! `POST https://api.openai.com/v1/responses`, the one route the plan-usage
//! tokens are accepted on, with a bearer token from the signed-in
//! [`ChatGptSession`](super::ChatGptSession) instead of a key
//! ([`CredentialKind::ChatGptOAuth`], [`crate::Credential::ChatGpt`]). The
//! rest of what makes it different is a decision here, each written down where
//! it is made:
//!
//! * **The bearer goes to one host.** [`ChatGptProvider::honours_base_url_override`]
//!   is `false`, so no base-URL setting, keychain slot or argument can send the
//!   token anywhere but [`CHATGPT_API_BASE_URL`]. Tests that need a mock
//!   server have [`ChatGptProvider::loopback_for_tests`], which exists only
//!   with the crate's `test-endpoints` feature (the same seam as
//!   [`Endpoints::loopback_for_tests`](super::Endpoints::loopback_for_tests))
//!   and accepts only a loopback address.
//! * **Tools go down a ladder** ([`ToolRung`]). The preview allows a
//!   namespace or `additional_tools` input items; third parties report plain
//!   top-level functions also work; and only a real account can say whether
//!   one namespace of about ninety tools is taken. So the provider starts
//!   with the whole list in one `edytlab` namespace, and when the route
//!   answers 400 with a `param` that names the tools it steps to the slim
//!   list in the namespace, then to `additional_tools`, then to a flat list
//!   ([`LlmProvider::adapt_after_refusal`]). The rung that worked is kept for
//!   as long as the provider lives ([`ChatGptProvider::tool_rung`], and
//!   [`ChatGptProvider::with_tool_rung`] to start a new one where the last
//!   left off), a step never sends the same body twice, and the last refusal
//!   is shown as what it is.
//! * **Reasoning items** are asked for (`include`), kept and echoed back
//!   within a turn, never across turns ([`ReasoningReplay`]); a 400 whose
//!   `param` is `include` turns that off and carries on.
//! * **Reasoning effort** is sent as `reasoning.effort` when the model's
//!   catalogue entry lists the level ([`LlmProvider::effort_for_model`]).
//! * **No classifier.** [`LlmProvider::skips_classifier`] is `true`; see the
//!   method.
//! * **Failures are typed** ([`LlmProvider::classify_failure`],
//!   [`PlanUsageError`]), and nothing falls back to another provider.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use serde_json::Value;

use crate::anthropic::{Effort, MessagesRequest, StreamEvent};
use crate::models::ModelInfo;
use crate::provider::{
    Adaptation, CredentialKind, LlmProvider, ProviderError, ProviderFailure, ToolSet, WireFormat,
};
use crate::responses::{RequestOptions, ResponsesCodec, ToolGrouping};

use super::errors::{is_retryable_stream_code, renews_on_401, PlanUsageError};

/// Stable id for the ChatGPT plan provider. Its keychain slots follow the
/// pattern (`chatgpt_model`, `chatgpt_effort`, and `active_provider` can be
/// `chatgpt`); it has no `chatgpt_api_key` and no `chatgpt_base_url`, which
/// would be a key and an override for a provider that takes neither. The
/// refresh token's slot is [`super::store::REFRESH_TOKEN_SLOT`].
pub const CHATGPT_ID: &str = "chatgpt";

/// Where the bearer goes: OpenAI's API, and nowhere else. The model list is
/// `GET /v1/models` here too.
pub const CHATGPT_API_BASE_URL: &str = "https://api.openai.com";

/// The one namespace the agent's tools are grouped in, on the first two rungs.
pub const CHATGPT_NAMESPACE: &str = "edytlab";

/// The model used when none has been chosen.
///
/// The plan-usage docs' own examples use `gpt-6.1-sol`
/// (<https://developers.openai.com/siwc/token-sharing-open-source/models-and-inference>),
/// and `OPENAI_DEFAULT_MODEL` says it takes tools only through the Responses
/// API. It is a fallback and not a promise: "Models are per account", and
/// "No model should be hard-coded as the default" (spec §3.8). What a signed-in
/// account really has is `GET /v1/models`
/// ([`crate::models::list_chatgpt_models`]), and the caller that signs a
/// person in picks from it.
pub const CHATGPT_DEFAULT_MODEL: &str = "gpt-6.1-sol";

/// How long to wait before the one second try of a transient in-stream
/// failure.
pub const STREAM_RETRY_DELAY: Duration = Duration::from_secs(2);

/// What happens to the model's reasoning between the steps of a turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReasoningReplay {
    /// Ask for `reasoning.encrypted_content`, keep the `reasoning` items of
    /// each response, and echo them back in `input` with the tool results that
    /// follow, as the function-calling guide says a reasoning model needs.
    /// Only within the turn: [`crate::agent_loop::strip_thinking`] removes them
    /// at the start of the next, and when the tool layout changes, because
    /// they are tied to the `system` and `tools` they were produced under, and
    /// to the model that issued them (`invalid_encrypted_content` otherwise).
    Echo,
    /// Neither ask for nor keep them: the request this provider made before
    /// the question was settled. What a 400 about `include` switches to.
    Off,
}

/// How the agent's tools are offered, from the fullest to the last resort.
/// The provider starts on the first and steps down only when the route
/// refuses a layout ([`LlmProvider::adapt_after_refusal`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolRung {
    /// Every tool, in one `edytlab` namespace: the documented layout.
    NamespaceFull,
    /// A core list and the tools the message names ([`ToolSet::Slim`]), still
    /// in the namespace.
    NamespaceSlim,
    /// Every tool, in one `additional_tools` input item. Its shape is the
    /// obvious reading of a page that names it and shows none: a rung to try.
    AdditionalTools,
    /// Every tool as a plain top-level function.
    Flat,
}

impl ToolRung {
    /// How much of the tool list goes out. Slim only on the rung that is for
    /// a list that was too big: a refusal of the slim list in the namespace
    /// says the layout, and not the size, is what the route does not take.
    pub fn tool_set(self) -> ToolSet {
        match self {
            Self::NamespaceSlim => ToolSet::Slim,
            _ => ToolSet::Full,
        }
    }

    /// How the request lays the tools out.
    pub fn grouping(self) -> ToolGrouping {
        match self {
            Self::NamespaceFull | Self::NamespaceSlim => {
                ToolGrouping::Namespace(CHATGPT_NAMESPACE.to_string())
            }
            Self::AdditionalTools => ToolGrouping::AdditionalTools,
            Self::Flat => ToolGrouping::Flat,
        }
    }

    /// How far down the ladder this rung is: 0 for the first.
    pub fn depth(self) -> u8 {
        match self {
            Self::NamespaceFull => 0,
            Self::NamespaceSlim => 1,
            Self::AdditionalTools => 2,
            Self::Flat => 3,
        }
    }

    /// The rung after this one, if there is one.
    pub fn next(self) -> Option<Self> {
        match self {
            Self::NamespaceFull => Some(Self::NamespaceSlim),
            Self::NamespaceSlim => Some(Self::AdditionalTools),
            Self::AdditionalTools => Some(Self::Flat),
            Self::Flat => None,
        }
    }

    /// A stable word, for logs and for a window that shows or stores the rung.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NamespaceFull => "namespace",
            Self::NamespaceSlim => "namespace_slim",
            Self::AdditionalTools => "additional_tools",
            Self::Flat => "flat",
        }
    }

    /// The rung a stored word names.
    pub fn parse(word: &str) -> Option<Self> {
        [
            Self::NamespaceFull,
            Self::NamespaceSlim,
            Self::AdditionalTools,
            Self::Flat,
        ]
        .into_iter()
        .find(|r| r.as_str() == word)
    }
}

/// What the provider changes while it runs.
#[derive(Debug, Clone)]
struct State {
    rung: ToolRung,
    reasoning: ReasoningReplay,
    /// The `supported_reasoning_levels` of each model the last catalogue
    /// fetch listed, by slug.
    levels: HashMap<String, Vec<String>>,
}

/// The ChatGPT plan provider.
#[derive(Debug)]
pub struct ChatGptProvider {
    api_base: String,
    state: Mutex<State>,
    codec: ResponsesCodec,
    stream_retry_delay: Duration,
}

impl Default for ChatGptProvider {
    fn default() -> Self {
        Self::new(ToolRung::NamespaceFull, ReasoningReplay::Echo)
    }
}

impl Clone for ChatGptProvider {
    /// A copy starts with no response in flight, and where the original
    /// stands: the same rung, the same choice about reasoning, the same
    /// levels.
    fn clone(&self) -> Self {
        Self {
            api_base: self.api_base.clone(),
            state: Mutex::new(self.lock().clone()),
            codec: self.codec.clone(),
            stream_retry_delay: self.stream_retry_delay,
        }
    }
}

impl ChatGptProvider {
    fn new(rung: ToolRung, reasoning: ReasoningReplay) -> Self {
        Self {
            api_base: CHATGPT_API_BASE_URL.to_string(),
            state: Mutex::new(State {
                rung,
                reasoning,
                levels: HashMap::new(),
            }),
            codec: ResponsesCodec::new(ToolGrouping::Namespace(CHATGPT_NAMESPACE.to_string())),
            stream_retry_delay: STREAM_RETRY_DELAY,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Start on `rung`: the one a previous provider ended on, or the one a
    /// real account is known to take.
    pub fn with_tool_rung(self, rung: ToolRung) -> Self {
        self.lock().rung = rung;
        self
    }

    /// Start on the rung that offers `tool_set` in the namespace.
    pub fn with_tool_set(self, tool_set: ToolSet) -> Self {
        self.with_tool_rung(match tool_set {
            ToolSet::Full => ToolRung::NamespaceFull,
            ToolSet::Slim => ToolRung::NamespaceSlim,
        })
    }

    /// Whether reasoning items are asked for and echoed within a turn.
    pub fn with_reasoning(self, reasoning: ReasoningReplay) -> Self {
        self.lock().reasoning = reasoning;
        self
    }

    /// How long to wait before the one second try of a transient failure in a
    /// stream.
    pub fn with_stream_retry_delay(mut self, delay: Duration) -> Self {
        self.stream_retry_delay = delay;
        self
    }

    /// The rung the tools are on now: where the ladder stands. Remember it
    /// for the session and start the next provider there
    /// ([`Self::with_tool_rung`]), so a refusal is not learned again.
    pub fn tool_rung(&self) -> ToolRung {
        self.lock().rung
    }

    /// Moves the ladder to `rung` if that is further down than where it stands,
    /// and says whether it moved. For what a copy of this provider learned
    /// (Test, the model list) to be taken on **between turns**: a turn in
    /// flight keeps the layout it started with, and the next one starts where
    /// the copy ended. Never moves back up, because the ladder is walked in
    /// one direction and a copy that learned less must not undo what this one
    /// has learned ([`Self::reset_adaptations`] is how a new account starts
    /// again).
    pub fn adopt_rung(&self, rung: ToolRung) -> bool {
        let mut state = self.lock();
        if rung.depth() > state.rung.depth() {
            state.rung = rung;
            true
        } else {
            false
        }
    }

    /// The reasoning levels the last catalogue fetch listed, for every model
    /// it named. What a copy of this provider read (Test, the model list)
    /// and the app carries to the next agent and, between turns, to the one
    /// running ([`Self::adopt_levels`]).
    pub fn levels_snapshot(&self) -> HashMap<String, Vec<String>> {
        self.lock().levels.clone()
    }

    /// Takes `levels` as what the catalogue lists, in place of what this
    /// provider knows ([`LlmProvider::note_catalogue`] with the levels only).
    /// Between turns: the levels decide whether a request carries the
    /// person's reasoning effort, so changing them under a turn in flight
    /// would change that turn's reasoning layout.
    pub fn adopt_levels(&self, levels: HashMap<String, Vec<String>>) {
        self.lock().levels = levels;
    }

    /// The choice about reasoning now: `Off` after a 400 about `include`.
    pub fn reasoning_choice(&self) -> ReasoningReplay {
        self.lock().reasoning
    }

    /// Forget what was learned about the route (back to the first rung, and
    /// reasoning on): for a new account or a new sign-in, whose route may
    /// take what the last did not.
    pub fn reset_adaptations(&self) {
        let mut state = self.lock();
        state.rung = ToolRung::NamespaceFull;
        state.reasoning = ReasoningReplay::Echo;
    }

    /// The reasoning levels the last catalogue fetch listed for `model`.
    pub fn reasoning_levels(&self, model: &str) -> Vec<String> {
        self.lock().levels.get(model).cloned().unwrap_or_default()
    }

    /// A provider that talks to a mock API server on this machine, for tests:
    /// `base` must be `http` or `https` on a loopback address, with no path,
    /// query or credentials. Compiled only with the `test-endpoints` feature,
    /// which no build of the app enables (see `Endpoints` in
    /// [`crate::chatgpt`]).
    #[cfg(feature = "test-endpoints")]
    pub fn loopback_for_tests(base: &str) -> Result<Self, crate::oauth::EndpointError> {
        use crate::oauth::EndpointError;
        let url = url::Url::parse(base)?;
        if !crate::oauth::is_loopback_host(&url)
            || !matches!(url.scheme(), "http" | "https")
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || url.path() != "/"
        {
            return Err(EndpointError::NotHttps(format!(
                "{} (the test API must be a bare loopback origin)",
                url.origin().ascii_serialization()
            )));
        }
        Ok(Self {
            api_base: base.trim_end_matches('/').to_string(),
            // The tests do not wait two seconds.
            stream_retry_delay: Duration::from_millis(5),
            ..Self::default()
        })
    }
}

/// The `error.param` of a 400's body, lower-cased, if it names one.
fn param_of(body: &str) -> Option<String> {
    let json: Value = serde_json::from_str(body).ok()?;
    let param = json.get("error")?.get("param")?.as_str()?;
    (!param.is_empty()).then(|| param.to_lowercase())
}

impl LlmProvider for ChatGptProvider {
    fn id(&self) -> &'static str {
        CHATGPT_ID
    }

    fn base_url(&self) -> &str {
        &self.api_base
    }

    fn default_model(&self) -> &str {
        CHATGPT_DEFAULT_MODEL
    }

    /// Not used: the classifier is skipped ([`Self::skips_classifier`]). The
    /// default model, so that a caller which asks anyway gets a model the
    /// route knows.
    fn classifier_model(&self) -> &str {
        CHATGPT_DEFAULT_MODEL
    }

    fn translate_model(&self, model: &str) -> String {
        model.to_string()
    }

    fn apply_auth(&self, req: reqwest::RequestBuilder, bearer: &str) -> reqwest::RequestBuilder {
        // `bearer_auth` marks the header sensitive: a debug print of the
        // request or its headers shows no token.
        req.bearer_auth(bearer)
    }

    fn endpoint_path(&self) -> &str {
        "/v1/responses"
    }

    fn list_models_path(&self) -> &str {
        "/v1/models"
    }

    fn wire_format(&self) -> WireFormat {
        WireFormat::Responses
    }

    fn credential_kind(&self) -> CredentialKind {
        CredentialKind::ChatGptOAuth
    }

    fn honours_base_url_override(&self) -> bool {
        false
    }

    /// The reasoning effort applies: `<provider>_effort` is read like any
    /// other provider's, and [`Self::effort_for_model`] holds it to the levels
    /// the model lists.
    fn supports_effort(&self) -> bool {
        true
    }

    fn tool_set(&self) -> ToolSet {
        self.lock().rung.tool_set()
    }

    /// The mode classifier is a model call of its own before every turn, and
    /// every call here is spent from the person's Plus or Pro allowance, which
    /// is shared with Codex and every other app they use and has a five-hour
    /// limit on Plus (spec §3.8, §4.8). Skipping it is the first of the two
    /// options the plan names (the other was the lowest listed effort on the
    /// cheapest listed model); the catalogue says nothing about price, so
    /// there is no cheapest to choose, and a call that is only worth a word is
    /// not worth a share of the allowance.
    ///
    /// What it costs: every message is taken as the general mode, so the
    /// mashup, mix and voice prompts are never added and a request that would
    /// have been called a mashup is not held for a plan unless the person has
    /// Plan first on. Plan first is unaffected, and its plan request is the
    /// main model's own work at the effort the person chose.
    fn skips_classifier(&self) -> bool {
        true
    }

    fn replays_thinking(&self) -> bool {
        self.lock().reasoning == ReasoningReplay::Echo
    }

    fn serialize_request(&self, req: &MessagesRequest<'_>) -> Value {
        let options = {
            let state = self.lock();
            RequestOptions {
                grouping: state.rung.grouping(),
                reasoning: state.reasoning == ReasoningReplay::Echo,
                effort: effort_in(&state, req.model, req.output_config.map(|o| o.effort)),
            }
        };
        self.codec.serialize_with(req, &options)
    }

    fn parse_stream_chunk(&self, raw: &str) -> Result<Vec<StreamEvent>, ProviderError> {
        self.codec.parse_stream_chunk(raw)
    }

    fn effort_for_model(&self, model: &str, effort: Option<Effort>) -> Option<Effort> {
        effort_in(&self.lock(), model, effort)
    }

    fn note_catalogue(&self, models: &[ModelInfo]) {
        let mut state = self.lock();
        state.levels = models
            .iter()
            .map(|m| (m.id.clone(), m.reasoning_levels.clone()))
            .collect();
    }

    fn classify_failure(&self, failure: &ProviderFailure<'_>) -> Option<crate::Error> {
        let error = match failure {
            ProviderFailure::Http {
                status,
                body,
                request_id,
            } => PlanUsageError::from_http(*status, body, *request_id),
            ProviderFailure::Stream {
                kind,
                message,
                param,
                request_id,
            } => PlanUsageError::from_code(kind, *param, message, *request_id),
        };
        error.map(crate::Error::PlanUsage)
    }

    /// Steps down the ladder, or turns reasoning off, when a 400 names the
    /// field at fault. The `param` decides:
    ///
    /// * one that starts with `include`: the route does not take
    ///   `reasoning.encrypted_content`. Reasoning goes [`Off`](ReasoningReplay::Off).
    /// * one that names the tools (`tools`, `tools[0]...`, `tool_choice`):
    ///   the layout is refused. The next rung.
    /// * one that starts with `input`, while the tools are in an
    ///   `additional_tools` item (which is in `input`): that rung's shape is
    ///   wrong. The next rung. On any other rung a 400 about `input` is a
    ///   history item the route does not take (it lists the ones it does), and
    ///   no other layout of the tools changes that, so it is **never**
    ///   retried: the error is shown.
    ///
    /// Anything else, a 400 with no `param`, any other status, is not
    /// adapted to. Every `Some` leaves the provider somewhere it was not, so
    /// the loop cannot send one body twice, and the ladder ends at the last
    /// rung.
    fn adapt_after_refusal(&self, failure: &ProviderFailure<'_>) -> Option<Adaptation> {
        let ProviderFailure::Http {
            status: 400, body, ..
        } = failure
        else {
            return None;
        };
        let param = param_of(body)?;
        let mut state = self.lock();
        if param.starts_with("include") {
            return (state.reasoning == ReasoningReplay::Echo).then(|| {
                state.reasoning = ReasoningReplay::Off;
                tracing::warn!("chatgpt: the route refused `include`; reasoning is off");
                Adaptation::Reasoning
            });
        }
        let names_tools = param.contains("tool");
        let names_input = param.starts_with("input");
        let step = names_tools || (names_input && state.rung == ToolRung::AdditionalTools);
        if !step {
            return None;
        }
        let next = state.rung.next()?;
        tracing::warn!(
            from = state.rung.as_str(),
            to = next.as_str(),
            param = %param,
            "chatgpt: the route refused the tool layout; stepping down"
        );
        state.rung = next;
        Some(Adaptation::ToolLayout)
    }

    fn retries_stream_failure(&self, code: &str) -> bool {
        is_retryable_stream_code(code)
    }

    fn stream_retry_delay(&self) -> Duration {
        self.stream_retry_delay
    }

    fn renews_on_401(&self, body: &str) -> bool {
        renews_on_401(body)
    }

    fn label(&self) -> &str {
        "ChatGPT plan"
    }
}

/// `effort` if `model` lists it among its reasoning levels, else none. The
/// route refuses a level a model does not support, and a model the catalogue
/// has not been read for is not known to support any.
fn effort_in(state: &State, model: &str, effort: Option<Effort>) -> Option<Effort> {
    let effort = effort?;
    state
        .levels
        .get(model)?
        .iter()
        .any(|level| level == effort.as_str())
        .then_some(effort)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anthropic::{ContentBlock, Message, OutputConfig, Role, SystemBlock};
    use crate::chatgpt::PlanUsageKind;
    use serde_json::json;

    fn request<'a>(
        messages: &'a [Message],
        tools: Option<Value>,
        effort: Option<Effort>,
    ) -> MessagesRequest<'a> {
        MessagesRequest {
            model: "gpt-x",
            max_tokens: 64_000,
            system: vec![SystemBlock {
                kind: "text",
                text: "be a DAW",
                cache_control: None,
            }],
            messages,
            tools,
            tool_choice: None,
            stream: false,
            output_config: effort.map(OutputConfig::effort),
        }
    }

    fn hi() -> [Message; 1] {
        [Message {
            role: Role::User,
            content: vec![ContentBlock::Text { text: "hi".into() }],
        }]
    }

    fn one_tool() -> Value {
        json!([{ "name": "gain", "description": "d", "input_schema": {"type": "object"} }])
    }

    fn refusal(param: &str) -> String {
        json!({ "error": {
            "code": "subscription_sharing_unsupported_capability",
            "message": "no", "param": param
        } })
        .to_string()
    }

    fn http<'a>(status: u16, body: &'a str) -> ProviderFailure<'a> {
        ProviderFailure::Http {
            status,
            body,
            request_id: None,
        }
    }

    #[test]
    fn it_is_the_responses_provider_on_oauth() {
        let p = ChatGptProvider::default();
        assert_eq!(p.id(), "chatgpt");
        assert_eq!(p.wire_format(), WireFormat::Responses);
        assert_eq!(p.endpoint_path(), "/v1/responses");
        assert_eq!(p.list_models_path(), "/v1/models");
        assert_eq!(p.base_url(), "https://api.openai.com");
        assert_eq!(p.credential_kind(), CredentialKind::ChatGptOAuth);
        assert!(!p.requires_api_key(), "it has a session, not a key");
        assert!(p.supports_effort());
        assert_eq!(p.label(), "ChatGPT plan");
    }

    /// The exfiltration guard: nothing a person or a stored setting says moves
    /// the host the bearer goes to.
    #[test]
    fn it_ignores_a_base_url_override() {
        let p = ChatGptProvider::default();
        assert!(!p.honours_base_url_override());
        let cfg = crate::LlmConfig::new(std::sync::Arc::new(p), "k")
            .with_base_url("https://evil.example");
        assert_eq!(cfg.base_url(), "https://api.openai.com");
        let cfg = crate::LlmConfig::new(crate::provider::provider_from_id(CHATGPT_ID), "k")
            .with_base_url("http://127.0.0.1:1");
        assert_eq!(cfg.base_url(), "https://api.openai.com");
    }

    #[test]
    fn the_bearer_is_the_only_credential_header() {
        let p = ChatGptProvider::default();
        let req = reqwest::Client::new().post("https://api.openai.com/v1/responses");
        let built = p.apply_auth(req, "AT-1").build().expect("builds");
        assert_eq!(built.headers()["authorization"], "Bearer AT-1");
        // Marked sensitive, so a debug print of the request or its headers
        // shows no token.
        assert!(built.headers()["authorization"].is_sensitive());
        assert!(!format!("{:?}", built.headers()).contains("AT-1"));
        for other in ["x-api-key", "openai-organization", "openai-project"] {
            assert!(built.headers().get(other).is_none(), "{other}");
        }
    }

    #[test]
    fn it_skips_the_classifier_and_other_providers_do_not() {
        assert!(ChatGptProvider::default().skips_classifier());
        for id in crate::SUPPORTED_PROVIDER_IDS {
            let p = crate::provider::provider_from_id(id);
            assert_eq!(p.skips_classifier(), *id == CHATGPT_ID, "{id}");
        }
    }

    /// Each rung is a body of its own: the namespace the page documents, the
    /// `additional_tools` item it names, and the plain functions third
    /// parties report working.
    #[test]
    fn every_rung_lays_the_tools_out_its_own_way() {
        let messages = hi();
        let tool_in =
            |p: &ChatGptProvider| p.serialize_request(&request(&messages, Some(one_tool()), None));

        let ns = tool_in(&ChatGptProvider::default());
        assert_eq!(ns["tools"][0]["type"], "namespace");
        assert_eq!(ns["tools"][0]["name"], "edytlab");
        assert!(ns["tools"][0]["description"]
            .as_str()
            .is_some_and(|d| !d.is_empty()));
        assert_eq!(ns["tools"][0]["tools"][0]["name"], "gain");
        assert_eq!(ns["tools"].as_array().unwrap().len(), 1);
        let slim = tool_in(&ChatGptProvider::default().with_tool_rung(ToolRung::NamespaceSlim));
        assert_eq!(slim["tools"][0]["type"], "namespace");

        let additional =
            tool_in(&ChatGptProvider::default().with_tool_rung(ToolRung::AdditionalTools));
        assert!(additional.get("tools").is_none(), "{additional}");
        assert_eq!(additional["input"][0]["type"], "additional_tools");
        assert_eq!(additional["input"][0]["tools"][0]["name"], "gain");
        assert_eq!(additional["input"][1]["role"], "user");

        let flat = tool_in(&ChatGptProvider::default().with_tool_rung(ToolRung::Flat));
        assert_eq!(flat["tools"][0]["type"], "function");
        assert_eq!(flat["tools"][0]["name"], "gain");
        assert!(flat["input"]
            .as_array()
            .unwrap()
            .iter()
            .all(|i| i["type"] != "additional_tools"));
        assert_eq!(flat["tool_choice"], "auto");

        // The three layouts are three bodies. (The slim namespace is the same
        // body for the same tools: whether it differs depends on the tool
        // list, which the agent loop knows and the provider does not.)
        let all = [ns, additional, flat];
        for (i, a) in all.iter().enumerate() {
            for b in &all[i + 1..] {
                assert_ne!(a, b);
            }
        }
    }

    #[test]
    fn the_tool_set_follows_the_rung() {
        for (rung, set) in [
            (ToolRung::NamespaceFull, ToolSet::Full),
            (ToolRung::NamespaceSlim, ToolSet::Slim),
            (ToolRung::AdditionalTools, ToolSet::Full),
            (ToolRung::Flat, ToolSet::Full),
        ] {
            assert_eq!(rung.tool_set(), set, "{rung:?}");
            assert_eq!(
                ChatGptProvider::default().with_tool_rung(rung).tool_set(),
                set
            );
            assert_eq!(ToolRung::parse(rung.as_str()), Some(rung));
        }
        assert_eq!(ChatGptProvider::default().tool_set(), ToolSet::Full);
        assert_eq!(
            ChatGptProvider::default()
                .with_tool_set(ToolSet::Slim)
                .tool_rung(),
            ToolRung::NamespaceSlim
        );
        assert_eq!(ToolRung::parse("nonsense"), None);
    }

    /// The whole ladder, driven by the 400's `param`, one step at a time, and
    /// out of rungs at the end.
    #[test]
    fn a_400_about_the_tools_steps_down_one_rung_at_a_time() {
        let p = ChatGptProvider::default();
        let mut seen = vec![p.tool_rung()];
        for param in ["tools", "tools[0].tools", "tools"] {
            let body = refusal(param);
            assert_eq!(
                p.adapt_after_refusal(&http(400, &body)),
                Some(Adaptation::ToolLayout),
                "{param}"
            );
            seen.push(p.tool_rung());
        }
        assert_eq!(
            seen,
            [
                ToolRung::NamespaceFull,
                ToolRung::NamespaceSlim,
                ToolRung::AdditionalTools,
                ToolRung::Flat
            ]
        );
        // Out of rungs: the refusal is the answer.
        assert_eq!(p.adapt_after_refusal(&http(400, &refusal("tools"))), None);
        assert_eq!(p.tool_rung(), ToolRung::Flat);
    }

    #[test]
    fn only_a_400_that_names_the_tools_moves_the_ladder() {
        let p = ChatGptProvider::default();
        let about_tools = refusal("tools");
        let nothing = [
            // Other statuses.
            http(429, &about_tools),
            http(503, &about_tools),
            http(401, &about_tools),
        ];
        let bodies = [
            refusal("model"),
            refusal("temperature"),
            // The history, not the tools (the route lists the items it takes):
            // never retried, on any rung but the additional-tools one.
            refusal("input[3]"),
            refusal("input"),
            json!({ "error": { "message": "no param" } }).to_string(),
            "not json".to_string(),
        ];
        for failure in nothing {
            assert_eq!(p.adapt_after_refusal(&failure), None);
        }
        for body in &bodies {
            assert_eq!(p.adapt_after_refusal(&http(400, body)), None, "{body}");
        }
        // A stream error is not a refusal of the request.
        assert_eq!(
            p.adapt_after_refusal(&ProviderFailure::Stream {
                kind: "subscription_sharing_unsupported_capability",
                message: "m",
                param: Some("tools"),
                request_id: None,
            }),
            None
        );
        assert_eq!(p.tool_rung(), ToolRung::NamespaceFull, "nothing moved");
    }

    /// `additional_tools` lives in `input`, so a refusal about `input` is that
    /// rung's own shape and the next rung is the answer; on any other rung it
    /// is a history item and nothing changes.
    #[test]
    fn a_400_about_input_moves_only_the_additional_tools_rung() {
        let p = ChatGptProvider::default().with_tool_rung(ToolRung::AdditionalTools);
        assert_eq!(
            p.adapt_after_refusal(&http(400, &refusal("input[0]"))),
            Some(Adaptation::ToolLayout)
        );
        assert_eq!(p.tool_rung(), ToolRung::Flat);
        assert_eq!(
            p.adapt_after_refusal(&http(400, &refusal("input[0]"))),
            None
        );
        for rung in [
            ToolRung::NamespaceFull,
            ToolRung::NamespaceSlim,
            ToolRung::Flat,
        ] {
            let p = ChatGptProvider::default().with_tool_rung(rung);
            assert_eq!(
                p.adapt_after_refusal(&http(400, &refusal("input[0]"))),
                None
            );
            assert_eq!(p.tool_rung(), rung);
        }
    }

    #[test]
    fn a_400_about_include_turns_reasoning_off_once() {
        let p = ChatGptProvider::default();
        assert!(p.replays_thinking());
        assert_eq!(
            p.adapt_after_refusal(&http(400, &refusal("include"))),
            Some(Adaptation::Reasoning)
        );
        assert!(!p.replays_thinking());
        assert_eq!(p.reasoning_choice(), ReasoningReplay::Off);
        let body = p.serialize_request(&request(&hi(), None, None));
        assert!(body.get("include").is_none(), "{body}");
        // Already off: a second refusal is not adapted to.
        assert_eq!(
            p.adapt_after_refusal(&http(400, &refusal("include[0]"))),
            None
        );
        // The tool layout was not touched.
        assert_eq!(p.tool_rung(), ToolRung::NamespaceFull);
        // And it can be reset for a new account.
        p.reset_adaptations();
        assert!(p.replays_thinking());
    }

    #[test]
    fn what_was_learned_is_kept_by_a_copy_and_reset_by_reset() {
        let p = ChatGptProvider::default();
        p.adapt_after_refusal(&http(400, &refusal("tools")));
        p.adapt_after_refusal(&http(400, &refusal("include")));
        let copy = p.clone();
        assert_eq!(copy.tool_rung(), ToolRung::NamespaceSlim);
        assert_eq!(copy.reasoning_choice(), ReasoningReplay::Off);
        p.reset_adaptations();
        assert_eq!(p.tool_rung(), ToolRung::NamespaceFull);
        assert_eq!(
            copy.tool_rung(),
            ToolRung::NamespaceSlim,
            "the copy is its own"
        );
    }

    /// Test and the model list run beside a turn on a copy of the provider
    /// (#474, review finding 7). Whatever the copy does (steps the ladder,
    /// turns reasoning off, reads another catalogue, builds a request, which
    /// starts its parser afresh) must not change the original, or a turn in
    /// flight would change its tools, its reasoning layout or lose its
    /// stream half way.
    #[test]
    fn a_copy_that_learns_or_builds_a_request_leaves_the_original_as_it_was() {
        let p = ChatGptProvider::default();
        p.note_catalogue(&[model("gpt-x", &["low", "high"])]);
        let delta = |text: &str| {
            json!({
                "type": "response.output_text.delta",
                "output_index": 0, "content_index": 0, "delta": text
            })
            .to_string()
        };
        // The turn's request is built, and its response is half way through.
        let before = p.serialize_request(&request(&hi(), None, Some(Effort::High)));
        let first = p.parse_stream_chunk(&delta("cut here")).unwrap();
        assert!(first.len() > 1, "the first chunk starts the block");

        let copy = p.clone();
        assert_eq!(
            copy.adapt_after_refusal(&http(400, &refusal("tools"))),
            Some(Adaptation::ToolLayout)
        );
        assert_eq!(
            copy.adapt_after_refusal(&http(400, &refusal("include"))),
            Some(Adaptation::Reasoning)
        );
        copy.note_catalogue(&[model("gpt-x", &[])]);
        copy.serialize_request(&request(&hi(), None, Some(Effort::High)));
        copy.parse_stream_chunk(&delta("the copy's own")).unwrap();
        copy.reset_adaptations();

        assert_eq!(p.tool_rung(), ToolRung::NamespaceFull);
        assert_eq!(p.reasoning_choice(), ReasoningReplay::Echo);
        assert_eq!(p.reasoning_levels("gpt-x"), ["low", "high"]);
        // The response goes on from where it was: one delta, not a restart.
        let more = p.parse_stream_chunk(&delta("more")).unwrap();
        assert_eq!(more.len(), 1, "{more:?}");
        assert!(matches!(more[0], StreamEvent::ContentBlockDelta { .. }));
        assert_eq!(
            p.serialize_request(&request(&hi(), None, Some(Effort::High))),
            before,
            "the next request: the same tools and reasoning layout"
        );
    }

    #[test]
    fn levels_are_read_out_and_adopted_whole() {
        let p = ChatGptProvider::default();
        p.note_catalogue(&[model("a", &["low"]), model("b", &[])]);
        let levels = p.levels_snapshot();
        assert_eq!(levels["a"], ["low"]);
        assert!(levels["b"].is_empty());
        let q = ChatGptProvider::default();
        q.note_catalogue(&[model("old", &["max"])]);
        q.adopt_levels(levels);
        assert_eq!(q.reasoning_levels("a"), ["low"]);
        assert!(q.reasoning_levels("old").is_empty(), "replaced, not merged");
    }

    /// What a copy learned goes back on purpose, between turns, and only
    /// down: the ladder is walked in one direction, so a copy that learned
    /// less cannot undo what the original has learned.
    #[test]
    fn a_rung_is_adopted_only_further_down_the_ladder() {
        let p = ChatGptProvider::default();
        assert!(p.adopt_rung(ToolRung::AdditionalTools));
        assert_eq!(p.tool_rung(), ToolRung::AdditionalTools);
        // Not back up, and not to where it is.
        assert!(!p.adopt_rung(ToolRung::NamespaceFull));
        assert!(!p.adopt_rung(ToolRung::AdditionalTools));
        assert_eq!(p.tool_rung(), ToolRung::AdditionalTools);
        assert!(p.adopt_rung(ToolRung::Flat));
        assert_eq!(p.tool_rung(), ToolRung::Flat);
    }

    #[test]
    fn reasoning_is_asked_for_by_default_and_can_be_turned_off() {
        let on = ChatGptProvider::default();
        let body = on.serialize_request(&request(&hi(), None, None));
        assert_eq!(body["include"], json!(["reasoning.encrypted_content"]));
        assert_eq!(body["store"], json!(false));
        assert_eq!(body["stream"], json!(true));
        let off = ChatGptProvider::default().with_reasoning(ReasoningReplay::Off);
        assert!(!off.replays_thinking());
        let body = off.serialize_request(&request(&hi(), None, None));
        assert!(body.get("include").is_none(), "{body}");
    }

    fn model(id: &str, levels: &[&str]) -> ModelInfo {
        ModelInfo {
            id: id.into(),
            display_name: id.into(),
            reasoning_levels: levels.iter().map(|l| l.to_string()).collect(),
            ..ModelInfo::default()
        }
    }

    /// The effort the person chose is sent only for a model that lists it.
    #[test]
    fn an_effort_is_sent_only_when_the_model_lists_it() {
        let p = ChatGptProvider::default();
        let messages = hi();
        let effort_in = |p: &ChatGptProvider, e: Effort| {
            p.serialize_request(&request(&messages, None, Some(e)))
                .get("reasoning")
                .cloned()
        };
        // The catalogue has not been read: none is known to be supported.
        assert_eq!(effort_in(&p, Effort::High), None);
        p.note_catalogue(&[
            model("gpt-x", &["low", "medium", "high"]),
            model("other", &["max"]),
        ]);
        assert_eq!(
            effort_in(&p, Effort::High),
            Some(json!({ "effort": "high" }))
        );
        assert_eq!(effort_in(&p, Effort::Low), Some(json!({ "effort": "low" })));
        assert_eq!(effort_in(&p, Effort::XHigh), None, "not listed for gpt-x");
        assert_eq!(
            effort_in(&p, Effort::Max),
            None,
            "listed for another model only"
        );
        assert_eq!(p.reasoning_levels("other"), ["max"]);
        // None chosen: none sent, and the body is otherwise the same.
        let none = p.serialize_request(&request(&messages, None, None));
        assert!(none.get("reasoning").is_none());
        // The one-shot path asks the same question.
        assert_eq!(
            p.effort_for_model("gpt-x", Some(Effort::High)),
            Some(Effort::High)
        );
        assert_eq!(p.effort_for_model("gpt-x", Some(Effort::Max)), None);
        assert_eq!(p.effort_for_model("gpt-x", None), None);
        // A new catalogue replaces the old.
        p.note_catalogue(&[model("gpt-x", &[])]);
        assert_eq!(effort_in(&p, Effort::High), None);
    }

    #[test]
    fn a_failure_it_knows_is_its_own_error_and_one_it_does_not_is_left_alone() {
        let p = ChatGptProvider::default();
        let kind = |failure: &ProviderFailure<'_>| match p.classify_failure(failure) {
            Some(crate::Error::PlanUsage(e)) => {
                Some((e.kind().clone(), e.request_id().map(str::to_string)))
            }
            Some(other) => panic!("{other:?}"),
            None => None,
        };
        assert_eq!(
            kind(&ProviderFailure::Stream {
                kind: "subscription_sharing_usage_limit_exceeded",
                message: "m",
                param: None,
                request_id: Some("req_1"),
            }),
            Some((PlanUsageKind::UsageLimit, Some("req_1".into())))
        );
        assert_eq!(
            kind(&ProviderFailure::Http {
                status: 403,
                body: r#"{"error":{"code":"subscription_sharing_user_not_eligible"}}"#,
                request_id: None,
            }),
            Some((PlanUsageKind::NotEligible, None))
        );
        assert_eq!(
            kind(&ProviderFailure::Stream {
                kind: "context_length_exceeded",
                message: "m",
                param: None,
                request_id: None,
            }),
            None
        );
        assert_eq!(kind(&http(500, "oops")), None);
    }

    #[test]
    fn stream_retries_and_the_401_rule_are_the_errors_pages() {
        let p = ChatGptProvider::default();
        assert!(p.retries_stream_failure("server_error"));
        assert!(p.retries_stream_failure("subscription_sharing_usage_unavailable"));
        assert!(!p.retries_stream_failure("subscription_sharing_usage_limit_exceeded"));
        assert!(p.renews_on_401(""));
        assert!(!p.renews_on_401(r#"{"error":{"code":"subscription_sharing_user_not_eligible"}}"#));
        assert_eq!(p.stream_retry_delay(), STREAM_RETRY_DELAY);
        assert_eq!(
            p.with_stream_retry_delay(Duration::ZERO)
                .stream_retry_delay(),
            Duration::ZERO
        );
    }

    #[cfg(feature = "test-endpoints")]
    #[test]
    fn the_test_api_must_be_a_bare_loopback_origin() {
        for ok in [
            "http://127.0.0.1:8080",
            "http://127.0.0.1:8080/",
            "http://[::1]:9",
        ] {
            let p = ChatGptProvider::loopback_for_tests(ok).unwrap_or_else(|e| panic!("{ok}: {e}"));
            assert!(!p.base_url().ends_with('/'), "{}", p.base_url());
        }
        for bad in [
            "https://api.openai.com",
            "https://evil.example",
            "http://10.0.0.1:80",
            "http://127.0.0.1.evil.example",
            "http://user:pw@127.0.0.1:80",
            "http://127.0.0.1:80/v1",
            "file:///tmp/x",
        ] {
            assert!(ChatGptProvider::loopback_for_tests(bad).is_err(), "{bad}");
        }
    }
}
