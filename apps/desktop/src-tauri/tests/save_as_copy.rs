//! What Save As puts in the copy (#571, #576), through the real commands.
//!
//! A project with history made by the agent's own tools: a take loaded from
//! outside the project, a destructive edit (audio in `derived/`), and a copy
//! and paste (a blob in `clipboard/`). Its two caches hold a few renders, and
//! the user keeps folders of their own that happen to share the caches'
//! names. Save As is called over IPC:
//!
//! * the caches, `.audiograph/previews/` and `.audiograph/auditions/`, are
//!   left behind and counted, since every entry rebuilds on demand;
//! * a folder that is only called `previews` or `auditions` is the user's,
//!   and travels, as does the clipboard;
//! * the window is on the copy, and the copy's head renders there.
//!
//! What the copy holds before anything opens it, and that it plays once the
//! original is gone, is shown on `project::copy_project` itself (its unit
//! tests): opening the copy sweeps its `derived/` audio while the original
//! is still there (#612), which is not this change's to fix.
//!
//! Windows: gated like `commands_mock.rs`, for the same reason (Wry's
//! WebView2 imports).

#![cfg(not(target_os = "windows"))]

use std::path::{Path, PathBuf};

use edytlab_desktop_lib::commands;
use edytlab_desktop_lib::state::AppState;
use serde_json::{json, Value};
use tauri::ipc::{CallbackFn, InvokeBody};
use tauri::test::{mock_builder, mock_context, noop_assets, MockRuntime, INVOKE_KEY};
use tauri::webview::InvokeRequest;
use tauri::{WebviewWindow, WebviewWindowBuilder};

fn request(cmd: &str, body: Value) -> InvokeRequest {
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

fn call(webview: &WebviewWindow<MockRuntime>, cmd: &str, body: Value) -> Result<Value, String> {
    tauri::test::get_ipc_response(webview, request(cmd, body))
        .map(|r| r.deserialize::<Value>().expect("json"))
        .map_err(|e| e.as_str().unwrap_or_default().to_string())
}

/// A mono 16-bit WAV of `frames` frames at 8 kHz, a square wave.
fn write_wav(path: &Path, frames: u32) {
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
    for n in 0..frames {
        let sample: i16 = if (n / 20) % 2 == 0 { 4_000 } else { -4_000 };
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    std::fs::write(path, bytes).expect("write wav");
}

/// Run one of the agent's tools on `store`, as the agent would.
fn tool(
    store: &mut session::Store,
    clipboard: &mut Option<tools::Clipboard>,
    name: &str,
    args: Value,
) {
    let mut engine = audio_engine::Engine::new();
    let dispatcher = tools::ToolDispatcher::default_dispatcher();
    let mut ctx = tools::ToolContext {
        store,
        engine: &mut engine,
        user_message: "",
        clipboard,
        allowed_tools: None,
    };
    match dispatcher.invoke(name, args, &mut ctx).expect("invoke") {
        tools::ToolResult::Ok(_) => {}
        tools::ToolResult::Error(m) => panic!("{name} failed: {m}"),
    }
}

/// Every file under `dir`, relative to it.
fn files_under(dir: &Path) -> Vec<PathBuf> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                out.push(path.strip_prefix(root).unwrap().to_path_buf());
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

#[test]
fn a_copy_leaves_the_caches_behind_and_its_head_renders() {
    let take_dir = tempfile::tempdir().expect("tempdir");
    let original = tempfile::tempdir().expect("tempdir");
    let copies = tempfile::tempdir().expect("tempdir");
    let src = original.path();
    let store_dir = src.join(session::STORE_DIR);

    // History, made by the tools.
    let take = take_dir.path().join("take.wav");
    write_wav(&take, 16_000);
    let head = {
        let mut store = session::Store::open(src).expect("store");
        let mut clipboard = None;
        tool(&mut store, &mut clipboard, "load", json!({ "path": take }));
        tool(
            &mut store,
            &mut clipboard,
            "silence_region",
            json!({ "track": 0, "start_sec": 0.2, "end_sec": 0.4 }),
        );
        tool(
            &mut store,
            &mut clipboard,
            "copy_region",
            json!({ "track": 0, "range": { "start_sec": 0.5, "end_sec": 0.7 } }),
        );
        tool(
            &mut store,
            &mut clipboard,
            "paste_region",
            json!({ "track": 0, "at": 1.0 }),
        );
        store.head().expect("a head")
    };

    // Two caches with renders in them, and two folders that only share
    // their names.
    for (dir, name) in [
        (store_dir.join("previews"), "0123.wav"),
        (store_dir.join("previews"), "4567.wav"),
        (store_dir.join("auditions"), "89ab.wav"),
        (src.join("previews"), "my-mixdown.wav"),
        (src.join("auditions"), "take-3.wav"),
        (store_dir.join("derived").join("previews"), "kept.wav"),
    ] {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(name), vec![0u8; 4096]).unwrap();
    }

    let app = mock_builder()
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            commands::open_project,
            commands::save_project_as,
            commands::render_preview,
        ])
        .build(mock_context(noop_assets()))
        .expect("mock app");
    let webview = WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .expect("webview");
    call(&webview, "open_project", json!({ "path": src })).expect("open_project");
    let clipboard = files_under(&store_dir.join("clipboard"));
    assert!(!clipboard.is_empty(), "the paste wrote no clipboard blob");

    let dest = copies.path().join("copy");
    let report =
        call(&webview, "save_project_as", json!({ "dest": dest })).expect("save_project_as");

    let copied = files_under(&dest);
    let copy_store = Path::new(session::STORE_DIR);
    for cache in ["previews", "auditions"] {
        assert!(
            !dest.join(session::STORE_DIR).join(cache).exists(),
            "the copy took the cache {cache}/: {copied:?}"
        );
    }
    for kept in [
        PathBuf::from("previews").join("my-mixdown.wav"),
        PathBuf::from("auditions").join("take-3.wav"),
        copy_store.join("derived").join("previews").join("kept.wav"),
    ] {
        assert!(
            copied.contains(&kept),
            "{} is not a cache and was left behind: {copied:?}",
            kept.display()
        );
    }
    for file in &clipboard {
        assert!(copied.contains(&copy_store.join("clipboard").join(file)));
    }
    assert_eq!(
        report["skipped_cache_files"],
        json!(3),
        "the report counts the cache files it left: {report}"
    );

    // The window is on the copy, and the copy's head renders there.
    let preview = call(&webview, "render_preview", json!({ "node": head.to_hex() }))
        .expect("the copy's head renders");
    let preview = PathBuf::from(preview.as_str().expect("a path"));
    assert!(
        preview.starts_with(dest.join(session::STORE_DIR)),
        "{}",
        preview.display()
    );
    assert!(preview.is_file());
}
