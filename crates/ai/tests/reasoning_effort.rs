//! The reasoning-effort setting, end to end against a mocked provider
//! (#442).
//!
//! A higher effort changes more than one field of the request. The model
//! thinks before it answers, and the API streams that thinking back as a
//! `thinking` block (with a signature) ahead of the text and tool calls.
//! These drive whole turns through the public `Agent` API and check what
//! goes on the wire in both directions:
//!
//! * the effort is sent as `output_config.effort` on the streaming
//!   requests and not on the classifier's;
//! * the thinking that came back is replayed, unmodified and first, in
//!   the request that follows the tool call it led to;
//! * block types the client does not model are skipped, not errors;
//! * with no effort set nothing is added, and a provider that cannot take
//!   the field never sees it.

mod common;

use std::sync::{Arc, Mutex};

use ai::anthropic::{ContentBlock, Role};
use ai::{Agent, Effort, LlmConfig};
use common::{classifier_json, encode_sse, ok, SeqResponder};
use serde_json::{json, Value};
use tools::schema::{anthropic_tool, object_schema};
use tools::{Tool, ToolContext, ToolDispatcher, ToolResult};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer};

/// A tool the model can call, so a turn has a tool round-trip.
struct Echo;

impl Tool for Echo {
    fn name(&self) -> &'static str {
        "echo"
    }
    fn schema(&self) -> Value {
        anthropic_tool(
            "echo",
            "Reads the session.",
            object_schema(&[("track", "integer", true)]),
        )
    }
    fn mutates(&self) -> bool {
        false
    }
    fn invoke(&self, _args: Value, _ctx: &mut ToolContext) -> tools::Result<ToolResult> {
        Ok(ToolResult::Ok(json!({ "ok": true })))
    }
}

const THINKING: &str = "The user wants track 1 read first.";
const SIGNATURE: &str = "EqQBCkYIBxgCKkB-signature-bytes";
const REDACTED: &str = "EmwKAhgBEgy3va3pzix-opaque";

fn message_start() -> (&'static str, Value) {
    (
        "message_start",
        json!({
            "type": "message_start",
            "message": {
                "id": "msg_test", "type": "message", "role": "assistant", "content": [],
                "model": "claude-sonnet-4-6", "stop_reason": null, "stop_sequence": null,
                "usage": { "input_tokens": 1, "output_tokens": 0 }
            }
        }),
    )
}

fn stop(index: usize) -> (&'static str, Value) {
    (
        "content_block_stop",
        json!({ "type": "content_block_stop", "index": index }),
    )
}

fn finish(reason: &str) -> Vec<(&'static str, Value)> {
    vec![
        (
            "message_delta",
            json!({
                "type": "message_delta",
                "delta": { "stop_reason": reason, "stop_sequence": null },
                "usage": { "output_tokens": 1 }
            }),
        ),
        ("message_stop", json!({ "type": "message_stop" })),
    ]
}

/// The first model step as a high-effort request gets it: thinking (text
/// then signature), a redacted thinking block, a block of a type this
/// client has never heard of with a delta to match, the text, and a tool
/// call.
fn thinking_tool_step() -> String {
    let mut events = vec![message_start()];
    events.push((
        "content_block_start",
        json!({ "type": "content_block_start", "index": 0,
                "content_block": { "type": "thinking", "thinking": "", "signature": "" } }),
    ));
    events.push((
        "content_block_delta",
        json!({ "type": "content_block_delta", "index": 0,
                "delta": { "type": "thinking_delta", "thinking": "The user wants " } }),
    ));
    events.push((
        "content_block_delta",
        json!({ "type": "content_block_delta", "index": 0,
                "delta": { "type": "thinking_delta", "thinking": "track 1 read first." } }),
    ));
    events.push((
        "content_block_delta",
        json!({ "type": "content_block_delta", "index": 0,
                "delta": { "type": "signature_delta", "signature": SIGNATURE } }),
    ));
    events.push(stop(0));
    events.push((
        "content_block_start",
        json!({ "type": "content_block_start", "index": 1,
                "content_block": { "type": "redacted_thinking", "data": REDACTED } }),
    ));
    events.push(stop(1));
    events.push((
        "content_block_start",
        json!({ "type": "content_block_start", "index": 2,
                "content_block": { "type": "some_future_block", "payload": [1, 2, 3] } }),
    ));
    events.push((
        "content_block_delta",
        json!({ "type": "content_block_delta", "index": 2,
                "delta": { "type": "some_future_delta", "payload": { "a": 1 } } }),
    ));
    events.push(stop(2));
    events.push((
        "content_block_start",
        json!({ "type": "content_block_start", "index": 3,
                "content_block": { "type": "text", "text": "" } }),
    ));
    events.push((
        "content_block_delta",
        json!({ "type": "content_block_delta", "index": 3,
                "delta": { "type": "text_delta", "text": "Reading it." } }),
    ));
    events.push(stop(3));
    events.push((
        "content_block_start",
        json!({ "type": "content_block_start", "index": 4,
                "content_block": { "type": "tool_use", "id": "toolu_1", "name": "echo",
                                   "input": {}, "caller": { "type": "direct" } } }),
    ));
    events.push((
        "content_block_delta",
        json!({ "type": "content_block_delta", "index": 4,
                "delta": { "type": "input_json_delta", "partial_json": "{\"track\":1}" } }),
    ));
    events.push(stop(4));
    events.extend(finish("tool_use"));
    encode_sse(&events)
}

