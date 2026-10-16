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
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use session::{NodeId, SessionNode, Store};
use tauri::{AppHandle, Emitter, Manager, Runtime, State};
use tools::Range;

use crate::events::{
    DonePayload, NodeCreatedPayload, PlanPayload, PlanUnavailablePayload, TextDeltaPayload,
    ToolCallEndPayload, ToolCallPayload, DONE, NODE_CREATED, PLAN, PLAN_REJECTED, PLAN_UNAVAILABLE,
    TEXT_DELTA, TOOL_CALL, TOOL_CALL_END,
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

    /// Plan first was turned on or off for this launch, but the choice could
    /// not be written to the settings file (#569), so the next launch will
    /// start from whatever was saved before. The text is for the composer
    /// to show as it is.
    #[error("could not save the Plan first setting ({0}); it applies until edytlab quits")]
    PlanFirstNotSaved(std::io::Error),

    #[error("invalid node id: {0}")]
    InvalidNodeId(String),

    #[error("no session loaded; call open_project first")]
    NoSession,

    #[error("no agent configured; call set_api_key first")]
    NoAgent,

    /// No agent, because the keychain refused access at launch and was not
    /// asked again. Starts like [`CommandError::NoAgent`] so the frontend,
    /// which reads the text, still offers Settings for it.
    #[error(
        "no agent configured: {0}. Press Save in Settings to try the keychain again, \
         or unlock your keyring and restart edytlab."
    )]
    NoAgentKeychainRefused(String),

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

    /// A node names derived audio that is gone and could not be rebuilt.
    /// The message already says which file and why.
    #[error("{0}")]
    MissingAudio(String),

    /// The task a command's blocking work ran on did not finish: the work
    /// panicked, or the runtime was shutting down. See [`blocking`].
    ///
    /// The panic case exists in dev and test builds only. The release
    /// profile sets `panic = "abort"`, so a panic in a shipped build ends
    /// the app before the task can report anything; what a shipped build
    /// can see here is the runtime shutting down.
    #[error("internal: a command's task did not finish ({0})")]
    TaskFailed(String),
}

impl From<CommandError> for String {
    fn from(value: CommandError) -> Self {
        value.to_string()
    }
}

type CmdResult<T> = std::result::Result<T, String>;

/// Put back any of `node`'s audio the history sweep removed, before a
/// command renders the node or moves the head to it (#98).
///
/// A no-op when everything is on disk, which is every node until
/// something sweeps. See `tools::rederive::materialize`.
fn materialize(store: &session::Store, node: NodeId) -> Result<(), CommandError> {
    tools::rederive::materialize(store, node)
        .map(|_| ())
        .map_err(CommandError::MissingAudio)
}

/// Convenience: lock a `std::sync::Mutex` and turn poisoning into a
/// `CommandError` so we never `unwrap()` outside test code.
fn lock_std<'a, T>(
    mu: &'a Mutex<T>,
    label: &'static str,
) -> Result<std::sync::MutexGuard<'a, T>, CommandError> {
    mu.lock().map_err(|_| CommandError::Poisoned(label))
}

