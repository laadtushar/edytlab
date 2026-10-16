//! Running a tool without the store's lock held, and what keeps that safe
//! (#421).
//!
//! A time-stretch of a 32 s stereo track takes about ten seconds in a
//! debug build, and the agent used to hold the store, engine and
//! clipboard locks for all of them, so every read of the session waited.
//! `Shared::dispatch` runs a tool that opts in against a staged store
//! handle with no lock held, and publishes its result only if the session's
//! head did not move meanwhile.
//!
//! These tests are deterministic and single-threaded. A fixture tool
//! stands in for "the user, mid-tool": from inside `invoke` it reaches the
//! app's shared store the way a command would. If it can take the lock, no
//! lock was held; what it then does to the store is the concurrent edit.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use hound::{SampleFormat, WavSpec, WavWriter};
use serde_json::{json, Value};
use session::{Annotation, AnnotationId, AnnotationKind, NodeId, Store};
use tempfile::TempDir;
use tools::schema::{anthropic_tool, object_schema};
use tools::{
    DispatchError, Dispatched, Shared, Tool, ToolContext, ToolDispatcher, ToolResult,
    OFF_LOCK_ATTEMPTS,
};

/// What the tool "wrote" before its node exists.
const ORPHAN: &str = "output-not-yet-named.wav";

fn marker(name: &str) -> Annotation {
    Annotation {
        id: AnnotationId::new(),
        name: name.to_string(),
        kind: AnnotationKind::Marker { time_sec: 1.0 },
    }
}

/// A session with nothing in it.
fn plain_node() -> session::SessionNode {
    session::SessionNode {
        id: NodeId([0u8; 32]),
        parent: None,
        created_at: chrono::Utc::now(),
        label: None,
        reasoning: None,
        state: session::SessionState {
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
        },
        op: None,
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

// ---------------------------------------------------------------------
// The fixture tool
// ---------------------------------------------------------------------

/// What `Probe` saw on one call.
#[derive(Debug, Clone, Copy)]
struct Call {
    /// Whether the app's store lock could be taken from inside the tool.
    lock_free: bool,
    /// The head the tool was handed.
    head_seen: Option<NodeId>,
    /// Whether the dispatcher's lock could be taken from inside the tool.
    dispatcher_free: bool,
    /// Files a sweep run from inside the tool removed, if it ran one.
    swept: Option<usize>,
}

/// A tool that edits the session by adding a marker, and can play the
/// part of a user who edits at the same time.
struct Probe {
    /// The app's store: the one `Shared` locks, not the one `invoke` is
    /// handed.
    app_store: Arc<Mutex<Store>>,
    off_lock: bool,
    /// Whether the tool appends a node at all.
    appends: bool,
    /// On each of the first this-many calls that find the lock free, a
    /// user marker is added to the app's store before the tool appends.
    user_edits: usize,
    /// Return an error after appending.
    fail_after_append: bool,
    /// Panic after appending.
    panic_after_append: bool,
    /// Write a file into `derived/` that no node names, as a tool does
    /// before it appends the node that will, and run the orphan sweep
    /// while it is there.
    sweep_during: bool,
    /// The app's dispatcher, set once it exists.
    app_dispatcher: OnceLock<Arc<Mutex<ToolDispatcher>>>,
    calls: AtomicUsize,
    log: Mutex<Vec<Call>>,
}

impl Probe {
    fn new(app_store: &Arc<Mutex<Store>>) -> Self {
        Self {
            app_store: Arc::clone(app_store),
            off_lock: true,
            appends: true,
            user_edits: 0,
            fail_after_append: false,
            panic_after_append: false,
            sweep_during: false,
            app_dispatcher: OnceLock::new(),
            calls: AtomicUsize::new(0),
            log: Mutex::new(Vec::new()),
        }
    }
}

impl Tool for Probe {
    fn name(&self) -> &'static str {
        "probe"
    }

    fn schema(&self) -> Value {
        anthropic_tool("probe", "A fixture.", object_schema(&[]))
    }

    fn mutates(&self) -> bool {
        self.appends
    }

    fn runs_off_the_lock(&self) -> bool {
        self.off_lock
    }

    fn invoke(&self, _args: Value, ctx: &mut ToolContext) -> tools::Result<ToolResult> {
        let n = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
        let head_seen = ctx.store.head();

        let mut swept = None;
        let lock_free = match self.app_store.try_lock() {
            Ok(mut app) => {
                if n <= self.user_edits {
                    let head = app.head().expect("a head");
                    app.add_annotation(head, marker("user")).expect("user edit");
                }
                if self.sweep_during {
                    let derived = tools::derived_dir(app.project_dir());
                    std::fs::create_dir_all(&derived).expect("derived dir");
                    std::fs::write(derived.join(ORPHAN), b"RIFF-not-yet-named").expect("write");
                    swept = Some(
                        tools::reclaim::sweep_orphans(&app)
                            .expect("sweep")
                            .removed_files,
                    );
                }
                true
            }
            Err(_) => false,
        };
        let dispatcher_free = self
            .app_dispatcher
            .get()
            .is_some_and(|d| d.try_lock().is_ok());
        self.log.lock().unwrap().push(Call {
            lock_free,
            head_seen,
            dispatcher_free,
            swept,
        });

        if !self.appends {
            return Ok(ToolResult::Ok(json!({ "read": true })));
        }
        let head = ctx.store.head().expect("a head");
        let id = ctx
            .store
            .add_annotation(head, marker("tool"))
            .expect("tool edit");
        if self.panic_after_append {
            panic!("the tool panicked");
        }
        if self.fail_after_append {
            return Ok(ToolResult::Error("failed after appending".into()));
        }
        Ok(ToolResult::Ok(json!({ "node_id": id.to_hex() })))
    }
}

// ---------------------------------------------------------------------
// The app's shared state
// ---------------------------------------------------------------------

struct App {
    _dir: TempDir,
    dispatcher: Arc<Mutex<ToolDispatcher>>,
    store: Arc<Mutex<Store>>,
    engine: Mutex<audio_engine::Engine>,
    clipboard: Mutex<Option<tools::Clipboard>>,
    base: NodeId,
    probe: Arc<Probe>,
}

/// A `Tool` that forwards to a shared `Probe`, so a test can read the
/// probe's log after the dispatcher has taken ownership of the tool.
struct Forward(Arc<Probe>);

impl Tool for Forward {
    fn name(&self) -> &'static str {
        self.0.name()
    }
    fn schema(&self) -> Value {
        self.0.schema()
    }
    fn mutates(&self) -> bool {
        self.0.mutates()
    }
    fn runs_off_the_lock(&self) -> bool {
        self.0.runs_off_the_lock()
    }
    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> tools::Result<ToolResult> {
        self.0.invoke(args, ctx)
    }
}

