//! A turn that ends in an error never leaves a `tool_use` unanswered (#486).
//!
//! When the model sends malformed tool arguments twice in a row, or a
//! tool's input fails validation twice, `run_turn` gives up with
//! `Error::ToolValidation`. By then the assistant message holding the
//! `tool_use` was already stored, and no `tool_result` had been pushed for
//! it. The next user message was appended as a separate message after it,
//! and every provider rejects that history:
//!
//! * Anthropic: "`tool_use` ids were found without `tool_result` blocks
//!   immediately after".
//! * OpenAI-compatible: "An assistant message with 'tool_calls' must be
//!   followed by tool messages".
//!
//! The chat was then stuck: every later message failed the same way, until
//! the user started a new one.
//!
//! These drive whole turns through the public `Agent` API against a mocked
//! provider on both wire formats. The mock is strict: like the real APIs,
//! it answers `400` to a request whose history has an unanswered call, so
//! "the next message works" is what is asserted, not an inference from the
//! shape of the history.
//!
//! The failing turn is cut short at every point the loop can give up:
//!
//! * the second malformed call in a row (`two_malformed_calls`),
//! * the second call in a row that fails validation (`two_invalid_calls`),
//! * either one in the middle of a step of several calls, where the calls
//!   before it have run and the calls after it never will
//!   (`a_step_cut_short_*`), and
//! * the same on a model that thinks by default, where the history is
//!   append-only within a turn and loses its thinking at the next one
//!   (`thinking_models_*`, #484).

mod common;

use std::collections::BTreeSet;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use ai::anthropic::{ContentBlock, Message, Role};
use ai::{Agent, AgentEvent, LlmConfig};
use common::{
    chat_reply_json, chat_sse_text, chat_sse_tool_step, classifier_json, ok, sse_text,
    sse_thinking_tool_step, sse_tool_step, Entry, SeqResponder,
};
use serde_json::{json, Value};
use tokio::sync::Notify;
use tools::schema::{anthropic_tool, object_schema};
use tools::{Tool, ToolContext, ToolDispatcher, ToolResult};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Respond, ResponseTemplate};

// ---------------------------------------------------------------------
// The two wire formats
// ---------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
enum Wire {
    /// Anthropic's Messages API: `tool_use` and `tool_result` blocks.
    Anthropic,
    /// OpenAI chat completions: `tool_calls` and `role: "tool"` messages.
    OpenAi,
}

impl Wire {
    fn config(self, base_url: String) -> LlmConfig {
        match self {
            Wire::Anthropic => LlmConfig::new_anthropic("test-key"),
            Wire::OpenAi => LlmConfig::new_openai("test-key"),
        }
        .with_base_url(base_url)
    }

    fn endpoint(self) -> &'static str {
        match self {
            Wire::Anthropic => "/v1/messages",
            Wire::OpenAi => "/v1/chat/completions",
        }
    }

    /// The classifier's answer, which opens every turn.
    fn classifier(self) -> Entry {
        ok(match self {
            Wire::Anthropic => classifier_json("general"),
            Wire::OpenAi => chat_reply_json("general"),
        })
    }

    /// A model step that only speaks, and ends the turn.
    fn text(self, text: &str) -> Entry {
        ok(match self {
            Wire::Anthropic => sse_text(text),
            Wire::OpenAi => chat_sse_text(text),
        })
    }

    /// A model step that makes the given `(id, name, arguments)` calls.
    fn calls(self, calls: &[(&str, &str, &str)]) -> Entry {
        ok(match self {
            Wire::Anthropic => sse_tool_step(calls),
            Wire::OpenAi => chat_sse_tool_step(calls),
        })
    }

    /// What the provider says when it refuses a history with an
    /// unanswered call, in its own words and envelope.
    fn rejection(self, unanswered: &[String]) -> Value {
        match self {
            Wire::Anthropic => json!({
                "type": "error",
                "error": {
                    "type": "invalid_request_error",
                    "message": format!(
                        "messages: `tool_use` ids were found without `tool_result` blocks \
                         immediately after: {}. Each `tool_use` block must have a \
                         corresponding `tool_result` block in the next message.",
                        unanswered.join(", ")
                    )
                }
            }),
            Wire::OpenAi => json!({
                "error": {
                    "type": "invalid_request_error",
                    "message": format!(
                        "An assistant message with 'tool_calls' must be followed by tool \
                         messages responding to each 'tool_call_id'. The following \
                         tool_call_ids did not have response messages: {}",
                        unanswered.join(", ")
                    )
                }
            }),
        }
    }
}

