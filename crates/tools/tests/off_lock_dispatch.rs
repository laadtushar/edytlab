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
    DispatchError, Shared, Tool, ToolContext, ToolDispatcher, ToolResult, OFF_LOCK_ATTEMPTS,
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
        let base = store
            .append(session::SessionNode {
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
            })
            .expect("base node");
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
    assert!(matches!(unknown, Err(DispatchError::Unknown(_))));

    let invalid = shared().dispatch("probe", json!({ "x": 1 }), "", None);
    assert!(matches!(invalid, Err(DispatchError::SchemaValidation(_))));

    let allowed: std::collections::HashSet<String> = ["other".to_string()].into();
    let refused = shared().dispatch("probe", json!({}), "", Some(&allowed));
    assert!(matches!(refused, Err(DispatchError::NotPermitted(_))));
    assert_eq!(app.calls(), 0, "a refused call ran the tool");
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