impl App {
    /// A project with one node, the base, and a dispatcher holding the
    /// probe that `configure` builds.
    fn new(configure: impl FnOnce(&mut Probe)) -> Self {
        let dir = TempDir::new().expect("tempdir");
        let mut store = Store::open(dir.path()).expect("open store");
        let base = store.append(plain_node()).expect("base node");
        let store = Arc::new(Mutex::new(store));

        let mut probe = Probe::new(&store);
        configure(&mut probe);
        let probe = Arc::new(probe);

        let mut dispatcher = ToolDispatcher::new();
        dispatcher.register(Box::new(Forward(Arc::clone(&probe))));
        let dispatcher = Arc::new(Mutex::new(dispatcher));
        // A cycle, which leaks for the length of the test.
        probe
            .app_dispatcher
            .set(Arc::clone(&dispatcher))
            .unwrap_or_else(|_| unreachable!("set once"));

        Self {
            _dir: dir,
            dispatcher,
            store,
            engine: Mutex::new(audio_engine::Engine::new()),
            clipboard: Mutex::new(None),
            base,
            probe,
        }
    }

    fn dispatch(&self) -> tools::Result<ToolResult> {
        self.dispatched().result
    }

    /// The call, with where it left the head.
    fn dispatched(&self) -> Dispatched {
        Shared {
            dispatcher: &self.dispatcher,
            store: &self.store,
            engine: &self.engine,
            clipboard: &self.clipboard,
        }
        .dispatch("probe", json!({}), "", None)
    }

    fn head(&self) -> NodeId {
        self.store.lock().unwrap().head().expect("a head")
    }

    fn node(&self, id: NodeId) -> session::SessionNode {
        self.store.lock().unwrap().get(id).expect("node")
    }

    fn calls(&self) -> usize {
        self.probe.calls.load(Ordering::SeqCst)
    }

    fn log(&self) -> Vec<Call> {
        self.probe.log.lock().unwrap().clone()
    }

    fn node_count(&self) -> usize {
        self.store.lock().unwrap().list_nodes().unwrap().len()
    }
}

// ---------------------------------------------------------------------
// The lock
// ---------------------------------------------------------------------

/// The point of the whole change: while the tool runs, the store is free.
///
/// RED if `Shared::dispatch` ignores `runs_off_the_lock`: the locked path
/// holds the store's lock, `try_lock` inside the tool fails, and the
/// first assertion says so.
#[test]
fn the_store_is_free_while_an_off_lock_tool_runs() {
    let app = App::new(|_| {});

    let result = app.dispatch().expect("dispatch");

    let ToolResult::Ok(v) = result else {
        panic!("probe failed: {result:?}")
    };
    let log = app.log();
    assert_eq!(log.len(), 1);
    assert!(
        log[0].lock_free,
        "the store's lock was held while the tool ran"
    );
    assert_eq!(log[0].head_seen, Some(app.base));

    // And what it did was published afterwards, with its provenance.
    let head = app.head();
    assert_eq!(v["node_id"], head.to_hex());
    let node = app.node(head);
    assert_eq!(node.parent, Some(app.base));
    assert_eq!(names(&app.store.lock().unwrap(), head), vec!["tool"]);
    assert_eq!(
        node.op.expect("the dispatcher recorded an op").tool,
        "probe"
    );
    assert_eq!(app.store.lock().unwrap().staged_in_flight(), 0);
}

#[test]
fn a_tool_that_does_not_opt_in_runs_under_the_lock_as_before() {
    let app = App::new(|p| p.off_lock = false);

    let result = app.dispatch().expect("dispatch");

    assert!(matches!(result, ToolResult::Ok(_)), "{result:?}");
    let log = app.log();
    assert_eq!(log.len(), 1);
    assert!(
        !log[0].lock_free,
        "a tool that did not opt in ran without the lock"
    );
    let head = app.head();
    assert_eq!(app.node(head).parent, Some(app.base));
    assert_eq!(app.node(head).op.expect("op").tool, "probe");
}

/// The agent used to hold the dispatcher for the whole call, which was half
/// of the lock-order inversion with the commands that take the store first
/// and the dispatcher last. Locked or not, it is free once the call is
/// admitted.
#[test]
fn the_dispatcher_is_free_while_a_tool_runs() {
    for off_lock in [true, false] {
        let app = App::new(|p| p.off_lock = off_lock);

        app.dispatch().expect("dispatch");

        assert!(
            app.log()[0].dispatcher_free,
            "off_lock={off_lock}: the dispatcher's lock was held during the tool"
        );
    }
}

// ---------------------------------------------------------------------
// Races with the user
// ---------------------------------------------------------------------

/// The user edits while the tool runs. The edit is kept, the tool runs
/// again on top of it, and the attempt that lost published nothing.
#[test]
fn an_edit_made_while_the_tool_ran_is_kept_and_the_tool_reruns_on_top_of_it() {
    let app = App::new(|p| p.user_edits = 1);

    let result = app.dispatch().expect("dispatch");
    assert!(matches!(result, ToolResult::Ok(_)), "{result:?}");

    assert_eq!(
        app.calls(),
        2,
        "the tool should run once more after the edit"
    );
    let heads: Vec<_> = app.log().iter().map(|c| c.head_seen).collect();
    assert_eq!(heads[0], Some(app.base));
    assert_ne!(heads[1], Some(app.base), "the retry ran on the old head");

    let h = app.head();
    let h_node = app.node(h);
    let m = h_node.parent.expect("parent");
    let m_node = app.node(m);
    assert_eq!(m_node.parent, Some(app.base), "chain: base -> user -> tool");
    assert_eq!(heads[1], Some(m), "the retry ran on the user's edit");

    let store = app.store.lock().unwrap();
    assert_eq!(names(&store, m), vec!["user"]);
    assert_eq!(
        names(&store, h),
        vec!["user", "tool"],
        "the user's marker or the tool's is missing"
    );
    drop(store);

    // The lost attempt left nothing behind: base, the user's, the tool's.
    assert_eq!(app.node_count(), 3);
    // The head is a node the dispatcher recorded an op on.
    assert_eq!(h_node.op.expect("op").tool, "probe");
    assert_eq!(app.store.lock().unwrap().staged_in_flight(), 0);
}

/// A read does not conflict: there is nothing to throw away, and running
/// it again would only answer a question the user has moved past.
#[test]
fn a_read_only_tool_is_not_rerun_when_the_head_moves() {
    let app = App::new(|p| {
        p.appends = false;
        p.user_edits = 1;
    });

    let result = app.dispatch().expect("dispatch");

    assert!(matches!(result, ToolResult::Ok(_)), "{result:?}");
    assert_eq!(app.calls(), 1);
    let head = app.head();
    assert_ne!(head, app.base, "the user's edit is the head");
    assert_eq!(names(&app.store.lock().unwrap(), head), vec!["user"]);
    assert_eq!(app.node_count(), 2);
}

