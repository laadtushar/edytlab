//! A/B compare renders into the bounded preview cache and nowhere else
//! (#575).
//!
//! `prepare_compare` used to render the whole of both nodes to
//! `<OS temp dir>/edytlab-compare-{a,b}-<node>.wav`: about 110 MB for a
//! five-minute stereo project, a new pair for every new pair of nodes,
//! and nothing ever removed them. The window's `render_preview` already
//! rendered the very same mix into `.audiograph/previews/`, keyed by the
//! node, so each side of a compare is that cache's entry for its node.
//!
//! The cache evicts its least recently used entries, and the A/B bar reads
//! both files again on every switch, so the pair is held for as long as
//! the bar is open: eviction leaves it alone until `end_compare`. A held
//! pair is outside the cap, so a long session whose sides do not fit in it
//! together can still be compared, and once the compare ends the cache is
//! back under its cap.
//!
//! The claim is about where files end up, so each test points the OS temp
//! directory at an empty folder of its own and looks at it afterwards. The
//! environment is process-wide, so every test takes [`Sandbox`], which
//! holds a lock for as long as it is alive.
//!
//! Windows: gated like `commands_mock.rs`, for the same reason (Wry's
//! WebView2 imports).

#![cfg(not(target_os = "windows"))]

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use edytlab_desktop_lib::commands;
use edytlab_desktop_lib::state::AppState;
use serde_json::{json, Value};
use session::NodeId;
use tauri::ipc::{CallbackFn, InvokeBody};
use tauri::test::{mock_builder, mock_context, noop_assets, MockRuntime, INVOKE_KEY};
use tauri::webview::InvokeRequest;
use tauri::WebviewWindowBuilder;

static ENV: Mutex<()> = Mutex::new(());

/// The variables `std::env::temp_dir` reads, here and on Windows.
const TMP_VARS: [&str; 3] = ["TMPDIR", "TMP", "TEMP"];
const CAP_VAR: &str = tools::preview_cache::CAP_ENV;

/// mtime is the cache's LRU clock, and its resolution is coarse on some
/// filesystems: a gap between renders makes the order one the test chose.
const TICK: Duration = Duration::from_millis(20);

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

/// A mono 16-bit WAV of `frames` frames at 8 kHz, every sample 1000.
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
    for _ in 0..frames {
        bytes.extend_from_slice(&1_000i16.to_le_bytes());
    }
    std::fs::write(path, bytes).expect("write wav");
}

/// A project with a load and five edits after it, opened over IPC, and an
/// OS temp directory of its own, empty.
///
/// Every node is the same length, so every whole-mix render is the same
/// size, which lets a test set the cache's cap in renders.
struct Sandbox {
    _lock: MutexGuard<'static, ()>,
    _root: tempfile::TempDir,
    project: PathBuf,
    os_tmp: PathBuf,
    saved: Vec<(&'static str, Option<std::ffi::OsString>)>,
    /// Oldest first: the load, then each edit.
    nodes: Vec<NodeId>,
    webview: tauri::WebviewWindow<MockRuntime>,
    _app: tauri::App<MockRuntime>,
}

impl Sandbox {
    fn new() -> Self {
        let lock = ENV.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        // Made before the OS temp dir is pointed into it.
        let root = tempfile::tempdir().expect("tempdir");
        let project = root.path().join("project");
        let os_tmp = root.path().join("os-tmp");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(&os_tmp).unwrap();

        let mut saved: Vec<_> = TMP_VARS.iter().map(|v| (*v, std::env::var_os(v))).collect();
        saved.push((CAP_VAR, std::env::var_os(CAP_VAR)));
        for var in TMP_VARS {
            std::env::set_var(var, &os_tmp);
        }
        std::env::remove_var(CAP_VAR);

        let take = project.join("take.wav");
        write_wav(&take, 16_000);
        let nodes = {
            let mut store = session::Store::open(&project).expect("store");
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
            let mut run = |tool: &str, args: Value, ctx: &mut tools::ToolContext| {
                let result = dispatcher.invoke(tool, args, ctx).expect("invoke");
                match result {
                    tools::ToolResult::Ok(_) => nodes.push(ctx.store.head().expect("head")),
                    tools::ToolResult::Error(m) => panic!("{tool} failed: {m}"),
                }
            };
            run("load", json!({ "path": take }), &mut ctx);
            for start in [0.0, 0.3, 0.6, 0.9, 1.2] {
                run(
                    "silence_region",
                    json!({ "track": 0, "start_sec": start, "end_sec": start + 0.1 }),
                    &mut ctx,
                );
            }
            nodes
        };

        let app = mock_builder()
            .manage(AppState::new())
            .invoke_handler(tauri::generate_handler![
                commands::open_project,
                commands::render_preview,
                commands::prepare_compare,
                commands::end_compare,
            ])
            .build(mock_context(noop_assets()))
            .expect("mock app");
        let webview = WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .expect("webview");
        let s = Self {
            _lock: lock,
            _root: root,
            project,
            os_tmp,
            saved,
            nodes,
            webview,
            _app: app,
        };
        s.call("open_project", json!({ "path": s.project }))
            .expect("open_project");
        s
    }

