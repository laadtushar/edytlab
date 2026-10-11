//! Plan first does not hold a preview or an audition (#535), and still
//! holds an edit.
//!
//! `audition_effect` and `render_preview` were held with Plan first on
//! because `render_preview` could leave an uncapped temp file outside the
//! project. Both now write only into the bounded preview caches, which
//! `Tool::mutates` does not count, so a turn that only auditions and
//! previews runs without a card, and one that also edits is held as before.
//!
//! These drive the public `Agent` against a mocked provider with the
//! app's real tools (`ToolDispatcher::default_dispatcher`) on a real
//! session, because the claim is about those tools and not about a
//! fixture's. `tests/plan_first_gate.rs` covers the gate itself.

mod common;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use ai::{Agent, AgentEvent, LlmConfig};
use common::{classifier_json, ok, plan_json, sse_text, sse_tool_step, SeqResponder};
use serde_json::{json, Value};
use tokio::sync::Notify;
use tools::{ToolContext, ToolDispatcher, ToolResult};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer};

/// The turn's events, reduced to what these tests compare.
#[derive(Debug, Clone, PartialEq)]
enum Ev {
    ToolEnd(String, bool),
    NotRun(String),
    Plan(Vec<Value>),
    PlanRejected,
    Done,
}

struct Project {
    server: MockServer,
    dir: tempfile::TempDir,
    store: Arc<Mutex<session::Store>>,
    agent: Agent,
    notify: Arc<Notify>,
    rejected: Arc<AtomicBool>,
    head: String,
}

fn write_tone(path: &std::path::Path) {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 44_100,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec).expect("wav writer");
    for i in 0..44_100 {
        let t = i as f32 / 44_100.0;
        let s = (t * 440.0 * std::f32::consts::TAU).sin() * 0.5;
        writer.write_sample((s * i16::MAX as f32) as i16).unwrap();
    }
    writer.finalize().unwrap();
}

impl Project {
    /// A one-second session with Plan first on, whose provider will answer
    /// the classifier, a plan request with no plan in it, then `steps`.
    async fn new(steps: impl FnOnce(&str) -> Vec<String>) -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let wav = dir.path().join("take.wav");
        write_tone(&wav);

        let dispatcher = ToolDispatcher::default_dispatcher();
        let mut store = session::Store::open(dir.path()).expect("open store");
        let mut engine = audio_engine::Engine::new();
        let mut clipboard: Option<tools::Clipboard> = None;
        let loaded = dispatcher
            .invoke(
                "load",
                json!({ "path": wav.to_string_lossy() }),
                &mut ToolContext {
                    store: &mut store,
                    engine: &mut engine,
                    user_message: "",
                    clipboard: &mut clipboard,
                    allowed_tools: None,
                },
            )
            .expect("load dispatches");
        assert!(matches!(loaded, ToolResult::Ok(_)), "load: {loaded:?}");
        let head = store.head().expect("a head after load").to_hex();

        let mut responses = vec![ok(classifier_json("general")), ok(plan_json("Sure."))];
        responses.extend(steps(&head).into_iter().map(ok));
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(SeqResponder::new(responses))
            .mount(&server)
            .await;

        let store = Arc::new(Mutex::new(store));
        let notify = Arc::new(Notify::new());
        let rejected = Arc::new(AtomicBool::new(false));
        let slot = Arc::new(Mutex::new(None));
        let mut agent = Agent::new(
            LlmConfig::new_anthropic("test-key").with_base_url(server.uri()),
            Arc::new(Mutex::new(dispatcher)),
            Arc::clone(&store),
            Arc::new(Mutex::new(engine)),
            Arc::clone(&notify),
            slot,
            Arc::clone(&rejected),
            Arc::new(Mutex::new(None::<tools::Clipboard>)),
        );
        agent.set_plan_first(true);

