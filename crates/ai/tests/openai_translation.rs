//! End-to-end OpenAI translation: round-trip request shape + stream
//! parsing. These tests pin the wire-format contract so accidental
//! regressions in the chat-completions translator surface early.
//!
//! There are two halves:
//!
//! 1. **Serialization** — build a canonical [`MessagesRequest`] with a
//!    system prompt, a user turn, an assistant turn that mixes text +
//!    tool_use blocks, and a follow-up user turn carrying tool_result
//!    blocks. Confirm the serialised body is a valid OpenAI
//!    chat-completions request: `tool_calls` on the assistant message,
//!    `role: "tool"` for the result, `max_completion_tokens` (not
//!    `max_tokens`), `tools` translated from `input_schema`.
//!
//! 2. **Stream parsing** — feed a sequence of OpenAI SSE chunk JSONs
//!    (representative of a real tool-calling round-trip) through
//!    `parse_stream_chunk` and assert the canonical [`StreamEvent`]
//!    sequence reconstructs to what the agent loop expects.

use ai::anthropic::{
    ContentBlock, ContentBlockDelta, ContentBlockStart, Message, MessagesRequest, Role,
    StreamEvent, SystemBlock,
};
use ai::provider::{LlmProvider, OpenAIProvider};
use serde_json::{json, Value};

