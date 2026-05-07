//! Application state shared across Tauri commands.
//!
//! Phase 1 design notes:
//!
//! * The store, dispatcher, and engine each live behind their own
//!   `Arc<Mutex<_>>` so individual commands can lock for the shortest
//!   possible scope. The same `Arc<Mutex<_>>` handles are passed to
//!   [`ai::Agent::new`] when the agent is constructed, which lets the
//!   agent and the command layer share state without copying it.
//! * The agent is held under a `tokio::sync::Mutex` because
//!   [`ai::Agent::turn`] is `async` and `send_message` holds the lock
//!   across `.await` points. Using `std::sync::Mutex` here would make
//!   the resulting future `!Send`, which Tauri's IPC dispatcher would
//!   reject.
//! * The agent and the store both start as `None`. The agent is built
//!   once both an API key is known *and* a project has been opened —
//!   either `set_api_key` or `open_project` will perform the
//!   construction when the second prerequisite arrives.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use ai::Agent;
use audio_engine::Engine;
use session::Store;
use tools::ToolDispatcher;

/// Shared, mutex-guarded application state. Cloning is cheap (`Arc`).
#[derive(Clone, Default)]
pub struct AppState {
    /// AI agent. `None` until the user provides an Anthropic API key
    /// *and* a project has been opened.
    pub agent: Arc<tokio::sync::Mutex<Option<Agent>>>,
    /// Tool dispatcher (Phase 1 default tool set). Constructed once at
    /// boot; never replaced.
    pub dispatcher: Arc<Mutex<ToolDispatcher>>,
    /// Session store. `None` until `open_project` is called. Wrapped in
    /// `Arc<Mutex<Store>>` so the same handle can be shared with the
    /// agent (which expects a non-`Option` `Arc<Mutex<Store>>`).
    pub store: Arc<Mutex<Option<Arc<Mutex<Store>>>>>,
    /// Audio engine. Stateless in Phase 1 but wrapped in `Arc<Mutex<_>>`
    /// to match the type the agent and tools expect.
    pub engine: Arc<Mutex<Engine>>,
    /// Currently-open project directory, if any.
    pub project_dir: Arc<Mutex<Option<PathBuf>>>,
    /// In-memory cache of the API key. We re-read from the OS keychain
    /// on demand; this exists so commands that need the key (agent
    /// construction) do not have to re-prompt the user.
    pub api_key: Arc<Mutex<Option<String>>>,
}

impl AppState {
    /// Build a fresh `AppState` with the default Phase-1 dispatcher and
    /// a new audio engine. The agent, store, and api key start empty.
    pub fn new() -> Self {
        Self {
            agent: Arc::new(tokio::sync::Mutex::new(None)),
            dispatcher: Arc::new(Mutex::new(ToolDispatcher::default_dispatcher())),
            store: Arc::new(Mutex::new(None)),
            engine: Arc::new(Mutex::new(Engine::new())),
            project_dir: Arc::new(Mutex::new(None)),
            api_key: Arc::new(Mutex::new(None)),
        }
    }

    /// Snapshot the currently-open store handle, if any. The returned
    /// `Arc<Mutex<Store>>` clones cheaply and can be passed to
    /// [`ai::Agent::new`].
    pub fn store_handle(&self) -> Option<Arc<Mutex<Store>>> {
        self.store
            .lock()
            .expect("store mutex poisoned")
            .as_ref()
            .map(Arc::clone)
    }

    /// Snapshot the currently-cached API key, if any.
    pub fn api_key_snapshot(&self) -> Option<String> {
        self.api_key.lock().expect("api_key mutex poisoned").clone()
    }

    /// Replace the cached API key with `key`.
    pub fn set_api_key_cache(&self, key: Option<String>) {
        *self.api_key.lock().expect("api_key mutex poisoned") = key;
    }

    /// Replace the open store with `handle`. Pass `None` to clear it.
    pub fn set_store(&self, handle: Option<Arc<Mutex<Store>>>) {
        *self.store.lock().expect("store mutex poisoned") = handle;
    }

    /// Replace the project directory.
    pub fn set_project_dir(&self, dir: Option<PathBuf>) {
        *self.project_dir.lock().expect("project_dir mutex poisoned") = dir;
    }
}
