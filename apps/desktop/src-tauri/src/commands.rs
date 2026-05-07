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

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use session::{NodeId, SessionNode, Store};
use tauri::{AppHandle, Emitter, Runtime, State};

use crate::events::{
    DonePayload, NodeCreatedPayload, TextDeltaPayload, ToolCallPayload, DONE, NODE_CREATED,
    TEXT_DELTA, TOOL_CALL,
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

    #[error("internal: a state mutex was poisoned ({0})")]
    Poisoned(&'static str),
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

#[tauri::command]
pub async fn set_api_key(state: State<'_, AppState>, key: String) -> CmdResult<()> {
    if key.trim().is_empty() {
        return Err("api key must not be empty".into());
    }
    ai::keychain::save_api_key(&key).map_err(CommandError::from)?;
    state.set_api_key_cache(Some(key));
    rebuild_agent(&state).await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// has_api_key / clear_api_key / test_api_key
// ---------------------------------------------------------------------------

/// Whether the OS keychain holds an Anthropic API key.
///
/// Used by the frontend on mount to decide whether to render the M13
/// blocking-modal first-launch flow. We deliberately re-read from the
/// keychain rather than trusting the in-memory cache so the answer
/// reflects the actual on-disk state — `clear_api_key` mutates the
/// keychain and we want subsequent `has_api_key` calls to immediately
/// see "no key" without needing the cache to also be cleared in lockstep.
#[tauri::command]
pub async fn has_api_key() -> CmdResult<bool> {
    Ok(ai::keychain::load_api_key().is_some())
}

/// Remove the stored API key and tear down the agent.
///
/// After this call the app must behave as if it had just launched with
/// no key configured — the M13 acceptance criterion #3 says clearing the
/// key returns the app to the first-launch state without restart. We
/// drop the cached key and rebuild the agent so any subsequent
/// `send_message` will fail with `NoAgent` and the frontend's
/// `has_api_key()` check on next mount returns `false`.
#[tauri::command]
pub async fn clear_api_key(state: State<'_, AppState>) -> CmdResult<()> {
    ai::keychain::delete_api_key().map_err(CommandError::from)?;
    state.set_api_key_cache(None);
    rebuild_agent(&state).await?;
    Ok(())
}

/// Probe an API key with a 1-token Messages call. Used by the Settings
/// "Test" button. The key is *not* persisted by this command — that's
/// `set_api_key`'s job. Validation runs in Rust so the proposed key
/// never has to leave the trusted process.
///
/// On 200 returns `Ok(())`; on any other response or transport failure
/// returns `Err("<status> <body>")` (e.g. `"401 invalid x-api-key"`),
/// matching M13 acceptance criterion #2.
#[tauri::command]
pub async fn test_api_key(key: String) -> CmdResult<()> {
    ai::validate::test_api_key(&key).await
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

    // Tempdir path so we don't litter the project. Phase 2 will write
    // these into a per-project preview cache; today's transient render
    // is purely for the playback button in the UI.
    //
    // We previously joined the OS tempdir with a deterministic
    // `edytlab-preview-<node_id>.wav` filename, but that lets two edytlab
    // instances rendering the same node race to the same path (and on
    // multi-user systems is symlink-attackable). `tempfile_in` creates
    // the file with O_CREAT|O_EXCL semantics and a randomised suffix;
    // `keep()` then persists it past the handle's drop.
    let tmp = tempfile::Builder::new()
        .prefix("edytlab-preview-")
        .suffix(".wav")
        .tempfile_in(std::env::temp_dir())
        .map_err(CommandError::from)?;
    let out_path = tmp.path().to_path_buf();

    {
        let engine = lock_std(&state.engine, "engine")?;
        engine
            .render_to_wav(&session_node.state, &out_path, None)
            .map_err(CommandError::from)?;
    }

    tmp.into_temp_path()
        .keep()
        .map_err(|e| CommandError::from(e.error))?;

    out_path
        .to_str()
        .map(str::to_owned)
        .ok_or_else(|| CommandError::InvalidPath("temp path is not valid UTF-8".into()).into())
}

// ---------------------------------------------------------------------------
// send_message
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn send_message<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    text: String,
) -> CmdResult<()> {
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

    agent
        .turn(text, on_event)
        .await
        .map_err(CommandError::from)?;

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
        ai::AgentEvent::ToolCallEnd { .. } => {
            // The Phase 1 frontend resolves tool badges off the
            // subsequent NodeCreated / Done events. ToolCallEnd is kept
            // internal; surfacing it would require a second event shape
            // and is deferred to M12 if the UI ends up needing it.
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
    }
}

// ---------------------------------------------------------------------------
// Agent (re)construction
// ---------------------------------------------------------------------------

/// Rebuild the agent from the current state. Called after the API key
/// or the project store changes; either or both prerequisites being
/// missing is fine and clears the agent rather than failing.
async fn rebuild_agent(state: &AppState) -> Result<(), CommandError> {
    let api_key = state.api_key_snapshot();
    let store_handle = state.store_handle();

    let mut guard = state.agent.lock().await;
    *guard = match (api_key, store_handle) {
        (Some(key), Some(store)) => {
            let cfg = ai::AnthropicConfig::new(key);
            Some(ai::Agent::new(
                cfg,
                Arc::clone(&state.dispatcher),
                store,
                Arc::clone(&state.engine),
            ))
        }
        _ => None,
    };
    Ok(())
}

/// Try to construct the agent at app startup using a key that may
/// already be in the OS keychain. If no key is stored, this is a no-op
/// and the frontend's first action is to call `set_api_key`.
pub fn try_load_api_key_at_startup(state: &AppState) {
    if let Some(key) = ai::keychain::load_api_key() {
        state.set_api_key_cache(Some(key));
    }
}

// ---------------------------------------------------------------------------
// Helpers re-exported for tests.
// ---------------------------------------------------------------------------

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

    #[test]
    fn command_error_serializes_to_string_at_boundary() {
        let err: String = CommandError::NoSession.into();
        assert!(err.contains("no session loaded"), "got: {err}");

        let err: String = CommandError::NoAgent.into();
        assert!(err.contains("no agent configured"), "got: {err}");
    }
}