    fn call(&self, cmd: &str, body: Value) -> Result<Value, String> {
        tauri::test::get_ipc_response(&self.webview, make_request(cmd, body))
            .map(|r| r.deserialize::<Value>().expect("json"))
            .map_err(|e| e.as_str().unwrap_or_default().to_string())
    }

    fn compare(&self, a: NodeId, b: NodeId) -> Result<Value, String> {
        self.call(
            "prepare_compare",
            json!({ "a": a.to_hex(), "b": b.to_hex() }),
        )
    }

    fn preview(&self, node: NodeId) -> Result<PathBuf, String> {
        self.call("render_preview", json!({ "node": node.to_hex() }))
            .map(|v| PathBuf::from(v.as_str().expect("a path")))
    }

    /// A render by the agent's `render_preview` tool, into the same cache:
    /// an ordinary entry that holds nothing. The window's own
    /// `render_preview` holds the entry it returns (#607), so a test about
    /// what a compare's hold does to the entries around it renders those
    /// with this.
    fn agent_render(&self, node: NodeId) -> PathBuf {
        let mut store = session::Store::open(&self.project).expect("store");
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
            .invoke(
                "render_preview",
                json!({ "node_id": node.to_hex() }),
                &mut ctx,
            )
            .expect("invoke")
        {
            tools::ToolResult::Ok(v) => {
                PathBuf::from(v["path"].as_str().expect("the tool answers a path"))
            }
            tools::ToolResult::Error(m) => panic!("render_preview failed: {m}"),
        }
    }

    fn end_compare(&self, answer: &Value) {
        let id = answer["compare_id"].clone();
        assert!(id.is_u64(), "prepare_compare names the compare: {answer}");
        assert_eq!(
            self.call("end_compare", json!({ "compareId": id })),
            Ok(Value::Null)
        );
    }

    fn previews(&self) -> PathBuf {
        self.project.join(".audiograph").join("previews")
    }

    /// Where the cache keeps the whole mix of `node`.
    fn entry(&self, node: NodeId) -> PathBuf {
        self.previews().join(format!("{}.wav", node.to_hex()))
    }

    /// The size of one whole-mix render, from a render made for it.
    fn render_bytes(&self) -> u64 {
        let path = self.preview(self.nodes[0]).expect("render_preview");
        std::fs::metadata(path).expect("rendered").len()
    }

    fn cap(&self, bytes: u64) {
        std::env::set_var(CAP_VAR, bytes.to_string());
    }

    /// Everything in the OS temp directory, by name.
    fn os_tmp_files(&self) -> Vec<String> {
        names_in(&self.os_tmp)
    }

    /// Anything in the cache folder that is not a finished entry.
    fn staging_files(&self) -> Vec<String> {
        names_in(&self.previews())
            .into_iter()
            .filter(|n| !n.ends_with(".wav"))
            .collect()
    }

    /// Bytes on disk for the whole-mix entries of `nodes` that are there.
    fn bytes_of(&self, nodes: &[NodeId]) -> u64 {
        nodes
            .iter()
            .filter_map(|n| std::fs::metadata(self.entry(*n)).ok())
            .map(|m| m.len())
            .sum()
    }

