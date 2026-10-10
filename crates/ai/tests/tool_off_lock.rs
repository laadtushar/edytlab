//! A long tool leaves the store readable, and a user's edit made while it
//! runs is kept (#421).
//!
//! The agent used to lock the dispatcher, the store, the engine and the
//! clipboard, run the tool, and release them when it returned. A
//! time-stretch takes ten seconds in a debug build, and for all of them
//! the app's reads of the session — `list_tracks`, the markers, undo —
//! waited on the store's lock, and the window with them: in the native
//! end-to-end run, a `list_tracks` issued two seconds into one returned at
//! nine.
//!
//! These drive the public `Agent` API against a mocked provider, on a
//! multi-threaded runtime, with a fixture tool that holds still in the
//! middle of its work. A reader thread plays the window: it waits for the
//! tool to start, tries to take the store's lock, and, if it can, reads the
//! head and adds a marker, as the app's commands do.

mod common;

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ai::{Agent, AgentEvent, LlmConfig};
use common::{classifier_json, ok, sse_text, sse_tool_step, Entry, SeqResponder};
use serde_json::{json, Value};
use session::{Annotation, AnnotationId, AnnotationKind, NodeId, Store};
use tokio::sync::Notify;
use tools::schema::{anthropic_tool, object_schema};
use tools::{Tool, ToolContext, ToolDispatcher, ToolResult};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer};

fn marker(name: &str) -> Annotation {
    Annotation {
        id: AnnotationId::new(),
        name: name.to_string(),
        kind: AnnotationKind::Marker { time_sec: 1.0 },
    }
}

/// A session with nothing in it, built through its JSON form so this test
/// does not need a clock crate for `created_at`.
fn base_node() -> session::SessionNode {
    let state = session::SessionState {
        tracks: Vec::new(),
        bus_routing: session::BusGraph::default(),
        master_chain: Vec::new(),
        tempo_map: session::TempoMap::default(),
        key_map: None,
        transcript: None,
        sample_rate: 48_000,
        length_samples: 0,
        annotations: Vec::new(),
        sync_lock: false,
    };
    serde_json::from_value(json!({
        "id": "00".repeat(32),
        "parent": null,
        "created_at": "2026-01-01T00:00:00Z",
        "label": null,
        "reasoning": null,
        "state": serde_json::to_value(state).expect("state"),
        "op": null,
    }))
    .expect("a node")
}

/// An edit that holds still in the middle of its first run: it says it has
/// started, and waits to be released. Then it appends a marker, as a real
/// tool appends a node.
struct SlowEdit {
    off_lock: bool,
    started: Mutex<Sender<()>>,
    release: Mutex<Receiver<()>>,
    runs: Arc<AtomicUsize>,
}

impl Tool for SlowEdit {
    fn name(&self) -> &'static str {
        "slow_edit"
    }
    fn schema(&self) -> Value {
        anthropic_tool("slow_edit", "A long edit.", object_schema(&[]))
    }
    fn runs_off_the_lock(&self) -> bool {
        self.off_lock
    }
    fn invoke(&self, _args: Value, ctx: &mut ToolContext) -> tools::Result<ToolResult> {
        let run = self.runs.fetch_add(1, Ordering::SeqCst) + 1;
        if run == 1 {
            self.started.lock().unwrap().send(()).ok();
            // Bounded, so a regression fails instead of hanging.
            self.release
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(10))
                .ok();
        }
        let head = ctx.store.head().expect("a head");
        let id = ctx
            .store
            .add_annotation(head, marker("from-tool"))
            .expect("append");
        Ok(ToolResult::Ok(json!({ "node_id": id.to_hex() })))
    }
}

/// What the reader thread found.
#[derive(Debug)]
struct Reading {
    /// It could not take the store's lock within the bound.
    blocked: bool,
    /// The head it read, and whether that node could be read.
    head: Option<NodeId>,
    head_readable: bool,
    /// The marker it added, as a command would.
    user_edit: Option<NodeId>,
}

