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
//! And the copy is a project of its own (#612). Save As opens the copy, and
//! every open sweeps `derived/` of the audio no node names. The copy's node
//! files are the original's, naming the original's files, which are still
//! there; read with those paths, the copy's own audio was named by nothing
//! and the open deleted it, leaving the copy playing from the original's
//! folder. Deleting the original, the usual second half of moving a
//! project, then cost the copy every edit no replay can redo. The tests
//! after the first make that history with a paste, which is one, and check
//! that the copy plays every clip once the original (and, for a copy of the
//! copy, the copy before it) is deleted.
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

// ---------------------------------------------------------------------------
// The copy plays on its own (#612)
// ---------------------------------------------------------------------------

/// A window with the commands these tests use.
fn window() -> (tauri::App<MockRuntime>, WebviewWindow<MockRuntime>) {
    let app = mock_builder()
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            commands::open_project,
            commands::save_project_as,
            commands::render_preview,
            commands::get_session_head,
            commands::move_clip,
        ])
        .build(mock_context(noop_assets()))
        .expect("mock app");
    let webview = WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .expect("webview");
    (app, webview)
}

/// A project made by the agent's tools, and the user's takes outside it.
///
/// Two takes, loaded from where the user keeps them. The first is edited
/// twice (`silence_region`), so its first edit's audio is history only and
/// its second's is the head's. A stretch of it is copied and pasted into
/// the second take, which no replay can redo (`paste_region` reads the
/// clipboard), so nothing can rebuild that audio if it is lost. The head
/// plays both edits' outputs and nothing of the takes but through them,
/// so a third take, left untouched, is the head's user file in place. Last,
/// a generated tone, which lives in `<project>/generated/`, outside the
/// store.
struct History {
    takes: tempfile::TempDir,
    project: tempfile::TempDir,
    head: session::NodeId,
}

impl History {
    fn new() -> Self {
        let takes = tempfile::tempdir().expect("tempdir");
        let project = tempfile::tempdir().expect("tempdir");
        let mut store = session::Store::open(project.path()).expect("store");
        let mut clipboard = None;
        for i in 0..3 {
            let take = takes.path().join(format!("take-{i}.wav"));
            write_wav(&take, 16_000 + 4_000 * i);
            tool(&mut store, &mut clipboard, "load", json!({ "path": take }));
        }
        for (name, args) in [
            (
                "silence_region",
                json!({ "track": 0, "start_sec": 0.2, "end_sec": 0.4 }),
            ),
            (
                "copy_region",
                json!({ "track": 0, "range": { "start_sec": 0.5, "end_sec": 0.9 } }),
            ),
            ("paste_region", json!({ "track": 1, "at": 1.0 })),
            (
                "silence_region",
                json!({ "track": 0, "start_sec": 1.2, "end_sec": 1.4 }),
            ),
            (
                "generate_tone",
                json!({ "frequency_hz": 660.0, "duration_sec": 0.5 }),
            ),
        ] {
            tool(&mut store, &mut clipboard, name, args);
        }
        let head = store.head().expect("a head");
        Self {
            takes,
            project,
            head,
        }
    }

    fn src(&self) -> &Path {
        self.project.path()
    }
}

fn derived(project: &Path) -> PathBuf {
    project.join(session::STORE_DIR).join("derived")
}

/// The names of the files in `project`'s `derived/`.
fn derived_names(project: &Path) -> Vec<PathBuf> {
    files_under(&derived(project))
}

/// Every file under `dir`, with its bytes.
fn contents(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    files_under(dir)
        .into_iter()
        .map(|rel| {
            let bytes = std::fs::read(dir.join(&rel)).expect("read");
            (rel, bytes)
        })
        .collect()
}

/// `node`, rendered by the window's own command, as the bytes it came out
/// as.
fn render(webview: &WebviewWindow<MockRuntime>, node: session::NodeId) -> Vec<u8> {
    let preview = call(webview, "render_preview", json!({ "node": node.to_hex() }))
        .unwrap_or_else(|e| panic!("node {} does not render: {e}", node.to_hex()));
    std::fs::read(preview.as_str().expect("a path")).expect("the preview")
}

