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
//!   either a provider save or `open_project` will perform the
//!   construction when the second prerequisite arrives.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use agent_profiles::ProfileLibrary;
use ai::Agent;
use audio_engine::Engine;
use mcp::{McpConfig, McpRegistry};
use memory::MemoryStore;
use session::{LiveStore, Store};
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
    /// Session store. `None` until `open_project` is called. A
    /// [`LiveStore`], which only the registry in `session` makes, so a store
    /// that is not the one live for its folder cannot be put here (#566): a
    /// project opened again later, or by a tool, finds this one and does not
    /// build a second writer on the folder. It is `Arc<Mutex<Store>>`
    /// underneath, shared with the agent (which expects a non-`Option`
    /// `Arc<Mutex<Store>>`: see [`AppState::store_handle`]).
    ///
    /// The registry also holds the stores that are alive but not here, such
    /// as the one a running turn keeps after the window has moved to another
    /// project (#501).
    pub store: Arc<Mutex<Option<LiveStore>>>,
    /// Audio engine. Stateless in Phase 1 but wrapped in `Arc<Mutex<_>>`
    /// to match the type the agent and tools expect.
    pub engine: Arc<Mutex<Engine>>,
    /// Currently-open project directory, if any.
    pub project_dir: Arc<Mutex<Option<PathBuf>>>,
    /// What the agent is built from: the active provider and its key, every
    /// provider's chosen model, and what startup found when it read the
    /// keychain. Owned by [`crate::agent_settings`] (ADR 0005).
    pub agent_settings: crate::agent_settings::AgentSettings,
    /// The plan gate (M27, ADR 0002): where a turn waits for the user to
    /// approve, revise or reject the plan card. The agent holds a clone;
    /// `approve_plan` and `reject_plan` answer through this one, never
    /// touching the agent Mutex, so there is no deadlock even though
    /// `send_message` holds the agent lock across its `.await` points.
    ///
    /// An answer is taken only while a card is open, so a stray Run click
    /// on a card that should have gone away is a no-op rather than a
    /// permit banked for the next turn (#251), and a turn that times out
    /// or is dropped closes it with no reset here.
    pub plan_gate: ai::PlanGate,
    /// Whether the user asked to see a plan before every turn. Kept on
    /// the state rather than in the agent so it survives an agent
    /// rebuild — which happens on every key, model or base-URL change.
    ///
    /// Also kept in the settings file, and read from it at launch
    /// ([`AppState::install_settings`]): it is an approval gate, so one
    /// that came back off after a restart would let edits through with no
    /// card (#569). A state nothing installed settings into starts off.
    /// Write it with [`AppState::save_plan_first`], not by storing to the
    /// atomic, or the next launch will not see the change.
    pub plan_first: Arc<std::sync::atomic::AtomicBool>,
    /// The app's settings file (`~/.edytlab/settings.json`). Empty until
    /// `install_settings` runs, as the other `install_*` paths are, so a
    /// bare `AppState::new()` keeps its settings in memory only.
    pub settings_path: Arc<Mutex<PathBuf>>,
    /// Current timeline selection, pushed from the frontend via
    /// `set_selection_context`. Read per-turn in `send_message` to build
    /// the `SessionContext` injected into the system prompt.
    pub selection: Arc<Mutex<Option<Range>>>,
    /// The A/B compare the window has open, which holds both sides'
    /// preview-cache entries out of eviction, and out of the cap, until
    /// `end_compare` (#575). Its lock is taken alone, never with the store,
    /// engine, dispatcher or agent locks; letting a pair go takes the preview
    /// cache's hold lock under it, which is the last lock there is.
    pub compare: Arc<Mutex<crate::compare::CompareHolds>>,
    /// The mix the window plays, whose preview-cache entry is held out of
    /// eviction, and out of the cap, until the window's next `render_preview`
    /// or a project change (#607). Filled only by that command, never by the
    /// agent's tools. Its lock is taken alone, never with the store, engine,
    /// dispatcher or agent locks; a hold is dropped after the lock is let go.
    pub window_mix: Arc<Mutex<crate::window_mix::WindowMix>>,
    /// In-memory audio clipboard for `copy_region` / `paste_region`.
    /// Shared with the `Agent` so the clipboard persists across turns and
    /// is accessible from both the tool layer and (future) IPC commands.
    pub clipboard: Arc<Mutex<Option<tools::Clipboard>>>,
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
    /// The ChatGPT plan's sign-in session and what the app remembers around
    /// it (#474). A handle with no session until [`AppState::install_chatgpt`]
    /// runs, so a bare `AppState::new()` is a state with ChatGPT unavailable.
    pub chatgpt: crate::chatgpt::ChatGptHandle,
    /// Opt-in crash reports (#958): off until the person turns them on in
    /// Settings, and inert in a build with no DSN. Read from the settings
    /// file with Plan first ([`AppState::install_settings`]); written with
    /// [`AppState::save_crash_reports`].
    pub crash_reports: Arc<crate::crash_reports::CrashReporter>,
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
            agent_settings: crate::agent_settings::AgentSettings::default(),
            plan_gate: ai::PlanGate::new(),
            plan_first: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            settings_path: Arc::new(Mutex::new(PathBuf::new())),
            selection: Arc::new(Mutex::new(None)),
            compare: Arc::new(Mutex::new(crate::compare::CompareHolds::default())),
            window_mix: Arc::new(Mutex::new(crate::window_mix::WindowMix::default())),
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
            chatgpt: crate::chatgpt::ChatGptHandle::default(),
            crash_reports: Arc::new(crate::crash_reports::CrashReporter::default()),
        }
    }

    /// Opens the ChatGPT session, whose files are in `<app_config_dir>/chatgpt`
    /// and whose refresh token is in the keychain slot `chatgpt_refresh_token`
    /// (#474). Reads `registration.json` and nothing else: no keychain read
    /// and no request. Called once at startup from `lib.rs::run`; a session
    /// that cannot be opened leaves ChatGPT unavailable and never stops the
    /// app starting. Returns whether it opened.
    ///
    /// The keychain is not asked after a startup it refused, as for an API
    /// key ([`crate::chatgpt::KeychainTokenStore`]).
    pub fn install_chatgpt(&self, app_config_dir: &std::path::Path) -> bool {
        use ai::chatgpt::{ChatGptSession, SessionConfig};

        let refusal: crate::chatgpt::Refusal = self.agent_settings.refusal();
        let tokens = Arc::new(crate::chatgpt::KeychainTokenStore::new(
            crate::chatgpt::OsTokenSlot,
            Arc::clone(&refusal),
        ));
        let session = match ChatGptSession::open(SessionConfig::new(app_config_dir), tokens.clone())
        {
            Ok(session) => Arc::new(session),
            Err(e) => {
                tracing::warn!(error = %e, "could not open the ChatGPT session");
                return false;
            }
        };
        self.chatgpt.install(
            session,
            crate::chatgpt::Hooking {
                persistence: Arc::new(move || tokens.persistence()),
                refusal,
                refresh_models: true,
                private_dir: Some(app_config_dir.to_path_buf()),
            },
        );
        true
    }

    /// Install the settings file path and read what it holds. Called once
    /// at startup from `lib.rs::run`, before the window can ask for
    /// anything, so the first message of the launch already knows whether
    /// Plan first is on (#569). A missing or unreadable file means the
    /// defaults, which is off, and never stops the app starting.
    ///
    /// Send crash reports (#958) is read here too, and applied: off (the
    /// default, and what a missing or corrupt file means) builds nothing; on
    /// builds the reporting client, which sends nothing until something goes
    /// wrong, and only in a build that has a DSN.
    pub fn install_settings(&self, path: PathBuf) {
        let plan_first = crate::settings::read_plan_first(&path);
        let crash_reports = crate::settings::read_crash_reports(&path);
        {
            let mut slot = self
                .settings_path
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            *slot = path;
            self.plan_first
                .store(plan_first, std::sync::atomic::Ordering::SeqCst);
        }
        self.crash_reports.set_enabled(crash_reports);
    }

    /// Turn Send crash reports on or off for this launch and record it for
    /// the next (#958), as [`AppState::save_plan_first`] does for Plan
    /// first: the choice applies whatever the disk does, and an `Err` means
    /// only that the next launch is in doubt.
    ///
    /// The choice is recorded under the settings lock, and applied after it
    /// is let go: turning reporting off waits (up to two seconds) for a
    /// report already queued, so call this off the async runtime's workers.
    pub fn save_crash_reports(&self, enabled: bool) -> std::io::Result<()> {
        // Two quick toggles must land in the same order on disk and in the
        // client, or the last choice saved could be "off" while a client
        // from an earlier "on" keeps sending. This lock orders write then
        // apply across callers. It is not the settings lock, which stays a
        // leaf: closing the client can wait up to two seconds.
        static ORDER: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _order = ORDER
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let saved = {
            let path = self
                .settings_path
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if path.as_os_str().is_empty() {
                Ok(())
            } else {
                crate::settings::write_crash_reports(&path, enabled)
            }
        };
        self.crash_reports.set_enabled(enabled);
        saved
    }

    /// Turn Plan first on or off for this launch and record it for the next.
    ///
    /// The running session takes the choice whatever the disk says: the
    /// gate does what the person asked for as long as the app is open. An
    /// `Err` means only that the next launch is in doubt, and the caller
    /// says so. With no settings path installed (a bare `AppState::new()`)
    /// nothing is written.
    ///
    /// Both happen under the settings lock, so two quick toggles land in
    /// the same order in memory and on disk. That lock is a leaf: taken
    /// alone, and nothing else is waited for while it is held.
    pub fn save_plan_first(&self, enabled: bool) -> std::io::Result<()> {
        let path = self
            .settings_path
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        self.plan_first
            .store(enabled, std::sync::atomic::Ordering::SeqCst);
        if path.as_os_str().is_empty() {
            return Ok(());
        }
        crate::settings::write_plan_first(&path, enabled)
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
                // Every failure but "it was not there" (#268).
                //
                // `let _ =` returned `Ok(())` whatever happened, so a
                // delete blocked by a read-only directory, a Windows
                // read-only attribute or a handle held by antivirus
                // reported success while the sidecar survived — and the
                // sidecar is the *sole* persistence for this selection.
                // The next launch read it back and silently re-applied
                // the profile, restoring a tool whitelist and a pinned
                // model/provider the user believed they had turned off.
                //
                // `NotFound` is genuinely fine: clearing a selection on
                // a fresh install has nothing to delete, and an
                // unconditional `?` would turn that into an error.
                match std::fs::remove_file(&sidecar) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e),
                }
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

    /// Snapshot the currently-open store handle, if any. The returned
    /// `Arc<Mutex<Store>>` clones cheaply and can be passed to
    /// [`ai::Agent::new`].
    pub fn store_handle(&self) -> Option<Arc<Mutex<Store>>> {
        self.store
            .lock()
            .expect("store mutex poisoned")
            .as_ref()
            .map(LiveStore::handle)
    }

    /// Make `store` the open project's store and `dir` its directory.
    ///
    /// `store` is the project's live store, as `session::Store::open_shared`
    /// gave it out. Nothing else is accepted: a [`LiveStore`] is made only
    /// by the registry, so an open of the folder later finds this store while
    /// anything holds it instead of building another (#566).
    ///
    /// The two are written together, holding the `store` slot's lock for
    /// both, so a reader that sees the new store sees the new directory
    /// too. The order is the `store` slot, then the `project_dir` slot; no
    /// code takes them the other way round.
    pub fn set_open_project(&self, store: LiveStore, dir: PathBuf) {
        // A crash report never names the folder (#958).
        self.crash_reports.redact_path(&dir, "<project>");
        {
            let mut slot = self.store.lock().expect("store mutex poisoned");
            *slot = Some(store);
            *self.project_dir.lock().expect("project_dir mutex poisoned") = Some(dir);
        }
        self.release_window_mix();
    }

    /// The window's mix is one of a project that is no longer the open one,
    /// and the window starts over (#607). Called with no other lock held,
    /// slots included, and the hold is dropped with the window-mix lock
    /// released too: a hold's drop evicts the old project's cache back under
    /// its cap.
    pub fn release_window_mix(&self) {
        let released = self
            .window_mix
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .release();
        drop(released);
    }

    /// Let go of the open project if its store is `stale` and nothing else
    /// holds that store, and say whether it did (#566).
    ///
    /// For a project whose folder was replaced at the same path: the store
    /// is of the old folder, and a new one cannot be opened for the path
    /// while it lives. `others` is how many handles besides this slot's and
    /// the caller's own `stale` the caller expects (the idle agent's, which
    /// it is about to drop). Any more than that is a command, a reader or a
    /// turn using the store, and then nothing is changed.
    ///
    /// The count is taken with the `store` slot's lock held, which every
    /// `store_handle()` takes to clone, so no new handle can appear between
    /// the count and the release. The two slots are written together, in the
    /// order [`AppState::set_open_project`] takes them. The window's mix is
    /// not touched here, because its lock is taken alone: the caller releases
    /// it ([`AppState::release_window_mix`]) once it holds no lock.
    pub fn release_open_project_if_exclusive(&self, stale: &LiveStore, others: usize) -> bool {
        let mut slot = self.store.lock().expect("store mutex poisoned");
        let Some(held) = slot.as_ref() else {
            return false;
        };
        if !held.ptr_eq(stale) || Arc::strong_count(held.as_arc()) != 2 + others {
            return false;
        }
        *slot = None;
        *self.project_dir.lock().expect("project_dir mutex poisoned") = None;
        true
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
    pub fn clipboard_handle(&self) -> Arc<Mutex<Option<tools::Clipboard>>> {
        Arc::clone(&self.clipboard)
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod active_profile_tests {
    //! Clearing the active agent profile has to be honest about failing
    //! (#268).
    //!
    //! The `.active` sidecar is the *sole* persistence for the
    //! selection, and `install_agent_profile_library` reads it back at
    //! startup. A clear that reported success while the file survived
    //! therefore re-applied the profile on the next launch — restoring
    //! a tool whitelist and a pinned model/provider the user believed
    //! they had turned off.

    use super::*;

    #[test]
    fn clearing_with_no_sidecar_succeeds() {
        // The fresh-install case, and the reason this cannot simply be
        // an unconditional `?`: there is nothing to delete.
        let dir = tempfile::tempdir().expect("tempdir");
        let state = AppState::new();
        state.install_agent_profile_library(dir.path().to_path_buf());

        state
            .set_active_agent_profile(None)
            .expect("clearing with nothing to delete is not a failure");
        assert_eq!(state.active_agent_profile(), None);
    }

    #[test]
    fn clearing_removes_the_sidecar() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = AppState::new();
        state.install_agent_profile_library(dir.path().to_path_buf());
        std::fs::write(dir.path().join(".active"), "editor").expect("seed sidecar");

        state.set_active_agent_profile(None).expect("clear");

        assert!(
            !dir.path().join(".active").exists(),
            "the selection must not survive on disk, or the next launch \
             re-applies it"
        );
    }

    /// A delete that cannot succeed must be reported, not swallowed.
    ///
    /// The sidecar is a *directory* here rather than a read-only file:
    /// `remove_file` refuses a directory on every platform, and unlike
    /// a permission trick it still fails when the tests run as root —
    /// which they do in this project's containers.
    #[test]
    fn a_failing_delete_is_reported_rather_than_reported_as_success() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = AppState::new();
        state.install_agent_profile_library(dir.path().to_path_buf());
        std::fs::create_dir(dir.path().join(".active")).expect("undeletable sidecar");

        let err = state
            .set_active_agent_profile(None)
            .expect_err("a delete that did not happen must not report Ok");

        assert_ne!(
            err.kind(),
            std::io::ErrorKind::NotFound,
            "NotFound is the one kind that is genuinely fine; this is not it"
        );
    }

    /// The in-memory selection still clears even when the disk write
    /// fails — the running session behaves as the user asked, and only
    /// the restart was ever wrong.
    #[test]
    fn the_in_memory_selection_clears_even_when_the_delete_fails() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = AppState::new();
        state.install_agent_profile_library(dir.path().to_path_buf());
        std::fs::create_dir(dir.path().join(".active")).expect("undeletable sidecar");

        let _ = state.set_active_agent_profile(None);

        assert_eq!(state.active_agent_profile(), None);
    }
}

