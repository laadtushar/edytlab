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
use memory::MemoryStore;
use session::Store;
use tools::{Range, ToolDispatcher};

/// Shared, mutex-guarded application state. Cloning is cheap (`Arc`).
#[derive(Clone)]
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
    /// In-memory cache of the API key for the *currently active*
    /// provider. We re-read from the OS keychain on demand; this exists
    /// so commands that need the key (agent construction) do not have to
    /// re-prompt the user. When the user switches the active provider we
    /// reload this slot from the new provider's keychain entry.
    pub api_key: Arc<Mutex<Option<String>>>,
    /// Stable id of the active provider (e.g. `"anthropic"` or
    /// `"openrouter"`). Defaults to `"anthropic"` when no preference is
    /// recorded — matches the pre-multi-provider behaviour.
    pub active_provider: Arc<Mutex<String>>,
    /// Per-provider model id selected by the user. Surfaced via
    /// `set_active_model`/`get_active_model`. The Settings UI persists
    /// the selection to localStorage and pushes it down here so the
    /// next `rebuild_agent` builds the LlmConfig with the chosen model.
    pub active_model_by_provider: Arc<Mutex<std::collections::HashMap<String, String>>>,
    /// Plan-approval signal for mashup mode (M27). The agent turn loop
    /// waits on this notifier; the `approve_plan` command fires it
    /// directly — without ever touching the agent Mutex — so there is no
    /// deadlock even though `send_message` holds the agent lock across its
    /// `.await` points.
    pub plan_notify: Arc<tokio::sync::Notify>,
    /// Current timeline selection, pushed from the frontend via
    /// `set_selection_context`. Read per-turn in `send_message` to build
    /// the `SessionContext` injected into the system prompt.
    pub selection: Arc<Mutex<Option<Range>>>,
    /// In-memory audio clipboard for `copy_region` / `paste_region`.
    /// Shared with the `Agent` so the clipboard persists across turns and
    /// is accessible from both the tool layer and (future) IPC commands.
    pub clipboard: Arc<Mutex<Option<Vec<f32>>>>,
    /// User memory (global + project) — system-prompt fragment surface.
    /// Shares `project_dir` with `AppState` so the project file
    /// resolves correctly without a rebuild on `open_project`. Built
    /// once at startup via `install_memory_store`.
    pub memory: Arc<Mutex<Option<Arc<MemoryStore>>>>,
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
            active_provider: Arc::new(Mutex::new(ai::ANTHROPIC_ID.to_string())),
            active_model_by_provider: Arc::new(Mutex::new(std::collections::HashMap::new())),
            plan_notify: Arc::new(tokio::sync::Notify::new()),
            selection: Arc::new(Mutex::new(None)),
            clipboard: Arc::new(Mutex::new(None)),
            memory: Arc::new(Mutex::new(None)),
        }
    }

    /// Install the memory store. Called once at startup from
    /// `lib.rs::run` after the app data directory is resolved. Shares
    /// the same `project_dir` `Arc` as `AppState` so reads / writes
    /// pick up `open_project` immediately, without a memory-store
    /// rebuild.
    pub fn install_memory_store(&self, global_memory_path: PathBuf) {
        let store = Arc::new(MemoryStore::new(
            global_memory_path,
            Arc::clone(&self.project_dir),
        ));
        *self.memory.lock().expect("memory mutex poisoned") = Some(store);
    }

    /// Snapshot the installed memory store, if any.
    pub fn memory_handle(&self) -> Option<Arc<MemoryStore>> {
        self.memory
            .lock()
            .expect("memory mutex poisoned")
            .as_ref()
            .map(Arc::clone)
    }

    /// Snapshot the active provider id. Defaults to `"anthropic"` when
    /// no preference is recorded — matches the pre-multi-provider build.
    pub fn active_provider_id(&self) -> String {
        self.active_provider
            .lock()
            .expect("active_provider mutex poisoned")
            .clone()
    }

    /// Replace the active provider id.
    pub fn set_active_provider(&self, id: String) {
        *self
            .active_provider
            .lock()
            .expect("active_provider mutex poisoned") = id;
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

    /// Snapshot the model id selected for `provider_id`, if any.
    pub fn model_for(&self, provider_id: &str) -> Option<String> {
        self.active_model_by_provider
            .lock()
            .expect("active_model_by_provider mutex poisoned")
            .get(provider_id)
            .cloned()
    }

    /// Persist the model id chosen for `provider_id`.
    pub fn set_model_for(&self, provider_id: String, model: String) {
        self.active_model_by_provider
            .lock()
            .expect("active_model_by_provider mutex poisoned")
            .insert(provider_id, model);
    }

    /// Replace the current timeline selection. Pass `None` to clear.
    pub fn set_selection(&self, sel: Option<Range>) {
        *self.selection.lock().expect("selection mutex poisoned") = sel;
    }

    /// Snapshot the current timeline selection.
    pub fn selection_snapshot(&self) -> Option<Range> {
        *self.selection.lock().expect("selection mutex poisoned")
    }

    /// Clone the clipboard `Arc` handle so callers (e.g. `rebuild_agent`)
    /// can share the same clipboard instance with the `Agent`.
    pub fn clipboard_handle(&self) -> Arc<Mutex<Option<Vec<f32>>>> {
        Arc::clone(&self.clipboard)
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}
