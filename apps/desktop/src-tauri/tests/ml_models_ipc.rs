//! The speech model commands over IPC, against a local server (#383).
//!
//! `ml_model_status`, `download_ml_model`, `cancel_ml_model_download` and
//! `remove_ml_model` are what the Settings row calls. This drives them
//! through `tauri::test::mock_builder`, the way the window does, with the
//! model served from 127.0.0.1 so nothing leaves the machine. It checks
//! what unit tests cannot: that the commands are registered and take
//! `{ model }`, that the answers have the shape `tauri-bridge.ts` types,
//! that progress and the end arrive on the `ml-model-progress` event, and
//! that nothing is requested until `download_ml_model` is called.
//!
//! The download engine has its own tests in `crates/ml-pipeline`; the
//! guard that nothing but this command starts a transfer is
//! `crates/ml-pipeline/tests/only_the_user_downloads.rs`.
//!
//! Gated off Windows for the reason `commands_mock.rs` gives.

#![cfg(not(target_os = "windows"))]

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use edytlab_desktop_lib::commands::{self, ML_MODEL_PROGRESS_EVENT};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tauri::ipc::{CallbackFn, InvokeBody};
use tauri::test::{mock_builder, mock_context, noop_assets, MockRuntime, INVOKE_KEY};
use tauri::webview::InvokeRequest;
use tauri::{Listener, Manager, WebviewWindow, WebviewWindowBuilder};

fn make_request(cmd: &str, body: Value) -> InvokeRequest {
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

/// A server that answers every GET with `body`, and counts them.
fn serve(body: Vec<u8>) -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let hits = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&hits);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            counted.fetch_add(1, Ordering::SeqCst);
            let body = body.clone();
            std::thread::spawn(move || {
                let mut head = Vec::new();
                let mut byte = [0u8; 1];
                while !head.ends_with(b"\r\n\r\n") {
                    if stream.read(&mut byte).unwrap_or(0) == 0 {
                        return;
                    }
                    head.push(byte[0]);
                }
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(&body);
            });
        }
    });
    (format!("http://{addr}/model.bin"), hits)
}

struct Rig {
    _app: tauri::App<MockRuntime>,
    webview: WebviewWindow<MockRuntime>,
    events: mpsc::Receiver<Value>,
    hits: Arc<AtomicUsize>,
    dir: tempfile::TempDir,
    body: Vec<u8>,
}

fn rig() -> Rig {
    let body: Vec<u8> = (0..700_000u32)
        .map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8)
        .collect();
    let (url, hits) = serve(body.clone());
    let sha: String = Sha256::digest(&body)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let catalog = ml_pipeline::Catalog::from_json(&format!(
        r#"{{ "models": [ {{
            "id": "speech-test", "title": "Speech test", "repo": "example/speech-test",
            "revision": "0123456789abcdef0123456789abcdef01234567", "license": "MIT",
            "files": [ {{ "path": "model.bin", "url": "{url}", "size": {}, "sha256": "{sha}" }} ]
        }} ] }}"#,
        body.len()
    ))
    .expect("catalog");

    let app = mock_builder()
        .invoke_handler(tauri::generate_handler![
            commands::ml_model_status,
            commands::download_ml_model,
            commands::cancel_ml_model_download,
            commands::remove_ml_model
        ])
        .build(mock_context(noop_assets()))
        .expect("mock app");
    let dir = tempfile::tempdir().expect("tempdir");
    app.manage(commands::ml_models_with_catalog_for_app(
        app.handle().clone(),
        Some(dir.path().join("models")),
        catalog,
    ));
    let (tx, events) = mpsc::channel();
    app.listen(ML_MODEL_PROGRESS_EVENT, move |event| {
        let _ = tx.send(serde_json::from_str::<Value>(event.payload()).expect("a JSON payload"));
    });
    let webview = WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .expect("webview");
    Rig {
        _app: app,
        webview,
        events,
        hits,
        dir,
        body,
    }
}

