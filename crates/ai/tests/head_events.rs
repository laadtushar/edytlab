//! The window follows the store's head, not a tool's `node_id` (#594).
//!
//! The app moves its head, and the undo path it keeps (`useSession`), on
//! `AgentEvent::NodeCreated`. The agent loop used to send one for any
//! `node_id` in a tool's result, which is right only for a tool whose
//! `node_id` happens to be the head it moved to. `name_node` returns the node
//! it labelled, which is not where the head is, so the window jumped there;
//! `apply_diff` returns `branches` and `apply_recipe` returns `head`, so the
//! window never heard that the store's head had moved.
//!
//! These drive the public `Agent` API against a mocked provider with the
//! real tools, and compare the events with the store's head after the turn.

mod common;

use std::sync::{Arc, Mutex};

use ai::{Agent, AgentEvent, LlmConfig};
use common::{classifier_json, ok, sse_text, sse_tool_step, Entry, SeqResponder};
use serde_json::{json, Value};
use session::{Annotation, AnnotationId, AnnotationKind, NodeId, Store};
use tools::ToolDispatcher;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer};

fn marker(name: &str, time_sec: f64) -> Annotation {
    Annotation {
        id: AnnotationId::new(),
        name: name.to_string(),
        kind: AnnotationKind::Marker { time_sec },
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

/// A project whose history is `base -> one -> two`, each a marker added by
/// the user. The head is `two`.
struct Project {
    dir: tempfile::TempDir,
    store: Store,
    one: NodeId,
    two: NodeId,
}

fn project() -> Project {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut store = Store::open(dir.path()).expect("open store");
    let base = store.append(base_node()).expect("base");
    let one = store
        .add_annotation(base, marker("one", 1.0))
        .expect("first edit");
    let two = store
        .add_annotation(one, marker("two", 2.0))
        .expect("second edit");
    assert_eq!(store.head(), Some(two));
    Project {
        dir,
        store,
        one,
        two,
    }
}

/// What a turn did.
struct Turn {
    result: ai::Result<ai::TurnResult>,
    /// `(id, ok)` per finished call.
    ended: Vec<(String, bool)>,
    /// Every `NodeCreated`, in order.
    created: Vec<NodeId>,
    /// What the model was sent back for the call.
    tool_result: Option<String>,
    store: Arc<Mutex<Store>>,
    _dir: tempfile::TempDir,
}

impl Turn {
    fn head(&self) -> NodeId {
        self.store.lock().unwrap().head().expect("a head")
    }

    fn markers(&self, id: NodeId) -> Vec<String> {
        self.store
            .lock()
            .unwrap()
            .get(id)
            .expect("node")
            .state
            .annotations
            .into_iter()
            .map(|a| a.name)
            .collect()
    }
}

/// One turn in which the model makes one call, `tool` with `args`, with
/// the default tool set, and then says it is done.
async fn turn(project: Project, tool: &str, args: Value) -> Turn {
    let server = MockServer::start().await;
    let args = args.to_string();
    let responses: Vec<Entry> = vec![
        ok(classifier_json("general")),
        ok(sse_tool_step(&[("t1", tool, &args)])),
        ok(sse_text("Done.")),
    ];
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(SeqResponder::new(responses))
        .mount(&server)
        .await;

    let store = Arc::new(Mutex::new(project.store));
    let cfg = LlmConfig::new_anthropic("test-key").with_base_url(server.uri());
    let mut agent = Agent::new(
        cfg,
        Arc::new(Mutex::new(ToolDispatcher::default_dispatcher())),
        Arc::clone(&store),
        Arc::new(Mutex::new(audio_engine::Engine::new())),
        ai::PlanGate::new(),
        Arc::new(Mutex::new(None::<tools::Clipboard>)),
    );
    agent.set_plan_first(false);

    let mut ended = Vec::new();
    let mut created = Vec::new();
    let result = agent
        .turn("go".to_string(), |event| match event {
            AgentEvent::ToolCallEnd { id, ok, .. } => ended.push((id, ok)),
            AgentEvent::NodeCreated(id) => created.push(id),
            _ => {}
        })
        .await;

    // The third request carries the call's result back to the model.
    let requests = server.received_requests().await.unwrap_or_default();
    let tool_result = requests.get(2).and_then(|r| {
        let body: Value = serde_json::from_slice(&r.body).ok()?;
        body["messages"]
            .as_array()?
            .iter()
            .flat_map(|m| m["content"].as_array().cloned().unwrap_or_default())
            .find(|b| b["type"] == "tool_result" && b["tool_use_id"] == "t1")
            .and_then(|b| b["content"].as_str().map(str::to_string))
    });

    Turn {
        result,
        ended,
        created,
        tool_result,
        store,
        _dir: project.dir,
    }
}

/// A recipe file in the project's folder, with one `label` step per
/// `(name, args)`.
fn recipe(project: &Project, steps: &[Value]) -> String {
    let recipe = json!({
        "format_version": 1,
        "engine_version": "test",
        "steps": steps
            .iter()
            .map(|params| json!({ "tool": "label", "params": params, "replayable": true }))
            .collect::<Vec<_>>(),
    });
    let path = project.dir.path().join("recipe.json");
    std::fs::write(&path, recipe.to_string()).expect("write recipe");
    path.to_string_lossy().into_owned()
}

/// Labelling a node is not a move. The window stays on the head, so undo
/// still goes back along the path the user took.
///
/// RED before #594: the tool's result names the labelled node as
/// `node_id`, and `created` was `[one]`, a node that is not the head.
#[tokio::test]
async fn name_node_moves_nothing_and_reports_nothing() {
    let p = project();
    let (one, two) = (p.one, p.two);

    let t = turn(
        p,
        "name_node",
        json!({ "node_id": one.to_hex(), "label": "first take" }),
    )
    .await;

    assert!(t.result.is_ok(), "{:?}", t.result);
    assert_eq!(t.ended, vec![("t1".to_string(), true)]);
    assert_eq!(t.head(), two, "the store's head moved");
    assert_eq!(
        t.created,
        Vec::<NodeId>::new(),
        "the window was told to move to the node that was labelled"
    );
    assert_eq!(
        t.store.lock().unwrap().get(one).unwrap().label.as_deref(),
        Some("first take"),
        "the label was not set"
    );
    // The model still reads which node it labelled.
    let sent = t.tool_result.expect("the call's result went back");
    assert!(sent.contains(&one.to_hex()), "{sent}");
}

/// `apply_diff` moves the store's head to its last branch.
///
/// RED before #594: its result has `branches` and no `node_id`, and
/// `created` was empty.
#[tokio::test]
async fn apply_diff_reports_the_head_it_moves_to() {
    let p = project();
    let two = p.two;
    let rate = |hz: u32| {
        json!({
            "added": [],
            "removed": [],
            "modified": [[
                { "op": "sample_rate", "value": 48_000 },
                { "op": "sample_rate", "value": hz }
            ]]
        })
    };

    let t = turn(
        p,
        "apply_diff",
        json!({
            "from_node": two.to_hex(),
            "branches": [
                { "ops": rate(44_100), "label": "44.1k" },
                { "ops": rate(32_000), "label": "32k" }
            ]
        }),
    )
    .await;

    assert!(t.result.is_ok(), "{:?}", t.result);
    assert_eq!(t.ended, vec![("t1".to_string(), true)]);
    let head = t.head();
    assert_ne!(head, two, "apply_diff did not move the head");
    assert_eq!(
        t.store.lock().unwrap().get(head).unwrap().state.sample_rate,
        32_000,
        "the head is not the last branch"
    );
    assert_eq!(t.created, vec![head], "the window was not told");
}

/// `apply_recipe` moves the head once per step. The window hears once,
/// with the head the replay ended on.
///
/// RED before #594: its result has `head` and no `node_id`, and `created`
/// was empty.
#[tokio::test]
async fn apply_recipe_reports_the_head_it_ends_on_once() {
    let p = project();
    let two = p.two;
    let path = recipe(
        &p,
        &[
            json!({ "name": "intro", "time": 3.0 }),
            json!({ "name": "outro", "time": 4.0 }),
        ],
    );

    let t = turn(p, "apply_recipe", json!({ "path": path })).await;

    assert!(t.result.is_ok(), "{:?}", t.result);
    assert_eq!(t.ended, vec![("t1".to_string(), true)]);
    let head = t.head();
    assert_eq!(t.markers(head), vec!["one", "two", "intro", "outro"]);
    let middle = t.store.lock().unwrap().get(head).unwrap().parent;
    assert_eq!(
        middle.and_then(|m| t.store.lock().unwrap().get(m).unwrap().parent),
        Some(two)
    );
    assert_eq!(
        t.created,
        vec![head],
        "the window should hear of the final head, once"
    );
}

/// A call that moves the head and then fails still moved it: a replay that
/// stops at a bad step keeps the steps before it. The window must follow,
/// or its next undo starts from a head the store is not on.
///
/// RED before #594: nothing was reported for a failed call.
#[tokio::test]
async fn a_call_that_fails_after_moving_the_head_still_reports_it() {
    let p = project();
    let two = p.two;
    let path = recipe(
        &p,
        &[
            json!({ "name": "intro", "time": 3.0 }),
            // A region that ends before it starts: `label` refuses it.
            json!({ "name": "bad", "start": 5.0, "end": 4.0 }),
        ],
    );

    let t = turn(p, "apply_recipe", json!({ "path": path })).await;

    assert!(t.result.is_ok(), "{:?}", t.result);
    assert_eq!(t.ended, vec![("t1".to_string(), false)], "the call failed");
    let head = t.head();
    assert_ne!(head, two, "the first step was not kept");
    assert_eq!(t.markers(head), vec!["one", "two", "intro"]);
    assert_eq!(t.created, vec![head]);
}

/// A move to a node that already exists is a move. The window records it
/// as a step, as it did when this went by the result's `node_id`.
#[tokio::test]
async fn revert_to_reports_the_node_it_moves_back_to() {
    let p = project();
    let one = p.one;

    let t = turn(p, "revert_to", json!({ "target": one.to_hex() })).await;

    assert!(t.result.is_ok(), "{:?}", t.result);
    assert_eq!(t.ended, vec![("t1".to_string(), true)]);
    assert_eq!(t.head(), one);
    assert_eq!(t.created, vec![one]);
}

/// A call that leaves the head where it is reports nothing, whatever its
/// result says: `revert_to` the head itself returns it as `node_id`.
#[tokio::test]
async fn a_call_that_leaves_the_head_where_it_is_reports_nothing() {
    let p = project();
    let two = p.two;

    let t = turn(p, "revert_to", json!({ "target": two.to_hex() })).await;

    assert!(t.result.is_ok(), "{:?}", t.result);
    assert_eq!(t.ended, vec![("t1".to_string(), true)]);
    assert_eq!(t.head(), two);
    assert_eq!(t.created, Vec::<NodeId>::new());
}