        Self {
            server,
            dir,
            store,
            agent,
            notify,
            rejected,
            head,
        }
    }

    /// Run one turn. A card is approved if `approve` is true, declined if
    /// not; either way the test goes on instead of waiting for a person.
    async fn turn(&mut self, approve: bool) -> Vec<Ev> {
        let notify = Arc::clone(&self.notify);
        let rejected = Arc::clone(&self.rejected);
        let mut events = Vec::new();
        self.agent
            .turn("try a limiter on it".to_string(), |event| match event {
                AgentEvent::ToolCallEnd { id, ok, .. } => events.push(Ev::ToolEnd(id, ok)),
                AgentEvent::ToolCallNotRun { id } => events.push(Ev::NotRun(id)),
                AgentEvent::PlanRejected => events.push(Ev::PlanRejected),
                AgentEvent::Done => events.push(Ev::Done),
                AgentEvent::Plan { steps } => {
                    events.push(Ev::Plan(steps));
                    if !approve {
                        rejected.store(true, Ordering::SeqCst);
                    }
                    notify.notify_one();
                }
                _ => {}
            })
            .await
            .expect("the turn");
        events
    }

    fn node_count(&self) -> usize {
        self.store.lock().unwrap().list_nodes().unwrap().len()
    }

    fn head_now(&self) -> String {
        self.store.lock().unwrap().head().unwrap().to_hex()
    }

    /// Names of the finished entries in `.audiograph/<cache>/`.
    fn cached(&self, cache: &str) -> Vec<String> {
        let dir = self.dir.path().join(".audiograph").join(cache);
        std::fs::read_dir(dir)
            .map(|read| {
                read.flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default()
    }

    async fn requests(&self) -> usize {
        self.server
            .received_requests()
            .await
            .expect("recording is on")
            .len()
    }
}

fn audition(id: &str) -> (String, String, String) {
    (
        id.to_string(),
        "audition_effect".to_string(),
        r#"{"track":0,"kind":"gain","params":{"db":3.0}}"#.to_string(),
    )
}

fn preview(id: &str, node: &str) -> (String, String, String) {
    (
        id.to_string(),
        "render_preview".to_string(),
        format!(r#"{{"node_id":"{node}"}}"#),
    )
}

fn ranged_preview(id: &str, node: &str) -> (String, String, String) {
    (
        id.to_string(),
        "render_preview".to_string(),
        format!(r#"{{"node_id":"{node}","range":[0,4000]}}"#),
    )
}

fn gain(id: &str) -> (String, String, String) {
    (
        id.to_string(),
        "gain".to_string(),
        r#"{"track":0,"db":3.0}"#.to_string(),
    )
}

fn step(calls: &[(String, String, String)]) -> String {
    let borrowed: Vec<(&str, &str, &str)> = calls
        .iter()
        .map(|(a, b, c)| (a.as_str(), b.as_str(), c.as_str()))
        .collect();
    sse_tool_step(&borrowed)
}

// ---------------------------------------------------------------------

#[tokio::test]
async fn an_audition_and_a_preview_run_without_a_card() {
    let mut p = Project::new(|head| {
        vec![
            step(&[
                audition("t1"),
                preview("t2", head),
                ranged_preview("t3", head),
            ]),
            sse_text("Heard it."),
        ]
    })
    .await;
    let nodes = p.node_count();

    let events = p.turn(true).await;

    assert!(
        !events.iter().any(|e| matches!(e, Ev::Plan(_))),
        "Plan first held a call that cannot change the project: {events:#?}"
    );
    assert_eq!(
        events,
        vec![
            Ev::ToolEnd("t1".into(), true),
            Ev::ToolEnd("t2".into(), true),
            Ev::ToolEnd("t3".into(), true),
            Ev::Done,
        ]
    );

    // They ran: three files, all in the caches, and no node.
    assert_eq!(p.cached("auditions").len(), 1);
    assert_eq!(p.cached("previews").len(), 2, "the mix and the excerpt");
    assert_eq!(p.node_count(), nodes, "a preview appended a node");
    assert_eq!(p.head_now(), p.head, "a preview moved the head");
    // Classifier, plan request, the step, the follow-up.
    assert_eq!(p.requests().await, 4);
}

#[tokio::test]
async fn an_edit_is_still_held() {
    let mut p = Project::new(|_| vec![step(&[gain("t1")]), sse_text("Done.")]).await;
    let nodes = p.node_count();

    let events = p.turn(false).await;

    assert!(
        matches!(&events[..], [Ev::Plan(steps), ..]
            if steps.len() == 1 && steps[0]["tool"] == "gain"),
        "the edit should be held on a card of its own: {events:#?}"
    );
    assert!(
        events.ends_with(&[Ev::NotRun("t1".into()), Ev::PlanRejected]),
        "{events:#?}"
    );
    assert_eq!(p.node_count(), nodes, "a declined edit ran");
    assert_eq!(p.head_now(), p.head);
}

/// Approving a held step runs all of it, so the card lists all of it: an
/// audition next to an edit is held with the edit, and shown.
#[tokio::test]
async fn a_preview_next_to_an_edit_is_held_with_it() {
    let mut p = Project::new(|head| {
        vec![
            step(&[audition("t1"), gain("t2"), preview("t3", head)]),
            sse_text("Done."),
        ]
    })
    .await;
    let nodes = p.node_count();

    let events = p.turn(false).await;

    let Some(Ev::Plan(steps)) = events.first() else {
        panic!("the step has an edit in it, so it is held: {events:#?}");
    };
    let tools: Vec<&str> = steps.iter().map(|s| s["tool"].as_str().unwrap()).collect();
    assert_eq!(tools, ["audition_effect", "gain", "render_preview"]);
    assert_eq!(p.node_count(), nodes);
    assert!(
        p.cached("auditions").is_empty() && p.cached("previews").is_empty(),
        "nothing in a declined step runs, the previews included"
    );

    // And approved, the whole step runs.
    let mut p = Project::new(|head| {
        vec![
            step(&[audition("t1"), gain("t2"), preview("t3", head)]),
            sse_text("Done."),
        ]
    })
    .await;
    let events = p.turn(true).await;
    assert!(events.iter().any(|e| matches!(e, Ev::Plan(_))));
    assert_eq!(events.last(), Some(&Ev::Done));
    assert_eq!(p.node_count(), nodes + 1, "the approved edit did not run");
}
