//! A turn that reaches its tool budget ends with a summary, not an error
//! (#439).
//!
//! The budget used to be a hard cap that returned `Err` from the turn,
//! after the edits before it had been applied: a real request to master
//! a mix and export it made 7 to 10 sensible calls, hit the cap on the
//! export, and the user was shown an error and no export. The model was
//! not told the number either, so it could not plan within it.
//!
//! Now the model is told the cap in its system prompt, and a step whose
//! calls would go past it is not run: each call is answered "not run",
//! the model gets one last request with tools off to say what was done
//! and what is left, and the turn ends normally.
//!
//! These drive the public `Agent` API against a mocked provider. `edit`
//! mutates and counts what it ran with; `peek` reads only, so Plan
//! first never holds it.

mod common;

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use ai::anthropic::{ContentBlock, Message, Role};
use ai::{Agent, AgentEvent, LlmConfig, MAX_TOOL_CALLS_PER_TURN as CAP};
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

/// The events of a turn, reduced to what these tests compare.
#[derive(Debug, Clone, PartialEq)]
enum Ev {
    Text(String),
    ToolStart(String),
    ToolEnd(String, bool),
    /// Announced and never dispatched: refused for the budget, or called
    /// during the tools-off summary. Not a failure; nothing ran.
    NotRun(String),
    Done,
    /// The approval card, with how many edits had run when it was shown.
    Card {
        edits_run: usize,
    },
    PlanRejected,
    Unavailable {
        held: bool,
    },
}

struct Fixture {
    server: MockServer,
    _dir: tempfile::TempDir,
    agent: Agent,
    notify: Arc<Notify>,
    edits: Arc<Mutex<Vec<Value>>>,
    peeks: Arc<AtomicUsize>,
}

impl Fixture {
    async fn new(plan_first: bool, responses: Vec<Entry>) -> Self {
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
        let cfg = LlmConfig::new_anthropic("test-key").with_base_url(server.uri());
        let mut agent = Agent::new(
            cfg,
            Arc::new(Mutex::new(dispatcher)),
            Arc::new(Mutex::new(
                session::Store::open(dir.path()).expect("open store"),
            )),
            Arc::new(Mutex::new(audio_engine::Engine::new())),
            Arc::clone(&notify),
            Arc::new(Mutex::new(None)),
            Arc::new(AtomicBool::new(false)),
            Arc::new(Mutex::new(None::<tools::Clipboard>)),
        );
        agent.set_plan_first(plan_first);

        Self {
            server,
            _dir: dir,
            agent,
            notify,
            edits,
            peeks,
        }
    }

