//! Integration test exercising the Tauri command surface through
//! `tauri::test::mock_builder`.
//!
//! Why a separate file: the unit tests in `src/commands.rs` cover the
//! pure helpers (state mutation, error mapping, store round-trips). The
//! goal here is the IPC plumbing — the `#[tauri::command]` attribute,
//! the handler registration in `generate_handler!`, and the
//! `State<AppState>` injection. We pick `open_project` as the
//! happy-path smoke because (a) it doesn't need the OS keychain or the
//! network, (b) it touches both arms of the State (writes the store,
//! writes the project_dir), and (c) its return value is the
//! `ProjectInfo` struct whose serialised shape is the contract with
//! the TS bridge.
//!
//! Windows: this entire file is gated `cfg(not(target_os = "windows"))`.
//! Calling `mock_builder` and `WebviewWindowBuilder::new(...).build()`
//! pulls in Wry, which statically imports symbols from a WebView2 DLL
//! whose runtime version on the GitHub `windows-latest` image doesn't
//! export them — the test binary fails to load with
//! `STATUS_ENTRYPOINT_NOT_FOUND` (`0xc0000139`) before any test code or
//! `#[ignore]` check runs, so a function-level ignore is too late. The
//! macOS CI job exercises the same IPC path, and `src/commands.rs` unit
//! tests cover the command logic itself, so the Windows skip preserves
//! coverage. To run locally on a Windows host with the matching WebView2
//! runtime installed, drop the `cfg(...)` gate.

#![cfg(not(target_os = "windows"))]

use edytlab_desktop_lib::commands;
use edytlab_desktop_lib::state::AppState;
use serde_json::json;
use tauri::ipc::{CallbackFn, InvokeBody};
use tauri::test::{assert_ipc_response, mock_builder, mock_context, noop_assets, INVOKE_KEY};
use tauri::webview::InvokeRequest;
use tauri::{Manager, WebviewWindowBuilder};

fn make_request(cmd: &str, body: serde_json::Value) -> InvokeRequest {
    InvokeRequest {
        cmd: cmd.into(),
        callback: CallbackFn(0),
        error: CallbackFn(1),
        url: if cfg!(any(windows, target_os = "android")) {
            "http://tauri.localhost"
        } else {
            "tauri://localhost"
        }
        .parse()
        .unwrap(),
        body: InvokeBody::Json(body),
        headers: Default::default(),
        invoke_key: INVOKE_KEY.to_string(),
    }
}

#[test]
fn open_project_via_ipc_returns_project_info() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project_path = tmp.path().to_str().expect("utf-8 path").to_string();

    let app = mock_builder()
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            commands::open_project,
            commands::send_message,
            commands::set_api_key,
            commands::get_session_head,
            commands::get_node,
            commands::render_preview,
        ])
        .build(mock_context(noop_assets()))
        .expect("mock app");

    let webview = WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .expect("webview");

    // Happy path: open a fresh project, expect ProjectInfo with the
    // path we passed and head=null.
    assert_ipc_response(
        &webview,
        make_request("open_project", json!({ "path": project_path })),
        Ok(json!({ "path": project_path, "head": null })),
    );
}

/// Opening a project removes derived audio no node names (#98) — here, a
/// leftover in a project with no history — through the real command.
#[test]
fn opening_a_project_removes_audio_no_node_names() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project_path = tmp.path().to_str().expect("utf-8 path").to_string();
    let derived = tmp.path().join(".audiograph").join("derived");
    std::fs::create_dir_all(&derived).expect("derived dir");
    let leftover = derived.join("left-by-a-failed-edit.wav");
    std::fs::write(&leftover, b"RIFF").expect("leftover");

    let app = mock_builder()
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![commands::open_project])
        .build(mock_context(noop_assets()))
        .expect("mock app");
    let webview = WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .expect("webview");

    assert_ipc_response(
        &webview,
        make_request("open_project", json!({ "path": project_path })),
        Ok(json!({ "path": project_path, "head": null })),
    );
    assert!(
        !leftover.exists(),
        "the orphan is gone once the project is open"
    );
}