/// The second step: a thinking block again, then the answer.
fn thinking_final_step() -> String {
    let mut events = vec![message_start()];
    events.push((
        "content_block_start",
        json!({ "type": "content_block_start", "index": 0,
                "content_block": { "type": "thinking", "thinking": "", "signature": "" } }),
    ));
    events.push((
        "content_block_delta",
        json!({ "type": "content_block_delta", "index": 0,
                "delta": { "type": "thinking_delta", "thinking": "All good." } }),
    ));
    events.push((
        "content_block_delta",
        json!({ "type": "content_block_delta", "index": 0,
                "delta": { "type": "signature_delta", "signature": "SECOND-SIG" } }),
    ));
    events.push(stop(0));
    events.push((
        "content_block_start",
        json!({ "type": "content_block_start", "index": 1,
                "content_block": { "type": "text", "text": "" } }),
    ));
    events.push((
        "content_block_delta",
        json!({ "type": "content_block_delta", "index": 1,
                "delta": { "type": "text_delta", "text": "Track 1 is fine." } }),
    ));
    events.push(stop(1));
    events.extend(finish("end_turn"));
    encode_sse(&events)
}

struct Run {
    result: ai::Result<ai::TurnResult>,
    requests: Vec<Value>,
    history: Vec<ai::anthropic::Message>,
}

/// One turn against a mock that answers classifier, tool step, final step.
async fn run(cfg: LlmConfig) -> Run {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(SeqResponder::new(vec![
            ok(classifier_json("general")),
            ok(thinking_tool_step()),
            ok(thinking_final_step()),
        ]))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().expect("tempdir");
    let mut dispatcher = ToolDispatcher::new();
    dispatcher.register(Box::new(Echo));
    let mut agent = Agent::new(
        cfg.with_base_url(server.uri()),
        Arc::new(Mutex::new(dispatcher)),
        Arc::new(Mutex::new(
            session::Store::open(dir.path()).expect("open store"),
        )),
        Arc::new(Mutex::new(audio_engine::Engine::new())),
        Arc::new(tokio::sync::Notify::new()),
        Arc::new(Mutex::new(None)),
        Arc::new(std::sync::atomic::AtomicBool::new(false)),
        Arc::new(Mutex::new(None::<tools::Clipboard>)),
    );
    let result = agent.turn("read track 1".to_string(), |_| {}).await;
    let requests = server
        .received_requests()
        .await
        .expect("recording on")
        .iter()
        .map(|r| serde_json::from_slice(&r.body).expect("a JSON body"))
        .collect();
    Run {
        result,
        requests,
        history: agent.conversation().to_vec(),
    }
}

fn block_types(message: &Value) -> Vec<&str> {
    message["content"]
        .as_array()
        .expect("content array")
        .iter()
        .map(|b| b["type"].as_str().expect("typed block"))
        .collect()
}

/// The whole thing on Anthropic at `xhigh`.
#[tokio::test]
async fn a_turn_at_xhigh_sends_the_effort_and_replays_the_thinking() {
    let r = run(LlmConfig::new_anthropic("k").with_effort(Effort::XHigh)).await;
    let result = r
        .result
        .expect("the turn must not error on thinking blocks");
    assert_eq!(result.text, "Reading it.Track 1 is fine.");

    // Classifier, tool step, final step.
    assert_eq!(r.requests.len(), 3);

    // The classifier runs on the cheap model, which rejects `effort`.
    assert!(
        r.requests[0].get("output_config").is_none(),
        "the classifier must not carry an effort: {}",
        r.requests[0]
    );

    for (i, body) in r.requests[1..].iter().enumerate() {
        assert_eq!(
            body["output_config"],
            json!({ "effort": "xhigh" }),
            "streaming request {i}"
        );
        assert!(body.get("effort").is_none(), "no top-level effort");
        // Room to think and still answer.
        assert_eq!(body["max_tokens"], 16384, "streaming request {i}");
    }

    // The follow-up replays the model's step as it came: thinking, then
    // redacted thinking, then the text and the tool call. The block of a
    // type nobody models is gone.
    let followup = &r.requests[2];
    let assistant = &followup["messages"][1];
    assert_eq!(assistant["role"], "assistant");
    assert_eq!(
        block_types(assistant),
        ["thinking", "redacted_thinking", "text", "tool_use"]
    );
    let blocks = assistant["content"].as_array().unwrap();
    assert_eq!(
        blocks[0],
        json!({ "type": "thinking", "thinking": THINKING, "signature": SIGNATURE })
    );
    assert_eq!(
        blocks[1],
        json!({ "type": "redacted_thinking", "data": REDACTED })
    );
    assert_eq!(blocks[2]["text"], "Reading it.");
    assert_eq!(blocks[3]["id"], "toolu_1");
    assert_eq!(blocks[3]["input"], json!({ "track": 1 }));
    // The tool's answer follows it.
    assert_eq!(followup["messages"][2]["role"], "user");

    // The history holds the same, plus the final step's own thinking.
    let last = r.history.last().expect("a final message");
    assert_eq!(last.role, Role::Assistant);
    assert!(matches!(
        &last.content[0],
        ContentBlock::Thinking { signature, .. } if signature == "SECOND-SIG"
    ));
}