/// Progress is guaranteed: a user who edits during every attempt cannot
/// starve the tool. After `OFF_LOCK_ATTEMPTS` lost races it runs once
/// under the locks, where nothing can move the head.
#[test]
fn after_two_lost_races_the_tool_runs_once_under_the_lock() {
    assert_eq!(OFF_LOCK_ATTEMPTS, 2, "this test is written for two");
    // The user edits whenever the lock is free, which is every off-lock
    // attempt and never the locked one.
    let app = App::new(|p| p.user_edits = usize::MAX);

    let result = app.dispatch().expect("dispatch");
    assert!(matches!(result, ToolResult::Ok(_)), "{result:?}");

    assert_eq!(app.calls(), 3);
    let locks: Vec<bool> = app.log().iter().map(|c| c.lock_free).collect();
    assert_eq!(
        locks,
        vec![true, true, false],
        "the third run is the locked one"
    );

    // base -> user -> user -> tool, every user edit present.
    let h = app.head();
    let store = app.store.lock().unwrap();
    let m2 = store.get(h).unwrap().parent.expect("parent");
    let m1 = store.get(m2).unwrap().parent.expect("parent");
    assert_eq!(store.get(m1).unwrap().parent, Some(app.base));
    assert_eq!(names(&store, m1), vec!["user"]);
    assert_eq!(names(&store, m2), vec!["user", "user"]);
    assert_eq!(names(&store, h), vec!["user", "user", "tool"]);
    assert_eq!(store.list_nodes().unwrap().len(), 4);
    assert_eq!(store.staged_in_flight(), 0);
}

// ---------------------------------------------------------------------
// The sweep
// ---------------------------------------------------------------------

/// A tool writes its output before the node that names it exists, so for
/// that stretch the file is an orphan, and the orphan sweep would take it.
/// With the lock free, a sweep can run in exactly that stretch. It must
/// not delete anything; it runs again once the tool has finished.
#[test]
fn a_sweep_that_runs_while_an_off_lock_tool_works_deletes_nothing() {
    let app = App::new(|p| p.sweep_during = true);
    let derived = {
        let store = app.store.lock().unwrap();
        tools::derived_dir(store.project_dir())
    };

    app.dispatch().expect("dispatch");

    assert_eq!(
        app.log()[0].swept,
        Some(0),
        "a sweep removed a file while a tool was running off the lock"
    );
    assert!(derived.join(ORPHAN).is_file());

    // Once nothing is in flight the same sweep does its job: the deferral
    // postponed the removal, it did not lose it.
    let store = app.store.lock().unwrap();
    let report = tools::reclaim::sweep_orphans(&store).expect("sweep");
    assert_eq!(report.removed_files, 1, "{report:?}");
    assert!(!derived.join(ORPHAN).exists());
}

// ---------------------------------------------------------------------
// Real threads
// ---------------------------------------------------------------------

/// An edit that takes a while, then adds a marker named for the call.
struct Slow {
    work: std::time::Duration,
}

impl Tool for Slow {
    fn name(&self) -> &'static str {
        "slow"
    }
    fn schema(&self) -> Value {
        anthropic_tool(
            "slow",
            "A fixture.",
            object_schema(&[("i", "integer", true)]),
        )
    }
    fn runs_off_the_lock(&self) -> bool {
        true
    }
    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> tools::Result<ToolResult> {
        std::thread::sleep(self.work);
        let head = ctx.store.head().expect("a head");
        let name = format!("tool-{}", args["i"]);
        let id = ctx
            .store
            .add_annotation(head, marker(&name))
            .expect("tool edit");
        Ok(ToolResult::Ok(json!({ "node_id": id.to_hex() })))
    }
}

/// Users edit from several threads, as commands do — each holding the
/// store's lock for its own edit — while a tool is dispatched over and
/// over. However the two interleave, nothing is lost: every marker the
/// users added and every marker the tools added is in the final state, the
/// chain from the head is whole, and no handle is left counting.
///
/// Some runs lose their race and some may fall back to the locked run;
/// which is up to the scheduler. This asserts only what must hold either
/// way.
#[test]
fn concurrent_user_edits_and_tool_runs_lose_nothing() {
    const USERS: usize = 4;
    const EDITS_EACH: usize = 20;
    const TOOL_RUNS: usize = 8;

    let dir = TempDir::new().unwrap();
    let mut store = Store::open(dir.path()).unwrap();
    let base = store.append(plain_node()).unwrap();
    let store = Arc::new(Mutex::new(store));
    let dispatcher = {
        let mut d = ToolDispatcher::new();
        d.register(Box::new(Slow {
            work: std::time::Duration::from_millis(15),
        }));
        Mutex::new(d)
    };
    let engine = Mutex::new(audio_engine::Engine::new());
    let clipboard = Mutex::new(None::<tools::Clipboard>);

    std::thread::scope(|scope| {
        for user in 0..USERS {
            let store = Arc::clone(&store);
            scope.spawn(move || {
                for n in 0..EDITS_EACH {
                    {
                        let mut s = store.lock().unwrap();
                        let head = s.head().expect("a head");
                        s.add_annotation(head, marker(&format!("user-{user}-{n}")))
                            .expect("user edit");
                    }
                    std::thread::sleep(std::time::Duration::from_millis(3));
                }
            });
        }
        for i in 0..TOOL_RUNS {
            let result = Shared {
                dispatcher: &dispatcher,
                store: &store,
                engine: &engine,
                clipboard: &clipboard,
            }
            .dispatch("slow", json!({ "i": i }), "", None)
            .result
            .expect("dispatch");
            assert!(matches!(result, ToolResult::Ok(_)), "{result:?}");
        }
    });

    let store = store.lock().unwrap();
    let head = store.head().expect("head");
    let mut names: Vec<String> = names(&store, head);
    names.sort();
    let mut expected: Vec<String> = (0..USERS)
        .flat_map(|u| (0..EDITS_EACH).map(move |n| format!("user-{u}-{n}")))
        .chain((0..TOOL_RUNS).map(|i| format!("tool-{i}")))
        .collect();
    expected.sort();
    assert_eq!(names, expected, "an edit was lost or duplicated");

    // The chain from the head to the first node can be walked, with every
    // node's file present: nothing the head depends on was left unwritten.
    let mut cursor = Some(head);
    let mut length = 0;
    while let Some(id) = cursor {
        cursor = store.get(id).expect("a node on the head's chain").parent;
        length += 1;
    }
    assert_eq!(length, 1 + USERS * EDITS_EACH + TOOL_RUNS);
    assert_eq!(store.get(base).unwrap().parent, None);
    assert_eq!(store.staged_in_flight(), 0);
}

// ---------------------------------------------------------------------
// Agreement with the locked path
// ---------------------------------------------------------------------

/// A tool that appends and then fails leaves its node behind when it runs
/// under the lock. Off the lock it must agree: committed whatever the
/// result was.
#[test]
fn a_tool_that_appends_then_fails_leaves_its_node_on_both_paths() {
    for off_lock in [true, false] {
        let app = App::new(|p| {
            p.off_lock = off_lock;
            p.fail_after_append = true;
        });

        let result = app.dispatch().expect("dispatch");

        assert_eq!(
            result,
            ToolResult::Error("failed after appending".into()),
            "off_lock={off_lock}"
        );
        let head = app.head();
        assert_ne!(head, app.base, "off_lock={off_lock}: the node was lost");
        assert_eq!(app.node(head).parent, Some(app.base));
    }
}

/// A panic in a tool that runs without the lock poisons nothing, and the
/// handle stops counting as in flight as it unwinds.
#[test]
fn a_panic_in_an_off_lock_tool_poisons_no_lock() {
    let app = App::new(|p| p.panic_after_append = true);

    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| app.dispatch()));

    assert!(caught.is_err(), "the panic should reach the caller");
    let store = app.store.lock().expect("the store lock was poisoned");
    assert_eq!(store.head(), Some(app.base), "a panicked run published");
    assert_eq!(store.staged_in_flight(), 0);
    drop(store);
    assert!(app.dispatcher.lock().is_ok());
    assert!(app.engine.lock().is_ok());
    assert!(app.clipboard.lock().is_ok());
}