#[test]
fn get_session_head_returns_error_when_no_project_open() {
    let app = mock_builder()
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            commands::open_project,
            commands::send_message,
            commands::set_api_key,
            commands::get_session_head,
            commands::get_node,
            commands::render_preview,
        ])
        .build(mock_context(noop_assets()))
        .expect("mock app");

    let webview = WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .expect("webview");

    // No project opened, so the command should return an error string
    // mentioning the missing session.
    let response =
        tauri::test::get_ipc_response(&webview, make_request("get_session_head", json!({})));

    let err = response.expect_err("expected error response, got Ok");
    let err_str = err.as_str().expect("error payload is a string");
    assert!(
        err_str.contains("no session loaded"),
        "unexpected error string: {err_str}"
    );
}

/// The active provider as the frontend asks for it, with `stash` as what
/// startup found wrong with the keychain (`None` for a clean start).
fn active_provider_over_ipc(stash: Option<&str>) -> Result<String, String> {
    let state = AppState::new();
    state.set_keychain_read_error(stash.map(str::to_string));
    let app = mock_builder()
        .manage(state)
        .invoke_handler(tauri::generate_handler![commands::get_active_provider])
        .build(mock_context(noop_assets()))
        .expect("mock app");
    let webview = WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .expect("webview");
    tauri::test::get_ipc_response(&webview, make_request("get_active_provider", json!({})))
        .map(|r| r.deserialize::<String>().expect("a string"))
        .map_err(|e| e.as_str().unwrap_or_default().to_string())
}

/// #497 item 1. After a startup the keychain refused, the active provider
/// was never read, so `anthropic` is the default and not an answer. Settings
/// takes it for the stored choice, and its Save writes it back over the
/// real one.
#[test]
fn the_active_provider_is_refused_while_the_keychain_could_not_be_read() {
    assert_eq!(
        active_provider_over_ipc(Some("locked")),
        Err("locked".to_string()),
        "the reason alone, as `has_api_key` gives it: the banner supplies the rest"
    );
}

/// And the same command on a clean start still answers: the default is
/// an answer when startup read cleanly and found no preference.
#[test]
fn the_active_provider_is_answered_when_the_keychain_was_read() {
    assert_eq!(active_provider_over_ipc(None), Ok("anthropic".to_string()));
}

/// A command over IPC, answered from `state`: what the frontend sees.
fn ipc(state: AppState, cmd: &str, body: serde_json::Value) -> Result<serde_json::Value, String> {
    let app = mock_builder()
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            commands::get_active_model,
            commands::get_base_url_for,
            commands::get_effort_for,
            commands::list_models_for,
        ])
        .build(mock_context(noop_assets()))
        .expect("mock app");
    let webview = WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .expect("webview");
    tauri::test::get_ipc_response(&webview, make_request(cmd, body))
        .map(|r| r.deserialize::<serde_json::Value>().expect("json"))
        .map_err(|e| e.as_str().unwrap_or_default().to_string())
}

/// What startup leaves when the keychain refused access: the flag, and
/// the stash the reads kept if they got as far as one. A refusal at the
/// models alone leaves no stash.
fn refused_state(stash: Option<&str>) -> AppState {
    let state = AppState::new();
    state.set_keychain_refused(true);
    state.set_keychain_read_error(stash.map(str::to_string));
    state
}

/// #497, the model field. After a startup the keychain refused, the
/// chosen model was never restored, so "nothing chosen" is not what the
/// map says, it is what the map lacks. Settings shows the answer in its
/// model field and its Save writes the field back over the stored model.
#[test]
fn the_active_model_is_not_answered_as_none_chosen_after_a_refused_startup() {
    for stash in [Some("locked"), None] {
        let err = ipc(
            refused_state(stash),
            "get_active_model",
            json!({ "provider": "anthropic" }),
        )
        .expect_err("what was never read is not an empty answer");
        assert!(err.contains("refused access"), "says why: {err}");
        if let Some(reason) = stash {
            assert!(err.contains(reason), "and the reason it kept: {err}");
        }
    }
}

/// A model that is there is an answer whatever startup found: it was
/// restored before the refusal, or chosen in this session since.
#[test]
fn a_model_that_is_known_is_answered_after_a_refused_startup() {
    let state = refused_state(Some("locked"));
    state.set_model_for("anthropic".into(), "claude-opus-5".into());
    assert_eq!(
        ipc(
            state,
            "get_active_model",
            json!({ "provider": "anthropic" })
        ),
        Ok(json!("claude-opus-5"))
    );
}

