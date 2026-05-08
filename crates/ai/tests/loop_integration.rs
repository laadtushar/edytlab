//! Integration test for the agent tool-calling loop, with the
//! Anthropic API mocked via `wiremock`.
//!
//! The test:
//! 1. Spins up a `MockServer` that returns a recorded SSE stream
//!    matching one Anthropic `messages` response: a brief text
//!    introduction followed by a single `normalize` tool_use, then a
//!    follow-up response with no tool calls (just text).
//! 2. Seeds a real session store with one short WAV via the `load`
//!    tool (bypassing the agent — we're testing the agent loop, not
//!    the load tool).
//! 3. Runs `Agent::turn("normalize this to -1 dBFS", ...)` and asserts
//!    the event stream order, that exactly one node was created, and
//!    that the outgoing request body had `cache_control: ephemeral`
//!    on the system prompt.

use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Respond, ResponseTemplate};

// ---------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------

/// Build the SSE body the mocked Anthropic server will return for the
/// FIRST turn-step. The model says a sentence, then issues one
/// `normalize` tool call with `track=0, target_dbfs=-1.0`.
fn first_response_sse() -> String {
    let events: &[(&str, Value)] = &[
        (
            "message_start",
            json!({
                "type": "message_start",
                "message": {
                    "id": "msg_test_1",
                    "type": "message",
                    "role": "assistant",
                    "content": [],
                    "model": "claude-sonnet-4-6",
                    "stop_reason": null,
                    "stop_sequence": null,
                    "usage": { "input_tokens": 10, "output_tokens": 0 }
                }
            }),
        ),
        (
            "content_block_start",
            json!({
                "type": "content_block_start",
                "index": 0,
                "content_block": { "type": "text", "text": "" }
            }),
        ),
        (
            "content_block_delta",
            json!({
                "type": "content_block_delta",
                "index": 0,
                "delta": { "type": "text_delta", "text": "Sure, " }
            }),
        ),
        (
            "content_block_delta",
            json!({
                "type": "content_block_delta",
                "index": 0,
                "delta": { "type": "text_delta", "text": "normalizing now." }
            }),
        ),
        (
            "content_block_stop",
            json!({ "type": "content_block_stop", "index": 0 }),
        ),
        (
            "content_block_start",
            json!({
                "type": "content_block_start",
                "index": 1,
                "content_block": {
                    "type": "tool_use",
                    "id": "toolu_test_1",
                    "name": "normalize",
                    "input": {}
                }
            }),
        ),
        (
            "content_block_delta",
            json!({
                "type": "content_block_delta",
                "index": 1,
                "delta": {
                    "type": "input_json_delta",
                    "partial_json": "{\"track\":0,"
                }
            }),
        ),
        (
            "content_block_delta",
            json!({
                "type": "content_block_delta",
                "index": 1,
                "delta": {
                    "type": "input_json_delta",
                    "partial_json": "\"target_dbfs\":-1.0}"
                }
            }),
        ),
        (
            "content_block_stop",
            json!({ "type": "content_block_stop", "index": 1 }),
        ),
        (
            "message_delta",
            json!({
                "type": "message_delta",
                "delta": { "stop_reason": "tool_use", "stop_sequence": null },
                "usage": { "output_tokens": 20 }
            }),
        ),
        ("message_stop", json!({ "type": "message_stop" })),
    ];
    encode_sse(events)
}

/// SSE body for the SECOND turn-step (after we feed the tool result
/// back). The model just says "Done." and stops.
fn second_response_sse() -> String {
    let events: &[(&str, Value)] = &[
        (
            "message_start",
            json!({
                "type": "message_start",
                "message": {
                    "id": "msg_test_2",
                    "type": "message",
                    "role": "assistant",
                    "content": [],
                    "model": "claude-sonnet-4-6",
                    "stop_reason": null,
                    "stop_sequence": null,
                    "usage": { "input_tokens": 30, "output_tokens": 0 }
                }
            }),
        ),
        (
            "content_block_start",
            json!({
                "type": "content_block_start",
                "index": 0,
                "content_block": { "type": "text", "text": "" }
            }),
        ),
        (
            "content_block_delta",
            json!({
                "type": "content_block_delta",
                "index": 0,
                "delta": { "type": "text_delta", "text": "Done." }
            }),
        ),
        (
            "content_block_stop",
            json!({ "type": "content_block_stop", "index": 0 }),
        ),
        (
            "message_delta",
            json!({
                "type": "message_delta",
                "delta": { "stop_reason": "end_turn", "stop_sequence": null },
                "usage": { "output_tokens": 5 }
            }),
        ),
        ("message_stop", json!({ "type": "message_stop" })),
    ];
    encode_sse(events)
}

