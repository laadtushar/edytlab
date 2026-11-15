//! A turn's policy against a scripted model (ADR 0007).
//!
//! `ScriptedExchange` answers each step from a list, so these tests state
//! what the model said as plain steps: no SSE bytes, no wire format, no mock
//! server. The classifier request, which is not behind the port yet, goes to a
//! closed port and falls back to `general`, as it does for a provider that is
//! down. The wire-format tests (`request_size.rs`, `no_orphan_tool_use.rs`,
//! `prior_turn_thinking.rs`, ...) stay as they are and prove the HTTP adapter
//! sends the same bytes.

mod common;

use std::sync::{Arc, Mutex};

use ai::exchange::{Refusal, Scripted, ScriptedExchange, Step, StepBlock};
use ai::provider::Adaptation;
use ai::{Agent, AgentEvent, LlmConfig, MAX_TOOL_CALLS_PER_TURN as CAP};
use serde_json::{json, Value};

struct Fixture {
    _dir: tempfile::TempDir,
    agent: Agent,
    model: Arc<ScriptedExchange>,
    edits: Arc<Mutex<Vec<Value>>>,
}

impl Fixture {
    fn new(model: ScriptedExchange) -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let (dispatcher, edits, _peeks) = common::edit_and_peek();
        // Nothing listens here: the classifier fails and the turn is general.
        let cfg = LlmConfig::new_anthropic("test-key").with_base_url("http://127.0.0.1:1");
        let model = Arc::new(model);
        let agent = common::agent_over(
            cfg,
            dispatcher,
            common::store_in(dir.path()),
            ai::PlanGate::new(),
        )
        .with_exchange(model.clone());
        Self {
            _dir: dir,
            agent,
            model,
            edits,
        }
    }

    async fn turn(&mut self, message: &str) -> (ai::Result<ai::TurnResult>, Vec<String>) {
        let mut events = Vec::new();
        let result = self
            .agent
            .turn(message.to_string(), |event| match event {
                AgentEvent::TextDelta(t) => events.push(format!("text:{t}")),
                AgentEvent::ToolCallStart { id, .. } => events.push(format!("start:{id}")),
                AgentEvent::ToolCallEnd { id, .. } => events.push(format!("end:{id}")),
                AgentEvent::ToolCallNotRun { id } => events.push(format!("not_run:{id}")),
                AgentEvent::Done => events.push("done".into()),
                _ => {}
            })
            .await;
        (result, events)
    }
}

fn edit(id: &str, track: u32) -> Step {
    Step::call(id, "edit", &json!({ "track": track }))
}

#[tokio::test]
async fn a_text_step_is_the_turns_answer() {
    let mut fx = Fixture::new(ScriptedExchange::of_steps([Step::say("Hello.")]));
    let (result, events) = fx.turn("hi").await;
    assert_eq!(result.expect("turn").text, "Hello.");
    assert_eq!(events, ["text:Hello.", "done"]);
    assert_eq!(fx.model.requests().len(), 1);
    common::assert_history_is_valid(fx.agent.conversation(), true);
}

#[tokio::test]
async fn a_tool_step_runs_the_tool_and_the_next_request_carries_its_result() {
    let mut fx = Fixture::new(ScriptedExchange::of_steps([
        edit("e0", 3),
        Step::say("Done."),
    ]));
    let (result, events) = fx.turn("edit track 3").await;
    result.expect("turn");
    assert_eq!(*fx.edits.lock().unwrap(), [json!({ "track": 3 })]);
    assert_eq!(events, ["start:e0", "end:e0", "text:Done.", "done"]);

    let requests = fx.model.requests();
    assert_eq!(requests.len(), 2);
    let last = requests[1]["messages"].as_array().unwrap().last().unwrap();
    assert_eq!(last["content"][0]["type"], "tool_result");
    assert_eq!(last["content"][0]["tool_use_id"], "e0");
}

#[tokio::test]
async fn a_turn_past_the_tool_budget_ends_with_a_summary_request_with_tools_off() {
    let mut script: Vec<Step> = (0..CAP).map(|i| edit(&format!("e{i}"), 1)).collect();
    script.push(edit("over", 2));
    script.push(Step::say("Applied the edits."));
    let mut fx = Fixture::new(ScriptedExchange::of_steps(script));

    let (result, events) = fx.turn("keep going").await;

    assert!(result.expect("turn").text.contains("Applied the edits."));
    assert_eq!(fx.edits.lock().unwrap().len(), CAP, "the refused step ran");
    assert!(events.contains(&"not_run:over".to_string()), "{events:?}");
    let requests = fx.model.requests();
    assert_eq!(requests.len(), CAP + 2);
    assert_eq!(
        requests.last().unwrap()["tool_choice"],
        json!({"type": "none"})
    );
    assert_eq!(requests[0]["tool_choice"], json!({"type": "auto"}));
    assert_eq!(fx.model.remaining(), 0);
    common::assert_history_is_valid(fx.agent.conversation(), false);
}