/// And after a clean start, "no model chosen" is an answer: the empty
/// string Settings has always read it as.
#[test]
fn no_model_chosen_is_answered_empty_after_a_clean_startup() {
    assert_eq!(
        ipc(
            AppState::new(),
            "get_active_model",
            json!({ "provider": "anthropic" })
        ),
        Ok(json!(""))
    );
}

/// #497, the unlock prompts. Opening Settings reads the stored base URL,
/// the level, and (for the model list) the key and the base URL again.
/// After a startup the keychain refused, each is another unlock prompt
/// the moment Settings opens. They are refused up front, with the
/// reason, and the keychain is not asked.
#[test]
fn settings_reads_after_a_refused_startup_do_not_ask_the_keychain_again() {
    for stash in [Some("locked"), None] {
        for (cmd, body) in [
            ("get_base_url_for", json!({ "provider": "anthropic" })),
            ("get_effort_for", json!({ "provider": "anthropic" })),
            ("list_models_for", json!({ "provider": "anthropic" })),
        ] {
            let err = ipc(refused_state(stash), cmd, body)
                .expect_err(&format!("{cmd} must not read a keychain that refused"));
            assert!(
                err.contains("refused access") && err.contains("not asked again"),
                "{cmd}: {err}"
            );
        }
    }
}

/// #497. After a startup the keychain refused, the agent is left unbuilt
/// (it cannot be built without reading the keychain again), and a chat
/// message says why instead of "call set_api_key first", which sends the
/// person to a key they may already have saved.
#[test]
fn a_message_after_a_refused_startup_says_the_keychain_refused() {
    let app = mock_builder()
        .manage(refused_state(Some("locked")))
        .invoke_handler(tauri::generate_handler![commands::send_message])
        .build(mock_context(noop_assets()))
        .expect("mock app");
    let webview = WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .expect("webview");

    let err = tauri::test::get_ipc_response(
        &webview,
        make_request(
            "send_message",
            json!({ "text": "trim the silence", "disabledTools": [] }),
        ),
    )
    .expect_err("there is no agent to answer");
    let err = err.as_str().expect("a string").to_string();

    assert!(
        err.starts_with("no agent configured")
            && err.contains("refused access")
            && err.contains("locked"),
        "{err}"
    );
}

/// How long the reads are held in these tests. The checks are one-sided
/// (a call cannot answer before they are released), so a slow machine can
/// only make them wait longer, never fail.
const HELD_FOR: std::time::Duration = std::time::Duration::from_millis(400);

/// Call `cmd` while startup's keychain reads are still going, release them
/// [`HELD_FOR`] later, and report how long the call took and what it
/// answered. The stash makes the answers that depend on it "locked"
/// without touching the real keychain.
fn call_while_startup_reads_are_pending(
    cmd: &str,
    body: serde_json::Value,
) -> (std::time::Duration, Result<serde_json::Value, String>) {
    let state = AppState::new();
    state.set_keychain_read_error(Some("locked".into()));
    state.begin_startup_reads();
    let reads = state.startup_reads_guard();
    let app = mock_builder()
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            commands::has_api_key,
            commands::get_active_provider,
            commands::get_active_model,
        ])
        .build(mock_context(noop_assets()))
        .expect("mock app");
    let webview = WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .expect("webview");

    let releaser = std::thread::spawn(move || {
        std::thread::sleep(HELD_FOR);
        drop(reads);
    });
    let started = std::time::Instant::now();
    let answer = tauri::test::get_ipc_response(&webview, make_request(cmd, body))
        .map(|r| r.deserialize::<serde_json::Value>().expect("json"))
        .map_err(|e| e.as_str().unwrap_or_default().to_string());
    let waited = started.elapsed();
    releaser.join().expect("releaser");
    (waited, answer)
}

/// Not answered before the reads were released. Allows for the time it
/// took to get from starting the releaser to making the call.
fn assert_held(waited: std::time::Duration) {
    assert!(
        waited >= HELD_FOR / 2,
        "answered after {waited:?}, before the reads were done"
    );
}

