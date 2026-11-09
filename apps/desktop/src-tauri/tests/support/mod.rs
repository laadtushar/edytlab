//! What the IPC tests share: the request they send, the window they send it
//! through, the project they send it about.
//!
//! Every test here drives the real commands through Tauri's mock runtime,
//! and each used to carry its own copy of the same pieces: an
//! `InvokeRequest`, the mock app and its window, the call that turns an
//! answer into a `Result<Value, String>`, a small WAV, and a project built
//! by the agent's tools. A test binary includes this with
//! `#[macro_use] mod support;` and uses a subset.
//!
//! The tests that include it are gated off Windows, for the reason
//! `commands_mock.rs` gives (Wry's WebView2 imports).
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use edytlab_desktop_lib::commands;
use edytlab_desktop_lib::state::AppState;
use serde_json::{json, Value};
use session::NodeId;
use tauri::http::HeaderMap;
use tauri::ipc::{CallbackFn, InvokeBody};
use tauri::test::{MockRuntime, INVOKE_KEY};
use tauri::webview::InvokeRequest;
use tauri::{WebviewUrl, WebviewWindow, WebviewWindowBuilder};

/// An invoke request for `cmd`, as the webview sends one.
pub fn make_request(cmd: &str, body: Value) -> InvokeRequest {
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
        headers: HeaderMap::default(),
        invoke_key: INVOKE_KEY.to_string(),
    }
}

/// The window of `app`, the one the commands are called through.
pub fn open_window(app: &tauri::App<MockRuntime>) -> WebviewWindow<MockRuntime> {
    WebviewWindowBuilder::new(app, "main", WebviewUrl::default())
        .build()
        .expect("webview")
}

/// A mock app managing `$state` and answering the commands named after it,
/// with its window: `mock_window!(AppState::new(); commands::open_project)`.
/// The app is returned too, because dropping it ends the window.
macro_rules! mock_window {
    ($state:expr; $($handlers:tt)*) => {{
        let app = tauri::test::mock_builder()
            .manage($state)
            .invoke_handler(tauri::generate_handler![$($handlers)*])
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");
        let webview = $crate::support::open_window(&app);
        (app, webview)
    }};
}

/// `cmd` over IPC, answered as the frontend sees it: the JSON, or the
/// error's text.
pub fn call(webview: &WebviewWindow<MockRuntime>, cmd: &str, body: Value) -> Result<Value, String> {
    tauri::test::get_ipc_response(webview, make_request(cmd, body))
        .map(|r| r.deserialize::<Value>().expect("json"))
        .map_err(|e| e.as_str().unwrap_or_default().to_string())
}

/// A mono 16-bit WAV of `frames` frames at 8 kHz, sample `n` being
/// `sample(n)`.
pub fn write_wav_with(path: &Path, frames: u32, sample: impl Fn(u32) -> i16) {
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
        bytes.extend_from_slice(&sample(n).to_le_bytes());
    }
    std::fs::write(path, bytes).expect("write wav");
}

/// A mono 16-bit WAV of `frames` frames at 8 kHz, every sample 1000.
pub fn write_wav(path: &Path, frames: u32) {
    write_wav_with(path, frames, |_| 1_000);
}

/// Run one of the agent's tools on `store`, as the agent would, and return
/// what it answered. A tool that fails fails the test.
pub fn invoke_tool(
    store: &mut session::Store,
    clipboard: &mut Option<tools::Clipboard>,
    name: &str,
    args: Value,
) -> Value {
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
        tools::ToolResult::Ok(v) => v,
        tools::ToolResult::Error(m) => panic!("{name} failed: {m}"),
    }
}

