//! With Plan first on, no edit runs without approval (#415).
//!
//! The plan card already covered the case where the model writes a plan.
//! When it does not (it chose not to, or the planning request failed in
//! any of the ways #267 named), the turn used to go ahead and the tools
//! ran unapproved. Now the first model step that includes a call which
//! can change the session is held before anything in it dispatches, and
//! its concrete tool calls are what the user is asked to approve.
//!
//! These drive the public `Agent` API against a mocked provider. The two
//! fixture tools stand for the two kinds there are: `edit` mutates (the
//! default), `peek` says it does not. The test answers the gate from
//! inside `on_event`, the way the UI does: it sets what the answer is,
//! then fires the notifier, and the permit is consumed by the wait that
//! follows the emit.

mod common;

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use ai::anthropic::{ContentBlock, Role};
use ai::{Agent, AgentEvent, LlmConfig};
use common::{classifier_json, ok, plan_json, sse_text, sse_tool_step, Entry, SeqResponder};
use serde_json::{json, Value};
use tokio::sync::Notify;
use tools::schema::{anthropic_tool, object_schema};
use tools::{Tool, ToolContext, ToolDispatcher, ToolResult};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer};

// ---------------------------------------------------------------------
// Fixture tools
// ---------------------------------------------------------------------

/// Mutates, because it says nothing about it. Records what it ran with.
struct Edit {
    log: Arc<Mutex<Vec<Value>>>,
}

impl Tool for Edit {
    fn name(&self) -> &'static str {
        "edit"
    }
    fn schema(&self) -> Value {
        anthropic_tool(
            "edit",
            "Changes the session.",
            object_schema(&[("track", "integer", true)]),
        )
    }
    fn invoke(&self, args: Value, _ctx: &mut ToolContext) -> tools::Result<ToolResult> {
        self.log.lock().unwrap().push(args);
        Ok(ToolResult::Ok(json!({ "ok": true })))
    }
}

/// Reads only, and says so. Counts its calls.
struct Peek {
    calls: Arc<AtomicUsize>,
}

impl Tool for Peek {
    fn name(&self) -> &'static str {
        "peek"
    }
    fn schema(&self) -> Value {
        anthropic_tool(
            "peek",
            "Reads the session.",
            object_schema(&[("track", "integer", true)]),
        )
    }
    fn mutates(&self) -> bool {
        false
    }
    fn invoke(&self, _args: Value, _ctx: &mut ToolContext) -> tools::Result<ToolResult> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(ToolResult::Ok(json!({ "ok": true })))
    }
}

// ---------------------------------------------------------------------
// Fixture
// ---------------------------------------------------------------------

/// What the user does to a card.
enum Answer {
    Approve,
    Reject,
    Revise(String),
    /// The user walks away: nothing answers, and the gate times out.
    Ignore,
}

/// The events of a turn, reduced to what these tests compare.
#[derive(Debug, Clone, PartialEq)]
enum Ev {
    Text,
    ToolStart(String),
    ToolEnd(String, bool),
    /// Announced and never dispatched: declined, reworded or unanswered.
    NotRun(String),
    Node,
    Done,
    /// The card, with how much had run by the time it was shown.
    Plan {
        steps: Vec<Value>,
        edits_run: usize,
        peeks_run: usize,
    },
    PlanRejected,
    Unavailable {
        reason: String,
        held: bool,
    },
}

struct Fixture {
    server: MockServer,
    _dir: tempfile::TempDir,
    agent: Agent,
    notify: Arc<Notify>,
    rejected: Arc<AtomicBool>,
    slot: Arc<Mutex<Option<String>>>,
    edits: Arc<Mutex<Vec<Value>>>,
    peeks: Arc<AtomicUsize>,
}