/// #497 item 5. The keychain reads run off the setup thread, so the window
/// is up while they wait on the user. `has_api_key`, which the frontend
/// calls on mount, stays pending until they finish (the frontend shows
/// "waiting for your keyring" meanwhile), and then answers from what they
/// found.
#[test]
fn has_api_key_is_pending_until_the_startup_reads_finish() {
    let (waited, answer) = call_while_startup_reads_are_pending("has_api_key", json!({}));
    assert_held(waited);
    assert_eq!(answer, Err("locked".to_string()), "then it answers");
}

/// The default provider is not an answer while it is still being read,
/// and neither is "no model chosen yet", once it is known why none is.
#[test]
fn the_active_provider_and_model_wait_for_the_startup_reads_too() {
    let (waited, answer) = call_while_startup_reads_are_pending("get_active_provider", json!({}));
    assert_held(waited);
    assert_eq!(answer, Err("locked".to_string()));

    let (waited, answer) = call_while_startup_reads_are_pending(
        "get_active_model",
        json!({ "provider": "anthropic" }),
    );
    assert_held(waited);
    // The stash is "locked", so no model is known for lack of a read, and
    // that is what it says rather than "none chosen".
    let reason = answer.expect_err("the models were never read");
    assert!(reason.contains("locked"), "{reason}");
}

/// #497 item 4. A restart from Settings after a startup the keychain
/// refused would read the keychain for each reference the server has, and
/// each is an unlock prompt. The server is not started, says why, and
/// Settings' list shows it as an error with that reason.
#[test]
fn restarting_a_keychain_server_after_a_refused_startup_is_refused_with_a_reason() {
    let state = AppState::new();
    state.mcp_config.lock().unwrap().servers.insert(
        "github".into(),
        mcp::McpServerConfig::Stdio {
            command: "/nonexistent/edytlab-test-no-such-command".into(),
            args: vec![],
            env: [(
                "GITHUB_TOKEN".to_string(),
                "<keychain:github_token>".to_string(),
            )]
            .into(),
            enabled: true,
        },
    );
    state.set_keychain_refused(true);
    state.set_keychain_read_error(Some("locked".into()));
    let app = mock_builder()
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            commands::restart_mcp_server,
            commands::list_mcp_servers,
        ])
        .build(mock_context(noop_assets()))
        .expect("mock app");
    let webview = WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .expect("webview");

    let err = tauri::test::get_ipc_response(
        &webview,
        make_request("restart_mcp_server", json!({ "id": "github" })),
    )
    .expect_err("refused");
    let err = err.as_str().expect("a string").to_string();
    assert!(
        err.contains("refused access") && err.contains("locked"),
        "says why: {err}"
    );

    let list = tauri::test::get_ipc_response(&webview, make_request("list_mcp_servers", json!({})))
        .expect("list")
        .deserialize::<serde_json::Value>()
        .expect("json");
    assert_eq!(list[0]["id"], "github");
    assert_eq!(list[0]["status"], "error", "the Settings badge");
    assert!(
        list[0]["last_error"]
            .as_str()
            .unwrap_or_default()
            .contains("refused access"),
        "{list}"
    );
}

/// A mono 16-bit WAV of `frames` frames at 8 kHz, every sample 1000.
fn write_wav(path: &std::path::Path, frames: u32) {
    let data_len = frames * 2;
    let mut bytes = Vec::with_capacity(44 + data_len as usize);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes()); // PCM
    bytes.extend_from_slice(&1u16.to_le_bytes()); // mono
    bytes.extend_from_slice(&8_000u32.to_le_bytes());
    bytes.extend_from_slice(&16_000u32.to_le_bytes()); // byte rate
    bytes.extend_from_slice(&2u16.to_le_bytes()); // block align
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_len.to_le_bytes());
    for _ in 0..frames {
        bytes.extend_from_slice(&1_000i16.to_le_bytes());
    }
    std::fs::write(path, bytes).expect("write wav");
}