/// `project` names its own `derived/`, `clipboard/` and `generated/` files
/// and the user's takes, in every node, as every reader of it sees them.
fn assert_names_only_its_own(project: &Path, takes: &Path) {
    let store = session::Store::open(project).expect("store");
    for node in store.list_nodes().expect("nodes") {
        for clip in node.state.tracks.iter().flat_map(|t| &t.clips) {
            let path = &clip.source_path;
            assert!(
                path.starts_with(derived(project))
                    || path.starts_with(project.join(session::STORE_DIR).join("clipboard"))
                    || path.starts_with(project.join("generated"))
                    || path.starts_with(takes),
                "node {} of {} names {}",
                node.id.to_hex(),
                project.display(),
                path.display()
            );
        }
    }
}

/// #612 as a user meets it: Save As, then the original deleted. The copy's
/// head plays every clip, the pasted one included, from the copy's files.
#[test]
fn a_copy_plays_every_clip_once_the_original_is_deleted() {
    let history = History::new();
    let copies = tempfile::tempdir().expect("tempdir");
    let dest = copies.path().join("copy");
    let (_app, webview) = window();
    call(&webview, "open_project", json!({ "path": history.src() })).expect("open_project");
    let before = render(&webview, history.head);
    let audio = derived_names(history.src());
    assert_eq!(audio.len(), 3, "two silences and a paste: {audio:?}");

    call(&webview, "save_project_as", json!({ "dest": dest })).expect("save_project_as");
    std::fs::remove_dir_all(history.src()).expect("the original is deleted");

    assert!(
        render(&webview, history.head) == before,
        "the copy's head renders differently from the original's"
    );
    for file in &audio {
        assert!(
            derived_names(&dest).contains(file),
            "the copy lost {}: {:?}",
            file.display(),
            derived_names(&dest)
        );
    }
}

/// The copy names only its own audio and the user's takes, opening it
/// leaves the original exactly as it was, and the background reclaimer,
/// over its cap, takes none of the copy's audio for an orphan and keeps
/// every file the head plays.
#[test]
fn a_copy_names_its_own_audio_and_leaves_the_original_alone() {
    use tauri::Manager;
    let history = History::new();
    let copies = tempfile::tempdir().expect("tempdir");
    let dest = copies.path().join("copy");
    let (app, webview) = window();
    call(&webview, "open_project", json!({ "path": history.src() })).expect("open_project");
    let original = contents(history.src());

    call(&webview, "save_project_as", json!({ "dest": dest })).expect("save_project_as");

    assert_eq!(
        derived_names(&dest),
        derived_names(history.src()),
        "the copy's open swept its own audio"
    );
    assert_names_only_its_own(&dest, history.takes.path());
    assert!(
        contents(history.src()) == original,
        "opening the copy changed the original"
    );

    // Over a cap of nothing it frees all it may: the first silence's audio,
    // which only history names and a replay rebuilds.
    let report = edytlab_desktop_lib::reclaimer::reclaim_once(&app.state::<AppState>(), 0)
        .expect("over its cap, the reclaimer sweeps");
    assert_eq!(
        report.removed_orphans, 0,
        "the reclaimer took the copy's audio for orphans: {report:?}"
    );
    assert_eq!(report.removed_files, 1, "{report:?}");
    let reader = session::Store::open(&dest).expect("store");
    assert_eq!(
        tools::rederive::missing_paths(&reader, history.head),
        Vec::<PathBuf>::new(),
        "the reclaimer took audio the head plays"
    );
    assert!(
        contents(history.src()) == original,
        "the reclaimer changed the original"
    );
}