    /// Run one turn, approving every card the way the UI's Run does.
    async fn turn(&mut self, message: &str) -> (ai::Result<ai::TurnResult>, Vec<Ev>) {
        let notify = Arc::clone(&self.notify);
        let edits = Arc::clone(&self.edits);
        let mut events = Vec::new();
        let result = self
            .agent
            .turn(message.to_string(), |event| match event {
                AgentEvent::TextDelta(t) => events.push(Ev::Text(t)),
                AgentEvent::ToolCallStart { id, .. } => events.push(Ev::ToolStart(id)),
                AgentEvent::ToolCallEnd { id, ok, .. } => events.push(Ev::ToolEnd(id, ok)),
                AgentEvent::ToolCallNotRun { id } => events.push(Ev::NotRun(id)),
                AgentEvent::Done => events.push(Ev::Done),
                AgentEvent::PlanRejected => events.push(Ev::PlanRejected),
                AgentEvent::PlanUnavailable {
                    first_edit_held, ..
                } => events.push(Ev::Unavailable {
                    held: first_edit_held,
                }),
                AgentEvent::Plan { .. } => {
                    events.push(Ev::Card {
                        edits_run: edits.lock().unwrap().len(),
                    });
                    notify.notify_one();
                }
                AgentEvent::NodeCreated(_) => {}
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

    fn peeks_run(&self) -> usize {
        self.peeks.load(Ordering::SeqCst)
    }
}

/// A step of `edit` calls on tracks `0..n`, ids `<prefix><i>`.
fn edit_step(prefix: &str, n: usize) -> String {
    step_of("edit", prefix, n)
}

fn step_of(tool: &str, prefix: &str, n: usize) -> String {
    let ids: Vec<String> = (0..n).map(|i| format!("{prefix}{i}")).collect();
    let args: Vec<String> = (0..n).map(|i| format!(r#"{{"track":{i}}}"#)).collect();
    let calls: Vec<(&str, &str, &str)> = ids
        .iter()
        .zip(&args)
        .map(|(id, a)| (id.as_str(), tool, a.as_str()))
        .collect();
    sse_tool_step(&calls)
}

/// Classifier, then the model's steps. No plan request: Plan first is off.
fn steps(steps: Vec<String>) -> Vec<Entry> {
    let mut responses = vec![ok(classifier_json("general"))];
    responses.extend(steps.into_iter().map(ok));
    responses
}

/// Classifier, a plan reply with no plan in it, then the model's steps.
fn steps_without_a_plan(steps: Vec<String>) -> Vec<Entry> {
    let mut responses = vec![ok(classifier_json("general")), ok(plan_json("Sure."))];
    responses.extend(steps.into_iter().map(ok));
    responses
}

/// `CAP` steps of one `edit` call each: exactly the whole budget.
fn the_whole_budget_one_edit_at_a_time() -> Vec<String> {
    (0..CAP).map(|i| edit_step(&format!("e{i}_"), 1)).collect()
}

fn text_of(events: &[Ev]) -> String {
    events
        .iter()
        .filter_map(|e| match e {
            Ev::Text(t) => Some(t.as_str()),
            _ => None,
        })
        .collect()
}

fn cards(events: &[Ev]) -> usize {
    events
        .iter()
        .filter(|e| matches!(e, Ev::Card { .. }))
        .count()
}

/// Every call that was announced was ended exactly once, and the turn
/// said it was done as its last word.
fn assert_every_badge_resolves(events: &[Ev]) {
    let started: Vec<&String> = events
        .iter()
        .filter_map(|e| match e {
            Ev::ToolStart(id) => Some(id),
            _ => None,
        })
        .collect();
    for id in started {
        let ends = events
            .iter()
            .filter(|e| matches!(e, Ev::ToolEnd(i, _) | Ev::NotRun(i) if i == id))
            .count();
        assert_eq!(ends, 1, "{id} was ended {ends} times in {events:#?}");
    }
}

/// The wire message list obeys the provider's rules: roles alternate, and
/// every `tool_use` is answered by a `tool_result` in the next message
/// (and the other way round). A violation is a 400 on the next request.
fn assert_history_is_valid(history: &[Message]) {
    for pair in history.windows(2) {
        assert_ne!(pair[0].role, pair[1].role, "roles must alternate");
    }
    for (i, m) in history.iter().enumerate() {
        let used: Vec<&str> = m
            .content
            .iter()
            .filter_map(|b| match b {
                ContentBlock::ToolUse { id, .. } => Some(id.as_str()),
                _ => None,
            })
            .collect();
        let answered: Vec<&str> = history
            .get(i + 1)
            .map(|next| {
                next.content
                    .iter()
                    .filter_map(|b| match b {
                        ContentBlock::ToolResult { tool_use_id, .. } => Some(tool_use_id.as_str()),
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default();
        assert_eq!(
            used, answered,
            "message {i} used {used:?} and the next answered {answered:?}"
        );
    }
}

/// The text of a request's system prompt.
fn system_text(request: &Value) -> String {
    request["system"]
        .as_array()
        .expect("system blocks")
        .iter()
        .filter_map(|b| b["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

// ---------------------------------------------------------------------
// The model is told the budget
// ---------------------------------------------------------------------

#[tokio::test]
async fn the_system_prompt_states_the_cap() {
    let mut fx = Fixture::new(false, steps(vec![sse_text("Hi.")])).await;
    let (result, _) = fx.turn("hello").await;
    result.expect("turn");

    let requests = fx.requests().await;
    let stated = format!("at most {CAP} tool calls");
    // The streaming request is the model's own; the classifier before it
    // is a different prompt.
    let model_request = &requests[1];
    assert_eq!(model_request["stream"], true);
    let system = system_text(model_request);
    assert!(
        system.contains(&stated),
        "the system prompt never states the cap ({stated:?}): {system}"
    );
    // After the mode's own text, as one more line of the base prompt.
    assert!(
        system.find("You are edytlab").unwrap() < system.find(&stated).unwrap(),
        "{system}"
    );
}

// ---------------------------------------------------------------------
// Reaching the budget ends the turn with a summary
// ---------------------------------------------------------------------

#[tokio::test]
async fn a_turn_that_keeps_calling_tools_past_the_cap_ends_with_a_summary() {
    let summary = "Applied 20 edits. Still to do: the export.";
    let mut steps_to_run = the_whole_budget_one_edit_at_a_time();
    // The 21st call, in a step of two: past the cap, and the model keeps
    // asking.
    steps_to_run.push(edit_step("over", 2));
    steps_to_run.push(sse_text(summary));
    let mut fx = Fixture::new(false, steps(steps_to_run)).await;

    let (result, events) = fx.turn("master the mix, then export it").await;

    // Ends normally, with the summary as the turn's text.
    let result = result.expect("reaching the budget is not an error");
    assert!(result.text.contains(summary), "{:?}", result.text);
    assert!(text_of(&events).contains(summary), "{events:#?}");
    assert_eq!(events.last(), Some(&Ev::Done), "{events:#?}");
    assert_eq!(
        events.iter().filter(|e| **e == Ev::Done).count(),
        1,
        "{events:#?}"
    );

    // Dispatched exactly the budget, and not the step that crossed it.
    let ran = fx.edits_run();
    assert_eq!(ran.len(), CAP, "{ran:?}");
    assert!(
        !ran.contains(&json!({"track": 1})),
        "a call of the refused step ran: {ran:?}"
    );

    // The refused step's badges were started as they streamed and are
    // resolved as not run, which is not the same as failed.
    for id in ["over0", "over1"] {
        assert!(events.contains(&Ev::ToolStart(id.into())), "{events:#?}");
        assert!(
            events.contains(&Ev::NotRun(id.into())),
            "{id} was left running: {events:#?}"
        );
    }
    assert_every_badge_resolves(&events);

    // classifier + the budget's worth of steps + the refused step + the
    // summary; nothing beyond.
    let requests = fx.requests().await;
    assert_eq!(
        requests.len(),
        1 + CAP + 1 + 1,
        "{} requests",
        requests.len()
    );

    // Every step offered tools; the last one turned them off, and kept
    // the definitions, which the tool calls in the history require.
    for step in &requests[1..requests.len() - 1] {
        assert_eq!(step["tool_choice"], json!({"type": "auto"}));
    }
    let last = requests.last().unwrap();
    assert_eq!(last["tool_choice"], json!({"type": "none"}), "{last}");
    assert!(
        last["tools"].as_array().is_some_and(|t| !t.is_empty()),
        "Anthropic rejects tool calls in the history with no tools defined"
    );
    assert_eq!(last["stream"], true, "the summary streams as normal");

    // The refused step's calls were each answered, as errors that say why.
    let messages = last["messages"].as_array().unwrap();
    let answers = messages.last().unwrap();
    assert_eq!(answers["role"], "user");
    let results: Vec<&Value> = answers["content"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|b| b["type"] == "tool_result")
        .collect();
    assert_eq!(results.len(), 2, "{answers}");
    for (result, id) in results.iter().zip(["over0", "over1"]) {
        assert_eq!(result["tool_use_id"], id);
        assert_eq!(result["is_error"], true);
        let said = result["content"].as_str().unwrap();
        assert!(said.starts_with("Not run"), "{said}");
        assert!(said.contains(&CAP.to_string()), "{said}");
    }

    // And the history that is kept is valid for the next request.
    let history = fx.agent.conversation();
    assert_history_is_valid(history);
    assert_eq!(history.last().unwrap().role, Role::Assistant);
}

/// A step is refused whole. One that would cross the cap by a single
/// call runs none of its calls, rather than the ones that fit.
#[tokio::test]
async fn a_step_that_would_cross_the_cap_runs_none_of_its_calls() {
    let mut fx = Fixture::new(
        false,
        steps(vec![
            edit_step("fits", CAP - 1),
            edit_step("over", 2),
            sse_text("Nineteen done, one step left."),
        ]),
    )
    .await;

    let (result, events) = fx.turn("do a lot").await;
    result.expect("turn");

    assert_eq!(
        fx.edits_run().len(),
        CAP - 1,
        "the refused step ran part of itself"
    );
    assert!(events.contains(&Ev::NotRun("over0".into())));
    assert!(events.contains(&Ev::NotRun("over1".into())));
    assert_every_badge_resolves(&events);
    assert_eq!(events.last(), Some(&Ev::Done));
}

/// A request that fits is untouched: the whole budget is usable, and the
/// turn is the one it always was.
#[tokio::test]
async fn a_turn_within_the_cap_runs_everything_and_is_never_asked_to_stop() {
    let mut steps_to_run = vec![edit_step("a", CAP - 1), edit_step("b", 1)];
    steps_to_run.push(sse_text("All done."));
    let mut fx = Fixture::new(false, steps(steps_to_run)).await;

    let (result, events) = fx.turn("do exactly the budget").await;
    result.expect("turn");

    assert_eq!(fx.edits_run().len(), CAP);
    assert_eq!(events.last(), Some(&Ev::Done));
    for request in &fx.requests().await[1..] {
        assert_eq!(request["tool_choice"], json!({"type": "auto"}));
    }
}

// ---------------------------------------------------------------------
// The last request
// ---------------------------------------------------------------------

/// A provider that does not honour `tool_choice: none` and calls a tool
/// anyway. The call is not run, its badge resolves, and the history
/// stays valid.
#[tokio::test]
async fn a_tool_call_in_the_summary_request_is_not_run() {
    let mut steps_to_run = the_whole_budget_one_edit_at_a_time();
    steps_to_run.push(edit_step("over", 1));
    // Says something, then calls a tool anyway.
    steps_to_run.push(edit_step("late", 1));
    let mut fx = Fixture::new(false, steps(steps_to_run)).await;

    let (result, events) = fx.turn("keep going").await;
    result.expect("turn");

    assert_eq!(fx.edits_run().len(), CAP, "the late call ran");
    assert!(events.contains(&Ev::ToolStart("late0".into())));
    assert!(events.contains(&Ev::NotRun("late0".into())));
    assert_every_badge_resolves(&events);
    assert_eq!(events.last(), Some(&Ev::Done));
    assert_history_is_valid(fx.agent.conversation());
    assert_eq!(
        fx.requests().await.len(),
        1 + CAP + 1 + 1,
        "no further request"
    );
}

/// A summary with no words would leave the user with nothing; they are
/// told what happened.
#[tokio::test]
async fn a_summary_with_no_words_is_replaced_by_a_notice() {
    let mut steps_to_run = the_whole_budget_one_edit_at_a_time();
    steps_to_run.push(edit_step("over", 1));
    steps_to_run.push(sse_text(""));
    let mut fx = Fixture::new(false, steps(steps_to_run)).await;

    let (result, events) = fx.turn("keep going").await;
    let result = result.expect("turn");

    let shown = text_of(&events);
    assert!(shown.contains(&CAP.to_string()), "{shown:?}");
    assert!(shown.contains("limit"), "{shown:?}");
    assert_eq!(
        result.text, shown,
        "the turn's text is not what was shown to the user"
    );
    assert_eq!(events.last(), Some(&Ev::Done));
    assert_history_is_valid(fx.agent.conversation());
}

/// If the summary request itself fails, the turn ends with that error
/// like any other failed request, and the history is still sound.
#[tokio::test]
async fn a_failed_summary_request_is_the_turns_error() {
    let mut responses = steps(the_whole_budget_one_edit_at_a_time());
    responses.push(ok(edit_step("over", 1)));
    responses.push((503, "overloaded".to_string()));
    let mut fx = Fixture::new(false, responses).await;

    let (result, events) = fx.turn("keep going").await;

    match result {
        Err(ai::Error::Api { status: 503, .. }) => {}
        other => panic!("expected the provider's error, got {other:?}"),
    }
    assert!(!events.contains(&Ev::Done), "{events:#?}");
    assert_every_badge_resolves(&events);
    assert_eq!(fx.edits_run().len(), CAP);

    // The conversation ends in the answers to the refused step, which the
    // next message joins; it holds no call without its answer.
    let history = fx.agent.conversation();
    assert_eq!(history.last().unwrap().role, Role::User);
    assert_history_is_valid(history);
}

// ---------------------------------------------------------------------
// Plan first
// ---------------------------------------------------------------------

/// The budget is checked before the held-step gate, so a step that will
/// not run is never put to the user for approval: no card, and nothing in
/// it runs.
#[tokio::test]
async fn with_plan_first_a_step_over_the_cap_is_never_shown_for_approval() {
    let mut fx = Fixture::new(
        true,
        steps_without_a_plan(vec![
            // Reads only, so the gate lets it through: the whole budget.
            step_of("peek", "look", CAP),
            // The first edit, one call past the cap.
            edit_step("over", 1),
            sse_text("Looked at everything; no edits made. Next: the edit."),
        ]),
    )
    .await;

    let (result, events) = fx.turn("tidy it up").await;

    let result = result.expect("the budget is not an error");
    assert!(result.text.contains("no edits made"), "{:?}", result.text);
    assert_eq!(
        cards(&events),
        0,
        "a step that cannot run was shown: {events:#?}"
    );
    assert!(
        events.contains(&Ev::Unavailable { held: true }),
        "the first edit was meant to be held: {events:#?}"
    );
    assert_eq!(fx.peeks_run(), CAP);
    assert!(
        fx.edits_run().is_empty(),
        "an edit ran without approval: {:?}",
        fx.edits_run()
    );
    assert!(events.contains(&Ev::NotRun("over0".into())));
    assert!(!events.contains(&Ev::PlanRejected), "nobody declined");
    assert_every_badge_resolves(&events);
    assert_eq!(events.last(), Some(&Ev::Done));
    assert_history_is_valid(fx.agent.conversation());
}

/// The control for the test above: the same turn one call shorter fits,
/// so the edit is held and shown. This is what makes "never shown" a
/// result of the budget and not of the fixture.
#[tokio::test]
async fn with_plan_first_the_same_edit_within_the_cap_is_still_held() {
    let mut fx = Fixture::new(
        true,
        steps_without_a_plan(vec![
            step_of("peek", "look", CAP - 1),
            edit_step("last", 1),
            sse_text("Done."),
        ]),
    )
    .await;

    let (result, events) = fx.turn("tidy it up").await;
    result.expect("turn");

    assert_eq!(
        cards(&events),
        1,
        "the first edit is held for approval: {events:#?}"
    );
    assert!(
        events.contains(&Ev::Card { edits_run: 0 }),
        "the card is shown before the edit runs: {events:#?}"
    );
    assert_eq!(
        fx.edits_run(),
        vec![json!({"track": 0})],
        "approved, so it ran"
    );
    assert_eq!(fx.peeks_run(), CAP - 1);
}