/// `list_tracks` hands the lane its audio on the session's axis (#348).
///
/// Straight after a load the clip is its whole source at zero, and the
/// source is the file. Once the clip is moved, the source would draw its
/// first second under the ruler's first second — so the lane gets a
/// file that is silent until the clip starts instead.
#[test]
fn list_tracks_hands_the_lane_audio_on_the_session_axis() {
    let project = tempfile::tempdir().expect("tempdir");
    let media = tempfile::tempdir().expect("tempdir");
    let take = media.path().join("take.wav");
    write_wav(&take, 8_000);

    let app = mock_builder()
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            commands::open_project,
            commands::batch_load,
            commands::move_clip,
            commands::list_tracks,
        ])
        .build(mock_context(noop_assets()))
        .expect("mock app");
    let webview = WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .expect("webview");
    let call = |cmd: &str, body: serde_json::Value| {
        tauri::test::get_ipc_response(&webview, make_request(cmd, body))
            .unwrap_or_else(|e| panic!("{cmd} failed: {e:?}"))
            .deserialize::<serde_json::Value>()
            .expect("json")
    };
    let lane_path = |tracks: serde_json::Value| -> std::path::PathBuf {
        tracks[0]["audio_path"]
            .as_str()
            .expect("the track has audio")
            .into()
    };

    call("open_project", json!({ "path": project.path() }));
    call("batch_load", json!({ "paths": [take] }));
    assert_eq!(
        lane_path(call("list_tracks", json!({}))),
        take,
        "a file just loaded is its own lane"
    );

    call(
        "move_clip",
        json!({ "track": 0, "clip": 0, "startSec": 0.5 }),
    );
    let moved = lane_path(call("list_tracks", json!({})));
    assert_ne!(moved, take, "the source would start at 0:00, not 0:00.5");

    let bytes = std::fs::read(&moved).expect("the lane's file exists");
    let samples: Vec<i16> = bytes[44..]
        .chunks_exact(2)
        .map(|b| i16::from_le_bytes([b[0], b[1]]))
        .collect();
    assert_eq!(
        samples.len(),
        12_000,
        "half a second of lead-in, then the take"
    );
    assert!(
        samples[..4_000].iter().all(|&s| s == 0),
        "silent until 0.5 s"
    );
    assert!(
        samples[4_000..].iter().all(|&s| s == 1_000),
        "then the take"
    );
}

/// A project whose history holds destructive edits, opened through IPC.
///
/// The edits go through the tools crate as the agent's do; the app then
/// opens the project the way it does at launch. Returns the node ids,
/// oldest first — the load, then each edit.
struct SweptProject {
    dir: tempfile::TempDir,
    nodes: Vec<session::NodeId>,
    webview: tauri::WebviewWindow<tauri::test::MockRuntime>,
    app: tauri::App<tauri::test::MockRuntime>,
}

impl SweptProject {
    fn open() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let take = dir.path().join("take.wav");
        write_wav(&take, 16_000);
        let nodes = {
            let mut store = session::Store::open(dir.path()).expect("store");
            let mut engine = audio_engine::Engine::new();
            let mut clipboard = None;
            let dispatcher = tools::ToolDispatcher::default_dispatcher();
            let mut ctx = tools::ToolContext {
                store: &mut store,
                engine: &mut engine,
                user_message: "",
                clipboard: &mut clipboard,
                allowed_tools: None,
            };
            let mut nodes = Vec::new();
            let mut run = |tool: &str, args: serde_json::Value, ctx: &mut tools::ToolContext| {
                match dispatcher.invoke(tool, args, ctx).expect("invoke") {
                    tools::ToolResult::Ok(_) => nodes.push(ctx.store.head().expect("head")),
                    tools::ToolResult::Error(m) => panic!("{tool} failed: {m}"),
                }
            };
            run("load", json!({ "path": take }), &mut ctx);
            for start in [0.0, 0.5, 1.0] {
                run(
                    "silence_region",
                    json!({ "track": 0, "start_sec": start, "end_sec": start + 0.2 }),
                    &mut ctx,
                );
            }
            nodes
        };

