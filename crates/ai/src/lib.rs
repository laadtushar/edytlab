//! AI layer (Phase 1, M10).
//!
//! Wraps Anthropic's Messages API behind an [`Agent`] type that runs
//! the tool-calling loop against the [`tools::ToolDispatcher`] from
//! M07–M09. The agent owns the conversation history (in-memory; Phase 2
//! adds persistence) and emits a stream of [`AgentEvent`]s suitable for
//! piping into a Tauri channel.
//!
//! Usage sketch:
//!
//! ```ignore
//! use std::sync::{Arc, Mutex};
//! use ai::{Agent, LlmConfig};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let api_key = ai::keychain::load_api_key(ai::ANTHROPIC_ID).expect("api key not configured");
//! let dispatcher = Arc::new(Mutex::new(tools::ToolDispatcher::default_dispatcher()));
//! let store = Arc::new(Mutex::new(session::Store::open(std::path::Path::new("/tmp/proj"))?));
//! let engine = Arc::new(Mutex::new(audio_engine::Engine::new()));
//!
//! let mut agent = Agent::new(
//!     LlmConfig::new_anthropic(api_key),
//!     dispatcher,
//!     store,
//!     engine,
//!     Arc::new(tokio::sync::Notify::new()),
//! );
//!
//! let result = agent
//!     .turn("normalize this to -1 dBFS".to_string(), |event| {
//!         println!("{event:?}");
//!     })
//!     .await?;
//! println!("final: {}", result.text);
//! # Ok(()) }
//! ```

pub mod agent_loop;
pub mod anthropic;
pub mod keychain;
pub mod models;
pub mod prompt;
pub mod provider;
pub mod validate;

use std::sync::{Arc, Mutex};

use anthropic::Message;

pub use models::{list_models_for, ModelInfo};
pub use prompt::{DEFAULT_BASE_URL, DEFAULT_MODEL, MAX_TOOL_CALLS_PER_TURN};
pub use provider::{
    AnthropicProvider, LlmProvider, OpenAIProvider, OpenRouterProvider, ANTHROPIC_ID, OPENAI_ID,
    OPENROUTER_ID, SUPPORTED_PROVIDER_IDS,
};

/// Classifier model used for cheap mode detection (M27).
pub const CLASSIFIER_MODEL: &str = "claude-haiku-4-5-20251001";

/// Configuration for the LLM client.
///
/// Extracted from the original `AnthropicConfig` to support multiple
/// providers (Anthropic + OpenRouter today, more later). The provider
/// supplies headers and model translation; the [`LlmConfig`] owns the
/// per-call data (key, model, optional base URL override).
///
/// `base_url_override` is parameterised so integration tests can point
/// the agent at a `wiremock` server without monkey-patching DNS or env
/// vars. When `None` the provider's default base URL is used.
#[derive(Debug, Clone)]
pub struct LlmConfig {
    /// Provider implementation: defines auth headers, default model,
    /// and model id translation.
    pub provider: Arc<dyn LlmProvider>,
    /// API key, loaded from the OS keychain. Never logged.
    pub api_key: String,
    /// Canonical (Anthropic-shaped) model id. Translated per-provider
    /// before being sent on the wire. Defaults to the provider's
    /// [`LlmProvider::default_model`].
    pub model: String,
    /// Base URL override (no trailing slash). When `None` the
    /// provider's [`LlmProvider::base_url`] is used. Tests set this to
    /// the `wiremock` server URI.
    pub base_url_override: Option<String>,
}

impl LlmConfig {
    /// Build a config from a provider + key. Uses the provider's default
    /// model and default base URL.
    pub fn new(provider: Arc<dyn LlmProvider>, api_key: impl Into<String>) -> Self {
        let model = provider.default_model().to_string();
        Self {
            provider,
            api_key: api_key.into(),
            model,
            base_url_override: None,
        }
    }

    /// Override the (canonical) model id.
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }

    /// Override the base URL (used by tests against `wiremock`).
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url_override = Some(base_url.into());
        self
    }

    /// Resolve the base URL: override if set, else the provider default.
    pub fn base_url(&self) -> &str {
        self.base_url_override
            .as_deref()
            .unwrap_or_else(|| self.provider.base_url())
    }

    /// Wire-format model id (with the provider's translation applied).
    pub fn wire_model(&self) -> String {
        self.provider.translate_model(&self.model)
    }

    /// Wire-format classifier model id (with the provider's translation
    /// applied to the provider's [`LlmProvider::classifier_model`]).
    pub fn wire_classifier_model(&self) -> String {
        self.provider
            .translate_model(self.provider.classifier_model())
    }
}

impl LlmConfig {
    /// Construct an Anthropic-flavoured config from just the API key —
    /// equivalent to the pre-multi-provider `AnthropicConfig::new(key)`.
    /// Preserved as the canonical short-form constructor for the
    /// Anthropic happy path (CLI, tests, the desktop `rebuild_agent`
    /// path).
    pub fn new_anthropic(api_key: impl Into<String>) -> Self {
        Self::new(Arc::new(AnthropicProvider), api_key)
    }

    /// Construct an OpenRouter-flavoured config from just the API key.
    pub fn new_openrouter(api_key: impl Into<String>) -> Self {
        Self::new(Arc::new(OpenRouterProvider), api_key)
    }

    /// Construct an OpenAI-flavoured config from just the API key.
    pub fn new_openai(api_key: impl Into<String>) -> Self {
        Self::new(Arc::new(OpenAIProvider::default()), api_key)
    }
}