/// Unset: not one request carries the field, and the cap is the one the
/// loop always used. The thinking is still kept, because the server that
/// produced it asks for it back whether or not an effort was chosen.
#[tokio::test]
async fn a_turn_with_no_effort_adds_nothing_to_any_request() {
    let r = run(LlmConfig::new_anthropic("k")).await;
    r.result
        .expect("the turn must not error on thinking blocks");
    assert_eq!(r.requests.len(), 3);
    for body in &r.requests {
        let text = body.to_string();
        assert!(!text.contains("output_config"), "{text}");
        assert!(!text.contains("\"effort\""), "{text}");
    }
    assert_eq!(r.requests[1]["max_tokens"], ai::prompt::DEFAULT_MAX_TOKENS);
    assert_eq!(
        block_types(&r.requests[2]["messages"][1]),
        ["thinking", "redacted_thinking", "text", "tool_use"]
    );
}

/// OpenRouter speaks the same wire shape, but it is a gateway and not
/// the API that defines `output_config`, and it did not issue the
/// signatures. A level left over from Anthropic stays off its wire and
/// its history is exactly what it was before this setting existed.
#[tokio::test]
async fn openrouter_never_gets_the_effort_and_keeps_no_thinking() {
    let r = run(LlmConfig::new_openrouter("k").with_effort(Effort::Max)).await;
    r.result
        .expect("the turn must not error on thinking blocks");
    assert_eq!(r.requests.len(), 3);
    for body in &r.requests {
        let text = body.to_string();
        assert!(!text.contains("output_config"), "{text}");
        assert!(!text.contains("\"effort\""), "{text}");
    }
    assert_eq!(r.requests[1]["max_tokens"], ai::prompt::DEFAULT_MAX_TOKENS);
    assert_eq!(
        block_types(&r.requests[2]["messages"][1]),
        ["text", "tool_use"]
    );
}

/// A reply that is nothing but thinking (cut off at `max_tokens`) is not
/// an assistant turn to replay.
#[tokio::test]
async fn a_reply_that_is_only_thinking_leaves_no_assistant_message() {
    let mut events = vec![message_start()];
    events.push((
        "content_block_start",
        json!({ "type": "content_block_start", "index": 0,
                "content_block": { "type": "thinking", "thinking": "", "signature": "" } }),
    ));
    events.push((
        "content_block_delta",
        json!({ "type": "content_block_delta", "index": 0,
                "delta": { "type": "thinking_delta", "thinking": "hmm" } }),
    ));
    events.push((
        "content_block_delta",
        json!({ "type": "content_block_delta", "index": 0,
                "delta": { "type": "signature_delta", "signature": "SIG" } }),
    ));
    events.push(stop(0));
    events.extend(finish("max_tokens"));

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(SeqResponder::new(vec![
            ok(classifier_json("general")),
            ok(encode_sse(&events)),
        ]))
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().expect("tempdir");
    let mut agent = Agent::new(
        LlmConfig::new_anthropic("k")
            .with_effort(Effort::Max)
            .with_base_url(server.uri()),
        Arc::new(Mutex::new(ToolDispatcher::new())),
        Arc::new(Mutex::new(
            session::Store::open(dir.path()).expect("open store"),
        )),
        Arc::new(Mutex::new(audio_engine::Engine::new())),
        Arc::new(tokio::sync::Notify::new()),
        Arc::new(Mutex::new(None)),
        Arc::new(std::sync::atomic::AtomicBool::new(false)),
        Arc::new(Mutex::new(None::<tools::Clipboard>)),
    );
    let result = agent
        .turn("hello".to_string(), |_| {})
        .await
        .expect("a thinking-only reply is not an error");
    assert_eq!(result.stop_reason.as_deref(), Some("max_tokens"));
    assert!(
        agent.conversation().iter().all(|m| m.role == Role::User),
        "only the user's message is in the history: {:?}",
        agent.conversation()
    );
}