/// A minimal non-streaming Anthropic response for the classifier call
/// (classify_mode uses `stream: false` with a single-turn request).
/// Returns `"general"` so the existing tool loop is used unchanged.
fn classifier_response_json() -> String {
    serde_json::json!({
        "id": "msg_classify",
        "type": "message",
        "role": "assistant",
        "model": "claude-haiku-4-5-20251001",
        "stop_reason": "end_turn",
        "content": [{"type": "text", "text": "general"}],
        "usage": {"input_tokens": 10, "output_tokens": 1}
    })
    .to_string()
}

fn encode_sse(events: &[(&str, Value)]) -> String {
    let mut out = String::new();
    for (name, data) in events {
        out.push_str("event: ");
        out.push_str(name);
        out.push('\n');
        out.push_str("data: ");
        out.push_str(&serde_json::to_string(data).unwrap());
        out.push_str("\n\n");
    }
    out
}

/// Sequenced responder: returns `responses[i]` on the i-th request,
/// then 500s. Wiremock's built-in matchers don't have ordering, so we
/// roll our own.
///
/// If a response body starts with `{` we assume it is a JSON
/// (non-streaming) response and set `content-type: application/json`.
/// Otherwise we use `text/event-stream` for SSE bodies.
struct SeqResponder {
    counter: Mutex<usize>,
    responses: Vec<String>,
}

impl Respond for SeqResponder {
    fn respond(&self, _req: &wiremock::Request) -> ResponseTemplate {
        let mut c = self.counter.lock().unwrap();
        let idx = *c;
        *c += 1;
        match self.responses.get(idx) {
            Some(body) => {
                let ct = if body.trim_start().starts_with('{') {
                    "application/json"
                } else {
                    "text/event-stream"
                };
                ResponseTemplate::new(200)
                    .insert_header("content-type", ct)
                    .set_body_string(body.clone())
            }
            None => ResponseTemplate::new(500).set_body_string("no more responses"),
        }
    }
}

/// Write a 1s, 1ch, 44.1kHz WAV with a single 0.5-amplitude tone so
/// `normalize` has something to scan. Returns the file path; the
/// `tempfile::TempDir` keeps the file alive.
fn write_test_wav(dir: &std::path::Path) -> std::path::PathBuf {
    let path = dir.join("input.wav");
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 44100,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(&path, spec).expect("wav writer");
    // 100 ms of half-amplitude sine — peak ~0.5 linear, so normalize to
    // -1 dBFS should ADD ~+5 dB of gain.
    for i in 0..4410 {
        let t = i as f32 / 44100.0;
        let s = (t * 440.0 * std::f32::consts::TAU).sin() * 0.5;
        let int_sample = (s * i16::MAX as f32) as i16;
        writer.write_sample(int_sample).unwrap();
    }
    writer.finalize().unwrap();
    path
}

// ---------------------------------------------------------------------
// The test
// ---------------------------------------------------------------------