/// Back-compat type alias for the pre-multi-provider type name. Existing
/// code that imports `ai::AnthropicConfig` continues to compile; new code
/// should reach for [`LlmConfig`] directly.
pub type AnthropicConfig = LlmConfig;

/// Events emitted to `on_event` during a [`Agent::turn`] call.
#[derive(Debug, Clone)]
pub enum AgentEvent {
    /// A piece of streamed assistant text. Concatenating these in order
    /// yields the assistant's natural-language output for the turn.
    TextDelta(String),
    /// The model started a tool call. The same `id` will appear later
    /// in [`AgentEvent::ToolCallEnd`].
    ToolCallStart { name: String, id: String },
    /// The dispatcher finished invoking a tool call. `ok` is false for
    /// schema validation errors and tool-level errors alike.
    ToolCallEnd { id: String, ok: bool },
    /// A tool call resulted in a new session node. Emitted before the
    /// matching [`AgentEvent::ToolCallEnd`].
    NodeCreated(session::NodeId),
    /// Final event of a turn. Always emitted on success.
    Done,
    /// Emitted in mashup mode before tool execution. Contains the
    /// serialised plan steps. The loop suspends until `approve_plan` is
    /// called on the agent.
    Plan { steps: Vec<serde_json::Value> },
}

/// Outcome of a single [`Agent::turn`] call.
#[derive(Debug, Clone, Default)]
pub struct TurnResult {
    /// All assistant text concatenated, in order. The same string the
    /// caller could rebuild from the `TextDelta` events.
    pub text: String,
    /// `stop_reason` from the final Anthropic response (`end_turn`,
    /// `tool_use`, `max_tokens`, ...). `None` if the server did not
    /// emit one.
    pub stop_reason: Option<String>,
    /// Session node ids the agent created during the turn, in order.
    pub node_ids: Vec<session::NodeId>,
}

/// AI layer error type. Distinct from tool-level errors (which surface
/// via `tool_result { is_error: true }` to the model).
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("http error: {0}")]
    Http(reqwest::Error),

    #[error("anthropic api error ({status}): {message}")]
    Api { status: u16, message: String },

    #[error("anthropic stream error: {0}")]
    ApiStream(String),

    #[error("sse parse error: {0}")]
    Sse(eventsource_stream::EventStreamError<std::io::Error>),

    #[error("json parse error: {0}")]
    Json(serde_json::Error),

    #[error("streaming protocol violation: {0}")]
    Protocol(String),

    #[error(
        "model exceeded the per-turn tool budget of {0}; the run was aborted to protect the user"
    )]
    ToolBudgetExceeded(usize),

    #[error("tool argument validation failed twice: {0}")]
    ToolValidation(String),

    #[error("plan approval timed out (5 minutes); the mashup run was aborted")]
    PlanTimeout,
}

pub type Result<T> = std::result::Result<T, Error>;

/// The agent. Owns the HTTP client and conversation history; takes
/// shared references to the tool dispatcher, session store, and audio
/// engine so multiple agents (or other callers) can share them.
pub struct Agent {
    cfg: LlmConfig,
    http: reqwest::Client,
    dispatcher: Arc<Mutex<tools::ToolDispatcher>>,
    store: Arc<Mutex<session::Store>>,
    engine: Arc<Mutex<audio_engine::Engine>>,
    /// Per-agent conversation history. Phase 1 keeps this in memory;
    /// persistence comes later.
    conversation: Vec<Message>,
    /// Notification primitive for plan approval (M27). Shared with the
    /// Tauri `AppState` so the `approve_plan` command can fire it without
    /// acquiring the agent Mutex.
    pub(crate) plan_notify: Arc<tokio::sync::Notify>,
}

impl Agent {
    /// Build a new agent. `dispatcher`, `store`, and `engine` are
    /// reference-counted so the caller can keep using them concurrently
    /// (under their respective mutexes).
    pub fn new(
        cfg: LlmConfig,
        dispatcher: Arc<Mutex<tools::ToolDispatcher>>,
        store: Arc<Mutex<session::Store>>,
        engine: Arc<Mutex<audio_engine::Engine>>,
        plan_notify: Arc<tokio::sync::Notify>,
    ) -> Self {
        Self {
            cfg,
            http: reqwest::Client::new(),
            dispatcher,
            store,
            engine,
            conversation: Vec::new(),
            plan_notify,
        }
    }

    /// Single conversational turn. Streams the assistant's response,
    /// dispatches any tool calls (synchronously, against the
    /// dispatcher), and loops until the model emits a non-tool stop.
    ///
    /// `on_event` is called from the same task that drives the HTTP
    /// stream, in order; it should not block. The Tauri command layer
    /// pushes events into a `tauri::ipc::Channel` and returns
    /// immediately.
    pub async fn turn<F>(&mut self, user_message: String, on_event: F) -> Result<TurnResult>
    where
        F: FnMut(AgentEvent),
    {
        agent_loop::run_turn(
            &self.cfg,
            &self.http,
            &self.dispatcher,
            &self.store,
            &self.engine,
            &mut self.conversation,
            &self.plan_notify,
            user_message,
            on_event,
        )
        .await
    }

    /// Read-only access to the running conversation, for the UI to
    /// re-render history after a reload.
    pub fn conversation(&self) -> &[Message] {
        &self.conversation
    }

    /// Reset the conversation history. The next [`Agent::turn`] starts
    /// from a clean slate; the dispatcher / store / engine are
    /// untouched.
    pub fn reset(&mut self) {
        self.conversation.clear();
    }
}