        let app = mock_builder()
            .manage(AppState::new())
            .invoke_handler(tauri::generate_handler![
                commands::open_project,
                commands::get_session_head,
                commands::set_head_to,
                commands::accept_b,
                commands::render_preview,
                commands::prepare_compare,
                commands::render_range,
            ])
            .build(mock_context(noop_assets()))
            .expect("mock app");
        let webview = WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .expect("webview");
        let project = Self {
            dir,
            nodes,
            webview,
            app,
        };
        project
            .call("open_project", json!({ "path": project.dir.path() }))
            .expect("open_project");
        project
    }

    fn call(&self, cmd: &str, body: serde_json::Value) -> Result<serde_json::Value, String> {
        tauri::test::get_ipc_response(&self.webview, make_request(cmd, body))
            .map(|r| r.deserialize::<serde_json::Value>().expect("json"))
            .map_err(|e| e.as_str().unwrap_or_default().to_string())
    }

    /// Run the history sweep at cap 0, as a full disk would, and check it
    /// took `node`'s audio — otherwise the test that follows checks
    /// nothing.
    fn sweep_away(&self, node: session::NodeId) {
        let store = session::Store::open(self.dir.path()).expect("store");
        tools::reclaim::sweep(&store, 0).expect("sweep");
        assert!(
            !tools::rederive::missing_paths(&store, node).is_empty(),
            "the sweep removed the node's audio"
        );
    }

    fn assert_back(&self, node: session::NodeId) {
        let store = session::Store::open(self.dir.path()).expect("store");
        assert_eq!(
            tools::rederive::missing_paths(&store, node),
            Vec::<std::path::PathBuf>::new(),
            "the node's audio is back on disk"
        );
    }

    fn head(&self) -> String {
        self.call("get_session_head", json!({}))
            .expect("head")
            .as_str()
            .expect("hex")
            .to_string()
    }
}

/// Undo, redo and the history view all move the head with `set_head_to`.
/// The head's audio is always on disk, so moving to a node whose audio
/// was swept puts it back first (#98).
#[test]
fn moving_the_head_to_a_swept_node_puts_its_audio_back() {
    let p = SweptProject::open();
    let parent = p.nodes[2];
    p.sweep_away(parent);

    assert_eq!(
        p.call("set_head_to", json!({ "nodeId": parent.to_hex() })),
        Ok(json!(parent.to_hex()))
    );
    p.assert_back(parent);
}

/// And when it cannot, the head stays where it was: a head whose audio
/// is gone cannot play.
#[test]
fn a_head_move_that_cannot_rebuild_is_refused() {
    let p = SweptProject::open();
    let parent = p.nodes[2];
    let head = p.head();
    p.sweep_away(parent);
    std::fs::remove_file(p.dir.path().join("take.wav")).expect("remove the take");

    let err = p
        .call("set_head_to", json!({ "nodeId": parent.to_hex() }))
        .expect_err("refused");
    assert!(err.contains("rebuild"), "says why: {err}");
    assert_eq!(p.head(), head, "the head did not move");
}

#[test]
fn accepting_a_swept_b_puts_its_audio_back() {
    let p = SweptProject::open();
    let b = p.nodes[2];
    p.sweep_away(b);

    assert_eq!(
        p.call("accept_b", json!({ "b": b.to_hex() })),
        Ok(json!(b.to_hex()))
    );
    p.assert_back(b);
}

#[test]
fn previewing_a_swept_node_puts_its_audio_back() {
    let p = SweptProject::open();
    let node = p.nodes[2];
    p.sweep_away(node);

    p.call("render_preview", json!({ "node": node.to_hex() }))
        .expect("render_preview");
    p.assert_back(node);
}

#[test]
fn comparing_against_a_swept_node_puts_its_audio_back() {
    let p = SweptProject::open();
    // Two older takes on the same edit, both swept: either side can be
    // one the sweep reached.
    let a = p.nodes[1];
    let b = p.nodes[2];
    p.sweep_away(a);
    p.sweep_away(b);

    p.call(
        "prepare_compare",
        json!({ "a": a.to_hex(), "b": b.to_hex() }),
    )
    .expect("prepare_compare");
    p.assert_back(a);
    p.assert_back(b);
}

#[test]
fn exporting_a_range_of_a_swept_node_puts_its_audio_back() {
    let p = SweptProject::open();
    let node = p.nodes[2];
    p.sweep_away(node);
    let out = p.dir.path().join("selection.wav");

    p.call(
        "render_range",
        json!({ "nodeId": node.to_hex(), "startSec": 0.1, "endSec": 0.9, "outPath": out }),
    )
    .expect("render_range");
    p.assert_back(node);
    assert!(out.is_file(), "and the selection was written");
}