impl Fixture {
    async fn new(plan_first: bool, whitelist: Option<Vec<String>>, responses: Vec<Entry>) -> Self {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(SeqResponder::new(responses))
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().expect("tempdir");
        let edits = Arc::new(Mutex::new(Vec::new()));
        let peeks = Arc::new(AtomicUsize::new(0));
        let mut dispatcher = ToolDispatcher::new();
        dispatcher.register(Box::new(Edit {
            log: Arc::clone(&edits),
        }));
        dispatcher.register(Box::new(Peek {
            calls: Arc::clone(&peeks),
        }));

        let notify = Arc::new(Notify::new());
        let rejected = Arc::new(AtomicBool::new(false));
        let slot = Arc::new(Mutex::new(None));
        let cfg = LlmConfig::new_anthropic("test-key").with_base_url(server.uri());
        let mut agent = Agent::new(
            cfg,
            Arc::new(Mutex::new(dispatcher)),
            Arc::new(Mutex::new(
                session::Store::open(dir.path()).expect("open store"),
            )),
            Arc::new(Mutex::new(audio_engine::Engine::new())),
            Arc::clone(&notify),
            Arc::clone(&slot),
            Arc::clone(&rejected),
            Arc::new(Mutex::new(None::<tools::Clipboard>)),
        );
        if let Some(whitelist) = whitelist {
            agent = agent.with_tool_whitelist(whitelist);
        }
        agent.set_plan_first(plan_first);

        Self {
            server,
            _dir: dir,
            agent,
            notify,
            rejected,
            slot,
            edits,
            peeks,
        }
    }

    /// Run one turn, answering each card with `answer(n, steps)` where
    /// `n` counts the cards of this turn from zero.
    async fn turn(
        &mut self,
        message: &str,
        mut answer: impl FnMut(usize, &[Value]) -> Answer,
    ) -> (ai::Result<ai::TurnResult>, Vec<Ev>) {
        let notify = Arc::clone(&self.notify);
        let rejected = Arc::clone(&self.rejected);
        let slot = Arc::clone(&self.slot);
        let edits = Arc::clone(&self.edits);
        let peeks = Arc::clone(&self.peeks);

        let mut events = Vec::new();
        let mut cards = 0usize;
        let result = self
            .agent
            .turn(message.to_string(), |event| match event {
                AgentEvent::TextDelta(_) => events.push(Ev::Text),
                AgentEvent::ToolCallStart { id, .. } => events.push(Ev::ToolStart(id)),
                AgentEvent::ToolCallEnd { id, ok, .. } => events.push(Ev::ToolEnd(id, ok)),
                AgentEvent::ToolCallNotRun { id } => events.push(Ev::NotRun(id)),
                AgentEvent::NodeCreated(_) => events.push(Ev::Node),
                AgentEvent::Done => events.push(Ev::Done),
                AgentEvent::PlanRejected => events.push(Ev::PlanRejected),
                AgentEvent::PlanUnavailable {
                    reason,
                    first_edit_held,
                } => events.push(Ev::Unavailable {
                    reason,
                    held: first_edit_held,
                }),
                AgentEvent::Plan { steps } => {
                    events.push(Ev::Plan {
                        steps: steps.clone(),
                        edits_run: edits.lock().unwrap().len(),
                        peeks_run: peeks.load(Ordering::SeqCst),
                    });
                    let answered = answer(cards, &steps);
                    cards += 1;
                    match answered {
                        Answer::Approve => notify.notify_one(),
                        Answer::Reject => {
                            rejected.store(true, Ordering::SeqCst);
                            notify.notify_one();
                        }
                        Answer::Revise(text) => {
                            *slot.lock().unwrap() = Some(text);
                            notify.notify_one();
                        }
                        // Freezing the clock here, after every HTTP request
                        // so far has completed, lets tokio's auto-advance
                        // jump to the gate's five-minute deadline as soon
                        // as the runtime goes idle. There is no real wait,
                        // and nothing auto-advances under in-flight
                        // mock-server I/O. `pause` is legal because
                        // `#[tokio::test]` is current_thread and this runs
                        // inside the runtime's context. The timeout in
                        // `approval::await_answer` is created after the
                        // emit, so its deadline is on the paused clock. At
                        // most one card per test may do this: a second
                        // `pause` panics.
                        Answer::Ignore => tokio::time::pause(),
                    }
                }
            })
            .await;
        (result, events)
    }