#[test]
fn round_trip_system_user_assistant_tool_result() {
    let msgs: Vec<Message> = vec![
        Message {
            role: Role::User,
            content: vec![ContentBlock::Text {
                text: "make a mashup of these two tracks".into(),
            }],
        },
        Message {
            role: Role::Assistant,
            content: vec![
                ContentBlock::Text {
                    text: "Sure, analysing.".into(),
                },
                ContentBlock::ToolUse {
                    id: "call_abc".into(),
                    name: "analyze_track".into(),
                    input: json!({"path": "/a.wav"}),
                },
            ],
        },
        Message {
            role: Role::User,
            content: vec![ContentBlock::ToolResult {
                tool_use_id: "call_abc".into(),
                content: "{\"bpm\": 128}".into(),
                is_error: None,
            }],
        },
    ];

    let req = MessagesRequest {
        model: "gpt-4o-mini",
        max_tokens: 4096,
        system: vec![
            SystemBlock {
                kind: "text",
                text: "You are a DAW.",
                cache_control: None,
            },
            SystemBlock {
                kind: "text",
                text: "Always use tools.",
                cache_control: None,
            },
        ],
        messages: &msgs,
        tools: Some(json!([{
            "name": "analyze_track",
            "description": "Analyse a track and return BPM/key.",
            "input_schema": {
                "type": "object",
                "properties": {"path": {"type": "string"}},
                "required": ["path"]
            }
        }])),
        tool_choice: None,
        stream: true,
    };

    let p = OpenAIProvider::default();
    let body = p.serialize_request(&req);

    // Top-level chat-completions schema.
    assert_eq!(body["model"], "gpt-4o-mini");
    assert_eq!(body["stream"], true);
    assert_eq!(
        body["max_completion_tokens"], 4096,
        "OpenAI uses max_completion_tokens not max_tokens"
    );
    assert!(
        body.get("max_tokens").is_none(),
        "max_tokens must NOT be sent on chat-completions"
    );

    // System blocks concatenated into ONE system message.
    assert_eq!(body["messages"][0]["role"], "system");
    assert!(body["messages"][0]["content"]
        .as_str()
        .unwrap()
        .contains("You are a DAW."));
    assert!(body["messages"][0]["content"]
        .as_str()
        .unwrap()
        .contains("Always use tools."));

    // User turn -> {role:user, content:text}
    assert_eq!(body["messages"][1]["role"], "user");
    assert_eq!(
        body["messages"][1]["content"],
        "make a mashup of these two tracks"
    );

    // Assistant turn -> {role:assistant, content:text, tool_calls:[...]}
    assert_eq!(body["messages"][2]["role"], "assistant");
    assert_eq!(body["messages"][2]["content"], "Sure, analysing.");
    let tcs = body["messages"][2]["tool_calls"].as_array().unwrap();
    assert_eq!(tcs.len(), 1);
    assert_eq!(tcs[0]["id"], "call_abc");
    assert_eq!(tcs[0]["type"], "function");
    assert_eq!(tcs[0]["function"]["name"], "analyze_track");
    let args: Value = serde_json::from_str(tcs[0]["function"]["arguments"].as_str().unwrap())
        .expect("arguments must be a stringified JSON object");
    assert_eq!(args["path"], "/a.wav");

    // Tool result -> {role:tool, tool_call_id, content}
    assert_eq!(body["messages"][3]["role"], "tool");
    assert_eq!(body["messages"][3]["tool_call_id"], "call_abc");
    assert!(body["messages"][3]["content"]
        .as_str()
        .unwrap()
        .contains("128"));

    // Tools translated to function shape with parameters from input_schema.
    let tools = body["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0]["type"], "function");
    assert_eq!(tools[0]["function"]["name"], "analyze_track");
    assert_eq!(tools[0]["function"]["parameters"]["type"], "object");
    assert_eq!(body["tool_choice"], "auto");
}

#[test]
fn round_trip_user_with_tool_result_emits_tool_role_message() {
    // A user message that ONLY carries tool_result blocks must produce
    // a `role: "tool"` message — not a `role: "user"` with empty content.
    let msgs: Vec<Message> = vec![Message {
        role: Role::User,
        content: vec![ContentBlock::ToolResult {
            tool_use_id: "call_1".into(),
            content: "ok".into(),
            is_error: None,
        }],
    }];
    let req = MessagesRequest {
        model: "gpt-4o-mini",
        max_tokens: 1,
        system: vec![],
        messages: &msgs,
        tools: None,
        tool_choice: None,
        stream: false,
    };
    let p = OpenAIProvider::default();
    let body = p.serialize_request(&req);
    let messages = body["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["role"], "tool");
    assert_eq!(messages[0]["tool_call_id"], "call_1");
}

#[test]
fn stream_parsing_reconstructs_text_only_response() {
    // Representative SSE chunks for a plain text response with three
    // text deltas and a stop.
    let p = OpenAIProvider::default();
    let chunks = [
        r#"{"id":"c1","model":"gpt-4o-mini","choices":[{"index":0,"delta":{"role":"assistant","content":"Hello"}}]}"#,
        r#"{"id":"c1","model":"gpt-4o-mini","choices":[{"index":0,"delta":{"content":", "}}]}"#,
        r#"{"id":"c1","model":"gpt-4o-mini","choices":[{"index":0,"delta":{"content":"world"}}]}"#,
        r#"{"id":"c1","model":"gpt-4o-mini","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}"#,
    ];

    let mut events = Vec::new();
    for c in chunks {
        events.extend(p.parse_stream_chunk(c).unwrap());
    }

    // Reconstruct the streamed text from TextDelta events only.
    let text: String = events
        .iter()
        .filter_map(|e| match e {
            StreamEvent::ContentBlockDelta {
                delta: ContentBlockDelta::TextDelta { text },
                ..
            } => Some(text.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(text, "Hello, world");

    // Sequence ends with MessageStop and the canonical stop_reason maps.
    assert!(matches!(events.last().unwrap(), StreamEvent::MessageStop));
    let stop = events
        .iter()
        .find_map(|e| match e {
            StreamEvent::MessageDelta { delta } => delta.stop_reason.clone(),
            _ => None,
        })
        .unwrap();
    assert_eq!(stop, "end_turn");
}

#[test]
fn stream_parsing_reconstructs_tool_use_response() {
    // Represents OpenAI streaming a single tool call: id+name on the
    // first delta, arguments fragmented across three deltas, finishing
    // with finish_reason=tool_calls.
    let p = OpenAIProvider::default();
    let chunks = [
        r#"{"id":"c2","model":"gpt-4o-mini","choices":[{"index":0,"delta":{"role":"assistant","tool_calls":[{"index":0,"id":"call_z","type":"function","function":{"name":"analyze_track","arguments":""}}]}}]}"#,
        r#"{"id":"c2","model":"gpt-4o-mini","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"path\":"}}]}}]}"#,
        r#"{"id":"c2","model":"gpt-4o-mini","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"/a.wav\"}"}}]}}]}"#,
        r#"{"id":"c2","model":"gpt-4o-mini","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}"#,
    ];

    let mut events = Vec::new();
    for c in chunks {
        events.extend(p.parse_stream_chunk(c).unwrap());
    }

    // Find the tool_use ContentBlockStart.
    let (id, name) = events
        .iter()
        .find_map(|e| match e {
            StreamEvent::ContentBlockStart {
                content_block: ContentBlockStart::ToolUse { id, name, .. },
                ..
            } => Some((id.clone(), name.clone())),
            _ => None,
        })
        .expect("tool_use block start");
    assert_eq!(id, "call_z");
    assert_eq!(name, "analyze_track");

    // Concat all input_json_delta fragments and confirm valid JSON.
    let args: String = events
        .iter()
        .filter_map(|e| match e {
            StreamEvent::ContentBlockDelta {
                delta: ContentBlockDelta::InputJsonDelta { partial_json },
                ..
            } => Some(partial_json.clone()),
            _ => None,
        })
        .collect();
    let parsed: Value = serde_json::from_str(&args).unwrap();
    assert_eq!(parsed["path"], "/a.wav");

    // Stop reason canonicalised to tool_use.
    let stop = events
        .iter()
        .find_map(|e| match e {
            StreamEvent::MessageDelta { delta } => delta.stop_reason.clone(),
            _ => None,
        })
        .unwrap();
    assert_eq!(stop, "tool_use");
}

#[test]
fn stream_parsing_done_sentinel_yields_message_stop() {
    let p = OpenAIProvider::default();
    let events = p.parse_stream_chunk("[DONE]").unwrap();
    assert_eq!(events.len(), 1);
    assert!(matches!(events[0], StreamEvent::MessageStop));
}

#[test]
fn stream_parsing_surfaces_error_envelopes() {
    let p = OpenAIProvider::default();
    let events = p
        .parse_stream_chunk(
            r#"{"error":{"message":"context length exceeded","type":"invalid_request_error"}}"#,
        )
        .unwrap();
    assert!(matches!(events[0], StreamEvent::Error { .. }));
}