/// Wait for the tool to start, then try to use the store the way the
/// app's commands do. Always releases the tool afterwards.
fn spawn_reader(
    store: Arc<Mutex<Store>>,
    started: Receiver<()>,
    release: Sender<()>,
) -> std::thread::JoinHandle<Reading> {
    std::thread::spawn(move || {
        let mut reading = Reading {
            blocked: true,
            head: None,
            head_readable: false,
            user_edit: None,
        };
        if started.recv_timeout(Duration::from_secs(10)).is_ok() {
            let deadline = Instant::now() + Duration::from_secs(2);
            while Instant::now() < deadline {
                if let Ok(mut s) = store.try_lock() {
                    reading.blocked = false;
                    reading.head = s.head();
                    reading.head_readable = reading.head.is_some_and(|h| s.get(h).is_ok());
                    if let Some(h) = reading.head {
                        reading.user_edit = s.add_annotation(h, marker("from-user")).ok();
                    }
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        release.send(()).ok();
        reading
    })
}

struct Outcome {
    result: ai::Result<ai::TurnResult>,
    ended: Vec<(String, bool)>,
    created: Vec<NodeId>,
    runs: usize,
    reading: Reading,
    store: Arc<Mutex<Store>>,
    base: NodeId,
    _dir: tempfile::TempDir,
}

async fn run_turn(off_lock: bool) -> Outcome {
    let server = MockServer::start().await;
    let responses: Vec<Entry> = vec![
        ok(classifier_json("general")),
        ok(sse_tool_step(&[("t1", "slow_edit", "{}")])),
        ok(sse_text("Done.")),
    ];
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(SeqResponder::new(responses))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().expect("tempdir");
    let mut store = Store::open(dir.path()).expect("open store");
    let base = store.append(base_node()).expect("base");
    let store = Arc::new(Mutex::new(store));

    let (started_tx, started_rx) = channel();
    let (release_tx, release_rx) = channel();
    let runs = Arc::new(AtomicUsize::new(0));
    let mut dispatcher = ToolDispatcher::new();
    dispatcher.register(Box::new(SlowEdit {
        off_lock,
        started: Mutex::new(started_tx),
        release: Mutex::new(release_rx),
        runs: Arc::clone(&runs),
    }));

    let cfg = LlmConfig::new_anthropic("test-key").with_base_url(server.uri());
    let mut agent = Agent::new(
        cfg,
        Arc::new(Mutex::new(dispatcher)),
        Arc::clone(&store),
        Arc::new(Mutex::new(audio_engine::Engine::new())),
        Arc::new(Notify::new()),
        Arc::new(Mutex::new(None)),
        Arc::new(AtomicBool::new(false)),
        Arc::new(Mutex::new(None::<tools::Clipboard>)),
    );
    agent.set_plan_first(false);

    let reader = spawn_reader(Arc::clone(&store), started_rx, release_tx);

    let mut ended = Vec::new();
    let mut created = Vec::new();
    let result = agent
        .turn("stretch the take".to_string(), |event| match event {
            AgentEvent::ToolCallEnd { id, ok, .. } => ended.push((id, ok)),
            AgentEvent::NodeCreated(id) => created.push(id),
            _ => {}
        })
        .await;

    let reading = reader.join().expect("reader thread");
    Outcome {
        result,
        ended,
        created,
        runs: runs.load(Ordering::SeqCst),
        reading,
        store,
        base,
        _dir: dir,
    }
}

fn names(store: &Store, id: NodeId) -> Vec<String> {
    store
        .get(id)
        .expect("node")
        .state
        .annotations
        .into_iter()
        .map(|a| a.name)
        .collect()
}

/// The window's reads get through while the tool works, and the edit the
/// user makes in the middle is kept, with the tool's result on top of it.
///
/// RED on the pre-change dispatch (and with `off_lock: false`): the reader
/// cannot take the store's lock, and `blocked` is true.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_long_tool_leaves_the_store_readable_and_keeps_a_concurrent_edit() {
    let o = run_turn(true).await;

    assert!(
        !o.reading.blocked,
        "store read blocked: the store's lock was held for the whole tool"
    );
    assert_eq!(o.reading.head, Some(o.base), "it read the head as it was");
    assert!(o.reading.head_readable, "the head node could not be read");
    let user = o.reading.user_edit.expect("the user's edit landed");

    assert!(o.result.is_ok(), "{:?}", o.result);
    assert_eq!(o.ended, vec![("t1".to_string(), true)]);

    // The tool ran twice: once against the head it started on, whose result
    // was thrown away because the user's edit moved the head, and once on
    // top of that edit.
    assert_eq!(o.runs, 2);

    let store = o.store.lock().unwrap();
    let h = store.head().expect("head");
    assert_eq!(names(&store, h), vec!["from-user", "from-tool"]);
    assert_eq!(
        store.get(h).unwrap().parent,
        Some(user),
        "H.parent is the edit"
    );
    assert_eq!(store.get(user).unwrap().parent, Some(o.base));
    // base, the user's marker, the tool's: the attempt that lost left no node.
    assert_eq!(store.list_nodes().unwrap().len(), 3);
    // The frontend refreshes on this id, and it names the node that is the head.
    assert_eq!(o.created, vec![h]);
    assert_eq!(store.staged_in_flight(), 0);
}

/// The same turn for a tool that does not opt in: the store stays locked
/// for the whole call, as it always did. This is the baseline the test
/// above is red against, and the proof that the opt-in is what changed it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_tool_that_does_not_opt_in_still_holds_the_store() {
    let o = run_turn(false).await;

    assert!(
        o.reading.blocked,
        "the store was free while a tool that did not opt in ran"
    );
    assert!(o.reading.user_edit.is_none());
    assert!(o.result.is_ok(), "{:?}", o.result);
    assert_eq!(o.ended, vec![("t1".to_string(), true)]);
    assert_eq!(o.runs, 1);

    let store = o.store.lock().unwrap();
    let h = store.head().expect("head");
    assert_eq!(names(&store, h), vec!["from-tool"]);
    assert_eq!(store.get(h).unwrap().parent, Some(o.base));
    assert_eq!(store.list_nodes().unwrap().len(), 2);
    assert_eq!(o.created, vec![h]);
}
