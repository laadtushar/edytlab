//! Thinking blocks and the prefix they were produced under (#468).
//!
//! Anthropic's 5.x models (Fable 5.1, Opus 5.5, Sonnet 5.5, Haiku 5.5)
//! think by default and sign every thinking block with the request prefix
//! that produced it: the top-level `system`, the `tools` set, and every
//! message before it. A block replayed under a different prefix is a 400
//! (`Invalid signature in thinking block ... bound to a different
//! conversation`) for accounts created on or after 2026-08-31.
//!
//! This agent rebuilds `system` and may change `tools` on every turn, so
//! these drive whole turns through the public `Agent` API against a mocked
//! provider and check what goes on the wire:
//!
//! * the next turn replays no thinking from earlier turns, whether it is
//!   `system` or `tools` that changed, and keeps every text, `tool_use`
//!   and `tool_result` block in order;
//! * inside a turn the prefix is the same on every step, and the thinking
//!   a step produced is still replayed unmodified with the tool result
//!   that follows it (which is only valid because the prefix is fixed);
//! * the classifier still gets an answer from a model that thinks by
//!   default, so the mode it picks is not lost.

mod common;

use std::sync::{Arc, Mutex};

use ai::anthropic::{ContentBlock, Role};
use ai::{Agent, LlmConfig, SessionContext};
use common::{
    classifier_json, ok, sse_text, sse_thinking_text, sse_thinking_tool_step, SeqResponder,
};
use serde_json::{json, Value};
use tools::schema::{anthropic_tool, object_schema};
use tools::{Tool, ToolContext, ToolDispatcher, ToolResult};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

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

/// A second tool, so a whitelist can take one away.
struct Noop;

impl Tool for Noop {
    fn name(&self) -> &'static str {
        "noop"
    }
    fn schema(&self) -> Value {
        anthropic_tool("noop", "Does nothing.", object_schema(&[]))
    }
    fn mutates(&self) -> bool {
        false
    }
    fn invoke(&self, _args: Value, _ctx: &mut ToolContext) -> tools::Result<ToolResult> {
        Ok(ToolResult::Ok(json!({})))
    }
}

fn agent_against(server: &MockServer, dir: &std::path::Path) -> Agent {
    let mut dispatcher = ToolDispatcher::new();
    dispatcher.register(Box::new(Echo));
    dispatcher.register(Box::new(Noop));
    Agent::new(
        LlmConfig::new_anthropic("k").with_base_url(server.uri()),
        Arc::new(Mutex::new(dispatcher)),
        Arc::new(Mutex::new(session::Store::open(dir).expect("open store"))),
        Arc::new(Mutex::new(audio_engine::Engine::new())),
        Arc::new(tokio::sync::Notify::new()),
        Arc::new(Mutex::new(None)),
        Arc::new(std::sync::atomic::AtomicBool::new(false)),
        Arc::new(Mutex::new(None::<tools::Clipboard>)),
    )
}

/// Two turns, each: classifier, a step that thinks and calls `echo`, a
/// step that thinks and answers. Every block is signed with a name that
/// says which step made it, so a replayed block can be traced.
async fn mount_two_turns(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(SeqResponder::new(vec![
            ok(classifier_json("general")),
            ok(sse_thinking_tool_step(
                "SIG-T1-A",
                "RED-T1-A",
                ("toolu_1", "echo", "{\"track\":1}"),
            )),
            ok(sse_thinking_text("SIG-T1-B", "Track 1 is fine.")),
            ok(classifier_json("general")),
            ok(sse_thinking_tool_step(
                "SIG-T2-A",
                "RED-T2-A",
                ("toolu_2", "echo", "{\"track\":2}"),
            )),
            ok(sse_thinking_text("SIG-T2-B", "Track 2 is fine.")),
        ]))
        .mount(server)
        .await;
}

async fn bodies(server: &MockServer) -> Vec<Value> {
    server
        .received_requests()
        .await
        .expect("recording on")
        .iter()
        .map(|r| serde_json::from_slice(&r.body).expect("a JSON body"))
        .collect()
}

fn context_at(head: &str) -> SessionContext {
    SessionContext {
        head: Some(head.to_string()),
        ..SessionContext::default()
    }
}

/// `role:type,type` for every message, so a conversation's shape reads
/// as one comparable value.
fn shape(messages: &Value) -> Vec<String> {
    messages
        .as_array()
        .expect("messages")
        .iter()
        .map(|m| {
            let types: Vec<&str> = m["content"]
                .as_array()
                .expect("content array")
                .iter()
                .map(|b| b["type"].as_str().expect("typed block"))
                .collect();
            format!("{}:{}", m["role"].as_str().expect("role"), types.join(","))
        })
        .collect()
}

