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

use agent_profiles::ProfileLibrary;
use ai::Agent;
use audio_engine::Engine;
use mcp::{McpConfig, McpRegistry};
use memory::MemoryStore;
use session::Store;
use skills::SkillLibrary;
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
    /// Edited plan steps forwarded by the frontend when the user modifies
    /// one or more step descriptions before clicking Run. Set by
    /// `approve_plan` before it fires `plan_notify`; consumed and cleared
    /// by the agent loop immediately after `await_plan_approval` returns.
    /// `None` when the user approves without edits.
    pub plan_steps_override: Arc<Mutex<Option<String>>>,
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
    /// User-authored skill library. Reloaded via
    /// `reload_skills_from_disk` whenever the directory may have
    /// changed; reloads replace the library *in place* (under the
    /// existing `Mutex`) so every `Agent` that previously cloned this
    /// `Arc` continues to see the latest library on its next turn —
    /// no agent rebuild required.
    pub skills: Arc<Mutex<SkillLibrary>>,
    /// Absolute path of the global skills directory
    /// (`~/.edytlab/skills`). Resolved once at startup; subsequent
    /// reloads scan this path. Empty `PathBuf` until
    /// `install_skill_library` is called — production wires it in
    /// `lib.rs::setup`; tests using bare `AppState::new()` see an
    /// always-empty library.
    pub skills_dir: Arc<Mutex<PathBuf>>,
    /// Agent-profile library + directory, parallel to `skills` /
    /// `skills_dir`. Active profile selection rides on the
    /// `active_agent_profile_name` slot below. Same single-mutex
    /// reload-in-place model as `skills`.
    pub agent_profiles: Arc<Mutex<ProfileLibrary>>,
    pub agent_profiles_dir: Arc<Mutex<PathBuf>>,
    /// Name of the currently active profile. `None` means "no
    /// profile — use the global default agent". Mutated via
    /// `set_active_agent_profile` and persisted to a sidecar file
    /// (`<dir>/.active`) so the choice survives restart.
    pub active_agent_profile_name: Arc<Mutex<Option<String>>>,
    /// MCP server registration file (`~/.edytlab/mcp.json`) and the
    /// in-memory parsed config. Path is empty `PathBuf` until
    /// `install_mcp` is called.
    pub mcp_config: Arc<Mutex<McpConfig>>,
    pub mcp_config_path: Arc<Mutex<PathBuf>>,
    /// Runtime registry — owns running stdio clients and their
    /// last-known tool lists. `Arc` so the agent-loop (future
    /// follow-up) can hold a clone without going through `AppState`.
    pub mcp: Arc<McpRegistry>,
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
            plan_steps_override: Arc::new(Mutex::new(None)),
            selection: Arc::new(Mutex::new(None)),
            clipboard: Arc::new(Mutex::new(None)),
            memory: Arc::new(Mutex::new(None)),
            skills: Arc::new(Mutex::new(
                SkillLibrary::load_from(std::path::Path::new(""))
                    .expect("empty-dir skill library cannot fail"),
            )),
            skills_dir: Arc::new(Mutex::new(PathBuf::new())),
            agent_profiles: Arc::new(Mutex::new(
                ProfileLibrary::load_from(std::path::Path::new(""))
                    .expect("empty-dir profile library cannot fail"),
            )),
            agent_profiles_dir: Arc::new(Mutex::new(PathBuf::new())),
            active_agent_profile_name: Arc::new(Mutex::new(None)),
            mcp_config: Arc::new(Mutex::new(McpConfig::default())),
            mcp_config_path: Arc::new(Mutex::new(PathBuf::new())),
            mcp: Arc::new(McpRegistry::new()),
        }
    }

    /// Install the MCP config file path + initial load. Called once
    /// at startup from `lib.rs::run`.
    pub fn install_mcp(&self, path: PathBuf) {
        *self
            .mcp_config_path
            .lock()
            .expect("mcp_config_path mutex poisoned") = path.clone();
        match mcp::load_config(&path) {
            Ok(cfg) => {
                *self.mcp_config.lock().expect("mcp_config mutex poisoned") = cfg;
            }
            Err(e) => {
                tracing::warn!(error = ?e, "MCP config load failed; starting empty");
            }
        }
    }

    /// Save the current `mcp_config` back to disk.
    pub fn save_mcp_config(&self) -> mcp::Result<()> {
        let path = self
            .mcp_config_path
            .lock()
            .expect("mcp_config_path mutex poisoned")
            .clone();
        let cfg = self
            .mcp_config
            .lock()
            .expect("mcp_config mutex poisoned")
            .clone();
        mcp::save_config(&path, &cfg)
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

    /// Install the skill directory + initial load. Called once at
    /// startup from `lib.rs::run` after the home dir is resolved.
    pub fn install_skill_library(&self, skills_dir: PathBuf) {
        *self.skills_dir.lock().expect("skills_dir mutex poisoned") = skills_dir.clone();
        let lib = SkillLibrary::load_from(&skills_dir).unwrap_or_else(|e| {
            tracing::warn!(error = ?e, "skill library load failed; starting empty");
            SkillLibrary::load_from(std::path::Path::new(""))
                .expect("empty-dir library cannot fail")
        });
        *self.skills.lock().expect("skills mutex poisoned") = lib;
    }

    /// Snapshot the currently-installed skill library handle. Cheap
    /// to clone (`Arc`); passed to `Agent::with_skills` on rebuild.
    /// Reloads happen in place under the same `Mutex` so every clone
    /// sees the latest library on its next lock acquire — no agent
    /// rebuild required to pick up edits.
    pub fn skills_handle(&self) -> Arc<Mutex<SkillLibrary>> {
        Arc::clone(&self.skills)
    }

    /// Install the agent-profiles directory + initial load. Mirrors
    /// `install_skill_library`. Also restores the active-profile
    /// selection from `<dir>/.active` if present.
    pub fn install_agent_profile_library(&self, dir: PathBuf) {
        *self
            .agent_profiles_dir
            .lock()
            .expect("agent_profiles_dir mutex poisoned") = dir.clone();
        let lib = ProfileLibrary::load_from(&dir).unwrap_or_else(|e| {
            tracing::warn!(error = ?e, "agent-profile library load failed; starting empty");
            ProfileLibrary::load_from(std::path::Path::new(""))
                .expect("empty-dir profile library cannot fail")
        });
        *self
            .agent_profiles
            .lock()
            .expect("agent_profiles mutex poisoned") = lib;

        let sidecar = dir.join(".active");
        if let Ok(name) = std::fs::read_to_string(&sidecar) {
            let trimmed = name.trim();
            if !trimmed.is_empty() {
                *self
                    .active_agent_profile_name
                    .lock()
                    .expect("active profile mutex poisoned") = Some(trimmed.to_string());
            }
        }
    }

    /// Reload the agent-profile library from disk, in place.
    pub fn reload_agent_profiles_from_disk(
        &self,
    ) -> std::result::Result<(), agent_profiles::Error> {
        let dir = self
            .agent_profiles_dir
            .lock()
            .expect("agent_profiles_dir mutex poisoned")
            .clone();
        let lib = ProfileLibrary::load_from(&dir)?;
        *self
            .agent_profiles
            .lock()
            .expect("agent_profiles mutex poisoned") = lib;
        Ok(())
    }

    /// Snapshot the active profile name. `None` means "no profile —
    /// use the default agent".
    pub fn active_agent_profile(&self) -> Option<String> {
        self.active_agent_profile_name
            .lock()
            .expect("active profile mutex poisoned")
            .clone()
    }

    /// Persist the active-profile selection both in memory and to
    /// the sidecar file so it survives restart.
    pub fn set_active_agent_profile(&self, name: Option<String>) -> std::io::Result<()> {
        *self
            .active_agent_profile_name
            .lock()
            .expect("active profile mutex poisoned") = name.clone();
        let dir = self
            .agent_profiles_dir
            .lock()
            .expect("agent_profiles_dir mutex poisoned")
            .clone();
        let sidecar = dir.join(".active");
        match name {
            Some(n) => {
                std::fs::create_dir_all(&dir)?;
                std::fs::write(&sidecar, n)?;
            }
            None => {
                let _ = std::fs::remove_file(&sidecar);
            }
        }
        Ok(())
    }

    /// `(profile_body, tool_whitelist, model_override)` for the
    /// currently active profile, or `(None, None, None)` when no
    /// profile is active or the active name no longer exists on disk.
    /// Used by `rebuild_agent` to apply the profile.
    #[allow(clippy::type_complexity)]
    pub fn active_profile_overrides(
        &self,
    ) -> (
        Option<String>,
        Option<Vec<String>>,
        Option<(String, String)>,
    ) {
        let name = match self.active_agent_profile() {
            Some(n) => n,
            None => return (None, None, None),
        };
        let lib = self
            .agent_profiles
            .lock()
            .expect("agent_profiles mutex poisoned");
        match lib.find(&name) {
            Some(p) => (
                Some(agent_profiles::defang_body(&p.body)),
                p.tools.clone(),
                p.model.as_ref().map(|m| (m.provider.clone(), m.id.clone())),
            ),
            None => (None, None, None),
        }
    }

    /// Reload the skill library from `skills_dir`, replacing the
    /// library in place so any `Agent` that already cloned the handle
    /// sees the new content on its next turn.
    pub fn reload_skills_from_disk(&self) -> std::result::Result<(), skills::Error> {
        let dir = self
            .skills_dir
            .lock()
            .expect("skills_dir mutex poisoned")
            .clone();
        let lib = SkillLibrary::load_from(&dir)?;
        *self.skills.lock().expect("skills mutex poisoned") = lib;
        Ok(())
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

    /// Names of every tool registered in the dispatcher.
    pub fn all_tool_names(&self) -> Vec<String> {
        self.dispatcher
            .lock()
            .expect("dispatcher mutex poisoned")
            .tool_names()
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
