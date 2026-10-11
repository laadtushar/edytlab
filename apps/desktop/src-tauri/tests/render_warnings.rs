//! The app says what a render leaves out (#532).
//!
//! #531 made the renderer skip a stored effect it cannot run (reverb,
//! echo, eq, ... or a kind nobody knows) instead of failing every render,
//! and report each skip. The agent's tools pass that report on; the
//! window did not, so a project saved by an older build played and
//! exported without its reverb and said nothing.
//!
//! These tests drive the real commands over IPC, on a project whose
//! history holds a node with stored entries written straight into it, the
//! way an older build saved them (never through a tool, which refuses the
//! kind now):
//!
//! * `get_render_warnings` answers for a node, from the stored chains
//!   alone, with no render. The window asks it whenever the node it shows
//!   changes, so a warning clears when the effect is removed or bypassed.
//! * `render_range` (the export) and `prepare_compare` (A/B) carry the
//!   same list beside what they already returned, as an added key.
//! * `render_preview` still answers a bare path: the warnings reach the
//!   window through `get_render_warnings`, which is why its contract did
//!   not need to change.
//!
//! Windows: gated like `commands_mock.rs`, for the same reason (Wry's
//! WebView2 imports).

#![cfg(not(target_os = "windows"))]

use edytlab_desktop_lib::commands;
use edytlab_desktop_lib::state::AppState;
use serde_json::{json, Value};
use session::{Bus, EffectInstance, NodeId};
use tauri::ipc::{CallbackFn, InvokeBody};
use tauri::test::{mock_builder, mock_context, noop_assets, INVOKE_KEY};
use tauri::webview::InvokeRequest;
use tauri::WebviewWindowBuilder;

fn make_request(cmd: &str, body: Value) -> InvokeRequest {
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

fn entry(kind: &str, bypassed: bool) -> EffectInstance {
    EffectInstance {
        kind: kind.to_string(),
        params: json!({}),
        bypassed,
    }
}

/// A real, runnable entry: it sits in the chain beside a skipped one so a
/// warning's index is the place in the *stored* chain.
fn gain() -> EffectInstance {
    EffectInstance {
        kind: "gain".to_string(),
        params: json!({ "gain_db": -3.0 }),
        bypassed: false,
    }
}

/// The nodes of a project saved by an older build.
struct Nodes {
    /// One track, no effects.
    clean: NodeId,
    /// `clean` plus `gain, reverb` on track 0 and `reverb` on the master.
    stored: NodeId,
    /// `stored` with both reverbs bypassed.
    bypassed: NodeId,
    /// `stored` with both reverbs removed: what the agent's `remove_effect`
    /// leaves behind.
    removed: NodeId,
    /// A bus holding an `echo`, an unknown kind on the master, nothing on
    /// the track.
    bus_and_unknown: NodeId,
}

struct Project {
    dir: tempfile::TempDir,
    nodes: Nodes,
    track_name: String,
    webview: tauri::WebviewWindow<tauri::test::MockRuntime>,
    // Kept alive: the webview borrows nothing, but dropping the app ends it.
    _app: tauri::App<tauri::test::MockRuntime>,
}

impl Project {
    fn open() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let take = dir.path().join("take.wav");
        write_wav(&take, 16_000);

        let (nodes, track_name) = {
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
            match dispatcher
                .invoke("load", json!({ "path": take }), &mut ctx)
                .expect("invoke")
            {
                tools::ToolResult::Ok(_) => {}
                tools::ToolResult::Error(m) => panic!("load failed: {m}"),
            }
            let clean = store.head().expect("head after load");
            let base = store.get(clean).expect("the loaded node");
            let track_name = base.state.tracks[0].name.clone();

            let mut write = |edit: &dyn Fn(&mut session::SessionState)| -> NodeId {
                let mut node = store.get(store.head().unwrap()).unwrap();
                edit(&mut node.state);
                store.append(node).expect("append")
            };
            let stored = write(&|s| {
                s.tracks[0].effects = vec![gain(), entry("reverb", false)];
                s.master_chain = vec![entry("reverb", false)];
            });
            let bypassed = write(&|s| {
                s.tracks[0].effects[1].bypassed = true;
                s.master_chain[0].bypassed = true;
            });
            // Back to the stored state's chains, then without the entries.
            let removed = write(&|s| {
                s.tracks[0].effects = vec![gain()];
                s.master_chain = Vec::new();
            });
            let bus_and_unknown = write(&|s| {
                s.tracks[0].effects = Vec::new();
                s.bus_routing.buses.push(Bus {
                    id: uuid::Uuid::from_u128(7),
                    name: "Verb".to_string(),
                    effects: vec![entry("echo", false)],
                });
                s.master_chain = vec![entry("wobbulator", false)];
            });
            (
                Nodes {
                    clean,
                    stored,
                    bypassed,
                    removed,
                    bus_and_unknown,
                },
                track_name,
            )
        };

        let app = mock_builder()
            .manage(AppState::new())
            .invoke_handler(tauri::generate_handler![
                commands::open_project,
                commands::render_preview,
                commands::prepare_compare,
                commands::render_range,
                commands::get_render_warnings,
            ])
            .build(mock_context(noop_assets()))
            .expect("mock app");
        let webview = WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .expect("webview");
        let project = Self {
            dir,
            nodes,
            track_name,
            webview,
            _app: app,
        };
        project
            .call("open_project", json!({ "path": project.dir.path() }))
            .expect("open_project");
        project
    }

    fn call(&self, cmd: &str, body: Value) -> Result<Value, String> {
        tauri::test::get_ipc_response(&self.webview, make_request(cmd, body))
            .map(|r| r.deserialize::<Value>().expect("json"))
            .map_err(|e| e.as_str().unwrap_or_default().to_string())
    }

    fn warnings_of(&self, node: NodeId) -> Value {
        self.call("get_render_warnings", json!({ "node": node.to_hex() }))
            .expect("get_render_warnings")
    }

    fn export(&self, node: NodeId, name: &str) -> (Value, std::path::PathBuf) {
        let out = self.dir.path().join(name);
        let answer = self
            .call(
                "render_range",
                json!({ "nodeId": node.to_hex(), "startSec": 0.0, "endSec": 1.0, "outPath": out }),
            )
            .expect("render_range");
        (answer, out)
    }
}