#[tokio::test]
async fn agent_dispatches_normalize_and_emits_node_created() {
    // Mock server.
    // The first request is the M27 mode-classification call (non-streaming
    // JSON response); the subsequent two are the normal tool-calling loop
    // (SSE responses).
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(SeqResponder {
            counter: Mutex::new(0),
            responses: vec![
                classifier_response_json(),
                first_response_sse(),
                second_response_sse(),
            ],
        })
        .mount(&server)
        .await;

    // Project + WAV.
    let project = tempfile::tempdir().expect("tempdir");
    let wav_path = write_test_wav(project.path());

    // Real shared state.
    let dispatcher = Arc::new(Mutex::new(tools::ToolDispatcher::default_dispatcher()));
    let store = Arc::new(Mutex::new(
        session::Store::open(project.path()).expect("open store"),
    ));
    let engine = Arc::new(Mutex::new(audio_engine::Engine::new()));

    // Seed the store with a load via the dispatcher directly.
    {
        let d = dispatcher.lock().unwrap();
        let mut s = store.lock().unwrap();
        let mut e = engine.lock().unwrap();
        let mut ctx = tools::ToolContext {
            store: &mut s,
            engine: &mut e,
        };
        let res = d
            .invoke(
                "load",
                json!({ "path": wav_path.to_string_lossy() }),
                &mut ctx,
            )
            .expect("dispatch load");
        match res {
            tools::ToolResult::Ok(_) => {}
            tools::ToolResult::Error(e) => panic!("load failed: {e}"),
        }
    }

    // Agent pointed at the mock.
    let cfg = ai::LlmConfig::new_anthropic("test-key").with_base_url(server.uri());
    let plan_notify = Arc::new(tokio::sync::Notify::new());
    let mut agent = ai::Agent::new(
        cfg,
        dispatcher.clone(),
        store.clone(),
        engine.clone(),
        plan_notify,
    );

    let mut events: Vec<ai::AgentEvent> = Vec::new();
    let result = agent
        .turn("normalize this to -1 dBFS".to_string(), |ev| {
            events.push(ev)
        })
        .await
        .expect("turn");

    // ---- Event order assertions ----
    let mut saw_text = false;
    let mut saw_tool_start = false;
    let mut saw_node = false;
    let mut saw_tool_end_ok = false;
    let mut saw_done = false;
    for ev in &events {
        match ev {
            ai::AgentEvent::TextDelta(_) => saw_text = true,
            ai::AgentEvent::ToolCallStart { name, .. } => {
                assert_eq!(name, "normalize");
                assert!(saw_text, "text deltas should arrive before tool_use start");
                saw_tool_start = true;
            }
            ai::AgentEvent::NodeCreated(_) => {
                assert!(saw_tool_start);
                saw_node = true;
            }
            ai::AgentEvent::ToolCallEnd { ok, .. } => {
                assert!(*ok, "normalize should succeed");
                assert!(saw_node, "NodeCreated should precede ToolCallEnd");
                saw_tool_end_ok = true;
            }
            ai::AgentEvent::Done => {
                assert!(saw_tool_end_ok);
                saw_done = true;
            }
            // Plan events are only emitted in mashup mode (which the
            // integration test does not exercise — the mock server is
            // not the classifier endpoint). Ignore here.
            ai::AgentEvent::Plan { .. } => {}
        }
    }
    assert!(saw_text && saw_tool_start && saw_node && saw_tool_end_ok && saw_done);

    // Exactly one normalize call → one new session node.
    assert_eq!(result.node_ids.len(), 1, "expected one NodeCreated");
    assert!(
        result.text.contains("Done"),
        "final text should include the post-tool message; got: {:?}",
        result.text
    );

    // ---- Outgoing request shape: cache_control on system prompt ----
    let received = server.received_requests().await.expect("recording on");
    // M27 adds a classification pre-flight, so we now expect 3 round-trips:
    // [0] classify_mode (haiku, stream:false)
    // [1] first main turn (normalize tool_use)
    // [2] second main turn (final text)
    assert_eq!(
        received.len(),
        3,
        "expected three HTTP round-trips (classify + 2 main)"
    );
    let body: Value = serde_json::from_slice(&received[1].body).expect("json body");
    let system = body
        .get("system")
        .and_then(|v| v.as_array())
        .expect("system array");
    assert!(!system.is_empty());
    let cache = system[0]
        .get("cache_control")
        .expect("cache_control on system block");
    assert_eq!(
        cache.get("type").and_then(|v| v.as_str()),
        Some("ephemeral")
    );
    // Model + tool_choice present.
    assert_eq!(
        body.get("model").and_then(|v| v.as_str()),
        Some("claude-sonnet-4-6")
    );
    assert_eq!(
        body.get("tool_choice")
            .and_then(|v| v.get("type"))
            .and_then(|v| v.as_str()),
        Some("auto")
    );
    // Tools array also has cache_control on its last entry.
    let tools_arr = body
        .get("tools")
        .and_then(|v| v.as_array())
        .expect("tools array");
    assert!(!tools_arr.is_empty());
    let last_tool = tools_arr.last().unwrap();
    assert_eq!(
        last_tool
            .get("cache_control")
            .and_then(|v| v.get("type"))
            .and_then(|v| v.as_str()),
        Some("ephemeral"),
        "last tool entry should carry cache_control: ephemeral"
    );
    // Stream flag.
    assert_eq!(body.get("stream").and_then(|v| v.as_bool()), Some(true));

    // Third request (index 2): messages now contains the assistant's tool_use
    // and the user-role tool_result we appended.
    let body2: Value = serde_json::from_slice(&received[2].body).expect("json body 2");
    let msgs = body2
        .get("messages")
        .and_then(|v| v.as_array())
        .expect("messages");
    // user (initial) -> assistant (text + tool_use) -> user (tool_result)
    assert_eq!(msgs.len(), 3);
    assert_eq!(
        msgs[1].get("role").and_then(|v| v.as_str()),
        Some("assistant")
    );
    let assistant_blocks = msgs[1].get("content").and_then(|v| v.as_array()).unwrap();
    let has_tool_use = assistant_blocks
        .iter()
        .any(|b| b.get("type").and_then(|v| v.as_str()) == Some("tool_use"));
    assert!(has_tool_use);
    assert_eq!(msgs[2].get("role").and_then(|v| v.as_str()), Some("user"));
    let user_blocks = msgs[2].get("content").and_then(|v| v.as_array()).unwrap();
    let has_tool_result = user_blocks
        .iter()
        .any(|b| b.get("type").and_then(|v| v.as_str()) == Some("tool_result"));
    assert!(has_tool_result);
}