/// The ids of `message`'s content blocks of `kind`, read from `key`.
fn block_ids(message: &Value, kind: &str, key: &str) -> Vec<String> {
    message["content"]
        .as_array()
        .map(|blocks| {
            blocks
                .iter()
                .filter(|b| b["type"] == kind)
                .filter_map(|b| b[key].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// What `request` gets wrong about tool calls and their answers, by the
/// rules of `wire`'s API. Empty when the history is one it would accept.
fn unanswered_calls(wire: Wire, request: &Value) -> Vec<String> {
    let messages = request["messages"].as_array().cloned().unwrap_or_default();
    let mut found = Vec::new();
    match wire {
        Wire::Anthropic => {
            for (i, message) in messages.iter().enumerate() {
                let used = block_ids(message, "tool_use", "id");
                if message["role"] == "assistant" && !used.is_empty() {
                    // Every call is answered in the very next message,
                    // which is the user's.
                    let answered = messages
                        .get(i + 1)
                        .filter(|next| next["role"] == "user")
                        .map(|next| block_ids(next, "tool_result", "tool_use_id"))
                        .unwrap_or_default();
                    found.extend(used.into_iter().filter(|id| !answered.contains(id)));
                }
                // And the other way round: an answer needs its call just
                // before it, and comes ahead of any text in its message.
                let answers = block_ids(message, "tool_result", "tool_use_id");
                if message["role"] == "user" && !answers.is_empty() {
                    let called = i
                        .checked_sub(1)
                        .map(|p| block_ids(&messages[p], "tool_use", "id"))
                        .unwrap_or_default();
                    found.extend(
                        answers
                            .into_iter()
                            .filter(|id| !called.contains(id))
                            .map(|id| format!("{id} (a tool_result with no tool_use)")),
                    );
                    let blocks = message["content"].as_array().unwrap();
                    let first_text = blocks.iter().position(|b| b["type"] == "text");
                    let last_result = blocks.iter().rposition(|b| b["type"] == "tool_result");
                    if let (Some(text), Some(result)) = (first_text, last_result) {
                        if text < result {
                            found.push("text ahead of a tool_result".to_string());
                        }
                    }
                }
            }
        }
        Wire::OpenAi => {
            for (i, message) in messages.iter().enumerate() {
                let called: Vec<String> = message["tool_calls"]
                    .as_array()
                    .map(|calls| {
                        calls
                            .iter()
                            .filter_map(|c| c["id"].as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default();
                if message["role"] == "assistant" && !called.is_empty() {
                    // The tool messages that follow it, with nothing in
                    // between, answer it.
                    let answered: Vec<String> = messages[i + 1..]
                        .iter()
                        .take_while(|m| m["role"] == "tool")
                        .filter_map(|m| m["tool_call_id"].as_str().map(str::to_string))
                        .collect();
                    found.extend(called.into_iter().filter(|id| !answered.contains(id)));
                }
                if message["role"] == "tool" {
                    // Walk back over its siblings to the assistant message
                    // that called it.
                    let caller = messages[..i].iter().rev().find(|m| m["role"] != "tool");
                    let id = message["tool_call_id"].as_str().unwrap_or_default();
                    let known = caller
                        .and_then(|c| c["tool_calls"].as_array())
                        .is_some_and(|calls| calls.iter().any(|c| c["id"] == id));
                    if !known {
                        found.push(format!("{id} (a tool message with no tool_call)"));
                    }
                }
            }
        }
    }
    found
}

/// The provider's own refusal, for a request it would not accept.
struct Strict {
    wire: Wire,
    script: SeqResponder,
}

impl Respond for Strict {
    fn respond(&self, req: &wiremock::Request) -> ResponseTemplate {
        let body: Value = serde_json::from_slice(&req.body).unwrap_or(Value::Null);
        let unanswered = unanswered_calls(self.wire, &body);
        if unanswered.is_empty() {
            self.script.respond(req)
        } else {
            ResponseTemplate::new(400).set_body_json(self.wire.rejection(&unanswered))
        }
    }
}

/// How `request` answers the call `id`: its text, and whether it reads as
/// an error, in `wire`'s own terms.
fn answer_to(wire: Wire, request: &Value, id: &str) -> Option<(String, bool)> {
    let messages = request["messages"].as_array()?;
    match wire {
        Wire::Anthropic => messages
            .iter()
            .filter_map(|m| m["content"].as_array())
            .flatten()
            .find(|b| b["type"] == "tool_result" && b["tool_use_id"] == id)
            .map(|b| {
                (
                    b["content"].as_str().unwrap_or_default().to_string(),
                    b["is_error"] == true,
                )
            }),
        Wire::OpenAi => messages
            .iter()
            .find(|m| m["role"] == "tool" && m["tool_call_id"] == id)
            .map(|m| {
                let text = m["content"].as_str().unwrap_or_default();
                // The translation marks an error result with a prefix.
                match text.strip_prefix("error: ") {
                    Some(rest) => (rest.to_string(), true),
                    None => (text.to_string(), false),
                }
            }),
    }
}

/// The text of the last user message in `request`.
fn last_user_text(wire: Wire, request: &Value) -> String {
    let last = request["messages"]
        .as_array()
        .and_then(|m| m.last())
        .expect("a message");
    assert_eq!(last["role"], "user", "{last}");
    match wire {
        Wire::Anthropic => last["content"]
            .as_array()
            .expect("blocks")
            .iter()
            .filter(|b| b["type"] == "text")
            .filter_map(|b| b["text"].as_str())
            .collect(),
        Wire::OpenAi => last["content"].as_str().unwrap_or_default().to_string(),
    }
}

// ---------------------------------------------------------------------
// Fixture
// ---------------------------------------------------------------------

/// Mutates, because it says nothing about it; needs an integer `track`.
/// Records what it ran with.
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

/// The events of a turn, reduced to what these tests compare.
#[derive(Debug, Clone, PartialEq)]
enum Ev {
    ToolStart(String),
    ToolEnd(String, bool),
    NotRun(String),
    Done,
}

struct Fixture {
    wire: Wire,
    server: MockServer,
    _dir: tempfile::TempDir,
    agent: Agent,
    edits: Arc<Mutex<Vec<Value>>>,
}

impl Fixture {
    async fn new(wire: Wire, responses: Vec<Entry>) -> Self {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(wire.endpoint()))
            .respond_with(Strict {
                wire,
                script: SeqResponder::new(responses),
            })
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().expect("tempdir");
        let edits = Arc::new(Mutex::new(Vec::new()));
        let mut dispatcher = ToolDispatcher::new();
        dispatcher.register(Box::new(Edit {
            log: Arc::clone(&edits),
        }));

        let agent = Agent::new(
            wire.config(server.uri()),
            Arc::new(Mutex::new(dispatcher)),
            Arc::new(Mutex::new(
                session::Store::open(dir.path()).expect("open store"),
            )),
            Arc::new(Mutex::new(audio_engine::Engine::new())),
            Arc::new(Notify::new()),
            Arc::new(Mutex::new(None)),
            Arc::new(AtomicBool::new(false)),
            Arc::new(Mutex::new(None::<tools::Clipboard>)),
        );

        Self {
            wire,
            server,
            _dir: dir,
            agent,
            edits,
        }
    }

    async fn turn(&mut self, message: &str) -> (ai::Result<ai::TurnResult>, Vec<Ev>) {
        let mut events = Vec::new();
        let result = self
            .agent
            .turn(message.to_string(), |event| match event {
                AgentEvent::ToolCallStart { id, .. } => events.push(Ev::ToolStart(id)),
                AgentEvent::ToolCallEnd { id, ok, .. } => events.push(Ev::ToolEnd(id, ok)),
                AgentEvent::ToolCallNotRun { id } => events.push(Ev::NotRun(id)),
                AgentEvent::Done => events.push(Ev::Done),
                _ => {}
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

    /// The second message, as it should go: the turn that follows the
    /// failed one succeeds, says what the mock scripted, and the request it
    /// made is one the provider accepts and still carries the user's words.
    ///
    /// `before` is how many requests the failed turn made.
    async fn the_next_message_works(&mut self, before: usize, answer: &str) {
        let (result, events) = self.turn("ok, carry on").await;
        let result = result
            .unwrap_or_else(|e| panic!("the next message was refused, so the chat is stuck: {e}"));
        assert_eq!(result.text, answer);
        assert_eq!(events.last(), Some(&Ev::Done), "{events:#?}");

        let requests = self.requests().await;
        assert_eq!(
            requests.len(),
            before + 2,
            "the next turn is a classifier call and one model step"
        );
        let step = requests.last().unwrap();
        assert_eq!(step["stream"], true, "{step}");
        assert_eq!(unanswered_calls(self.wire, step), Vec::<String>::new());
        assert_eq!(last_user_text(self.wire, step), "ok, carry on");
    }
}

/// Valid JSON that is cut off, as a stream that stops mid-argument is.
const TRUNCATED: &str = r#"{"track":"#;
/// Valid JSON that fails the schema: the required `track` is missing.
const INCOMPLETE: &str = "{}";
/// Arguments that pass.
const FINE: &str = r#"{"track":0}"#;

/// Every call that was announced was ended exactly once, so no badge in
/// the UI is left reading "running".
fn assert_every_badge_resolves(events: &[Ev]) {
    for event in events {
        if let Ev::ToolStart(id) = event {
            let ends = events
                .iter()
                .filter(|e| matches!(e, Ev::ToolEnd(i, _) | Ev::NotRun(i) if i == id))
                .count();
            assert_eq!(ends, 1, "{id} was ended {ends} times in {events:#?}");
        }
    }
}

/// The stored history obeys the providers' rules: roles alternate, and
/// every `tool_use` is answered by the `tool_result`s of the next message,
/// the same calls in the same order. No message is empty.
fn assert_history_is_valid(history: &[Message]) {
    for pair in history.windows(2) {
        assert_ne!(pair[0].role, pair[1].role, "roles must alternate");
    }
    for (i, m) in history.iter().enumerate() {
        assert!(!m.content.is_empty(), "message {i} is empty");
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

/// The `tool_result` blocks of the last stored message, in order.
fn last_results(history: &[Message]) -> Vec<(&str, &str, Option<bool>)> {
    let last = history.last().expect("a message");
    assert_eq!(last.role, Role::User);
    last.content
        .iter()
        .filter_map(|b| match b {
            ContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => Some((tool_use_id.as_str(), content.as_str(), *is_error)),
            _ => None,
        })
        .collect()
}

/// A scenario run once on each wire format.
macro_rules! on_both_wires {
    ($name:ident) => {
        mod $name {
            use super::*;

            #[tokio::test]
            async fn anthropic() {
                super::$name(Wire::Anthropic).await
            }

            #[tokio::test]
            async fn openai_chat_completions() {
                super::$name(Wire::OpenAi).await
            }
        }
    };
}

// ---------------------------------------------------------------------
// The two exits named in the issue
// ---------------------------------------------------------------------

/// The model sends arguments that are not JSON, is told, and does it again.
async fn two_malformed_calls(wire: Wire) {
    let mut fx = Fixture::new(
        wire,
        vec![
            wire.classifier(),
            wire.calls(&[("bad1", "edit", TRUNCATED)]),
            wire.calls(&[("bad2", "edit", TRUNCATED)]),
            wire.classifier(),
            wire.text("Sorry about that. What next?"),
        ],
    )
    .await;

    let (result, events) = fx.turn("tidy the vocals").await;

    // The turn still ends in the error it always did.
    match result {
        Err(ai::Error::ToolValidation(message)) => {
            assert!(message.contains("malformed twice"), "{message}")
        }
        other => panic!("expected the validation error, got {other:?}"),
    }
    assert!(fx.edits_run().is_empty(), "a call with no arguments ran");

    // What the issue is about: the user's next message is accepted.
    let failed = fx.agent.conversation().to_vec();
    fx.the_next_message_works(3, "Sorry about that. What next?")
        .await;

    // The history the failed turn left is one a provider takes: the second
    // call has its answer, as an error that says why, and its badge is
    // resolved.
    assert_history_is_valid(&failed);
    assert_eq!(
        last_results(&failed)
            .iter()
            .map(|(id, _, err)| (*id, *err))
            .collect::<Vec<_>>(),
        vec![("bad2", Some(true))]
    );
    assert_every_badge_resolves(&events);
    assert!(!events.contains(&Ev::Done), "{events:#?}");

    // What the model reads for that second call: an error that names the
    // failure and says the turn was stopped.
    let requests = fx.requests().await;
    let (said, is_error) = answer_to(wire, requests.last().unwrap(), "bad2")
        .expect("the second call is answered in the request that follows");
    assert!(is_error, "{said}");
    assert!(said.contains("not valid JSON"), "{said}");
    assert!(said.contains("stopped"), "{said}");
}
on_both_wires!(two_malformed_calls);

/// The model sends arguments that parse and fail the tool's schema, is
/// told, and does it again.
async fn two_invalid_calls(wire: Wire) {
    let mut fx = Fixture::new(
        wire,
        vec![
            wire.classifier(),
            wire.calls(&[("bad1", "edit", INCOMPLETE)]),
            wire.calls(&[("bad2", "edit", INCOMPLETE)]),
            wire.classifier(),
            wire.text("Sorry about that. What next?"),
        ],
    )
    .await;

    let (result, events) = fx.turn("tidy the vocals").await;

    match result {
        Err(ai::Error::ToolValidation(message)) => {
            assert!(message.contains("failed validation twice"), "{message}")
        }
        other => panic!("expected the validation error, got {other:?}"),
    }
    assert!(fx.edits_run().is_empty(), "an invalid call ran");

    let failed = fx.agent.conversation().to_vec();
    fx.the_next_message_works(3, "Sorry about that. What next?")
        .await;

    assert_history_is_valid(&failed);
    assert_eq!(
        last_results(&failed)
            .iter()
            .map(|(id, _, err)| (*id, *err))
            .collect::<Vec<_>>(),
        vec![("bad2", Some(true))]
    );
    assert_every_badge_resolves(&events);
    assert!(!events.contains(&Ev::Done), "{events:#?}");

    // The answer carries the validator's own words, as it did for the
    // first failure, so the model can see what was wrong.
    let requests = fx.requests().await;
    let (first, _) = answer_to(wire, &requests[2], "bad1").expect("answered");
    let (second, is_error) = answer_to(wire, requests.last().unwrap(), "bad2")
        .expect("the second call is answered in the request that follows");
    assert!(is_error, "{second}");
    assert!(
        second.contains(&first),
        "{second:?} does not carry {first:?}"
    );
    assert!(second.contains("stopped"), "{second}");
}
on_both_wires!(two_invalid_calls);

// ---------------------------------------------------------------------
// Giving up in the middle of a step
// ---------------------------------------------------------------------

/// One step of four calls. The first is fine and runs. The second and third
/// both fail, which is the second failure in a row and ends the turn on the
/// third. The fourth is never reached.
///
/// Every call in the stored assistant message needs an answer, and each
/// gets the true one: the first its real result (it ran, and its edit is
/// in the session), the failures their errors, the fourth "not run".
async fn a_step_cut_short(wire: Wire, second: &str, third: &str, expected: &str) {
    let mut fx = Fixture::new(
        wire,
        vec![
            wire.classifier(),
            wire.calls(&[
                ("fine", "edit", FINE),
                ("bad1", "edit", second),
                ("bad2", "edit", third),
                ("unreached", "edit", r#"{"track":3}"#),
            ]),
            wire.classifier(),
            wire.text("Sorry about that. What next?"),
        ],
    )
    .await;

    let (result, events) = fx.turn("tidy the vocals").await;

    match result {
        Err(ai::Error::ToolValidation(message)) => {
            assert!(message.contains(expected), "{message}")
        }
        other => panic!("expected the validation error, got {other:?}"),
    }

    // The fourth call did not run; the first did, once.
    assert_eq!(fx.edits_run(), vec![json!({"track": 0})]);

    let failed = fx.agent.conversation().to_vec();
    fx.the_next_message_works(2, "Sorry about that. What next?")
        .await;

    // Four calls, four badges, each resolved once and as what happened.
    assert_every_badge_resolves(&events);
    assert_eq!(
        events
            .iter()
            .filter(|e| !matches!(e, Ev::ToolStart(_)))
            .collect::<Vec<_>>(),
        vec![
            &Ev::ToolEnd("fine".into(), true),
            &Ev::ToolEnd("bad1".into(), false),
            &Ev::ToolEnd("bad2".into(), false),
            &Ev::NotRun("unreached".into()),
        ]
    );

    // All four are answered, in the order they were called, in one message.
    assert_history_is_valid(&failed);
    let results = last_results(&failed);
    assert_eq!(
        results.iter().map(|(id, _, _)| *id).collect::<Vec<_>>(),
        vec!["fine", "bad1", "bad2", "unreached"]
    );
    assert_eq!(results[0].2, None, "the call that ran is not an error");
    assert!(results[0].1.contains("true"), "{:?}", results[0]);
    assert_eq!(results[1].2, Some(true));
    assert_eq!(results[2].2, Some(true));
    assert!(results[2].1.contains("stopped"), "{:?}", results[2]);
    assert_eq!(results[3].2, Some(true));
    assert!(results[3].1.starts_with("Not run"), "{:?}", results[3]);

    // And on the wire, the model reads the same four answers.
    let requests = fx.requests().await;
    let sent = requests.last().unwrap();
    for (id, error, starts) in [
        ("fine", false, ""),
        ("bad1", true, ""),
        ("bad2", true, ""),
        ("unreached", true, "Not run"),
    ] {
        let (said, is_error) = answer_to(wire, sent, id).unwrap_or_else(|| panic!("{id}"));
        assert_eq!(is_error, error, "{id}: {said}");
        assert!(said.starts_with(starts), "{id}: {said}");
    }
}

async fn a_step_cut_short_by_a_malformed_call(wire: Wire) {
    // Unparseable, then invalid: the turn ends in the schema check.
    a_step_cut_short(wire, TRUNCATED, INCOMPLETE, "failed validation twice").await
}
on_both_wires!(a_step_cut_short_by_a_malformed_call);

async fn a_step_cut_short_by_an_invalid_call(wire: Wire) {
    // Invalid, then unparseable: the turn ends in the JSON parse.
    a_step_cut_short(wire, INCOMPLETE, TRUNCATED, "malformed twice").await
}
on_both_wires!(a_step_cut_short_by_an_invalid_call);

// ---------------------------------------------------------------------
// A model that thinks by default (#484)
// ---------------------------------------------------------------------

/// Anthropic's 5.x models think by default and sign each thinking block
/// with the prefix that produced it. Closing the failed step must not
/// disturb that: the history stays append-only within the turn, so the
/// thinking of the step that failed is still on it, unmodified, and goes
/// away at the start of the next turn with no message left empty.
#[tokio::test]
async fn thinking_models_keep_a_valid_history_through_a_failed_turn() {
    let wire = Wire::Anthropic;
    let mut fx = Fixture::new(
        wire,
        vec![
            wire.classifier(),
            ok(sse_thinking_tool_step(
                "sig-1",
                "redacted-1",
                ("bad1", "edit", TRUNCATED),
            )),
            ok(sse_thinking_tool_step(
                "sig-2",
                "redacted-2",
                ("bad2", "edit", TRUNCATED),
            )),
            wire.classifier(),
            wire.text("Sorry about that. What next?"),
        ],
    )
    .await;

    let (result, _) = fx.turn("tidy the vocals").await;
    assert!(
        matches!(result, Err(ai::Error::ToolValidation(_))),
        "{result:?}"
    );

    // A copy of what the failed turn left, taken before the next turn
    // strips its thinking.
    let failed = fx.agent.conversation().to_vec();
    fx.the_next_message_works(3, "Sorry about that. What next?")
        .await;

    // Appended to, never rewritten: both steps keep their signed thinking,
    // and the history is whole (five messages, none empty, every call
    // answered).
    let signatures: Vec<&str> = failed
        .iter()
        .flat_map(|m| m.content.iter())
        .filter_map(|b| match b {
            ContentBlock::Thinking { signature, .. } => Some(signature.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(signatures, vec!["sig-1", "sig-2"]);
    assert_history_is_valid(&failed);
    assert_eq!(failed.len(), 5, "{failed:#?}");

    // In the turn, the second step replayed the first's thinking as it
    // was streamed (the prefix is fixed inside a turn).
    let requests = fx.requests().await;
    assert_eq!(
        requests[2]["messages"][1]["content"][0]["signature"], "sig-1",
        "{}",
        requests[2]["messages"]
    );

    // The next turn starts by stripping thinking, and what is left is
    // whole: nothing empty, every call still answered.
    let sent = requests.last().unwrap();
    let everything = sent["messages"].to_string();
    assert!(
        !everything.contains("thinking"),
        "thinking was replayed under a new prefix: {everything}"
    );
    for (i, message) in sent["messages"].as_array().unwrap().iter().enumerate() {
        assert!(
            message["content"].as_array().is_some_and(|c| !c.is_empty()),
            "message {i} is empty: {message}"
        );
    }
    let kinds: BTreeSet<String> = sent["messages"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|m| m["content"].as_array().unwrap())
        .map(|b| b["type"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        kinds,
        BTreeSet::from(["text", "tool_result", "tool_use"].map(String::from))
    );
}