/// The stored reverb on a track and the one on the master are each
/// reported, in signal order, with where they are and what they are.
#[test]
fn a_stored_reverb_on_a_track_and_on_the_master_is_reported() {
    let p = Project::open();

    assert_eq!(
        p.warnings_of(p.nodes.stored),
        json!([
            {
                "scope": { "type": "track", "index": 0, "name": p.track_name },
                // Counted in the stored chain, after the gain.
                "effect_index": 1,
                "kind": "reverb",
                "reason": "not_yet_streamable",
            },
            {
                "scope": { "type": "master" },
                "effect_index": 0,
                "kind": "reverb",
                "reason": "not_yet_streamable",
            },
        ])
    );
}

/// With none stored there is nothing to say.
#[test]
fn a_clean_session_has_no_warnings() {
    let p = Project::open();
    assert_eq!(p.warnings_of(p.nodes.clean), json!([]));
}

/// The warning is about the node asked for, so it clears when the head
/// moves to a state without the entry: bypassed (left out on purpose) or
/// removed.
#[test]
fn bypassing_or_removing_the_effect_clears_it() {
    let p = Project::open();
    assert_eq!(p.warnings_of(p.nodes.stored).as_array().unwrap().len(), 2);
    assert_eq!(p.warnings_of(p.nodes.bypassed), json!([]));
    assert_eq!(p.warnings_of(p.nodes.removed), json!([]));
}

/// A bus is named by its name, and a kind nobody knows says so.
#[test]
fn a_bus_and_an_unknown_kind_are_reported_for_what_they_are() {
    let p = Project::open();
    assert_eq!(
        p.warnings_of(p.nodes.bus_and_unknown),
        json!([
            {
                "scope": { "type": "bus", "name": "Verb" },
                "effect_index": 0,
                "kind": "echo",
                "reason": "not_yet_streamable",
            },
            {
                "scope": { "type": "master" },
                "effect_index": 0,
                "kind": "wobbulator",
                "reason": "unknown",
            },
        ])
    );
}

/// A node that is not in the store, or not an id at all, is refused like
/// every other command that takes one.
#[test]
fn a_node_that_does_not_exist_is_refused() {
    let p = Project::open();
    let err = p
        .call("get_render_warnings", json!({ "node": "not-hex" }))
        .expect_err("not an id");
    assert!(err.contains("not-hex"), "names the id: {err}");
    let missing = NodeId::from_hex(&"0".repeat(64)).unwrap();
    assert!(p
        .call("get_render_warnings", json!({ "node": missing.to_hex() }))
        .is_err());
}

/// Export still writes the file, and now says what it left out.
#[test]
fn an_export_names_what_it_left_out() {
    let p = Project::open();

    let (answer, out) = p.export(p.nodes.stored, "stored.wav");
    assert!(out.is_file(), "the selection is written all the same");
    assert_eq!(
        answer["warnings"],
        p.warnings_of(p.nodes.stored),
        "the export reports what the window already shows for that node: {answer}"
    );
    assert_eq!(answer["warnings"].as_array().unwrap().len(), 2);
    // What it returned before is still there.
    assert_eq!(answer["path"], json!(out));
    assert!(answer["frames_written"].as_u64().unwrap() > 0);
}

/// With none stored, the export carries an empty list: the same shape.
#[test]
fn an_export_of_a_clean_session_has_an_empty_list() {
    let p = Project::open();
    let (answer, out) = p.export(p.nodes.clean, "clean.wav");
    assert!(out.is_file());
    assert_eq!(answer["warnings"], json!([]));
}

/// A/B renders both sides; each side reports its own, so a B that removed
/// the reverb does not inherit A's warning.
#[test]
fn a_compare_reports_each_side_on_its_own() {
    let p = Project::open();
    let answer = p
        .call(
            "prepare_compare",
            json!({ "a": p.nodes.stored.to_hex(), "b": p.nodes.removed.to_hex() }),
        )
        .expect("prepare_compare");

    assert!(answer["a_path"].as_str().is_some(), "{answer}");
    assert!(answer["b_path"].as_str().is_some(), "{answer}");
    assert_eq!(answer["a_warnings"], p.warnings_of(p.nodes.stored));
    assert_eq!(answer["a_warnings"].as_array().unwrap().len(), 2);
    assert_eq!(answer["b_warnings"], json!([]));
}

/// The preview render is unchanged: a bare path, for a session holding an
/// entry the renderer leaves out. The warnings come from
/// `get_render_warnings`, so no caller of this command had to change.
#[test]
fn the_preview_render_still_answers_a_path() {
    let p = Project::open();
    let answer = p
        .call("render_preview", json!({ "node": p.nodes.stored.to_hex() }))
        .expect("render_preview");
    let path = answer.as_str().expect("a path, not an object");
    assert!(std::path::Path::new(path).is_file(), "{path}");
}
