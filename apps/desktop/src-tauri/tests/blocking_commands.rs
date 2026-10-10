//! A command waiting on a lock does not occupy a Tokio worker (#498).
//!
//! Tauri runs every `async` command as a task on the async runtime, and a
//! command declared `#[tauri::command(async)]` the same way. The session
//! commands do their work with `std::sync::Mutex` locks (the store, the
//! engine, the dispatcher), which a running tool can hold for a long
//! time, and a `std` lock blocks the thread rather than the task. Run
//! inside the task, each waiting command held a worker until the lock was
//! free: about as many fader clicks as there are cores, during one long
//! tool, and no async command could run at all, including ones that touch
//! no lock. The blocking work belongs on the blocking pool.
//!
//! The runtime here has a single worker, so one command left waiting on
//! its worker is enough to show it. The test holds the three locks,
//! sends a command for each way in, and then one that takes no lock; the
//! last must answer while the others are still waiting.
//!
//! Why this needs no sleeping to be sure the others are waiting by then:
//! a task spawned from outside the runtime goes on its injection queue,
//! which is first in, first out, and the one worker runs the tasks in
//! that order. So by the time it reaches the last, it has already run
//! each of the others as far as its first wait. If one of them blocks
//! the worker there, the last never runs.

#![cfg(not(target_os = "windows"))]

use std::sync::mpsc::{self, Receiver};
use std::sync::Once;
use std::time::Duration;

use edytlab_desktop_lib::commands;
use edytlab_desktop_lib::state::AppState;
use serde_json::json;
use tauri::ipc::{CallbackFn, InvokeBody, InvokeResponse};
use tauri::test::{mock_builder, mock_context, noop_assets, MockRuntime, INVOKE_KEY};
use tauri::webview::InvokeRequest;
use tauri::{WebviewWindow, WebviewWindowBuilder};

/// Make Tauri's async runtime one with a single worker. Must run before
/// anything else in this process touches the runtime, and can run once.
fn one_worker() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .expect("runtime");
        tauri::async_runtime::set(runtime.handle().clone());
        // The handle is all Tauri keeps; the runtime has to outlive it.
        Box::leak(Box::new(runtime));
    });
}

fn request(cmd: &str, body: serde_json::Value) -> InvokeRequest {
    InvokeRequest {
        cmd: cmd.into(),
        callback: CallbackFn(0),
        error: CallbackFn(1),
        url: "tauri://localhost".parse().unwrap(),
        body: InvokeBody::Json(body),
        headers: Default::default(),
        invoke_key: INVOKE_KEY.to_string(),
    }
}

/// Send a command and return at once, with the channel its answer comes
/// back on. `get_ipc_response` would wait for the answer.
fn send(
    webview: &WebviewWindow<MockRuntime>,
    cmd: &str,
    body: serde_json::Value,
) -> Receiver<InvokeResponse> {
    let (tx, rx) = mpsc::channel();
    webview.as_ref().clone().on_message(
        request(cmd, body),
        Box::new(move |_webview, _cmd, response, _callback, _error| {
            let _ = tx.send(response);
        }),
    );
    rx
}

const NODE: &str = "0000000000000000000000000000000000000000000000000000000000000000";

#[test]
fn commands_waiting_on_a_lock_leave_the_worker_free() {
    one_worker();
    let project = tempfile::tempdir().expect("tempdir");

    let state = AppState::new();
    let app = mock_builder()
        .manage(state.clone())
        .invoke_handler(tauri::generate_handler![
            commands::open_project,
            commands::get_plan_first,
            commands::set_track_gain,
            commands::set_sync_lock,
            commands::set_head_to,
            commands::rename_node,
            commands::list_markers,
            commands::get_transcript,
            commands::get_sync_lock,
            commands::get_graph,
            commands::get_node,
            commands::get_session_head,
            commands::list_capabilities,
        ])
        .build(mock_context(noop_assets()))
        .expect("mock app");
    let webview = WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .expect("webview");

    tauri::test::get_ipc_response(
        &webview,
        request("open_project", json!({ "path": project.path() })),
    )
    .expect("open the project");

    // What a running tool holds, for as long as it runs.
    let store = state.store_handle().expect("a project is open");
    let store_guard = store.lock().expect("store");
    let engine_guard = state.engine.lock().expect("engine");
    let dispatcher_guard = state.dispatcher.lock().expect("dispatcher");

    let waiting: Vec<(&str, Receiver<InvokeResponse>)> = [
        ("set_track_gain", json!({ "track": 0, "gainDb": -3.0 })),
        ("set_sync_lock", json!({ "enabled": true })),
        ("set_head_to", json!({ "nodeId": NODE })),
        ("rename_node", json!({ "nodeId": NODE, "label": "x" })),
        ("list_markers", json!({})),
        ("get_transcript", json!({})),
        ("get_sync_lock", json!({})),
        ("get_graph", json!({})),
        ("get_node", json!({ "id": NODE })),
        ("get_session_head", json!({})),
        ("list_capabilities", json!({})),
    ]
    .into_iter()
    .map(|(cmd, body)| (cmd, send(&webview, cmd, body)))
    .collect();

    // A command that touches no lock, sent after all of them.
    let probe = send(&webview, "get_plan_first", json!({}));
    let probe_answer = probe.recv_timeout(Duration::from_secs(5));

    // Let go before asserting, so a failure here cannot leave the others
    // waiting for ever.
    drop(dispatcher_guard);
    drop(engine_guard);
    drop(store_guard);

    assert!(
        probe_answer.is_ok(),
        "a command that takes no lock was not answered in 5 s while others \
         waited on the store, engine and dispatcher: they are holding the \
         runtime's only worker"
    );

    // And nothing was lost: once the locks are free, every waiting
    // command is answered.
    for (cmd, answer) in waiting {
        assert!(
            answer.recv_timeout(Duration::from_secs(30)).is_ok(),
            "`{cmd}` was never answered after the locks were released"
        );
    }
}