/// `dispatch` refuses what `invoke` refuses, with the same errors.
#[test]
fn dispatch_reports_what_invoke_reports() {
    let app = App::new(|_| {});
    let shared = || Shared {
        dispatcher: &app.dispatcher,
        store: &app.store,
        engine: &app.engine,
        clipboard: &app.clipboard,
    };

    let unknown = shared().dispatch("nope", json!({}), "", None);
    assert!(matches!(unknown.result, Err(DispatchError::Unknown(_))));
    assert_eq!(unknown.moved_head, None);

    let invalid = shared().dispatch("probe", json!({ "x": 1 }), "", None);
    assert!(matches!(
        invalid.result,
        Err(DispatchError::SchemaValidation(_))
    ));
    assert_eq!(invalid.moved_head, None);

    let allowed: std::collections::HashSet<String> = ["other".to_string()].into();
    let refused = shared().dispatch("probe", json!({}), "", Some(&allowed));
    assert!(matches!(
        refused.result,
        Err(DispatchError::NotPermitted(_))
    ));
    assert_eq!(refused.moved_head, None);
    assert_eq!(app.calls(), 0, "a refused call ran the tool");
}

// ---------------------------------------------------------------------
// Where the call left the head (#594)
// ---------------------------------------------------------------------
//
// The app's window follows the agent's head moves from what `dispatch`
// reports, so it must be the store's head after the call, only when the
// call moved it, and never a move the user made meanwhile.

/// Locked or not, a call that appends reports the head it moved to.
#[test]
fn a_call_that_moves_the_head_reports_where_on_both_paths() {
    for off_lock in [true, false] {
        let app = App::new(|p| p.off_lock = off_lock);

        let d = app.dispatched();

        assert!(matches!(d.result, Ok(ToolResult::Ok(_))), "{d:?}");
        let head = app.head();
        assert_ne!(head, app.base, "off_lock={off_lock}: nothing was appended");
        assert_eq!(d.moved_head, Some(head), "off_lock={off_lock}");
    }
}

/// A call that reads moves nothing, and says so.
#[test]
fn a_read_only_call_reports_no_move_on_both_paths() {
    for off_lock in [true, false] {
        let app = App::new(|p| {
            p.off_lock = off_lock;
            p.appends = false;
        });

        let d = app.dispatched();

        assert!(matches!(d.result, Ok(ToolResult::Ok(_))), "{d:?}");
        assert_eq!(app.head(), app.base);
        assert_eq!(d.moved_head, None, "off_lock={off_lock}");
    }
}

/// The user's edit while a read runs off the lock moves the store's head.
/// That move is the user's, and the window has it already; reporting it as
/// the agent's would put it on the undo path a second time.
#[test]
fn a_users_edit_during_a_read_is_not_reported_as_the_calls() {
    let app = App::new(|p| {
        p.appends = false;
        p.user_edits = 1;
    });

    let d = app.dispatched();

    assert!(matches!(d.result, Ok(ToolResult::Ok(_))), "{d:?}");
    assert_ne!(app.head(), app.base, "the user's edit is the head");
    assert_eq!(d.moved_head, None);
}

/// A run that lost the race to the user's edit and ran again reports the
/// head of the run that published, which sits on the user's edit, and not
/// the user's edit itself.
#[test]
fn a_rerun_reports_its_own_head_not_the_users() {
    let app = App::new(|p| p.user_edits = 1);

    let d = app.dispatched();

    assert!(matches!(d.result, Ok(ToolResult::Ok(_))), "{d:?}");
    assert_eq!(app.calls(), 2);
    let head = app.head();
    let user = app.node(head).parent.expect("the user's edit");
    assert_ne!(user, app.base);
    assert_eq!(d.moved_head, Some(head));
}

/// After two lost races the locked run is the one that publishes, and it
/// reports its own head.
#[test]
fn the_locked_run_after_two_lost_races_reports_its_own_head() {
    let app = App::new(|p| p.user_edits = usize::MAX);

    let d = app.dispatched();

    assert!(matches!(d.result, Ok(ToolResult::Ok(_))), "{d:?}");
    assert_eq!(app.calls(), 3);
    let head = app.head();
    assert_eq!(
        names(&app.store.lock().unwrap(), head),
        vec!["user", "user", "tool"]
    );
    assert_eq!(d.moved_head, Some(head));
}

/// A tool that appends and then fails leaves its node, and the head on it,
/// so the move is reported with the error.
#[test]
fn a_call_that_moves_the_head_then_fails_reports_the_move_on_both_paths() {
    for off_lock in [true, false] {
        let app = App::new(|p| {
            p.off_lock = off_lock;
            p.fail_after_append = true;
        });

        let d = app.dispatched();

        assert!(
            matches!(d.result, Ok(ToolResult::Error(_))),
            "off_lock={off_lock}: {d:?}"
        );
        let head = app.head();
        assert_ne!(head, app.base);
        assert_eq!(d.moved_head, Some(head), "off_lock={off_lock}");
    }
}

// ---------------------------------------------------------------------
// Real tools
// ---------------------------------------------------------------------

const SR: u32 = 44_100;