#[cfg(test)]
mod plan_first_settings_tests {
    //! Plan first survives a restart (#569).
    //!
    //! It is an approval gate: with it on, no edit runs until the person
    //! approves it. It lived only in memory, so a person who turned it on
    //! to protect a session and then restarted got a composer with the
    //! toggle off and edits that applied with no card. These build a state
    //! the way `run` does, from a settings location, drop it as a quit
    //! does, and build another from the same location.

    use super::*;

    fn is_on(state: &AppState) -> bool {
        state.plan_first.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// A launch: a new state reading the settings at `path`.
    fn launch(path: &std::path::Path) -> AppState {
        let state = AppState::new();
        state.install_settings(path.to_path_buf());
        state
    }

    /// Where `run` puts it. Its folder does not exist yet on a fresh
    /// install, and saving has to cope.
    fn settings_in(home: &tempfile::TempDir) -> PathBuf {
        home.path().join(".edytlab").join("settings.json")
    }

    #[test]
    fn a_fresh_install_starts_with_plan_first_off() {
        let home = tempfile::tempdir().unwrap();
        assert!(!is_on(&launch(&settings_in(&home))));
    }

    /// The bug.
    #[test]
    fn plan_first_turned_on_is_still_on_in_the_next_launch() {
        let home = tempfile::tempdir().unwrap();
        let path = settings_in(&home);

        let first = launch(&path);
        first.save_plan_first(true).expect("saved");
        assert!(is_on(&first));
        drop(first);

        assert!(is_on(&launch(&path)));
    }

    #[test]
    fn turning_it_off_again_is_kept_too() {
        let home = tempfile::tempdir().unwrap();
        let path = settings_in(&home);
        let first = launch(&path);
        first.save_plan_first(true).expect("saved");
        first.save_plan_first(false).expect("saved");
        drop(first);

        assert!(!is_on(&launch(&path)));
    }

    /// A hand-edited or half-written file must not stop the app starting,
    /// and must not leave the person unable to choose: the next choice
    /// replaces it. Off, because that is what a fresh install does.
    #[test]
    fn a_corrupt_settings_file_starts_off_and_the_next_choice_replaces_it() {
        let home = tempfile::tempdir().unwrap();
        let path = settings_in(&home);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{ \"plan_first\": tru").unwrap();

        let state = launch(&path);
        assert!(!is_on(&state));

        state
            .save_plan_first(true)
            .expect("saved over the corrupt file");
        drop(state);
        assert!(is_on(&launch(&path)));
    }

    /// A file that is there and cannot be read (here a directory, which
    /// fails the same way on every platform and still fails as root) is
    /// not a reason to refuse to start either.
    #[test]
    fn a_settings_file_that_cannot_be_read_starts_off() {
        let home = tempfile::tempdir().unwrap();
        let path = settings_in(&home);
        std::fs::create_dir_all(&path).unwrap();

        assert!(!is_on(&launch(&path)));
    }

    /// A save that fails is reported, and the choice still applies to the
    /// running session: the gate does what the person asked for as long as
    /// the app is open, and only the restart is in doubt.
    #[test]
    fn a_save_that_cannot_be_written_is_reported_and_still_applies_for_this_launch() {
        let home = tempfile::tempdir().unwrap();
        // Its folder is a file, so the settings cannot be created.
        let blocker = home.path().join(".edytlab");
        std::fs::write(&blocker, "not a folder").unwrap();
        let state = launch(&blocker.join("settings.json"));

        state
            .save_plan_first(true)
            .expect_err("the file cannot be written, and that must not read as saved");

        assert!(is_on(&state));
    }

    /// A state nothing installed settings into (every test, the CLI) keeps
    /// the choice in memory and touches no disk.
    #[test]
    fn without_a_settings_location_the_choice_is_kept_in_memory_only() {
        let state = AppState::new();
        state
            .save_plan_first(true)
            .expect("nothing to write, nothing to fail");
        assert!(is_on(&state));
        assert!(!is_on(&AppState::new()));
    }

    /// Two quick toggles must not leave the disk holding the first while
    /// memory holds the second: after a restart the person would get the
    /// opposite of what they last saw.
    #[test]
    fn racing_saves_leave_the_disk_and_memory_agreeing() {
        let home = tempfile::tempdir().unwrap();
        let path = settings_in(&home);
        let state = launch(&path);

        let threads: Vec<_> = (0..16)
            .map(|i| {
                let state = state.clone();
                std::thread::spawn(move || state.save_plan_first(i % 2 == 0).expect("saved"))
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }

        assert_eq!(is_on(&launch(&path)), is_on(&state));
    }
}