/// The `type` of every content block in `messages`, flattened.
fn all_block_types(messages: &Value) -> Vec<String> {
    messages
        .as_array()
        .expect("messages")
        .iter()
        .flat_map(|m| m["content"].as_array().expect("content array").iter())
        .map(|b| b["type"].as_str().expect("typed block").to_string())
        .collect()
}

fn has_thinking(messages: &Value) -> bool {
    all_block_types(messages)
        .iter()
        .any(|t| t == "thinking" || t == "redacted_thinking")
}

/// The pair of requests the whole fix is about. `system` changes between
/// turns (the head moved), and turn 2's first request must carry none of
/// turn 1's thinking.
#[tokio::test]
async fn a_new_turn_replays_no_thinking_when_the_system_prompt_changed() {
    let server = MockServer::start().await;
    mount_two_turns(&server).await;
    let dir = tempfile::tempdir().expect("tempdir");
    let mut agent = agent_against(&server, dir.path());

    agent
        .turn_with_context("read track 1".into(), Some(&context_at("aaaa")), |_| {})
        .await
        .expect("turn 1");
    agent
        .turn_with_context("read track 2".into(), Some(&context_at("bbbb")), |_| {})
        .await
        .expect("turn 2");

    let r = bodies(&server).await;
    // Classifier, two steps, classifier, two steps.
    assert_eq!(r.len(), 6, "{r:#?}");
    let (t1_step1, t1_step2) = (&r[1], &r[2]);
    let (t2_step1, t2_step2) = (&r[4], &r[5]);

    // The premise: the session context is in `system`, and it moved.
    assert_ne!(
        t1_step1["system"], t2_step1["system"],
        "turn 2 was meant to send a different system prompt"
    );

    // Inside turn 1 nothing is rebuilt, which is what makes replaying a
    // step's thinking with its tool result valid.
    assert_eq!(t1_step1["system"], t1_step2["system"]);
    assert_eq!(t1_step1["tools"], t1_step2["tools"]);
    // ...and it is replayed there, first, exactly as received.
    assert_eq!(
        shape(&t1_step2["messages"]),
        [
            "user:text",
            "assistant:thinking,redacted_thinking,text,tool_use",
            "user:tool_result"
        ]
    );
    let replayed = &t1_step2["messages"][1]["content"];
    assert_eq!(replayed[0]["signature"], "SIG-T1-A");
    assert_eq!(replayed[1]["data"], "RED-T1-A");

    // Turn 2 starts: none of turn 1's thinking, in any message.
    assert!(
        !has_thinking(&t2_step1["messages"]),
        "an earlier turn's thinking was replayed under a new system prompt: {}",
        t2_step1["messages"]
    );
    // Everything else survived, in order, with the call and its result
    // still paired.
    assert_eq!(
        shape(&t2_step1["messages"]),
        [
            "user:text",
            "assistant:text,tool_use",
            "user:tool_result",
            "assistant:text",
            "user:text"
        ]
    );
    let messages = &t2_step1["messages"];
    assert_eq!(messages[0]["content"][0]["text"], "read track 1");
    assert_eq!(messages[1]["content"][0]["text"], "Working on it.");
    assert_eq!(messages[1]["content"][1]["id"], "toolu_1");
    assert_eq!(messages[1]["content"][1]["input"], json!({ "track": 1 }));
    assert_eq!(messages[2]["content"][0]["tool_use_id"], "toolu_1");
    assert_eq!(messages[3]["content"][0]["text"], "Track 1 is fine.");
    assert_eq!(messages[4]["content"][0]["text"], "read track 2");

    // Turn 2's own thinking is replayed within turn 2, first, and only
    // that: the earlier turn's is still gone.
    assert_eq!(t2_step1["system"], t2_step2["system"]);
    assert_eq!(t2_step1["tools"], t2_step2["tools"]);
    assert_eq!(
        shape(&t2_step2["messages"]),
        [
            "user:text",
            "assistant:text,tool_use",
            "user:tool_result",
            "assistant:text",
            "user:text",
            "assistant:thinking,redacted_thinking,text,tool_use",
            "user:tool_result"
        ]
    );
    let own = &t2_step2["messages"][5]["content"];
    assert_eq!(own[0]["signature"], "SIG-T2-A");
    assert_eq!(own[1]["data"], "RED-T2-A");

    // Every message sent had something in it: stripping left none empty.
    for body in r.iter().filter(|b| b["stream"] == json!(true)) {
        for m in body["messages"].as_array().unwrap() {
            assert!(!m["content"].as_array().unwrap().is_empty(), "{body}");
        }
    }

    // The history after turn 2 holds turn 2's thinking and not turn 1's.
    let history = agent.conversation();
    let thinking_signatures: Vec<&str> = history
        .iter()
        .flat_map(|m| m.content.iter())
        .filter_map(|b| match b {
            ContentBlock::Thinking { signature, .. } => Some(signature.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(thinking_signatures, ["SIG-T2-A", "SIG-T2-B"]);
    assert_eq!(
        history.last().expect("a last message").role,
        Role::Assistant
    );
}

/// The other half of the prefix: a different tool set, with the very same
/// system prompt, is just as much a new prefix.
#[tokio::test]
async fn a_new_turn_replays_no_thinking_when_only_the_tools_changed() {
    let server = MockServer::start().await;
    mount_two_turns(&server).await;
    let dir = tempfile::tempdir().expect("tempdir");
    let mut agent = agent_against(&server, dir.path());
    let ctx = context_at("aaaa");

    agent
        .turn_with_context("read track 1".into(), Some(&ctx), |_| {})
        .await
        .expect("turn 1");
    // The per-turn disabled-tools list does exactly this.
    agent.swap_tool_whitelist(Some(vec!["echo".to_string()]));
    agent
        .turn_with_context("read track 2".into(), Some(&ctx), |_| {})
        .await
        .expect("turn 2");

    let r = bodies(&server).await;
    assert_eq!(r.len(), 6, "{r:#?}");
    let (t1_step1, t2_step1) = (&r[1], &r[4]);

    assert_eq!(
        t1_step1["system"], t2_step1["system"],
        "the premise is that only the tools differ"
    );
    assert_ne!(t1_step1["tools"], t2_step1["tools"]);
    assert!(
        !has_thinking(&t2_step1["messages"]),
        "thinking was replayed under a different tool set: {}",
        t2_step1["messages"]
    );
    assert_eq!(
        shape(&t2_step1["messages"]),
        [
            "user:text",
            "assistant:text,tool_use",
            "user:tool_result",
            "assistant:text",
            "user:text"
        ]
    );
}

/// What a model that thinks by default does with the classifier's request:
/// unless thinking is disabled it reasons first, and at the cap the
/// classifier used to ask for (10) that is the whole reply, with
/// `stop_reason: max_tokens` and no text.
fn model_that_thinks_by_default(req: &wiremock::Request) -> ResponseTemplate {
    let body: Value = serde_json::from_slice(&req.body).expect("a JSON body");
    if body["stream"] == json!(true) {
        return ResponseTemplate::new(200)
            .insert_header("content-type", "text/event-stream")
            .set_body_string(sse_text("On it."));
    }
    let is_classifier = body["system"]
        .as_str()
        .is_some_and(|s| s.contains("Classify the user's request"));
    let reply = if is_classifier {
        let thinks = body["thinking"]["type"] != json!("disabled");
        let room = body["max_tokens"].as_u64().unwrap_or(0);
        if thinks && room <= 200 {
            json!({
                "content": [{ "type": "thinking", "thinking": "", "signature": "SIG-CLS" }],
                "stop_reason": "max_tokens"
            })
        } else {
            json!({
                "content": [{ "type": "text", "text": "mashup" }],
                "stop_reason": "end_turn"
            })
        }
    } else {
        // The plan request: an answer with no plan in it, so the turn
        // moves on to its first step.
        json!({
            "content": [{ "type": "text", "text": "No plan needed." }],
            "stop_reason": "end_turn"
        })
    };
    ResponseTemplate::new(200).set_body_json(reply)
}

/// A classifier that gets no text back reads as `general`, silently. On a
/// model that thinks by default that is every request, so a mashup would
/// never get the mashup prompt. The mode shows in the streaming request's
/// system prompt.
#[tokio::test]
async fn the_classifier_still_reads_a_mode_from_a_model_that_thinks_by_default() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(model_that_thinks_by_default)
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().expect("tempdir");
    let mut agent = agent_against(&server, dir.path());

    agent
        .turn("mash these two songs up".into(), |_| {})
        .await
        .expect("the turn");

    let streaming: Vec<Value> = bodies(&server)
        .await
        .into_iter()
        .filter(|b| b["stream"] == json!(true))
        .collect();
    assert_eq!(streaming.len(), 1);
    let system = streaming[0]["system"].to_string();
    assert!(
        system.contains("Mashup Mode"),
        "the classifier's answer was lost and the turn ran as general: {system}"
    );
}