#[tokio::test]
async fn a_tool_call_in_the_summary_step_is_not_run_and_not_kept() {
    let mut script: Vec<Step> = (0..CAP).map(|i| edit(&format!("e{i}"), 1)).collect();
    script.push(edit("over", 1));
    script.push(edit("late", 1));
    let mut fx = Fixture::new(ScriptedExchange::of_steps(script));

    let (result, events) = fx.turn("keep going").await;

    result.expect("turn");
    assert_eq!(fx.edits.lock().unwrap().len(), CAP);
    assert!(events.contains(&"not_run:late".to_string()), "{events:?}");
    assert_eq!(fx.model.requests().len(), CAP + 2, "no further request");
    common::assert_history_is_valid(fx.agent.conversation(), false);
}

#[tokio::test]
async fn thinking_goes_back_only_when_the_model_replays_it_and_signed_it() {
    let thinking_step = |signature: &str| Step {
        blocks: vec![
            StepBlock::Thinking {
                thinking: "hmm".into(),
                signature: signature.into(),
            },
            StepBlock::ToolUse {
                id: "t0".into(),
                name: "edit".into(),
                args_json: json!({ "track": 1 }).to_string(),
            },
        ],
        stop_reason: Some("tool_use".into()),
        text: String::new(),
    };
    let types_of = |request: &Value| -> Vec<String> {
        let assistant = request["messages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["role"] == "assistant")
            .expect("an assistant message");
        assistant["content"]
            .as_array()
            .unwrap()
            .iter()
            .map(|b| b["type"].as_str().unwrap().to_string())
            .collect()
    };

    // A model whose thinking is replayed keeps the signed block, first.
    let mut fx = Fixture::new(
        ScriptedExchange::of_steps([thinking_step("SIG"), Step::say("ok")]).replaying_thinking(),
    );
    fx.turn("go").await.0.expect("turn");
    assert_eq!(types_of(&fx.model.requests()[1]), ["thinking", "tool_use"]);

    // An unsigned block cannot be verified by the server, so it is dropped.
    let mut fx = Fixture::new(
        ScriptedExchange::of_steps([thinking_step(""), Step::say("ok")]).replaying_thinking(),
    );
    fx.turn("go").await.0.expect("turn");
    assert_eq!(types_of(&fx.model.requests()[1]), ["tool_use"]);

    // A provider that does not replay thinking never has it in the history.
    let mut fx = Fixture::new(ScriptedExchange::of_steps([
        thinking_step("SIG"),
        Step::say("ok"),
    ]));
    fx.turn("go").await.0.expect("turn");
    assert_eq!(types_of(&fx.model.requests()[1]), ["tool_use"]);
}

#[tokio::test]
async fn an_interrupted_step_is_tried_once_more_and_then_ends_the_turn() {
    let interrupted = || Scripted::Interrupted(ai::Error::ApiStream("overloaded".into()));
    // One interruption, then an answer: the same request goes out twice.
    let mut fx = Fixture::new(ScriptedExchange::new([
        interrupted(),
        Scripted::Step(Step::say("Back.")),
    ]));
    let (result, _) = fx.turn("hi").await;
    assert_eq!(result.expect("turn").text, "Back.");
    let requests = fx.model.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0], requests[1]);

    // Two in a row: the second one is the turn's error.
    let mut fx = Fixture::new(ScriptedExchange::new([interrupted(), interrupted()]));
    let (result, _) = fx.turn("hi").await;
    assert!(matches!(result, Err(ai::Error::ApiStream(_))), "{result:?}");
    assert_eq!(fx.model.requests().len(), 2);
}

#[tokio::test]
async fn a_refusal_ends_the_turn_with_its_status_and_leaves_a_sendable_history() {
    let mut fx = Fixture::new(ScriptedExchange::new([Scripted::Refused(Refusal {
        status: 500,
        body: "boom".into(),
        request_id: None,
    })]));
    let (result, _) = fx.turn("hi").await;
    assert!(
        matches!(result, Err(ai::Error::Api { status: 500, .. })),
        "{result:?}"
    );
    // The failed turn took its message back.
    assert!(fx.agent.conversation().is_empty());
}