/// #98's acceptance, end to end: undo into a swept region and the audio
/// is what it was — the render of the node after the sweep is the render
/// from before it, byte for byte.
#[test]
fn undoing_into_a_swept_region_renders_what_it_did_before() {
    let p = SweptProject::open();
    let parent = p.nodes[2];
    let render = |name: &str| -> Vec<u8> {
        let out = p.dir.path().join(name);
        p.call(
            "render_range",
            json!({ "nodeId": parent.to_hex(), "startSec": 0.0, "endSec": 2.0, "outPath": out }),
        )
        .expect("render_range");
        std::fs::read(out).expect("rendered")
    };
    let before = render("before.wav");
    p.sweep_away(parent);

    p.call("set_head_to", json!({ "nodeId": parent.to_hex() }))
        .expect("undo");

    assert!(
        render("after.wav") == before,
        "the swept region renders differently"
    );
}

/// The background sweep, one pass: past the cap, history's audio goes and
/// the head's stays — and undo still reaches the swept history (#98).
#[test]
fn the_background_sweep_frees_history_and_undo_still_reaches_it() {
    use tauri::Manager;
    let p = SweptProject::open();
    let head = p.nodes[3];
    let older = p.nodes[1];
    let derived = p.dir.path().join(".audiograph").join("derived");
    let bytes = || -> u64 {
        std::fs::read_dir(&derived)
            .expect("derived")
            .flatten()
            .map(|e| e.metadata().expect("meta").len())
            .sum()
    };
    let before = bytes();

    let report = edytlab_desktop_lib::reclaimer::reclaim_once(&p.app.state::<AppState>(), 0)
        .expect("over the cap, something went");

    assert!(report.removed_files > 0, "{report:?}");
    assert_eq!(before - bytes(), report.freed_bytes, "the report is honest");
    p.assert_back(head);
    assert!(
        !tools::rederive::missing_paths(&session::Store::open(p.dir.path()).unwrap(), older)
            .is_empty(),
        "older history was swept"
    );
    assert_eq!(
        p.call("set_head_to", json!({ "nodeId": older.to_hex() })),
        Ok(json!(older.to_hex()))
    );
    p.assert_back(older);
}

/// Under the cap a pass is a directory listing and nothing else.
#[test]
fn under_the_cap_the_background_sweep_does_nothing() {
    use tauri::Manager;
    let p = SweptProject::open();
    let derived = p.dir.path().join(".audiograph").join("derived");
    let files = || -> Vec<std::path::PathBuf> {
        let mut v: Vec<_> = std::fs::read_dir(&derived)
            .expect("derived")
            .flatten()
            .map(|e| e.path())
            .collect();
        v.sort();
        v
    };
    let before = files();

    let report = edytlab_desktop_lib::reclaimer::reclaim_once(
        &p.app.state::<AppState>(),
        tools::reclaim::DEFAULT_DERIVED_CAP_BYTES,
    );

    assert_eq!(report, None);
    assert_eq!(files(), before);
}

/// The webview plays a rendered preview and an audition over the asset
/// protocol (#402). Both live under `<project>/.audiograph/`, a hidden
/// directory, and on Linux and macOS the asset scope's `**` does not
/// reach through one — so opening a project must grant the caches by
/// name, and only them. The names are the one list Save As leaves out of
/// a copy too (#576), so a cache added to it is checked here as well.
#[test]
fn opening_a_project_lets_the_webview_read_its_previews_and_auditions() {
    let p = SweptProject::open();
    let scope = p.app.asset_protocol_scope();
    let store = p.dir.path().join(".audiograph");

    let caches = tools::preview_cache::CACHE_DIRS;
    assert!(caches.contains(&"previews") && caches.contains(&"auditions"));
    for cache in caches {
        let wav = store.join(cache).join("0123abcd.wav");
        assert!(scope.is_allowed(&wav), "{} is not readable", wav.display());
    }
    // What else lives in the store is not the webview's to read.
    for private in [
        store.join("notes.json"),
        store.join("derived").join("0123abcd.wav"),
    ] {
        assert!(
            !scope.is_allowed(&private),
            "{} was exposed",
            private.display()
        );
    }
}

// ---------------------------------------------------------------------------
// Plan first across a restart (#569)
// ---------------------------------------------------------------------------