// ---------------------------------------------------------------------
// Bonus: the tool-call cap is enforced.
// ---------------------------------------------------------------------

/// Build an SSE stream that requests N tool calls in a single message.
/// Used to verify the per-turn cap. We use `gain` (a real registered
/// tool) so the dispatcher doesn't reject on "unknown tool".
fn loop_response_sse(n: usize) -> String {
    let mut events: Vec<(&str, Value)> = Vec::new();
    events.push((
        "message_start",
        json!({
            "type": "message_start",
            "message": {
                "id": "msg_loop",
                "type": "message",
                "role": "assistant",
                "content": [],
                "model": "claude-sonnet-4-6",
                "stop_reason": null,
                "stop_sequence": null,
                "usage": { "input_tokens": 1, "output_tokens": 0 }
            }
        }),
    ));
    for i in 0..n {
        events.push((
            "content_block_start",
            json!({
                "type": "content_block_start",
                "index": i,
                "content_block": {
                    "type": "tool_use",
                    "id": format!("toolu_{i}"),
                    "name": "gain",
                    "input": {}
                }
            }),
        ));
        events.push((
            "content_block_delta",
            json!({
                "type": "content_block_delta",
                "index": i,
                "delta": {
                    "type": "input_json_delta",
                    "partial_json": "{\"track\":0,\"db\":1.0}"
                }
            }),
        ));
        events.push((
            "content_block_stop",
            json!({ "type": "content_block_stop", "index": i }),
        ));
    }
    events.push((
        "message_delta",
        json!({
            "type": "message_delta",
            "delta": { "stop_reason": "tool_use", "stop_sequence": null },
            "usage": { "output_tokens": 1 }
        }),
    ));
    events.push(("message_stop", json!({ "type": "message_stop" })));
    encode_sse(&events)
}

#[tokio::test]
async fn agent_enforces_tool_call_cap() {
    let server = MockServer::start().await;
    // Always respond with 11 tool calls in one message — exceeds the
    // hard cap of 10.
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(loop_response_sse(11)),
        )
        .mount(&server)
        .await;

    let project = tempfile::tempdir().unwrap();
    let wav_path = write_test_wav(project.path());

    let dispatcher = Arc::new(Mutex::new(tools::ToolDispatcher::default_dispatcher()));
    let store = Arc::new(Mutex::new(session::Store::open(project.path()).unwrap()));
    let engine = Arc::new(Mutex::new(audio_engine::Engine::new()));

    {
        let d = dispatcher.lock().unwrap();
        let mut s = store.lock().unwrap();
        let mut e = engine.lock().unwrap();
        let mut ctx = tools::ToolContext {
            store: &mut s,
            engine: &mut e,
        };
        d.invoke(
            "load",
            json!({ "path": wav_path.to_string_lossy() }),
            &mut ctx,
        )
        .unwrap();
    }

    let cfg = ai::LlmConfig::new_anthropic("test-key").with_base_url(server.uri());
    let plan_notify = Arc::new(tokio::sync::Notify::new());
    let mut agent = ai::Agent::new(cfg, dispatcher, store, engine, plan_notify);

    let err = agent
        .turn("loop please".to_string(), |_| {})
        .await
        .expect_err("should hit cap");
    match err {
        ai::Error::ToolBudgetExceeded(n) => assert_eq!(n, ai::MAX_TOOL_CALLS_PER_TURN),
        other => panic!("expected ToolBudgetExceeded, got: {other}"),
    }
}
