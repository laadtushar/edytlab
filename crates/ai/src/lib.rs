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
//! use ai::{Agent, AnthropicConfig};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let api_key = ai::keychain::load_api_key().expect("api key not configured");
//! let dispatcher = Arc::new(Mutex::new(tools::ToolDispatcher::default_dispatcher()));
//! let store = Arc::new(Mutex::new(session::Store::open(std::path::Path::new("/tmp/proj"))?));
//! let engine = Arc::new(Mutex::new(audio_engine::Engine::new()));
//!
//! let mut agent = Agent::new(
//!     AnthropicConfig::new(api_key),
//!     dispatcher,
//!     store,
//!     engine,
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
pub mod prompt;

use std::sync::{Arc, Mutex};

use anthropic::Message;

pub use prompt::{DEFAULT_BASE_URL, DEFAULT_MODEL, MAX_TOOL_CALLS_PER_TURN};

/// Configuration for the Anthropic client.
///
/// `base_url` is overridable so integration tests can point the agent
/// at a `wiremock` server without monkey-patching DNS or env vars.
#[derive(Debug, Clone)]
pub struct AnthropicConfig {
    /// API key, loaded from the OS keychain. Never logged.
    pub api_key: String,
    /// Anthropic model id; defaults to [`DEFAULT_MODEL`].
    pub model: String,
    /// Base URL (no trailing slash). Defaults to
    /// [`DEFAULT_BASE_URL`]; tests override.
    pub base_url: String,
}

impl AnthropicConfig {
    /// Build a config with the default model and base URL.
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            model: DEFAULT_MODEL.to_string(),
            base_url: DEFAULT_BASE_URL.to_string(),
        }
    }

    /// Override the model id.
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }

    /// Override the base URL (used by tests against `wiremock`).
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }
}

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
}

pub type Result<T> = std::result::Result<T, Error>;

/// The agent. Owns the HTTP client and conversation history; takes
/// shared references to the tool dispatcher, session store, and audio
/// engine so multiple agents (or other callers) can share them.
pub struct Agent {
    cfg: AnthropicConfig,
    http: reqwest::Client,
    dispatcher: Arc<Mutex<tools::ToolDispatcher>>,
    store: Arc<Mutex<session::Store>>,
    engine: Arc<Mutex<audio_engine::Engine>>,
    /// Per-agent conversation history. Phase 1 keeps this in memory;
    /// persistence comes later.
    conversation: Vec<Message>,
}

impl Agent {
    /// Build a new agent. `dispatcher`, `store`, and `engine` are
    /// reference-counted so the caller can keep using them concurrently
    /// (under their respective mutexes).
    pub fn new(
        cfg: AnthropicConfig,
        dispatcher: Arc<Mutex<tools::ToolDispatcher>>,
        store: Arc<Mutex<session::Store>>,
        engine: Arc<Mutex<audio_engine::Engine>>,
    ) -> Self {
        Self {
            cfg,
            http: reqwest::Client::new(),
            dispatcher,
            store,
            engine,
            conversation: Vec::new(),
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
