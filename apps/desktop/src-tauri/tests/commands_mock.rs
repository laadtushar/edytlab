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