/// A launch of the app over IPC: a state reading the settings at
/// `settings`, and the two Plan first commands the composer calls.
fn plan_first_launch(
    settings: &std::path::Path,
) -> (
    tauri::App<tauri::test::MockRuntime>,
    tauri::WebviewWindow<tauri::test::MockRuntime>,
) {
    let state = AppState::new();
    state.install_settings(settings.to_path_buf());
    let app = mock_builder()
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            commands::set_plan_first,
            commands::get_plan_first
        ])
        .build(mock_context(noop_assets()))
        .expect("mock app");
    let webview = WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .expect("webview");
    (app, webview)
}

fn plan_first_over_ipc(webview: &tauri::WebviewWindow<tauri::test::MockRuntime>) -> bool {
    tauri::test::get_ipc_response(webview, make_request("get_plan_first", json!({})))
        .expect("answered")
        .deserialize::<bool>()
        .expect("a bool")
}

/// The bug, through the commands the composer calls: the toggle is
/// clicked on, the app is quit, and the next launch's `get_plan_first`
/// (what the toggle reads when the chat box opens) says on.
#[test]
fn plan_first_set_through_the_command_is_on_in_the_next_launch() {
    let home = tempfile::tempdir().expect("tempdir");
    let settings = home.path().join(".edytlab").join("settings.json");

    let (app, webview) = plan_first_launch(&settings);
    assert!(!plan_first_over_ipc(&webview), "a fresh install starts off");
    assert_ipc_response(
        &webview,
        make_request("set_plan_first", json!({ "enabled": true })),
        Ok(json!(null)),
    );
    assert!(plan_first_over_ipc(&webview));
    drop(webview);
    drop(app);

    let (_app, webview) = plan_first_launch(&settings);
    assert!(
        plan_first_over_ipc(&webview),
        "Plan first was turned on, and the next launch started with it off"
    );
}

/// A save that fails says so, with words the composer can show, and the
/// choice still applies: the toggle must not be told it failed while the
/// next turn is held (or not) the other way.
#[test]
fn a_plan_first_save_that_fails_says_so_and_still_applies() {
    let home = tempfile::tempdir().expect("tempdir");
    // The settings folder is a file, so nothing can be created in it.
    let blocker = home.path().join(".edytlab");
    std::fs::write(&blocker, "not a folder").expect("blocker");

    let (_app, webview) = plan_first_launch(&blocker.join("settings.json"));
    let response = tauri::test::get_ipc_response(
        &webview,
        make_request("set_plan_first", json!({ "enabled": true })),
    );

    let err = response.expect_err("a failed save must not answer Ok");
    let text = err.as_str().expect("error payload is a string");
    assert!(
        text.contains("could not save") && text.contains("Plan first"),
        "unexpected error string: {text}"
    );
    assert!(plan_first_over_ipc(&webview), "the choice still applies");
}

/// `ml_runtime_status` over IPC answers with a `state` the window can read,
/// and returns: it loads a library at most (the one `ORT_DYLIB_PATH` names,
/// if this shell sets it), never panics or hangs on a machine with none.
/// The mapping of each state to JSON is covered by the unit tests in
/// `src/commands.rs`; this is the registration and the wire.
#[test]
fn ml_runtime_status_over_ipc_answers_with_a_state() {
    let app = mock_builder()
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![commands::ml_runtime_status])
        .build(mock_context(noop_assets()))
        .expect("mock app");
    let webview = WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .expect("webview");

    let answer =
        tauri::test::get_ipc_response(&webview, make_request("ml_runtime_status", json!({})))
            .expect("a status is an answer, not an error")
            .deserialize::<serde_json::Value>()
            .expect("json");

    match answer["state"].as_str() {
        Some("missing") => {
            let searched = answer["searched"].as_array().expect("searched is a list");
            assert!(searched.iter().all(|p| p.is_string()), "{answer}");
        }
        Some("found") => {
            assert!(answer["version"].as_str().is_some(), "{answer}");
            assert!(answer["path"].as_str().is_some(), "{answer}");
            assert!(
                matches!(
                    answer["source"].as_str(),
                    Some("bundled" | "environment" | "executable")
                ),
                "{answer}"
            );
        }
        Some("loadFailed") => assert!(answer["reason"].as_str().is_some(), "{answer}"),
        Some("unsupportedOs") => assert!(answer["needs"].as_str().is_some(), "{answer}"),
        other => panic!("not a known state: {other:?} in {answer}"),
    }
}