fn write_tone(path: &Path) -> PathBuf {
    let spec = WavSpec {
        channels: 1,
        sample_rate: SR,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut w = WavWriter::create(path, spec).expect("wav writer");
    for n in 0..(SR as usize * 3 / 2) {
        let t = n as f32 / SR as f32;
        let v = (2.0 * std::f32::consts::PI * (220.0 + 110.0 * t) * t).sin() * 8000.0;
        w.write_sample(v as i16).unwrap();
    }
    w.finalize().unwrap();
    path.to_path_buf()
}

/// The app's state for a real session.
struct Real {
    _dir: TempDir,
    dispatcher: Mutex<ToolDispatcher>,
    store: Mutex<Store>,
    engine: Mutex<audio_engine::Engine>,
    clipboard: Mutex<Option<tools::Clipboard>>,
}

impl Real {
    fn new() -> Self {
        let dir = TempDir::new().expect("tempdir");
        let store = Store::open(dir.path()).expect("store");
        Self {
            _dir: dir,
            dispatcher: Mutex::new(ToolDispatcher::default_dispatcher()),
            store: Mutex::new(store),
            engine: Mutex::new(audio_engine::Engine::new()),
            clipboard: Mutex::new(None),
        }
    }

    /// Through `Shared`: off the lock if the tool says so.
    fn shared(&self, tool: &str, args: Value, message: &str) -> Value {
        let result = Shared {
            dispatcher: &self.dispatcher,
            store: &self.store,
            engine: &self.engine,
            clipboard: &self.clipboard,
        }
        .dispatch(tool, args, message, None)
        .result
        .expect("dispatch");
        match result {
            ToolResult::Ok(v) => v,
            ToolResult::Error(m) => panic!("{tool} failed: {m}"),
        }
    }

    /// The way every caller but the agent runs a tool: `invoke` under the
    /// locks.
    fn locked(&self, tool: &str, args: Value, message: &str) -> Value {
        let d = self.dispatcher.lock().unwrap();
        let mut store = self.store.lock().unwrap();
        let mut engine = self.engine.lock().unwrap();
        let mut clipboard = self.clipboard.lock().unwrap();
        let mut ctx = ToolContext {
            store: &mut store,
            engine: &mut engine,
            user_message: message,
            clipboard: &mut clipboard,
            allowed_tools: None,
        };
        match d.invoke(tool, args, &mut ctx).expect("invoke") {
            ToolResult::Ok(v) => v,
            ToolResult::Error(m) => panic!("{tool} failed: {m}"),
        }
    }

    fn head(&self) -> NodeId {
        self.store.lock().unwrap().head().expect("head")
    }

    fn op(&self, id: NodeId) -> Value {
        let node = self.store.lock().unwrap().get(id).expect("node");
        serde_json::to_value(node.op.expect("the node has an op")).unwrap()
    }

    fn opts_in(&self, tool: &str) -> bool {
        self.dispatcher
            .lock()
            .unwrap()
            .get(tool)
            .expect("registered")
            .runs_off_the_lock()
    }
}

/// Run `tool` off the lock, then forget the node it made and run the same
/// call under the lock. Content addressing makes the second run land on
/// the same id if the two paths do the same thing, and removing the first
/// node means the second one's provenance is its own, not the first's
/// (`set_op` keeps the first writer).
fn same_node_either_way(tool: &str, args: Value, message: &str) -> (NodeId, Value) {
    let world = Real::new();
    let take = write_tone(&world._dir.path().join("take.wav"));
    world.locked("load", json!({ "path": take }), "");
    let base = world.head();
    assert!(world.opts_in(tool), "{tool} does not run off the lock");

    world.shared(tool, args.clone(), message);
    let off = world.head();
    assert_ne!(off, base, "{tool} appended nothing");
    let off_op = world.op(off);

    {
        let mut store = world.store.lock().unwrap();
        store.remove_node(off).unwrap();
        store.set_head(base).unwrap();
    }
    world.locked(tool, args, message);
    let on = world.head();

    assert_eq!(off, on, "{tool}: the two paths made different nodes");
    assert_eq!(
        off_op,
        world.op(on),
        "{tool}: the two paths recorded different provenance"
    );
    (off, off_op)
}

#[test]
fn off_lock_and_locked_runs_of_time_stretch_make_the_same_node() {
    let (_, op) = same_node_either_way("time_stretch", json!({ "track": 0, "factor": 1.25 }), "");
    assert_eq!(op["tool"], "time_stretch");
    assert_eq!(op["reproducible"], true);
}

/// `fade` reads its range from the chat message when the call names none,
/// and the dispatcher records the range it used so the node replays. That
/// recording is in the half of `invoke` that now runs off the lock too.
#[test]
fn off_lock_and_locked_runs_of_fade_with_a_message_range_make_the_same_node() {
    let (_, op) = same_node_either_way(
        "fade",
        json!({ "track": 0, "kind": "out" }),
        "[apply to 0:00-0:01] fade that out",
    );
    assert_eq!(
        op["params"]["range"],
        json!({ "start_sec": 0.0, "end_sec": 1.0 })
    );
}

/// `load` closes its op over the file it read. Two sessions loading the
/// same file, one off the lock and one under it, record the same op.
#[test]
fn off_lock_and_locked_loads_record_the_same_op() {
    let source = TempDir::new().unwrap();
    let take = write_tone(&source.path().join("take.wav"));

    let off = Real::new();
    let on = Real::new();
    assert!(off.opts_in("load"));
    off.shared("load", json!({ "path": take }), "");
    on.locked("load", json!({ "path": take }), "");

    let (off_op, on_op) = (off.op(off.head()), on.op(on.head()));
    assert_eq!(off_op, on_op);
    assert_eq!(off_op["tool"], "load");
    assert!(off_op["inputs"]["source"]["audio_hash"].is_string());
}

/// A real edit is not lost to, and does not lose, a real concurrent edit.
/// The "user" here adds a marker to the app's store between the tool's
/// stage and its commit by way of a second tool that runs first.
///
/// This stages by hand, so it pins the pieces (`stage`, `ToolDispatcher::run`
/// on a staged handle, `commit` answering `Conflict`, a fresh dispatch landing
/// on the marker) and not `Shared`'s conflict loop with a real tool. That loop
/// is run with real tools by the tests under "Edits that change what a call's
/// arguments mean" below, where the edit lands from inside the tool.
#[test]
fn a_real_tool_reruns_on_top_of_a_marker_added_while_it_ran() {
    let world = Real::new();
    let take = write_tone(&world._dir.path().join("take.wav"));
    world.locked("load", json!({ "path": take }), "");
    let base = world.head();

    // Stage by hand, as `Shared` does, and run the tool against the handle.
    let mut staged = world.store.lock().unwrap().stage().unwrap();
    let prepared = world
        .dispatcher
        .lock()
        .unwrap()
        .prepare("time_stretch", &json!({ "track": 0, "factor": 1.25 }), None)
        .unwrap();
    let mut engine = audio_engine::Engine::new();
    let mut clipboard = None;
    let mut ctx = ToolContext {
        store: &mut staged,
        engine: &mut engine,
        user_message: "",
        clipboard: &mut clipboard,
        allowed_tools: None,
    };
    let result =
        ToolDispatcher::run(&prepared, json!({ "track": 0, "factor": 1.25 }), &mut ctx).unwrap();
    assert!(matches!(result, ToolResult::Ok(_)));

    // The tool has run and nothing is published: the store is free, and
    // the user adds a marker.
    let user = world
        .store
        .lock()
        .unwrap()
        .add_annotation(base, marker("user"))
        .unwrap();

    // The commit finds the head moved and writes nothing.
    let outcome = world.store.lock().unwrap().commit(staged).unwrap();
    assert_eq!(outcome, session::Commit::Conflict);
    assert_eq!(world.head(), user);

    // Dispatching again lands on top of the marker, keeping it.
    world.shared("time_stretch", json!({ "track": 0, "factor": 1.25 }), "");
    let head = world.head();
    let store = world.store.lock().unwrap();
    let node = store.get(head).unwrap();
    assert_eq!(node.parent, Some(user));
    assert_eq!(node.state.annotations.len(), 1);
    assert_eq!(node.state.annotations[0].name, "user");
}

// ---------------------------------------------------------------------
// Edits that change what a call's arguments mean
// ---------------------------------------------------------------------
//
// A call names a track by its position, and a time by its seconds, as the
// model saw them. Before #421 the user's edit waited for the tool, so the
// tool always ran on what the model had seen. Off the lock the user's edit
// can land first, and running the tool again "on top of it" means running
// it on a session its arguments no longer describe: `time_stretch {track: 1}`
// after the user removed track 0 stretches the track that was number 2.
//
// These tests use the real tools throughout. `Hooked` wraps the real tool
// under test and, from inside its first run, makes the user's edit with
// another real tool, as the mixer does (`run_track_tool`), so the conflict
// loop in `Shared::dispatch` runs against a real session.

/// What the user does while the tool runs.
enum UserEdit {
    /// A real tool, called as the mixer calls them: under the locks.
    Tool(&'static str, Value),
    /// Undo or redo: the head moves to a node that already exists.
    SetHead(NodeId),
    /// A marker, which changes no track.
    Marker,
}

/// The wrapped tool's calls, and the edits left to make: one on each call
/// that finds the lock free, in order.
struct Script {
    edits: Mutex<std::collections::VecDeque<UserEdit>>,
    heads: Mutex<Vec<Option<NodeId>>>,
}

/// A real tool that, before it runs, plays the part of the user.
struct Hooked {
    inner: Box<dyn Tool>,
    app_store: Arc<Mutex<Store>>,
    user: ToolDispatcher,
    script: Arc<Script>,
}

impl Tool for Hooked {
    fn name(&self) -> &'static str {
        self.inner.name()
    }
    fn schema(&self) -> Value {
        self.inner.schema()
    }
    fn mutates(&self) -> bool {
        self.inner.mutates()
    }
    fn runs_off_the_lock(&self) -> bool {
        self.inner.runs_off_the_lock()
    }
    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> tools::Result<ToolResult> {
        self.script.heads.lock().unwrap().push(ctx.store.head());
        let edit = self.script.edits.lock().unwrap().pop_front();
        if let Some(edit) = edit {
            let mut app = self
                .app_store
                .try_lock()
                .expect("the store is free while an off-lock tool runs");
            match edit {
                UserEdit::Tool(name, args) => {
                    let mut engine = audio_engine::Engine::new();
                    let mut clipboard = None;
                    let mut user_ctx = ToolContext {
                        store: &mut app,
                        engine: &mut engine,
                        user_message: "",
                        clipboard: &mut clipboard,
                        allowed_tools: None,
                    };
                    match self.user.invoke(name, args, &mut user_ctx) {
                        Ok(ToolResult::Ok(_)) => {}
                        other => panic!("the user's {name} failed: {other:?}"),
                    }
                }
                UserEdit::SetHead(id) => app.set_head(id).expect("undo"),
                UserEdit::Marker => {
                    let head = app.head().expect("head");
                    app.add_annotation(head, marker("user")).expect("marker");
                }
            }
        }
        self.inner.invoke(args, ctx)
    }
}

/// A session of three tracks, drums, vocals and bass, in that order, one
/// clip each, with a tool registered under the user's hook.
struct Studio {
    dir: TempDir,
    dispatcher: Mutex<ToolDispatcher>,
    store: Arc<Mutex<Store>>,
    engine: Mutex<audio_engine::Engine>,
    clipboard: Mutex<Option<tools::Clipboard>>,
    script: Arc<Script>,
}

/// A clip as `(start, offset, length, source)`.
type ClipShape = (u64, u64, u64, PathBuf);

impl Studio {
    fn new(tool: Box<dyn Tool>) -> Self {
        let dir = TempDir::new().expect("tempdir");
        let store = Arc::new(Mutex::new(Store::open(dir.path()).expect("store")));

        // The tracks, loaded the way the agent loads them.
        let setup = ToolDispatcher::default_dispatcher();
        for name in ["drums", "vocals", "bass"] {
            let take = write_tone(&dir.path().join(format!("{name}.wav")));
            let mut engine = audio_engine::Engine::new();
            let mut clipboard = None;
            let mut guard = store.lock().unwrap();
            let mut ctx = ToolContext {
                store: &mut guard,
                engine: &mut engine,
                user_message: "",
                clipboard: &mut clipboard,
                allowed_tools: None,
            };
            let loaded = setup
                .invoke("load", json!({ "path": take }), &mut ctx)
                .expect("load");
            assert!(matches!(loaded, ToolResult::Ok(_)), "{loaded:?}");
        }

        let script = Arc::new(Script {
            edits: Mutex::new(Default::default()),
            heads: Mutex::new(Vec::new()),
        });
        let mut dispatcher = ToolDispatcher::new();
        dispatcher.register(Box::new(Hooked {
            inner: tool,
            app_store: Arc::clone(&store),
            user: ToolDispatcher::default_dispatcher(),
            script: Arc::clone(&script),
        }));
        let studio = Self {
            dir,
            dispatcher: Mutex::new(dispatcher),
            store,
            engine: Mutex::new(audio_engine::Engine::new()),
            clipboard: Mutex::new(None),
            script,
        };
        assert_eq!(studio.track_names(), ["drums", "vocals", "bass"]);
        studio
    }

    /// The user makes `edit` between the tool's stage and its commit, on
    /// the next call that finds the store free.
    fn then_the_user(&self, edit: UserEdit) -> &Self {
        self.script.edits.lock().unwrap().push_back(edit);
        self
    }

    /// The user's own edit, made before the agent starts, with the locks
    /// as the mixer takes them.
    fn user_now(&self, tool: &str, args: Value) {
        let d = ToolDispatcher::default_dispatcher();
        let mut store = self.store.lock().unwrap();
        let mut engine = audio_engine::Engine::new();
        let mut clipboard = None;
        let mut ctx = ToolContext {
            store: &mut store,
            engine: &mut engine,
            user_message: "",
            clipboard: &mut clipboard,
            allowed_tools: None,
        };
        let r = d.invoke(tool, args, &mut ctx).expect("invoke");
        assert!(matches!(r, ToolResult::Ok(_)), "{tool}: {r:?}");
    }

    fn dispatch(&self, tool: &str, args: Value, message: &str) -> ToolResult {
        Shared {
            dispatcher: &self.dispatcher,
            store: &self.store,
            engine: &self.engine,
            clipboard: &self.clipboard,
        }
        .dispatch(tool, args, message, None)
        .result
        .expect("dispatch")
    }

    fn head(&self) -> NodeId {
        self.store.lock().unwrap().head().expect("head")
    }

    fn track_names(&self) -> Vec<String> {
        self.tracks().into_iter().map(|t| t.0).collect()
    }

    /// Each track at the head, with its clips.
    fn tracks(&self) -> Vec<(String, Vec<ClipShape>)> {
        let store = self.store.lock().unwrap();
        let node = store.get(store.head().expect("head")).expect("head node");
        node.state
            .tracks
            .iter()
            .map(|t| {
                let clips = t
                    .clips
                    .iter()
                    .map(|c| {
                        (
                            c.start_in_track,
                            c.source_offset,
                            c.length,
                            c.source_path.clone(),
                        )
                    })
                    .collect();
                (t.name.clone(), clips)
            })
            .collect()
    }

    fn clips_of(&self, name: &str) -> Vec<ClipShape> {
        self.tracks()
            .into_iter()
            .find(|t| t.0 == name)
            .unwrap_or_else(|| panic!("no track called {name}"))
            .1
    }

    /// How many times the wrapped tool ran.
    fn runs(&self) -> usize {
        self.script.heads.lock().unwrap().len()
    }

    fn node_count(&self) -> usize {
        self.store.lock().unwrap().list_nodes().unwrap().len()
    }
}

fn error_text(result: &ToolResult) -> &str {
    match result {
        ToolResult::Error(m) => m,
        ToolResult::Ok(v) => panic!("expected an error, the call succeeded: {v}"),
    }
}

/// The reviewer's reproduction (B1).
///
/// The agent chose `time_stretch {track: 1}` when the tracks were drums,
/// vocals, bass: vocals. While it ran, the user removed drums in the mixer,
/// which takes the free lock at once. The commit loses, and the tool used to
/// run again on [vocals, bass] with the same `track: 1`: bass. It published,
/// and the model was told its call had succeeded.
///
/// The call must not edit a track it did not name. The user's removal stays.
#[test]
fn a_track_removed_while_a_tool_ran_does_not_send_the_rerun_to_another_track() {
    let studio = Studio::new(Box::new(tools::tool::TimeStretchTool));
    let vocals = studio.clips_of("vocals");
    let bass = studio.clips_of("bass");
    studio.then_the_user(UserEdit::Tool("remove_track", json!({ "track": 0 })));

    let result = studio.dispatch("time_stretch", json!({ "track": 1, "factor": 0.5 }), "");

    assert_eq!(
        studio.clips_of("bass"),
        bass,
        "bass was stretched by a call that named vocals: {result:?}"
    );
    assert_eq!(studio.clips_of("vocals"), vocals, "nothing was applied");
    let said = error_text(&result);
    assert!(
        said.contains("not applied") && said.contains("vocals") && said.contains("bass"),
        "the model is told what moved, by name: {said}"
    );
    // The user's removal is kept, and the tool did not run a second time.
    assert_eq!(studio.track_names(), ["vocals", "bass"]);
    assert_eq!(studio.runs(), 1, "the tool ran again on a changed session");
    assert_eq!(
        studio.store.lock().unwrap().staged_in_flight(),
        0,
        "a refused call left a handle counting"
    );
}

/// The refusal says where the track the call named is now, so that the
/// model does not send the same index again: it was told `track 1` was
/// vocals, and after drums were removed vocals is track 0.
#[test]
fn a_refusal_says_where_the_named_track_is_now() {
    let studio = Studio::new(Box::new(tools::tool::TimeStretchTool));
    studio.then_the_user(UserEdit::Tool("remove_track", json!({ "track": 0 })));

    let result = studio.dispatch("time_stretch", json!({ "track": 1, "factor": 0.5 }), "");

    let said = error_text(&result);
    assert!(said.contains("not applied"), "{said}");
    assert!(
        said.contains("\"vocals\" is now track 0"),
        "the model is told the new position of the track it named: {said}"
    );
    assert!(
        said.contains("with the track's new position"),
        "and what to do with it: {said}"
    );
    assert!(
        !said.contains("\"bass\" is now"),
        "bass was not named by the call: {said}"
    );
}

/// By undo the track moves down: [vocals, bass] was the session when the
/// call named bass at 1, and undoing the removal makes it track 2.
#[test]
fn a_refusal_after_an_undo_says_where_the_track_went() {
    let studio = Studio::new(Box::new(tools::tool::TimeStretchTool));
    let before_removal = studio.head();
    studio.user_now("remove_track", json!({ "track": 0 }));
    studio.then_the_user(UserEdit::SetHead(before_removal));

    let result = studio.dispatch("time_stretch", json!({ "track": 1, "factor": 0.5 }), "");

    let said = error_text(&result);
    assert!(said.contains("\"bass\" is now track 2"), "{said}");
}

/// A track that is gone is not somewhere else. The model is told what is
/// at the position now, and not that the track it named moved.
#[test]
fn a_refusal_for_a_removed_track_does_not_invent_a_new_position() {
    let studio = Studio::new(Box::new(tools::tool::TimeStretchTool));
    studio.then_the_user(UserEdit::Tool("remove_track", json!({ "track": 1 })));

    let result = studio.dispatch("time_stretch", json!({ "track": 1, "factor": 0.5 }), "");

    let said = error_text(&result);
    assert!(
        said.contains("became \"bass\""),
        "what is at the position now: {said}"
    );
    assert!(!said.contains("is now track"), "{said}");
    assert!(!said.contains("new position"), "{said}");
}

/// Every named track that moved is placed, and one that did not is not
/// said to have: the call named drums and bass, and the user removed
/// vocals between them.
#[test]
fn a_refusal_places_each_named_track_that_moved_and_only_those() {
    let studio = Studio::new(Box::new(tools::tool::MixToNewTrackTool));
    studio.then_the_user(UserEdit::Tool("remove_track", json!({ "track": 1 })));

    let result = studio.dispatch("mix_to_new_track", json!({ "track_indices": [0, 2] }), "");

    let said = error_text(&result);
    assert!(said.contains("\"bass\" is now track 1"), "{said}");
    assert!(
        !said.contains("\"drums\" is now"),
        "drums did not move: {said}"
    );
    assert!(said.contains("The track you named has moved"), "{said}");

    // And with two that moved: drums removed, vocals and bass named.
    let studio = Studio::new(Box::new(tools::tool::MixToNewTrackTool));
    studio.then_the_user(UserEdit::Tool("remove_track", json!({ "track": 0 })));

    let result = studio.dispatch("mix_to_new_track", json!({ "track_indices": [1, 2] }), "");

    let said = error_text(&result);
    assert!(
        said.contains("\"vocals\" is now track 0, \"bass\" is now track 1"),
        "{said}"
    );
    assert!(said.contains("The tracks you named have moved"), "{said}");
    assert!(said.contains("with the tracks' new positions"), "{said}");
}

/// A renamed track is found by what it is, not by what it is called, and
/// the model is told both names: it knows the track as "vocals".
#[test]
fn a_refusal_finds_a_track_the_user_renamed() {
    let studio = Studio::new(Box::new(tools::tool::TimeStretchTool));
    // First run: the rename, which moves no clip and so lets the call run
    // again. Second run: drums are removed.
    studio
        .then_the_user(UserEdit::Tool(
            "rename_track",
            json!({ "track": 1, "name": "lead" }),
        ))
        .then_the_user(UserEdit::Tool("remove_track", json!({ "track": 0 })));

    let result = studio.dispatch("time_stretch", json!({ "track": 1, "factor": 0.5 }), "");

    let said = error_text(&result);
    assert!(
        said.contains("\"vocals\" (now named \"lead\") is now track 0"),
        "{said}"
    );
    assert_eq!(studio.runs(), 2, "the rename alone did not stop the call");
}

/// The same hazard by undo. The tracks are [vocals, bass] after the user
/// removed drums; the agent stretches bass (`track: 1`); the user undoes the
/// removal. On [drums, vocals, bass], `track: 1` is vocals.
#[test]
fn an_undo_that_brings_a_track_back_does_not_send_the_rerun_to_another_track() {
    let studio = Studio::new(Box::new(tools::tool::TimeStretchTool));
    let before_removal = studio.head();
    studio.user_now("remove_track", json!({ "track": 0 }));
    let vocals = studio.clips_of("vocals");
    let bass = studio.clips_of("bass");
    studio.then_the_user(UserEdit::SetHead(before_removal));

    let result = studio.dispatch("time_stretch", json!({ "track": 1, "factor": 0.5 }), "");

    assert_eq!(studio.clips_of("vocals"), vocals, "{result:?}");
    assert_eq!(studio.clips_of("bass"), bass, "{result:?}");
    assert!(error_text(&result).contains("not applied"), "{result:?}");
    assert_eq!(studio.head(), before_removal, "the undo is kept");
    assert_eq!(studio.runs(), 1);
}

/// The window is not only the first attempt. A removal during the second
/// run must not be missed by the run that follows it under the lock.
#[test]
fn a_track_removed_during_the_second_run_does_not_reach_the_locked_run() {
    let studio = Studio::new(Box::new(tools::tool::TimeStretchTool));
    let bass = studio.clips_of("bass");
    // First run: a marker, which is a conflict but changes no track, so
    // the tool runs again. Second run: drums are removed.
    studio
        .then_the_user(UserEdit::Marker)
        .then_the_user(UserEdit::Tool("remove_track", json!({ "track": 0 })));

    let result = studio.dispatch("time_stretch", json!({ "track": 1, "factor": 0.5 }), "");

    assert_eq!(studio.clips_of("bass"), bass, "{result:?}");
    assert!(error_text(&result).contains("not applied"), "{result:?}");
    assert_eq!(studio.runs(), 2, "the locked run should not have started");
    assert_eq!(studio.store.lock().unwrap().staged_in_flight(), 0);
}

/// An edit that leaves the named track where it was is what the re-run is
/// for, and it still happens: drums are stretched, bass is gone.
#[test]
fn a_track_removed_after_the_one_named_leaves_the_rerun_alone() {
    let studio = Studio::new(Box::new(tools::tool::TimeStretchTool));
    let drums = studio.clips_of("drums");
    studio.then_the_user(UserEdit::Tool("remove_track", json!({ "track": 2 })));

    let result = studio.dispatch("time_stretch", json!({ "track": 0, "factor": 0.5 }), "");

    assert!(matches!(result, ToolResult::Ok(_)), "{result:?}");
    assert_eq!(studio.runs(), 2, "the tool should run again");
    let stretched = studio.clips_of("drums");
    assert!(
        stretched[0].2 > drums[0].2 * 3 / 2,
        "drums were not stretched: {drums:?} -> {stretched:?}"
    );
    assert_eq!(
        studio.track_names(),
        ["drums", "vocals"],
        "the user's removal is kept"
    );
}

/// A track added at the end moves nobody.
#[test]
fn a_track_added_while_a_tool_ran_leaves_the_rerun_alone() {
    let studio = Studio::new(Box::new(tools::tool::TimeStretchTool));
    let vocals = studio.clips_of("vocals");
    studio.then_the_user(UserEdit::Tool("add_track", json!({ "name": "fx" })));

    let result = studio.dispatch("time_stretch", json!({ "track": 1, "factor": 0.5 }), "");

    assert!(matches!(result, ToolResult::Ok(_)), "{result:?}");
    assert_eq!(studio.runs(), 2);
    assert!(studio.clips_of("vocals")[0].2 > vocals[0].2 * 3 / 2);
    assert_eq!(studio.track_names(), ["drums", "vocals", "bass", "fx"]);
}

/// A call that names no track has nothing to be moved off: `load` adds a
/// track wherever the session is.
#[test]
fn a_call_that_names_no_track_runs_again_after_a_track_is_removed() {
    let studio = Studio::new(Box::new(tools::tool::LoadTool));
    let take = write_tone(&studio.dir.path().join("extra.wav"));
    studio.then_the_user(UserEdit::Tool("remove_track", json!({ "track": 0 })));

    let result = studio.dispatch("load", json!({ "path": take }), "");

    assert!(matches!(result, ToolResult::Ok(_)), "{result:?}");
    assert_eq!(studio.runs(), 2);
    assert_eq!(studio.track_names(), ["vocals", "bass", "extra"]);
}

/// The milder variant: the track is the same one, but its clips are not
/// where the arguments think. `fade` takes absolute seconds, so after the
/// user drags the clip the same seconds are different audio.
#[test]
fn a_clip_moved_while_a_ranged_tool_ran_does_not_send_the_rerun_to_other_audio() {
    let studio = Studio::new(Box::new(tools::tool::FadeTool));
    let drums = studio.clips_of("drums");
    studio.then_the_user(UserEdit::Tool(
        "move_clip",
        json!({ "track": 0, "clip_index": 0, "start_sec": 0.5 }),
    ));

    let result = studio.dispatch(
        "fade",
        json!({ "track": 0, "kind": "out", "range": { "start_sec": 0.5, "end_sec": 1.0 } }),
        "",
    );

    let moved = studio.clips_of("drums");
    assert_ne!(moved[0].0, drums[0].0, "the user's move is kept");
    assert_eq!(
        moved[0].3, drums[0].3,
        "the fade was applied to a clip it was not meant for: {result:?}"
    );
    let said = error_text(&result);
    assert!(
        said.contains("not applied") && said.contains("drums") && said.contains("clips"),
        "{said}"
    );
    assert_eq!(studio.runs(), 1);
}

/// The same edit on another track is nothing to the call.
#[test]
fn a_clip_moved_on_another_track_leaves_the_rerun_alone() {
    let studio = Studio::new(Box::new(tools::tool::FadeTool));
    let drums = studio.clips_of("drums");
    studio.then_the_user(UserEdit::Tool(
        "move_clip",
        json!({ "track": 1, "clip_index": 0, "start_sec": 0.5 }),
    ));

    let result = studio.dispatch(
        "fade",
        json!({ "track": 0, "kind": "out", "range": { "start_sec": 0.5, "end_sec": 1.0 } }),
        "",
    );

    assert!(matches!(result, ToolResult::Ok(_)), "{result:?}");
    assert_eq!(studio.runs(), 2);
    assert_ne!(
        studio.clips_of("drums")[0].3,
        drums[0].3,
        "drums were not faded"
    );
    assert_ne!(studio.clips_of("vocals")[0].0, 0, "the user's move is kept");
}

/// A range taken from the chat message is a range in seconds like any
/// other.
#[test]
fn a_clip_moved_while_a_tool_ran_on_the_chat_selection_is_refused_too() {
    let studio = Studio::new(Box::new(tools::tool::FadeTool));
    let drums = studio.clips_of("drums");
    studio.then_the_user(UserEdit::Tool(
        "move_clip",
        json!({ "track": 0, "clip_index": 0, "start_sec": 0.5 }),
    ));

    let result = studio.dispatch(
        "fade",
        json!({ "track": 0, "kind": "out" }),
        "[apply to 0:00-0:01] fade that out",
    );

    assert_eq!(studio.clips_of("drums")[0].3, drums[0].3, "{result:?}");
    assert!(error_text(&result).contains("not applied"), "{result:?}");
}

/// A refusal costs the session nothing but what the user did.
#[test]
fn a_refused_call_adds_no_node() {
    let studio = Studio::new(Box::new(tools::tool::TimeStretchTool));
    let nodes = studio.node_count();
    studio.then_the_user(UserEdit::Tool("remove_track", json!({ "track": 0 })));

    studio.dispatch("time_stretch", json!({ "track": 1, "factor": 0.5 }), "");

    assert_eq!(studio.node_count(), nodes + 1, "only the user's removal");
}
