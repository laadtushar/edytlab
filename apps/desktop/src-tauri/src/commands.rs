//! Tauri command handlers wiring the frontend to the Phase 1 core.
//!
//! Each command converts its internal error type to `String` at the
//! IPC boundary because Tauri's command return values must serialise
//! straightforwardly. Internal errors flow through [`CommandError`] so
//! the call sites stay readable; a single `From` impl per source error
//! keeps the conversion noise out of the command bodies.
//!
//! State invariant: the agent is built lazily once both an API key and
//! a project store exist. `set_api_key` and `open_project` are the two
//! entry points that may trigger that construction; the helper
//! [`rebuild_agent`] centralises the logic so the two commands cannot
//! drift.

use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use session::{NodeId, SessionNode, Store};
use tauri::{AppHandle, Emitter, Manager, Runtime, State};
use tools::Range;

use crate::events::{
    DonePayload, NodeCreatedPayload, PlanPayload, TextDeltaPayload, ToolCallEndPayload,
    ToolCallPayload, DONE, NODE_CREATED, PLAN, PLAN_REJECTED, TEXT_DELTA, TOOL_CALL, TOOL_CALL_END,
};
use crate::state::AppState;

/// Information about an opened project, returned by `open_project` to
/// the frontend.
///
/// Keep this serializable shape stable: the corresponding TypeScript
/// interface in `apps/desktop/src/lib/tauri-bridge.ts` is
/// hand-mirrored. The integration test
/// `tests/commands_mock.rs::open_project_via_ipc_returns_project_info`
/// pins the JSON layout (`{path, head}`) so accidental drift trips CI.
#[derive(Debug, Clone, Serialize)]
pub struct ProjectInfo {
    /// Absolute path of the project directory, as a string (PathBuf
    /// would serialise to the same shape on Unix but Windows paths
    /// containing non-UTF8 bytes are rejected — Phase 1 only supports
    /// UTF-8 project paths).
    pub path: String,
    /// Hex-encoded session head, or `None` if the store is empty.
    pub head: Option<String>,
}