/// Run a command's blocking work on Tokio's blocking pool, and wait for it
/// (#498).
///
/// The session commands wait on `std::sync::Mutex` locks (the store, the
/// engine, the dispatcher) that a running tool can hold for a long time. A
/// `std` lock blocks the thread rather than the task, so a command that
/// waited inside its async task held one of the runtime's few workers until
/// the lock came free: about as many waiting fader clicks as there are
/// cores, during one long tool, left no worker to run any async command,
/// including ones that take no lock. The blocking pool exists to be
/// blocked, so the wait happens there and the worker stays free.
///
/// `body` gets the state as the commands do, as `&AppState`. [`AppState`]
/// is a bundle of `Arc`s, so the clone it runs on costs a few reference
/// counts and shares every lock with the original. Keep the rule in
/// CLAUDE.md inside `body`: take the store lock, drop it, then take the
/// engine's.
///
/// In a dev or test build, a panic in `body` (a poisoned mutex that is
/// `expect`ed, say) comes back as an error; without this it would end the
/// command's task and the frontend's promise would never settle. A shipped
/// build sets `panic = "abort"` in its release profile, so there a panic
/// ends the app instead, and this covers the runtime shutting down under
/// the command.
///
/// What goes here is any command that can wait on a lock a tool holds.
/// `tests/main_thread_commands.rs` checks that none is left outside it.
async fn blocking<T, F>(state: &State<'_, AppState>, body: F) -> CmdResult<T>
where
    T: Send + 'static,
    F: FnOnce(&AppState) -> CmdResult<T> + Send + 'static,
{
    let state = AppState::clone(state.inner());
    tauri::async_runtime::spawn_blocking(move || body(&state))
        .await
        .map_err(|e| CommandError::TaskFailed(e.to_string()))?
}

// ---------------------------------------------------------------------------
// open_project
// ---------------------------------------------------------------------------

/// Remove derived audio no node names, as a project opens (#98).
///
/// Every open runs it with the project's live store's lock held (see
/// [`open_project_with`]): no edit can be half-way through writing a file
/// that its node does not name yet. The one exception is the default project
/// at startup (`lib.rs`), swept before it is shared because no turn can
/// exist yet. Only orphans:
/// audio that older history still names stays, because a replay cannot
/// yet be trusted to rebuild it. A failure costs disk, not work, so it is
/// logged.
pub(crate) fn sweep_orphaned_audio(store: &Store) {
    match tools::reclaim::sweep_orphans(store) {
        Ok(r) if r.removed_files > 0 => tracing::info!(
            removed = r.removed_files,
            freed_bytes = r.freed_bytes,
            "removed derived audio no node names"
        ),
        Ok(_) => {}
        Err(e) => tracing::warn!(error = %e, "orphaned-audio sweep failed"),
    }
}

/// Let the webview read a project's audio over the asset protocol (#334).
///
/// The asset scope used to be `**` — any file the user can read, which
/// is what an injection in the webview would have been able to read too.
/// It is now the app's own data directory (the default project lives
/// there) plus what the app grants as it goes: each project as it opens,
/// the files `list_tracks` hands the timeline, and compare renders.
/// A failed grant shows as audio that will not load, so it is logged.
///
/// `allow_directory(dir, true)` is not enough on Linux and macOS: Tauri
/// matches the asset scope with `require_literal_leading_dot` there, so
/// `dir/**` does not reach through `.audiograph`, and the previews the
/// transport plays and the auditions the chat plays live under it (#402).
/// Those two caches are therefore granted by name, which a literal `.`
/// satisfies; the rest of `.audiograph` stays out of reach.
pub(crate) fn allow_assets_in_dir<R: Runtime>(app: &AppHandle<R>, dir: &std::path::Path) {
    let scope = app.asset_protocol_scope();
    let caches = [
        tools::PreviewCache::new(dir),
        tools::PreviewCache::in_dir(dir, tools::tool::audition::AUDITION_DIR),
    ];
    for allowed in std::iter::once(dir).chain(caches.iter().map(|c| c.dir())) {
        if let Err(e) = scope.allow_directory(allowed, true) {
            tracing::warn!(dir = %allowed.display(), error = %e, "could not allow a project in the asset scope");
        }
    }
}

/// One file the webview is about to load; see [`allow_assets_in_dir`].
pub(crate) fn allow_asset_file<R: Runtime>(app: &AppHandle<R>, file: &std::path::Path) {
    if let Err(e) = app.asset_protocol_scope().allow_file(file) {
        tracing::warn!(file = %file.display(), error = %e, "could not allow a file in the asset scope");
    }
}

#[tauri::command]
pub async fn open_project<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    path: String,
) -> CmdResult<ProjectInfo> {
    let project_path = PathBuf::from(&path);
    // Disk work (the store, the orphan sweep), so on the blocking pool.
    let info = {
        let project_path = project_path.clone();
        blocking(&state, move |state| {
            Ok(open_project_inner(state, project_path)?)
        })
        .await?
    };
    allow_assets_in_dir(&app, &project_path);
    // After the store is replaced we may now be able to construct the
    // agent (if an API key was already cached). Rebuild it, but never
    // wait for a turn to end to do it (#498).
    rebuild_agent_soon(&state);
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
    artist: Option<String>,
    album: Option<String>,
    year: Option<String>,
) -> CmdResult<crate::project::ProjectMeta> {
    let dir = current_project_dir(&state)?;
    if name.trim().is_empty() {
        return Err(CommandError::InvalidPath("a project name cannot be empty".into()).into());
    }
    let mut meta = crate::project::read_meta(&dir);
    meta.name = name.trim().to_string();
    // Each `None` means "leave this one alone", which is what `notes`
    // has always meant here — a caller editing the name should not
    // have to resend the tag defaults to avoid clearing them.
    if let Some(n) = notes {
        meta.notes = n;
    }
    if let Some(v) = artist {
        meta.artist = v.trim().to_string();
    }
    if let Some(v) = album {
        meta.album = v.trim().to_string();
    }
    if let Some(v) = year {
        meta.year = v.trim().to_string();
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

/// The path the user took through the history, as it was last saved with
/// the project (#453).
///
/// The frontend reads it as a project opens and hands undo and redo the
/// steps in it, so that after a restart undo retraces what the user did
/// instead of falling back on each node's first parent (#398). Never an
/// error because of the file: none, an unreadable one and one naming nodes
/// the project no longer has all answer with a shorter, or empty, trail
/// (see `session::trail`). On the blocking pool because it reads files,
/// and a few hundred of them: an `async` command that did so itself would
/// hold one of the runtime's few workers for the reads.
#[tauri::command]
pub async fn get_head_trail(state: State<'_, AppState>) -> CmdResult<session::trail::HeadTrail> {
    blocking(&state, |state| {
        let dir = current_project_dir(state)?;
        Ok(session::trail::read_trail(&dir))
    })
    .await
}

/// Keep the path the user has taken with the project, replacing the last.
///
/// Called after every head move, without waiting for the answer, and
/// never from inside the session's line of edits (`lib/sessionQueue.ts`):
/// it writes a file and fsyncs it, and an edit must not wait on that. A
/// trail that does not belong to the open project (its head is not one of
/// its nodes: a save sent for the project that was just left) is dropped
/// and answered as success; see `session::trail::write_trail`.
#[tauri::command]
pub async fn save_head_trail(
    state: State<'_, AppState>,
    trail: session::trail::HeadTrail,
) -> CmdResult<()> {
    blocking(&state, move |state| {
        let dir = current_project_dir(state)?;
        session::trail::write_trail(&dir, &trail).map_err(CommandError::from)?;
        Ok(())
    })
    .await
}

/// Copy the open project to `dest` and continue in the copy.
///
/// Save As, in the sense a DAW means it: the work carries on in the new
/// place, and the original is left as it was at this moment. That is
/// only honest because a project folder now contains its own audio
/// (#156) — before the storage layout moved, a copy would have been a
/// history pointing at files somewhere else.
#[tauri::command]
pub async fn save_project_as<R: Runtime>(
    app: AppHandle<R>,
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

    // The copy can be gigabytes: on the blocking pool, not a worker.
    let report = {
        let dest_path = dest_path.clone();
        blocking(&state, move |state| {
            let report =
                crate::project::copy_project(&src, &dest_path).map_err(CommandError::Io)?;

            // Continue in the copy. Leaving the original open would make
            // Save As a backup button, which is a different feature with a
            // different name.
            open_project_inner(state, dest_path)?;
            Ok(report)
        })
        .await?
    };
    allow_assets_in_dir(&app, &dest_path);
    // The agent holds the store it was built with, so left alone it would
    // go on editing the project this one just left (#506). Rebuild it as
    // `open_project` does, and for the same reason never wait for a turn to
    // end to do it: Save As is in the session's line (#498).
    rebuild_agent_soon(&state);
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
    open_project_with(state, path, sweep_orphaned_audio)
}

/// The live store for the project at `path`, and whether it is new.
///
/// A store that is still alive for the folder, wherever it is held (the
/// window, the agent mid-turn, a command), is the one to use
/// (`crate::live_stores`). Only when there is none is one built, and then
/// it is registered before it is returned. If two opens of a folder nothing
/// has open race, the registry keeps one of the two stores and both opens
/// get it back: the second finds it is not new and reads the folder into it.
fn live_or_new_store(
    state: &AppState,
    path: &Path,
) -> Result<(Arc<Mutex<Store>>, bool), CommandError> {
    live_or_new_store_with(state, path, |dir| Ok(Store::open(dir)?))
}

/// [`live_or_new_store`] with the building of a new store handed in, so a
/// test can run a competing open between the registry saying "none" and
/// this one registering its own.
fn live_or_new_store_with(
    state: &AppState,
    path: &Path,
    build: impl FnOnce(&Path) -> Result<Store, CommandError>,
) -> Result<(Arc<Mutex<Store>>, bool), CommandError> {
    if let Some(live) = state.live_stores.find(path) {
        return Ok((live, false));
    }
    let built = Arc::new(Mutex::new(build(path)?));
    let registered = state.live_stores.register(path, Arc::clone(&built));
    let is_new = Arc::ptr_eq(&registered, &built);
    Ok((registered, is_new))
}

/// [`open_project_inner`] with the sweep of unreferenced derived audio
/// handed in, so a test can stop at the moment it would run.
///
/// **A project that already has a live store** goes through that store
/// (#501). The agent holds the store it was built with for the length of a
/// turn, so a second `Store` for the same folder would leave the agent and
/// the window as two writers, each with its own idea of the head. That is
/// so when the project being opened is the one open, and as much when the
/// user opened another project and came back during the turn: `AppState`
/// holds the other then, but the turn's store is still alive and the
/// registry (`AppState::live_stores`) hands it out. The folder is read
/// again, as an open does, into the same `Arc<Mutex<Store>>`, and `AppState`
/// is pointed at it; the agent and every command keep one handle and one
/// lock.
///
/// The sweep then runs holding that lock. A tool that runs off the lock
/// takes it to stage, so with it held no tool can stage between the sweep's
/// in-flight check and its deletes, which would lose the output it had
/// written and no node names yet. A project with no live store is swept the
/// same way, under the lock of the store just registered for it, which
/// nothing can have staged from yet.
///
/// Lock order: the registry's lock is a leaf, held inside
/// `live_stores.find` / `register` and nowhere else. The store's lock is
/// then taken alone, before the sweep and kept until the store is in place,
/// and nothing in between takes the registry, the engine or the dispatcher.
/// `AppState::set_open_project` then writes the `store` and `project_dir`
/// slots together (the `store` slot's lock, then `project_dir`'s), after the
/// store's is released.
fn open_project_with(
    state: &AppState,
    path: PathBuf,
    sweep: impl FnOnce(&Store),
) -> Result<ProjectInfo, CommandError> {
    if !path.is_absolute() {
        return Err(CommandError::InvalidPath(format!(
            "expected absolute path, got `{}`",
            path.display()
        )));
    }
    let path_str = path
        .to_str()
        .ok_or_else(|| CommandError::InvalidPath("project path is not valid UTF-8".into()))?
        .to_string();
    let (handle, is_new) = live_or_new_store(state, &path)?;
    let mut held = lock_std(&handle, "store")?;
    // A store built just now has read the folder. A live one is read again,
    // under its own spelling of the folder where that still reaches it: a
    // staged edit refuses to commit into a store whose `project_dir` differs
    // from the one it was staged from. A spelling that has stopped reaching
    // the folder (a symlink removed since, or pointed elsewhere) is not read
    // through: that would make an empty folder there and hand the project an
    // empty history. The spelling given is read instead, into the same store,
    // so the turn and the window stay one writer on the real folder, and an
    // edit staged under the old spelling is refused at its commit.
    let reread = if is_new {
        None
    } else {
        let dir = if crate::live_stores::same_folder(held.project_dir(), &path) {
            held.project_dir().to_path_buf()
        } else {
            path.clone()
        };
        Some(Store::open(&dir)?)
    };
    let store: &Store = reread.as_ref().unwrap_or(&held);
    sweep(store);
    let head_hex = store.head().map(|id| id.to_hex());

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

    if let Some(reread) = reread {
        *held = reread;
    }
    let dir = held.project_dir().to_path_buf();
    drop(held);
    state.set_open_project(handle, dir);

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
///
/// Answers where the save landed, as [`set_api_key_for`] does.
#[tauri::command]
pub async fn set_api_key(
    state: State<'_, AppState>,
    key: String,
) -> CmdResult<KeychainPersistenceDto> {
    state.startup_reads_settled().await;
    let provider_id = state.active_provider_id();
    set_api_key_for_inner(&state, &provider_id, &key, &OsKeychain).await
}

/// Persist `key` for `provider`, mark `provider` as active, and rebuild
/// the agent. The frontend uses this when the user picks a provider in
/// the Settings picker and saves a key against it.
///
/// Answers where the save landed (see [`KeychainPersistenceDto`]): the key
/// and the active provider are two writes, and the answer is session-only
/// if either fell back to the kernel keyring because no Secret Service
/// could be reached, so it will not survive a reboot. The values are saved
/// either way. Settings says so after Save (#497).
#[tauri::command]
pub async fn set_api_key_for(
    state: State<'_, AppState>,
    provider: String,
    key: String,
) -> CmdResult<KeychainPersistenceDto> {
    set_api_key_for_inner(&state, &provider, &key, &OsKeychain).await
}

// ---------------------------------------------------------------------------
// Base URL override
// ---------------------------------------------------------------------------

/// Read a provider's base-URL override, or `None` when it uses the
/// built-in one.
///
/// Rejects when the keychain could not be read, which is not the same as
/// `None`: Settings saves what it showed, and a blank it took for "no
/// override" is sent back as a delete (#497).
///
/// Also rejects, without reading, after a startup the keychain refused
/// (see [`stored_setting_unless_refused`]). Settings asks for this when it
/// opens, and on a locked keyring a read is another unlock prompt.
#[tauri::command]
pub async fn get_base_url_for(
    state: State<'_, AppState>,
    provider: String,
) -> CmdResult<Option<String>> {
    state.startup_reads_settled().await;
    stored_setting_unless_refused(&state, || ai::keychain::try_load_base_url(&provider))
}

/// A stored setting as the frontend sees it: the value, or the reason the
/// keychain could not be read, never `None` for a failure.
///
/// The reason alone, as [`has_key_answer`] gives it: Settings says the
/// rest.
fn stored_setting<T>(read: Result<Option<T>, ai::keychain::KeychainError>) -> CmdResult<Option<T>> {
    read.map_err(|e| e.reason().to_string())
}

/// [`stored_setting`], except that `read` is not made at all once startup
/// found the keychain refusing access ([`AppState::keychain_refused`]):
/// the answer is then the refusal's own note ([`refusal_note`]).
///
/// What Settings reads when it opens would otherwise raise another unlock
/// prompt for each field, on a keyring the user has just declined to
/// unlock. The prompts end with the refusal, which the first save that
/// goes through lifts ([`keychain_readable_again`]).
fn stored_setting_unless_refused<T>(
    state: &AppState,
    read: impl FnOnce() -> Result<Option<T>, ai::keychain::KeychainError>,
) -> CmdResult<Option<T>> {
    match refusal_note(state) {
        Some(why) => Err(why),
        None => stored_setting(read()),
    }
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
///
/// Answers where the save landed, as [`set_api_key_for`] does. Clearing the
/// override writes nothing, so it is always durable: there is no value to
/// lose.
#[tauri::command]
pub async fn set_base_url_for(
    state: State<'_, AppState>,
    provider: String,
    base_url: String,
) -> CmdResult<KeychainPersistenceDto> {
    set_base_url_for_with(&state, &provider, &base_url, &OsKeychain).await
}

async fn set_base_url_for_with(
    state: &AppState,
    provider: &str,
    base_url: &str,
    keychain: &impl KeychainWrites,
) -> CmdResult<KeychainPersistenceDto> {
    let trimmed = base_url.trim();
    let landed = if trimmed.is_empty() {
        keychain
            .delete_base_url(provider)
            .map_err(CommandError::from)?;
        KeychainPersistenceDto::landed([])
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
        let at = keychain
            .save_base_url(provider, trimmed.trim_end_matches('/'))
            .map_err(CommandError::from)?;
        KeychainPersistenceDto::landed([at])
    };
    // The agent holds its endpoint for the life of the config, so a
    // saved URL that never reaches it looks like the setting was ignored.
    rebuild_agent(state).await?;
    Ok(landed)
}

// ---------------------------------------------------------------------------
// Reasoning effort
// ---------------------------------------------------------------------------

/// Read a provider's reasoning effort as its wire string (`"low"`,
/// `"medium"`, `"high"`, `"xhigh"` or `"max"`), or `None` for the
/// model's own default.
///
/// Rejects when the keychain could not be read, for the reason
/// [`get_base_url_for`] gives (#497), and without reading it after a
/// startup the keychain refused, for the reason it gives.
#[tauri::command]
pub async fn get_effort_for(
    state: State<'_, AppState>,
    provider: String,
) -> CmdResult<Option<String>> {
    state.startup_reads_settled().await;
    stored_setting_unless_refused(&state, || ai::keychain::try_load_effort(&provider))
        .map(|level| level.map(|e| e.as_str().to_string()))
}

/// Validate a requested effort for `provider`. `None` (or blank) means
/// "the model's default" and is always valid; a level is valid only for
/// a provider whose endpoint takes one and only if it is exactly one of
/// the five the API accepts. The API answers anything else with a 400 on
/// every request, so a typo saved now would break chat later with no
/// hint that a setting caused it.
fn parse_effort_setting(
    provider: &str,
    effort: Option<&str>,
) -> Result<Option<ai::Effort>, String> {
    let Some(raw) = effort.map(str::trim).filter(|e| !e.is_empty()) else {
        return Ok(None);
    };
    if !ai::validate::provider_for(provider).supports_effort() {
        return Err(format!(
            "{provider} has no reasoning effort setting; it applies to Anthropic only"
        ));
    }
    ai::Effort::parse(raw).map(Some).ok_or_else(|| {
        let levels: Vec<&str> = ai::Effort::ALL.iter().map(|e| e.as_str()).collect();
        format!(
            "unknown reasoning effort `{raw}` (expected one of: {})",
            levels.join(", ")
        )
    })
}

/// Set a provider's reasoning effort, or clear it with `None` (or a
/// blank string) so the model's default applies again, which is what the
/// UI sends for "Default".
///
/// Answers where the save landed, as [`set_base_url_for`] does.
#[tauri::command]
pub async fn set_effort_for(
    state: State<'_, AppState>,
    provider: String,
    effort: Option<String>,
) -> CmdResult<KeychainPersistenceDto> {
    set_effort_for_with(&state, &provider, effort.as_deref(), &OsKeychain).await
}

async fn set_effort_for_with(
    state: &AppState,
    provider: &str,
    effort: Option<&str>,
    keychain: &impl KeychainWrites,
) -> CmdResult<KeychainPersistenceDto> {
    let landed = match parse_effort_setting(provider, effort)? {
        Some(level) => {
            let at = keychain
                .save_effort(provider, level)
                .map_err(CommandError::from)?;
            KeychainPersistenceDto::landed([at])
        }
        None => {
            keychain
                .delete_effort(provider)
                .map_err(CommandError::from)?;
            KeychainPersistenceDto::landed([])
        }
    };
    // The agent holds its config for its lifetime, so a saved level
    // that never reaches it looks like the setting was ignored.
    rebuild_agent(state).await?;
    Ok(landed)
}

async fn set_api_key_for_inner(
    state: &AppState,
    provider_id: &str,
    key: &str,
    keychain: &impl KeychainWrites,
) -> CmdResult<KeychainPersistenceDto> {
    // Startup's reads fill the cache this writes over, and would write
    // over this in turn if they finished after it.
    state.startup_reads_settled().await;
    // Selecting a keyless provider is a save with an empty key — that is
    // how the UI says "make this one active" — so it must be allowed
    // through rather than rejected as a blank credential.
    if ai::validate::provider_for(provider_id).requires_api_key() && key.trim().is_empty() {
        return Err("api key must not be empty".into());
    }
    let key_at = keychain
        .save_api_key(provider_id, key)
        .map_err(CommandError::from)?;
    // Saving a key for a provider implicitly makes it active — that
    // matches the user's intent and avoids a second "switch provider"
    // step in the settings UI.
    let provider_at = keychain
        .save_active_provider(provider_id)
        .map_err(CommandError::from)?;
    // Both writes went through, so the keychain is readable and writable
    // now whatever it was at launch.
    keychain_readable_again(state, ai::keychain::try_load_model);
    state.set_active_provider(provider_id.to_string());
    state.set_api_key_cache(Some(key.to_string()));
    rebuild_agent(state).await?;
    Ok(KeychainPersistenceDto::landed([key_at, provider_at]))
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
///
/// Rejects when the keychain could not be read (locked, access denied,
/// no service). That is not the same as `false`: `false` sends the user
/// to enter a key, which on a locked keyring they already have (#394).
#[tauri::command]
pub async fn has_api_key(state: State<'_, AppState>) -> CmdResult<bool> {
    // Pending while startup's keychain reads are (#497): the window is up
    // before they are done, and the frontend says it is waiting for the
    // keyring until this answers.
    state.startup_reads_settled().await;
    let provider_id = state.active_provider_id();
    has_key_answer(state.keychain_read_error(), || {
        provider_is_configured(&provider_id)
    })
}

/// The answer to [`has_api_key`], given what startup found.
///
/// A keychain that could not be read at launch means the active provider
/// and key were never restored, so an answer computed from the defaults
/// would be about the wrong provider. Report the failure instead, and
/// only ask `configured` when startup read cleanly.
///
/// Both failures carry the reason alone (`KeychainError::reason`): the
/// banner that shows it already says the keychain could not be read, and
/// the full message would say so twice.
fn has_key_answer(
    startup_error: Option<String>,
    configured: impl FnOnce() -> Result<bool, ai::keychain::KeychainError>,
) -> CmdResult<bool> {
    match startup_error {
        Some(e) => Err(e),
        None => configured().map_err(|e| e.reason().to_string()),
    }
}

/// Whether `provider_id` is ready to use.
///
/// For a keyless provider that is unconditionally true: there is no
/// credential to store, so asking the keychain would answer "no" forever
/// and leave first launch stuck behind a key prompt it can never satisfy.
///
/// An unreadable keychain is an error rather than `false`.
fn provider_is_configured(provider_id: &str) -> Result<bool, ai::keychain::KeychainError> {
    if !ai::validate::provider_for(provider_id).requires_api_key() {
        return Ok(true);
    }
    ai::keychain::try_load_api_key(provider_id).map(|k| k.is_some())
}

/// Whether a key is stored for `provider`. The Settings UI uses this
/// when the user toggles the provider picker, so we can show "configured"
/// vs "needs a key" without forcing a save.
///
/// Rejects when the keychain could not be read, which is not the same as
/// `false`; Settings shows the reason where it shows other save errors.
#[tauri::command]
pub async fn has_api_key_for(provider: String) -> CmdResult<bool> {
    provider_is_configured(&provider).map_err(|e| e.to_string())
}

/// Where values saved now would end up, as the frontend needs to see it.
///
/// `persistent: false` means Linux with no Secret Service running, so
/// settings sit in the kernel keyring and are gone after the next restart
/// of the machine. `reason` is why, as the keyring library words it.
#[derive(Debug, Clone, Serialize)]
pub struct KeychainPersistenceDto {
    pub persistent: bool,
    pub reason: Option<String>,
}

impl KeychainPersistenceDto {
    /// Where a command's saves landed, taken together: session-only if any
    /// of them was (with the reason of the first), durable otherwise, and
    /// durable for a command that wrote nothing (a clear).
    fn landed(saves: impl IntoIterator<Item = ai::keychain::Persistence>) -> Self {
        saves
            .into_iter()
            .find(|at| matches!(at, ai::keychain::Persistence::SessionOnly { .. }))
            .unwrap_or(ai::keychain::Persistence::Durable)
            .into()
    }
}

impl From<ai::keychain::Persistence> for KeychainPersistenceDto {
    fn from(p: ai::keychain::Persistence) -> Self {
        match p {
            ai::keychain::Persistence::Durable => Self {
                persistent: true,
                reason: None,
            },
            ai::keychain::Persistence::SessionOnly { reason } => Self {
                persistent: false,
                reason: Some(reason),
            },
        }
    }
}

/// The keychain writes the save commands make, as a trait so a test can say
/// where each one landed without an OS keychain. [`OsKeychain`] is the real
/// one: `ai::keychain`'s own functions, whose `Persistence` is what the
/// commands report.
pub(crate) trait KeychainWrites: Sync {
    fn save_api_key(
        &self,
        provider: &str,
        key: &str,
    ) -> Result<ai::keychain::Persistence, keyring::Error>;
    fn save_active_provider(
        &self,
        provider: &str,
    ) -> Result<ai::keychain::Persistence, keyring::Error>;
    fn save_base_url(
        &self,
        provider: &str,
        base_url: &str,
    ) -> Result<ai::keychain::Persistence, keyring::Error>;
    fn delete_base_url(&self, provider: &str) -> Result<(), keyring::Error>;
    fn save_effort(
        &self,
        provider: &str,
        effort: ai::Effort,
    ) -> Result<ai::keychain::Persistence, keyring::Error>;
    fn delete_effort(&self, provider: &str) -> Result<(), keyring::Error>;
    fn save_model(
        &self,
        provider: &str,
        model: &str,
    ) -> Result<ai::keychain::Persistence, keyring::Error>;
}

/// The OS keychain.
pub(crate) struct OsKeychain;

impl KeychainWrites for OsKeychain {
    fn save_api_key(
        &self,
        provider: &str,
        key: &str,
    ) -> Result<ai::keychain::Persistence, keyring::Error> {
        ai::keychain::save_api_key(provider, key)
    }
    fn save_active_provider(
        &self,
        provider: &str,
    ) -> Result<ai::keychain::Persistence, keyring::Error> {
        ai::keychain::save_active_provider(provider)
    }
    fn save_base_url(
        &self,
        provider: &str,
        base_url: &str,
    ) -> Result<ai::keychain::Persistence, keyring::Error> {
        ai::keychain::save_base_url(provider, base_url)
    }
    fn delete_base_url(&self, provider: &str) -> Result<(), keyring::Error> {
        ai::keychain::delete_base_url(provider)
    }
    fn save_effort(
        &self,
        provider: &str,
        effort: ai::Effort,
    ) -> Result<ai::keychain::Persistence, keyring::Error> {
        ai::keychain::save_effort(provider, effort)
    }
    fn delete_effort(&self, provider: &str) -> Result<(), keyring::Error> {
        ai::keychain::delete_effort(provider)
    }
    fn save_model(
        &self,
        provider: &str,
        model: &str,
    ) -> Result<ai::keychain::Persistence, keyring::Error> {
        ai::keychain::save_model(provider, model)
    }
}

/// Whether settings and keys saved now survive a reboot.
///
/// Always persistent on macOS and Windows. On Linux it is not while no
/// Secret Service (GNOME Keyring, KWallet, KeePassXC) answers, and the
/// frontend says so once instead of letting the Welcome screen be the
/// first the user hears of it (#394).
#[tauri::command]
pub async fn get_keychain_persistence() -> CmdResult<KeychainPersistenceDto> {
    Ok(ai::keychain::persistence().into())
}

// ---------------------------------------------------------------------------
// ml_runtime_status
// ---------------------------------------------------------------------------

/// Where the ONNX Runtime library that loaded came from, as the window
/// phrases it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum MlRuntimeSource {
    /// The installer put it there: the app bundle's `Contents/Frameworks`,
    /// Tauri's resource directory on Linux, or beside the exe on Windows.
    Bundled,
    /// The file `ORT_DYLIB_PATH` names.
    Environment,
    /// Beside the executable on macOS or Linux, where a development build
    /// or the user left it. Not something an installer put there.
    Executable,
}

/// What `ml_runtime_status` answers: the ONNX Runtime library the app
/// found, or why it has none.
///
/// All four are answers, not failures of the command: "this build carries
/// no runtime" is the state every release build is in until the installers
/// ship it (#383), and the window shows it as a line of text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum MlRuntimeStatusDto {
    /// A library was found, checked and loaded.
    Found {
        /// The version the library itself reports, such as `1.23.2`.
        version: String,
        path: String,
        source: MlRuntimeSource,
    },
    /// No library exists at any place the loader looks. `searched` lists
    /// them, in order.
    Missing { searched: Vec<String> },
    /// A library was found, but this macOS is older than its minimum, so
    /// it was not opened.
    UnsupportedOs { needs: String, running: String },
    /// A library was found and could not be loaded, or the load failed
    /// for a reason that names no file.
    LoadFailed {
        path: Option<String>,
        reason: String,
    },
}

/// [`MlRuntimeStatusDto`] for what `ml_pipeline::runtime::info` returned.
fn ml_runtime_status_dto(
    answer: ml_pipeline::Result<ml_pipeline::runtime::RuntimeInfo>,
) -> MlRuntimeStatusDto {
    use ml_pipeline::runtime::Origin;
    use ml_pipeline::Error;

    match answer {
        Ok(info) => MlRuntimeStatusDto::Found {
            source: if info.bundled() {
                MlRuntimeSource::Bundled
            } else if info.origin == Origin::Environment {
                MlRuntimeSource::Environment
            } else {
                MlRuntimeSource::Executable
            },
            version: info.version,
            path: info.path.display().to_string(),
        },
        Err(Error::MissingRuntime { searched }) => MlRuntimeStatusDto::Missing {
            searched: searched.iter().map(|p| p.display().to_string()).collect(),
        },
        Err(Error::RuntimeUnsupported { needs, running }) => {
            MlRuntimeStatusDto::UnsupportedOs { needs, running }
        }
        Err(Error::RuntimeLoad { path, reason }) => MlRuntimeStatusDto::LoadFailed {
            path: Some(path.display().to_string()),
            reason,
        },
        // `runtime::info` returns no other kind; a new one is shown, not hidden.
        Err(other) => MlRuntimeStatusDto::LoadFailed {
            path: None,
            reason: other.to_string(),
        },
    }
}

/// Which ONNX Runtime library the app found, and its version (#383).
///
/// Loads it if it has not been: this is `ml_pipeline::runtime::info`, which
/// opens the library by absolute path, makes the checks `ort` makes and
/// hands it to `ort` once, so a missing, unsupported or broken library is an
/// answer here and never a panic or a hang. That is file and library work,
/// so it runs on the blocking pool, not on an async worker. It takes none
/// of the store, engine or dispatcher locks.
///
/// Nothing here says a model runs: a library that loads is not stem
/// separation or transcription, which are not shipped (#384, #385).
#[tauri::command]
pub async fn ml_runtime_status() -> CmdResult<MlRuntimeStatusDto> {
    let answer = tauri::async_runtime::spawn_blocking(ml_pipeline::runtime::info)
        .await
        .map_err(|e| CommandError::TaskFailed(e.to_string()))?;
    Ok(ml_runtime_status_dto(answer))
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
    // The active provider is what startup is reading: before that, this
    // would clear the default provider's key.
    state.startup_reads_settled().await;
    let provider_id = state.active_provider_id();
    clear_api_key_for_inner(&state, &provider_id).await
}

#[tauri::command]
pub async fn clear_api_key_for(state: State<'_, AppState>, provider: String) -> CmdResult<()> {
    clear_api_key_for_inner(&state, &provider).await
}

async fn clear_api_key_for_inner(state: &AppState, provider_id: &str) -> CmdResult<()> {
    state.startup_reads_settled().await;
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
    /// The provider has no such model; `tools_ok` is false then and
    /// `detail` is empty. Serialised as `modelUnavailable`.
    pub model_unavailable: bool,
}

impl From<ai::validate::ProbeReport> for ProbeReportDto {
    fn from(r: ai::validate::ProbeReport) -> Self {
        Self {
            model: r.model,
            tools_ok: r.tools_ok,
            detail: r.detail,
            model_unavailable: r.model_unavailable,
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
///
/// Rejects while startup's keychain read is still unaccounted for
/// ([`AppState::keychain_read_error`]): the default is then what was
/// never read, not an answer. Settings would show it as the stored choice
/// and its Save would write it over the real one (#497).
#[tauri::command]
pub async fn get_active_provider(state: State<'_, AppState>) -> CmdResult<String> {
    state.startup_reads_settled().await;
    active_provider_answer(state.keychain_read_error(), state.active_provider_id())
}

/// The answer to [`get_active_provider`], given what startup found.
///
/// The reason alone, as [`has_key_answer`] gives it: the banner that shows
/// it already says the keychain could not be read.
fn active_provider_answer(startup_error: Option<String>, active: String) -> CmdResult<String> {
    match startup_error {
        Some(e) => Err(e),
        None => Ok(active),
    }
}

/// Switch the active provider. Persists the choice and rebuilds the
/// agent against the new provider's stored key (if any). Does NOT
/// require a key to already be present; the frontend uses this when the
/// user toggles the picker before saving a key.
///
/// Answers where the choice landed, as [`set_api_key_for`] does. (When the
/// switch worked but the new provider's saved key could not be read, it
/// rejects with that instead: the choice is saved, and the message is the
/// one that matters.)
#[tauri::command]
pub async fn set_active_provider(
    state: State<'_, AppState>,
    provider: String,
) -> CmdResult<KeychainPersistenceDto> {
    set_active_provider_with(
        &state,
        &provider,
        &OsKeychain,
        ai::keychain::try_load_api_key,
    )
    .await
}

async fn set_active_provider_with(
    state: &AppState,
    provider: &str,
    keychain: &impl KeychainWrites,
    load_key: impl FnOnce(&str) -> Result<Option<String>, ai::keychain::KeychainError>,
) -> CmdResult<KeychainPersistenceDto> {
    if !ai::SUPPORTED_PROVIDER_IDS.contains(&provider) {
        return Err(format!("unsupported provider id: {provider}"));
    }
    state.startup_reads_settled().await;
    let at = keychain
        .save_active_provider(provider)
        .map_err(CommandError::from)?;
    keychain_readable_again(state, ai::keychain::try_load_model);
    state.set_active_provider(provider.to_string());
    // Re-cache the key for the now-active provider (if any) so the
    // next `rebuild_agent` picks the right credentials up. When the read
    // fails the cache is emptied rather than left holding the previous
    // provider's key, which the new provider would be sent.
    let read = key_for_switch(provider, load_key);
    state.set_api_key_cache(read.as_ref().ok().cloned().flatten());
    rebuild_agent(state).await?;
    // The switch itself worked, so say what did not instead of
    // failing it: the agent is built for the new provider with no key.
    if let Err(e) = read {
        return Err(format!(
            "switched to {provider}, but its saved key could not be read: {}",
            e.reason()
        ));
    }
    Ok(KeychainPersistenceDto::landed([at]))
}

/// The key to cache when switching to `provider`.
///
/// A provider that needs none (a local Ollama daemon) has nothing to
/// read, so the keychain is not asked: on a locked keyring that read is
/// an unlock prompt for a key that does not exist, and an error the
/// switch would then report.
fn key_for_switch(
    provider: &str,
    load: impl FnOnce(&str) -> Result<Option<String>, ai::keychain::KeychainError>,
) -> Result<Option<String>, ai::keychain::KeychainError> {
    if !ai::validate::provider_for(provider).requires_api_key() {
        return Ok(None);
    }
    load(provider)
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

/// A provider's models and which list they are: the answer of
/// [`list_models_for`]. Mirrors [`ai::Catalogue`] for the IPC boundary, like
/// [`ModelInfoDto`] mirrors [`ai::ModelInfo`].
#[derive(Debug, Clone, Serialize)]
pub struct ModelCatalogueDto {
    pub models: Vec<ModelInfoDto>,
    /// `"live"`, fetched from the provider, or `"fallback"`, edytlab's
    /// built-in list because the live one was unavailable.
    pub source: &'static str,
    /// Why the fallback is shown, in words for Settings; `None` for a live
    /// list.
    pub notice: Option<String>,
}

impl From<ai::Catalogue> for ModelCatalogueDto {
    fn from(c: ai::Catalogue) -> Self {
        let (source, notice) = match c.source {
            ai::CatalogueSource::Live => ("live", None),
            ai::CatalogueSource::Fallback { reason } => ("fallback", Some(reason)),
        };
        Self {
            models: c.models.into_iter().map(ModelInfoDto::from).collect(),
            source,
            notice,
        }
    }
}

/// What the catalogue is fetched with: the key, and the endpoint the agent
/// will use.
///
/// Both come from the keychain unless the key was typed into the form, and
/// after a startup the keychain refused neither is read: that is two more
/// unlock prompts every time Settings opens or the provider changes
/// (#497). The list is refused with [`refusal_note`] instead. Reading
/// only the key would leave the endpoint unknown, and a list from the
/// built-in endpoint is not the list of the server chat talks to, so a
/// key typed for a gateway would go to the wrong host.
fn catalogue_inputs(
    state: &AppState,
    provider: &str,
    typed: Option<String>,
    load_key: impl Fn(&str) -> Option<String>,
    load_base_url: impl Fn(&str) -> Option<String>,
) -> CmdResult<(Option<String>, Option<String>)> {
    if let Some(why) = refusal_note(state) {
        return Err(why);
    }
    Ok((
        catalogue_key(provider, typed, load_key),
        load_base_url(provider),
    ))
}

/// The key the catalogue is fetched with: the one typed into the form if
/// there is one, else the one saved for `provider`.
///
/// Settings sends only what was typed, and after a key is saved the form is
/// empty, so without the fallback the normal case, a saved key, never got a
/// live list. The saved key is read here and stays in this process; it is
/// not sent to the webview.
fn catalogue_key(
    provider: &str,
    typed: Option<String>,
    load: impl Fn(&str) -> Option<String>,
) -> Option<String> {
    typed
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty())
        .or_else(|| load(provider))
}

/// List the model catalogue for `provider`, and say which list it is.
///
/// `api_key` is the key typed into the form, if any; without one the saved
/// key for `provider` is used. It is required for OpenAI, Groq and Gemini
/// (their catalogue endpoints are auth-gated) and optional for OpenRouter,
/// whose catalogue is public. Anthropic's is fetched live with the key, and
/// without one, or when the fetch fails, the answer is edytlab's built-in
/// list with `source: "fallback"` and the reason as `notice`.
///
/// The Rust layer caches live results for 10 minutes, per provider,
/// endpoint and key; repeated calls within that window return cached
/// entries without hitting the network.
///
/// Rejects, without reading the keychain, after a startup the keychain
/// refused (see [`catalogue_inputs`]).
#[tauri::command]
pub async fn list_models_for(
    state: State<'_, AppState>,
    provider: String,
    api_key: Option<String>,
) -> CmdResult<ModelCatalogueDto> {
    state.startup_reads_settled().await;
    // Follow the same endpoint the agent will use. Listing one server's
    // catalogue while chat talks to another is worse than not listing.
    let (key, base) = catalogue_inputs(
        &state,
        &provider,
        api_key,
        ai::keychain::load_api_key,
        ai::keychain::load_base_url,
    )?;
    let catalogue = ai::list_models_for_at(&provider, key.as_deref(), base.as_deref())
        .await
        .map_err(|e| e.to_string())?;
    Ok(ModelCatalogueDto::from(catalogue))
}

// ---------------------------------------------------------------------------
// Model selection
// ---------------------------------------------------------------------------

/// Persist the chosen model id for `provider`. The next agent rebuild
/// reads this slot and constructs the [`ai::LlmConfig`] with the
/// chosen model overlaid on the provider's default.
///
/// Answers where the choice landed, as [`set_api_key_for`] does.
#[tauri::command]
pub async fn set_active_model(
    state: State<'_, AppState>,
    provider: String,
    model: String,
) -> CmdResult<KeychainPersistenceDto> {
    set_active_model_with(&state, provider, model, &OsKeychain).await
}

async fn set_active_model_with(
    state: &AppState,
    provider: String,
    model: String,
    keychain: &impl KeychainWrites,
) -> CmdResult<KeychainPersistenceDto> {
    if !ai::SUPPORTED_PROVIDER_IDS.iter().any(|p| *p == provider) {
        return Err(format!("unsupported provider id: {provider}"));
    }
    let model = model.trim().to_string();
    if model.is_empty() {
        return Err("model id must not be empty".into());
    }
    // Startup restores every provider's pick into the map this writes to.
    state.startup_reads_settled().await;
    // Persisted, not only cached (#249). `active_model_by_provider` is
    // in-memory, and nothing re-pushed it at startup — so after a
    // restart `rebuild_agent` saw no model and silently used
    // `provider.default_model()`, while Settings went on showing the
    // user's pick from localStorage. A different model at a different
    // price, with nothing on screen to say so.
    //
    // A failed write is reported rather than swallowed: the choice
    // still applies to this session, but the user should know it will
    // not survive a restart.
    let persisted = keychain.save_model(&provider, &model);
    state.set_model_for(provider, model);
    rebuild_agent(state).await?;
    persisted
        .map(|at| KeychainPersistenceDto::landed([at]))
        .map_err(|e| format!("model selected for this session, but could not be saved: {e}"))
}

/// Read the model id currently selected for `provider`. Empty string
/// when nothing has been chosen yet.
///
/// Rejects when startup could not read the keychain and nothing is known
/// for `provider` (see [`active_model_answer`]): "nothing chosen" is then
/// what the map lacks, not what the keychain says, and Settings would save
/// what its field shows over the stored model (#497).
#[tauri::command]
pub async fn get_active_model(state: State<'_, AppState>, provider: String) -> CmdResult<String> {
    state.startup_reads_settled().await;
    active_model_answer(models_unread_reason(&state), state.model_for(&provider))
}

/// The answer to [`get_active_model`]: the model known for the provider,
/// else "none chosen" if startup read the keychain cleanly, else the
/// reason it could not.
///
/// A model that is known is an answer whatever startup found. It was
/// restored before the refusal ended the reads, or chosen since.
fn active_model_answer(unread: Option<String>, known: Option<String>) -> CmdResult<String> {
    match (known, unread) {
        (Some(model), _) => Ok(model),
        (None, Some(why)) => Err(why),
        (None, None) => Ok(String::new()),
    }
}

/// Why startup's keychain reads left the chosen models unrestored, if
/// they did. Startup stops at the first refusal, so the models after it
/// were never read; a read that failed some other way (the stash) may have
/// been one of the models'. Either way an empty map entry proves nothing.
fn models_unread_reason(state: &AppState) -> Option<String> {
    refusal_note(state).or_else(|| {
        state
            .keychain_read_error()
            .map(|reason| format!("the keychain could not be read at launch ({reason})"))
    })
}

/// The error for a turn with no agent to run it: the plain one, unless the
/// keychain refused access at launch, which is why the agent was left
/// unbuilt ([`rebuild_agent_from`]) and what undoes it.
fn no_agent_error(state: &AppState) -> CommandError {
    match refusal_note(state) {
        Some(why) => CommandError::NoAgentKeychainRefused(why),
        None => CommandError::NoAgent,
    }
}

/// Why the keychain is not asked, if startup found it refusing access
/// ([`AppState::keychain_refused`]); `None` while it is not known to.
///
/// Each further read of a locked Secret Service raises its own unlock
/// prompt, right after the user dismissed the first, so what is read on
/// the user's behalf only because a panel opened is refused up front with
/// this instead (#497). The reason alone, in words for Settings' "Save
/// leaves it as it is" note.
fn refusal_note(state: &AppState) -> Option<String> {
    if !state.keychain_refused() {
        return None;
    }
    let why = state
        .keychain_read_error()
        .map(|reason| format!(" ({reason})"))
        .unwrap_or_default();
    Some(format!(
        "the keychain refused access at launch{why}, so it was not asked again"
    ))
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

// The dispatcher lock can be held by a running tool, so the wait is on the
// blocking pool (#498).
#[tauri::command]
pub async fn list_capabilities(state: State<'_, AppState>) -> CmdResult<Capabilities> {
    blocking(&state, list_capabilities_inner).await
}

fn list_capabilities_inner(state: &AppState) -> CmdResult<Capabilities> {
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
    let skills: Vec<CapabilityDescriptor> = read_skill_summaries(state)
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
    // Before any join (#266). `upsert_skill` and `delete_skill` have
    // always validated; this one did not, and the asymmetry was the
    // defect — `Path::join` with an *absolute* argument discards the
    // base entirely, so `name = "/etc/hosts"` resolved to
    // `/etc/hosts.md`, well outside the skills directory.
    let dir = skills_dir(&state)?;
    let path = skill_path(&dir, &name)?;
    if !path.exists() {
        // The name, not `path.display()`: echoing the resolved absolute
        // path back across the IPC boundary told the caller where the
        // skills directory lives, and confirmed the existence of
        // whatever they aimed at.
        return Err(CommandError::InvalidPath(format!("skill `{name}` not found")).into());
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
    validate_skill_content(&content)?;

    let dir = skills_dir(&state)?;
    std::fs::create_dir_all(&dir).map_err(CommandError::from)?;
    let path = skill_path(&dir, &name)?;
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
    let dir = skills_dir(&state)?;
    let path = skill_path(&dir, &name)?;
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

/// Resolve `<dir>/<name>.md`, refusing any name that is not a plain
/// filename-safe identifier.
///
/// The validation and the join belong together (#266). Keeping them
/// apart meant every call site had to remember the rule, and the two
/// read commands did not: `read_skill` and `read_agent_profile` joined
/// straight from the IPC parameter while their write and delete
/// siblings validated. `Path::join` with an *absolute* argument
/// discards the base entirely, so `"/etc/hosts"` resolved to
/// `/etc/hosts.md` rather than to anything under `dir`.
fn skill_path(dir: &Path, name: &str) -> Result<PathBuf, CommandError> {
    validate_skill_name(name)?;
    Ok(dir.join(format!("{name}.md")))
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
    use super::selection_frames;

    #[test]
    fn converts_seconds_at_the_session_rate() {
        let r = selection_frames(1.0, 2.5, 44_100);
        assert_eq!(r.start_frame, 44_100);
        assert_eq!(r.end_frame, 110_250);
    }

    /// The rate comes from the session node, not a constant. Exporting
    /// a 48 kHz session with 44.1 kHz frame maths would cut the
    /// selection ~8% short.
    #[test]
    fn follows_the_sessions_own_sample_rate() {
        let r = selection_frames(1.0, 2.0, 48_000);
        assert_eq!(r.start_frame, 48_000);
        assert_eq!(r.end_frame, 96_000);
    }

    /// Sub-frame positions truncate rather than round, so the exported
    /// region never starts before what the user selected.
    #[test]
    fn truncates_toward_the_start_of_the_selection() {
        let r = selection_frames(0.999_999, 1.000_001, 44_100);
        assert_eq!(r.start_frame, 44_099);
        assert_eq!(r.end_frame, 44_100);
    }

    /// Nothing between the timeline and here validates the range. A
    /// negative second must saturate to frame 0 rather than wrap to a
    /// huge positive, which would hand `render_to_wav` a start past the
    /// end of the audio.
    #[test]
    fn a_negative_position_saturates_to_zero() {
        let r = selection_frames(-3.0, 1.0, 44_100);
        assert_eq!(r.start_frame, 0);
        assert_eq!(r.end_frame, 44_100);
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
    blocking(&state, |state| {
        let store_handle = state.store_handle().ok_or(CommandError::NoSession)?;
        let store = lock_std(&store_handle, "store")?;
        let head = store.head().ok_or(CommandError::NoSession)?;
        Ok(head.to_hex())
    })
    .await
}

// ---------------------------------------------------------------------------
// get_node
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn get_node(state: State<'_, AppState>, id: String) -> CmdResult<SessionNode> {
    blocking(&state, move |state| {
        let store_handle = state.store_handle().ok_or(CommandError::NoSession)?;
        let node_id = NodeId::from_hex(&id).map_err(|_| CommandError::InvalidNodeId(id.clone()))?;
        let store = lock_std(&store_handle, "store")?;
        let node = store.get(node_id).map_err(CommandError::from)?;
        Ok(node)
    })
    .await
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
    blocking(&state, |state| {
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
    })
    .await
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
    blocking(&state, move |state| {
        let store_handle = state.store_handle().ok_or(CommandError::NoSession)?;
        let node_id =
            NodeId::from_hex(&node).map_err(|_| CommandError::InvalidNodeId(node.clone()))?;

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
            // A cached render needs none of the node's audio; a new one does.
            {
                let store = lock_std(&store_handle, "store")?;
                materialize(&store, node_id)?;
            }
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
    })
    .await
}

// ---------------------------------------------------------------------------
// get_render_warnings
// ---------------------------------------------------------------------------

/// Where in the session a stored effect sits, as the window names it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RenderWarningScope {
    /// A track's own chain: its index in the session (what the tools'
    /// `track` argument takes) and its name.
    Track { index: usize, name: String },
    /// A bus's chain.
    Bus { name: String },
    /// The master bus.
    Master,
}

/// Why the render leaves a stored effect out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RenderWarningReason {
    /// The effect exists, but this build cannot run it in a chain yet.
    NotYetStreamable,
    /// No effect of that kind exists.
    Unknown,
}

/// One switched-on effect in a stored chain that the render leaves out
/// (#520, #532).
///
/// The same finding as `audio_engine::effect_chain::SkippedEffect`, the one
/// the agent's render tools report, in a shape the window phrases for
/// itself: it names the place, the kind and the reason, and leaves the
/// sentence to the frontend.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct RenderWarning {
    pub scope: RenderWarningScope,
    /// Position in the stored chain, bypassed entries counted: the index
    /// `remove_effect` and `set_effect_bypassed` take.
    pub effect_index: usize,
    pub kind: String,
    pub reason: RenderWarningReason,
}

/// What a render of `state` leaves out, from the stored chains alone: no
/// render and no audio, so it answers for a render served from the cache
/// as well as for one just made.
fn render_warnings_of(state: &session::SessionState) -> Vec<RenderWarning> {
    use audio_engine::effect_chain::{ChainScope, KindSupport};
    audio_engine::effect_chain::skipped_effects(state)
        .into_iter()
        .map(|skipped| RenderWarning {
            scope: match skipped.scope {
                ChainScope::Track { index, name } => RenderWarningScope::Track { index, name },
                ChainScope::Bus { name } => RenderWarningScope::Bus { name },
                ChainScope::Master => RenderWarningScope::Master,
            },
            effect_index: skipped.index,
            kind: skipped.kind,
            reason: match skipped.support {
                KindSupport::Unknown => RenderWarningReason::Unknown,
                // `Streamable` is never skipped; the engine's own sentence
                // folds everything else into "cannot run it yet" too.
                _ => RenderWarningReason::NotYetStreamable,
            },
        })
        .collect()
}

/// The effects a render of `node` leaves out, so the window can say so.
///
/// A stored effect the renderer cannot run no longer fails a render (#531):
/// it is left out and reported. `render_preview` answers a bare path, and a
/// cached preview has no report at all, so the window asks for the warnings
/// by node instead, whenever the node it shows changes. A node id hashes the
/// state, so the answer for an id never changes; after the effect is
/// removed or bypassed the head is another node and the answer is empty.
///
/// Reads the node and nothing else: no render, no audio, no engine lock.
#[tauri::command]
pub async fn get_render_warnings(
    state: State<'_, AppState>,
    node: String,
) -> CmdResult<Vec<RenderWarning>> {
    blocking(&state, move |state| {
        let store_handle = state.store_handle().ok_or(CommandError::NoSession)?;
        let node_id =
            NodeId::from_hex(&node).map_err(|_| CommandError::InvalidNodeId(node.clone()))?;
        let session_node = {
            let store = lock_std(&store_handle, "store")?;
            store.get(node_id).map_err(CommandError::from)?
        };
        Ok(render_warnings_of(&session_node.state))
    })
    .await
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
/// Returns `{ "a_path": "…", "b_path": "…", "a_warnings": […],
/// "b_warnings": […] }` — the caller feeds the paths to the audio engine on
/// each A/B button click so the switch is instant (no render latency on
/// toggle). The warnings are what each side's render leaves out
/// (`get_render_warnings`' list for that node), so a comparison can say that
/// one side is missing an effect it stores (#532).
#[tauri::command]
pub async fn prepare_compare<R: Runtime>(
    a: String,
    b: String,
    app: AppHandle<R>,
    state: State<'_, AppState>,
) -> CmdResult<serde_json::Value> {
    blocking(&state, move |state| {
        let store_handle = state.store_handle().ok_or(CommandError::NoSession)?;

        let a_id = NodeId::from_hex(&a).map_err(|_| CommandError::InvalidNodeId(a.clone()))?;
        let b_id = NodeId::from_hex(&b).map_err(|_| CommandError::InvalidNodeId(b.clone()))?;

        let (a_state, b_state) = {
            let store = lock_std(&store_handle, "store")?;
            let a_node = store.get(a_id).map_err(CommandError::from)?;
            let b_node = store.get(b_id).map_err(CommandError::from)?;
            materialize(&store, a_id)?;
            materialize(&store, b_id)?;
            (a_node.state, b_node.state)
        };
        let a_warnings = render_warnings_of(&a_state);
        let b_warnings = render_warnings_of(&b_state);

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

        // Rendered to the OS temp directory, which is outside the asset
        // scope: grant exactly these two, not the whole temp directory.
        allow_asset_file(&app, &a_path);
        allow_asset_file(&app, &b_path);

        Ok(serde_json::json!({
            "a_path": a_path_str,
            "b_path": b_path_str,
            "a_warnings": a_warnings,
            "b_warnings": b_warnings,
        }))
    })
    .await
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
    blocking(&state, move |state| {
        let store_handle = state.store_handle().ok_or(CommandError::NoSession)?;
        let b_id = NodeId::from_hex(&b).map_err(|_| CommandError::InvalidNodeId(b.clone()))?;
        {
            let mut store = lock_std(&store_handle, "store")?;
            materialize(&store, b_id)?;
            store.set_head(b_id).map_err(CommandError::from)?;
        }
        Ok(b_id.to_hex())
    })
    .await
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

/// What the model is told about the session at the start of a turn: the
/// user's selection, the head's markers, and what the head itself holds.
///
/// `state` is the head node's state, or `None` when there is no head or it
/// cannot be read, in which case the turn still gets its selection and
/// markers. Tracks (#408) and the master chain (#460) are read from it,
/// and so are the buses (#552): without them the model learned a bus only
/// from its own `create_bus` result, or from the error an unknown name got.
fn session_context_for(
    selection: Option<Range>,
    markers: Vec<session::Annotation>,
    head: Option<String>,
    state: Option<&session::SessionState>,
) -> ai::SessionContext {
    let (tracks, master_effects) = state
        .map(|state| {
            (
                ai::TrackBrief::from_state(state),
                ai::SessionContext::master_effects_from_state(state),
            )
        })
        .unwrap_or_default();
    ai::SessionContext {
        selection,
        markers,
        head,
        tracks,
        buses: state.map(ai::BusBrief::from_state).unwrap_or_default(),
        master_effects,
    }
}

#[tauri::command]
pub async fn send_message<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    text: String,
    disabled_tools: Vec<String>,
) -> CmdResult<()> {
    // Build the per-turn SessionContext from the current selection, the
    // annotations on the store head, and the head's tracks (#408), buses
    // (#552) and master chain (#460). We snapshot these before acquiring
    // the agent lock to minimise the lock hold time.
    let selection = state.selection_snapshot();
    // On the blocking pool: a message sent while a tool holds the store
    // would otherwise park a worker here for as long as the tool runs, and
    // reading a track's clip rates opens its source files.
    let session_ctx = Some(
        blocking(&state, move |state| {
            Ok(match state.store_handle() {
                None => session_context_for(selection, vec![], None, None),
                Some(store_arc) => {
                    let store = lock_std(&*store_arc, "store")?;
                    match store.head() {
                        None => session_context_for(selection, vec![], None, None),
                        Some(head) => {
                            let markers = store.annotations_for(head).unwrap_or_default();
                            // A head that cannot be read still gets its selection
                            // and markers; the turn is not the place to fail.
                            let node = store.get(head).ok();
                            session_context_for(
                                selection,
                                markers,
                                Some(head.to_hex()),
                                node.as_ref().map(|node| &node.state),
                            )
                        }
                    }
                }
            })
        })
        .await?,
    );

    // Hold the agent lock for the duration of the turn. Phase 1 has a
    // single chat thread, so serialised turns are correct (the user
    // cannot race themselves). The lock is `tokio::sync::Mutex` because
    // we hold it across `.await`.
    let mut agent_guard = state.agent.lock().await;
    let agent = agent_guard.as_mut().ok_or_else(|| no_agent_error(&state))?;

    // The closure must be `FnMut` and synchronous (per `Agent::turn`'s
    // signature). We capture the AppHandle by clone so each emit is
    // independent; emit failures (the receiver window has gone away)
    // are logged rather than aborting the turn.
    let app_handle = app.clone();
    // The gate is open exactly between the Plan emission and the answer
    // that releases it (#251). Tracked here rather than inside the loop
    // because this is the boundary the *frontend* sees, and it is the
    // frontend's stray Run that used to bank a permit.
    let gate_open = Arc::clone(&state.plan_gate_open);
    let on_event = move |event: ai::AgentEvent| {
        match &event {
            ai::AgentEvent::Plan { .. } => {
                gate_open.store(true, std::sync::atomic::Ordering::SeqCst)
            }
            // A turn that ended — normally, or by the gate's own
            // timeout — is no longer waiting for an answer.
            ai::AgentEvent::Done | ai::AgentEvent::PlanRejected => {
                gate_open.store(false, std::sync::atomic::Ordering::SeqCst)
            }
            _ => {}
        }
        emit_agent_event(&app_handle, event);
    };

    // Apply the per-turn disabled-tools blacklist. We snapshot the
    // agent's current whitelist, compute an effective whitelist that
    // excludes the disabled tools, swap it in for the duration of this
    // turn, and restore the original afterwards — even on error.
    //
    // The effective list can need the dispatcher's tool names, and the
    // dispatcher lock can be held by a running tool, so it is worked out
    // on the blocking pool. That is an `.await`, and the whitelist must
    // not be left out of the agent across it: it is read here without
    // being kept out, and swapped for good once there is nothing to wait
    // for.
    let current = agent.swap_tool_whitelist(None);
    agent.swap_tool_whitelist(current.clone());
    let effective = {
        let current = current.clone();
        blocking(&state, move |state| {
            Ok(apply_blacklist(current, &disabled_tools, state))
        })
        .await?
    };
    agent.swap_tool_whitelist(effective); // put effective in

    let turn_result = agent
        .turn_with_context(text, session_ctx.as_ref(), on_event)
        .await;

    // However the turn ended, it is no longer waiting for an answer. Only
    // `Done` and `PlanRejected` clear the flag from inside the turn, so a
    // turn that timed out at the gate, or failed while holding it, left it
    // open and a stray Run click could bank a permit for the next turn
    // (#251).
    state
        .plan_gate_open
        .store(false, std::sync::atomic::Ordering::SeqCst);

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
            let payload = ToolCallEndPayload {
                id,
                ok,
                not_run: false,
                view,
            };
            if let Err(e) = app.emit(TOOL_CALL_END, payload) {
                tracing::warn!(error = %e, "failed to emit tool-call-end");
            }
        }
        // Same event, so the badge's lifecycle ends the way it always
        // does; `not_run` is what tells the UI it was never a failure.
        ai::AgentEvent::ToolCallNotRun { id } => {
            let payload = ToolCallEndPayload {
                id,
                ok: false,
                not_run: true,
                view: None,
            };
            if let Err(e) = app.emit(TOOL_CALL_END, payload) {
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
        ai::AgentEvent::PlanUnavailable {
            reason,
            first_edit_held,
        } => {
            let payload = PlanUnavailablePayload {
                reason,
                first_edit_held,
            };
            if let Err(e) = app.emit(PLAN_UNAVAILABLE, payload) {
                tracing::warn!(error = %e, "failed to emit plan-unavailable");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// approve_plan
// ---------------------------------------------------------------------------

/// Turn "plan before acting" on or off.
///
/// With it on, no edit runs without approval: the model's plan is shown
/// first, and when it writes none the first step that would change the
/// session is shown instead (#415).
///
/// Held on `AppState` rather than the agent because the agent is rebuilt
/// on every key, model or base-URL change, and a preference that
/// silently reset itself would be worse than not having one. It is also
/// saved to `~/.edytlab/settings.json` and read back at launch (#569): it
/// is an approval gate, so one that came back off after a restart would
/// let the next edits through with no card.
///
/// The choice applies to this launch whatever the disk does. If it could
/// not be saved, the answer is an error that says so; the composer then
/// asks `get_plan_first` what is in force rather than assuming the click
/// failed.
#[tauri::command]
pub async fn set_plan_first(state: State<'_, AppState>, enabled: bool) -> CmdResult<()> {
    // Applied and saved before the rebuild, which waits for any turn in
    // progress to end: the choice must not wait on that to be kept.
    let saved = state.save_plan_first(enabled);
    if let Err(e) = &saved {
        tracing::warn!(error = %e, "could not save the Plan first setting");
    }
    rebuild_agent(state.inner()).await?;
    saved.map_err(CommandError::PlanFirstNotSaved)?;
    Ok(())
}

/// Whether Plan first is on: what was saved at the last quit when the app
/// has just started, then whatever `set_plan_first` has set since.
#[tauri::command]
pub async fn get_plan_first(state: State<'_, AppState>) -> CmdResult<bool> {
    Ok(state.plan_first.load(std::sync::atomic::Ordering::SeqCst))
}

/// Decline a plan. Sets the rejection flag, then fires the same notifier
/// `approve_plan` uses, so the waiting turn wakes and ends cleanly
/// having run no tools and appended no node.
#[tauri::command]
pub async fn reject_plan(state: State<'_, AppState>) -> CmdResult<()> {
    // Nothing is waiting, so there is nothing to answer (#251). See
    // `approve_plan` for why a no-op beats notifying anyway.
    if !plan_gate_was_open(&state) {
        return Ok(());
    }
    state
        .plan_rejected
        .store(true, std::sync::atomic::Ordering::SeqCst);
    state.plan_notify.notify_one();
    Ok(())
}

/// Claim the open plan gate, if there is one.
///
/// `swap` rather than a read: the gate takes exactly one answer, so a
/// double-click cannot notify twice and leave a permit behind.
fn plan_gate_was_open(state: &AppState) -> bool {
    state
        .plan_gate_open
        .swap(false, std::sync::atomic::Ordering::SeqCst)
}

/// One step of the card, as the user left it. The tool is carried along
/// with the (possibly edited) description because the description alone
/// reads "track: 1": without the tool the model has to guess what to run
/// with it from its earlier `tool_use`.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct EditedStep {
    #[serde(default)]
    pub tool: String,
    pub description: String,
}

/// The message that tells the model what the user changed, or `None`
/// when there is nothing to say.
///
/// Each line reads `{n}. {tool} — {description}`, the way the card shows
/// it, so the model sees the tool and the arguments as the user did. A
/// step with no tool (a plan the model wrote without naming one) is just
/// `{n}. {description}`.
fn revision_text(steps: &[EditedStep]) -> Option<String> {
    if steps.is_empty() {
        return None;
    }
    let formatted = steps
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let tool = s.tool.trim();
            if tool.is_empty() {
                format!("{}. {}", i + 1, s.description)
            } else {
                format!("{}. {} — {}", i + 1, tool, s.description)
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    Some(format!(
        "I've updated the plan. Please follow these revised steps instead:\n{formatted}"
    ))
}

/// Called by the frontend "Run" button. Fires the plan-approval notifier
/// in `AppState`, unblocking the turn loop. What is approved is whatever
/// the card showed: a plan, or the held first edit (#415).
///
/// Deliberately does NOT acquire the agent mutex — `send_message` holds
/// it across its `.await` points, so touching the agent here would
/// deadlock.  The notifier lives independently on `AppState`.
///
/// When `steps` is non-empty the edited steps, each with its tool, are
/// stored in `plan_steps_override` as `{n}. {tool} — {description}` lines
/// before the notifier fires.  The agent loop reads and clears this slot
/// immediately after it wakes. For a plan it appends the override text to
/// the conversation so the model executes the user's modified plan rather
/// than the original one. For a held edit nothing runs: the model is
/// given the revision and proposes again.
#[tauri::command]
pub async fn approve_plan(
    state: State<'_, AppState>,
    steps: Option<Vec<EditedStep>>,
) -> CmdResult<()> {
    // A stray Run — on a card that should already have gone away — is a
    // no-op, not a banked permit (#251).
    //
    // `plan_notify` is a tokio `Notify`, and `notify_one` on a
    // waiterless one stores a permit. So this used to satisfy the
    // *next* turn's gate immediately, carrying whatever override text
    // came with the stray click: the following planned turn skipped its
    // checkpoint and followed a plan the user never saw for it.
    if !plan_gate_was_open(&state) {
        return Ok(());
    }
    if let Some(override_text) = steps.as_deref().and_then(revision_text) {
        *state
            .plan_steps_override
            .lock()
            .expect("plan_steps_override mutex poisoned") = Some(override_text);
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
// The store lock can be held by a running tool for its whole length, so
// the wait is on the blocking pool, off the main thread (#421) and off the
// runtime's workers (#498).
#[tauri::command]
pub async fn add_marker(
    app: AppHandle,
    state: State<'_, AppState>,
    time: f64,
    name: String,
) -> CmdResult<String> {
    let head = blocking(&state, move |state| add_marker_inner(state, time, name)).await?;
    let _ = app.emit("marker-changed", ());
    Ok(head)
}

fn add_marker_inner(state: &AppState, time: f64, name: String) -> CmdResult<String> {
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
    Ok(new_head.to_hex())
}

/// Remove the annotation identified by `id` (a UUID string). Appends a
/// new session node if an annotation with that id was found; returns the
/// head hex unchanged otherwise.
///
/// Also emits a `marker-changed` event.
// The store lock can be held by a running tool for its whole length, so
// the wait is on the blocking pool, off the main thread (#421) and off the
// runtime's workers (#498).
#[tauri::command]
pub async fn remove_marker(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> CmdResult<String> {
    let head = blocking(&state, move |state| remove_marker_inner(state, id)).await?;
    let _ = app.emit("marker-changed", ());
    Ok(head)
}

fn remove_marker_inner(state: &AppState, id: String) -> CmdResult<String> {
    let annotation_id = session::AnnotationId(
        uuid::Uuid::parse_str(&id).map_err(|e| CommandError::InvalidNodeId(e.to_string()))?,
    );
    let store_arc = state.store_handle().ok_or(CommandError::NoSession)?;
    let mut store = lock_std(&*store_arc, "store")?;
    let head = store.head().ok_or(CommandError::NoSession)?;
    let new_head = store
        .remove_annotation(head, annotation_id)
        .map_err(CommandError::from)?;
    Ok(new_head.to_hex())
}

/// Ask a running long tool to stop at its next checkpoint (#169 §1).
///
/// `batch_apply` checks between files, so a cancel never leaves a
/// project whose history ends halfway through a chain. Setting the flag
/// when nothing is running is harmless: the next run clears it before
/// its first file.
#[tauri::command]
pub fn cancel_long_running_tool() -> CmdResult<()> {
    tools::progress::request_cancel();
    Ok(())
}

/// Rename and/or move a label. Returns the new session head.
///
/// One command rather than a `rename` and a `move`, because the label
/// lane can do both in one gesture (drag while renaming) and because
/// each would otherwise be a separate undo step for what the user did
/// once. Omitted fields are left alone; a change that changes nothing
/// appends no node.
///
/// `time` moves the label to that time: a marker sits there, a region
/// starts there and keeps its length. `start` and `end` set a region's
/// bounds outright. The two are not mixed in one call.
// The store lock can be held by a running tool for its whole length, so
// the wait is on the blocking pool, off the main thread (#421) and off the
// runtime's workers (#498).
#[tauri::command]
pub async fn update_marker(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    name: Option<String>,
    time: Option<f64>,
    start: Option<f64>,
    end: Option<f64>,
) -> CmdResult<String> {
    let head = blocking(&state, move |state| {
        update_marker_inner(state, id, name, time, start, end)
    })
    .await?;
    let _ = app.emit("marker-changed", ());
    Ok(head)
}

fn update_marker_inner(
    state: &AppState,
    id: String,
    name: Option<String>,
    time: Option<f64>,
    start: Option<f64>,
    end: Option<f64>,
) -> CmdResult<String> {
    let annotation_id = session::AnnotationId(
        uuid::Uuid::parse_str(&id).map_err(|e| CommandError::InvalidNodeId(e.to_string()))?,
    );

    // A marker and a region are different shapes, and accepting both in
    // one call means silently picking one. That hides a caller bug —
    // the UI sending `time` alongside a stale `start`/`end` would look
    // like it worked — so a mixed request is refused instead.
    if time.is_some() && (start.is_some() || end.is_some()) {
        return Err(CommandError::InvalidMarker(
            "pass either `time` (a marker) or `start` and `end` (a region), not both".into(),
        )
        .into());
    }
    if start.is_some() != end.is_some() {
        return Err(
            CommandError::InvalidMarker("a region needs both `start` and `end`".into()).into(),
        );
    }

    // Same lower-bound guard as `add_marker`: a label at a negative
    // time cannot be seen, seeked to or deleted, so it must not be
    // reachable by dragging one there either. There is deliberately no
    // upper bound — the session length is not known here, and a label
    // past the end stays addressable and is pulled back by the next
    // edit rather than being lost.
    //
    // `time` moves the label, whatever shape it is; it is resolved
    // against the label itself below, once the store is locked.
    let mut kind = match (time, start, end) {
        (Some(t), _, _) => {
            if !t.is_finite() || t < 0.0 {
                return Err(CommandError::InvalidMarker(format!(
                    "time must be finite and >= 0; got {t}"
                ))
                .into());
            }
            None
        }
        (None, Some(s), Some(e)) => {
            if !s.is_finite() || !e.is_finite() || s < 0.0 || e <= s {
                return Err(CommandError::InvalidMarker(format!(
                    "region must be finite with end > start >= 0; got {s}..{e}"
                ))
                .into());
            }
            Some(session::AnnotationKind::Region {
                start_sec: s,
                end_sec: e,
            })
        }
        _ => None,
    };

    let store_arc = state.store_handle().ok_or(CommandError::NoSession)?;
    let mut store = lock_std(&*store_arc, "store")?;
    let head = store.head().ok_or(CommandError::NoSession)?;
    if let Some(t) = time {
        // A drag names one time. A marker goes there; a region goes
        // there with its length, so its end is not dropped (it used to
        // come back a point marker, and `export_labels` then wrote a
        // label the user never made). A label that is not there is left
        // for `update_annotation` to ignore, as before.
        kind = store
            .annotations_for(head)
            .map_err(CommandError::from)?
            .iter()
            .find(|a| a.id == annotation_id)
            .map(|a| a.kind.starting_at(t));
    }
    let new_head = store
        .update_annotation(head, annotation_id, name, kind)
        .map_err(CommandError::from)?;
    Ok(new_head.to_hex())
}

/// One transcribed word, as the transcript pane needs it.
#[derive(Debug, Clone, serde::Serialize)]
pub struct TranscriptWordOut {
    pub text: String,
    pub start_sec: f64,
    pub end_sec: f64,
    pub confidence: f32,
}

/// The transcript at the current head, or an empty list.
///
/// Empty and absent are deliberately the same shape here. The pane's
/// job when there is no transcript is to say "run transcribe", and it
/// can say that from an empty list — an error would make "you have not
/// transcribed yet", which is an ordinary state, look like a fault.
// The store lock can be held by a running tool for its whole length, so
// the wait is on the blocking pool, off the main thread (#421) and off the
// runtime's workers (#498).
#[tauri::command]
pub async fn get_transcript(state: State<'_, AppState>) -> CmdResult<Vec<TranscriptWordOut>> {
    blocking(&state, get_transcript_inner).await
}

fn get_transcript_inner(state: &AppState) -> CmdResult<Vec<TranscriptWordOut>> {
    let Some(store_arc) = state.store_handle() else {
        return Ok(Vec::new());
    };
    let store = lock_std(&store_arc, "store")?;
    let Some(head) = store.head() else {
        return Ok(Vec::new());
    };
    let node = store.get(head).map_err(CommandError::from)?;
    Ok(node
        .state
        .transcript
        .map(|t| {
            t.words
                .into_iter()
                .map(|w| TranscriptWordOut {
                    text: w.text,
                    start_sec: w.start_s as f64,
                    end_sec: w.end_s as f64,
                    confidence: w.confidence,
                })
                .collect()
        })
        .unwrap_or_default())
}

/// Cut a span of transcribed words, and the audio underneath.
///
/// Thin wrapper over the `cut_words` tool so the pane goes through the
/// same code path the agent does — one node, same provenance, same
/// timing fix-ups. A second implementation here is how the two would
/// drift.
// Through `run_track_tool` it takes the store, engine and dispatcher locks,
// which a running tool can hold for its whole length, so the wait is on the
// blocking pool, off the main thread (#421) and off the runtime's workers
// (#498).
#[tauri::command]
pub async fn cut_transcript_words(
    state: State<'_, AppState>,
    track: usize,
    from_word: usize,
    to_word: usize,
) -> CmdResult<String> {
    blocking(&state, move |state| {
        run_track_tool(
            state,
            "cut_words",
            serde_json::json!({ "track": track, "from_word": from_word, "to_word": to_word }),
        )
    })
    .await
}

/// Return all annotations (markers and region labels) visible at the
/// current session head as a JSON array. Each entry is the serialised
/// [`session::Annotation`] shape.
// The store lock can be held by a running tool for its whole length, so
// the wait is on the blocking pool, off the main thread (#421) and off the
// runtime's workers (#498).
#[tauri::command]
pub async fn list_markers(state: State<'_, AppState>) -> CmdResult<Vec<serde_json::Value>> {
    blocking(&state, list_markers_inner).await
}

fn list_markers_inner(state: &AppState) -> CmdResult<Vec<serde_json::Value>> {
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

/// Move the session head pointer to `node_id`. Used by undo and redo
/// and by the graph view's context menu ("Set as head"). Returns the new
/// head as hex on success.
///
/// Off the main thread (`async`): moving to a node whose audio was swept
/// replays the edits that made it, which can take seconds, and a sync
/// command would hold the window still for all of it. On the blocking pool
/// too (#498): the replay and the wait for the store lock both block, and
/// neither should hold one of the runtime's workers.
#[tauri::command]
pub async fn set_head_to(state: State<'_, AppState>, node_id: String) -> CmdResult<String> {
    blocking(&state, move |state| set_head_to_inner(state, node_id)).await
}

fn set_head_to_inner(state: &AppState, node_id: String) -> CmdResult<String> {
    let id = session::NodeId::from_hex(&node_id).map_err(CommandError::from)?;
    let store_arc = state.store_handle().ok_or(CommandError::NoSession)?;
    let mut store = lock_std(&*store_arc, "store")?;
    // Undo and the history view land on older nodes, whose audio a sweep
    // may have removed. The head's audio is always on disk; this keeps it
    // so, and refuses the move if it cannot.
    materialize(&store, id)?;
    store.set_head(id).map_err(CommandError::from)?;
    Ok(id.to_hex())
}

/// Set or clear a human-readable label on a node. Pass an empty
/// string to clear. Used by the graph view's "Rename" overlay.
// The store lock can be held by a running tool for its whole length, so
// the wait is on the blocking pool, off the main thread (#421) and off the
// runtime's workers (#498).
#[tauri::command]
pub async fn rename_node(
    state: State<'_, AppState>,
    node_id: String,
    label: String,
) -> CmdResult<()> {
    blocking(&state, move |state| {
        rename_node_inner(state, node_id, label)
    })
    .await
}

fn rename_node_inner(state: &AppState, node_id: String, label: String) -> CmdResult<()> {
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

/// One track at the current session head.
///
/// `audio_path` is the track's audio on the session's time axis, for the
/// timeline lane to draw: the clip's source when a single clip is its
/// whole source at zero, a flattened WAV otherwise, and `None` for a
/// track with no clips (see [`tools::lane_audio_path`]).
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
// The store lock can be held by a running tool for its whole length, so
// the wait is on the blocking pool, off the main thread (#421) and off the
// runtime's workers (#498).
#[tauri::command]
pub async fn list_tracks<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
) -> CmdResult<Vec<TrackSummary>> {
    blocking(&state, move |state| list_tracks_inner(&app, state)).await
}

fn list_tracks_inner<R: Runtime>(
    app: &AppHandle<R>,
    state: &AppState,
) -> CmdResult<Vec<TrackSummary>> {
    let store_arc = state.store_handle().ok_or(CommandError::NoSession)?;
    let store = lock_std(&*store_arc, "store")?;
    let head = store.head().ok_or(CommandError::NoSession)?;
    let node = store.get(head).map_err(CommandError::from)?;
    // Read before the lock drops: flattening a multi-clip track writes
    // its result into the project's own derived directory (#156).
    let project_dir_for_flatten = store.project_dir().to_path_buf();
    drop(store);
    let sr = node.state.sample_rate.max(1) as f64;
    let tracks: Vec<TrackSummary> = node
        .state
        .tracks
        .into_iter()
        .map(|t| TrackSummary {
            clips: t
                .clips
                .iter()
                .map(|c| {
                    // A clip's frames are counted in its own source's
                    // rate, not the session's (#234). Dividing by the
                    // session rate reported a 44.1 kHz bed in a 48 kHz
                    // project as 8.8% earlier and shorter than it
                    // renders — and because `move_clip` wrote the field
                    // through the same wrong conversion, the timeline
                    // agreed with the request while the render
                    // disagreed with both.
                    //
                    // Header-only, and it falls back to the session
                    // rate when the source cannot be read: a listing is
                    // not the place to fail, and an unreadable source
                    // is already visible as a lane that will not draw.
                    let rate = tools::clip_source_rate(c).map(|r| r as f64).unwrap_or(sr);
                    ClipSummary {
                        start_sec: c.start_in_track as f64 / rate,
                        length_sec: c.length as f64 / rate,
                        source_path: c.source_path.to_string_lossy().into_owned(),
                        volume_envelope: c
                            .volume_envelope
                            .iter()
                            .map(|p| EnvelopePointSummary {
                                // Envelope times are clip-relative, so
                                // they share the clip's domain.
                                time_sec: p.time_samples as f64 / rate,
                                gain_db: p.gain_db,
                            })
                            .collect(),
                    }
                })
                .collect(),
            id: t.id.0.to_string(),
            name: t.name,
            muted: t.muted,
            gain_db: t.gain_db,
            pan: t.pan,
            soloed: t.soloed,
            // Every lane draws on the session's time axis (#348), so the
            // file handed over has to start at session zero. A clip that
            // is its whole source, at zero, is handed over untouched —
            // the common case, and free. Anything else — a track split by
            // a cut, a clip moved along the timeline or trimmed at the
            // head — is flattened into one content-addressed WAV with
            // silence where no clip plays. Handing over the source for
            // those drew audio from some other time under the ruler.
            //
            // The CAS name is keyed on the clip list rather than the
            // audio, so this listing pays for a flatten once per distinct
            // arrangement and finds the file already written every time
            // after. A failure is not worth failing the listing over: the
            // lane falls back to blank.
            audio_path: tools::lane_audio_path(&project_dir_for_flatten, &t.clips)
                .map(|p| p.to_string_lossy().into_owned()),
        })
        .collect();
    // The timeline loads each track's audio straight from these paths,
    // and a track's source can be anywhere on disk: grant each one as it
    // is handed over (#334).
    for path in tracks.iter().filter_map(|t| t.audio_path.as_deref()) {
        allow_asset_file(app, std::path::Path::new(path));
    }
    Ok(tracks)
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
        let mut clipboard: Option<tools::Clipboard> = None;
        let mut ctx = tools::ToolContext {
            store: &mut store,
            engine: &mut engine,
            user_message: "",
            clipboard: &mut clipboard,
            allowed_tools: None,
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
// Through `run_track_tool` it takes the store, engine and dispatcher
// locks, which a running tool can hold for its whole length, so the wait is
// on the blocking pool, off the main thread (#421) and off the runtime's
// workers (#498).
#[tauri::command]
pub async fn set_track_gain(
    state: State<'_, AppState>,
    track: usize,
    gain_db: f32,
) -> CmdResult<String> {
    blocking(&state, move |state| {
        set_track_gain_inner(state, track, gain_db)
    })
    .await
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
// Through `run_track_tool` it takes the store, engine and dispatcher
// locks, which a running tool can hold for its whole length, so the wait is
// on the blocking pool, off the main thread (#421) and off the runtime's
// workers (#498).
#[tauri::command]
pub async fn set_track_pan(
    state: State<'_, AppState>,
    track: usize,
    pan: f32,
) -> CmdResult<String> {
    blocking(&state, move |state| set_track_pan_inner(state, track, pan)).await
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
// Through `run_track_tool` it takes the store, engine and dispatcher
// locks, which a running tool can hold for its whole length, so the wait is
// on the blocking pool, off the main thread (#421) and off the runtime's
// workers (#498).
#[tauri::command]
pub async fn rename_track(
    state: State<'_, AppState>,
    track: usize,
    name: String,
) -> CmdResult<String> {
    blocking(&state, move |state| rename_track_inner(state, track, name)).await
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
// Through `run_track_tool` it takes the store, engine and dispatcher
// locks, which a running tool can hold for its whole length, so the wait is
// on the blocking pool, off the main thread (#421) and off the runtime's
// workers (#498).
#[tauri::command]
pub async fn remove_track(state: State<'_, AppState>, track: usize) -> CmdResult<String> {
    blocking(&state, move |state| remove_track_inner(state, track)).await
}

pub(crate) fn remove_track_inner(state: &AppState, track: usize) -> CmdResult<String> {
    run_track_tool(state, "remove_track", serde_json::json!({ "track": track }))
}

/// Duplicate a track, clips and mixer settings included. Returns the
/// new session head.
// Through `run_track_tool` it takes the store, engine and dispatcher
// locks, which a running tool can hold for its whole length, so the wait is
// on the blocking pool, off the main thread (#421) and off the runtime's
// workers (#498).
#[tauri::command]
pub async fn duplicate_track(state: State<'_, AppState>, track: usize) -> CmdResult<String> {
    blocking(&state, move |state| duplicate_track_inner(state, track)).await
}

pub(crate) fn duplicate_track_inner(state: &AppState, track: usize) -> CmdResult<String> {
    run_track_tool(
        state,
        "duplicate_track",
        serde_json::json!({ "track": track }),
    )
}

/// Mute or unmute a track. Returns the new session head.
// Through `run_track_tool` it takes the store, engine and dispatcher
// locks, which a running tool can hold for its whole length, so the wait is
// on the blocking pool, off the main thread (#421) and off the runtime's
// workers (#498).
#[tauri::command]
pub async fn set_track_muted(
    state: State<'_, AppState>,
    track: usize,
    muted: bool,
) -> CmdResult<String> {
    blocking(&state, move |state| {
        set_track_muted_inner(state, track, muted)
    })
    .await
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

/// Whether sync-lock is on for the open session.
///
/// Read separately from `list_tracks` because it is a property of the
/// session rather than of any track — and the toggle has to show the
/// right state the moment a project opens, not after the first edit.
// The store lock can be held by a running tool for its whole length, so
// the wait is on the blocking pool, off the main thread (#421) and off the
// runtime's workers (#498).
#[tauri::command]
pub async fn get_sync_lock(state: State<'_, AppState>) -> CmdResult<bool> {
    blocking(&state, get_sync_lock_inner).await
}

fn get_sync_lock_inner(state: &AppState) -> CmdResult<bool> {
    let store_arc = state.store_handle().ok_or(CommandError::NoSession)?;
    let store = lock_std(&store_arc, "store")?;
    let Some(head) = store.head() else {
        // No session yet is not an error here: the toggle just shows
        // its default rather than the window refusing to draw.
        return Ok(false);
    };
    let node = store.get(head).map_err(CommandError::from)?;
    Ok(node.state.sync_lock)
}

/// Turn sync-lock on or off. Returns the new session head.
///
/// The mode lives in the session state rather than in the view, so it
/// travels with the project and the agent can read it before deciding
/// whether a cut is safe (#170 §3). Setting it to what it already is
/// appends no node, and the head comes back unchanged.
// Through `run_track_tool` it takes the store, engine and dispatcher
// locks, which a running tool can hold for its whole length, so the wait is
// on the blocking pool, off the main thread (#421) and off the runtime's
// workers (#498).
#[tauri::command]
pub async fn set_sync_lock(state: State<'_, AppState>, enabled: bool) -> CmdResult<String> {
    blocking(&state, move |state| set_sync_lock_inner(state, enabled)).await
}

pub(crate) fn set_sync_lock_inner(state: &AppState, enabled: bool) -> CmdResult<String> {
    run_track_tool(
        state,
        "set_sync_lock",
        serde_json::json!({ "enabled": enabled }),
    )
}

/// Solo or un-solo a track. Returns the new session head.
// Through `run_track_tool` it takes the store, engine and dispatcher
// locks, which a running tool can hold for its whole length, so the wait is
// on the blocking pool, off the main thread (#421) and off the runtime's
// workers (#498).
#[tauri::command]
pub async fn set_track_soloed(
    state: State<'_, AppState>,
    track: usize,
    soloed: bool,
) -> CmdResult<String> {
    blocking(&state, move |state| {
        set_track_soloed_inner(state, track, soloed)
    })
    .await
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
// Through `run_track_tool` it takes the store, engine and dispatcher
// locks, which a running tool can hold for its whole length, so the wait is
// on the blocking pool, off the main thread (#421) and off the runtime's
// workers (#498).
#[tauri::command]
pub async fn set_clip_envelope(
    state: State<'_, AppState>,
    track: usize,
    clip: usize,
    points: Vec<EnvelopePointSummary>,
) -> CmdResult<String> {
    blocking(&state, move |state| {
        set_clip_envelope_inner(state, track, clip, points)
    })
    .await
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
// Through `run_track_tool` it takes the store, engine and dispatcher
// locks, which a running tool can hold for its whole length, so the wait is
// on the blocking pool, off the main thread (#421) and off the runtime's
// workers (#498).
#[tauri::command]
pub async fn move_clip(
    state: State<'_, AppState>,
    track: usize,
    clip: usize,
    start_sec: f64,
) -> CmdResult<String> {
    blocking(&state, move |state| {
        move_clip_inner(state, track, clip, start_sec)
    })
    .await
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
// Through `run_track_tool` it takes the store, engine and dispatcher
// locks, which a running tool can hold for its whole length, so the wait is
// on the blocking pool, off the main thread (#421) and off the runtime's
// workers (#498).
#[tauri::command]
pub async fn remove_clip(
    state: State<'_, AppState>,
    track: usize,
    clip: usize,
) -> CmdResult<String> {
    blocking(&state, move |state| remove_clip_inner(state, track, clip)).await
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

// Appending the node waits for the store lock, which a running tool can
// hold, so it is on the blocking pool (#498).
#[tauri::command]
pub async fn apply_template(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
    name: String,
) -> CmdResult<String> {
    blocking(&state, move |state| {
        apply_template_inner(state, &app_handle, name)
    })
    .await
}

fn apply_template_inner(
    state: &AppState,
    app_handle: &tauri::AppHandle,
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
    // Same gap as `read_skill`, same fix (#266): the write and delete
    // siblings validate, this one did not.
    let dir = agent_profiles_dir(&state)?;
    let path = agent_profile_path(&dir, &name)?;
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
    let dir = agent_profiles_dir(&state)?;
    std::fs::create_dir_all(&dir).map_err(CommandError::from)?;
    let path = agent_profile_path(&dir, &name)?;
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
    let dir = agent_profiles_dir(&state)?;
    let path = agent_profile_path(&dir, &name)?;
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

/// [`skill_path`] for agent profiles — same invariant, same reason.
fn agent_profile_path(dir: &Path, name: &str) -> Result<PathBuf, CommandError> {
    validate_agent_profile_name(name)?;
    Ok(dir.join(format!("{name}.md")))
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

/// Wait for the agent's mutex, then rebuild the agent.
///
/// The mutex is held by `send_message` for a whole turn, which can be
/// minutes (a plan card waits up to five), so this waits that long too. A
/// command that is in the session's line (`sessionQueue.ts`) must not
/// call it: everything queued after that command would wait for the turn.
/// [`rebuild_agent_soon`] is the one for those.
///
/// What the agent is built from is read *after* the mutex is held, not
/// before: a caller that waited behind a turn builds from the state of the
/// moment it runs, not of the moment it was asked.
async fn rebuild_agent(state: &AppState) -> Result<(), CommandError> {
    let mut slot = state.agent.lock().await;
    rebuild_agent_into(state, &mut slot);
    Ok(())
}

/// Rebuild the agent without waiting for a turn to end (#498).
///
/// Opening a project and Save As call this, and the app sends both in the
/// session's line, so they must answer even while the agent is mid-turn.
///
/// - Nobody holds the agent (the usual case): rebuilt here and now, so the
///   agent is on the new store when the command answers.
/// - A turn holds it: the rebuild runs as its own task, which waits its
///   turn on the mutex and then reads the state as it is *then*. Several of
///   these queued behind one turn each build from the latest state, so an
///   older open cannot be applied after a newer one. The turn itself
///   carries on with the agent and the store it started with (#501).
fn rebuild_agent_soon(state: &AppState) {
    if let Ok(mut slot) = state.agent.try_lock() {
        rebuild_agent_into(state, &mut slot);
        return;
    }
    let state = state.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = rebuild_agent(&state).await {
            tracing::warn!(error = %e, "could not rebuild the agent after a turn");
        }
    });
}

/// Build the agent from the state as it is now and put it in `slot`, which
/// is the agent's mutex, held by the caller. Nothing here waits for a
/// turn; see [`rebuild_agent`] and [`rebuild_agent_soon`] for the callers.
///
/// After a startup the keychain refused, nothing is built and the keychain
/// is not read ([`rebuild_agent_from`]).
fn rebuild_agent_into(state: &AppState, slot: &mut Option<ai::Agent>) {
    rebuild_agent_from(
        state,
        slot,
        ai::keychain::load_api_key,
        ai::keychain::load_base_url,
        ai::keychain::load_effort,
    );
}

/// [`rebuild_agent_into`] with the three keychain reads it makes passed in,
/// so a test can count them without an OS keychain: another provider's key
/// (for a profile that pins one), and the base URL and the level of the
/// provider the agent is built for.
fn rebuild_agent_from(
    state: &AppState,
    slot: &mut Option<ai::Agent>,
    load_key: impl Fn(&str) -> Option<String>,
    load_base_url: impl Fn(&str) -> Option<String>,
    load_effort: impl Fn(&str) -> Option<ai::Effort>,
) {
    // After a startup the keychain refused (#497), no agent can be built
    // without asking it again: every agent reads its provider's base URL and
    // level, and a profile that pins another provider reads that provider's
    // key. On a locked Secret Service each read is an unlock prompt, raised
    // right after the user declined the first, and this runs at startup
    // behind the reads. Building without them would be a guess, an agent
    // pointed at the provider's own host when the stored URL is a gateway's
    // (the reason `list_models_for` is refused as well). So the agent is
    // left unbuilt, and `send_message` says why ([`no_agent_error`]). The
    // first key or provider save that goes through lifts the refusal
    // ([`keychain_readable_again`]) and builds it.
    if state.keychain_refused() {
        *slot = None;
        return;
    }
    let store_handle = state.store_handle();
    let global_provider_id = state.active_provider_id();
    let global_chosen_model = state.model_for(&global_provider_id);
    let (profile_body, tool_whitelist, profile_model) = state.active_profile_overrides();

    let (effective_provider_id, api_key, effective_model) = resolve_effective_llm(
        global_provider_id,
        state.api_key_snapshot(),
        global_chosen_model,
        profile_model,
        load_key,
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

    *slot = match (api_key, store_handle) {
        (Some(key), Some(store)) => {
            let mut cfg = ai::LlmConfig::new(provider, key);
            if let Some(m) = effective_model {
                if !m.trim().is_empty() {
                    cfg = cfg.with_model(m);
                }
            }
            // Point at a local server, a gateway, or a proxy. Absent, the
            // provider's own URL applies.
            if let Some(base) = load_base_url(&effective_provider_id) {
                cfg = cfg.with_base_url(base);
            }
            // Absent, the model's own default applies. A provider that
            // cannot take one drops it in `effective_effort`.
            if let Some(effort) = load_effort(&effective_provider_id) {
                cfg = cfg.with_effort(effort);
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

// Registering the tools takes the dispatcher lock, which a running tool can
// hold, so it is on the blocking pool (#498).
#[tauri::command]
pub async fn upsert_mcp_server(
    state: State<'_, AppState>,
    id: String,
    entry: McpServerEntry,
) -> CmdResult<()> {
    blocking(&state, move |state| {
        upsert_mcp_server_inner(state, id, entry)
    })
    .await
}

fn upsert_mcp_server_inner(state: &AppState, id: String, entry: McpServerEntry) -> CmdResult<()> {
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
    blocking(&state, move |state| delete_mcp_server_inner(state, id)).await
}

fn delete_mcp_server_inner(state: &AppState, id: String) -> CmdResult<()> {
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
    blocking(&state, move |state| restart_mcp_server_inner(state, id)).await
}

fn restart_mcp_server_inner(state: &AppState, id: String) -> CmdResult<()> {
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
    // Not while the keychain is refusing us (#497): starting it would be
    // another unlock prompt per reference. The Save that makes the
    // keychain readable again lifts this.
    if let Some(reason) = mcp_skip_reason(state, &server) {
        state.mcp.record_failure(&id, &reason);
        return Err(CommandError::InvalidMcpServer(reason).into());
    }
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

/// Why `server` must not be started now, if it must not.
///
/// After a startup the keychain refused ([`AppState::keychain_refused`]),
/// a server whose config refers to it (`<keychain:slot>`, see
/// `McpServerConfig::references_keychain`) would read it again, and on a
/// locked keyring that is another unlock prompt per reference, right
/// after the user dismissed the first. Servers that do not refer to the
/// keychain are unaffected.
///
/// The text is what Settings shows beside the server's `error` badge.
fn mcp_skip_reason(state: &AppState, server: &mcp::McpServerConfig) -> Option<String> {
    if !state.keychain_refused() || !server.references_keychain() {
        return None;
    }
    let why = state
        .keychain_read_error()
        .map(|reason| format!(" ({reason})"))
        .unwrap_or_default();
    Some(format!(
        "not started: the system keychain refused access at launch{why}, and this \
         server takes a secret from it. Unlock your keyring and restart edytlab, or \
         save your settings and restart this server."
    ))
}

/// Start every enabled MCP server and register its tools.
///
/// Called once at startup from `lib.rs::run`, off the setup thread.
/// Failures are logged and recorded on the registry (so the Settings
/// list shows an `error` badge with the reason) but never abort the
/// sweep — one misconfigured server must not stop the others.
///
/// Servers that need the keychain are skipped, with the reason on the
/// registry, when startup found it refusing access (see
/// [`mcp_skip_reason`]).
pub fn start_enabled_mcp_servers(state: &AppState) {
    start_enabled_mcp_servers_with(state, ai::keychain::load_api_key);
}

/// [`start_enabled_mcp_servers`], with the keychain read passed in so the
/// test can tell whether it was asked.
pub(crate) fn start_enabled_mcp_servers_with(
    state: &AppState,
    resolve: impl Fn(&str) -> Option<String>,
) {
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
        if let Some(reason) = mcp_skip_reason(state, &config) {
            tracing::warn!(server = %id, %reason, "mcp: autostart skipped");
            state.mcp.record_failure(&id, &reason);
            continue;
        }
        match state.mcp.restart(&id, &config, |slot| resolve(slot)) {
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
///
/// A keychain that cannot be read is not "nothing stored": the reason is
/// kept on `state` and `has_api_key` rejects with it, so the frontend can
/// tell the user instead of asking for a key they already entered (#394).
///
/// Blocks for as long as the keychain does, which for a locked Secret
/// Service is as long as the user takes to answer its prompt, so `run`
/// calls it off the setup thread and the commands that depend on it wait
/// for it (`AppState::startup_reads_settled`, #497).
pub fn try_load_api_key_at_startup(state: &AppState) {
    load_startup_settings(
        state,
        ai::keychain::try_load_active_provider,
        ai::keychain::try_load_api_key,
        ai::keychain::try_load_model,
    );
}

/// Startup, with the keychain reads passed in so the failure paths can
/// be tested without an OS keychain: the active provider, its key, then
/// every provider's chosen model.
///
/// A locked keychain stops it at the first refusal. Each read of a
/// locked Secret Service raises its own unlock prompt, so carrying on
/// would ask the user to dismiss the same prompt once per slot, eight
/// times, before the window is up.
pub(crate) fn load_startup_settings(
    state: &AppState,
    load_active: impl Fn() -> Result<Option<String>, ai::keychain::KeychainError>,
    load_key: impl Fn(&str) -> Result<Option<String>, ai::keychain::KeychainError>,
    load_model: impl Fn(&str) -> Result<Option<String>, ai::keychain::KeychainError>,
) {
    let mut first_error: Option<String> = None;
    let mut refused = false;
    match load_active() {
        Ok(Some(active)) => state.set_active_provider(active),
        Ok(None) => {}
        Err(e) => refused = note_startup_failure("active provider", &e, &mut first_error),
    }
    if !refused {
        match load_key(&state.active_provider_id()) {
            Ok(Some(key)) => state.set_api_key_cache(Some(key)),
            Ok(None) => {}
            Err(e) => refused = note_startup_failure("API key", &e, &mut first_error),
        }
    }
    state.set_keychain_read_error(first_error);
    if !refused {
        refused = restore_models(state, load_model);
    }
    // What the MCP autostart asks before it reads the keychain for a
    // server of its own (#497).
    state.set_keychain_refused(refused);
}

/// A key or provider save went through, so the keychain is readable and
/// writable now whatever it was at launch: drop the failure startup kept.
///
/// If there was one, startup stopped reading there and never restored the
/// chosen models, so read them now. Without that the agent would run each
/// provider's default model while Settings showed the stored pick (#249).
/// A provider with a pick already in this session is left alone: it was
/// made after the failure and may be newer than what the keychain holds.
///
/// With nothing kept this reads nothing, so an ordinary save costs no
/// extra keychain round trips.
///
/// The refusal startup recorded goes with the failure: MCP servers that
/// were skipped for it can be restarted from Settings now.
fn keychain_readable_again(
    state: &AppState,
    load_model: impl Fn(&str) -> Result<Option<String>, ai::keychain::KeychainError>,
) {
    if state.keychain_read_error().is_none() && !state.keychain_refused() {
        return;
    }
    state.set_keychain_read_error(None);
    state.set_keychain_refused(false);
    restore_models(state, |id| {
        if state.model_for(id).is_some() {
            Ok(None)
        } else {
            load_model(id)
        }
    });
}

/// Log a startup read that failed and keep the first one's reason.
/// Returns whether the keychain refused access, which ends startup's
/// reads.
///
/// Only the reason is kept (`KeychainError::reason`): the banner built
/// from it already says the keychain could not be read.
fn note_startup_failure(
    what: &str,
    e: &ai::keychain::KeychainError,
    first_error: &mut Option<String>,
) -> bool {
    tracing::warn!(error = %e, "could not read the {what} from the keychain at startup");
    first_error.get_or_insert_with(|| e.reason().to_string());
    e.is_access_denied()
}

/// Repopulate the in-memory model map from `load`.
///
/// Split out so the restart path is testable without an OS keychain:
/// the logic worth pinning is that *every* provider's choice comes back,
/// not just the active one's. Switching provider must not lose a choice
/// made earlier, and this map is what `rebuild_agent` reads after a
/// switch.
///
/// A slot that cannot be read is skipped, except that a keychain which
/// refused access ends the loop: the rest would be refused too, each
/// with an unlock prompt of its own. Returns whether it did.
pub(crate) fn restore_models(
    state: &AppState,
    load: impl Fn(&str) -> Result<Option<String>, ai::keychain::KeychainError>,
) -> bool {
    for id in ai::SUPPORTED_PROVIDER_IDS {
        match load(id) {
            Ok(Some(model)) => state.set_model_for((*id).to_string(), model),
            Ok(None) => {}
            Err(e) => {
                tracing::warn!(provider = %id, error = %e, "could not read the chosen model from the keychain at startup");
                if e.is_access_denied() {
                    return true;
                }
            }
        }
    }
    false
}

// ---------------------------------------------------------------------------
// render_range
// ---------------------------------------------------------------------------

/// Seconds → the frame range `render_to_wav` takes.
///
/// The conversion lives here rather than inline in the command so a
/// test can call the same code the export runs. Its predecessor was
/// `sec_to_frame_conversion`, which recomputed `(1.0 * 44100.0) as u64`
/// in the test body and asserted it equalled 44100 — a test of the
/// multiplication operator that would have passed with this command
/// deleted.
///
/// The `as u64` cast saturates: a negative second becomes frame 0
/// rather than wrapping to an enormous positive, which matters because
/// nothing on the path from the timeline down validates the range.
fn selection_frames(start_sec: f64, end_sec: f64, sample_rate: u32) -> audio_engine::TimeRange {
    audio_engine::TimeRange {
        start_frame: (start_sec * sample_rate as f64) as u64,
        end_frame: (end_sec * sample_rate as f64) as u64,
    }
}

/// Export a user-defined selection (start_sec … end_sec) of the session node
/// identified by `node_id` to `out_path` as a 16-bit PCM WAV.
///
/// The frontend calls this when the user clicks "Export Selection" in the
/// chat header. Frame conversion is done here (seconds × sample_rate) so the
/// frontend never has to know the session's sample rate.
///
/// The answer carries `warnings`: the stored effects the file was written
/// without, the same list `get_render_warnings` gives for the node.
#[tauri::command]
pub async fn render_range(
    state: State<'_, AppState>,
    node_id: String,
    start_sec: f64,
    end_sec: f64,
    out_path: String,
) -> CmdResult<serde_json::Value> {
    blocking(&state, move |state| {
        render_range_inner(state, node_id, start_sec, end_sec, out_path)
    })
    .await
}

fn render_range_inner(
    state: &AppState,
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
        let node = store
            .get(id)
            .map_err(|e| CommandError::Session(e).to_string())?;
        materialize(&store, id)?;
        node
    };

    let sample_rate = session_node.state.sample_rate;
    let range = selection_frames(start_sec, end_sec, sample_rate);

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
        // What the file was written without (#532): `get_render_warnings`'
        // list for this node, `[]` when nothing was left out.
        "warnings": render_warnings_of(&session_node.state),
    }))
}

// ---------------------------------------------------------------------------
// batch_load — import multiple files into a multi-track session in one call.
// ---------------------------------------------------------------------------

/// A file `batch_load` could not add to the session, and why.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct BatchLoadFailure {
    pub path: String,
    pub error: String,
}

/// What `batch_load` did: how many files became tracks, the head after
/// the last of them, and every file that did not.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct BatchLoadResult {
    pub tracks_loaded: usize,
    pub last_node_id: Option<String>,
    pub failures: Vec<BatchLoadFailure>,
}

/// Load multiple audio files into the session in sequence, creating one track
/// per file. Each file goes through the same `"load"` tool the agent uses,
/// so the session DAG gets a node per file with the same invariants as
/// single-file loads.
///
/// A file the tool refuses (not audio, missing, unreadable) is reported in
/// `failures` and does not stop the others: someone who drops five files
/// expects the four good ones to load. The tool refuses before it writes
/// anything, so a failed file leaves no node behind. This used to treat
/// every `Ok` from the dispatcher as a load, but a refusal is
/// `Ok(ToolResult::Error(..))` — so a file that does not exist was counted
/// as a track and the UI showed neither the file nor an error.
///
/// Returns the outcome so the frontend can refresh the track list and
/// waveform without a second round trip, and name what failed.
#[tauri::command]
pub async fn batch_load(
    state: tauri::State<'_, AppState>,
    paths: Vec<String>,
) -> CmdResult<BatchLoadResult> {
    // Decoding each file with the store, engine and dispatcher locks held
    // is the longest wait of any command here; it is on the blocking pool.
    blocking(&state, move |state| batch_load_inner(state, &paths)).await
}

pub(crate) fn batch_load_inner(state: &AppState, paths: &[String]) -> CmdResult<BatchLoadResult> {
    if paths.is_empty() {
        return Err("no files provided".into());
    }

    let store_handle = state.store_handle().ok_or(CommandError::NoSession)?;
    let mut loaded = 0usize;
    let mut failures = Vec::new();

    for path in paths {
        let args = serde_json::json!({ "path": path });

        // Acquire store, engine, and dispatcher in separate scopes so we
        // don't hold all three locks simultaneously (avoids double-borrow
        // panics and keeps the lock windows narrow).
        let outcome = {
            let mut store = lock_std(&store_handle, "store")?;
            let mut engine = lock_std(&state.engine, "engine")?;
            let dispatcher = lock_std(&state.dispatcher, "dispatcher")?;

            // The clipboard is not used by `load` but `ToolContext` requires
            // it. A local `None` suffices.
            let mut clipboard: Option<tools::Clipboard> = None;
            let mut ctx = tools::ToolContext {
                store: &mut store,
                engine: &mut engine,
                user_message: "",
                clipboard: &mut clipboard,
                allowed_tools: None,
            };

            dispatcher
                .invoke("load", args, &mut ctx)
                .map_err(|e| format!("load tool error: {e}"))?
        };

        match outcome {
            tools::ToolResult::Ok(_) => loaded += 1,
            tools::ToolResult::Error(error) => failures.push(BatchLoadFailure {
                path: path.clone(),
                error,
            }),
        }
    }

    // The head is read once, after the last file, so it is the session's
    // real head whatever mix of files loaded — `None` only when nothing
    // ever has.
    let last_node_id = lock_std(&store_handle, "store")?
        .head()
        .map(|id| id.to_hex());

    Ok(BatchLoadResult {
        tracks_loaded: loaded,
        last_node_id,
        failures,
    })
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

/// Record unattended: begin after a delay, stop after a duration
/// (#203 §2).
///
/// The decision of *when* lives in `recorder::next_action`, which is a
/// pure function and tested there — an unattended take that stops a
/// second early has clipped the last word of something nobody was in
/// the room for, and that off-by-one is worth testing away from a sound
/// card. This is the shell that drives it.
///
/// Runs on a background task so the command returns immediately and the
/// UI can show a countdown; progress goes through the same sink the
/// batch runner uses, and the same Cancel stops it.
#[tauri::command]
pub async fn timer_record(
    state: State<'_, RecorderState>,
    output_path: String,
    start_after_sec: Option<f64>,
    duration_sec: Option<f64>,
) -> CmdResult<serde_json::Value> {
    let schedule = recorder::Schedule {
        start_after_sec,
        duration_sec,
    };
    if !schedule.is_armed() {
        return Err(CommandError::InvalidPath(
            "a timer needs a start delay, a duration, or both — with neither, use start_recording"
                .into(),
        )
        .into());
    }
    if state.0.lock().unwrap().is_some() {
        return Err(CommandError::InvalidPath("already recording".into()).into());
    }

    tools::progress::begin();
    let armed_at = std::time::Instant::now();
    let mut recording = false;

    loop {
        // Cancel is the user's, and it applies whether the timer is
        // still counting down or already capturing. Stopping during the
        // countdown simply never starts.
        if tools::progress::cancelled() {
            let rec = state.0.lock().unwrap().take();
            if let Some(rec) = rec {
                let path = std::path::PathBuf::from(&output_path);
                let _ = rec.stop_and_save(&path);
            }
            tools::progress::report(serde_json::json!({
                "kind": "timer_record", "done": true, "cancelled": true,
            }));
            return Ok(serde_json::json!({ "cancelled": true, "recorded": recording }));
        }

        let elapsed = armed_at.elapsed().as_secs_f64();
        match recorder::next_action(&schedule, elapsed, recording) {
            recorder::Action::Wait | recorder::Action::Continue => {}
            recorder::Action::Start => {
                let rec = recorder::Recorder::start().map_err(|e| e.to_string())?;
                *state.0.lock().unwrap() = Some(rec);
                recording = true;
            }
            recorder::Action::Stop => break,
        }

        tools::progress::report(serde_json::json!({
            "kind": "timer_record",
            "recording": recording,
            "remaining_sec": schedule.remaining(elapsed, recording),
        }));

        // A quarter second is well inside what anyone can hear as late
        // and cheap enough to leave running through a long countdown.
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }

    let rec = state
        .0
        .lock()
        .unwrap()
        .take()
        .ok_or_else(|| "the recording vanished before it could be saved".to_string())?;
    let path = std::path::PathBuf::from(&output_path);
    let (saved, sr, ch) = rec.stop_and_save(&path).map_err(|e| e.to_string())?;

    tools::progress::report(serde_json::json!({
        "kind": "timer_record", "done": true, "cancelled": false,
    }));

    Ok(serde_json::json!({
        "path": saved.to_string_lossy(),
        "sample_rate": sr,
        "channels": ch,
        "cancelled": false,
    }))
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
    use super::*;
    use tempfile::tempdir;

    /// A mono 16-bit PCM WAV of `frames` silent samples, written by hand
    /// so the test needs no audio dependency.
    fn write_wav(path: &std::path::Path, frames: u32) {
        let data_len = frames * 2;
        let mut b = Vec::new();
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + data_len).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes()); // PCM
        b.extend_from_slice(&1u16.to_le_bytes()); // mono
        b.extend_from_slice(&48_000u32.to_le_bytes());
        b.extend_from_slice(&96_000u32.to_le_bytes());
        b.extend_from_slice(&2u16.to_le_bytes());
        b.extend_from_slice(&16u16.to_le_bytes());
        b.extend_from_slice(b"data");
        b.extend_from_slice(&data_len.to_le_bytes());
        b.resize(b.len() + data_len as usize, 0);
        std::fs::write(path, b).unwrap();
    }

    fn node_count(state: &AppState) -> usize {
        let handle = state.store_handle().unwrap();
        let store = handle.lock().unwrap();
        store.list_nodes().unwrap().len()
    }

    fn project() -> (tempfile::TempDir, AppState) {
        let tmp = tempdir().unwrap();
        let state = AppState::new();
        open_project_for_test(&state, tmp.path()).unwrap();
        (tmp, state)
    }

    #[test]
    fn empty_paths_is_err() {
        let (_tmp, state) = project();
        assert!(batch_load_inner(&state, &[]).is_err());
    }

    #[test]
    fn a_file_that_does_not_exist_is_a_failure_not_a_track() {
        let (tmp, state) = project();
        let missing = tmp.path().join("nope.wav").to_string_lossy().into_owned();

        let out = batch_load_inner(&state, std::slice::from_ref(&missing)).unwrap();

        assert_eq!(out.tracks_loaded, 0);
        assert_eq!(out.last_node_id, None);
        assert_eq!(out.failures.len(), 1);
        assert_eq!(out.failures[0].path, missing);
        assert!(!out.failures[0].error.is_empty());
        assert_eq!(node_count(&state), 0, "a refused load wrote a node");
    }

    #[test]
    fn a_file_that_is_not_audio_is_a_failure_not_a_track() {
        let (tmp, state) = project();
        let bad = tmp.path().join("notes.wav");
        std::fs::write(&bad, "this is not a wav file\n").unwrap();
        let bad = bad.to_string_lossy().into_owned();

        let out = batch_load_inner(&state, std::slice::from_ref(&bad)).unwrap();

        assert_eq!(out.tracks_loaded, 0);
        assert_eq!(out.failures.len(), 1);
        assert_eq!(out.failures[0].path, bad);
        assert_eq!(node_count(&state), 0);
    }

    #[test]
    fn one_bad_file_does_not_stop_the_good_ones() {
        let (tmp, state) = project();
        let a = tmp.path().join("a.wav");
        let b = tmp.path().join("b.wav");
        let bad = tmp.path().join("bad.wav");
        write_wav(&a, 4_800);
        write_wav(&b, 4_800);
        std::fs::write(&bad, "nope").unwrap();
        let paths: Vec<String> = [&a, &bad, &b]
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();

        let out = batch_load_inner(&state, &paths).unwrap();

        assert_eq!(out.tracks_loaded, 2);
        assert_eq!(out.failures.len(), 1);
        assert_eq!(out.failures[0].path, paths[1]);
        assert!(out.last_node_id.is_some());
        assert_eq!(
            node_count(&state),
            2,
            "one node per loaded file, none for the refusal"
        );
    }

    #[test]
    fn a_good_file_loads_with_no_failures() {
        let (tmp, state) = project();
        let a = tmp.path().join("a.wav");
        write_wav(&a, 4_800);

        let out = batch_load_inner(&state, &[a.to_string_lossy().into_owned()]).unwrap();

        assert_eq!(out.tracks_loaded, 1);
        assert!(out.failures.is_empty());
        assert!(out.last_node_id.is_some());
    }
}

// The label lane's commands (#203 §1).
//
// The lane adds, renames, moves and deletes labels through `add_marker`,
// `update_marker` and `remove_marker`. What it promises is that each
// gesture is one node the user can undo, and that what `export_labels`
// then writes is what the lane shows. Both are checked here against the
// commands' own bodies, with the real store and the real tools.
#[cfg(test)]
mod label_lane_tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    /// Run one tool against `store`, as the agent does, and return its answer.
    fn run_tool(
        store: &mut session::Store,
        tool: &str,
        args: serde_json::Value,
    ) -> serde_json::Value {
        let mut engine = audio_engine::Engine::new();
        let mut clipboard = None;
        let mut ctx = tools::ToolContext {
            store,
            engine: &mut engine,
            user_message: "",
            clipboard: &mut clipboard,
            allowed_tools: None,
        };
        match tools::ToolDispatcher::default_dispatcher()
            .invoke(tool, args, &mut ctx)
            .expect("invoke")
        {
            tools::ToolResult::Ok(v) => v,
            tools::ToolResult::Error(m) => panic!("{tool} failed: {m}"),
        }
    }

    /// A one-second mono WAV, written by hand so the test needs no audio
    /// dependency.
    fn write_wav(path: &std::path::Path) {
        let data_len = 8_000u32 * 2;
        let mut b = Vec::new();
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + data_len).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes()); // PCM
        b.extend_from_slice(&1u16.to_le_bytes()); // mono
        b.extend_from_slice(&8_000u32.to_le_bytes());
        b.extend_from_slice(&16_000u32.to_le_bytes());
        b.extend_from_slice(&2u16.to_le_bytes());
        b.extend_from_slice(&16u16.to_le_bytes());
        b.extend_from_slice(b"data");
        b.extend_from_slice(&data_len.to_le_bytes());
        b.resize(b.len() + data_len as usize, 0);
        std::fs::write(path, b).unwrap();
    }

    /// A project holding one track and the labels in `labels_text`
    /// (Audacity format), opened the way the app opens one at launch.
    struct Lane {
        dir: tempfile::TempDir,
        state: AppState,
    }

    impl Lane {
        fn open(labels_text: &str) -> Self {
            let dir = tempdir().unwrap();
            let take = dir.path().join("take.wav");
            write_wav(&take);
            {
                let mut store = session::Store::open(dir.path()).unwrap();
                run_tool(&mut store, "load", json!({ "path": take }));
                if !labels_text.is_empty() {
                    run_tool(
                        &mut store,
                        "import_labels",
                        json!({ "labels_text": labels_text }),
                    );
                }
            }
            let state = AppState::new();
            open_project_for_test(&state, dir.path()).unwrap();
            Self { dir, state }
        }

        /// What the lane draws: `list_markers`.
        fn lane(&self) -> Vec<serde_json::Value> {
            list_markers_inner(&self.state).unwrap()
        }

        fn id_of(&self, name: &str) -> String {
            self.lane()
                .iter()
                .find(|m| m["name"] == name)
                .unwrap_or_else(|| panic!("no label called {name} in {:?}", self.lane()))["id"]
                .as_str()
                .unwrap()
                .to_string()
        }

        fn add(&self, time: f64, name: &str) -> String {
            add_marker_inner(&self.state, time, name.into()).unwrap()
        }

        fn rename(&self, id: &str, name: &str) -> String {
            update_marker_inner(&self.state, id.into(), Some(name.into()), None, None, None)
                .unwrap()
        }

        /// What a drag in the lane sends: one time.
        fn drag(&self, id: &str, time: f64) -> String {
            update_marker_inner(&self.state, id.into(), None, Some(time), None, None).unwrap()
        }

        fn delete(&self, id: &str) -> String {
            remove_marker_inner(&self.state, id.into()).unwrap()
        }

        fn head(&self) -> String {
            let handle = self.state.store_handle().unwrap();
            let head = handle.lock().unwrap().head().unwrap();
            head.to_hex()
        }

        fn nodes(&self) -> usize {
            session::Store::open(self.dir.path())
                .unwrap()
                .list_nodes()
                .unwrap()
                .len()
        }

        /// The text `export_labels` writes for the project as it stands.
        fn export(&self) -> String {
            let handle = self.state.store_handle().unwrap();
            let mut store = handle.lock().unwrap();
            run_tool(&mut store, "export_labels", json!({}))["labels"]
                .as_str()
                .unwrap()
                .to_string()
        }

        /// `name@time` for each marker, the way the lane places a chip.
        fn chips(&self) -> Vec<String> {
            self.lane()
                .iter()
                .map(|m| format!("{}@{}", m["name"].as_str().unwrap(), m["time_sec"]))
                .collect()
        }
    }

    /// Each gesture in the lane is one node appended to the head, so Ctrl+Z
    /// takes back exactly one gesture and the lane then shows the labels the
    /// earlier head had.
    ///
    /// The last gesture deletes the label that was there to begin with. A
    /// delete of the label just added would land on the state the history
    /// started from, and nodes are named by their state (#398), so that one
    /// returns to an existing node rather than appending a new one.
    #[test]
    fn each_lane_gesture_is_one_node_and_undo_takes_back_one() {
        let p = Lane::open("1.5\t1.5\tintro");
        let mut heads = vec![p.head()];
        let n0 = p.nodes();

        heads.push(p.add(10.0, "chapter two"));
        let id = p.id_of("chapter two");
        heads.push(p.rename(&id, "Chapter 2"));
        heads.push(p.drag(&id, 12.0));
        heads.push(p.delete(&p.id_of("intro")));

        assert_eq!(p.nodes(), n0 + 4, "four gestures, four nodes");
        let mut distinct = heads.clone();
        distinct.dedup();
        assert_eq!(distinct, heads, "every gesture moved the head");
        assert_eq!(p.head(), heads[4]);
        assert_eq!(p.chips(), ["Chapter 2@12.0"]);

        // Undo walks them back, one gesture at a time. The app does it
        // with `set_head_to`, and the lane re-reads on the head moving.
        set_head_to_inner(&p.state, heads[3].clone()).unwrap();
        assert_eq!(
            p.chips(),
            ["intro@1.5", "Chapter 2@12.0"],
            "the delete is undone"
        );
        set_head_to_inner(&p.state, heads[2].clone()).unwrap();
        assert_eq!(
            p.chips(),
            ["intro@1.5", "Chapter 2@10.0"],
            "the move is undone"
        );
        set_head_to_inner(&p.state, heads[1].clone()).unwrap();
        assert_eq!(
            p.chips(),
            ["intro@1.5", "chapter two@10.0"],
            "the rename is undone"
        );
        set_head_to_inner(&p.state, heads[0].clone()).unwrap();
        assert_eq!(p.chips(), ["intro@1.5"], "and the add");
    }

    /// A label dragged back to where it started, or renamed to the name it
    /// has, is not an edit and costs no undo step.
    #[test]
    fn a_gesture_that_changes_nothing_appends_no_node() {
        let p = Lane::open("1.5\t1.5\tintro");
        let id = p.id_of("intro");
        let (head, n0) = (p.head(), p.nodes());

        assert_eq!(p.drag(&id, 1.5), head);
        assert_eq!(p.rename(&id, "intro"), head);

        assert_eq!(p.nodes(), n0);
    }

    /// A region label moved in the lane keeps its length. The lane shows a
    /// label at its start and a drag sends one time, so the region has to
    /// travel as a whole: it used to come back a point marker at the new
    /// time, its end dropped without a word, and `export_labels` then wrote
    /// a label the user never made.
    #[test]
    fn moving_a_region_label_keeps_its_length() {
        let p = Lane::open("3\t9\tverse");
        let id = p.id_of("verse");

        p.drag(&id, 20.0);

        assert_eq!(
            p.lane(),
            [json!({
                "id": id, "name": "verse", "kind": "region",
                "start_sec": 20.0, "end_sec": 26.0,
            })],
            "six seconds long before, six seconds long after"
        );
        assert_eq!(p.export(), "20\t26\tverse\n");
    }

    /// Dragging a region back to its own start changes nothing, as it
    /// does for a marker.
    #[test]
    fn dragging_a_region_label_to_where_it_is_appends_no_node() {
        let p = Lane::open("3\t9\tverse");
        let id = p.id_of("verse");
        let (head, n0) = (p.head(), p.nodes());

        assert_eq!(p.drag(&id, 3.0), head);
        assert_eq!(p.nodes(), n0);
    }

    #[test]
    fn renaming_a_region_label_leaves_its_extent_alone() {
        let p = Lane::open("3\t9\tverse");
        let id = p.id_of("verse");

        p.rename(&id, "first verse");

        assert_eq!(p.export(), "3\t9\tfirst verse\n");
    }

    /// The acceptance: what is typed in the lane is what `export_labels`
    /// writes, and writing it out and reading it back changes nothing.
    #[test]
    fn lane_edits_come_out_of_export_labels_unchanged() {
        let p = Lane::open("3\t9\tverse");

        p.add(0.5, "cold open");
        p.add(40.25, "chapter two");
        p.add(99.0, "to delete");
        let id = p.id_of("chapter two");
        p.rename(&id, "Chapter 2: the sequel");
        p.drag(&id, 41.5);
        p.delete(&p.id_of("to delete"));
        p.drag(&p.id_of("verse"), 4.0);

        let exported = p.export();
        assert_eq!(
            exported,
            "4\t10\tverse\n0.5\t0.5\tcold open\n41.5\t41.5\tChapter 2: the sequel\n"
        );

        // Into a project of its own and out again: the same text.
        let dir = tempdir().unwrap();
        let take = dir.path().join("take.wav");
        write_wav(&take);
        let mut store = session::Store::open(dir.path()).unwrap();
        run_tool(&mut store, "load", json!({ "path": take }));
        run_tool(
            &mut store,
            "import_labels",
            json!({ "labels_text": exported }),
        );
        assert_eq!(
            run_tool(&mut store, "export_labels", json!({}))["labels"],
            json!(exported)
        );
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

    fn step(tool: &str, description: &str) -> EditedStep {
        EditedStep {
            tool: tool.to_string(),
            description: description.to_string(),
        }
    }

    /// An edited held step used to reach the model as `1. track: 1`, with
    /// the tool left for it to guess from its earlier `tool_use`. The
    /// override names the tool, in the card's own `tool — description`
    /// form.
    #[test]
    fn revision_text_names_each_steps_tool() {
        assert_eq!(
            revision_text(&[step("reverse", "track: 1")]).as_deref(),
            Some(
                "I've updated the plan. Please follow these revised steps instead:\n\
                 1. reverse — track: 1"
            )
        );

        let two = revision_text(&[step("reverse", "track: 1"), step("normalize", "track: 2")])
            .expect("two steps");
        assert!(
            two.ends_with("\n1. reverse — track: 1\n2. normalize — track: 2"),
            "{two}"
        );

        // A plan step the model wrote without a tool keeps the old form,
        // and a tool that is only whitespace counts as none.
        let bare =
            revision_text(&[step("", "just text"), step("  ", "more text")]).expect("two steps");
        assert!(bare.ends_with("\n1. just text\n2. more text"), "{bare}");

        assert_eq!(revision_text(&[]), None);
    }

    #[test]
    fn an_edited_step_may_arrive_without_a_tool() {
        let steps: Vec<EditedStep> =
            serde_json::from_str(r#"[{"description":"track: 1"}]"#).expect("tool is optional");
        assert_eq!(steps[0].tool, "");
        assert_eq!(
            revision_text(&steps)
                .as_deref()
                .map(|t| t.ends_with("\n1. track: 1")),
            Some(true)
        );
    }

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
            sync_lock: false,
        }
    }

    /// A head with a bus and a track that sends to it, as the session holds
    /// them.
    fn state_with_a_reverb_bus() -> (SessionState, uuid::Uuid) {
        let bus_id = uuid::Uuid::new_v4();
        let mut state = empty_session_state();
        state.bus_routing.buses.push(session::Bus {
            id: bus_id,
            name: "Reverb".into(),
            effects: vec![session::EffectInstance {
                kind: "reverb".into(),
                params: serde_json::json!({}),
                bypassed: true,
            }],
        });
        state.tracks.push(session::Track {
            id: session::TrackId::new(),
            name: "vocal".into(),
            clips: vec![],
            gain_db: 0.0,
            pan: 0.0,
            muted: false,
            soloed: false,
            effects: vec![],
            sends: vec![session::Send {
                bus_id,
                level_db: -6.0,
            }],
        });
        (state, bus_id)
    }

    /// The turn's context names the head's buses (#552). It used to carry
    /// the tracks and the master chain and nothing of the buses, so the
    /// model could name a bus only after a refused tool call.
    #[test]
    fn the_turn_context_carries_the_heads_buses() {
        let (state, bus_id) = state_with_a_reverb_bus();

        let ctx = session_context_for(None, vec![], Some("ab12".into()), Some(&state));

        assert_eq!(ctx.buses.len(), 1, "{:?}", ctx.buses);
        assert_eq!(ctx.buses[0].name, "Reverb");
        assert_eq!(ctx.buses[0].id, bus_id.to_string());
        assert_eq!(ctx.buses[0].effects, ["reverb (bypassed)"]);
        assert_eq!(ctx.buses[0].sends.len(), 1);
        assert_eq!(ctx.buses[0].sends[0].track, 0);
        assert_eq!(ctx.buses[0].sends[0].track_name, "vocal");

        // And it reaches the text the model reads.
        let text = ai::render_session_block(&ctx);
        assert!(text.contains("## buses"), "{text}");
        assert!(text.contains(&bus_id.to_string()), "{text}");
    }

    /// Everything else the context held is still there, and a session with
    /// no buses has none in it.
    #[test]
    fn the_turn_context_keeps_the_tracks_the_master_and_the_selection() {
        let (mut state, _) = state_with_a_reverb_bus();
        state.bus_routing.buses.clear();
        state.tracks[0].sends.clear();
        state.master_chain.push(session::EffectInstance {
            kind: "limiter".into(),
            params: serde_json::json!({}),
            bypassed: false,
        });
        let selection = Some(Range {
            start_sec: 1.0,
            end_sec: 2.0,
        });

        let ctx = session_context_for(selection, vec![], Some("ab12".into()), Some(&state));

        assert!(ctx.buses.is_empty(), "{:?}", ctx.buses);
        assert_eq!(ctx.tracks.len(), 1);
        assert_eq!(ctx.master_effects, ["limiter"]);
        assert_eq!(ctx.selection, selection);
        assert_eq!(ctx.head.as_deref(), Some("ab12"));
        assert!(!ai::render_session_block(&ctx).contains("## buses"));
    }

    /// A head that cannot be read still gets its selection and markers.
    #[test]
    fn an_unreadable_head_still_gets_its_selection() {
        let selection = Some(Range {
            start_sec: 0.5,
            end_sec: 1.5,
        });

        let ctx = session_context_for(selection, vec![], Some("ab12".into()), None);

        assert_eq!(ctx.selection, selection);
        assert!(ctx.tracks.is_empty() && ctx.buses.is_empty());
        assert!(ctx.master_effects.is_empty());
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

    /// N1 (#421). A tool that runs off the store lock has written its
    /// output before the node that names it exists, so for that stretch the
    /// file is an orphan and the sweep an open runs would take it. Opening
    /// the project again, which the app lets the user do mid-turn, builds a
    /// fresh `Store`; the count of tools in flight has to be the project's,
    /// not that handle's.
    #[test]
    fn reopening_a_project_while_a_tool_runs_off_the_lock_does_not_sweep_its_output() {
        let tmp = tempdir().unwrap();
        let state = AppState::new();
        open_project_for_test(&state, tmp.path()).unwrap();
        let handle = state.store_handle().expect("store");
        handle
            .lock()
            .unwrap()
            .append(session::SessionNode {
                id: session::NodeId([0u8; 32]),
                parent: None,
                created_at: chrono::Utc::now(),
                label: None,
                reasoning: None,
                state: empty_session_state(),
                op: None,
            })
            .unwrap();

        // A tool is running without the lock, and has written its output.
        let running = handle.lock().unwrap().stage().unwrap();
        let derived = tools::derived_dir(tmp.path());
        std::fs::create_dir_all(&derived).unwrap();
        let output = derived.join("0123abcd-not-yet-named.wav");
        std::fs::write(&output, b"RIFF-not-yet-named").unwrap();

        open_project_for_test(&state, tmp.path()).unwrap();
        assert!(
            output.is_file(),
            "opening the project again swept the output of a tool that was running"
        );

        // The deferral is not a leak: once the tool is done the next open
        // removes what no node names.
        drop(running);
        open_project_for_test(&state, tmp.path()).unwrap();
        assert!(!output.exists(), "the orphan should go once nothing runs");
    }

    /// A node whose state differs by `length_samples`, so that appending two
    /// of them makes two nodes (ids hash the state, and the same state is the
    /// same node).
    fn an_edit(length_samples: u64) -> session::SessionNode {
        let mut state = empty_session_state();
        state.length_samples = length_samples;
        session::SessionNode {
            id: session::NodeId([0u8; 32]),
            parent: None,
            created_at: chrono::Utc::now(),
            label: None,
            reasoning: None,
            state,
            op: None,
        }
    }

    /// #501 item 1. The agent is built with the store that was open, and a
    /// turn keeps it until it ends. Opening the project that is already open
    /// during that turn used to build a second `Store` for the window, so the
    /// agent went on committing through one and the window read through the
    /// other: two writers on one folder, each with its own idea of the head.
    ///
    /// Re-opening the open project goes through the store it has. There is
    /// one writer, so what the agent commits is what the window sees.
    #[test]
    fn reopening_the_open_project_leaves_the_agent_and_the_window_one_store() {
        let tmp = tempdir().unwrap();
        let state = AppState::new();
        open_project_for_test(&state, tmp.path()).unwrap();
        // What the agent holds for the length of a turn.
        let agents = state.store_handle().expect("store");
        let first = agents.lock().unwrap().append(an_edit(1)).unwrap();

        let reopened = open_project_for_test(&state, tmp.path()).unwrap();

        let windows = state.store_handle().expect("store");
        assert!(
            Arc::ptr_eq(&agents, &windows),
            "re-opening the open project replaced its store: the agent mid-turn and the window \
             are now two writers on one folder"
        );
        // And the consequence, which is what the user would see: the agent
        // commits again, and the window's head is the agent's.
        let second = agents.lock().unwrap().append(an_edit(2)).unwrap();
        assert_eq!(
            windows.lock().unwrap().head(),
            Some(second),
            "the window's store did not see the agent's edit"
        );
        assert_eq!(
            reopened.head,
            Some(first.to_hex()),
            "the open reported the head it found, which was the first edit"
        );
    }

    /// The other half of the rule above: routing a re-open through the store
    /// it has must not make it a no-op. Opening a project reads it from disk,
    /// and a user who re-opens one that something else changed (the CLI, a
    /// copy dropped over it) expects to see that.
    #[test]
    fn reopening_the_open_project_still_reads_it_from_disk() {
        let tmp = tempdir().unwrap();
        let state = AppState::new();
        open_project_for_test(&state, tmp.path()).unwrap();

        // Another process moves the head: a store of its own on the folder.
        let mut elsewhere = Store::open(tmp.path()).unwrap();
        let moved = elsewhere.append(an_edit(7)).unwrap();
        assert_eq!(
            state.store_handle().unwrap().lock().unwrap().head(),
            None,
            "the open store cannot know yet"
        );

        let reopened = open_project_for_test(&state, tmp.path()).unwrap();

        assert_eq!(reopened.head, Some(moved.to_hex()));
        assert_eq!(
            state.store_handle().unwrap().lock().unwrap().head(),
            Some(moved),
            "the re-opened project did not pick up the head on disk"
        );
    }

    /// #501 item 2. The sweep decides what is unreferenced and then deletes
    /// it, and a tool that runs off the store lock stages (taking the lock),
    /// writes its output, and only later commits the node that names it. A
    /// sweep with no lock held could have its in-flight check pass just before
    /// such a tool stages, and then delete the output the tool wrote before
    /// the check could see it.
    ///
    /// So when the project being opened is the one already open, the sweep
    /// runs holding that store's lock: nothing can stage from it while the
    /// sweep checks and deletes. This stops the open at the point where the
    /// sweep would start and asks whether the lock is held there. (The
    /// closure stands in for the sweep, then runs the real one.)
    #[test]
    fn reopening_the_open_project_sweeps_while_holding_its_store_lock() {
        let tmp = tempdir().unwrap();
        let state = AppState::new();
        open_project_for_test(&state, tmp.path()).unwrap();
        let handle = state.store_handle().expect("store");

        let (at_the_sweep, opener_is_there) = std::sync::mpsc::channel();
        let (carry_on, opener_waits) = std::sync::mpsc::channel::<()>();
        let opener = {
            let state = state.clone();
            let dir = tmp.path().to_path_buf();
            std::thread::spawn(move || {
                open_project_with(&state, dir, |store| {
                    at_the_sweep.send(()).expect("the test is listening");
                    opener_waits.recv().expect("the test lets the sweep go on");
                    sweep_orphaned_audio(store);
                })
            })
        };

        opener_is_there
            .recv_timeout(std::time::Duration::from_secs(30))
            .expect("the open reached its sweep");
        // A tool that is about to stage takes the lock to do it.
        let could_stage = handle.try_lock().is_ok();
        carry_on.send(()).unwrap();
        opener.join().unwrap().unwrap();

        assert!(
            !could_stage,
            "the sweep ran without the open store's lock, so a tool could stage between its \
             in-flight check and its deletes and lose the output it had just written"
        );
    }

    /// #501, the case #514 left open. A turn is running on project A and the
    /// user opens B, then A again. `AppState` holds B by then, so A is not
    /// "the project already open", and the second open of A used to build a
    /// second `Store` for the folder while the turn still held the first:
    /// two writers on A.
    ///
    /// A store that is still alive for a folder (here: the one the turn
    /// holds) is the one any open of that folder gets, so the window and the
    /// turn are one writer again.
    #[test]
    fn a_project_opened_again_after_another_shares_the_store_a_turn_still_holds() {
        let (a, b) = (tempdir().unwrap(), tempdir().unwrap());
        let state = AppState::new();
        open_project_for_test(&state, a.path()).unwrap();
        // What the agent holds for the length of a turn.
        let turns = state.store_handle().expect("store");
        let first = turns.lock().unwrap().append(an_edit(1)).unwrap();

        open_project_for_test(&state, b.path()).unwrap();
        assert!(
            !Arc::ptr_eq(&turns, &state.store_handle().expect("store")),
            "B must not be on A's store"
        );

        let reopened = open_project_for_test(&state, a.path()).unwrap();

        let windows = state.store_handle().expect("store");
        assert!(
            Arc::ptr_eq(&turns, &windows),
            "opening A again after B built a second store for A while the turn still held the \
             first: two writers on one folder"
        );
        assert_eq!(
            state.project_dir.lock().unwrap().as_deref(),
            Some(a.path()),
            "the open project is A again"
        );
        // And the consequence: what the turn commits is what the window sees.
        let second = turns.lock().unwrap().append(an_edit(2)).unwrap();
        assert_eq!(
            windows.lock().unwrap().head(),
            Some(second),
            "the window's store did not see the turn's edit"
        );
        assert_eq!(reopened.head, Some(first.to_hex()));
    }

    /// #501 item 2, for the same case. The sweep an open runs must hold the
    /// lock a tool takes to stage, and the store a turn holds is the one a
    /// tool stages from. Opening A again after B has to sweep under *that*
    /// store's lock, not under a second store's that nothing stages from.
    /// Same method as the test above for the project already open: stop the
    /// open where the sweep would start and try the turn's lock.
    #[test]
    fn a_project_opened_again_after_another_sweeps_under_the_lock_the_turn_stages_with() {
        let (a, b) = (tempdir().unwrap(), tempdir().unwrap());
        let state = AppState::new();
        open_project_for_test(&state, a.path()).unwrap();
        let turns = state.store_handle().expect("store");
        open_project_for_test(&state, b.path()).unwrap();

        let (at_the_sweep, opener_is_there) = std::sync::mpsc::channel();
        let (carry_on, opener_waits) = std::sync::mpsc::channel::<()>();
        let opener = {
            let state = state.clone();
            let dir = a.path().to_path_buf();
            std::thread::spawn(move || {
                open_project_with(&state, dir, |store| {
                    at_the_sweep.send(()).expect("the test is listening");
                    opener_waits.recv().expect("the test lets the sweep go on");
                    sweep_orphaned_audio(store);
                })
            })
        };

        opener_is_there
            .recv_timeout(std::time::Duration::from_secs(30))
            .expect("the open reached its sweep");
        // A tool that is about to stage takes the turn's lock to do it.
        let could_stage = turns.try_lock().is_ok();
        carry_on.send(()).unwrap();
        opener.join().unwrap().unwrap();

        assert!(
            !could_stage,
            "the sweep ran without the lock of the store the turn stages from, so a tool could \
             stage between its in-flight check and its deletes and lose the output it had just \
             written"
        );
    }

    /// The output of a tool that is running off the lock survives A, then B,
    /// then A again, and goes once the tool is done. (The in-flight count is
    /// the project's, shared by every `Store` on it, so this holds without
    /// the registry; it guards the sweep against being skipped or run on
    /// anything else.)
    #[test]
    fn a_project_opened_again_after_another_keeps_the_output_of_a_tool_still_running() {
        let (a, b) = (tempdir().unwrap(), tempdir().unwrap());
        let state = AppState::new();
        open_project_for_test(&state, a.path()).unwrap();
        let turns = state.store_handle().expect("store");
        turns.lock().unwrap().append(an_edit(1)).unwrap();

        let running = turns.lock().unwrap().stage().unwrap();
        let derived = tools::derived_dir(a.path());
        std::fs::create_dir_all(&derived).unwrap();
        let output = derived.join("0123abcd-not-yet-named.wav");
        std::fs::write(&output, b"RIFF-not-yet-named").unwrap();

        open_project_for_test(&state, b.path()).unwrap();
        open_project_for_test(&state, a.path()).unwrap();
        assert!(
            output.is_file(),
            "opening A again swept the output of a tool that was running"
        );

        drop(running);
        open_project_for_test(&state, b.path()).unwrap();
        open_project_for_test(&state, a.path()).unwrap();
        assert!(!output.exists(), "the orphan should go once nothing runs");
    }

    /// The registry is a view of what is alive, not an owner. Once the turn
    /// lets go of A's store and the window has moved to B, nothing holds it,
    /// and the next open of A builds a store from the folder as it is then.
    #[test]
    fn a_store_nobody_holds_any_more_is_not_kept_for_the_next_open() {
        let (a, b) = (tempdir().unwrap(), tempdir().unwrap());
        let state = AppState::new();
        open_project_for_test(&state, a.path()).unwrap();
        let turns = state.store_handle().expect("store");
        let weak = Arc::downgrade(&turns);
        turns.lock().unwrap().append(an_edit(1)).unwrap();

        open_project_for_test(&state, b.path()).unwrap();
        assert!(
            weak.upgrade().is_some(),
            "the turn still holds A's store, so it is alive"
        );
        drop(turns);
        assert_eq!(
            weak.strong_count(),
            0,
            "A's store outlived its last holder: whatever remembers it must not own it"
        );

        // The folder moves on while nothing has it open.
        let mut elsewhere = Store::open(a.path()).unwrap();
        let moved = elsewhere.append(an_edit(9)).unwrap();

        let reopened = open_project_for_test(&state, a.path()).unwrap();
        assert_eq!(
            reopened.head,
            Some(moved.to_hex()),
            "the new store did not read the folder"
        );
        assert_eq!(
            state.store_handle().unwrap().lock().unwrap().head(),
            Some(moved)
        );
        assert_eq!(weak.strong_count(), 0, "the old store came back");
    }

    /// Two folders are two projects whatever their stores are doing: opening
    /// one never hands back the other's store.
    #[test]
    fn two_different_folders_never_share_a_store() {
        let (a, b) = (tempdir().unwrap(), tempdir().unwrap());
        let state = AppState::new();
        open_project_for_test(&state, a.path()).unwrap();
        let of_a = state.store_handle().expect("store");
        open_project_for_test(&state, b.path()).unwrap();
        let of_b = state.store_handle().expect("store");
        assert!(!Arc::ptr_eq(&of_a, &of_b), "A and B share a store");

        // Back and forth, each open gets its own folder's store.
        open_project_for_test(&state, a.path()).unwrap();
        assert!(Arc::ptr_eq(&of_a, &state.store_handle().unwrap()));
        open_project_for_test(&state, b.path()).unwrap();
        assert!(Arc::ptr_eq(&of_b, &state.store_handle().unwrap()));
        assert_eq!(of_a.lock().unwrap().project_dir(), a.path());
        assert_eq!(of_b.lock().unwrap().project_dir(), b.path());
    }

    /// One folder under two spellings (a `..` in the path, a symlink) is one
    /// project, as for the in-flight count (`session::store`): the registry
    /// is keyed by canonical path.
    #[test]
    fn two_spellings_of_one_folder_share_a_store() {
        let tmp = tempdir().unwrap();
        let (a, b) = (tmp.path().join("a"), tmp.path().join("b"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        let state = AppState::new();
        open_project_for_test(&state, &a).unwrap();
        let of_a = state.store_handle().expect("store");

        open_project_for_test(&state, &b).unwrap();
        let dotdot = tmp.path().join("b").join("..").join("a");
        open_project_for_test(&state, &dotdot).unwrap();
        assert!(
            Arc::ptr_eq(&of_a, &state.store_handle().unwrap()),
            "`b/../a` is a second store for the folder `a`"
        );
        // The store keeps the spelling it was made with, which is what a
        // staged edit is checked against when it commits.
        assert_eq!(of_a.lock().unwrap().project_dir(), a);

        #[cfg(unix)]
        {
            let link = tmp.path().join("link-to-a");
            std::os::unix::fs::symlink(&a, &link).unwrap();
            open_project_for_test(&state, &b).unwrap();
            open_project_for_test(&state, &link).unwrap();
            assert!(
                Arc::ptr_eq(&of_a, &state.store_handle().unwrap()),
                "a symlink to the folder `a` is a second store for it"
            );
        }
    }

    /// Two opens of a folder nothing has open can both find no live store and
    /// both build one. The registry keeps whichever registers first, and the
    /// other open must take that one instead of putting its own into
    /// `AppState`: its own would be a second live store for the folder.
    ///
    /// The build step is handed in so the competing open can run exactly
    /// there, after the registry has said "none" and before this one has
    /// registered.
    #[test]
    fn an_open_that_loses_the_race_to_register_takes_the_winners_store() {
        let tmp = tempdir().unwrap();
        let state = AppState::new();
        let competitor = Arc::new(Mutex::new(Store::open(tmp.path()).unwrap()));

        let (got, is_new) = live_or_new_store_with(&state, tmp.path(), |dir| {
            let won = state.live_stores.register(dir, Arc::clone(&competitor));
            assert!(
                Arc::ptr_eq(&won, &competitor),
                "the competitor registers first"
            );
            Ok(Store::open(dir)?)
        })
        .unwrap();

        assert!(
            Arc::ptr_eq(&got, &competitor),
            "the open that lost the race went on with a store of its own"
        );
        assert!(
            !is_new,
            "a store another open registered is not new: it has to be read"
        );
        assert!(Arc::ptr_eq(
            &state.live_stores.find(tmp.path()).unwrap(),
            &competitor
        ));
    }

    /// A live store keeps the spelling of the folder it was made with, and
    /// a spelling can stop reaching the folder while the store lives: open A
    /// through a symlink, let a turn hold the store, open B, remove the
    /// symlink, open A by its real path. Reading the folder again through the
    /// store's own spelling would create an empty folder where the symlink was
    /// and hand the window and the turn a project with no history.
    ///
    /// The folder is read through the spelling the user gave instead, into the
    /// same store, so the turn and the window stay one writer on the real
    /// folder. An edit staged before that carries the old spelling, and its
    /// commit is refused ("another project") rather than landing in a folder
    /// that is not the project.
    #[cfg(unix)]
    #[test]
    fn a_live_store_whose_spelling_no_longer_resolves_is_read_through_the_one_given() {
        let tmp = tempdir().unwrap();
        let (real, other, link) = (
            tmp.path().join("a"),
            tmp.path().join("b"),
            tmp.path().join("l"),
        );
        std::fs::create_dir_all(&real).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let state = AppState::new();

        open_project_for_test(&state, &link).unwrap();
        let turns = state.store_handle().expect("store");
        let first = turns.lock().unwrap().append(an_edit(1)).unwrap();
        // A tool the turn has running, staged under the symlink's spelling.
        let running = turns.lock().unwrap().stage().unwrap();

        open_project_for_test(&state, &other).unwrap();
        std::fs::remove_file(&link).unwrap();
        let reopened = open_project_for_test(&state, &real).unwrap();

        assert!(
            !link.exists(),
            "the folder was read through a spelling that no longer reaches it, which made a new \
             folder there"
        );
        let windows = state.store_handle().expect("store");
        assert!(
            Arc::ptr_eq(&turns, &windows),
            "the turn and the window are two writers"
        );
        assert_eq!(
            reopened.head,
            Some(first.to_hex()),
            "the project opened with no history"
        );
        assert_eq!(windows.lock().unwrap().head(), Some(first));
        assert_eq!(windows.lock().unwrap().project_dir(), real);
        assert_eq!(
            state.project_dir.lock().unwrap().as_deref(),
            Some(real.as_path())
        );
        assert!(
            matches!(
                turns.lock().unwrap().commit(running),
                Err(session::Error::Staged(_))
            ),
            "an edit staged under the old spelling must be refused, not written to it"
        );

        // From here the turn writes to the real folder.
        let second = turns.lock().unwrap().append(an_edit(2)).unwrap();
        assert_eq!(Store::open(&real).unwrap().head(), Some(second));
        assert!(!link.exists());
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
            Some("claude-sonnet-5-5".into()),
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
            Some("claude-sonnet-5-5".into()),
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
            Some("anthropic/claude-sonnet-5.5".into()),
            None,
            |_| panic!("must not hit the keychain when no profile is active"),
        );
        assert_eq!(provider, "openrouter");
        assert_eq!(key.as_deref(), Some("or-key"));
        assert_eq!(model.as_deref(), Some("anthropic/claude-sonnet-5.5"));
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

    // ---- Path traversal at the IPC boundary (#266) ----------------

    /// The names `upsert_skill` and `delete_skill` always rejected, and
    /// `read_skill` did not until #266.
    ///
    /// `/etc/hosts` is the one that matters most and is the least
    /// obvious: `Path::join` with an absolute argument *discards the
    /// base*, so this never resolved under the skills directory at all.
    /// `..` traversal is the familiar case; both are refused by the
    /// same character rule, since `/` and `\\` are not in the allowed
    /// set.
    #[test]
    fn skill_path_refuses_names_that_escape_the_directory() {
        let dir = tempdir().unwrap();
        for name in [
            "../x",
            "../../Documents/notes",
            "/etc/hosts",
            "..\\x",
            "a/b",
            "",
        ] {
            assert!(
                skill_path(dir.path(), name).is_err(),
                "skill_path accepted `{name}`, which does not resolve \
                 inside the skills directory"
            );
        }
    }

    #[test]
    fn agent_profile_path_refuses_names_that_escape_the_directory() {
        let dir = tempdir().unwrap();
        for name in ["../x", "/etc/hosts", "a/b", ""] {
            assert!(
                agent_profile_path(dir.path(), name).is_err(),
                "agent_profile_path accepted `{name}`"
            );
        }
    }

    /// The rejection has to be the *escape*, not the join — an ordinary
    /// name must still resolve, and resolve inside the directory.
    #[test]
    fn a_valid_name_resolves_inside_the_directory() {
        let dir = tempdir().unwrap();
        let path = skill_path(dir.path(), "my-skill_2").expect("valid name");
        assert_eq!(path, dir.path().join("my-skill_2.md"));
        assert!(path.starts_with(dir.path()));

        let profile = agent_profile_path(dir.path(), "editor").expect("valid name");
        assert!(profile.starts_with(dir.path()));
    }

    /// Why the helper exists rather than a convention to call the
    /// validator first: without it, the join silently leaves the base.
    #[test]
    fn an_unvalidated_join_would_leave_the_directory() {
        let dir = tempdir().unwrap();
        let unchecked = dir.path().join(format!("{}.md", "/etc/hosts"));
        assert!(
            !unchecked.starts_with(dir.path()),
            "this test encodes the reason for skill_path; if an absolute \
             join now stays under the base, the helper's rationale has \
             changed"
        );
    }

    // ---- The chosen model survives a restart (#249) ---------------

    /// The map lived only in memory and nothing re-read it at startup,
    /// so after a restart `rebuild_agent` saw no model and silently
    /// used `provider.default_model()` — while Settings went on
    /// showing the user's pick from localStorage.
    ///
    /// `restore_models` is the startup path with the keychain read
    /// injected, so this exercises the real wiring rather than a copy.
    #[test]
    fn a_restart_restores_the_last_selected_model() {
        let state = AppState::default();

        // Before: a fresh process knows nothing.
        assert_eq!(state.model_for("anthropic"), None);

        // What the keychain would hand back after the user picked one.
        restore_models(&state, |id| {
            Ok((id == "anthropic").then(|| "claude-opus-5".to_string()))
        });

        assert_eq!(
            state.model_for("anthropic").as_deref(),
            Some("claude-opus-5"),
            "the selection did not survive the restart, so rebuild_agent \
             would fall back to the provider default"
        );
    }

    /// Every provider's choice, not just the active one's: switching
    /// provider after a restart must not lose a model chosen earlier.
    #[test]
    fn a_restart_restores_every_provider_not_only_the_active_one() {
        let state = AppState::default();
        restore_models(&state, |id| Ok(Some(format!("{id}-chosen"))));

        for id in ai::SUPPORTED_PROVIDER_IDS {
            assert_eq!(
                state.model_for(id).as_deref(),
                Some(format!("{id}-chosen").as_str()),
                "provider {id} lost its model across the restart"
            );
        }
    }

    /// A provider the user never chose for must stay unset, so the
    /// agent uses that provider's own default rather than inheriting
    /// something arbitrary.
    #[test]
    fn a_provider_with_no_stored_choice_stays_unset() {
        let state = AppState::default();
        restore_models(&state, |_| Ok(None));

        for id in ai::SUPPORTED_PROVIDER_IDS {
            assert_eq!(state.model_for(id), None);
        }
    }

    // ---- A keychain that cannot be read is not a fresh install (#394) --

    use ai::keychain::KeychainError;

    /// Nothing stored, and every read answered: the Welcome flow is the
    /// right thing to show, so `has_api_key` answers `false` and does not
    /// reject.
    #[test]
    fn startup_with_nothing_stored_is_a_fresh_install() {
        let state = AppState::default();
        load_startup_settings(&state, || Ok(None), |_| Ok(None), |_| Ok(None));

        assert_eq!(state.active_provider_id(), ai::ANTHROPIC_ID);
        assert_eq!(state.api_key_snapshot(), None);
        assert_eq!(state.keychain_read_error(), None);
        assert_eq!(
            has_key_answer(state.keychain_read_error(), || Ok(false)),
            Ok(false)
        );
    }

    /// The bug: on a locked keyring every read failed, the failure was
    /// collapsed to "no key", and a user with a stored key was shown the
    /// first-run Welcome. The failure has to reach `has_api_key`.
    #[test]
    fn startup_read_failure_is_reported_not_treated_as_a_fresh_install() {
        let state = AppState::default();
        load_startup_settings(
            &state,
            || Err(KeychainError::new("locked")),
            |_| Ok(None),
            |_| Ok(None),
        );

        let stashed = state.keychain_read_error().expect("the failure is kept");
        assert_eq!(stashed, "locked", "the reason alone, not the whole message");
        // Even though a live read would say "no key", `has_api_key` must
        // not: that read is about the wrong provider.
        let answer = has_key_answer(state.keychain_read_error(), || Ok(false));
        let err = answer.expect_err("must reject, not answer false");
        assert!(err.contains("locked"), "{err}");
    }

    #[test]
    fn startup_key_read_failure_is_reported() {
        let state = AppState::default();
        load_startup_settings(
            &state,
            || Ok(Some("openrouter".to_string())),
            |_| Err(KeychainError::new("dismissed the unlock prompt")),
            |_| Ok(None),
        );

        assert_eq!(state.active_provider_id(), "openrouter");
        assert_eq!(state.api_key_snapshot(), None);
        let stashed = state.keychain_read_error().expect("the failure is kept");
        assert!(stashed.contains("dismissed the unlock prompt"), "{stashed}");
    }

    /// The first failure is the one reported, and a later one does not
    /// replace it.
    #[test]
    fn the_first_startup_failure_is_the_one_kept() {
        let state = AppState::default();
        load_startup_settings(
            &state,
            || Err(KeychainError::new("first")),
            |_| Err(KeychainError::new("second")),
            |_| Ok(None),
        );
        let stashed = state.keychain_read_error().unwrap();
        assert!(
            stashed.contains("first") && !stashed.contains("second"),
            "{stashed}"
        );
    }

    #[test]
    fn startup_restores_provider_and_key() {
        let state = AppState::default();
        load_startup_settings(
            &state,
            || Ok(Some("openrouter".to_string())),
            |id| Ok((id == "openrouter").then(|| "sk-or-1".to_string())),
            |id| Ok((id == "openrouter").then(|| "or-model".to_string())),
        );

        assert_eq!(state.active_provider_id(), "openrouter");
        assert_eq!(state.api_key_snapshot().as_deref(), Some("sk-or-1"));
        assert_eq!(state.model_for("openrouter").as_deref(), Some("or-model"));
        assert_eq!(state.keychain_read_error(), None);
        assert_eq!(
            has_key_answer(state.keychain_read_error(), || Ok(true)),
            Ok(true)
        );
    }

    /// A clean read after a failed one (the user saved a key, so the
    /// keychain is evidently working) clears the stash, or `has_api_key`
    /// would go on rejecting.
    #[test]
    fn a_later_clean_startup_read_clears_the_stash() {
        let state = AppState::default();
        state.set_keychain_read_error(Some("locked".to_string()));
        load_startup_settings(&state, || Ok(None), |_| Ok(None), |_| Ok(None));
        assert_eq!(state.keychain_read_error(), None);
    }

    // ---- MCP servers after a refused startup (#497) ---------------------

    use std::cell::RefCell;

    fn stdio_server(command: &str, env: &[(&str, &str)]) -> mcp::McpServerConfig {
        mcp::McpServerConfig::Stdio {
            command: command.to_string(),
            args: Vec::new(),
            env: env
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            enabled: true,
        }
    }

    /// A state with these servers registered, as `install_mcp` leaves it
    /// after reading `mcp.json`.
    fn state_with_servers(servers: &[(&str, mcp::McpServerConfig)]) -> AppState {
        let state = AppState::default();
        {
            let mut cfg = state.mcp_config.lock().unwrap();
            for (id, server) in servers {
                cfg.servers.insert((*id).to_string(), server.clone());
            }
        }
        state
    }

    /// Never run: the command does not exist, so a server that is
    /// attempted fails to spawn, and a server that is skipped says why.
    const NO_SUCH_COMMAND: &str = "/nonexistent/edytlab-test-no-such-command";

    fn registry_view(state: &AppState, id: &str) -> mcp::McpServerSummary {
        let cfg = state.mcp_config.lock().unwrap().servers[id].clone();
        state.mcp.summary(id, &cfg)
    }

    /// The bug: a user who dismissed startup's unlock prompt got another
    /// one for each MCP server whose env or headers refer to the keychain,
    /// as the autostart resolved them. A server that does not refer to it
    /// is not affected, and the skipped one shows its reason as an error.
    #[test]
    fn after_a_refused_startup_servers_that_need_the_keychain_are_not_started() {
        let state = state_with_servers(&[
            (
                "github",
                stdio_server(
                    NO_SUCH_COMMAND,
                    &[("GITHUB_TOKEN", "<keychain:github_token>")],
                ),
            ),
            ("plain", stdio_server(NO_SUCH_COMMAND, &[("MODE", "fast")])),
        ]);
        load_startup_settings(
            &state,
            || Err(KeychainError::access_denied("locked")),
            |_| Ok(None),
            |_| Ok(None),
        );
        assert!(state.keychain_refused(), "precondition");

        let asked = RefCell::new(Vec::new());
        start_enabled_mcp_servers_with(&state, |slot| {
            asked.borrow_mut().push(slot.to_string());
            None
        });

        assert!(
            asked.borrow().is_empty(),
            "the keychain was not asked for {:?}",
            asked.borrow()
        );
        let skipped = registry_view(&state, "github");
        assert_eq!(skipped.status, mcp::McpServerStatus::Error);
        let reason = skipped.last_error.expect("the reason is on the registry");
        assert!(
            reason.contains("refused access") && reason.contains("locked"),
            "{reason}"
        );

        let tried = registry_view(&state, "plain");
        assert_eq!(tried.status, mcp::McpServerStatus::Error);
        assert!(
            !tried.last_error.unwrap().contains("refused access"),
            "a server with no reference to the keychain is started as ever; \
             here that fails at the missing command"
        );
    }

    /// And with a keychain that was read, nothing changes: the reference
    /// is resolved, here to nothing, which is the server's own failure.
    #[test]
    fn after_a_clean_startup_servers_resolve_their_keychain_references() {
        let state = state_with_servers(&[(
            "github",
            stdio_server(
                NO_SUCH_COMMAND,
                &[("GITHUB_TOKEN", "<keychain:github_token>")],
            ),
        )]);
        load_startup_settings(&state, || Ok(None), |_| Ok(None), |_| Ok(None));
        assert!(!state.keychain_refused());

        let asked = RefCell::new(Vec::new());
        start_enabled_mcp_servers_with(&state, |slot| {
            asked.borrow_mut().push(slot.to_string());
            None
        });

        assert_eq!(*asked.borrow(), vec!["github_token".to_string()]);
        let reason = registry_view(&state, "github").last_error.unwrap();
        assert!(!reason.contains("refused access"), "{reason}");
    }

    /// Only a refusal skips them. A read that failed for another reason
    /// raises no prompt, so the servers go on as before.
    #[test]
    fn only_a_refusal_not_any_failed_read_is_recorded() {
        let failed = AppState::default();
        load_startup_settings(
            &failed,
            || Err(KeychainError::new("no secret service")),
            |_| Ok(None),
            |_| Ok(None),
        );
        assert!(failed.keychain_read_error().is_some(), "it did fail");
        assert!(!failed.keychain_refused(), "but it was not a refusal");

        let key_refused = AppState::default();
        load_startup_settings(
            &key_refused,
            || Ok(None),
            |_| Err(KeychainError::access_denied("dismissed")),
            |_| Ok(None),
        );
        assert!(key_refused.keychain_refused());

        // Refused only at the models, with the provider and key read: the
        // keychain is refusing all the same, though there is no stash.
        let models_refused = AppState::default();
        load_startup_settings(
            &models_refused,
            || Ok(None),
            |_| Ok(None),
            |_| Err(KeychainError::access_denied("dismissed")),
        );
        assert_eq!(models_refused.keychain_read_error(), None);
        assert!(models_refused.keychain_refused());
    }

    /// The flag clears with the stash, on the save that proves the
    /// keychain works, and a launch starts without it.
    #[test]
    fn a_save_that_worked_lifts_the_refusal_and_a_new_launch_starts_clean() {
        let state = AppState::default();
        load_startup_settings(
            &state,
            || Err(KeychainError::access_denied("locked")),
            |_| Ok(None),
            |_| Ok(None),
        );
        assert!(state.keychain_refused());

        keychain_readable_again(&state, |_| Ok(None));
        assert!(!state.keychain_refused());
        assert_eq!(state.keychain_read_error(), None);

        // Without a stash, too (refused at the models only).
        state.set_keychain_refused(true);
        keychain_readable_again(&state, |_| Ok(None));
        assert!(!state.keychain_refused());

        assert!(!AppState::default().keychain_refused());
    }

    /// When a server is skipped: only while the refusal stands, only for a
    /// server that refers to the keychain, and the reason says why.
    #[test]
    fn the_skip_reason_names_the_refusal_and_only_applies_to_keychain_servers() {
        let github = stdio_server("npx", &[("GITHUB_TOKEN", "<keychain:github_token>")]);
        let plain = stdio_server("npx", &[]);
        let state = AppState::default();

        assert_eq!(mcp_skip_reason(&state, &github), None, "not refused");
        state.set_keychain_refused(true);
        assert_eq!(mcp_skip_reason(&state, &plain), None, "no reference");
        let reason = mcp_skip_reason(&state, &github).expect("skipped");
        assert!(
            !reason.contains("()"),
            "no empty reason in brackets: {reason}"
        );
        state.set_keychain_read_error(Some("dismissed the prompt".into()));
        assert!(mcp_skip_reason(&state, &github)
            .unwrap()
            .contains("(dismissed the prompt)"));
        state.set_keychain_refused(false);
        assert_eq!(mcp_skip_reason(&state, &github), None, "lifted");
    }

    // ---- Settings' reads after a refused startup (#497) ----------------

    /// A keychain that was asked and answered. The tests below pass it as
    /// the read, and count how often it ran.
    fn stored_url(asked: &Cell<u32>) -> Result<Option<String>, KeychainError> {
        asked.set(asked.get() + 1);
        Ok(Some("http://localhost:1234/v1".to_string()))
    }

    /// The bug: opening Settings read the base URL and the level from the
    /// keychain whatever startup had found, and on a locked keyring each
    /// read is an unlock prompt the user had just declined. After a refused
    /// startup the read is not made, and the answer says why.
    #[test]
    fn a_stored_setting_is_not_read_after_a_refused_startup() {
        let state = AppState::default();
        state.set_keychain_refused(true);
        state.set_keychain_read_error(Some("locked".into()));
        let asked = Cell::new(0);

        let err = stored_setting_unless_refused(&state, || stored_url(&asked))
            .expect_err("a keychain that refused is not asked again");

        assert_eq!(asked.get(), 0, "the keychain was not asked");
        assert!(
            err.contains("refused access") && err.contains("locked") && err.contains("not asked"),
            "{err}"
        );
    }

    /// Everything else reads as before: a clean startup, a failed read that
    /// was not a refusal (no prompt to avoid, and the read may well work),
    /// and the read after the refusal was lifted.
    #[test]
    fn a_stored_setting_is_read_unless_startup_was_refused() {
        let asked = Cell::new(0);
        let state = AppState::default();
        assert_eq!(
            stored_setting_unless_refused(&state, || stored_url(&asked)),
            Ok(Some("http://localhost:1234/v1".to_string()))
        );

        state.set_keychain_read_error(Some("no secret service".into()));
        assert!(stored_setting_unless_refused(&state, || stored_url(&asked)).is_ok());
        assert_eq!(asked.get(), 2);

        state.set_keychain_refused(true);
        assert!(stored_setting_unless_refused(&state, || stored_url(&asked)).is_err());
        keychain_readable_again(&state, |_| Ok(None));
        assert!(stored_setting_unless_refused(&state, || stored_url(&asked)).is_ok());
        assert_eq!(asked.get(), 3, "asked again once a save went through");

        // And a read that fails when it is made is still the keychain's reason.
        let failing = AppState::default();
        assert_eq!(
            stored_setting_unless_refused::<String>(&failing, || Err(KeychainError::new(
                "ambiguous"
            ))),
            Err("ambiguous".to_string())
        );
    }

    /// The model list reads two things (the saved key and the saved base
    /// URL), so two more prompts. After a refused startup it reads neither,
    /// even though a key typed into the form would make the first read
    /// unnecessary: the base URL would still be a guess.
    #[test]
    fn the_model_list_reads_neither_key_nor_base_url_after_a_refused_startup() {
        let state = AppState::default();
        state.set_keychain_refused(true);
        let asked = RefCell::new(Vec::new());
        let load = |what: &'static str| {
            let asked = &asked;
            move |provider: &str| {
                asked.borrow_mut().push(format!("{what}:{provider}"));
                Some(what.to_string())
            }
        };

        for typed in [None, Some("sk-typed".to_string())] {
            let err = catalogue_inputs(&state, "anthropic", typed, load("key"), load("base"))
                .expect_err("refused");
            assert!(err.contains("refused access"), "{err}");
        }
        assert!(asked.borrow().is_empty(), "asked {:?}", asked.borrow());
    }

    /// Without a refusal the inputs are what they were: the typed key wins
    /// and spares the keychain read of the key, and the base URL is read.
    #[test]
    fn the_model_list_reads_its_inputs_when_startup_was_not_refused() {
        let state = AppState::default();
        let asked = RefCell::new(Vec::new());
        let load = |what: &'static str| {
            let asked = &asked;
            move |provider: &str| {
                asked.borrow_mut().push(format!("{what}:{provider}"));
                Some(format!("stored-{what}"))
            }
        };

        assert_eq!(
            catalogue_inputs(&state, "groq", None, load("key"), load("base")),
            Ok((Some("stored-key".into()), Some("stored-base".into())))
        );
        assert_eq!(*asked.borrow(), ["key:groq", "base:groq"]);

        asked.borrow_mut().clear();
        assert_eq!(
            catalogue_inputs(
                &state,
                "groq",
                Some(" gsk-typed ".into()),
                load("key"),
                load("base")
            ),
            Ok((Some("gsk-typed".into()), Some("stored-base".into())))
        );
        assert_eq!(
            *asked.borrow(),
            ["base:groq"],
            "the typed key is not looked up"
        );
    }

    /// The bug: a read that failed came back as `None`, which Settings
    /// shows as an empty field and saves as "delete the override".
    #[test]
    fn a_stored_setting_that_could_not_be_read_is_an_error_not_none() {
        let err = stored_setting::<String>(Err(KeychainError::new("locked")))
            .expect_err("a failed read must reject");
        assert_eq!(err, "locked", "the reason alone: Settings says the rest");
        assert_eq!(stored_setting::<String>(Ok(None)), Ok(None));
        assert_eq!(
            stored_setting(Ok(Some("http://localhost:1234".to_string()))),
            Ok(Some("http://localhost:1234".to_string()))
        );
    }

    #[test]
    fn the_active_provider_is_not_answered_while_startup_was_unreadable() {
        let err = active_provider_answer(Some("locked".into()), "anthropic".into())
            .expect_err("the default is not what is stored");
        assert_eq!(err, "locked");
        assert_eq!(
            active_provider_answer(None, "openrouter".into()),
            Ok("openrouter".to_string())
        );
    }

    /// The bug: after a startup the keychain refused, the models were never
    /// restored, and "no model" came back as the empty answer Settings reads
    /// as "none chosen". A model that is known is an answer all the same.
    #[test]
    fn a_model_is_not_answered_as_none_chosen_while_startup_left_it_unread() {
        let why = || Some("the keychain refused access at launch".to_string());
        assert_eq!(
            active_model_answer(why(), None),
            Err("the keychain refused access at launch".to_string())
        );
        assert_eq!(
            active_model_answer(why(), Some("claude-opus-5".into())),
            Ok("claude-opus-5".to_string()),
            "restored before the refusal, or chosen since"
        );
        assert_eq!(active_model_answer(None, None), Ok(String::new()));
        assert_eq!(
            active_model_answer(None, Some("claude-opus-5".into())),
            Ok("claude-opus-5".to_string())
        );
    }

    /// Both ways startup can leave the models unread count: a refusal ends
    /// the reads, and a failed read may have been a model's own.
    #[test]
    fn the_models_are_unread_after_a_refusal_or_after_any_failed_read() {
        let clean = AppState::default();
        assert_eq!(models_unread_reason(&clean), None);

        let failed = AppState::default();
        failed.set_keychain_read_error(Some("ambiguous".into()));
        let reason = models_unread_reason(&failed).expect("a failed read");
        assert!(
            reason.contains("could not be read") && reason.contains("ambiguous"),
            "{reason}"
        );

        let refused = AppState::default();
        refused.set_keychain_refused(true);
        let reason = models_unread_reason(&refused).expect("a refusal");
        assert!(
            reason.contains("refused access") && !reason.contains("()"),
            "{reason}"
        );
        refused.set_keychain_read_error(Some("locked".into()));
        assert!(models_unread_reason(&refused).unwrap().contains("(locked)"));
    }

    #[test]
    fn has_key_answer_passes_a_live_read_failure_through() {
        let answer = has_key_answer(None, || Err(KeychainError::new("locked")));
        let err = answer.expect_err("a read failure is not `false`");
        assert_eq!(
            err, "locked",
            "the reason alone: the banner supplies the rest"
        );
    }

    // ---- A locked keychain is asked once, not once per slot (#394) -----

    use std::cell::Cell;

    /// The user dismissed the unlock prompt. Every further read would
    /// raise it again, so startup must stop at the first refusal instead
    /// of asking for the active provider, its key and six models in a row.
    #[test]
    fn a_refused_active_provider_read_ends_startup_there() {
        let state = AppState::default();
        let reads = Cell::new(0u32);
        let counted = || reads.set(reads.get() + 1);
        load_startup_settings(
            &state,
            || {
                counted();
                Err(KeychainError::access_denied("prompt dismissed"))
            },
            |_| {
                counted();
                Ok(Some("sk-x".to_string()))
            },
            |_| {
                counted();
                Ok(Some("a-model".to_string()))
            },
        );

        assert_eq!(reads.get(), 1, "nothing was read after the refusal");
        assert_eq!(
            state.keychain_read_error().as_deref(),
            Some("prompt dismissed")
        );
        assert_eq!(state.api_key_snapshot(), None);
        assert_eq!(state.model_for("anthropic"), None);
    }

    #[test]
    fn a_refused_key_read_skips_the_model_reads() {
        let state = AppState::default();
        let model_reads = Cell::new(0u32);
        load_startup_settings(
            &state,
            || Ok(None),
            |_| Err(KeychainError::access_denied("prompt dismissed")),
            |_| {
                model_reads.set(model_reads.get() + 1);
                Ok(None)
            },
        );

        assert_eq!(model_reads.get(), 0);
        assert_eq!(
            state.keychain_read_error().as_deref(),
            Some("prompt dismissed")
        );
    }

    /// Refused partway through the models: the ones already read stay,
    /// and the rest are left alone.
    #[test]
    fn a_refused_model_read_ends_the_loop_and_keeps_what_was_read() {
        let state = AppState::default();
        let asked = std::cell::RefCell::new(Vec::new());
        restore_models(&state, |id| {
            asked.borrow_mut().push(id.to_string());
            if asked.borrow().len() == 3 {
                Err(KeychainError::access_denied("prompt dismissed"))
            } else {
                Ok(Some(format!("{id}-chosen")))
            }
        });

        let asked = asked.into_inner();
        assert_eq!(asked.len(), 3, "stopped at the refusal: {asked:?}");
        assert_eq!(
            state.model_for(&asked[0]).as_deref(),
            Some(format!("{}-chosen", asked[0]).as_str())
        );
        assert_eq!(state.model_for(&asked[2]), None);
    }

    /// Any other failure is about one slot, so the next one is still
    /// worth reading, and the failure is not reported as the keychain
    /// being unreadable (the key and provider came back fine).
    #[test]
    fn a_model_slot_that_cannot_be_decoded_does_not_end_startup() {
        let state = AppState::default();
        load_startup_settings(
            &state,
            || Ok(None),
            |_| Ok(Some("sk-x".to_string())),
            |id| {
                if id == "anthropic" {
                    Err(KeychainError::new("not UTF-8"))
                } else {
                    Ok(Some(format!("{id}-chosen")))
                }
            },
        );

        assert_eq!(state.keychain_read_error(), None);
        assert_eq!(state.api_key_snapshot().as_deref(), Some("sk-x"));
        assert_eq!(state.model_for("anthropic"), None);
        assert_eq!(
            state.model_for("openrouter").as_deref(),
            Some("openrouter-chosen")
        );
    }

    /// A failure that is not a refusal does not end startup's reads: the
    /// models are still restored after a key slot that would not decode.
    #[test]
    fn an_unreadable_key_slot_that_is_not_a_refusal_still_restores_models() {
        let state = AppState::default();
        load_startup_settings(
            &state,
            || Ok(None),
            |_| Err(KeychainError::new("not UTF-8")),
            |id| Ok((id == "anthropic").then(|| "m".to_string())),
        );

        assert_eq!(state.keychain_read_error().as_deref(), Some("not UTF-8"));
        assert_eq!(state.model_for("anthropic").as_deref(), Some("m"));
    }

    // ---- Models come back once the keychain does (#394) ----------------

    /// Startup was refused, so the chosen models were never read, and the
    /// first save that worked cleared the failure without reading them.
    /// The agent then ran the provider's default model while Settings went
    /// on showing the stored pick.
    #[test]
    fn a_save_after_a_refused_startup_restores_the_models_startup_skipped() {
        let state = AppState::default();
        load_startup_settings(
            &state,
            || Err(KeychainError::access_denied("prompt dismissed")),
            |_| Ok(None),
            |_| Ok(Some("never read".to_string())),
        );
        assert_eq!(state.model_for("anthropic"), None, "startup skipped them");

        keychain_readable_again(&state, |id| {
            Ok((id == "anthropic").then(|| "claude-opus-5".to_string()))
        });

        assert_eq!(state.keychain_read_error(), None, "the stash is cleared");
        assert_eq!(
            state.model_for("anthropic").as_deref(),
            Some("claude-opus-5"),
            "the stored pick is back, so rebuild_agent uses it"
        );
    }

    /// A model picked after the failure may be newer than the stored one
    /// (the pick could not be saved while the keychain was locked), so
    /// recovery fills gaps and overwrites nothing.
    #[test]
    fn recovery_does_not_overwrite_a_model_picked_since() {
        let state = AppState::default();
        state.set_keychain_read_error(Some("locked".to_string()));
        state.set_model_for("anthropic".to_string(), "picked-today".to_string());

        keychain_readable_again(&state, |id| Ok(Some(format!("{id}-stored"))));

        assert_eq!(
            state.model_for("anthropic").as_deref(),
            Some("picked-today")
        );
        assert_eq!(
            state.model_for("openrouter").as_deref(),
            Some("openrouter-stored")
        );
    }

    /// Every successful save passes through here, and almost none follow a
    /// failed startup. Those must not read eight model slots each.
    #[test]
    fn a_save_with_no_failure_kept_reads_no_models() {
        let state = AppState::default();
        let reads = Cell::new(0u32);
        keychain_readable_again(&state, |_| {
            reads.set(reads.get() + 1);
            Ok(None)
        });
        assert_eq!(reads.get(), 0);
        assert_eq!(state.keychain_read_error(), None);
    }

    // ---- Switching to a provider that needs no key ---------------------

    /// Ollama has no key to read. Asking the keychain anyway raised an
    /// unlock prompt on a locked keyring, and a failure the switch then
    /// reported over a provider that works with nothing stored.
    #[test]
    fn switching_to_a_keyless_provider_does_not_read_the_keychain() {
        let reads = Cell::new(0u32);
        let got = key_for_switch("ollama", |_| {
            reads.set(reads.get() + 1);
            Err(KeychainError::access_denied("locked"))
        });
        assert_eq!(got, Ok(None));
        assert_eq!(reads.get(), 0);
    }

    #[test]
    fn switching_to_a_provider_that_needs_a_key_reads_it() {
        let got = key_for_switch("openrouter", |id| {
            assert_eq!(id, "openrouter");
            Ok(Some("sk-or-1".to_string()))
        });
        assert_eq!(got, Ok(Some("sk-or-1".to_string())));

        let err = key_for_switch("openrouter", |_| {
            Err(KeychainError::access_denied("locked"))
        })
        .expect_err("a read failure is reported, not turned into no key");
        assert_eq!(err.reason(), "locked");
    }

    #[test]
    fn the_persistence_dto_says_why_it_is_not_persistent() {
        let durable: KeychainPersistenceDto = ai::keychain::Persistence::Durable.into();
        assert!(durable.persistent && durable.reason.is_none());
        let session: KeychainPersistenceDto = ai::keychain::Persistence::SessionOnly {
            reason: "no secret service".to_string(),
        }
        .into();
        assert!(!session.persistent);
        assert_eq!(session.reason.as_deref(), Some("no secret service"));
    }

    // ---- Reasoning effort setting -------------------------------------

    #[test]
    fn every_level_is_accepted_for_anthropic() {
        for level in ai::Effort::ALL {
            assert_eq!(
                parse_effort_setting("anthropic", Some(level.as_str())),
                Ok(Some(level))
            );
        }
        // Stray whitespace around a real level is the level.
        assert_eq!(
            parse_effort_setting("anthropic", Some("  xhigh ")),
            Ok(Some(ai::Effort::XHigh))
        );
    }

    /// "Default" in the UI: no value, or a blank one, clears the slot.
    /// Valid for every provider, so a stale slot can always be cleared.
    #[test]
    fn no_value_or_a_blank_one_means_the_models_default() {
        for id in ai::SUPPORTED_PROVIDER_IDS {
            assert_eq!(parse_effort_setting(id, None), Ok(None), "{id}");
            assert_eq!(parse_effort_setting(id, Some("")), Ok(None), "{id}");
            assert_eq!(parse_effort_setting(id, Some("  ")), Ok(None), "{id}");
        }
    }

    #[test]
    fn a_level_the_api_would_reject_is_refused_at_save_time() {
        for bad in ["extra high", "XHigh", "ultra", "0", "low;"] {
            let err = parse_effort_setting("anthropic", Some(bad)).unwrap_err();
            assert!(err.contains("unknown reasoning effort"), "{bad}: {err}");
            assert!(err.contains("xhigh"), "the error lists the levels: {err}");
        }
    }

    #[test]
    fn a_provider_with_no_such_setting_refuses_a_level() {
        for id in ai::SUPPORTED_PROVIDER_IDS
            .iter()
            .filter(|id| **id != "anthropic")
        {
            let err = parse_effort_setting(id, Some("high")).unwrap_err();
            assert!(err.contains("Anthropic only"), "{id}: {err}");
        }
    }

    // -----------------------------------------------------------------
    // Model catalogue
    // -----------------------------------------------------------------

    /// A saved key is the normal case: the form is empty once a key is
    /// saved, and a catalogue fetched with no key is a catalogue that
    /// cannot be live.
    #[test]
    fn the_catalogue_uses_the_saved_key_when_none_is_typed() {
        let saved = |p: &str| {
            assert_eq!(p, "anthropic", "the saved key of the provider on screen");
            Some("saved".to_string())
        };
        assert_eq!(
            super::catalogue_key("anthropic", None, saved).as_deref(),
            Some("saved")
        );
        assert_eq!(
            super::catalogue_key("anthropic", Some("  ".into()), saved).as_deref(),
            Some("saved"),
            "a blank field is no key"
        );
        assert_eq!(
            super::catalogue_key("anthropic", Some("t".into()), |_| {
                panic!("a typed key must not read the keychain")
            })
            .as_deref(),
            Some("t")
        );
        assert_eq!(
            super::catalogue_key("anthropic", Some(" t ".into()), |_| None).as_deref(),
            Some("t"),
            "the typed key is trimmed"
        );
        assert_eq!(super::catalogue_key("ollama", None, |_| None), None);
    }

    #[test]
    fn a_fallback_catalogue_serialises_with_its_notice() {
        let dto = super::ModelCatalogueDto::from(ai::Catalogue {
            models: vec![ai::ModelInfo {
                id: "claude-sonnet-5-5".into(),
                display_name: "Claude Sonnet 5.5 (default)".into(),
                context_length: Some(1_000_000),
                provider_hint: None,
            }],
            source: ai::CatalogueSource::Fallback {
                reason: "no API key saved".into(),
            },
        });
        assert_eq!(
            serde_json::to_value(&dto).unwrap(),
            serde_json::json!({
                "models": [{
                    "id": "claude-sonnet-5-5",
                    "display_name": "Claude Sonnet 5.5 (default)",
                    "context_length": 1_000_000,
                    "provider_hint": null,
                }],
                "source": "fallback",
                "notice": "no API key saved",
            })
        );
    }

    #[test]
    fn a_live_catalogue_has_no_notice() {
        let dto = super::ModelCatalogueDto::from(ai::Catalogue {
            models: vec![],
            source: ai::CatalogueSource::Live,
        });
        let json = serde_json::to_value(&dto).unwrap();
        assert_eq!(json["source"], "live");
        assert!(json["notice"].is_null());
    }

    // ---- The agent after a refused startup (#497) ----------------------

    /// A state an agent can be built in: the default project's store is
    /// open, as `run` leaves it. Without one nothing is ever built whatever
    /// the keychain says, and the tests below would prove nothing about it.
    fn state_with_a_store(dir: &std::path::Path) -> AppState {
        let state = AppState::default();
        let store = Store::open(dir).expect("store");
        state.set_open_project(Arc::new(Mutex::new(store)), dir.to_path_buf());
        state
    }

    /// An agent profile that pins `openai`'s model, active in `state`.
    fn pin_openai_profile(state: &AppState, dir: &std::path::Path) {
        std::fs::write(
            dir.join("pinned.md"),
            "---\ndescription: pins another provider\nmodel:\n  provider: openai\n  id: gpt-4o-mini\n---\nbody\n",
        )
        .expect("profile");
        state.install_agent_profile_library(dir.to_path_buf());
        state
            .set_active_agent_profile(Some("pinned".to_string()))
            .expect("select it");
    }

    /// What one rebuild asked the keychain for ("key:openai",
    /// "base_url:ollama", "effort:ollama"), and whether it built an agent.
    fn rebuild_and_record(state: &AppState) -> (Vec<String>, bool) {
        let asked = RefCell::new(Vec::new());
        let mut slot = None;
        rebuild_agent_from(
            state,
            &mut slot,
            |provider| {
                asked.borrow_mut().push(format!("key:{provider}"));
                Some("stored-key".to_string())
            },
            |provider| {
                asked.borrow_mut().push(format!("base_url:{provider}"));
                None
            },
            |provider| {
                asked.borrow_mut().push(format!("effort:{provider}"));
                None
            },
        );
        (asked.into_inner(), slot.is_some())
    }

    /// The bug: `rebuild_agent_into` reads the base URL and the level of
    /// whatever provider it builds for, and it runs at startup right after
    /// the reads. After a startup the keychain refused, a keyless active
    /// provider (a local Ollama) was therefore built, and read the
    /// keychain twice more, each an unlock prompt on a keyring the user had
    /// just declined to unlock.
    ///
    /// The agent is left unbuilt instead. It cannot be built from guesses:
    /// the stored URL may be a gateway's, and the built-in one is not.
    #[test]
    fn a_refused_startup_leaves_the_agent_unbuilt_and_the_keychain_unasked() {
        let dir = tempdir().unwrap();
        let state = state_with_a_store(dir.path());
        state.set_active_provider("ollama".to_string());
        state.set_keychain_refused(true);
        state.set_keychain_read_error(Some("locked".into()));

        let (asked, built) = rebuild_and_record(&state);

        assert!(asked.is_empty(), "the keychain was asked for {asked:?}");
        assert!(!built, "an agent built without its stored endpoint");
    }

    /// The same for a profile that pins another provider's model, which
    /// reads that provider's key as well.
    #[test]
    fn a_refused_startup_does_not_read_the_key_of_a_provider_a_profile_pins() {
        let dir = tempdir().unwrap();
        let agents = tempdir().unwrap();
        let state = state_with_a_store(dir.path());
        state.set_api_key_cache(Some("anthropic-key".to_string()));
        pin_openai_profile(&state, agents.path());
        state.set_keychain_refused(true);

        let (asked, built) = rebuild_and_record(&state);

        assert!(asked.is_empty(), "the keychain was asked for {asked:?}");
        assert!(!built);
    }

    /// And it is the refusal that does it, not the setup: the same states,
    /// readable, build an agent and read what they always read. Without
    /// this the two tests above would pass for a rebuild that never built
    /// anything.
    #[test]
    fn without_a_refusal_the_rebuild_reads_the_keychain_and_builds() {
        let dir = tempdir().unwrap();
        let state = state_with_a_store(dir.path());
        state.set_active_provider("ollama".to_string());
        let (asked, built) = rebuild_and_record(&state);
        assert_eq!(asked, ["base_url:ollama", "effort:ollama"]);
        assert!(built, "a keyless provider needs no key to be built");

        let agents = tempdir().unwrap();
        let state = state_with_a_store(dir.path());
        state.set_api_key_cache(Some("anthropic-key".to_string()));
        pin_openai_profile(&state, agents.path());
        let (asked, built) = rebuild_and_record(&state);
        assert_eq!(asked, ["key:openai", "base_url:openai", "effort:openai"]);
        assert!(built);

        // A failed read that was not a refusal raises no prompt to avoid,
        // so it does not stop the build either.
        let state = state_with_a_store(dir.path());
        state.set_active_provider("ollama".to_string());
        state.set_keychain_read_error(Some("no secret service".into()));
        assert!(rebuild_and_record(&state).1);
    }

    /// The refusal ends with the first save that goes through, and the
    /// rebuild after it reads and builds again.
    #[test]
    fn the_rebuild_after_a_lifted_refusal_builds_the_agent() {
        let dir = tempdir().unwrap();
        let state = state_with_a_store(dir.path());
        state.set_active_provider("ollama".to_string());
        state.set_keychain_refused(true);
        assert!(!rebuild_and_record(&state).1, "precondition");

        keychain_readable_again(&state, |_| Ok(None));

        let (asked, built) = rebuild_and_record(&state);
        assert_eq!(asked, ["base_url:ollama", "effort:ollama"]);
        assert!(built);
    }

    // ---- Plan first across a restart (#569) -----------------------------

    /// An agent built the way startup builds one: a state with a store, a
    /// keyless provider (so no key is needed) and the settings file at
    /// `settings`, read as `run` reads it.
    fn agent_built_at_launch(settings: &std::path::Path) -> ai::Agent {
        let project = tempdir().unwrap();
        let state = state_with_a_store(project.path());
        state.set_active_provider("ollama".to_string());
        state.install_settings(settings.to_path_buf());
        let mut slot = None;
        rebuild_agent_from(&state, &mut slot, |_| None, |_| None, |_| None);
        slot.expect("a keyless provider builds an agent")
    }

    /// The bug: Plan first is an approval gate, and it was off again after
    /// every restart. The first turn after one ran its edits with no card,
    /// and the composer's toggle gave no sign of it. The agent startup
    /// builds must already hold edits when the person had it on, without
    /// the toggle being touched.
    #[test]
    fn the_agent_built_at_launch_holds_edits_when_plan_first_was_left_on() {
        let home = tempdir().unwrap();
        let settings = home.path().join(".edytlab").join("settings.json");

        // The last launch: the person turns it on.
        let before = AppState::new();
        before.install_settings(settings.clone());
        before.save_plan_first(true).expect("saved");
        drop(before);

        assert!(agent_built_at_launch(&settings).plan_first());
    }

    /// And it is the file that does it: with nothing saved, the same
    /// startup builds an agent that acts directly, as a fresh install does.
    #[test]
    fn the_agent_built_at_launch_acts_directly_when_nothing_was_saved() {
        let home = tempdir().unwrap();
        let settings = home.path().join(".edytlab").join("settings.json");
        assert!(!agent_built_at_launch(&settings).plan_first());
    }

    /// A setting turned off again stays off after a restart.
    #[test]
    fn the_agent_built_at_launch_acts_directly_when_plan_first_was_turned_off() {
        let home = tempdir().unwrap();
        let settings = home.path().join(".edytlab").join("settings.json");
        let before = AppState::new();
        before.install_settings(settings.clone());
        before.save_plan_first(true).expect("saved");
        before.save_plan_first(false).expect("saved");
        drop(before);

        assert!(!agent_built_at_launch(&settings).plan_first());
    }

    /// Where a chat turn finds no agent, it says why. After a refused
    /// startup that is the keychain, and the person is told what undoes it,
    /// in words that still read as "no agent" to the frontend (which offers
    /// Settings for those).
    #[test]
    fn no_agent_after_a_refused_startup_says_the_keychain_refused() {
        let state = AppState::default();
        state.set_keychain_refused(true);
        state.set_keychain_read_error(Some("locked".into()));

        let text: String = no_agent_error(&state).into();

        assert!(
            text.starts_with("no agent configured")
                && text.contains("refused access")
                && text.contains("locked")
                && text.contains("Settings"),
            "{text}"
        );
    }

    /// Without a refusal it is the error it always was.
    #[test]
    fn no_agent_without_a_refusal_is_the_plain_error() {
        let state = AppState::default();
        state.set_keychain_read_error(Some("no secret service".into()));
        let text: String = no_agent_error(&state).into();
        assert_eq!(text, "no agent configured; call set_api_key first");
    }

    // ---- The save commands report where they landed (#497) -------------

    use ai::keychain::Persistence;

    fn fell_back(reason: &str) -> Persistence {
        Persistence::SessionOnly {
            reason: reason.to_string(),
        }
    }

    /// A keychain that answers each save as told, and records what was
    /// written. `session_only` names the saves that fall back to the kernel
    /// keyring ("api_key", "active_provider", "base_url", "effort",
    /// "model"), `failing` the ones that fail outright. The reason a
    /// fallback gives is `no secret service for <what>`, so a test can tell
    /// whose it got.
    #[derive(Default)]
    struct FakeKeychain {
        session_only: Vec<&'static str>,
        failing: Vec<&'static str>,
        wrote: Mutex<Vec<String>>,
    }

    impl FakeKeychain {
        fn saving_session_only(what: &[&'static str]) -> Self {
            Self {
                session_only: what.to_vec(),
                ..Self::default()
            }
        }

        fn failing(what: &[&'static str]) -> Self {
            Self {
                failing: what.to_vec(),
                ..Self::default()
            }
        }

        fn save(&self, what: &'static str) -> Result<Persistence, keyring::Error> {
            self.wrote.lock().unwrap().push(format!("save {what}"));
            if self.failing.contains(&what) {
                return Err(keyring::Error::PlatformFailure("the store is gone".into()));
            }
            Ok(if self.session_only.contains(&what) {
                fell_back(&format!("no secret service for {what}"))
            } else {
                Persistence::Durable
            })
        }

        fn clear(&self, what: &'static str) -> Result<(), keyring::Error> {
            self.wrote.lock().unwrap().push(format!("clear {what}"));
            Ok(())
        }

        fn wrote(&self) -> Vec<String> {
            self.wrote.lock().unwrap().clone()
        }
    }

    impl KeychainWrites for FakeKeychain {
        fn save_api_key(&self, _: &str, _: &str) -> Result<Persistence, keyring::Error> {
            self.save("api_key")
        }
        fn save_active_provider(&self, _: &str) -> Result<Persistence, keyring::Error> {
            self.save("active_provider")
        }
        fn save_base_url(&self, _: &str, _: &str) -> Result<Persistence, keyring::Error> {
            self.save("base_url")
        }
        fn delete_base_url(&self, _: &str) -> Result<(), keyring::Error> {
            self.clear("base_url")
        }
        fn save_effort(&self, _: &str, _: ai::Effort) -> Result<Persistence, keyring::Error> {
            self.save("effort")
        }
        fn delete_effort(&self, _: &str) -> Result<(), keyring::Error> {
            self.clear("effort")
        }
        fn save_model(&self, _: &str, _: &str) -> Result<Persistence, keyring::Error> {
            self.save("model")
        }
    }

    /// What a command answered, reduced to what the frontend reads.
    fn landing(answer: CmdResult<KeychainPersistenceDto>) -> (bool, Option<String>) {
        let dto = answer.expect("the command succeeded");
        (dto.persistent, dto.reason)
    }

    fn durable() -> (bool, Option<String>) {
        (true, None)
    }

    fn session_only(reason: &str) -> (bool, Option<String>) {
        (false, Some(reason.to_string()))
    }

    /// Saves taken together: durable when nothing fell back (and for a
    /// command that saved nothing), else session-only with the reason of
    /// the first that did.
    #[test]
    fn saves_taken_together_are_durable_unless_one_fell_back() {
        let durable_dto = |saves: Vec<Persistence>| {
            let dto = KeychainPersistenceDto::landed(saves);
            (dto.persistent, dto.reason)
        };
        assert_eq!(durable_dto(vec![]), durable());
        assert_eq!(durable_dto(vec![Persistence::Durable]), durable());
        assert_eq!(
            durable_dto(vec![Persistence::Durable, Persistence::Durable]),
            durable()
        );
        assert_eq!(
            durable_dto(vec![Persistence::Durable, fell_back("second")]),
            session_only("second")
        );
        assert_eq!(
            durable_dto(vec![fell_back("first"), Persistence::Durable]),
            session_only("first")
        );
        assert_eq!(
            durable_dto(vec![fell_back("first"), fell_back("second")]),
            session_only("first"),
            "the first reason, not the last"
        );
    }

    /// What the frontend's `KeychainPersistence` reads, the same shape
    /// `get_keychain_persistence` answers with.
    #[test]
    fn a_landing_serialises_as_persistent_and_reason() {
        assert_eq!(
            serde_json::to_value(KeychainPersistenceDto::landed([fell_back("no bus")])).unwrap(),
            serde_json::json!({ "persistent": false, "reason": "no bus" })
        );
        assert_eq!(
            serde_json::to_value(KeychainPersistenceDto::landed([])).unwrap(),
            serde_json::json!({ "persistent": true, "reason": null })
        );
    }

    /// The bug: the `save_*` functions return where a value landed and no
    /// command passed it on, so a save that fell back to the kernel keyring
    /// (a Secret Service that went away after launch) looked like any other.
    /// A key and the active provider are two writes; either falling back
    /// makes the save session-only.
    #[test]
    fn saving_a_key_reports_where_it_landed() {
        let save = |keychain: &FakeKeychain| {
            let state = AppState::default();
            let answer = tauri::async_runtime::block_on(set_api_key_for_inner(
                &state, "openai", "sk-test", keychain,
            ));
            (landing(answer), state.api_key_snapshot())
        };

        let (answer, cached) = save(&FakeKeychain::default());
        assert_eq!(answer, durable());
        assert_eq!(cached.as_deref(), Some("sk-test"));

        let (answer, cached) = save(&FakeKeychain::saving_session_only(&["api_key"]));
        assert_eq!(answer, session_only("no secret service for api_key"));
        assert_eq!(
            cached.as_deref(),
            Some("sk-test"),
            "the key is saved and used all the same"
        );

        let (answer, _) = save(&FakeKeychain::saving_session_only(&["active_provider"]));
        assert_eq!(
            answer,
            session_only("no secret service for active_provider")
        );

        let (answer, _) = save(&FakeKeychain::saving_session_only(&[
            "active_provider",
            "api_key",
        ]));
        assert_eq!(
            answer,
            session_only("no secret service for api_key"),
            "the key is written first, so its reason leads"
        );
    }

    /// A save that fails is still an error, with nothing reported.
    #[test]
    fn a_failed_save_is_still_an_error() {
        let state = AppState::default();
        let keychain = FakeKeychain::failing(&["api_key"]);
        let err = tauri::async_runtime::block_on(set_api_key_for_inner(
            &state, "openai", "sk-test", &keychain,
        ))
        .expect_err("the save failed");
        assert!(err.contains("keychain error"), "{err}");
        assert_eq!(keychain.wrote(), ["save api_key"], "and went no further");
        assert_eq!(state.api_key_snapshot(), None);
    }

    #[test]
    fn saving_a_base_url_reports_where_it_landed() {
        let state = AppState::default();
        let run = |keychain: &FakeKeychain, url: &str| {
            tauri::async_runtime::block_on(set_base_url_for_with(&state, "ollama", url, keychain))
        };

        assert_eq!(
            landing(run(&FakeKeychain::default(), "http://localhost:1234/v1")),
            durable()
        );
        let fallen = FakeKeychain::saving_session_only(&["base_url"]);
        assert_eq!(
            landing(run(&fallen, "http://localhost:1234/v1/")),
            session_only("no secret service for base_url")
        );

        // Clearing the override writes nothing, so there is nothing that
        // could be lost: durable, whatever the service is doing.
        assert_eq!(landing(run(&fallen, "  ")), durable());
        assert_eq!(fallen.wrote(), ["save base_url", "clear base_url"]);

        // A URL that is refused is refused before anything is written.
        let untouched = FakeKeychain::saving_session_only(&["base_url"]);
        assert!(run(&untouched, "localhost:1234").is_err());
        assert!(untouched.wrote().is_empty());
    }

    #[test]
    fn saving_an_effort_reports_where_it_landed() {
        let state = AppState::default();
        let run = |keychain: &FakeKeychain, level: Option<&str>| {
            tauri::async_runtime::block_on(set_effort_for_with(
                &state,
                "anthropic",
                level,
                keychain,
            ))
        };

        assert_eq!(
            landing(run(&FakeKeychain::default(), Some("high"))),
            durable()
        );
        let fallen = FakeKeychain::saving_session_only(&["effort"]);
        assert_eq!(
            landing(run(&fallen, Some("xhigh"))),
            session_only("no secret service for effort")
        );
        // "Default" clears the slot: nothing written, nothing to report.
        assert_eq!(landing(run(&fallen, None)), durable());
        assert_eq!(fallen.wrote(), ["save effort", "clear effort"]);

        // A level the provider cannot take is refused before any write.
        let untouched = FakeKeychain::saving_session_only(&["effort"]);
        assert!(tauri::async_runtime::block_on(set_effort_for_with(
            &state,
            "openai",
            Some("high"),
            &untouched,
        ))
        .is_err());
        assert!(untouched.wrote().is_empty());
    }

    #[test]
    fn switching_provider_reports_where_the_choice_landed() {
        let run = |keychain: &FakeKeychain, provider: &str| {
            let state = AppState::default();
            tauri::async_runtime::block_on(set_active_provider_with(
                &state,
                provider,
                keychain,
                |_| panic!("a provider that needs no key reads none"),
            ))
        };

        assert_eq!(landing(run(&FakeKeychain::default(), "ollama")), durable());
        assert_eq!(
            landing(run(
                &FakeKeychain::saving_session_only(&["active_provider"]),
                "ollama"
            )),
            session_only("no secret service for active_provider")
        );

        let untouched = FakeKeychain::saving_session_only(&["active_provider"]);
        assert!(run(&untouched, "no-such-provider").is_err());
        assert!(untouched.wrote().is_empty());
    }

    #[test]
    fn choosing_a_model_reports_where_the_choice_landed() {
        let state = AppState::default();
        let run = |keychain: &FakeKeychain| {
            tauri::async_runtime::block_on(set_active_model_with(
                &state,
                "anthropic".to_string(),
                " claude-opus-5 ".to_string(),
                keychain,
            ))
        };

        assert_eq!(landing(run(&FakeKeychain::default())), durable());
        assert_eq!(
            landing(run(&FakeKeychain::saving_session_only(&["model"]))),
            session_only("no secret service for model")
        );
        assert_eq!(
            state.model_for("anthropic").as_deref(),
            Some("claude-opus-5"),
            "chosen for this session either way"
        );

        // A failed write keeps its own, older, message.
        let err = run(&FakeKeychain::failing(&["model"])).expect_err("not saved");
        assert!(
            err.starts_with("model selected for this session, but could not be saved"),
            "{err}"
        );
    }

    // -- ml_runtime_status (#383) ------------------------------------------

    fn runtime_json(
        answer: ml_pipeline::Result<ml_pipeline::runtime::RuntimeInfo>,
    ) -> serde_json::Value {
        serde_json::to_value(ml_runtime_status_dto(answer)).expect("serialises")
    }

    fn found(
        origin: ml_pipeline::runtime::Origin,
    ) -> ml_pipeline::Result<ml_pipeline::runtime::RuntimeInfo> {
        Ok(ml_pipeline::runtime::RuntimeInfo {
            path: std::path::PathBuf::from("/opt/edytlab/lib/libonnxruntime.so"),
            version: "1.23.2".to_string(),
            origin,
        })
    }

    /// The window reads these by name, so the wire shape is the contract:
    /// `mlRuntimeStatus` in `tauri-bridge.ts` types it.
    #[test]
    fn a_found_runtime_is_reported_with_its_version_path_and_source() {
        use ml_pipeline::runtime::Origin;

        assert_eq!(
            runtime_json(found(Origin::Bundle)),
            serde_json::json!({
                "state": "found",
                "version": "1.23.2",
                "path": "/opt/edytlab/lib/libonnxruntime.so",
                "source": "bundled",
            })
        );
        assert_eq!(
            runtime_json(found(Origin::Environment))["source"],
            "environment"
        );
        // Beside the executable is the installer's only on Windows.
        let expected = if cfg!(windows) {
            "bundled"
        } else {
            "executable"
        };
        assert_eq!(runtime_json(found(Origin::Executable))["source"], expected);
    }

    #[test]
    fn a_runtime_that_is_not_there_lists_where_it_looked() {
        let answer = Err(ml_pipeline::Error::MissingRuntime {
            searched: vec![
                std::path::PathBuf::from("/usr/lib/edytlab/libonnxruntime.so"),
                std::path::PathBuf::from("/usr/bin/libonnxruntime.so"),
            ],
        });
        assert_eq!(
            runtime_json(answer),
            serde_json::json!({
                "state": "missing",
                "searched": [
                    "/usr/lib/edytlab/libonnxruntime.so",
                    "/usr/bin/libonnxruntime.so",
                ],
            })
        );
    }

    #[test]
    fn a_macos_that_is_too_old_is_reported_with_both_versions() {
        let answer = Err(ml_pipeline::Error::RuntimeUnsupported {
            needs: "13.4".to_string(),
            running: "12.6.1".to_string(),
        });
        assert_eq!(
            runtime_json(answer),
            serde_json::json!({
                "state": "unsupportedOs",
                "needs": "13.4",
                "running": "12.6.1",
            })
        );
    }

    #[test]
    fn a_runtime_that_fails_to_load_is_reported_with_the_file_and_the_reason() {
        let answer = Err(ml_pipeline::Error::RuntimeLoad {
            path: std::path::PathBuf::from("/opt/edytlab/lib/libonnxruntime.so"),
            reason: "it has no OrtGetApiBase entry point".to_string(),
        });
        assert_eq!(
            runtime_json(answer),
            serde_json::json!({
                "state": "loadFailed",
                "path": "/opt/edytlab/lib/libonnxruntime.so",
                "reason": "it has no OrtGetApiBase entry point",
            })
        );
        // An error that names no file is still shown, with no path.
        let other = runtime_json(Err(ml_pipeline::Error::Cancelled));
        assert_eq!(other["state"], "loadFailed");
        assert_eq!(other["path"], serde_json::Value::Null);
        assert!(other["reason"].as_str().is_some_and(|r| !r.is_empty()));
    }
}