    fn cached_bytes(&self) -> u64 {
        names_in(&self.previews())
            .iter()
            .filter(|n| n.ends_with(".wav"))
            .map(|n| std::fs::metadata(self.previews().join(n)).unwrap().len())
            .sum()
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        for (var, value) in &self.saved {
            match value {
                Some(v) => std::env::set_var(var, v),
                None => std::env::remove_var(var),
            }
        }
    }
}

fn names_in(dir: &Path) -> Vec<String> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = read
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

fn path(answer: &Value, key: &str) -> PathBuf {
    PathBuf::from(
        answer[key]
            .as_str()
            .unwrap_or_else(|| panic!("no {key}: {answer}")),
    )
}

/// Bytes no render writes, so finding them again proves nothing rendered.
const MARK: &[u8] = b"RIFF-not-a-render: left here by the test";

// ---------------------------------------------------------------------
// Where the renders go
// ---------------------------------------------------------------------

/// The issue's first acceptance item: nothing in the OS temp directory,
/// and both sides inside `<project>/.audiograph/previews/`, each under its
/// node's own name.
#[test]
fn a_compare_renders_into_the_preview_cache_and_nowhere_else() {
    let s = Sandbox::new();
    let (a, b) = (s.nodes[1], s.nodes[2]);

    let answer = s.compare(a, b).expect("prepare_compare");

    assert_eq!(
        s.os_tmp_files(),
        Vec::<String>::new(),
        "a compare left files in the OS temp directory"
    );
    assert_eq!(
        path(&answer, "a_path"),
        s.entry(a),
        "A is not A's cache entry"
    );
    assert_eq!(
        path(&answer, "b_path"),
        s.entry(b),
        "B is not B's cache entry"
    );
    assert!(s.entry(a).is_file() && s.entry(b).is_file());
    assert_eq!(s.staging_files(), Vec::<String>::new());
}

/// The second: comparing the same pair again serves both sides from the
/// cache. Each entry is overwritten with bytes no render writes; a render
/// would replace them.
#[test]
fn comparing_the_same_pair_again_renders_nothing() {
    let s = Sandbox::new();
    let (a, b) = (s.nodes[1], s.nodes[2]);
    let first = s.compare(a, b).expect("prepare_compare");
    for side in ["a_path", "b_path"] {
        std::fs::write(path(&first, side), MARK).expect("mark the entry");
    }

    let second = s.compare(a, b).expect("prepare_compare again");

    assert_eq!(second["a_path"], first["a_path"]);
    assert_eq!(second["b_path"], first["b_path"]);
    for side in ["a_path", "b_path"] {
        assert!(
            std::fs::read(path(&second, side)).unwrap() == MARK,
            "{side} was rendered again for an unchanged session"
        );
    }
    assert_eq!(s.os_tmp_files(), Vec::<String>::new());
}

/// And the A side is the render the window already made for that node with
/// `render_preview`: the head is usually A, and its mix is usually what was
/// just played.
#[test]
fn the_a_side_is_the_preview_already_rendered_for_that_node() {
    let s = Sandbox::new();
    let (a, b) = (s.nodes[3], s.nodes[4]);
    let played = s.preview(a).expect("render_preview");
    std::fs::write(&played, MARK).expect("mark the preview");

    let answer = s.compare(a, b).expect("prepare_compare");

    assert_eq!(path(&answer, "a_path"), played);
    assert!(
        std::fs::read(&played).unwrap() == MARK,
        "the A side was rendered again although its preview was cached"
    );
}

/// The third: with no bounded place to put the renders, the compare fails.
/// It used to have no need of one.
#[test]
fn a_cache_that_cannot_be_written_is_an_error_not_a_temp_file() {
    let s = Sandbox::new();
    std::fs::create_dir_all(s.previews().parent().unwrap()).unwrap();
    // A file where the folder should be: the one way to make a cache
    // unwritable that a test running as root cannot get around.
    std::fs::write(s.previews(), b"in the way").unwrap();

    let result = s.compare(s.nodes[1], s.nodes[2]);

    assert!(result.is_err(), "expected an error, got {result:?}");
    assert_eq!(
        s.os_tmp_files(),
        Vec::<String>::new(),
        "an unwritable cache sent the renders to the OS temp directory"
    );
}

/// A compare whose render fails leaves nothing behind, in the cache or out
/// of it. Here B's audio was swept from history and its source is gone, so
/// it cannot be rebuilt. A is the head, whose audio a sweep never takes.
#[test]
fn a_compare_that_cannot_render_leaves_nothing_behind() {
    let s = Sandbox::new();
    let (a, b) = (s.nodes[5], s.nodes[1]);
    {
        let store = session::Store::open(&s.project).expect("store");
        tools::reclaim::sweep(&store, 0).expect("sweep");
        assert!(
            !tools::rederive::missing_paths(&store, b).is_empty(),
            "the sweep removed B's audio"
        );
    }
    std::fs::remove_file(s.project.join("take.wav")).expect("remove the take");

    let result = s.compare(a, b);

    assert!(result.is_err(), "expected an error, got {result:?}");
    assert!(!s.entry(b).exists(), "a side that failed was cached");
    assert_eq!(s.staging_files(), Vec::<String>::new());
    assert_eq!(s.os_tmp_files(), Vec::<String>::new());
}

// ---------------------------------------------------------------------
// The pair is held while it is being compared, outside the cap
// ---------------------------------------------------------------------

/// The A/B bar loads a side from its file on every switch, so a side the
/// cache evicts while the bar is open is a switch that fails. The pair is
/// the cache's least recently used entries here, so without the hold the
/// first eviction takes A.
#[test]
fn the_pair_being_compared_outlives_renders_that_evict() {
    let s = Sandbox::new();
    let (a, b) = (s.nodes[1], s.nodes[2]);
    let one = s.render_bytes();
    std::fs::remove_file(s.entry(s.nodes[0])).unwrap();
    // Room for one render and a half.
    let cap = one * 3 / 2;
    s.cap(cap);

    let _answer = s.compare(a, b).expect("prepare_compare");
    for node in [s.nodes[3], s.nodes[4]] {
        std::thread::sleep(TICK);
        s.agent_render(node);
    }

    assert!(s.entry(a).is_file(), "A was evicted while it was compared");
    assert!(s.entry(b).is_file(), "B was evicted while it was compared");
    assert!(
        !s.entry(s.nodes[3]).exists(),
        "the render evicted should be the oldest one not being compared"
    );
    assert!(
        s.bytes_of(&[s.nodes[3], s.nodes[4]]) <= cap,
        "the renders not being compared are over the cap"
    );
}

/// The pair is outside the cap, so a render made while the bar is open is
/// judged on its own: it fits if it fits the cap alone, evicting only
/// renders that are not being compared.
#[test]
fn a_render_beside_the_pair_fits_because_the_pair_is_outside_the_cap() {
    let s = Sandbox::new();
    let (a, b) = (s.nodes[1], s.nodes[2]);
    let one = s.render_bytes();
    std::fs::remove_file(s.entry(s.nodes[0])).unwrap();
    s.cap(one * 3 / 2);
    let _answer = s.compare(a, b).expect("prepare_compare");

    let c = s.preview(s.nodes[3]).expect("fits the cap on its own");

    assert!(c.is_file());
    assert!(s.entry(a).is_file() && s.entry(b).is_file());
    assert_eq!(s.cached_bytes(), one * 3, "over the cap by the pair");
    assert_eq!(s.staging_files(), Vec::<String>::new());
}

/// A long session (an hour-long mix) can be compared although its two
/// sides do not fit in the cap together: the pair is held outside it.
#[test]
fn a_pair_bigger_than_the_cap_can_still_be_compared() {
    let s = Sandbox::new();
    let one = s.render_bytes();
    s.cap(one * 3 / 2);
    let (a, b) = (s.nodes[1], s.nodes[2]);

    let answer = s.compare(a, b).expect("the pair is outside the cap");

    assert_eq!(path(&answer, "a_path"), s.entry(a));
    assert_eq!(path(&answer, "b_path"), s.entry(b));
    assert!(
        s.entry(a).is_file() && s.entry(b).is_file(),
        "a side was evicted as the other was rendered"
    );
    assert_eq!(s.os_tmp_files(), Vec::<String>::new());
}

/// Once the bar closes, the pair counts against the cap again, and the
/// cache is back under it straight away: least recently used first, which
/// is the pair.
#[test]
fn ending_the_compare_brings_the_cache_back_under_its_cap() {
    let s = Sandbox::new();
    let (a, b) = (s.nodes[1], s.nodes[2]);
    let one = s.render_bytes();
    std::fs::remove_file(s.entry(s.nodes[0])).unwrap();
    let cap = one * 3 / 2;
    s.cap(cap);
    let answer = s.compare(a, b).expect("prepare_compare");
    std::thread::sleep(TICK);
    let later = s.agent_render(s.nodes[3]);
    assert!(
        s.cached_bytes() > cap,
        "the pair puts the cache over its cap"
    );

    s.end_compare(&answer);

    assert!(
        s.cached_bytes() <= cap,
        "still over the cap once the compare ended"
    );
    assert!(!s.entry(a).exists() && !s.entry(b).exists());
    assert!(later.is_file(), "the newest render should have been kept");
}

/// Holding the pair does not get round the one refusal there is: a side
/// bigger than the cap on its own is an error, as it is for
/// `render_preview`, and not a path to a file that is already gone.
#[test]
fn a_side_bigger_than_the_cap_is_an_error_not_a_dangling_path() {
    let s = Sandbox::new();
    let one = s.render_bytes();
    s.cap(one / 2);

    let err = s
        .compare(s.nodes[1], s.nodes[2])
        .expect_err("a side does not fit the cap");

    assert!(err.contains("cap"), "should say why: {err}");
    assert!(!s.entry(s.nodes[1]).exists() && !s.entry(s.nodes[2]).exists());
    assert_eq!(s.staging_files(), Vec::<String>::new());
    assert_eq!(s.os_tmp_files(), Vec::<String>::new());
}
