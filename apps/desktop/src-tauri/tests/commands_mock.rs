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

use edytlab_desktop_lib::commands;
use edytlab_desktop_lib::state::AppState;
use serde_json::json;
use tauri::ipc::{CallbackFn, InvokeBody};
use tauri::test::{assert_ipc_response, mock_builder, mock_context, noop_assets, INVOKE_KEY};
use tauri::webview::InvokeRequest;
use tauri::WebviewWindowBuilder;

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