#[tokio::test]
async fn an_unanswered_call_is_answered_before_the_next_message() {
    // A call whose arguments never parse, twice in a row: the turn gives up,
    // and no call is left without its result.
    let bad = |id: &str| Step {
        blocks: vec![StepBlock::ToolUse {
            id: id.into(),
            name: "edit".into(),
            args_json: "{not json".into(),
        }],
        stop_reason: Some("tool_use".into()),
        text: String::new(),
    };
    let mut fx = Fixture::new(ScriptedExchange::of_steps([
        bad("b0"),
        bad("b1"),
        Step::say("Next."),
    ]));
    let (first, _) = fx.turn("first").await;
    assert!(first.is_err(), "two malformed calls in a row end the turn");
    common::assert_history_is_valid(fx.agent.conversation(), false);

    let (second, _) = fx.turn("second").await;
    assert_eq!(second.expect("the next message works").text, "Next.");
}

#[tokio::test]
async fn a_failed_exchange_is_the_turns_error() {
    let mut fx = Fixture::new(ScriptedExchange::new([Scripted::Fail(
        ai::Error::Protocol("the line dropped".into()),
    )]));
    let (result, events) = fx.turn("hi").await;
    assert!(
        matches!(&result, Err(ai::Error::Protocol(m)) if m == "the line dropped"),
        "{result:?}"
    );
    assert!(events.is_empty(), "{events:?}");
    assert!(fx.agent.conversation().is_empty());
}

#[tokio::test]
async fn a_script_that_runs_out_is_a_protocol_error() {
    let mut fx = Fixture::new(ScriptedExchange::new([edit("e0", 1).into()]));
    let (result, _) = fx.turn("edit").await;
    assert!(
        matches!(&result, Err(ai::Error::Protocol(m)) if m.contains("ran out of steps")),
        "{result:?}"
    );
    assert_eq!(fx.model.requests().len(), 2);
}

fn refused() -> Scripted {
    Scripted::Refused(Refusal {
        status: 400,
        body: "the tools are refused".into(),
        request_id: Some("req_1".into()),
    })
}

fn has_thinking(request: &Value) -> bool {
    request["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|m| m["content"].as_array())
        .flatten()
        .any(|b| b["type"] == "thinking")
}

#[tokio::test]
async fn a_refusal_that_adapts_sends_the_changed_request_and_the_turn_goes_on() {
    // The step that follows a thinking step is refused. Adapting drops the
    // thinking (the rule in `tauri-conventions.md`), so the request built
    // again differs and is sent.
    let thinking_call = Step {
        blocks: vec![
            StepBlock::Thinking {
                thinking: "hmm".into(),
                signature: "SIG".into(),
            },
            StepBlock::ToolUse {
                id: "t0".into(),
                name: "edit".into(),
                args_json: json!({ "track": 1 }).to_string(),
            },
        ],
        stop_reason: Some("tool_use".into()),
        text: String::new(),
    };
    let mut fx = Fixture::new(
        ScriptedExchange::new([thinking_call.into(), refused(), Step::say("Done.").into()])
            .replaying_thinking()
            .adapting([Adaptation::ToolLayout]),
    );

    let (result, _) = fx.turn("go").await;

    assert_eq!(result.expect("turn").text, "Done.");
    let requests = fx.model.requests();
    assert_eq!(requests.len(), 3);
    assert!(has_thinking(&requests[1]), "the refused request");
    assert!(!has_thinking(&requests[2]), "the adapted request");
    assert_eq!(requests[1]["tools"], requests[2]["tools"]);
    common::assert_history_is_valid(fx.agent.conversation(), false);
}

#[tokio::test]
async fn an_adaptation_that_leaves_the_request_unchanged_is_not_sent_again() {
    // Nothing the adaptations change shows in this request, so each one
    // builds the body that was refused: it is asked for the next, and when
    // it has none the refusal is the turn's error.
    let mut fx = Fixture::new(
        ScriptedExchange::new([refused()])
            .adapting([Adaptation::ToolLayout, Adaptation::Reasoning]),
    );
    let (result, _) = fx.turn("hi").await;
    assert!(
        matches!(result, Err(ai::Error::Api { status: 400, .. })),
        "{result:?}"
    );
    assert_eq!(
        fx.model.requests().len(),
        1,
        "the refused body went out again"
    );
}

#[tokio::test]
async fn a_model_that_keeps_adapting_to_the_same_request_is_stopped() {
    let mut fx =
        Fixture::new(ScriptedExchange::new([refused()]).adapting([Adaptation::ToolLayout; 12]));
    let (result, _) = fx.turn("hi").await;
    assert!(
        matches!(result, Err(ai::Error::Api { status: 400, .. })),
        "{result:?}"
    );
    assert_eq!(fx.model.requests().len(), 1);
}