/// Save As from the copy, after an edit made in it: the second copy names
/// its own audio whether a node was written in the original or in the
/// first copy, and plays both nodes once both are deleted.
#[test]
fn a_copy_of_the_copy_plays_once_both_before_it_are_deleted() {
    let history = History::new();
    let copies = tempfile::tempdir().expect("tempdir");
    let first = copies.path().join("first");
    let second = copies.path().join("second");
    let (_app, webview) = window();
    call(&webview, "open_project", json!({ "path": history.src() })).expect("open_project");
    call(&webview, "save_project_as", json!({ "dest": first })).expect("save_project_as");

    // An edit in the first copy, written there.
    call(
        &webview,
        "move_clip",
        json!({ "track": 2, "clip": 0, "startSec": 0.25 }),
    )
    .expect("move_clip");
    let moved = session::NodeId::from_hex(
        call(&webview, "get_session_head", json!({}))
            .expect("head")
            .as_str()
            .expect("hex"),
    )
    .expect("an id");
    assert_ne!(moved, history.head);
    let renders = [
        (history.head, render(&webview, history.head)),
        (moved, render(&webview, moved)),
    ];

    call(&webview, "save_project_as", json!({ "dest": second })).expect("save_project_as");
    assert_names_only_its_own(&second, history.takes.path());
    std::fs::remove_dir_all(history.src()).expect("the original is deleted");
    std::fs::remove_dir_all(&first).expect("the first copy is deleted");

    for (node, before) in renders {
        assert!(
            render(&webview, node) == before,
            "node {} renders differently in the second copy",
            node.to_hex()
        );
    }
    assert_names_only_its_own(&second, history.takes.path());
}

/// A project folder copied by hand, as a file manager does, and opened: the
/// same as Save As, since the open is what sweeps.
#[test]
fn a_folder_copied_by_hand_keeps_its_audio() {
    fn copy_folder(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for entry in std::fs::read_dir(from).unwrap().flatten() {
            let target = to.join(entry.file_name());
            if entry.path().is_dir() {
                copy_folder(&entry.path(), &target);
            } else {
                std::fs::copy(entry.path(), target).unwrap();
            }
        }
    }
    let history = History::new();
    let copies = tempfile::tempdir().expect("tempdir");
    let dest = copies.path().join("copied by hand");
    copy_folder(history.src(), &dest);
    let (_app, webview) = window();
    call(&webview, "open_project", json!({ "path": history.src() })).expect("open_project");
    let before = render(&webview, history.head);

    call(&webview, "open_project", json!({ "path": dest })).expect("open the copy");
    std::fs::remove_dir_all(history.src()).expect("the original is deleted");

    assert!(
        render(&webview, history.head) == before,
        "the copy's head renders differently from the original's"
    );
    assert_names_only_its_own(&dest, history.takes.path());
}

/// A copy saved by 0.6.0: its open swept its whole `derived/`, and it has
/// played from the original's folder since. Opening it with the original
/// still there copies that audio back into it, after which the original
/// can be deleted and the copy plays every clip.
#[test]
fn opening_a_copy_saved_by_0_6_0_brings_its_audio_home() {
    fn copy_folder(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for entry in std::fs::read_dir(from).unwrap().flatten() {
            let target = to.join(entry.file_name());
            if entry.path().is_dir() {
                copy_folder(&entry.path(), &target);
            } else {
                std::fs::copy(entry.path(), target).unwrap();
            }
        }
    }
    let history = History::new();
    let copies = tempfile::tempdir().expect("tempdir");
    let dest = copies.path().join("saved by 0.6.0");
    copy_folder(history.src(), &dest);
    for name in derived_names(&dest) {
        std::fs::remove_file(derived(&dest).join(name)).unwrap();
    }
    let (_app, webview) = window();
    call(&webview, "open_project", json!({ "path": history.src() })).expect("open_project");
    let before = render(&webview, history.head);
    let audio = derived_names(history.src());

    call(&webview, "open_project", json!({ "path": dest })).expect("open the copy");

    // The copy runs beside the window; the open does not wait for it.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while derived_names(&dest) != audio {
        assert!(
            std::time::Instant::now() < deadline,
            "the copy's audio did not come home: {:?}, wanted {audio:?}",
            derived_names(&dest)
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    std::fs::remove_dir_all(history.src()).expect("the original is deleted");

    assert!(
        render(&webview, history.head) == before,
        "the copy's head renders differently from the original's"
    );
    assert_names_only_its_own(&dest, history.takes.path());
}