    /// The JSON bodies the provider was sent, in order.
    async fn requests(&self) -> Vec<Value> {
        self.server
            .received_requests()
            .await
            .expect("recording is on")
            .iter()
            .map(|r| serde_json::from_slice(&r.body).expect("a JSON body"))
            .collect()
    }

    fn edits_run(&self) -> Vec<Value> {
        self.edits.lock().unwrap().clone()
    }
}

/// Classifier, a plan reply with no plan in it, then the model's steps.
fn without_a_plan(steps: Vec<String>) -> Vec<Entry> {
    let mut responses = vec![ok(classifier_json("general")), ok(plan_json("Sure."))];
    responses.extend(steps.into_iter().map(ok));
    responses
}

fn edit_track(id: &str, track: u32) -> String {
    sse_tool_step(&[(id, "edit", &format!(r#"{{"track":{track}}}"#))])
}

fn cards(events: &[Ev]) -> Vec<&Ev> {
    events
        .iter()
        .filter(|e| matches!(e, Ev::Plan { .. }))
        .collect()
}

fn position(events: &[Ev], wanted: impl Fn(&Ev) -> bool) -> usize {
    events
        .iter()
        .position(wanted)
        .unwrap_or_else(|| panic!("event not found in {events:#?}"))
}

fn approve(_: usize, _: &[Value]) -> Answer {
    Answer::Approve
}

// ---------------------------------------------------------------------
// The path that was already there
// ---------------------------------------------------------------------

#[tokio::test]
async fn a_written_plan_is_approved_exactly_as_before() {
    let plan = r#"<plan>[{"step":1,"tool":"edit","description":"Edit track 0"}]</plan>"#;
    let mut fx = Fixture::new(
        true,
        None,
        vec![
            ok(classifier_json("general")),
            ok(plan_json(plan)),
            ok(edit_track("t1", 0)),
            ok(sse_text("Done.")),
        ],
    )
    .await;

    let (result, events) = fx.turn("make it louder", approve).await;
    result.expect("turn");

    let cards = cards(&events);
    assert_eq!(cards.len(), 1, "{events:#?}");
    assert!(
        matches!(
            cards[0],
            Ev::Plan { steps, .. }
                if *steps == vec![json!({"step":1,"tool":"edit","description":"Edit track 0"})]
        ),
        "the model's own steps, verbatim: {cards:?}"
    );
    assert!(
        !events.iter().any(|e| matches!(e, Ev::Unavailable { .. })),
        "a plan was produced: {events:#?}"
    );
    assert_eq!(fx.edits_run(), vec![json!({"track": 0})]);
    assert_eq!(events.last(), Some(&Ev::Done));
    assert_eq!(fx.requests().await.len(), 4);
}

// ---------------------------------------------------------------------
// No plan: the first edit is held
// ---------------------------------------------------------------------

#[tokio::test]
async fn no_plan_block_holds_the_first_edit_until_approved() {
    let mut fx = Fixture::new(
        true,
        None,
        without_a_plan(vec![edit_track("t1", 0), sse_text("Done.")]),
    )
    .await;

    let (result, events) = fx.turn("make it louder", approve).await;
    result.expect("turn");

    let unavailable = position(&events, |e| matches!(e, Ev::Unavailable { .. }));
    let card = position(&events, |e| matches!(e, Ev::Plan { .. }));
    assert!(unavailable < card, "{events:#?}");
    assert!(
        matches!(&events[unavailable], Ev::Unavailable { held: true, .. }),
        "Plan first is on, so the first edit is held: {:?}",
        events[unavailable]
    );

    match &events[card] {
        Ev::Plan {
            steps,
            edits_run,
            peeks_run,
        } => {
            assert_eq!(
                *steps,
                vec![json!({"step": 1, "tool": "edit", "description": "track: 0"})]
            );
            assert_eq!(*edits_run, 0, "the edit ran before it was approved");
            assert_eq!(*peeks_run, 0);
        }
        other => panic!("not a card: {other:?}"),
    }

    // The call was announced as it streamed, before the card.
    assert!(position(&events, |e| *e == Ev::ToolStart("t1".into())) < card);

    assert_eq!(fx.edits_run(), vec![json!({"track": 0})]);
    assert_eq!(events.last(), Some(&Ev::Done));
    assert_eq!(fx.requests().await.len(), 4);
}

#[tokio::test]
async fn a_rejected_step_runs_nothing_and_ends_the_turn() {
    let mut fx = Fixture::new(true, None, without_a_plan(vec![edit_track("t1", 0)])).await;

    let (result, events) = fx.turn("make it louder", |_, _| Answer::Reject).await;
    result.expect("a declined step is not an error");

    assert!(fx.edits_run().is_empty(), "a declined edit ran");
    // The held call reads "not run", not as a call that failed.
    let tail = &events[events.len() - 2..];
    assert_eq!(
        tail,
        [Ev::NotRun("t1".into()), Ev::PlanRejected],
        "{events:#?}"
    );
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, Ev::ToolEnd(id, _) if id == "t1")),
        "a call that never ran was reported as finished: {events:#?}"
    );
    assert!(
        !events.contains(&Ev::Done),
        "a declined turn does not finish"
    );
    assert_eq!(
        fx.requests().await.len(),
        3,
        "no follow-up model call after a decline"
    );

    // The model's tool_use is answered, so the conversation stays valid.
    let last = fx.agent.conversation().last().expect("a message");
    assert_eq!(last.role, Role::User);
    assert!(
        matches!(
            last.content.as_slice(),
            [ContentBlock::ToolResult { tool_use_id, content, is_error: Some(true) }]
                if tool_use_id == "t1" && content.contains("Declined by the user")
        ),
        "{last:?}"
    );
}

#[tokio::test]
async fn the_turn_after_a_rejection_is_well_formed() {
    let mut responses = without_a_plan(vec![edit_track("t1", 0)]);
    responses.extend([ok(classifier_json("general")), ok(sse_text("Okay."))]);
    let mut fx = Fixture::new(true, None, responses).await;

    let (first, _) = fx.turn("make it louder", |_, _| Answer::Reject).await;
    first.expect("first turn");

    fx.agent.set_plan_first(false);
    let (second, events) = fx.turn("next request", approve).await;
    second.expect("second turn");
    assert_eq!(events.last(), Some(&Ev::Done));

    let requests = fx.requests().await;
    let main = &requests[4];
    let messages = main["messages"].as_array().expect("messages");
    for pair in messages.windows(2) {
        assert_ne!(
            pair[0]["role"], pair[1]["role"],
            "a role repeats, which providers reject: {messages:#?}"
        );
    }
    let last = messages.last().unwrap();
    assert_eq!(last["role"], "user");
    let blocks = last["content"].as_array().expect("blocks");
    assert_eq!(blocks.len(), 2, "{blocks:#?}");
    assert_eq!(blocks[0]["type"], "tool_result");
    assert_eq!(blocks[0]["tool_use_id"], "t1");
    assert_eq!(blocks[1]["type"], "text");
    assert_eq!(blocks[1]["text"], "next request");
}

#[tokio::test]
async fn an_unanswered_step_times_out_and_runs_nothing() {
    let mut responses = without_a_plan(vec![edit_track("t1", 0)]);
    responses.extend([ok(classifier_json("general")), ok(sse_text("Okay."))]);
    let mut fx = Fixture::new(true, None, responses).await;

    let (result, events) = fx.turn("make it louder", |_, _| Answer::Ignore).await;

    assert!(
        matches!(result, Err(ai::Error::PlanTimeout)),
        "an unanswered card ends the turn with a timeout: {result:?}"
    );
    assert!(fx.edits_run().is_empty(), "an unanswered edit ran");
    match cards(&events).as_slice() {
        [Ev::Plan { edits_run, .. }] => assert_eq!(*edits_run, 0),
        other => panic!("expected one card, got {other:?}"),
    }
    // The held call's badge is resolved as not run, and nothing follows
    // it: the turn neither finished nor was declined.
    assert_eq!(events.last(), Some(&Ev::NotRun("t1".into())), "{events:#?}");
    assert!(!events.contains(&Ev::Done), "{events:#?}");
    assert!(!events.contains(&Ev::PlanRejected), "{events:#?}");
    assert_eq!(
        fx.requests().await.len(),
        3,
        "no follow-up model call after a timeout"
    );

    // The model's tool_use is answered, so the conversation stays valid.
    let last = fx.agent.conversation().last().expect("a message");
    assert_eq!(last.role, Role::User);
    assert!(
        matches!(
            last.content.as_slice(),
            [ContentBlock::ToolResult { tool_use_id, content, is_error: Some(true) }]
                if tool_use_id == "t1"
                    && content.contains("did not answer the approval request")
        ),
        "{last:?}"
    );

    // The next turn builds on that: roles alternate, and the user's words
    // join the tool_result message instead of starting a second one.
    tokio::time::resume();
    fx.agent.set_plan_first(false);
    let (second, events) = fx.turn("next request", approve).await;
    second.expect("second turn");
    assert_eq!(events.last(), Some(&Ev::Done));

    let requests = fx.requests().await;
    let messages = requests[4]["messages"].as_array().expect("messages");
    for pair in messages.windows(2) {
        assert_ne!(
            pair[0]["role"], pair[1]["role"],
            "a role repeats, which providers reject: {messages:#?}"
        );
    }
    let blocks = messages.last().unwrap()["content"]
        .as_array()
        .expect("blocks");
    assert_eq!(blocks.len(), 2, "{blocks:#?}");
    assert_eq!(blocks[0]["type"], "tool_result");
    assert_eq!(blocks[0]["tool_use_id"], "t1");
    assert_eq!(blocks[1]["type"], "text");
    assert_eq!(blocks[1]["text"], "next request");
}

// ---------------------------------------------------------------------
// What is and is not held
// ---------------------------------------------------------------------

#[tokio::test]
async fn a_read_only_step_is_never_held() {
    let mut fx = Fixture::new(
        true,
        None,
        without_a_plan(vec![
            sse_tool_step(&[("p1", "peek", r#"{"track":0}"#)]),
            sse_text("Done."),
        ]),
    )
    .await;

    let (result, events) = fx.turn("what tempo is this?", approve).await;
    result.expect("turn");

    assert!(cards(&events).is_empty(), "a read was held: {events:#?}");
    assert_eq!(fx.peeks.load(Ordering::SeqCst), 1);
    assert_eq!(events.last(), Some(&Ev::Done));
}

#[tokio::test]
async fn a_step_mixing_reads_and_edits_is_held_whole() {
    let mut fx = Fixture::new(
        true,
        None,
        without_a_plan(vec![
            sse_tool_step(&[
                ("p1", "peek", r#"{"track":0}"#),
                ("e1", "edit", r#"{"track":1}"#),
            ]),
            sse_text("Done."),
        ]),
    )
    .await;

    let (result, events) = fx.turn("check it and fix it", approve).await;
    result.expect("turn");

    let cards = cards(&events);
    assert_eq!(cards.len(), 1, "{events:#?}");
    match cards[0] {
        Ev::Plan {
            steps,
            edits_run,
            peeks_run,
        } => {
            assert_eq!(
                *steps,
                vec![
                    json!({"step": 1, "tool": "peek", "description": "track: 0"}),
                    json!({"step": 2, "tool": "edit", "description": "track: 1"}),
                ]
            );
            assert_eq!((*edits_run, *peeks_run), (0, 0), "something ran first");
        }
        other => panic!("not a card: {other:?}"),
    }
    assert_eq!(fx.edits_run(), vec![json!({"track": 1})]);
    assert_eq!(fx.peeks.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn only_the_first_edit_is_held() {
    let mut fx = Fixture::new(
        true,
        None,
        without_a_plan(vec![
            edit_track("t1", 0),
            edit_track("t2", 1),
            sse_text("Done."),
        ]),
    )
    .await;

    let (result, events) = fx.turn("do two things", approve).await;
    result.expect("turn");

    assert_eq!(cards(&events).len(), 1, "{events:#?}");
    assert_eq!(
        fx.edits_run(),
        vec![json!({"track": 0}), json!({"track": 1})]
    );
}

// ---------------------------------------------------------------------
// Every way of getting no plan
// ---------------------------------------------------------------------

#[tokio::test]
async fn a_planning_fault_also_holds_the_first_edit() {
    let faults: [(u16, &str, &str); 2] = [
        (503, "upstream unavailable", "503"),
        (200, "this is not json", "could not be read"),
    ];
    for (status, body, names) in faults {
        let mut fx = Fixture::new(
            true,
            None,
            vec![
                ok(classifier_json("general")),
                (status, body.to_string()),
                ok(edit_track("t1", 0)),
                ok(sse_text("Done.")),
            ],
        )
        .await;

        let (result, events) = fx.turn("make it louder", approve).await;
        result.unwrap_or_else(|e| panic!("{status}: {e}"));

        let notice = position(&events, |e| matches!(e, Ev::Unavailable { .. }));
        match &events[notice] {
            Ev::Unavailable { reason, held } => {
                assert!(reason.contains(names), "{status}: {reason}");
                assert!(*held, "{status}: Plan first is on");
            }
            other => panic!("{other:?}"),
        }
        match cards(&events).as_slice() {
            [Ev::Plan { edits_run, .. }] => {
                assert_eq!(*edits_run, 0, "{status}: ran before approval")
            }
            other => panic!("{status}: expected one card, got {other:?}"),
        }
        assert_eq!(fx.edits_run(), vec![json!({"track": 0})], "{status}");
    }
}

#[tokio::test]
async fn an_empty_plan_is_treated_as_no_plan() {
    let mut fx = Fixture::new(
        true,
        None,
        vec![
            ok(classifier_json("general")),
            ok(plan_json("<plan>[]</plan>")),
            ok(edit_track("t1", 0)),
            ok(sse_text("Done.")),
        ],
    )
    .await;

    let (result, events) = fx.turn("make it louder", approve).await;
    result.expect("turn");

    let notice = position(&events, |e| matches!(e, Ev::Unavailable { .. }));
    match &events[notice] {
        Ev::Unavailable { reason, held } => {
            assert!(reason.contains("no steps"), "{reason}");
            assert!(*held);
        }
        other => panic!("{other:?}"),
    }
    // One card, and it is the held edit rather than a zero-step plan.
    match cards(&events).as_slice() {
        [Ev::Plan {
            steps, edits_run, ..
        }] => {
            assert_eq!(steps.len(), 1);
            assert_eq!(*edits_run, 0);
        }
        other => panic!("expected one card, got {other:?}"),
    }
}

#[tokio::test]
async fn mashup_without_plan_first_keeps_the_notice_only() {
    let mut fx = Fixture::new(
        false,
        None,
        vec![
            ok(classifier_json("mashup")),
            ok(plan_json("Sure.")),
            ok(edit_track("t1", 0)),
            ok(sse_text("Done.")),
        ],
    )
    .await;

    let (result, events) = fx.turn("mash these two up", approve).await;
    result.expect("turn");

    assert!(
        events
            .iter()
            .any(|e| matches!(e, Ev::Unavailable { held: false, .. })),
        "{events:#?}"
    );
    assert!(cards(&events).is_empty(), "no gate without Plan first");
    assert_eq!(fx.edits_run(), vec![json!({"track": 0})]);
    assert_eq!(events.last(), Some(&Ev::Done));
}

// ---------------------------------------------------------------------
// Revising the held step
// ---------------------------------------------------------------------

#[tokio::test]
async fn a_revised_step_runs_nothing_and_is_held_again() {
    let revision =
        "I've updated the plan. Please follow these revised steps instead:\n1. edit — track: 1";
    let mut fx = Fixture::new(
        true,
        None,
        without_a_plan(vec![
            edit_track("t1", 0),
            edit_track("t2", 1),
            sse_text("Done."),
        ]),
    )
    .await;

    let (result, events) = fx
        .turn("make it louder", |n, _| match n {
            0 => Answer::Revise(revision.to_string()),
            _ => Answer::Approve,
        })
        .await;
    result.expect("turn");

    let shown = cards(&events);
    assert_eq!(shown.len(), 2, "{events:#?}");
    for card in &shown {
        match card {
            Ev::Plan { edits_run, .. } => {
                assert_eq!(*edits_run, 0, "an edit ran before its card was approved")
            }
            other => panic!("{other:?}"),
        }
    }
    assert!(
        matches!(
            shown[1],
            Ev::Plan { steps, .. }
                if *steps == vec![json!({"step": 1, "tool": "edit", "description": "track: 1"})]
        ),
        "the second card shows the model's new proposal: {:?}",
        shown[1]
    );
    // The first card's call was resolved, not left "running".
    let first_card = position(&events, |e| matches!(e, Ev::Plan { .. }));
    let resolved = position(&events, |e| *e == Ev::NotRun("t1".into()));
    assert!(first_card < resolved, "{events:#?}");

    // Only what the user approved as shown ran.
    assert_eq!(fx.edits_run(), vec![json!({"track": 1})]);
    assert_eq!(events.last(), Some(&Ev::Done));

    // The model was told, and given the user's words, in one message.
    let requests = fx.requests().await;
    let after_revision = &requests[3]["messages"];
    let answer = after_revision
        .as_array()
        .and_then(|m| m.last())
        .expect("a last message");
    assert_eq!(answer["role"], "user");
    let blocks = answer["content"].as_array().expect("blocks");
    assert_eq!(blocks[0]["type"], "tool_result");
    assert_eq!(blocks[0]["tool_use_id"], "t1");
    assert!(
        blocks[0]["content"]
            .as_str()
            .unwrap()
            .contains("Not run: the user changed this step"),
        "{blocks:#?}"
    );
    assert_eq!(blocks[1]["type"], "text");
    assert_eq!(blocks[1]["text"], revision);
}

// ---------------------------------------------------------------------
// Calls that would be refused are not held
// ---------------------------------------------------------------------

#[tokio::test]
async fn a_call_that_would_be_refused_is_not_held() {
    // (a) A tool turned off for the turn, named anyway.
    let mut fx = Fixture::new(
        true,
        Some(vec!["peek".to_string()]),
        without_a_plan(vec![edit_track("t1", 0), sse_text("Okay.")]),
    )
    .await;
    let (result, events) = fx.turn("make it louder", approve).await;
    result.expect("turn");
    assert!(cards(&events).is_empty(), "{events:#?}");
    assert!(fx.edits_run().is_empty(), "a turned-off tool ran");
    let requests = fx.requests().await;
    assert!(
        requests[3].to_string().contains("turned off"),
        "the model was not told the tool is off"
    );

    // (b) Arguments that fail the tool's schema.
    let mut fx = Fixture::new(
        true,
        None,
        without_a_plan(vec![
            sse_tool_step(&[("t1", "edit", r#"{"track":"zero"}"#)]),
            sse_text("Okay."),
        ]),
    )
    .await;
    let (result, events) = fx.turn("make it louder", approve).await;
    result.expect("turn");
    assert!(cards(&events).is_empty(), "{events:#?}");
    assert!(fx.edits_run().is_empty());
}

// ---------------------------------------------------------------------
// A leftover answer cannot answer a new card
// ---------------------------------------------------------------------

#[tokio::test]
async fn a_stale_rejection_flag_cannot_decline_a_new_gate() {
    let mut fx = Fixture::new(
        true,
        None,
        without_a_plan(vec![edit_track("t1", 0), sse_text("Done.")]),
    )
    .await;
    // Left over from some earlier gate that never consumed it.
    fx.rejected.store(true, Ordering::SeqCst);

    let (result, events) = fx.turn("make it louder", approve).await;
    result.expect("turn");

    assert!(!events.contains(&Ev::PlanRejected), "{events:#?}");
    assert_eq!(fx.edits_run(), vec![json!({"track": 0})]);
    assert_eq!(events.last(), Some(&Ev::Done));
}