/// Internal error type for the commands layer. Converts to `String`
/// at the IPC boundary because Tauri's return values must implement
/// `Serialize`, and `Display`-derived strings carry exactly as much
/// information as we want surfaced to the frontend.
#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    #[error("session error: {0}")]
    Session(#[from] session::Error),

    #[error("audio engine error: {0}")]
    Engine(#[from] audio_engine::Error),

    #[error("ai error: {0}")]
    Ai(#[from] ai::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("keychain error: {0}")]
    Keychain(#[from] keyring::Error),

    #[error("invalid node id: {0}")]
    InvalidNodeId(String),

    #[error("no session loaded; call open_project first")]
    NoSession,

    #[error("no agent configured; call set_api_key first")]
    NoAgent,

    #[error("invalid path: {0}")]
    InvalidPath(String),

    #[error("invalid marker: {0}")]
    InvalidMarker(String),

    #[error("invalid track control: {0}")]
    InvalidTrackControl(String),

    #[error("internal: a state mutex was poisoned ({0})")]
    Poisoned(&'static str),

    #[error("memory error: {0}")]
    Memory(#[from] memory::Error),

    #[error("invalid memory scope: {0}")]
    InvalidMemoryScope(String),

    #[error("memory store not initialised")]
    NoMemory,

    #[error("skills directory not initialised")]
    NoSkillsDir,

    #[error("invalid skill: {0}")]
    InvalidSkill(String),

    #[error("agent profiles directory not initialised")]
    NoAgentProfilesDir,

    #[error("invalid agent profile: {0}")]
    InvalidAgentProfile(String),

    #[error("mcp error: {0}")]
    Mcp(#[from] mcp::McpError),

    #[error("invalid mcp server: {0}")]
    InvalidMcpServer(String),
}

impl From<CommandError> for String {
    fn from(value: CommandError) -> Self {
        value.to_string()
    }
}

type CmdResult<T> = std::result::Result<T, String>;

/// Convenience: lock a `std::sync::Mutex` and turn poisoning into a
/// `CommandError` so we never `unwrap()` outside test code.
fn lock_std<'a, T>(
    mu: &'a Mutex<T>,
    label: &'static str,
) -> Result<std::sync::MutexGuard<'a, T>, CommandError> {
    mu.lock().map_err(|_| CommandError::Poisoned(label))
}

// ---------------------------------------------------------------------------
// open_project
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn open_project(state: State<'_, AppState>, path: String) -> CmdResult<ProjectInfo> {
    let project_path = PathBuf::from(&path);
    let info = open_project_inner(&state, project_path)?;
    // After the store is replaced we may now be able to construct the
    // agent (if an API key was already cached). Rebuild it eagerly.
    rebuild_agent(&state).await?;
    Ok(info)
}

// ---------------------------------------------------------------------------
// project metadata, view state and recents (#156)
// ---------------------------------------------------------------------------

/// `~/.edytlab`, where things that belong to no single project live.
///
/// Resolved from the OS home directory rather than from Tauri's path
/// resolver so these commands stay callable from tests, which have no
/// `App`. Same location either way.
fn edytlab_home() -> PathBuf {
    #[allow(deprecated)]
    std::env::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

/// The project currently open, or an error naming what to do instead.
fn current_project_dir(state: &AppState) -> Result<PathBuf, CommandError> {
    let dir = lock_std(&state.project_dir, "project_dir")?;
    dir.clone().ok_or(CommandError::NoSession)
}

/// Metadata for the open project: its name, when it was created, notes.
#[tauri::command]
pub fn get_project_meta(state: State<'_, AppState>) -> CmdResult<crate::project::ProjectMeta> {
    let dir = current_project_dir(&state)?;
    Ok(crate::project::read_meta(&dir))
}

/// Rename the open project. The name is the project's own — a folder
/// path is not a name, which is the gap #156 describes.
#[tauri::command]
pub fn set_project_meta(
    state: State<'_, AppState>,
    name: String,
    notes: Option<String>,
) -> CmdResult<crate::project::ProjectMeta> {
    let dir = current_project_dir(&state)?;
    if name.trim().is_empty() {
        return Err(CommandError::InvalidPath("a project name cannot be empty".into()).into());
    }
    let mut meta = crate::project::read_meta(&dir);
    meta.name = name.trim().to_string();
    if let Some(n) = notes {
        meta.notes = n;
    }
    crate::project::write_meta(&dir, &meta).map_err(CommandError::from)?;
    // Keep the recents row in step, or the list shows the old name
    // until the project is next opened.
    let _ = crate::project::push_recent(
        &edytlab_home(),
        crate::project::RecentProject {
            path: dir.to_string_lossy().to_string(),
            name: meta.name.clone(),
            last_opened_at: meta.last_opened_at.clone(),
        },
    );
    Ok(meta)
}

/// Where the user was: head, zoom, selection, playhead.
#[tauri::command]
pub fn get_view_state(state: State<'_, AppState>) -> CmdResult<crate::project::ViewState> {
    let dir = current_project_dir(&state)?;
    Ok(crate::project::read_view(&dir))
}

/// Remember where the user is, so reopening resumes rather than
/// restarts. Disposable by design — losing it costs a scroll.
#[tauri::command]
pub fn save_view_state(
    state: State<'_, AppState>,
    view: crate::project::ViewState,
) -> CmdResult<()> {
    let dir = current_project_dir(&state)?;
    crate::project::write_view(&dir, &view).map_err(CommandError::from)?;
    Ok(())
}

/// Copy the open project to `dest` and continue in the copy.
///
/// Save As, in the sense a DAW means it: the work carries on in the new
/// place, and the original is left as it was at this moment. That is
/// only honest because a project folder now contains its own audio
/// (#156) — before the storage layout moved, a copy would have been a
/// history pointing at files somewhere else.
#[tauri::command]
pub async fn save_project_as(
    state: State<'_, AppState>,
    dest: String,
) -> CmdResult<crate::project::CopyReport> {
    let src = current_project_dir(&state)?;
    let dest_path = PathBuf::from(&dest);
    if !dest_path.is_absolute() {
        return Err(
            CommandError::InvalidPath(format!("expected absolute path, got `{dest}`")).into(),
        );
    }
    if dest_path == src {
        return Err(
            CommandError::InvalidPath("that is where the project already is".into()).into(),
        );
    }

    let report = crate::project::copy_project(&src, &dest_path).map_err(CommandError::Io)?;

    // Continue in the copy. Leaving the original open would make Save As
    // a backup button, which is a different feature with a different
    // name.
    open_project_inner(&state, dest_path)?;
    Ok(report)
}

/// Projects this machine has opened, most recent first.
///
/// Entries whose folder is gone are dropped on read: a recents list
/// full of dead rows is worse than a short one.
#[tauri::command]
pub fn list_recent_projects() -> CmdResult<Vec<crate::project::RecentProject>> {
    let home = edytlab_home();
    let all = crate::project::read_recents(&home);
    let live: Vec<_> = all
        .iter()
        .filter(|r| std::path::Path::new(&r.path).is_dir())
        .cloned()
        .collect();
    if live.len() != all.len() {
        // Rewrite so the pruning sticks rather than repeating every
        // time the list is shown.
        for dead in all
            .iter()
            .filter(|r| !std::path::Path::new(&r.path).is_dir())
        {
            let _ = crate::project::forget_recent(&home, &dead.path);
        }
    }
    Ok(live)
}

/// Remove a project from the list without touching the project itself.
#[tauri::command]
pub fn forget_recent_project(path: String) -> CmdResult<Vec<crate::project::RecentProject>> {
    crate::project::forget_recent(&edytlab_home(), &path).map_err(|e| CommandError::Io(e).into())
}

fn open_project_inner(state: &AppState, path: PathBuf) -> Result<ProjectInfo, CommandError> {
    if !path.is_absolute() {
        return Err(CommandError::InvalidPath(format!(
            "expected absolute path, got `{}`",
            path.display()
        )));
    }
    let store = Store::open(&path)?;
    let head_hex = store.head().map(|id| id.to_hex());
    let path_str = path
        .to_str()
        .ok_or_else(|| CommandError::InvalidPath("project path is not valid UTF-8".into()))?
        .to_string();

    // A project is now a thing with a name and a history of being
    // opened (#156), not just a folder that happens to contain a store.
    // Neither write is allowed to stop the project opening: a read-only
    // folder still has readable audio in it.
    let now = chrono::Utc::now().to_rfc3339();
    let meta = crate::project::touch_opened(&path, &now);
    let _ = crate::project::push_recent(
        &edytlab_home(),
        crate::project::RecentProject {
            path: path_str.clone(),
            name: meta.name,
            last_opened_at: Some(now),
        },
    );

    state.set_store(Some(Arc::new(Mutex::new(store))));
    state.set_project_dir(Some(path));

    Ok(ProjectInfo {
        path: path_str,
        head: head_hex,
    })
}

// ---------------------------------------------------------------------------
// set_api_key
// ---------------------------------------------------------------------------

/// Persist `key` for the active provider, refresh the in-memory cache,
/// and rebuild the agent. Equivalent to
/// [`set_api_key_for`] with the active provider id; preserved as a thin
/// shim so existing tests / front-end callers that only know about
/// "the API key" continue to work.
#[tauri::command]
pub async fn set_api_key(state: State<'_, AppState>, key: String) -> CmdResult<()> {
    let provider_id = state.active_provider_id();
    set_api_key_for_inner(&state, &provider_id, &key).await
}

/// Persist `key` for `provider`, mark `provider` as active, and rebuild
/// the agent. The frontend uses this when the user picks a provider in
/// the Settings picker and saves a key against it.
#[tauri::command]
pub async fn set_api_key_for(
    state: State<'_, AppState>,
    provider: String,
    key: String,
) -> CmdResult<()> {
    set_api_key_for_inner(&state, &provider, &key).await
}

// ---------------------------------------------------------------------------
// Base URL override
// ---------------------------------------------------------------------------

/// Read a provider's base-URL override, or `None` when it uses the
/// built-in one.
#[tauri::command]
pub async fn get_base_url_for(provider: String) -> CmdResult<Option<String>> {
    Ok(ai::keychain::load_base_url(&provider))
}

/// The URL a provider ships with, so the UI can show it as a placeholder
/// rather than hardcoding six strings that would drift.
#[tauri::command]
pub async fn default_base_url_for(provider: String) -> CmdResult<String> {
    Ok(ai::validate::provider_for(&provider).base_url().to_string())
}

/// Point a provider somewhere else — a local server on another port, a
/// gateway, a proxy, anything speaking the same API.
///
/// An empty string clears the override and restores the built-in URL,
/// which is what the UI sends when the field is emptied.
///
/// The URL is validated here rather than at request time. A typo saved
/// now would otherwise surface much later as a connection error with no
/// hint that the cause was a malformed setting.
#[tauri::command]
pub async fn set_base_url_for(
    state: State<'_, AppState>,
    provider: String,
    base_url: String,
) -> CmdResult<()> {
    let trimmed = base_url.trim();
    if trimmed.is_empty() {
        ai::keychain::delete_base_url(&provider).map_err(CommandError::from)?;
    } else {
        let rest = trimmed
            .strip_prefix("http://")
            .or_else(|| trimmed.strip_prefix("https://"))
            .ok_or_else(|| {
                format!("base URL must start with http:// or https:// — got `{trimmed}`")
            })?;
        if rest.is_empty() || rest.starts_with('/') {
            return Err(format!("base URL has no host — got `{trimmed}`"));
        }
        // A trailing slash turns `{base}/v1/models` into `{base}//v1/models`
        // on some servers. Normalising once here beats every call site
        // remembering.
        ai::keychain::save_base_url(&provider, trimmed.trim_end_matches('/'))
            .map_err(CommandError::from)?;
    }
    // The agent holds its endpoint for the life of the config, so a
    // saved URL that never reaches it looks like the setting was ignored.
    rebuild_agent(state.inner()).await?;
    Ok(())
}

async fn set_api_key_for_inner(state: &AppState, provider_id: &str, key: &str) -> CmdResult<()> {
    // Selecting a keyless provider is a save with an empty key — that is
    // how the UI says "make this one active" — so it must be allowed
    // through rather than rejected as a blank credential.
    if ai::validate::provider_for(provider_id).requires_api_key() && key.trim().is_empty() {
        return Err("api key must not be empty".into());
    }
    ai::keychain::save_api_key(provider_id, key).map_err(CommandError::from)?;
    // Saving a key for a provider implicitly makes it active — that
    // matches the user's intent and avoids a second "switch provider"
    // step in the settings UI.
    ai::keychain::save_active_provider(provider_id).map_err(CommandError::from)?;
    state.set_active_provider(provider_id.to_string());
    state.set_api_key_cache(Some(key.to_string()));
    rebuild_agent(state).await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// has_api_key / clear_api_key / test_api_key
// ---------------------------------------------------------------------------

/// Whether the OS keychain holds an API key for the active provider.
///
/// Used by the frontend on mount to decide whether to render the M13
/// blocking-modal first-launch flow. We deliberately re-read from the
/// keychain rather than trusting the in-memory cache so the answer
/// reflects the actual on-disk state — `clear_api_key` mutates the
/// keychain and we want subsequent `has_api_key` calls to immediately
/// see "no key" without needing the cache to also be cleared in lockstep.
#[tauri::command]
pub async fn has_api_key(state: State<'_, AppState>) -> CmdResult<bool> {
    let provider_id = state.active_provider_id();
    Ok(provider_is_configured(&provider_id))
}

/// Whether `provider_id` is ready to use.
///
/// For a keyless provider that is unconditionally true: there is no
/// credential to store, so asking the keychain would answer "no" forever
/// and leave first launch stuck behind a key prompt it can never satisfy.
fn provider_is_configured(provider_id: &str) -> bool {
    !ai::validate::provider_for(provider_id).requires_api_key()
        || ai::keychain::load_api_key(provider_id).is_some()
}

/// Whether a key is stored for `provider`. The Settings UI uses this
/// when the user toggles the provider picker, so we can show "configured"
/// vs "needs a key" without forcing a save.
#[tauri::command]
pub async fn has_api_key_for(provider: String) -> CmdResult<bool> {
    Ok(provider_is_configured(&provider))
}

/// Remove the stored API key for the active provider and tear down the
/// agent. Equivalent to [`clear_api_key_for`] with the active provider.
///
/// After this call the app must behave as if it had just launched with
/// no key configured for the active provider — the M13 acceptance
/// criterion #3 says clearing the key returns the app to the
/// first-launch state without restart.
#[tauri::command]
pub async fn clear_api_key(state: State<'_, AppState>) -> CmdResult<()> {
    let provider_id = state.active_provider_id();
    clear_api_key_for_inner(&state, &provider_id).await
}

#[tauri::command]
pub async fn clear_api_key_for(state: State<'_, AppState>, provider: String) -> CmdResult<()> {
    clear_api_key_for_inner(&state, &provider).await
}

async fn clear_api_key_for_inner(state: &AppState, provider_id: &str) -> CmdResult<()> {
    ai::keychain::delete_api_key(provider_id).map_err(CommandError::from)?;
    if state.active_provider_id() == provider_id {
        state.set_api_key_cache(None);
        rebuild_agent(state).await?;
    }
    Ok(())
}

/// Probe an API key with a 1-token Messages call against the *active*
/// provider's endpoint. Used by the Settings "Test" button. The key is
/// *not* persisted by this command — that's `set_api_key`'s job.
/// Validation runs in Rust so the proposed key never has to leave the
/// trusted process.
///
/// On 200 returns `Ok(())`; on any other response or transport failure
/// returns `Err("<status> <body>")` (e.g. `"401 invalid x-api-key"`),
/// matching M13 acceptance criterion #2.
#[tauri::command]
pub async fn test_api_key(state: State<'_, AppState>, key: String) -> CmdResult<()> {
    let provider_id = state.active_provider_id();
    ai::validate::test_api_key_for(&provider_id, &key).await
}

/// Mirror of [`ai::validate::ProbeReport`] for the IPC boundary. Kept
/// as its own struct so the TS bindings stay self-contained, in the
/// same way [`ModelInfoDto`] does.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeReportDto {
    pub model: String,
    pub tools_ok: bool,
    pub detail: Option<String>,
}

impl From<ai::validate::ProbeReport> for ProbeReportDto {
    fn from(r: ai::validate::ProbeReport) -> Self {
        Self {
            model: r.model,
            tools_ok: r.tools_ok,
            detail: r.detail,
        }
    }
}

/// Probe an API key against `provider`'s endpoint. The Settings UI
/// uses this when the user picks a provider — the test should be run
/// against the chosen provider, not the currently-active one.
///
/// `base_url` and `model` are the values currently on screen. They are
/// passed rather than read from the keychain because TEST exists to
/// check settings *before* they are saved; probing the saved endpoint
/// would report on a server the user is not about to use.
///
/// `Err` still means unreachable or rejected. `Ok` now distinguishes
/// two states via `tools_ok`: a model that calls tools, and one that
/// connects but will fail on the first edit.
#[tauri::command]
pub async fn test_api_key_for(
    provider: String,
    key: String,
    base_url: Option<String>,
    model: Option<String>,
) -> CmdResult<ProbeReportDto> {
    ai::validate::probe_provider_for(&provider, &key, base_url.as_deref(), model.as_deref())
        .await
        .map(ProbeReportDto::from)
}

// ---------------------------------------------------------------------------
// Provider selection
// ---------------------------------------------------------------------------

/// Stable list of provider ids the app supports. Surfaces directly to
/// the frontend; mirror this in the Settings picker.
#[tauri::command]
pub async fn list_providers() -> CmdResult<Vec<String>> {
    Ok(ai::SUPPORTED_PROVIDER_IDS
        .iter()
        .map(|s| (*s).to_string())
        .collect())
}

/// The active provider id. Defaults to `"anthropic"` for back-compat
/// when no preference has been recorded yet.
#[tauri::command]
pub async fn get_active_provider(state: State<'_, AppState>) -> CmdResult<String> {
    Ok(state.active_provider_id())
}

/// Switch the active provider. Persists the choice and rebuilds the
/// agent against the new provider's stored key (if any). Does NOT
/// require a key to already be present; the frontend uses this when the
/// user toggles the picker before saving a key.
#[tauri::command]
pub async fn set_active_provider(state: State<'_, AppState>, provider: String) -> CmdResult<()> {
    if !ai::SUPPORTED_PROVIDER_IDS.iter().any(|p| *p == provider) {
        return Err(format!("unsupported provider id: {provider}"));
    }
    ai::keychain::save_active_provider(&provider).map_err(CommandError::from)?;
    state.set_active_provider(provider.clone());
    // Re-cache the key for the now-active provider (if any) so the
    // next `rebuild_agent` picks the right credentials up.
    let cached = ai::keychain::load_api_key(&provider);
    state.set_api_key_cache(cached);
    rebuild_agent(&state).await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Model catalogue
// ---------------------------------------------------------------------------

/// Mirror of [`ai::ModelInfo`] — repeated here as a Serde-serializable
/// struct rather than re-exported so the Tauri IPC schema stays
/// self-contained for the front-end TS bindings.
#[derive(Debug, Clone, Serialize)]
pub struct ModelInfoDto {
    pub id: String,
    pub display_name: String,
    pub context_length: Option<u32>,
    pub provider_hint: Option<String>,
}

impl From<ai::ModelInfo> for ModelInfoDto {
    fn from(m: ai::ModelInfo) -> Self {
        Self {
            id: m.id,
            display_name: m.display_name,
            context_length: m.context_length,
            provider_hint: m.provider_hint,
        }
    }
}

/// List the model catalogue for `provider`. `api_key` is required for
/// OpenAI (the catalogue endpoint is auth-gated); optional for the
/// other providers — Anthropic returns a static curated list and
/// OpenRouter's catalogue is publicly accessible.
///
/// The Rust layer caches results for 10 minutes; repeated calls within
/// that window return cached entries without hitting the network.
#[tauri::command]
pub async fn list_models_for(
    provider: String,
    api_key: Option<String>,
) -> CmdResult<Vec<ModelInfoDto>> {
    let key_ref = api_key.as_deref();
    // Follow the same endpoint the agent will use. Listing one server's
    // catalogue while chat talks to another is worse than not listing.
    let base = ai::keychain::load_base_url(&provider);
    let models = ai::list_models_for_at(&provider, key_ref, base.as_deref())
        .await
        .map_err(|e| e.to_string())?;
    Ok(models.into_iter().map(ModelInfoDto::from).collect())
}

// ---------------------------------------------------------------------------
// Model selection
// ---------------------------------------------------------------------------

/// Persist the chosen model id for `provider`. The next agent rebuild
/// reads this slot and constructs the [`ai::LlmConfig`] with the
/// chosen model overlaid on the provider's default.
#[tauri::command]
pub async fn set_active_model(
    state: State<'_, AppState>,
    provider: String,
    model: String,
) -> CmdResult<()> {
    if !ai::SUPPORTED_PROVIDER_IDS.iter().any(|p| *p == provider) {
        return Err(format!("unsupported provider id: {provider}"));
    }
    let model = model.trim().to_string();
    if model.is_empty() {
        return Err("model id must not be empty".into());
    }
    state.set_model_for(provider, model);
    rebuild_agent(&state).await?;
    Ok(())
}

/// Read the model id currently selected for `provider`. Empty string
/// when nothing has been chosen yet.
#[tauri::command]
pub async fn get_active_model(state: State<'_, AppState>, provider: String) -> CmdResult<String> {
    Ok(state.model_for(&provider).unwrap_or_default())
}

// ---------------------------------------------------------------------------
// list_capabilities
// ---------------------------------------------------------------------------

/// One tool descriptor in the capabilities listing.
///
/// `name` is the canonical tool name (matches the value used in
/// `agent://tool-call` events). `description` is the model-facing
/// description from the Anthropic tool schema, surfaced verbatim so
/// the user sees the same hint the model does. `category` is one of
/// `audio`, `session`, `analysis` — derived from a small name-prefix
/// map so the popover can group tools without the dispatcher having
/// to grow a category field.
#[derive(Debug, Clone, Serialize)]
pub struct CapabilityDescriptor {
    /// The identifier the rest of the system uses for this capability —
    /// what a disabled-list entry has to contain to actually match.
    ///
    /// Separate from `name` because the two differ for MCP tools, and
    /// conflating them made the menu's checkboxes inert: it persisted the
    /// `<server>::<tool>` display name while the tool blacklist is
    /// matched against the dispatcher's mangled `<server>__<tool>` wire
    /// name, so nothing a user disabled was ever filtered.
    pub id: String,
    /// What the user reads. For MCP tools this is the unmangled
    /// `<server>::<tool>`; the wire name is an implementation detail of
    /// the 64-character Anthropic limit and means nothing to a person.
    pub name: String,
    pub description: String,
    pub category: String,
}

/// What the `+` capabilities menu renders.
///
/// `tools` is the live set of registered built-in tools. The other
/// three arrays are placeholders so the menu can declare future
/// surfaces ("Skills", "Agents", "MCP servers") with empty state
/// today. Shipping them as part of the response keeps the TS shape
/// stable when we wire real implementations in later.
#[derive(Debug, Clone, Serialize)]
pub struct Capabilities {
    pub tools: Vec<CapabilityDescriptor>,
    pub skills: Vec<CapabilityDescriptor>,
    pub agents: Vec<CapabilityDescriptor>,
    pub mcp_servers: Vec<CapabilityDescriptor>,
}

/// Map a tool name to a coarse UI category. Centralised here (and not
/// on the `Tool` trait) so adding a tool does not require touching the
/// trait; new prefixes fall back to "audio" which is what every
/// edit-y tool already lives under.
fn category_for(name: &str) -> &'static str {
    match name {
        "load" | "render_preview" | "render_final" | "add_track" | "remove_track" => "session",
        "transcribe" | "separate_stems" | "analyze_track" => "analysis",
        "fork_node" | "apply_diff" | "compare_nodes" | "revert_to" | "name_node" => "history",
        "label" => "annotation",
        _ => "audio",
    }
}

#[tauri::command]
pub async fn list_capabilities(state: State<'_, AppState>) -> CmdResult<Capabilities> {
    // Refresh the skill library before snapshotting, the same way
    // `list_skills` does. Keeps both surfaces consistent — the `+`
    // menu can't show a different skill set than Settings → Skills
    // just because the cached library is older.
    if let Err(e) = state.reload_skills_from_disk() {
        tracing::warn!(error = ?e, "skill reload from disk failed in list_capabilities");
    }
    let dispatcher = lock_std(&state.dispatcher, "dispatcher")?;
    let schemas = dispatcher.tool_schemas();
    let mut tools: Vec<CapabilityDescriptor> = schemas
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|s| {
                    let name = s.get("name")?.as_str()?.to_string();
                    let description = s
                        .get("description")
                        .and_then(|d| d.as_str())
                        .unwrap_or_default()
                        .to_string();
                    let category = category_for(&name).to_string();
                    Some(CapabilityDescriptor {
                        // A built-in tool's dispatcher name is both its
                        // identifier and its label.
                        id: name.clone(),
                        name,
                        description,
                        category,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    // Stable alphabetical order so the menu doesn't reshuffle between
    // launches just because `HashMap` iteration is unspecified.
    tools.sort_by(|a, b| a.name.cmp(&b.name));
    drop(dispatcher);

    // The capabilities menu uses a single `CapabilityDescriptor`
    // shape across all groups. Skills carry a trigger summary as
    // their category so the popover can show users when the skill
    // fires without growing the shape with a new field.
    let skills: Vec<CapabilityDescriptor> = read_skill_summaries(&state)
        .into_iter()
        .map(|s| CapabilityDescriptor {
            id: s.name.clone(),
            name: s.name,
            description: s.description,
            category: s.trigger,
        })
        .collect();

    // Remote MCP tools are real capabilities the model can call, so
    // they belong in the menu alongside the built-ins. They are listed
    // under their unmangled `<server>::<tool>` display name — the
    // mangled wire name is an implementation detail of the 64-char
    // Anthropic limit and means nothing to the user.
    let mcp_servers: Vec<CapabilityDescriptor> = state
        .mcp
        .remote_tools()
        .into_iter()
        .map(|t| CapabilityDescriptor {
            id: t.wire_name,
            name: t.display_name,
            description: t.description,
            category: t.server,
        })
        .collect();

    Ok(Capabilities {
        tools,
        skills,
        agents: Vec::new(),
        mcp_servers,
    })
}

// ---------------------------------------------------------------------------
// skills: read-only list (phase 2). Edit / delete come in phase 3.
// ---------------------------------------------------------------------------

/// One skill entry returned by `list_skills` and `list_capabilities`.
/// `trigger` is a short, human-friendly summary (`"always"`,
/// `"keywords: mix, sidechain"`, `"regex"`) so the menu can show
/// users when a skill fires without re-implementing the parser on the
/// TS side.
#[derive(Debug, Clone, Serialize)]
pub struct SkillSummary {
    pub name: String,
    pub description: String,
    pub trigger: String,
    pub enabled: bool,
}

#[tauri::command]
pub async fn list_skills(state: State<'_, AppState>) -> CmdResult<Vec<SkillSummary>> {
    // Pick up new / removed files since startup without an app
    // restart. Failures fall back to the previously-loaded library
    // and surface as an empty list rather than an error.
    if let Err(e) = state.reload_skills_from_disk() {
        tracing::warn!(error = ?e, "skill reload from disk failed; returning cached library");
    }
    Ok(read_skill_summaries(&state))
}

fn read_skill_summaries(state: &AppState) -> Vec<SkillSummary> {
    let handle = state.skills_handle();
    let lib = handle.lock().expect("skill library mutex poisoned");
    lib.skills()
        .iter()
        .map(|s| SkillSummary {
            name: s.name.clone(),
            description: s.description.clone(),
            trigger: trigger_summary(&s.trigger),
            enabled: s.enabled,
        })
        .collect()
}

fn trigger_summary(t: &skills::Trigger) -> String {
    match t {
        skills::Trigger::Always => "always".into(),
        skills::Trigger::Keywords(words) => {
            let preview: Vec<&str> = words.iter().take(4).map(String::as_str).collect();
            let suffix = if words.len() > 4 { ", …" } else { "" };
            format!("keywords: {}{suffix}", preview.join(", "))
        }
        skills::Trigger::Regex(_) => "regex".into(),
    }
}

// ---------------------------------------------------------------------------
// skills: read / write / delete (phase 3)
// ---------------------------------------------------------------------------

/// Round-trip shape for the skill editor. Mirrors the on-disk
/// frontmatter + body verbatim so the UI can edit either side. `name`
/// is the canonical id (always the filename stem); the loader rejects
/// a frontmatter `name` that doesn't match, so we keep it implicit on
/// write and let the user rename via the dedicated UI affordance.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct SkillContent {
    pub name: String,
    pub description: String,
    /// `"always"` | `"keywords"` | `"regex"`.
    pub trigger: String,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub pattern: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub body: String,
}

fn default_true() -> bool {
    true
}

#[tauri::command]
pub async fn read_skill(state: State<'_, AppState>, name: String) -> CmdResult<SkillContent> {
    let dir = skills_dir(&state)?;
    let path = dir.join(format!("{name}.md"));
    if !path.exists() {
        return Err(CommandError::InvalidPath(format!(
            "skill `{name}` not found at {}",
            path.display()
        ))
        .into());
    }
    let raw = std::fs::read_to_string(&path).map_err(CommandError::from)?;
    let parsed = parse_skill_file(&raw)?;
    let stem = name.clone();
    Ok(SkillContent {
        name: stem,
        description: parsed.description,
        trigger: parsed.trigger,
        keywords: parsed.keywords,
        pattern: parsed.pattern,
        enabled: parsed.enabled,
        body: parsed.body,
    })
}

#[tauri::command]
pub async fn upsert_skill(
    state: State<'_, AppState>,
    name: String,
    content: SkillContent,
) -> CmdResult<()> {
    if name != content.name {
        return Err(CommandError::InvalidSkill(format!(
            "name parameter `{name}` does not match content.name `{}`",
            content.name
        ))
        .into());
    }
    validate_skill_name(&name)?;
    validate_skill_content(&content)?;

    let dir = skills_dir(&state)?;
    std::fs::create_dir_all(&dir).map_err(CommandError::from)?;
    let path = dir.join(format!("{name}.md"));
    let serialised = serialise_skill_file(&content);

    // Atomic write — tempfile in the same dir, then persist.
    let mut tmp = tempfile::NamedTempFile::new_in(&dir).map_err(CommandError::from)?;
    std::io::Write::write_all(&mut tmp, serialised.as_bytes()).map_err(CommandError::from)?;
    tmp.flush().map_err(CommandError::from)?;
    tmp.persist(&path).map_err(|e| CommandError::Io(e.error))?;

    if let Err(e) = state.reload_skills_from_disk() {
        // Save succeeded; surface the reload failure so the editor
        // can show the user that the on-disk file looks malformed.
        return Err(CommandError::InvalidSkill(format!("saved but reload failed: {e}")).into());
    }
    Ok(())
}

#[tauri::command]
pub async fn delete_skill(state: State<'_, AppState>, name: String) -> CmdResult<()> {
    validate_skill_name(&name)?;
    let dir = skills_dir(&state)?;
    let path = dir.join(format!("{name}.md"));
    if path.exists() {
        std::fs::remove_file(&path).map_err(CommandError::from)?;
    }
    if let Err(e) = state.reload_skills_from_disk() {
        return Err(CommandError::InvalidSkill(format!(
            "skill `{name}` deleted but reload failed: {e}"
        ))
        .into());
    }
    Ok(())
}

fn skills_dir(state: &AppState) -> Result<PathBuf, CommandError> {
    let dir = state
        .skills_dir
        .lock()
        .map_err(|_| CommandError::Poisoned("skills_dir"))?
        .clone();
    if dir.as_os_str().is_empty() {
        return Err(CommandError::NoSkillsDir);
    }
    Ok(dir)
}

/// Filename-safe identifier: ASCII alphanumeric, dash, underscore.
/// Refusing other characters keeps paths well-behaved on every
/// filesystem and avoids `..`-style escapes since `/` and `\` aren't
/// in the allowed set.
fn validate_skill_name(name: &str) -> Result<(), CommandError> {
    if name.is_empty() || name.len() > 64 {
        return Err(CommandError::InvalidSkill(
            "skill name must be 1-64 characters".into(),
        ));
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(CommandError::InvalidSkill(
            "skill name may only contain ASCII letters, digits, dash, or underscore".into(),
        ));
    }
    Ok(())
}

fn validate_skill_content(content: &SkillContent) -> Result<(), CommandError> {
    match content.trigger.as_str() {
        "always" => {}
        "keywords" => {
            if content.keywords.is_empty() {
                return Err(CommandError::InvalidSkill(
                    "trigger=keywords requires at least one keyword".into(),
                ));
            }
        }
        "regex" => {
            if content.pattern.trim().is_empty() {
                return Err(CommandError::InvalidSkill(
                    "trigger=regex requires a non-empty pattern".into(),
                ));
            }
            // Cheap syntactic validation so the editor surfaces bad
            // regexes immediately rather than at next agent turn.
            regex::Regex::new(&content.pattern).map_err(|e| {
                CommandError::InvalidSkill(format!("regex pattern is invalid: {e}"))
            })?;
        }
        other => {
            return Err(CommandError::InvalidSkill(format!(
                "unknown trigger `{other}` (expected always|keywords|regex)"
            )))
        }
    }
    Ok(())
}

/// Serialise a `SkillContent` back to the on-disk markdown format.
/// The schema mirrors what `crates/skills` parses; we deliberately
/// omit empty optional fields so files written from the editor look
/// the same as files a user would hand-write.
fn serialise_skill_file(c: &SkillContent) -> String {
    let mut out = String::from("---\n");
    out.push_str(&format!(
        "description: {}\n",
        quote_if_needed(&c.description)
    ));
    out.push_str(&format!("trigger: {}\n", c.trigger));
    if c.trigger == "keywords" && !c.keywords.is_empty() {
        let inner: Vec<String> = c
            .keywords
            .iter()
            .map(|k| quote_if_needed(k.trim()))
            .collect();
        out.push_str(&format!("keywords: [{}]\n", inner.join(", ")));
    }
    if c.trigger == "regex" && !c.pattern.is_empty() {
        out.push_str(&format!("pattern: {}\n", quote_if_needed(&c.pattern)));
    }
    if !c.enabled {
        out.push_str("enabled: false\n");
    }
    out.push_str("---\n");
    out.push_str(&c.body);
    if !c.body.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// Quote a scalar when it contains characters the loader would parse
/// specially (`:`, `,`, `[`, `]`, leading/trailing whitespace). The
/// loader accepts both quoted and unquoted forms, so quoting is a
/// safety convenience.
fn quote_if_needed(s: &str) -> String {
    let trimmed = s.trim();
    let needs = trimmed != s
        || trimmed.is_empty()
        || trimmed.contains([':', ',', '[', ']', '#'])
        || trimmed.starts_with('"')
        || trimmed.starts_with('\'');
    if needs {
        let escaped = trimmed.replace('"', "\\\"");
        format!("\"{escaped}\"")
    } else {
        trimmed.to_string()
    }
}

/// Loosely parse a skill file into the editor's flat shape. Mirrors
/// the loader rules in `crates/skills` but keeps the legible fallback
/// strings the UI wants (e.g. `enabled` defaults to `true`).
fn parse_skill_file(raw: &str) -> Result<ParsedSkillFile, CommandError> {
    let body_start = raw
        .strip_prefix("---\n")
        .or_else(|| raw.strip_prefix("---\r\n"))
        .ok_or_else(|| CommandError::InvalidSkill("missing frontmatter delimiters".into()))?;
    let end = body_start.find("\n---").ok_or_else(|| {
        CommandError::InvalidSkill("missing frontmatter closing delimiter".into())
    })?;
    let fm = &body_start[..end];
    let body = body_start[end + "\n---".len()..]
        .trim_start_matches('\r')
        .trim_start_matches('\n')
        .to_string();

    let mut out = ParsedSkillFile {
        description: String::new(),
        trigger: "always".into(),
        keywords: Vec::new(),
        pattern: String::new(),
        enabled: true,
        body,
    };
    for line in fm.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (k, v) = match line.split_once(':') {
            Some(kv) => kv,
            None => continue,
        };
        let k = k.trim();
        let v = strip_quotes(v.trim());
        match k {
            "description" => out.description = v.to_string(),
            "trigger" => out.trigger = v.to_string(),
            "pattern" => out.pattern = v.to_string(),
            "enabled" => out.enabled = v == "true",
            "keywords" => {
                if let Some(arr) = v.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
                    out.keywords = split_array_items(arr);
                }
            }
            _ => {}
        }
    }
    Ok(out)
}

struct ParsedSkillFile {
    description: String,
    trigger: String,
    keywords: Vec<String>,
    pattern: String,
    enabled: bool,
    body: String,
}

fn strip_quotes(s: &str) -> &str {
    let s = s.trim();
    if (s.starts_with('"') && s.ends_with('"') && s.len() >= 2)
        || (s.starts_with('\'') && s.ends_with('\'') && s.len() >= 2)
    {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

/// Quote-aware split of an inline-array body (the part between the
/// outer `[` and `]`). Commas inside `"…"` or `'…'` literals are
/// preserved so a keyword like `"two,three"` survives the round trip.
/// Each returned item is `strip_quotes`-d and trimmed; empty items
/// are dropped.
fn split_array_items(inner: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut in_quote: Option<char> = None;
    let mut escaped = false;
    for ch in inner.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }
        if ch == '\\' && in_quote.is_some() {
            // Preserve the backslash so `strip_quotes` sees the
            // original `\"` sequence; the consumer can de-escape if
            // it cares.
            current.push(ch);
            escaped = true;
            continue;
        }
        match in_quote {
            Some(q) if ch == q => {
                current.push(ch);
                in_quote = None;
            }
            Some(_) => current.push(ch),
            None => {
                if ch == '"' || ch == '\'' {
                    in_quote = Some(ch);
                    current.push(ch);
                } else if ch == ',' {
                    let item = strip_quotes(current.trim()).to_string();
                    if !item.is_empty() {
                        out.push(item);
                    }
                    current.clear();
                } else {
                    current.push(ch);
                }
            }
        }
    }
    let item = strip_quotes(current.trim()).to_string();
    if !item.is_empty() {
        out.push(item);
    }
    out
}

#[cfg(test)]
mod render_range_tests {
    #[test]
    fn sec_to_frame_conversion() {
        let sample_rate: u32 = 44100;
        let start_frame = (1.0_f64 * sample_rate as f64) as u64;
        let end_frame = (2.5_f64 * sample_rate as f64) as u64;
        assert_eq!(start_frame, 44100);
        assert_eq!(end_frame, 110250);
    }
}

#[cfg(test)]
mod commands_tests {
    use super::*;

    #[test]
    fn split_array_items_basic() {
        assert_eq!(
            split_array_items("a, b, c"),
            vec!["a".to_string(), "b".into(), "c".into()]
        );
    }

    #[test]
    fn split_array_items_preserves_quoted_commas() {
        assert_eq!(
            split_array_items("one, \"two,three\", four"),
            vec!["one".to_string(), "two,three".into(), "four".into()]
        );
    }

    #[test]
    fn split_array_items_drops_empties() {
        assert_eq!(split_array_items(",,  a , ,"), vec!["a".to_string()]);
    }
}

// ---------------------------------------------------------------------------
// get_session_head
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn get_session_head(state: State<'_, AppState>) -> CmdResult<String> {
    let store_handle = state.store_handle().ok_or(CommandError::NoSession)?;
    let store = lock_std(&store_handle, "store")?;
    let head = store.head().ok_or(CommandError::NoSession)?;
    Ok(head.to_hex())
}

// ---------------------------------------------------------------------------
// get_node
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn get_node(state: State<'_, AppState>, id: String) -> CmdResult<SessionNode> {
    let store_handle = state.store_handle().ok_or(CommandError::NoSession)?;
    let node_id = NodeId::from_hex(&id).map_err(|_| CommandError::InvalidNodeId(id.clone()))?;
    let store = lock_std(&store_handle, "store")?;
    let node = store.get(node_id).map_err(CommandError::from)?;
    Ok(node)
}

// ---------------------------------------------------------------------------
// get_graph
// ---------------------------------------------------------------------------

/// One node entry in the [`GraphSummary`] returned by [`get_graph`].
///
/// Stripped down from the full [`SessionNode`] so the frontend graph
/// view doesn't pull a complete `SessionState` per node — for a
/// 200-node session we only need ids, parent links, and display
/// metadata. Field names are hand-aligned with the TypeScript
/// `GraphNode` interface in `apps/desktop/src/lib/tauri-bridge.ts`.
#[derive(Debug, Clone, Serialize)]
pub struct GraphNode {
    pub id: String,
    pub parent: Option<String>,
    pub label: Option<String>,
    /// Best-effort tool name. We don't store the producing tool
    /// explicitly on `SessionNode` today, so we derive it from the
    /// first whitespace-delimited token of `label` (e.g. `"load
    /// foo.wav"` -> `"load"`). When `label` is `None` this is `None`
    /// and the UI falls back to displaying just the id prefix.
    pub tool: Option<String>,
    pub created_at: String,
}

/// All nodes plus the current head, returned by [`get_graph`].
///
/// This is the shape consumed by the M25 `<GraphView />` component.
/// Field names are hand-aligned with the TypeScript `GraphSummary`
/// interface in `apps/desktop/src/lib/tauri-bridge.ts`.
#[derive(Debug, Clone, Serialize)]
pub struct GraphSummary {
    pub nodes: Vec<GraphNode>,
    pub head: Option<String>,
}

#[tauri::command]
pub async fn get_graph(state: State<'_, AppState>) -> CmdResult<GraphSummary> {
    let store_handle = state.store_handle().ok_or(CommandError::NoSession)?;
    let store = lock_std(&store_handle, "store")?;
    let nodes = store.list_nodes().map_err(CommandError::from)?;
    let head = store.head().map(|id| id.to_hex());

    let graph_nodes = nodes
        .into_iter()
        .map(|n| GraphNode {
            id: n.id.to_hex(),
            parent: n.parent.map(|p| p.to_hex()),
            tool: n.label.as_deref().and_then(first_token).map(str::to_owned),
            label: n.label,
            created_at: n.created_at.to_rfc3339(),
        })
        .collect();

    Ok(GraphSummary {
        nodes: graph_nodes,
        head,
    })
}

/// First whitespace-delimited token of `s`, or `None` if `s` is blank.
fn first_token(s: &str) -> Option<&str> {
    s.split_whitespace().next()
}

// ---------------------------------------------------------------------------
// render_preview
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn render_preview(state: State<'_, AppState>, node: String) -> CmdResult<String> {
    let store_handle = state.store_handle().ok_or(CommandError::NoSession)?;
    let node_id = NodeId::from_hex(&node).map_err(|_| CommandError::InvalidNodeId(node.clone()))?;

    let session_node = {
        let store = lock_std(&store_handle, "store")?;
        store.get(node_id).map_err(CommandError::from)?
    };

    // Cached under `<project>/.audiograph/previews/`, keyed by node id.
    // The id is a hash of the session state, so undo and redo land back
    // on renders that already exist instead of paying for the mix again
    // — which is the difference between an editor that feels live and
    // one that goes quiet after every edit.
    let project_dir = {
        let store = lock_std(&store_handle, "store")?;
        store.project_dir().to_path_buf()
    };
    let cache = tools::PreviewCache::new(&project_dir);

    let (out_path, _hit) = cache.get_or_render::<_, CommandError>(node_id, |path| {
        let engine = lock_std(&state.engine, "engine")?;
        engine
            .render_to_wav(&session_node.state, path, None)
            .map_err(CommandError::from)?;
        Ok(())
    })?;

    out_path
        .to_str()
        .map(str::to_owned)
        .ok_or_else(|| CommandError::InvalidPath("temp path is not valid UTF-8".into()).into())
}

// ---------------------------------------------------------------------------
// prepare_compare
// ---------------------------------------------------------------------------

/// Pre-render both A and B nodes so the frontend A/B toggle is gapless.
///
/// Renders each node to a stable path under the OS temp directory:
/// - `<tempdir>/compare_a.wav`
/// - `<tempdir>/compare_b.wav`
///
/// Returns `{ "a_path": "…", "b_path": "…" }` — the caller feeds these
/// paths to the audio engine on each A/B button click so the switch is
/// instant (no render latency on toggle).
#[tauri::command]
pub async fn prepare_compare<R: Runtime>(
    a: String,
    b: String,
    _app: AppHandle<R>,
    state: State<'_, AppState>,
) -> CmdResult<serde_json::Value> {
    let store_handle = state.store_handle().ok_or(CommandError::NoSession)?;

    let a_id = NodeId::from_hex(&a).map_err(|_| CommandError::InvalidNodeId(a.clone()))?;
    let b_id = NodeId::from_hex(&b).map_err(|_| CommandError::InvalidNodeId(b.clone()))?;

    let (a_state, b_state) = {
        let store = lock_std(&store_handle, "store")?;
        let a_node = store.get(a_id).map_err(CommandError::from)?;
        let b_node = store.get(b_id).map_err(CommandError::from)?;
        (a_node.state, b_node.state)
    };

    let mut a_path = std::env::temp_dir();
    a_path.push(format!("edytlab-compare-a-{}.wav", a_id.to_hex()));
    let mut b_path = std::env::temp_dir();
    b_path.push(format!("edytlab-compare-b-{}.wav", b_id.to_hex()));

    {
        let engine = lock_std(&state.engine, "engine")?;
        engine
            .render_to_wav(&a_state, &a_path, None)
            .map_err(CommandError::from)?;
        engine
            .render_to_wav(&b_state, &b_path, None)
            .map_err(CommandError::from)?;
    }

    let a_path_str = a_path
        .to_str()
        .map(str::to_owned)
        .ok_or_else(|| CommandError::InvalidPath("compare_a path is not valid UTF-8".into()))?;
    let b_path_str = b_path
        .to_str()
        .map(str::to_owned)
        .ok_or_else(|| CommandError::InvalidPath("compare_b path is not valid UTF-8".into()))?;

    Ok(serde_json::json!({
        "a_path": a_path_str,
        "b_path": b_path_str,
    }))
}

// ---------------------------------------------------------------------------
// accept_b
// ---------------------------------------------------------------------------

/// Accept the B side of an A/B compare: set the session head to `b`.
///
/// Returns the new head hex so the frontend can update its local head
/// pointer without a separate `get_session_head` round-trip.
#[tauri::command]
pub async fn accept_b<R: Runtime>(
    b: String,
    _app: AppHandle<R>,
    state: State<'_, AppState>,
) -> CmdResult<String> {
    let store_handle = state.store_handle().ok_or(CommandError::NoSession)?;
    let b_id = NodeId::from_hex(&b).map_err(|_| CommandError::InvalidNodeId(b.clone()))?;
    {
        let mut store = lock_std(&store_handle, "store")?;
        store.set_head(b_id).map_err(CommandError::from)?;
    }
    Ok(b_id.to_hex())
}

// ---------------------------------------------------------------------------
// send_message
// ---------------------------------------------------------------------------

/// Build an effective tool whitelist by removing `blacklist` entries.
///
/// * If the caller already has a profile-based whitelist, remove any
///   blacklisted names from it.
/// * If there is no profile whitelist, expand to *all* registered tools
///   and then remove the blacklisted names (so the model still gets every
///   non-disabled tool rather than an unrestricted set).
/// * If `blacklist` is empty, return `whitelist` unchanged.
// Note: acquires dispatcher lock indirectly via `state.all_tool_names()`.
// Callers must not hold the dispatcher lock already; the agent lock
// (held by send_message) is acquired before this call.
fn apply_blacklist(
    whitelist: Option<Vec<String>>,
    blacklist: &[String],
    state: &AppState,
) -> Option<Vec<String>> {
    if blacklist.is_empty() {
        return whitelist;
    }
    match whitelist {
        Some(list) => Some(
            list.into_iter()
                .filter(|t| !blacklist.contains(t))
                .collect(),
        ),
        None => {
            let names = state.all_tool_names();
            Some(
                names
                    .into_iter()
                    .filter(|t| !blacklist.contains(t))
                    .collect(),
            )
        }
    }
}

#[tauri::command]
pub async fn send_message<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    text: String,
    disabled_tools: Vec<String>,
) -> CmdResult<()> {
    // Build the per-turn SessionContext from the current selection and
    // the annotations on the store head. We snapshot these before
    // acquiring the agent lock to minimise the lock hold time.
    let selection = state.selection_snapshot();
    let markers = match state.store_handle() {
        None => vec![],
        Some(store_arc) => {
            let store = lock_std(&*store_arc, "store")?;
            match store.head() {
                None => vec![],
                Some(head) => store.annotations_for(head).unwrap_or_default(),
            }
        }
    };
    let session_ctx = if selection.is_some() || !markers.is_empty() {
        Some(ai::SessionContext { selection, markers })
    } else {
        None
    };

    // Hold the agent lock for the duration of the turn. Phase 1 has a
    // single chat thread, so serialised turns are correct (the user
    // cannot race themselves). The lock is `tokio::sync::Mutex` because
    // we hold it across `.await`.
    let mut agent_guard = state.agent.lock().await;
    let agent = agent_guard.as_mut().ok_or(CommandError::NoAgent)?;

    // The closure must be `FnMut` and synchronous (per `Agent::turn`'s
    // signature). We capture the AppHandle by clone so each emit is
    // independent; emit failures (the receiver window has gone away)
    // are logged rather than aborting the turn.
    let app_handle = app.clone();
    let on_event = move |event: ai::AgentEvent| {
        emit_agent_event(&app_handle, event);
    };

    // Apply the per-turn disabled-tools blacklist. We snapshot the
    // agent's current whitelist, compute an effective whitelist that
    // excludes the disabled tools, swap it in for the duration of this
    // turn, and restore the original afterwards — even on error.
    let current = agent.swap_tool_whitelist(None); // take current out
    let effective = apply_blacklist(current.clone(), &disabled_tools, &state);
    agent.swap_tool_whitelist(effective); // put effective in

    let turn_result = agent
        .turn_with_context(text, session_ctx.as_ref(), on_event)
        .await;

    // Restore the original whitelist regardless of turn success/failure.
    // Restore happens before the `?` propagation so it is guaranteed even
    // on Err. Panics inside an async Tokio task surface as task failures
    // (not process panics), so the agent lock is released and the restore
    // is visible once the task unwinds — acceptable for this use-case.
    agent.swap_tool_whitelist(current);

    turn_result.map_err(CommandError::from)?;

    Ok(())
}

/// Forward a single [`ai::AgentEvent`] to the Tauri event bus.
///
/// Emit failures are logged at `warn` and swallowed: an emit error
/// usually means the receiving window has been closed, in which case
/// continuing the turn (or letting the agent finish so the conversation
/// history stays consistent) is the right call.
fn emit_agent_event<R: tauri::Runtime>(app: &AppHandle<R>, event: ai::AgentEvent) {
    match event {
        ai::AgentEvent::TextDelta(text) => {
            if let Err(e) = app.emit(TEXT_DELTA, TextDeltaPayload { text }) {
                tracing::warn!(error = %e, "failed to emit text-delta");
            }
        }
        ai::AgentEvent::ToolCallStart { name, id } => {
            if let Err(e) = app.emit(TOOL_CALL, ToolCallPayload { name, id }) {
                tracing::warn!(error = %e, "failed to emit tool-call");
            }
        }
        ai::AgentEvent::ToolCallEnd { id, ok, view } => {
            if let Err(e) = app.emit(TOOL_CALL_END, ToolCallEndPayload { id, ok, view }) {
                tracing::warn!(error = %e, "failed to emit tool-call-end");
            }
        }
        ai::AgentEvent::NodeCreated(id) => {
            let payload = NodeCreatedPayload {
                node_id: id.to_hex(),
            };
            if let Err(e) = app.emit(NODE_CREATED, payload) {
                tracing::warn!(error = %e, "failed to emit node-created");
            }
        }
        ai::AgentEvent::Done => {
            if let Err(e) = app.emit(DONE, DonePayload {}) {
                tracing::warn!(error = %e, "failed to emit done");
            }
        }
        ai::AgentEvent::Plan { steps } => {
            if let Err(e) = app.emit(PLAN, PlanPayload { steps }) {
                tracing::warn!(error = %e, "failed to emit plan");
            }
        }
        ai::AgentEvent::PlanRejected => {
            if let Err(e) = app.emit(PLAN_REJECTED, ()) {
                tracing::warn!(error = %e, "failed to emit plan rejection");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// approve_plan
// ---------------------------------------------------------------------------

/// Called by the frontend "Approve plan" button. Fires the plan-approval
/// notifier in `AppState`, unblocking the mashup-mode turn loop.
///
/// Deliberately does NOT acquire the agent mutex — `send_message` holds
/// it across its `.await` points, so touching the agent here would
/// deadlock.  The notifier lives independently on `AppState`.
///
/// When `steps` is non-empty the edited step descriptions are stored in
/// `plan_steps_override` before the notifier fires.  The agent loop reads
/// and clears this slot immediately after it wakes, then appends the
/// override text to the conversation so the model executes the user's
/// modified plan rather than the original one.
/// Turn "plan before acting" on or off.
///
/// Held on `AppState` rather than the agent because the agent is rebuilt
/// on every key, model or base-URL change, and a preference that
/// silently reset itself would be worse than not having one.
#[tauri::command]
pub async fn set_plan_first(state: State<'_, AppState>, enabled: bool) -> CmdResult<()> {
    state
        .plan_first
        .store(enabled, std::sync::atomic::Ordering::SeqCst);
    rebuild_agent(state.inner()).await?;
    Ok(())
}

#[tauri::command]
pub async fn get_plan_first(state: State<'_, AppState>) -> CmdResult<bool> {
    Ok(state.plan_first.load(std::sync::atomic::Ordering::SeqCst))
}

/// Decline a plan. Sets the rejection flag, then fires the same notifier
/// `approve_plan` uses, so the waiting turn wakes and ends cleanly
/// having run no tools and appended no node.
#[tauri::command]
pub async fn reject_plan(state: State<'_, AppState>) -> CmdResult<()> {
    state
        .plan_rejected
        .store(true, std::sync::atomic::Ordering::SeqCst);
    state.plan_notify.notify_one();
    Ok(())
}

#[tauri::command]
pub async fn approve_plan(state: State<'_, AppState>, steps: Option<Vec<String>>) -> CmdResult<()> {
    if let Some(steps) = steps {
        if !steps.is_empty() {
            let formatted = steps
                .iter()
                .enumerate()
                .map(|(i, s)| format!("{}. {}", i + 1, s))
                .collect::<Vec<_>>()
                .join("\n");
            let override_text = format!(
                "I've updated the plan. Please follow these revised steps instead:\n{formatted}"
            );
            *state
                .plan_steps_override
                .lock()
                .expect("plan_steps_override mutex poisoned") = Some(override_text);
        }
    }
    state.plan_notify.notify_one();
    Ok(())
}

// ---------------------------------------------------------------------------
// E1: set_selection_context
// ---------------------------------------------------------------------------

/// Push the user's current timeline selection from the frontend into
/// the app state. Called whenever the waveform selection changes so the
/// next `send_message` turn can splice the selection into the system
/// prompt via `SessionContext`.
///
/// Pass `null` / `None` to clear the selection.
#[tauri::command]
pub fn set_selection_context(state: State<'_, AppState>, range: Option<Range>) -> CmdResult<()> {
    state.set_selection(range);
    Ok(())
}

// ---------------------------------------------------------------------------
// E2: add_marker / remove_marker / list_markers
// ---------------------------------------------------------------------------

/// Add a point marker at `time` seconds with `name`. Appends a new
/// session node; the returned hex string is the new head.
///
/// Also emits a `marker-changed` event so the waveform UI can refresh
/// its overlay without polling.
#[tauri::command]
pub fn add_marker(
    app: AppHandle,
    state: State<'_, AppState>,
    time: f64,
    name: String,
) -> CmdResult<String> {
    // A marker outside the timeline cannot be rendered, selected or
    // removed — it is simply stuck in the session. The ruler used to
    // send negative times for a click on its sidebar spacer; that is
    // fixed, but the command should not have accepted them either.
    if !time.is_finite() || time < 0.0 {
        return Err(CommandError::InvalidMarker(format!(
            "time must be finite and >= 0; got {time}"
        ))
        .into());
    }
    let store_arc = state.store_handle().ok_or(CommandError::NoSession)?;
    let mut store = lock_std(&*store_arc, "store")?;
    let head = store.head().ok_or(CommandError::NoSession)?;
    let annotation = session::Annotation {
        id: session::AnnotationId::new(),
        name: name.clone(),
        kind: session::AnnotationKind::Marker { time_sec: time },
    };
    let new_head = store
        .add_annotation(head, annotation)
        .map_err(CommandError::from)?;
    drop(store);
    let _ = app.emit("marker-changed", ());
    Ok(new_head.to_hex())
}

/// Remove the annotation identified by `id` (a UUID string). Appends a
/// new session node if an annotation with that id was found; returns the
/// head hex unchanged otherwise.
///
/// Also emits a `marker-changed` event.
#[tauri::command]
pub fn remove_marker(app: AppHandle, state: State<'_, AppState>, id: String) -> CmdResult<String> {
    let annotation_id = session::AnnotationId(
        uuid::Uuid::parse_str(&id).map_err(|e| CommandError::InvalidNodeId(e.to_string()))?,
    );
    let store_arc = state.store_handle().ok_or(CommandError::NoSession)?;
    let mut store = lock_std(&*store_arc, "store")?;
    let head = store.head().ok_or(CommandError::NoSession)?;
    let new_head = store
        .remove_annotation(head, annotation_id)
        .map_err(CommandError::from)?;
    drop(store);
    let _ = app.emit("marker-changed", ());
    Ok(new_head.to_hex())
}

/// Return all annotations (markers and region labels) visible at the
/// current session head as a JSON array. Each entry is the serialised
/// [`session::Annotation`] shape.
#[tauri::command]
pub fn list_markers(state: State<'_, AppState>) -> CmdResult<Vec<serde_json::Value>> {
    let store_arc = state.store_handle().ok_or(CommandError::NoSession)?;
    let store = lock_std(&*store_arc, "store")?;
    let head = store.head().ok_or(CommandError::NoSession)?;
    let annotations = store.annotations_for(head).map_err(CommandError::from)?;
    drop(store);
    Ok(annotations
        .into_iter()
        .map(|a| serde_json::to_value(a).expect("Annotation always serialises"))
        .collect())
}

/// Move the session head pointer to `node_id`. Used by the graph
/// view's context menu ("Set as head"). Returns the new head as
/// hex on success.
#[tauri::command]
pub fn set_head_to(state: State<'_, AppState>, node_id: String) -> CmdResult<String> {
    let id = session::NodeId::from_hex(&node_id).map_err(CommandError::from)?;
    let store_arc = state.store_handle().ok_or(CommandError::NoSession)?;
    let mut store = lock_std(&*store_arc, "store")?;
    store.set_head(id).map_err(CommandError::from)?;
    Ok(id.to_hex())
}

/// Set or clear a human-readable label on a node. Pass an empty
/// string to clear. Used by the graph view's "Rename" overlay.
#[tauri::command]
pub fn rename_node(state: State<'_, AppState>, node_id: String, label: String) -> CmdResult<()> {
    let id = session::NodeId::from_hex(&node_id).map_err(CommandError::from)?;
    let label_opt = if label.trim().is_empty() {
        None
    } else {
        Some(label)
    };
    let store_arc = state.store_handle().ok_or(CommandError::NoSession)?;
    let mut store = lock_std(&*store_arc, "store")?;
    store.set_label(id, label_opt).map_err(CommandError::from)?;
    Ok(())
}

/// One track at the current session head. `audio_path` is the source
/// path of the track's single clip when there is exactly one; `None`
/// when the track has zero or multiple clips (the multi-clip mix is
/// not previewable until the M22+ realtime engine lands).
#[derive(Debug, Clone, Serialize)]
pub struct TrackSummary {
    pub id: String,
    pub name: String,
    pub muted: bool,
    pub gain_db: f32,
    /// -1.0 hard left, 0.0 centre, 1.0 hard right.
    pub pan: f32,
    pub soloed: bool,
    pub audio_path: Option<String>,
    /// Clips in track order, with whatever the automation lane needs to
    /// draw. Deliberately not the whole [`session::Clip`] — the clip
    /// timeline (#103) will want source paths, stretch factors and
    /// content hashes, and can widen this when it needs them.
    pub clips: Vec<ClipSummary>,
}

/// One clip's placement and its volume automation, in seconds.
///
/// Seconds rather than samples because the lane draws against the
/// waveform's time axis and `set_clip_envelope` already speaks seconds;
/// converting in two places invites the two from drifting apart.
#[derive(Debug, Clone, Serialize)]
pub struct ClipSummary {
    pub start_sec: f64,
    pub length_sec: f64,
    /// File this clip reads from. The lane labels chips with its stem,
    /// which is the only way to tell two clips apart once a track has
    /// been cut and both halves point at the same source.
    pub source_path: String,
    pub volume_envelope: Vec<EnvelopePointSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvelopePointSummary {
    pub time_sec: f64,
    pub gain_db: f32,
}

/// Return one entry per track at the current session head. Used by
/// the frontend Timeline to render per-lane waveforms — without this
/// every lane fell back to the same mix path.
#[tauri::command]
pub fn list_tracks(state: State<'_, AppState>) -> CmdResult<Vec<TrackSummary>> {
    let store_arc = state.store_handle().ok_or(CommandError::NoSession)?;
    let store = lock_std(&*store_arc, "store")?;
    let head = store.head().ok_or(CommandError::NoSession)?;
    let node = store.get(head).map_err(CommandError::from)?;
    // Read before the lock drops: flattening a multi-clip track writes
    // its result into the project's own derived directory (#156).
    let project_dir_for_flatten = store.project_dir().to_path_buf();
    drop(store);
    let sr = node.state.sample_rate.max(1) as f64;
    Ok(node
        .state
        .tracks
        .into_iter()
        .map(|t| TrackSummary {
            clips: t
                .clips
                .iter()
                .map(|c| ClipSummary {
                    start_sec: c.start_in_track as f64 / sr,
                    length_sec: c.length as f64 / sr,
                    source_path: c.source_path.to_string_lossy().into_owned(),
                    volume_envelope: c
                        .volume_envelope
                        .iter()
                        .map(|p| EnvelopePointSummary {
                            time_sec: p.time_samples as f64 / sr,
                            gain_db: p.gain_db,
                        })
                        .collect(),
                })
                .collect(),
            id: t.id.0.to_string(),
            name: t.name,
            muted: t.muted,
            gain_db: t.gain_db,
            pan: t.pan,
            soloed: t.soloed,
            // One clip already *is* a file, so hand its path over
            // untouched — the overwhelmingly common case, and free.
            //
            // A track split by an interior cut is not any single file on
            // disk, and returning `None` for it left the timeline lane
            // blank: the audio was there, the render was correct, and the
            // UI simply showed nothing. Flattening the clips into one
            // content-addressed WAV gives the lane something true to
            // draw. The CAS name is keyed on the clip list rather than
            // the audio, so this listing pays for a decode once per
            // distinct arrangement and finds the file already written
            // every time after.
            //
            // A failure here is not worth failing the listing over — the
            // lane falls back to blank, exactly as before.
            audio_path: match t.clips.len() {
                0 => None,
                1 => Some(t.clips[0].source_path.to_string_lossy().into_owned()),
                _ => tools::flattened_track_wav(&project_dir_for_flatten, &t.clips)
                    .ok()
                    .map(|p| p.to_string_lossy().into_owned()),
            },
        })
        .collect())
}

// ---------------------------------------------------------------------------
// Mixer controls (#94)
// ---------------------------------------------------------------------------
//
// Gain, pan, mute and solo were reachable only by asking the agent in
// English. The engine has had a pan law since #84 with nothing in the
// product able to reach it, and the timeline's `mute` button only ever
// set the local WaveSurfer volume — it looked like a mixer control and
// persisted nothing.
//
// These four commands run the *tools* rather than mutating the session
// themselves. The tools already validate the track index, append
// exactly one node and produce the label that shows up in history, and
// duplicating that is the trap #80 was: three copies of one edit path,
// two of which kept a bug after the third was fixed.
//
// Range checks stay here as well as in the tool, because this is the
// layer that accepts input from outside the process — the same reason
// `add_marker` validates its own time rather than trusting the ruler.

/// Widest gain a fader may set, in dB.
///
/// Not a limit of the mixer — `set_track_gain` the *tool* accepts any
/// finite value, and a session written by the agent can hold more. It
/// is a bound on what a slider is allowed to send, so a UI bug or a
/// hand-rolled `invoke` cannot put +200 dB into the session where it
/// would render as a wall of clipping.
const GAIN_LIMIT_DB: f32 = 24.0;
const GAIN_FLOOR_DB: f32 = -60.0;

/// Invoke `tool` with `args` and return the new session head.
///
/// The store, engine and dispatcher locks are taken together and
/// dropped together, matching `open_multiple`: a tool needs all three
/// for the duration of one call, and the head has to be read while the
/// store lock is still held or another caller could append in between.
fn run_track_tool(state: &AppState, tool: &str, args: serde_json::Value) -> CmdResult<String> {
    let store_arc = state.store_handle().ok_or(CommandError::NoSession)?;
    let head_hex = {
        let mut store = lock_std(&store_arc, "store")?;
        let mut engine = lock_std(&state.engine, "engine")?;
        let dispatcher = lock_std(&state.dispatcher, "dispatcher")?;
        let mut clipboard: Option<Vec<f32>> = None;
        let mut ctx = tools::ToolContext {
            store: &mut store,
            engine: &mut engine,
            user_message: "",
            clipboard: &mut clipboard,
        };
        // A tool that fails validation returns `ToolResult::Error`
        // rather than `Err` — that is the dispatcher contract, and it
        // is where "track index 7 out of range" comes from. Surfacing
        // it as a command error is what puts the message in front of
        // the user instead of silently reporting the unchanged head.
        match dispatcher
            .invoke(tool, args, &mut ctx)
            .map_err(|e| CommandError::InvalidTrackControl(format!("{tool}: {e}")))?
        {
            tools::ToolResult::Ok(_) => {}
            tools::ToolResult::Error(msg) => {
                return Err(CommandError::InvalidTrackControl(msg).into())
            }
        }
        store.head().map(|id| id.to_hex())
    };
    head_hex.ok_or_else(|| CommandError::NoSession.into())
}

/// Set a track's gain in dB. Returns the new session head.
#[tauri::command]
pub fn set_track_gain(state: State<'_, AppState>, track: usize, gain_db: f32) -> CmdResult<String> {
    set_track_gain_inner(&state, track, gain_db)
}

pub(crate) fn set_track_gain_inner(
    state: &AppState,
    track: usize,
    gain_db: f32,
) -> CmdResult<String> {
    if !gain_db.is_finite() || !(GAIN_FLOOR_DB..=GAIN_LIMIT_DB).contains(&gain_db) {
        return Err(CommandError::InvalidTrackControl(format!(
            "gain must be finite and between {GAIN_FLOOR_DB} and {GAIN_LIMIT_DB} dB; got {gain_db}"
        ))
        .into());
    }
    run_track_tool(
        state,
        "set_track_gain",
        serde_json::json!({ "track": track, "db": gain_db }),
    )
}

/// Set a track's stereo position: -1.0 hard left, 0.0 centre, 1.0 hard
/// right. Returns the new session head.
///
/// Out-of-range values are rejected rather than silently clamped. The
/// tool clamps because a model can reasonably say "pan it to 1.5" and
/// mean "hard right"; a slider that sends 1.5 is broken, and quietly
/// accepting it hides that.
#[tauri::command]
pub fn set_track_pan(state: State<'_, AppState>, track: usize, pan: f32) -> CmdResult<String> {
    set_track_pan_inner(&state, track, pan)
}

pub(crate) fn set_track_pan_inner(state: &AppState, track: usize, pan: f32) -> CmdResult<String> {
    if !pan.is_finite() || !(-1.0..=1.0).contains(&pan) {
        return Err(CommandError::InvalidTrackControl(format!(
            "pan must be finite and between -1.0 and 1.0; got {pan}"
        ))
        .into());
    }
    run_track_tool(
        state,
        "set_pan",
        serde_json::json!({ "track": track, "pan": pan }),
    )
}

/// Rename a track. Returns the new session head.
///
/// The name is the one thing on a track head that a user expects to be
/// able to change directly; until now it could only be changed by
/// asking the agent in a sentence.
#[tauri::command]
pub fn rename_track(state: State<'_, AppState>, track: usize, name: String) -> CmdResult<String> {
    rename_track_inner(&state, track, name)
}

pub(crate) fn rename_track_inner(
    state: &AppState,
    track: usize,
    name: String,
) -> CmdResult<String> {
    // Rejected here rather than clamped or defaulted: an empty name on
    // a track head is indistinguishable from a rendering bug, and the
    // tool's schema requires one character anyway — catching it here
    // gives the UI a message instead of a schema-validation string.
    if name.trim().is_empty() {
        return Err(
            CommandError::InvalidTrackControl("a track name cannot be empty".into()).into(),
        );
    }
    run_track_tool(
        state,
        "rename_track",
        serde_json::json!({ "track": track, "name": name }),
    )
}

/// Remove a track from the session. Returns the new session head.
///
/// Destructive in the ordinary sense — the node it appends can be
/// undone like any other, which is why this does not ask for
/// confirmation down here. Whether to ask is the UI's decision.
#[tauri::command]
pub fn remove_track(state: State<'_, AppState>, track: usize) -> CmdResult<String> {
    remove_track_inner(&state, track)
}

pub(crate) fn remove_track_inner(state: &AppState, track: usize) -> CmdResult<String> {
    run_track_tool(state, "remove_track", serde_json::json!({ "track": track }))
}

/// Duplicate a track, clips and mixer settings included. Returns the
/// new session head.
#[tauri::command]
pub fn duplicate_track(state: State<'_, AppState>, track: usize) -> CmdResult<String> {
    duplicate_track_inner(&state, track)
}

pub(crate) fn duplicate_track_inner(state: &AppState, track: usize) -> CmdResult<String> {
    run_track_tool(
        state,
        "duplicate_track",
        serde_json::json!({ "track": track }),
    )
}

/// Mute or unmute a track. Returns the new session head.
#[tauri::command]
pub fn set_track_muted(state: State<'_, AppState>, track: usize, muted: bool) -> CmdResult<String> {
    set_track_muted_inner(&state, track, muted)
}

pub(crate) fn set_track_muted_inner(
    state: &AppState,
    track: usize,
    muted: bool,
) -> CmdResult<String> {
    run_track_tool(
        state,
        "mute_track",
        serde_json::json!({ "track": track, "muted": muted }),
    )
}

/// Solo or un-solo a track. Returns the new session head.
#[tauri::command]
pub fn set_track_soloed(
    state: State<'_, AppState>,
    track: usize,
    soloed: bool,
) -> CmdResult<String> {
    set_track_soloed_inner(&state, track, soloed)
}

pub(crate) fn set_track_soloed_inner(
    state: &AppState,
    track: usize,
    soloed: bool,
) -> CmdResult<String> {
    run_track_tool(
        state,
        "solo_track",
        serde_json::json!({ "track": track, "solo": soloed }),
    )
}

// ---------------------------------------------------------------------------
// Volume automation (#95)
// ---------------------------------------------------------------------------

/// Range a drawn automation point may occupy, in dB.
///
/// Same reasoning as `GAIN_LIMIT_DB`: not a limit of the engine, a limit
/// on what a drag is allowed to write. The floor is lower than the
/// fader's because an automation curve's job includes ducking to
/// silence, and -60 dB is not silence.
const ENVELOPE_MIN_DB: f32 = -96.0;
const ENVELOPE_MAX_DB: f32 = 12.0;

/// Replace a clip's volume automation curve. Returns the new head.
///
/// Points are `(time_sec, gain_db)` measured from the clip's own start,
/// which is the coordinate system `set_clip_envelope` the tool already
/// uses. An empty list clears the curve.
///
/// Sorting is the tool's job and it already does it, so a caller that
/// drags a point past its neighbour does not have to reorder first.
#[tauri::command]
pub fn set_clip_envelope(
    state: State<'_, AppState>,
    track: usize,
    clip: usize,
    points: Vec<EnvelopePointSummary>,
) -> CmdResult<String> {
    set_clip_envelope_inner(&state, track, clip, points)
}

pub(crate) fn set_clip_envelope_inner(
    state: &AppState,
    track: usize,
    clip: usize,
    points: Vec<EnvelopePointSummary>,
) -> CmdResult<String> {
    for p in &points {
        if !p.time_sec.is_finite() || p.time_sec < 0.0 {
            return Err(CommandError::InvalidTrackControl(format!(
                "envelope times must be finite and >= 0; got {}",
                p.time_sec
            ))
            .into());
        }
        if !p.gain_db.is_finite() || !(ENVELOPE_MIN_DB..=ENVELOPE_MAX_DB).contains(&p.gain_db) {
            return Err(CommandError::InvalidTrackControl(format!(
                "envelope gain must be finite and between {ENVELOPE_MIN_DB} and \
                 {ENVELOPE_MAX_DB} dB; got {}",
                p.gain_db
            ))
            .into());
        }
    }
    let points: Vec<serde_json::Value> = points
        .iter()
        .map(|p| serde_json::json!({ "time_sec": p.time_sec, "gain_db": p.gain_db }))
        .collect();
    run_track_tool(
        state,
        "set_clip_envelope",
        serde_json::json!({
            "track_index": track,
            "clip_index": clip,
            "points": points,
        }),
    )
}

// ---------------------------------------------------------------------------
// Clip placement (#103)
// ---------------------------------------------------------------------------

/// Move one clip to a new start, in seconds from the top of the
/// timeline. Returns the new head.
///
/// The tool sorts the track's clips afterwards, so a clip dragged past
/// its neighbour comes back at a different index than it went in with.
/// Callers re-read `list_tracks` rather than assuming their index
/// survived — which the UI does anyway to reconcile.
#[tauri::command]
pub fn move_clip(
    state: State<'_, AppState>,
    track: usize,
    clip: usize,
    start_sec: f64,
) -> CmdResult<String> {
    move_clip_inner(&state, track, clip, start_sec)
}

pub(crate) fn move_clip_inner(
    state: &AppState,
    track: usize,
    clip: usize,
    start_sec: f64,
) -> CmdResult<String> {
    if !start_sec.is_finite() || start_sec < 0.0 {
        return Err(CommandError::InvalidTrackControl(format!(
            "clip start must be finite and >= 0; got {start_sec}"
        ))
        .into());
    }
    run_track_tool(
        state,
        "move_clip",
        serde_json::json!({ "track": track, "clip_index": clip, "start_sec": start_sec }),
    )
}

/// Remove one clip from a track. Returns the new head.
#[tauri::command]
pub fn remove_clip(state: State<'_, AppState>, track: usize, clip: usize) -> CmdResult<String> {
    remove_clip_inner(&state, track, clip)
}

pub(crate) fn remove_clip_inner(state: &AppState, track: usize, clip: usize) -> CmdResult<String> {
    run_track_tool(
        state,
        "remove_clip",
        serde_json::json!({ "track": track, "clip_index": clip }),
    )
}

// ---------------------------------------------------------------------------
// Templates
// ---------------------------------------------------------------------------

#[derive(Debug, serde::Serialize)]
pub struct TemplateInfo {
    pub name: String,
    pub description: String,
}

#[tauri::command]
pub async fn list_templates(app_handle: tauri::AppHandle) -> CmdResult<Vec<TemplateInfo>> {
    let resource_dir = app_handle
        .path()
        .resource_dir()
        .map_err(|e| format!("resource dir: {e}"))?;
    let templates_dir = resource_dir.join("templates");
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(&templates_dir) else {
        return Ok(out); // dev mode: no bundled resources
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let raw = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
        let v: serde_json::Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
        out.push(TemplateInfo {
            name: v["name"].as_str().unwrap_or("").to_string(),
            description: v["description"].as_str().unwrap_or("").to_string(),
        });
    }
    Ok(out)
}

#[tauri::command]
pub async fn apply_template(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
    name: String,
) -> CmdResult<String> {
    let resource_dir = app_handle
        .path()
        .resource_dir()
        .map_err(|e| format!("resource dir: {e}"))?;
    let templates_dir = resource_dir.join("templates");
    let entries = std::fs::read_dir(&templates_dir).map_err(|e| e.to_string())?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let raw = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
        let v: serde_json::Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
        if v["name"].as_str().unwrap_or("") != name {
            continue;
        }
        let session_state: session::SessionState =
            serde_json::from_value(v["state"].clone()).map_err(|e| e.to_string())?;
        let store_arc = state.store_handle().ok_or(CommandError::NoSession)?;
        let mut store = lock_std(&store_arc, "store")?;
        let node = session::SessionNode {
            id: session::NodeId([0u8; 32]),
            parent: store.head(),
            created_at: chrono::Utc::now(),
            label: Some(format!("Template: {name}")),
            reasoning: None,
            state: session_state,
            op: None,
        };
        let new_id = store.append(node).map_err(|e| e.to_string())?;
        return Ok(new_id.to_hex());
    }
    Err(format!("template '{name}' not found"))
}

// ---------------------------------------------------------------------------
// Agent (re)construction
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// memory: read / write the user's global + project memory files
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn read_memory(state: State<'_, AppState>, scope: String) -> CmdResult<String> {
    let store = state.memory_handle().ok_or(CommandError::NoMemory)?;
    let scope = memory::Scope::parse(&scope).ok_or(CommandError::InvalidMemoryScope(scope))?;
    Ok(store.read(scope).map_err(CommandError::from)?)
}

#[tauri::command]
pub async fn write_memory(
    state: State<'_, AppState>,
    scope: String,
    contents: String,
) -> CmdResult<()> {
    let store = state.memory_handle().ok_or(CommandError::NoMemory)?;
    let scope = memory::Scope::parse(&scope).ok_or(CommandError::InvalidMemoryScope(scope))?;
    store.write(scope, &contents).map_err(CommandError::from)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// agent profiles: list / read / upsert / delete + active selection
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct AgentProfileSummary {
    pub name: String,
    pub description: String,
    pub model: Option<AgentProfileModel>,
    pub tool_count: Option<usize>,
}

#[derive(Debug, Clone, serde::Deserialize, Serialize)]
pub struct AgentProfileModel {
    pub provider: String,
    pub id: String,
}

#[derive(Debug, Clone, serde::Deserialize, Serialize)]
pub struct AgentProfileContent {
    pub name: String,
    pub description: String,
    /// `null` = use global default model.
    pub model: Option<AgentProfileModel>,
    /// `null` = all tools; empty `[]` = no tools.
    pub tools: Option<Vec<String>>,
    pub body: String,
}

#[tauri::command]
pub async fn list_agent_profiles(
    state: State<'_, AppState>,
) -> CmdResult<Vec<AgentProfileSummary>> {
    if let Err(e) = state.reload_agent_profiles_from_disk() {
        tracing::warn!(error = ?e, "agent-profile reload failed; returning cached library");
    }
    let lib = state
        .agent_profiles
        .lock()
        .map_err(|_| CommandError::Poisoned("agent_profiles"))?;
    Ok(lib
        .profiles()
        .iter()
        .map(|p| AgentProfileSummary {
            name: p.name.clone(),
            description: p.description.clone(),
            model: p.model.as_ref().map(|m| AgentProfileModel {
                provider: m.provider.clone(),
                id: m.id.clone(),
            }),
            tool_count: p.tools.as_ref().map(|t| t.len()),
        })
        .collect())
}

#[tauri::command]
pub async fn read_agent_profile(
    state: State<'_, AppState>,
    name: String,
) -> CmdResult<AgentProfileContent> {
    let dir = agent_profiles_dir(&state)?;
    let path = dir.join(format!("{name}.md"));
    if !path.exists() {
        return Err(CommandError::InvalidPath(format!("profile `{name}` not found")).into());
    }
    let raw = std::fs::read_to_string(&path).map_err(CommandError::from)?;
    let parsed = parse_agent_profile_file(&raw)?;
    Ok(AgentProfileContent {
        name,
        description: parsed.description,
        model: parsed.model,
        tools: parsed.tools,
        body: parsed.body,
    })
}

#[tauri::command]
pub async fn upsert_agent_profile(
    state: State<'_, AppState>,
    name: String,
    content: AgentProfileContent,
) -> CmdResult<()> {
    if name != content.name {
        return Err(CommandError::InvalidAgentProfile(format!(
            "name `{name}` does not match content.name `{}`",
            content.name
        ))
        .into());
    }
    validate_agent_profile_name(&name)?;

    let dir = agent_profiles_dir(&state)?;
    std::fs::create_dir_all(&dir).map_err(CommandError::from)?;
    let path = dir.join(format!("{name}.md"));
    let serialised = serialise_agent_profile(&content);
    let mut tmp = tempfile::NamedTempFile::new_in(&dir).map_err(CommandError::from)?;
    std::io::Write::write_all(&mut tmp, serialised.as_bytes()).map_err(CommandError::from)?;
    tmp.flush().map_err(CommandError::from)?;
    tmp.persist(&path).map_err(|e| CommandError::Io(e.error))?;

    if let Err(e) = state.reload_agent_profiles_from_disk() {
        return Err(
            CommandError::InvalidAgentProfile(format!("saved but reload failed: {e}")).into(),
        );
    }
    // If the user is editing the currently active profile, rebuild
    // the agent so the override takes effect on the next turn.
    if state.active_agent_profile().as_deref() == Some(name.as_str()) {
        rebuild_agent(&state).await?;
    }
    Ok(())
}

#[tauri::command]
pub async fn delete_agent_profile(state: State<'_, AppState>, name: String) -> CmdResult<()> {
    validate_agent_profile_name(&name)?;
    let dir = agent_profiles_dir(&state)?;
    let path = dir.join(format!("{name}.md"));
    if path.exists() {
        std::fs::remove_file(&path).map_err(CommandError::from)?;
    }
    // If the deleted profile was active, drop the selection and
    // rebuild the agent against the default config.
    if state.active_agent_profile().as_deref() == Some(name.as_str()) {
        state
            .set_active_agent_profile(None)
            .map_err(CommandError::from)?;
        rebuild_agent(&state).await?;
    }
    if let Err(e) = state.reload_agent_profiles_from_disk() {
        return Err(
            CommandError::InvalidAgentProfile(format!("deleted but reload failed: {e}")).into(),
        );
    }
    Ok(())
}

#[tauri::command]
pub async fn get_active_agent_profile(state: State<'_, AppState>) -> CmdResult<Option<String>> {
    Ok(state.active_agent_profile())
}

#[tauri::command]
pub async fn set_active_agent_profile(
    state: State<'_, AppState>,
    name: Option<String>,
) -> CmdResult<()> {
    if let Some(ref n) = name {
        validate_agent_profile_name(n)?;
        // Ensure the profile exists on disk before pinning the
        // selection; the user sees the error immediately.
        let lib = state
            .agent_profiles
            .lock()
            .map_err(|_| CommandError::Poisoned("agent_profiles"))?;
        if lib.find(n).is_none() {
            return Err(
                CommandError::InvalidAgentProfile(format!("profile `{n}` does not exist")).into(),
            );
        }
    }
    state
        .set_active_agent_profile(name)
        .map_err(CommandError::from)?;
    rebuild_agent(&state).await?;
    Ok(())
}

fn agent_profiles_dir(state: &AppState) -> Result<PathBuf, CommandError> {
    let dir = state
        .agent_profiles_dir
        .lock()
        .map_err(|_| CommandError::Poisoned("agent_profiles_dir"))?
        .clone();
    if dir.as_os_str().is_empty() {
        return Err(CommandError::NoAgentProfilesDir);
    }
    Ok(dir)
}

fn validate_agent_profile_name(name: &str) -> Result<(), CommandError> {
    if name.is_empty() || name.len() > 64 {
        return Err(CommandError::InvalidAgentProfile(
            "agent profile name must be 1-64 characters".into(),
        ));
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(CommandError::InvalidAgentProfile(
            "agent profile name may only contain ASCII letters, digits, dash, or underscore".into(),
        ));
    }
    Ok(())
}

fn serialise_agent_profile(c: &AgentProfileContent) -> String {
    let mut out = String::from("---\n");
    out.push_str(&format!(
        "description: {}\n",
        quote_if_needed(&c.description)
    ));
    if let Some(m) = &c.model {
        out.push_str("model:\n");
        out.push_str(&format!("  provider: {}\n", quote_if_needed(&m.provider)));
        out.push_str(&format!("  id: {}\n", quote_if_needed(&m.id)));
    }
    if let Some(t) = &c.tools {
        let inner: Vec<String> = t.iter().map(|x| quote_if_needed(x.trim())).collect();
        out.push_str(&format!("tools: [{}]\n", inner.join(", ")));
    }
    out.push_str("---\n");
    out.push_str(&c.body);
    if !c.body.ends_with('\n') {
        out.push('\n');
    }
    out
}

struct ParsedAgentProfile {
    description: String,
    model: Option<AgentProfileModel>,
    tools: Option<Vec<String>>,
    body: String,
}

fn parse_agent_profile_file(raw: &str) -> Result<ParsedAgentProfile, CommandError> {
    let body_start = raw
        .strip_prefix("---\n")
        .or_else(|| raw.strip_prefix("---\r\n"))
        .ok_or_else(|| {
            CommandError::InvalidAgentProfile("missing frontmatter delimiters".into())
        })?;
    let end = body_start.find("\n---").ok_or_else(|| {
        CommandError::InvalidAgentProfile("missing frontmatter closing delimiter".into())
    })?;
    let fm = &body_start[..end];
    let body = body_start[end + "\n---".len()..]
        .trim_start_matches('\r')
        .trim_start_matches('\n')
        .to_string();

    let mut description = String::new();
    let mut model_provider: Option<String> = None;
    let mut model_id: Option<String> = None;
    let mut tools: Option<Vec<String>> = None;
    let mut in_model = false;
    for line in fm.lines() {
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let indented = line.starts_with("  ") || line.starts_with('\t');
        let trimmed = line.trim();
        if indented && in_model {
            let (k, v) = match trimmed.split_once(':') {
                Some(kv) => kv,
                None => continue,
            };
            let v = strip_quotes(v.trim()).to_string();
            match k.trim() {
                "provider" => model_provider = Some(v),
                "id" => model_id = Some(v),
                _ => {}
            }
            continue;
        }
        in_model = false;
        let (k, v) = match trimmed.split_once(':') {
            Some(kv) => kv,
            None => continue,
        };
        let v_raw = v.trim();
        match k.trim() {
            "description" => description = strip_quotes(v_raw).to_string(),
            "model" => in_model = true,
            "tools" => {
                if let Some(arr) = v_raw.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
                    tools = Some(split_array_items(arr));
                }
            }
            _ => {}
        }
    }
    let model = match (model_provider, model_id) {
        (Some(p), Some(i)) => Some(AgentProfileModel { provider: p, id: i }),
        _ => None,
    };
    Ok(ParsedAgentProfile {
        description,
        model,
        tools,
        body,
    })
}

/// Rebuild the agent from the current state. Called after the API key,
/// active provider, or project store changes; either or both
/// prerequisites being missing is fine and clears the agent rather than
/// failing.
/// Public wrapper called from `lib.rs::setup` after auto-initialising
/// the default project store. The internal helper is private; tests
/// don't currently call it directly so a thin pub façade keeps the
/// rest of the module's seal intact.
pub async fn rebuild_agent_public(state: &AppState) -> Result<(), CommandError> {
    rebuild_agent(state).await
}

/// Resolve the `(provider_id, api_key, model)` triple the agent should
/// be built with.
///
/// A profile that pins `model: { provider, id }` overrides *both* the
/// provider and the model id. When the pinned provider differs from the
/// globally active one, the cached API key belongs to the wrong
/// provider — sending it would 401 against the pinned provider's
/// endpoint — so the key is re-read from that provider's keychain slot
/// via `load_key`.
///
/// Split out of [`rebuild_agent`] as a pure function so the
/// provider/key pairing can be unit-tested without a keychain or a
/// live `AppState`.
fn resolve_effective_llm(
    global_provider_id: String,
    global_key: Option<String>,
    global_model: Option<String>,
    profile_model: Option<(String, String)>,
    load_key: impl Fn(&str) -> Option<String>,
) -> (String, Option<String>, Option<String>) {
    match profile_model {
        // Profile pins a provider that is not the active one: the
        // cached key is for a different service.
        Some((provider, id)) if provider != global_provider_id => {
            let key = load_key(&provider);
            (provider, key, Some(id))
        }
        // Profile pins a model on the provider that is already active.
        Some((provider, id)) => (provider, global_key, Some(id)),
        // No profile override: the global picker wins.
        None => (global_provider_id, global_key, global_model),
    }
}

async fn rebuild_agent(state: &AppState) -> Result<(), CommandError> {
    let store_handle = state.store_handle();
    let global_provider_id = state.active_provider_id();
    let global_chosen_model = state.model_for(&global_provider_id);
    let (profile_body, tool_whitelist, profile_model) = state.active_profile_overrides();

    let (effective_provider_id, api_key, effective_model) = resolve_effective_llm(
        global_provider_id,
        state.api_key_snapshot(),
        global_chosen_model,
        profile_model,
        ai::keychain::load_api_key,
    );
    let provider = ai::validate::provider_for(&effective_provider_id);

    // A provider that needs no key (a local Ollama daemon) still has to
    // produce an agent. Before `requires_api_key` existed, the match
    // below was the hard stop: no key meant no agent, so a keyless
    // provider could never be used at all.
    let api_key = match api_key {
        Some(k) => Some(k),
        None if !provider.requires_api_key() => Some(String::new()),
        None => None,
    };

    let mut guard = state.agent.lock().await;
    *guard = match (api_key, store_handle) {
        (Some(key), Some(store)) => {
            let mut cfg = ai::LlmConfig::new(provider, key);
            if let Some(m) = effective_model {
                if !m.trim().is_empty() {
                    cfg = cfg.with_model(m);
                }
            }
            // Point at a local server, a gateway, or a proxy. Absent, the
            // provider's own URL applies.
            if let Some(base) = ai::keychain::load_base_url(&effective_provider_id) {
                cfg = cfg.with_base_url(base);
            }
            let plan_first = state.plan_first.load(std::sync::atomic::Ordering::SeqCst);
            let mut agent = ai::Agent::new(
                cfg,
                Arc::clone(&state.dispatcher),
                store,
                Arc::clone(&state.engine),
                Arc::clone(&state.plan_notify),
                Arc::clone(&state.plan_steps_override),
                Arc::clone(&state.plan_rejected),
                state.clipboard_handle(),
            );
            agent.set_plan_first(plan_first);
            let agent = match state.memory_handle() {
                Some(mem) => agent.with_memory(mem),
                None => agent,
            };
            let agent = agent.with_skills(state.skills_handle());
            let agent = match profile_body {
                Some(body) => agent.with_profile_body(body),
                None => agent,
            };
            let agent = match tool_whitelist {
                Some(list) => agent.with_tool_whitelist(list),
                None => agent,
            };
            Some(agent)
        }
        _ => None,
    };
    Ok(())
}

// ---------------------------------------------------------------------------
// MCP servers (phase 5)
// ---------------------------------------------------------------------------

/// IPC shape for one server in the registration file. Mirrors
/// `mcp::McpServerConfig` but flattened so the TS bridge can render
/// it without an untagged-union discriminator.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct McpServerEntry {
    pub id: String,
    pub transport: String, // "stdio" | "sse"
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: std::collections::HashMap<String, String>,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub headers: std::collections::HashMap<String, String>,
    #[serde(default = "default_true_helper")]
    pub enabled: bool,
}

fn default_true_helper() -> bool {
    true
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct McpServerListEntry {
    pub id: String,
    pub transport: String,
    pub enabled: bool,
    pub status: mcp::McpServerStatus,
    pub tools_count: usize,
    pub last_error: Option<String>,
}

#[tauri::command]
pub async fn list_mcp_servers(state: State<'_, AppState>) -> CmdResult<Vec<McpServerListEntry>> {
    let cfg = state
        .mcp_config
        .lock()
        .map_err(|_| CommandError::Poisoned("mcp_config"))?
        .clone();
    let mut out: Vec<McpServerListEntry> = cfg
        .servers
        .iter()
        .map(|(id, c)| {
            let s = state.mcp.summary(id, c);
            McpServerListEntry {
                id: id.clone(),
                transport: s.transport.into(),
                enabled: c.enabled(),
                status: s.status,
                tools_count: s.tools_count,
                last_error: s.last_error,
            }
        })
        .collect();
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

#[tauri::command]
pub async fn read_mcp_server(state: State<'_, AppState>, id: String) -> CmdResult<McpServerEntry> {
    let cfg = state
        .mcp_config
        .lock()
        .map_err(|_| CommandError::Poisoned("mcp_config"))?
        .clone();
    let server = cfg
        .servers
        .get(&id)
        .ok_or_else(|| CommandError::InvalidMcpServer(format!("server `{id}` not found")))?
        .clone();
    Ok(mcp_server_to_entry(&id, &server))
}

#[tauri::command]
pub async fn upsert_mcp_server(
    state: State<'_, AppState>,
    id: String,
    entry: McpServerEntry,
) -> CmdResult<()> {
    if id != entry.id {
        return Err(CommandError::InvalidMcpServer(format!(
            "id `{id}` does not match entry.id `{}`",
            entry.id
        ))
        .into());
    }
    validate_mcp_id(&id)?;
    let config = entry_to_server_config(&entry)?;
    {
        let mut cfg = state
            .mcp_config
            .lock()
            .map_err(|_| CommandError::Poisoned("mcp_config"))?;
        cfg.servers.insert(id.clone(), config);
    }
    state.save_mcp_config().map_err(CommandError::from)?;

    // An edit can change the command, args, env, or disable the server
    // outright, so whatever the old process advertised is now suspect.
    // Stop it and drop its tools; the user restarts to pick the new
    // config up. Leaving the old tools registered would let the model
    // call into a process built from stale settings.
    state.mcp.stop(&id);
    {
        let mut dispatcher = lock_std(&state.dispatcher, "dispatcher")?;
        crate::mcp_tool::refresh_dispatcher_for_server(
            &mut dispatcher,
            Arc::clone(&state.mcp),
            &id,
        );
    }
    Ok(())
}

#[tauri::command]
pub async fn delete_mcp_server(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    validate_mcp_id(&id)?;
    state.mcp.stop(&id);
    {
        let mut cfg = state
            .mcp_config
            .lock()
            .map_err(|_| CommandError::Poisoned("mcp_config"))?;
        cfg.servers.remove(&id);
    }
    state.save_mcp_config().map_err(CommandError::from)?;

    // Stopping the server does not unregister its tools: without this
    // the model keeps seeing tools for a server that no longer exists
    // and every call fails at dispatch time with "is not running".
    {
        let mut dispatcher = lock_std(&state.dispatcher, "dispatcher")?;
        crate::mcp_tool::refresh_dispatcher_for_server(
            &mut dispatcher,
            Arc::clone(&state.mcp),
            &id,
        );
    }
    Ok(())
}

#[tauri::command]
pub async fn restart_mcp_server(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    validate_mcp_id(&id)?;
    let server = {
        let cfg = state
            .mcp_config
            .lock()
            .map_err(|_| CommandError::Poisoned("mcp_config"))?;
        cfg.servers
            .get(&id)
            .cloned()
            .ok_or_else(|| CommandError::InvalidMcpServer(format!("server `{id}` not found")))?
    };
    let resolve = |slot: &str| ai::keychain::load_api_key(slot);
    state
        .mcp
        .restart(&id, &server, resolve)
        .map_err(CommandError::InvalidMcpServer)?;
    // Re-register this server's remote tools in the dispatcher so the
    // agent sees the up-to-date `tools/list`. `start` may have added,
    // removed, or renamed tools relative to the previous run.
    {
        let mut dispatcher = lock_std(&state.dispatcher, "dispatcher")?;
        crate::mcp_tool::refresh_dispatcher_for_server(
            &mut dispatcher,
            Arc::clone(&state.mcp),
            &id,
        );
    }
    Ok(())
}

/// Start every enabled MCP server and register its tools.
///
/// Called once at startup from `lib.rs::run`, off the setup thread.
/// Failures are logged and recorded on the registry (so the Settings
/// list shows an `error` badge with the reason) but never abort the
/// sweep — one misconfigured server must not stop the others.
pub fn start_enabled_mcp_servers(state: &AppState) {
    let servers = {
        let Ok(cfg) = state.mcp_config.lock() else {
            tracing::warn!("mcp_config mutex poisoned; skipping MCP autostart");
            return;
        };
        cfg.servers
            .iter()
            .filter(|(_, c)| c.enabled())
            .map(|(id, c)| (id.clone(), c.clone()))
            .collect::<Vec<_>>()
    };

    for (id, config) in servers {
        match state.mcp.restart(&id, &config, ai::keychain::load_api_key) {
            Ok(()) => match state.dispatcher.lock() {
                Ok(mut dispatcher) => {
                    crate::mcp_tool::refresh_dispatcher_for_server(
                        &mut dispatcher,
                        Arc::clone(&state.mcp),
                        &id,
                    );
                    tracing::info!(server = %id, "mcp: started");
                }
                Err(_) => {
                    tracing::warn!(server = %id, "dispatcher mutex poisoned; tools not registered")
                }
            },
            Err(reason) => {
                // `restart` already stored the reason for the UI.
                tracing::warn!(server = %id, %reason, "mcp: autostart failed");
            }
        }
    }
}

fn validate_mcp_id(id: &str) -> Result<(), CommandError> {
    if id.is_empty() || id.len() > 64 {
        return Err(CommandError::InvalidMcpServer(
            "id must be 1-64 characters".into(),
        ));
    }
    if !id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(CommandError::InvalidMcpServer(
            "id may only contain ASCII letters, digits, dash, or underscore".into(),
        ));
    }
    Ok(())
}

fn mcp_server_to_entry(id: &str, cfg: &mcp::McpServerConfig) -> McpServerEntry {
    match cfg {
        mcp::McpServerConfig::Stdio {
            command,
            args,
            env,
            enabled,
        } => McpServerEntry {
            id: id.to_string(),
            transport: "stdio".into(),
            command: command.clone(),
            args: args.clone(),
            env: env.clone(),
            url: String::new(),
            headers: Default::default(),
            enabled: *enabled,
        },
        mcp::McpServerConfig::Sse {
            url,
            headers,
            enabled,
        } => McpServerEntry {
            id: id.to_string(),
            transport: "sse".into(),
            command: String::new(),
            args: Vec::new(),
            env: Default::default(),
            url: url.clone(),
            headers: headers.clone(),
            enabled: *enabled,
        },
    }
}

fn entry_to_server_config(e: &McpServerEntry) -> Result<mcp::McpServerConfig, CommandError> {
    match e.transport.as_str() {
        "stdio" => {
            if e.command.trim().is_empty() {
                return Err(CommandError::InvalidMcpServer(
                    "stdio transport requires a `command`".into(),
                ));
            }
            Ok(mcp::McpServerConfig::Stdio {
                command: e.command.clone(),
                args: e.args.clone(),
                env: e.env.clone(),
                enabled: e.enabled,
            })
        }
        "sse" => {
            if e.url.trim().is_empty() {
                return Err(CommandError::InvalidMcpServer(
                    "sse transport requires a `url`".into(),
                ));
            }
            Ok(mcp::McpServerConfig::Sse {
                url: e.url.clone(),
                headers: e.headers.clone(),
                enabled: e.enabled,
            })
        }
        other => Err(CommandError::InvalidMcpServer(format!(
            "unknown transport `{other}` (expected stdio|sse)"
        ))),
    }
}

/// At app startup, restore the active provider from the keychain (if
/// recorded) and load that provider's stored key into the in-memory
/// cache. If nothing is stored, this is a no-op and the frontend's
/// first action is to surface the Settings modal.
pub fn try_load_api_key_at_startup(state: &AppState) {
    if let Some(active) = ai::keychain::load_active_provider() {
        state.set_active_provider(active);
    }
    let provider_id = state.active_provider_id();
    if let Some(key) = ai::keychain::load_api_key(&provider_id) {
        state.set_api_key_cache(Some(key));
    }
}

// ---------------------------------------------------------------------------
// render_range
// ---------------------------------------------------------------------------

/// Export a user-defined selection (start_sec … end_sec) of the session node
/// identified by `node_id` to `out_path` as a 16-bit PCM WAV.
///
/// The frontend calls this when the user clicks "Export Selection" in the
/// chat header. Frame conversion is done here (seconds × sample_rate) so the
/// frontend never has to know the session's sample rate.
#[tauri::command]
pub async fn render_range(
    state: State<'_, AppState>,
    node_id: String,
    start_sec: f64,
    end_sec: f64,
    out_path: String,
) -> CmdResult<serde_json::Value> {
    let id = NodeId::from_hex(&node_id)
        .map_err(|_| CommandError::InvalidNodeId(node_id.clone()).to_string())?;
    let out = std::path::PathBuf::from(&out_path);

    let store_handle = state
        .store_handle()
        .ok_or_else(|| CommandError::NoSession.to_string())?;

    let session_node = {
        let store = lock_std(&store_handle, "store").map_err(|e| e.to_string())?;
        store
            .get(id)
            .map_err(|e| CommandError::Session(e).to_string())?
    };

    let sample_rate = session_node.state.sample_rate;
    let start_frame = (start_sec * sample_rate as f64) as u64;
    let end_frame = (end_sec * sample_rate as f64) as u64;
    let range = audio_engine::TimeRange {
        start_frame,
        end_frame,
    };

    let report = {
        let engine = lock_std(&state.engine, "engine").map_err(|e| e.to_string())?;
        engine
            .render_to_wav(&session_node.state, &out, Some(range))
            .map_err(|e| CommandError::Engine(e).to_string())?
    };

    Ok(serde_json::json!({
        "path": out_path,
        "frames_written": report.frames_written,
        "sample_rate": report.sample_rate,
        "channels": report.channels,
        "peak_dbfs": report.peak_dbfs,
        "summary": format!(
            "Exported selection ({:.2}s–{:.2}s) → {}",
            start_sec, end_sec, out_path
        ),
    }))
}

// ---------------------------------------------------------------------------
// batch_load — import multiple files into a multi-track session in one call.
// ---------------------------------------------------------------------------

/// Load multiple audio files into the session in sequence, creating one track
/// per file. Each file goes through the same `"load"` tool the agent uses,
/// so the session DAG gets a node per file with the same invariants as
/// single-file loads.
///
/// Returns `{ tracks_loaded, last_node_id }` so the frontend can refresh the
/// track list and waveform display without a second round trip.
#[tauri::command]
pub async fn batch_load(
    state: tauri::State<'_, AppState>,
    paths: Vec<String>,
) -> CmdResult<serde_json::Value> {
    if paths.is_empty() {
        return Err("no files provided".into());
    }

    let store_handle = state.store_handle().ok_or(CommandError::NoSession)?;
    let mut loaded = 0usize;
    let mut last_node_id: Option<String> = None;

    for path in &paths {
        let args = serde_json::json!({ "path": path });

        // Acquire store, engine, and dispatcher in separate scopes so we
        // don't hold all three locks simultaneously (avoids double-borrow
        // panics and keeps the lock windows narrow).
        let node_id_hex = {
            let mut store = lock_std(&store_handle, "store")?;
            let mut engine = lock_std(&state.engine, "engine")?;
            let dispatcher = lock_std(&state.dispatcher, "dispatcher")?;

            // The clipboard is not used by `load` but `ToolContext` requires
            // it. A local `None` suffices.
            let mut clipboard: Option<Vec<f32>> = None;
            let mut ctx = tools::ToolContext {
                store: &mut store,
                engine: &mut engine,
                user_message: "",
                clipboard: &mut clipboard,
            };

            dispatcher
                .invoke("load", args, &mut ctx)
                .map_err(|e| format!("load tool error: {e}"))?;

            // The load tool records the new head in the store. Read it now
            // while we still hold the store lock.
            store.head().map(|id| id.to_hex())
        };

        loaded += 1;
        last_node_id = node_id_hex;
    }

    Ok(serde_json::json!({
        "tracks_loaded": loaded,
        "last_node_id": last_node_id,
    }))
}

// ---------------------------------------------------------------------------
// Microphone recording commands.
// ---------------------------------------------------------------------------

pub struct RecorderState(pub std::sync::Mutex<Option<recorder::Recorder>>);

#[tauri::command]
pub fn start_recording(state: State<'_, RecorderState>) -> CmdResult<String> {
    let rec = recorder::Recorder::start().map_err(|e| e.to_string())?;
    *state.0.lock().unwrap() = Some(rec);
    Ok("recording started".into())
}

#[tauri::command]
pub fn stop_recording(
    state: State<'_, RecorderState>,
    output_path: String,
) -> CmdResult<serde_json::Value> {
    let rec = state
        .0
        .lock()
        .unwrap()
        .take()
        .ok_or_else(|| "no active recording".to_string())?;
    let path = std::path::PathBuf::from(&output_path);
    let (saved_path, sr, ch) = rec.stop_and_save(&path).map_err(|e| e.to_string())?;
    Ok(serde_json::json!({
        "path": saved_path.to_string_lossy(),
        "sample_rate": sr,
        "channels": ch
    }))
}

// ---------------------------------------------------------------------------
// install_bundled_skills (Task 2)
// ---------------------------------------------------------------------------

/// Copy the skills bundled inside the Tauri resource dir into
/// `~/.edytlab/skills/` on the very first launch.
///
/// The command is idempotent: if the target directory already contains
/// any `.md` file it exits immediately and returns `Ok(0)`, so user
/// edits or manually added skills are never overwritten.
///
/// Returns the number of files copied (0 when the skills dir already
/// exists, or when running in dev mode where the `bundled-skills/`
/// resource subdir is absent).
#[tauri::command]
pub fn install_bundled_skills(app: tauri::AppHandle) -> CmdResult<usize> {
    let resource_dir = app.path().resource_dir().map_err(|e| e.to_string())?;
    let bundled_dir = resource_dir.join("bundled-skills");

    let skills_dir = match app.path().home_dir() {
        Ok(home) => home.join(".edytlab").join("skills"),
        Err(_) => app
            .path()
            .app_data_dir()
            .map_err(|e| e.to_string())?
            .join(".edytlab")
            .join("skills"),
    };

    // If skills dir already has .md files, don't overwrite user customisations.
    if skills_dir.exists() {
        let has_skills = std::fs::read_dir(&skills_dir)
            .map_err(|e| e.to_string())?
            .filter_map(|e| e.ok())
            .any(|e| e.path().extension().and_then(|s| s.to_str()) == Some("md"));
        if has_skills {
            return Ok(0);
        }
    }

    if !bundled_dir.exists() {
        return Ok(0); // dev mode: bundled-skills/ not yet present
    }

    std::fs::create_dir_all(&skills_dir).map_err(|e| e.to_string())?;

    let mut count = 0usize;
    for entry in std::fs::read_dir(&bundled_dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let src = entry.path();
        if src.extension().and_then(|s| s.to_str()) != Some("md") {
            continue;
        }
        let dst = skills_dir.join(entry.file_name());
        std::fs::copy(&src, &dst).map_err(|e| e.to_string())?;
        count += 1;
    }
    Ok(count)
}

// ---------------------------------------------------------------------------
// install_plugin (Task 4)
// ---------------------------------------------------------------------------

fn edytlab_skills_dir(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    let base = match app.path().home_dir() {
        Ok(home) => home,
        Err(_) => app.path().app_data_dir().map_err(|e| e.to_string())?,
    };
    Ok(base.join(".edytlab").join("skills"))
}

fn edytlab_agents_dir(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    let base = match app.path().home_dir() {
        Ok(home) => home,
        Err(_) => app.path().app_data_dir().map_err(|e| e.to_string())?,
    };
    Ok(base.join(".edytlab").join("agents"))
}

/// Ceiling on the compressed archive we will pull down.
const PLUGIN_MAX_DOWNLOAD_BYTES: u64 = 50 * 1024 * 1024;

/// Ceiling on the *uncompressed* total. A zip's declared sizes are
/// attacker-controlled, so this is also enforced against bytes actually
/// written, not just the header claim.
const PLUGIN_MAX_UNPACKED_BYTES: u64 = 200 * 1024 * 1024;

/// Ceiling on entry count, so a zip full of empty files cannot spend
/// unbounded time and inodes.
const PLUGIN_MAX_ENTRIES: usize = 4096;

/// Network budget for fetching a plugin archive.
const PLUGIN_DOWNLOAD_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// Reject `org` / `repo` components that are empty, dot-only, or contain
/// anything outside the GitHub-legal set.
///
/// Dot-only components matter: `..` passes a naive "alphanumeric, dash,
/// underscore, dot" filter, and while it can only walk the *path* of a
/// hard-coded host (never the host itself) there is no legitimate repo
/// named `.` or `..`, so it is rejected rather than reasoned about.
fn valid_github_component(s: &str) -> bool {
    !s.is_empty()
        && !s.chars().all(|c| c == '.')
        && s.chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.')
}

/// Download and unpack a plugin archive into a temp directory.
///
/// The returned [`tempfile::TempDir`] owns the extraction root: dropping
/// it removes the tree, so every early return below cleans up without a
/// bespoke error path.
fn download_github_plugin(repo: &str) -> Result<(std::path::PathBuf, tempfile::TempDir), String> {
    let parts: Vec<&str> = repo.splitn(2, '/').collect();
    if parts.len() != 2 {
        return Err(format!("invalid GitHub repo `{repo}`; expected `org/repo`"));
    }
    if !valid_github_component(parts[0]) || !valid_github_component(parts[1]) {
        return Err(format!(
            "invalid GitHub repo `{repo}`; org and repo must be alphanumeric/-/_/."
        ));
    }
    let url = format!("https://github.com/{repo}/archive/refs/heads/main.zip");
    let client = reqwest::blocking::Client::builder()
        .timeout(PLUGIN_DOWNLOAD_TIMEOUT)
        .build()
        .map_err(|e| e.to_string())?;
    let response = client
        .get(&url)
        .send()
        .map_err(|e| format!("fetch {url}: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("HTTP {} fetching {url}", response.status()));
    }

    // Refuse an oversized body up front when the server declares one,
    // then cap the read regardless — `Content-Length` is a hint, not a
    // promise.
    if let Some(len) = response.content_length() {
        if len > PLUGIN_MAX_DOWNLOAD_BYTES {
            return Err(format!(
                "plugin archive is {len} bytes, over the {PLUGIN_MAX_DOWNLOAD_BYTES} byte limit"
            ));
        }
    }
    let mut bytes = Vec::new();
    let mut capped = std::io::Read::take(response, PLUGIN_MAX_DOWNLOAD_BYTES + 1);
    std::io::Read::read_to_end(&mut capped, &mut bytes).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > PLUGIN_MAX_DOWNLOAD_BYTES {
        return Err(format!(
            "plugin archive exceeds the {PLUGIN_MAX_DOWNLOAD_BYTES} byte limit"
        ));
    }

    // `TempDir` gives RAII cleanup and a name that is not predictable,
    // so a pre-existing attacker-created path cannot be hijacked.
    let tmp = tempfile::Builder::new()
        .prefix("edytlab-plugin-")
        .tempdir()
        .map_err(|e| e.to_string())?;

    let extract_dir = tmp.path().join("extracted");
    std::fs::create_dir_all(&extract_dir).map_err(|e| e.to_string())?;

    let mut archive =
        zip::ZipArchive::new(std::io::Cursor::new(&bytes)).map_err(|e| e.to_string())?;
    if archive.len() > PLUGIN_MAX_ENTRIES {
        return Err(format!(
            "plugin archive has {} entries, over the {PLUGIN_MAX_ENTRIES} limit",
            archive.len()
        ));
    }

    let mut unpacked: u64 = 0;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).map_err(|e| e.to_string())?;
        // `enclosed_name` rejects absolute paths, drive prefixes, and any
        // `..` that escapes the root — the zip-slip guard.
        let outpath = match file.enclosed_name() {
            Some(p) => extract_dir.join(p),
            None => return Err(format!("zip entry '{}' has unsafe path", file.name())),
        };
        if file.is_dir() {
            std::fs::create_dir_all(&outpath).map_err(|e| e.to_string())?;
            continue;
        }
        if let Some(parent) = outpath.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        // Cap on bytes actually written rather than the header's claimed
        // size, which an attacker controls.
        let remaining = PLUGIN_MAX_UNPACKED_BYTES.saturating_sub(unpacked);
        let mut outfile = std::fs::File::create(&outpath).map_err(|e| e.to_string())?;
        let written = std::io::copy(
            &mut std::io::Read::take(&mut file, remaining + 1),
            &mut outfile,
        )
        .map_err(|e| e.to_string())?;
        unpacked += written;
        if unpacked > PLUGIN_MAX_UNPACKED_BYTES {
            return Err(format!(
                "plugin archive unpacks to more than {PLUGIN_MAX_UNPACKED_BYTES} bytes"
            ));
        }
    }

    // GitHub zip extracts to a single top-level dir: <repo-name>-main/
    let plugin_dir = std::fs::read_dir(&extract_dir)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok())
        .find(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .map(|e| e.path())
        .ok_or_else(|| "extracted zip has no top-level directory".to_string())?;

    Ok((plugin_dir, tmp))
}

/// Install a plugin from a GitHub repo (`github:org/repo`) or a local
/// directory (`local:/abs/path`).
///
/// For GitHub sources the command downloads the `main` branch zip,
/// extracts it to a temp directory, then delegates to
/// `skills::plugin::PluginManifest` to copy skill and agent files into
/// the user's `~/.edytlab/` directories.
///
/// MCP servers declared by the manifest are registered too, but always
/// **disabled** — see [`register_plugin_mcp_servers`].
///
/// Returns a JSON summary: counts of installed skills and agents, the
/// server ids that were registered, and any that were skipped because
/// the id was already taken.
#[tauri::command]
pub fn install_plugin(
    source: String,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> CmdResult<serde_json::Value> {
    // `_tmp` is bound rather than dropped so the extracted tree outlives
    // the install; every return below cleans it up automatically.
    let (plugin_dir, _tmp) = if let Some(repo) = source.strip_prefix("github:") {
        let (dir, tmp) = download_github_plugin(repo)?;
        (dir, Some(tmp))
    } else if let Some(raw) = source.strip_prefix("local:") {
        // The user names this path themselves in the UI, so reading from
        // it is their choice, not an escalation. `..` is still refused so
        // a relative source cannot quietly climb out of the working
        // directory into somewhere they did not mean to point at.
        let p = std::path::PathBuf::from(raw);
        if p.components().any(|c| c == std::path::Component::ParentDir) {
            return Err("local: path must not contain `..` components".into());
        }
        (p, None)
    } else {
        return Err(format!(
            "unknown source `{source}`. Use `github:org/repo` or `local:/path/to/dir`"
        ));
    };

    let manifest_path = plugin_dir.join("edytlab-plugin.json");
    if !manifest_path.exists() {
        return Err(format!(
            "no edytlab-plugin.json found in `{}`",
            plugin_dir.display()
        ));
    }

    let manifest = skills::plugin::PluginManifest::load(&manifest_path)?;

    let skills_installed = manifest.install_skills(&plugin_dir, &edytlab_skills_dir(&app)?)?;
    let agents_installed = manifest.install_agents(&plugin_dir, &edytlab_agents_dir(&app)?)?;
    let mcp = register_plugin_mcp_servers(&state, &manifest.mcp_servers)?;

    let mut summary = format!(
        "Installed plugin '{}' v{}: {} skill(s), {} agent(s)",
        manifest.name,
        manifest.version,
        skills_installed.len(),
        agents_installed.len(),
    );
    if !mcp.registered.is_empty() {
        summary.push_str(&format!(
            ", {} MCP server(s) added but left disabled — review each command before enabling",
            mcp.registered.len()
        ));
    }
    if !mcp.skipped.is_empty() {
        summary.push_str(&format!(
            ", {} MCP server(s) skipped (id already in use)",
            mcp.skipped.len()
        ));
    }

    Ok(serde_json::json!({
        "name": manifest.name,
        "version": manifest.version,
        "skills_installed": skills_installed.len(),
        "agents_installed": agents_installed.len(),
        // Kept for back-compat with callers that only wanted the names.
        "mcp_keys": mcp.registered,
        "mcp_registered": mcp.registered,
        "mcp_skipped": mcp.skipped,
        "summary": summary,
    }))
}

/// Outcome of merging a manifest's `mcpServers` into the user's config.
#[derive(Debug, Default)]
struct PluginMcpOutcome {
    registered: Vec<String>,
    /// Ids already present in the user's config. A plugin must not be
    /// able to redefine a server the user has configured — that would
    /// swap out a command line they vetted.
    skipped: Vec<String>,
}

/// Merge plugin-declared MCP servers into `~/.edytlab/mcp.json`.
///
/// Two rules, both load-bearing:
///
/// 1. **Everything lands disabled.** An MCP server config is a command
///    line, and enabled servers are spawned at launch, so registering a
///    plugin's server in an enabled state would make *installing* a
///    plugin equivalent to *running* whatever it names. Enabling stays a
///    deliberate per-server act with the command visible in Settings.
/// 2. **Existing ids are never overwritten.** Otherwise a plugin could
///    redefine a server the user already trusts and inherit its
///    keychain-backed secrets.
///
/// Malformed entries are reported rather than silently dropped: a
/// manifest that declares a server the app cannot parse is a broken
/// plugin, and saying so beats pretending it installed.
fn register_plugin_mcp_servers(
    state: &AppState,
    declared: &std::collections::HashMap<String, serde_json::Value>,
) -> Result<PluginMcpOutcome, CommandError> {
    let mut outcome = PluginMcpOutcome::default();
    if declared.is_empty() {
        return Ok(outcome);
    }

    {
        let mut cfg = state
            .mcp_config
            .lock()
            .map_err(|_| CommandError::Poisoned("mcp_config"))?;
        // Deterministic order so the summary reads the same twice.
        let mut ids: Vec<&String> = declared.keys().collect();
        ids.sort();
        for id in ids {
            validate_mcp_id(id)?;
            if cfg.servers.contains_key(id) {
                outcome.skipped.push(id.clone());
                continue;
            }
            let parsed: mcp::McpServerConfig = serde_json::from_value(declared[id].clone())
                .map_err(|e| {
                    CommandError::InvalidMcpServer(format!(
                        "plugin declares MCP server `{id}` in an unrecognised shape: {e}"
                    ))
                })?;
            cfg.servers.insert(id.clone(), parsed.disabled());
            outcome.registered.push(id.clone());
        }
    }

    if !outcome.registered.is_empty() {
        state.save_mcp_config().map_err(CommandError::from)?;
    }
    Ok(outcome)
}

// ---------------------------------------------------------------------------
// Helpers re-exported for tests.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod batch_load_tests {
    #[test]
    fn empty_paths_is_err() {
        let paths: Vec<String> = vec![];
        let result: Result<(), &str> = if paths.is_empty() {
            Err("no files provided")
        } else {
            Ok(())
        };
        assert!(result.is_err());
    }
}

#[cfg(test)]
pub(crate) fn open_project_for_test(
    state: &AppState,
    path: &std::path::Path,
) -> Result<ProjectInfo, CommandError> {
    open_project_inner(state, path.to_path_buf())
}

#[cfg(test)]
mod tests {
    //! Pure state / helper tests. The Tauri-IPC happy path is covered
    //! by the integration test under `tests/commands_mock.rs`.

    use super::*;
    use session::SessionState;
    use tempfile::tempdir;

    fn empty_session_state() -> SessionState {
        SessionState {
            tracks: Vec::new(),
            bus_routing: session::BusGraph::default(),
            master_chain: Vec::new(),
            tempo_map: session::TempoMap::default(),
            key_map: None,
            transcript: None,
            sample_rate: 48_000,
            length_samples: 0,
            annotations: Vec::new(),
        }
    }

    #[test]
    fn open_project_creates_store_and_sets_dir() {
        let tmp = tempdir().unwrap();
        let state = AppState::new();

        let info = open_project_for_test(&state, tmp.path()).unwrap();
        assert_eq!(info.path, tmp.path().to_str().unwrap());
        assert!(info.head.is_none(), "fresh project has no head");
        assert!(state.store_handle().is_some(), "store handle was set");
        assert_eq!(
            state.project_dir.lock().unwrap().as_deref(),
            Some(tmp.path())
        );
    }

    #[test]
    fn open_project_rejects_relative_path() {
        let state = AppState::new();
        let err = open_project_for_test(&state, std::path::Path::new("relative/path"))
            .expect_err("should reject relative path");
        assert!(matches!(err, CommandError::InvalidPath(_)), "got {err:?}");
    }

    #[test]
    fn get_node_round_trips_an_appended_node() {
        let tmp = tempdir().unwrap();
        let state = AppState::new();
        open_project_for_test(&state, tmp.path()).unwrap();

        let store_handle = state.store_handle().expect("store");
        let id = {
            let mut store = store_handle.lock().unwrap();
            store
                .append(session::SessionNode {
                    id: session::NodeId([0u8; 32]),
                    parent: None,
                    created_at: chrono::Utc::now(),
                    label: Some("test".into()),
                    reasoning: None,
                    state: empty_session_state(),
                    op: None,
                })
                .unwrap()
        };

        // Look the node up via the helper used by the command body.
        let node = {
            let store = store_handle.lock().unwrap();
            store.get(id).unwrap()
        };
        assert_eq!(node.id, id);
    }

    // -----------------------------------------------------------------
    // Mixer controls (#94)
    // -----------------------------------------------------------------

    /// A project with `n` tracks at the head, ready for the mixer
    /// commands. The tracks carry no clips — gain, pan, mute and solo
    /// are all track metadata, and none of them needs audio to be set.
    fn state_with_tracks(tmp: &std::path::Path, n: usize) -> AppState {
        let state = AppState::new();
        open_project_for_test(&state, tmp).unwrap();
        let mut session_state = empty_session_state();
        for i in 0..n {
            session_state.tracks.push(session::Track {
                id: session::TrackId::new(),
                name: format!("track {i}"),
                clips: Vec::new(),
                gain_db: 0.0,
                pan: 0.0,
                muted: false,
                soloed: false,
                sends: Vec::new(),
                effects: Vec::new(),
            });
        }
        let handle = state.store_handle().unwrap();
        let mut store = handle.lock().unwrap();
        store
            .append(session::SessionNode {
                id: session::NodeId([0u8; 32]),
                parent: None,
                created_at: chrono::Utc::now(),
                label: Some("fixture".into()),
                reasoning: None,
                state: session_state,
                op: None,
            })
            .unwrap();
        drop(store);
        state
    }

    fn head_tracks(state: &AppState) -> Vec<session::Track> {
        let handle = state.store_handle().unwrap();
        let store = handle.lock().unwrap();
        let head = store.head().unwrap();
        store.get(head).unwrap().state.tracks
    }

    fn node_count(state: &AppState) -> usize {
        let handle = state.store_handle().unwrap();
        let store = handle.lock().unwrap();
        let mut n = 0usize;
        let mut cursor = store.head();
        while let Some(id) = cursor {
            n += 1;
            cursor = store.get(id).unwrap().parent;
        }
        n
    }

    /// The acceptance criterion: each control writes the value through
    /// to the session, and does it in exactly one node so a single undo
    /// takes it back.
    #[test]
    fn each_mixer_command_appends_one_node_and_round_trips() {
        let tmp = tempdir().unwrap();
        let state = state_with_tracks(tmp.path(), 2);
        let before = node_count(&state);

        set_track_gain_inner(&state, 1, -6.5).unwrap();
        assert_eq!(head_tracks(&state)[1].gain_db, -6.5);
        assert_eq!(node_count(&state), before + 1);

        set_track_pan_inner(&state, 1, -1.0).unwrap();
        assert_eq!(head_tracks(&state)[1].pan, -1.0);
        assert_eq!(node_count(&state), before + 2);

        set_track_muted_inner(&state, 0, true).unwrap();
        assert!(head_tracks(&state)[0].muted);
        assert_eq!(node_count(&state), before + 3);

        set_track_soloed_inner(&state, 0, true).unwrap();
        assert!(head_tracks(&state)[0].soloed);
        assert_eq!(node_count(&state), before + 4);

        // Track 1's earlier edits survived every later append — the
        // commands build on the head rather than on the fixture.
        let tracks = head_tracks(&state);
        assert_eq!(tracks[1].gain_db, -6.5);
        assert_eq!(tracks[1].pan, -1.0);
    }

    /// Out-of-range input is refused at the command, not left to the
    /// slider's `min`/`max`. A rejected call must also leave the
    /// session alone — a half-applied mixer move is worse than none.
    #[test]
    fn mixer_commands_reject_out_of_range_values() {
        let tmp = tempdir().unwrap();
        let state = state_with_tracks(tmp.path(), 1);
        let before = node_count(&state);

        for bad in [1.5f32, -1.5, f32::NAN, f32::INFINITY] {
            let err = set_track_pan_inner(&state, 0, bad).expect_err("pan {bad} should be refused");
            assert!(err.contains("pan must be"), "got: {err}");
        }
        for bad in [100.0f32, -1000.0, f32::NAN] {
            let err =
                set_track_gain_inner(&state, 0, bad).expect_err("gain {bad} should be refused");
            assert!(err.contains("gain must be"), "got: {err}");
        }
        assert_eq!(node_count(&state), before, "a refused call wrote a node");
        assert_eq!(head_tracks(&state)[0].pan, 0.0);
    }

    /// The tool's own validation has to reach the user. Without the
    /// `ToolResult::Error` arm this returned the unchanged head and the
    /// UI would have shown success.
    #[test]
    fn an_out_of_range_track_index_is_an_error_not_a_silent_no_op() {
        let tmp = tempdir().unwrap();
        let state = state_with_tracks(tmp.path(), 1);
        let before = node_count(&state);

        let err = set_track_muted_inner(&state, 7, true).expect_err("track 7 does not exist");
        assert!(err.contains("out of range"), "got: {err}");
        assert_eq!(node_count(&state), before);
    }

    #[test]
    fn list_tracks_reports_pan_and_solo() {
        let tmp = tempdir().unwrap();
        let state = state_with_tracks(tmp.path(), 1);
        set_track_pan_inner(&state, 0, 0.75).unwrap();
        set_track_soloed_inner(&state, 0, true).unwrap();

        // `list_tracks` takes `State`, which needs a running app; its
        // body is a projection of the head node, so assert on the same
        // source the projection reads.
        let t = &head_tracks(&state)[0];
        assert_eq!(t.pan, 0.75);
        assert!(t.soloed);
    }

    // -----------------------------------------------------------------
    // Volume automation (#95)
    // -----------------------------------------------------------------

    /// A track with one clip, so `set_clip_envelope` has something to
    /// write to. The clip points at no real file — nothing here decodes
    /// audio, and the envelope is clip metadata.
    fn state_with_one_clip(tmp: &std::path::Path) -> AppState {
        let state = state_with_tracks(tmp, 1);
        let handle = state.store_handle().unwrap();
        let mut store = handle.lock().unwrap();
        let head = store.head().unwrap();
        let mut session_state = store.get(head).unwrap().state;
        session_state.tracks[0].clips.push(session::Clip {
            source_path: tmp.join("clip.wav"),
            start_in_track: 0,
            source_offset: 0,
            length: 48_000,
            content_hash: None,
            time_stretch_factor: None,
            pitch_shift_semitones: None,
            beat_grid: None,
            volume_envelope: Vec::new(),
        });
        store
            .append(session::SessionNode {
                id: session::NodeId([0u8; 32]),
                parent: None,
                created_at: chrono::Utc::now(),
                label: Some("clip fixture".into()),
                reasoning: None,
                state: session_state,
                op: None,
            })
            .unwrap();
        drop(store);
        state
    }

    fn head_envelope(state: &AppState) -> Vec<session::EnvelopePoint> {
        head_tracks(state)[0].clips[0].volume_envelope.clone()
    }

    /// Points go in as seconds and come back as samples on the session's
    /// own rate — the conversion the tool does. The fixture is 48 kHz.
    #[test]
    fn setting_an_envelope_appends_one_node_and_round_trips() {
        let tmp = tempdir().unwrap();
        let state = state_with_one_clip(tmp.path());
        let before = node_count(&state);

        set_clip_envelope_inner(
            &state,
            0,
            0,
            vec![
                EnvelopePointSummary {
                    time_sec: 0.5,
                    gain_db: -6.0,
                },
                EnvelopePointSummary {
                    time_sec: 0.0,
                    gain_db: 0.0,
                },
            ],
        )
        .unwrap();

        assert_eq!(node_count(&state), before + 1, "one gesture, one node");
        let env = head_envelope(&state);
        assert_eq!(env.len(), 2);
        // Sorted on the way in, so a point dragged past its neighbour
        // does not need the caller to reorder first.
        assert_eq!(env[0].time_samples, 0);
        assert_eq!(env[0].gain_db, 0.0);
        assert_eq!(env[1].time_samples, 24_000);
        assert_eq!(env[1].gain_db, -6.0);
    }

    #[test]
    fn an_empty_point_list_clears_the_curve() {
        let tmp = tempdir().unwrap();
        let state = state_with_one_clip(tmp.path());
        set_clip_envelope_inner(
            &state,
            0,
            0,
            vec![EnvelopePointSummary {
                time_sec: 0.1,
                gain_db: -3.0,
            }],
        )
        .unwrap();
        assert_eq!(head_envelope(&state).len(), 1);

        set_clip_envelope_inner(&state, 0, 0, Vec::new()).unwrap();
        assert!(head_envelope(&state).is_empty());
    }

    /// Out-of-range input is refused at the command, and a refused call
    /// leaves the curve alone.
    #[test]
    fn envelope_values_are_validated_before_anything_is_written() {
        let tmp = tempdir().unwrap();
        let state = state_with_one_clip(tmp.path());
        let before = node_count(&state);

        for bad in [200.0f32, -300.0, f32::NAN] {
            let err = set_clip_envelope_inner(
                &state,
                0,
                0,
                vec![EnvelopePointSummary {
                    time_sec: 0.0,
                    gain_db: bad,
                }],
            )
            .expect_err("gain out of range should be refused");
            assert!(err.contains("envelope gain"), "got: {err}");
        }

        let err = set_clip_envelope_inner(
            &state,
            0,
            0,
            vec![EnvelopePointSummary {
                time_sec: -1.0,
                gain_db: 0.0,
            }],
        )
        .expect_err("negative time should be refused");
        assert!(err.contains("envelope times"), "got: {err}");

        assert_eq!(node_count(&state), before, "a refused call wrote a node");
        assert!(head_envelope(&state).is_empty());
    }

    /// The tool's own range check has to reach the user rather than
    /// returning the unchanged head as if it had worked.
    #[test]
    fn a_bad_clip_index_is_an_error() {
        let tmp = tempdir().unwrap();
        let state = state_with_one_clip(tmp.path());
        let err =
            set_clip_envelope_inner(&state, 0, 9, Vec::new()).expect_err("clip 9 does not exist");
        assert!(err.contains("clip_index"), "got: {err}");
    }

    #[test]
    fn first_token_handles_typical_labels() {
        assert_eq!(super::first_token("load /tmp/foo.wav"), Some("load"));
        assert_eq!(super::first_token("normalize"), Some("normalize"));
        assert_eq!(super::first_token(""), None);
        assert_eq!(super::first_token("   "), None);
        assert_eq!(super::first_token("  trim  end"), Some("trim"));
    }

    #[test]
    fn command_error_serializes_to_string_at_boundary() {
        let err: String = CommandError::NoSession.into();
        assert!(err.contains("no session loaded"), "got: {err}");

        let err: String = CommandError::NoAgent.into();
        assert!(err.contains("no agent configured"), "got: {err}");
    }

    #[test]
    fn list_capabilities_returns_built_in_tools() {
        let state = AppState::new();
        let dispatcher = state.dispatcher.lock().unwrap();
        let schemas = dispatcher.tool_schemas();
        drop(dispatcher);

        // Sanity: the dispatcher exposes at least the tools the user
        // sees on day one — `load`, `gain`, and `render_preview` are
        // load-bearing for the rest of the chat-UI work.
        let names: Vec<&str> = schemas
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|s| s.get("name").and_then(|v| v.as_str()))
            .collect();
        assert!(names.contains(&"load"), "missing load tool");
        assert!(names.contains(&"gain"), "missing gain tool");
        assert!(
            names.contains(&"render_preview"),
            "missing render_preview tool"
        );

        // Category map covers the names we explicitly group.
        assert_eq!(super::category_for("load"), "session");
        assert_eq!(super::category_for("transcribe"), "analysis");
        assert_eq!(super::category_for("fork_node"), "history");
        assert_eq!(super::category_for("label"), "annotation");
        // Anything unmapped lands in `audio` so new tools render without
        // a code change to `category_for`.
        assert_eq!(super::category_for("some_future_tool"), "audio");
    }

    // ------------------------------------------------------------------
    // apply_blacklist tests
    // ------------------------------------------------------------------

    /// Helper: exercise the filtering logic used by `apply_blacklist` for the
    /// `Some(whitelist)` branch without needing a real `AppState`.
    fn filter_whitelist(whitelist: Vec<String>, blacklist: &[String]) -> Vec<String> {
        whitelist
            .into_iter()
            .filter(|t| !blacklist.contains(t))
            .collect()
    }

    #[test]
    fn apply_blacklist_filters_disabled_from_profile_whitelist() {
        // With a profile whitelist [load, gain, fade], disabling [gain]
        // should produce [load, fade].
        let whitelist = vec!["load".to_string(), "gain".to_string(), "fade".to_string()];
        let blacklist = vec!["gain".to_string()];
        let result = filter_whitelist(whitelist, &blacklist);
        assert_eq!(result, vec!["load", "fade"]);
    }

    #[test]
    fn apply_blacklist_empty_blacklist_returns_whitelist_unchanged() {
        let whitelist = vec!["load".to_string(), "gain".to_string()];
        let blacklist: Vec<String> = vec![];
        let result = filter_whitelist(whitelist.clone(), &blacklist);
        assert_eq!(result, whitelist);
    }

    #[test]
    fn apply_blacklist_blacklist_not_in_whitelist_is_noop() {
        // Disabling a tool that isn't in the whitelist should not change it.
        let whitelist = vec!["load".to_string(), "gain".to_string()];
        let blacklist = vec!["unknown_tool".to_string()];
        let result = filter_whitelist(whitelist.clone(), &blacklist);
        assert_eq!(result, whitelist);
    }

    #[test]
    fn apply_blacklist_none_whitelist_returns_all_tools_minus_blacklist() {
        // When there is no profile whitelist (None), apply_blacklist should
        // expand to all registered tool names and then remove the blacklisted
        // ones — returning Some(filtered_all_tools).
        let state = AppState::new();
        let all_names = state.all_tool_names();
        assert!(
            !all_names.is_empty(),
            "dispatcher must expose at least one tool"
        );

        // Pick any two real tool names to blacklist.
        let blacklist: Vec<String> = all_names.iter().take(2).cloned().collect();

        let result = apply_blacklist(None, &blacklist, &state);

        // Result must be Some.
        let filtered =
            result.expect("expected Some when whitelist is None and blacklist is non-empty");

        // Must not contain the blacklisted tools.
        for name in &blacklist {
            assert!(
                !filtered.contains(name),
                "blacklisted tool '{name}' should not appear in result"
            );
        }

        // Must contain every tool that was NOT blacklisted.
        for name in &all_names {
            if !blacklist.contains(name) {
                assert!(
                    filtered.contains(name),
                    "non-blacklisted tool '{name}' should appear in result"
                );
            }
        }

        // The filtered list length should equal all_names minus the blacklist.
        assert_eq!(filtered.len(), all_names.len() - blacklist.len());
    }

    // -----------------------------------------------------------------
    // resolve_effective_llm — provider / key / model pairing
    // -----------------------------------------------------------------

    /// A profile pinning a model on a DIFFERENT provider must pull that
    /// provider's key from the keychain. Using the cached key (which
    /// belongs to the globally active provider) 401s at runtime.
    #[test]
    fn profile_on_other_provider_reloads_that_providers_key() {
        let (provider, key, model) = super::resolve_effective_llm(
            "anthropic".into(),
            Some("anthropic-key".into()),
            Some("claude-sonnet-4-6".into()),
            Some(("openai".into(), "gpt-4o-mini".into())),
            |p| {
                assert_eq!(p, "openai", "must look up the pinned provider");
                Some("openai-key".to_string())
            },
        );
        assert_eq!(provider, "openai");
        assert_eq!(key.as_deref(), Some("openai-key"));
        assert_eq!(model.as_deref(), Some("gpt-4o-mini"));
    }

    /// Same provider as the active one: the cached key is already
    /// correct, so no keychain round-trip is needed.
    #[test]
    fn profile_on_same_provider_keeps_cached_key() {
        let (provider, key, model) = super::resolve_effective_llm(
            "anthropic".into(),
            Some("anthropic-key".into()),
            Some("claude-sonnet-4-6".into()),
            Some(("anthropic".into(), "claude-opus-4-7".into())),
            |_| panic!("must not hit the keychain for the active provider"),
        );
        assert_eq!(provider, "anthropic");
        assert_eq!(key.as_deref(), Some("anthropic-key"));
        assert_eq!(model.as_deref(), Some("claude-opus-4-7"));
    }

    /// No profile override: the global picker's provider, key, and
    /// model all pass through untouched.
    #[test]
    fn no_profile_uses_global_picker() {
        let (provider, key, model) = super::resolve_effective_llm(
            "openrouter".into(),
            Some("or-key".into()),
            Some("anthropic/claude-sonnet-4-6".into()),
            None,
            |_| panic!("must not hit the keychain when no profile is active"),
        );
        assert_eq!(provider, "openrouter");
        assert_eq!(key.as_deref(), Some("or-key"));
        assert_eq!(model.as_deref(), Some("anthropic/claude-sonnet-4-6"));
    }

    /// A pinned provider with no stored key yields `None` rather than
    /// silently falling back to the wrong provider's key. `rebuild_agent`
    /// then leaves the agent unbuilt, which surfaces as the familiar
    /// "no agent configured" error instead of a confusing 401.
    #[test]
    fn pinned_provider_without_key_yields_none() {
        let (provider, key, _) = super::resolve_effective_llm(
            "anthropic".into(),
            Some("anthropic-key".into()),
            None,
            Some(("openai".into(), "gpt-4o-mini".into())),
            |_| None,
        );
        assert_eq!(provider, "openai");
        assert!(
            key.is_none(),
            "must not fall back to the active provider's key"
        );
    }
}