impl Rig {
    fn call(&self, cmd: &str, model: &str) -> Result<Value, String> {
        tauri::test::get_ipc_response(&self.webview, make_request(cmd, json!({ "model": model })))
            .map(|r| r.deserialize::<Value>().expect("json"))
            .map_err(|e| e.as_str().expect("an error is a string").to_string())
    }

    /// The next event whose `state` is `state`.
    fn wait_for_state(&self, state: &str) -> Value {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let event = self
                .events
                .recv_timeout(left)
                .unwrap_or_else(|_| panic!("no `{state}` event arrived"));
            if event["state"] == state {
                return event;
            }
            assert_ne!(event["state"], "failed", "the download failed: {event}");
        }
    }
}

#[test]
fn the_status_is_asked_for_without_any_request_and_says_not_downloaded() {
    let rig = rig();
    let answer = rig
        .call("ml_model_status", "speech-test")
        .expect("a status");
    assert_eq!(
        answer,
        json!({
            "model": "speech-test",
            "state": "not_downloaded",
            "bytes": 0,
            "size": rig.body.len(),
        })
    );
    assert_eq!(
        rig.hits.load(Ordering::SeqCst),
        0,
        "a status downloads nothing"
    );
}

#[test]
fn download_status_event_and_remove_over_ipc() {
    let rig = rig();
    let size = rig.body.len();

    let started = rig
        .call("download_ml_model", "speech-test")
        .expect("start answers at once");
    assert_eq!(started["state"], "downloading");
    assert_eq!(started["total"], size);
    assert_eq!(started["model"], "speech-test");

    // The end arrives as an event, in the shape the command answers with.
    let ready = rig.wait_for_state("ready");
    assert_eq!(
        ready,
        json!({ "model": "speech-test", "state": "ready", "bytes": size, "size": size })
    );
    assert_eq!(rig.hits.load(Ordering::SeqCst), 1);
    let on_disk = rig.dir.path().join("models/speech-test/model.bin");
    assert_eq!(std::fs::read(&on_disk).expect("the model"), rig.body);

    let status = rig.call("ml_model_status", "speech-test").expect("status");
    assert_eq!(status["state"], "ready");

    // Remove deletes it and says so, and the event follows.
    let removed = rig.call("remove_ml_model", "speech-test").expect("remove");
    assert_eq!(removed["state"], "not_downloaded");
    assert_eq!(removed["bytes"], 0);
    assert!(!on_disk.exists());
    rig.wait_for_state("not_downloaded");
}

#[test]
fn cancel_with_nothing_running_is_not_an_error() {
    let rig = rig();
    let answer = rig
        .call("cancel_ml_model_download", "speech-test")
        .expect("cancel");
    assert_eq!(answer["state"], "not_downloaded");
    assert_eq!(rig.hits.load(Ordering::SeqCst), 0);
}

#[test]
fn an_id_that_is_not_in_the_catalog_is_an_error_for_every_command() {
    let rig = rig();
    for cmd in [
        "ml_model_status",
        "download_ml_model",
        "cancel_ml_model_download",
        "remove_ml_model",
    ] {
        let err = rig
            .call(cmd, "../speech-test")
            .expect_err("a model that is not listed");
        assert!(err.contains("unknown model"), "{cmd}: {err}");
    }
    assert_eq!(rig.hits.load(Ordering::SeqCst), 0);
    assert!(!rig.dir.path().join("models").exists());
}

#[test]
fn an_app_with_no_data_folder_says_so_instead_of_failing_to_find_a_state() {
    let app = mock_builder()
        .invoke_handler(tauri::generate_handler![commands::ml_model_status])
        .build(mock_context(noop_assets()))
        .expect("mock app");
    app.manage(commands::ml_models_for_app(app.handle().clone(), None));
    let webview = WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .expect("webview");
    let err = tauri::test::get_ipc_response(
        &webview,
        make_request("ml_model_status", json!({ "model": "whisper-base" })),
    )
    .expect_err("nowhere to keep a model");
    assert!(
        err.as_str().is_some_and(|e| e.contains("no data folder")),
        "{err}"
    );
}