/// Write `project/take.wav` (16 000 frames of a constant), load it, and
/// silence `len_sec` from each of `starts`, through the tools the agent
/// uses. The node ids come back oldest first: the load, then each edit.
pub fn silence_history(project: &Path, starts: &[f64], len_sec: f64) -> Vec<NodeId> {
    let take = project.join("take.wav");
    write_wav(&take, 16_000);
    let mut store = session::Store::open(project).expect("store");
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
    let mut run = |tool: &str, args: Value, ctx: &mut tools::ToolContext| match dispatcher
        .invoke(tool, args, ctx)
        .expect("invoke")
    {
        tools::ToolResult::Ok(_) => nodes.push(ctx.store.head().expect("head")),
        tools::ToolResult::Error(m) => panic!("{tool} failed: {m}"),
    };
    run("load", json!({ "path": take }), &mut ctx);
    for &start in starts {
        run(
            "silence_region",
            json!({ "track": 0, "start_sec": start, "end_sec": start + len_sec }),
            &mut ctx,
        );
    }
    nodes
}

/// A project with a load and five edits after it, opened over IPC, with the
/// commands that render and compare its mixes.
///
/// Every node is the same length, so every whole-mix render is the same
/// size, which lets a test set the preview cache's cap in renders. The
/// environment tests change (the cap, the OS temp directory) is theirs to
/// set and restore; this holds none of it.
pub struct MixBed {
    _root: tempfile::TempDir,
    pub project: PathBuf,
    /// Oldest first: the load, then each edit.
    pub nodes: Vec<NodeId>,
    pub webview: WebviewWindow<MockRuntime>,
    _app: tauri::App<MockRuntime>,
}

impl MixBed {
    /// Build the history in `project` (a folder of `root`, made already) and
    /// open it. `root` is kept for as long as the bed lives.
    pub fn open(root: tempfile::TempDir, project: PathBuf) -> Self {
        let nodes = silence_history(&project, &[0.0, 0.3, 0.6, 0.9, 1.2], 0.1);
        let (app, webview) = mock_window!(
            AppState::new();
            commands::open_project,
            commands::render_preview,
            commands::prepare_compare,
            commands::end_compare,
        );
        let bed = Self {
            _root: root,
            project,
            nodes,
            webview,
            _app: app,
        };
        bed.call("open_project", json!({ "path": bed.project }))
            .expect("open_project");
        bed
    }

    pub fn call(&self, cmd: &str, body: Value) -> Result<Value, String> {
        call(&self.webview, cmd, body)
    }

    /// The window's render of `node`: what Play and Preview ask for.
    pub fn window_render(&self, node: NodeId) -> Result<PathBuf, String> {
        self.call("render_preview", json!({ "node": node.to_hex() }))
            .map(|v| PathBuf::from(v.as_str().expect("a path")))
    }

    /// The agent's `render_preview` tool for `node`, as the dispatcher runs
    /// it: into the same cache, by the tool and not by the window's command.
    pub fn agent_render(&self, node: NodeId) -> PathBuf {
        let mut store = session::Store::open(&self.project).expect("store");
        let answer = invoke_tool(
            &mut store,
            &mut None,
            "render_preview",
            json!({ "node_id": node.to_hex() }),
        );
        PathBuf::from(answer["path"].as_str().expect("the tool answers a path"))
    }

    pub fn previews(&self) -> PathBuf {
        self.project.join(".audiograph").join("previews")
    }

    /// Where the cache keeps the whole mix of `node`.
    pub fn entry(&self, node: NodeId) -> PathBuf {
        self.previews().join(format!("{}.wav", node.to_hex()))
    }

    /// Bytes on disk for the whole-mix entries of `nodes` that are there.
    pub fn bytes_of(&self, nodes: &[NodeId]) -> u64 {
        nodes
            .iter()
            .filter_map(|n| std::fs::metadata(self.entry(*n)).ok())
            .map(|m| m.len())
            .sum()
    }
}

#[allow(unused_macros)]
/// A `Sandbox` that is a `MixBed` with a lock and some environment on top.
macro_rules! sandbox_derefs_to_bed {
    ($sandbox:ty) => {
        impl std::ops::Deref for $sandbox {
            type Target = $crate::support::MixBed;
            fn deref(&self) -> &Self::Target {
                &self.bed
            }
        }
    };
}
