//! Helpers shared by the integration tests that drive `ai::Agent` against
//! a mocked provider.
//!
//! The mock speaks Anthropic's Messages API, or, through the `chat_*`
//! helpers, the chat-completions API that Ollama, Groq, Gemini and OpenAI
//! speak. A turn makes, in order: one non-streaming call to classify the
//! request, optionally one more for a plan, then one streaming call per
//! model step. [`SeqResponder`] answers them in that order, and can answer
//! one of them with an error status.

// Each test binary uses a subset.
#![allow(dead_code)]

use std::sync::Mutex;

use serde_json::{json, Value};
use wiremock::{Respond, ResponseTemplate};

/// Server-sent-events framing for `(event name, data)` pairs.
pub fn encode_sse(events: &[(&str, Value)]) -> String {
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

fn message_start() -> (&'static str, Value) {
    (
        "message_start",
        json!({
            "type": "message_start",
            "message": {
                "id": "msg_test",
                "type": "message",
                "role": "assistant",
                "content": [],
                "model": "claude-sonnet-4-6",
                "stop_reason": null,
                "stop_sequence": null,
                "usage": { "input_tokens": 1, "output_tokens": 0 }
            }
        }),
    )
}

fn text_block(index: usize, text: &str) -> Vec<(&'static str, Value)> {
    vec![
        (
            "content_block_start",
            json!({
                "type": "content_block_start",
                "index": index,
                "content_block": { "type": "text", "text": "" }
            }),
        ),
        (
            "content_block_delta",
            json!({
                "type": "content_block_delta",
                "index": index,
                "delta": { "type": "text_delta", "text": text }
            }),
        ),
        (
            "content_block_stop",
            json!({ "type": "content_block_stop", "index": index }),
        ),
    ]
}

fn finish(stop_reason: &str) -> Vec<(&'static str, Value)> {
    vec![
        (
            "message_delta",
            json!({
                "type": "message_delta",
                "delta": { "stop_reason": stop_reason, "stop_sequence": null },
                "usage": { "output_tokens": 1 }
            }),
        ),
        ("message_stop", json!({ "type": "message_stop" })),
    ]
}

/// A streamed model step that only speaks, and ends the turn.
pub fn sse_text(text: &str) -> String {
    let mut events = vec![message_start()];
    events.extend(text_block(0, text));
    events.extend(finish("end_turn"));
    encode_sse(&events)
}

/// A streamed model step that says a sentence and then makes one tool
/// call per `(id, name, arguments as JSON text)`.
pub fn sse_tool_step(calls: &[(&str, &str, &str)]) -> String {
    let mut events = vec![message_start()];
    events.extend(text_block(0, "Working on it."));
    for (i, (id, name, args_json)) in calls.iter().enumerate() {
        let index = i + 1;
        events.push((
            "content_block_start",
            json!({
                "type": "content_block_start",
                "index": index,
                "content_block": { "type": "tool_use", "id": id, "name": name, "input": {} }
            }),
        ));
        if !args_json.is_empty() {
            events.push((
                "content_block_delta",
                json!({
                    "type": "content_block_delta",
                    "index": index,
                    "delta": { "type": "input_json_delta", "partial_json": args_json }
                }),
            ));
        }
        events.push((
            "content_block_stop",
            json!({ "type": "content_block_stop", "index": index }),
        ));
    }
    events.extend(finish("tool_use"));
    encode_sse(&events)
}

/// A `thinking` block as the stream delivers it: opened empty, its text
/// in a delta, its signature in another.
fn thinking_block(index: usize, text: &str, signature: &str) -> Vec<(&'static str, Value)> {
    vec![
        (
            "content_block_start",
            json!({
                "type": "content_block_start",
                "index": index,
                "content_block": { "type": "thinking", "thinking": "", "signature": "" }
            }),
        ),
        (
            "content_block_delta",
            json!({
                "type": "content_block_delta",
                "index": index,
                "delta": { "type": "thinking_delta", "thinking": text }
            }),
        ),
        (
            "content_block_delta",
            json!({
                "type": "content_block_delta",
                "index": index,
                "delta": { "type": "signature_delta", "signature": signature }
            }),
        ),
        (
            "content_block_stop",
            json!({ "type": "content_block_stop", "index": index }),
        ),
    ]
}

/// A `redacted_thinking` block, which arrives whole.
fn redacted_block(index: usize, data: &str) -> Vec<(&'static str, Value)> {
    vec![
        (
            "content_block_start",
            json!({
                "type": "content_block_start",
                "index": index,
                "content_block": { "type": "redacted_thinking", "data": data }
            }),
        ),
        (
            "content_block_stop",
            json!({ "type": "content_block_stop", "index": index }),
        ),
    ]
}

/// A model step on a model that thinks by default, ending in one tool
/// call `(id, name, arguments as JSON text)`: a thinking block signed
/// `signature`, a redacted one carrying `redacted`, a sentence, the call.
pub fn sse_thinking_tool_step(signature: &str, redacted: &str, call: (&str, &str, &str)) -> String {
    let (id, name, args_json) = call;
    let mut events = vec![message_start()];
    events.extend(thinking_block(0, "Reasoning before the call.", signature));
    events.extend(redacted_block(1, redacted));
    events.extend(text_block(2, "Working on it."));
    events.push((
        "content_block_start",
        json!({
            "type": "content_block_start",
            "index": 3,
            "content_block": { "type": "tool_use", "id": id, "name": name, "input": {} }
        }),
    ));
    events.push((
        "content_block_delta",
        json!({
            "type": "content_block_delta",
            "index": 3,
            "delta": { "type": "input_json_delta", "partial_json": args_json }
        }),
    ));
    events.push((
        "content_block_stop",
        json!({ "type": "content_block_stop", "index": 3 }),
    ));
    events.extend(finish("tool_use"));
    encode_sse(&events)
}

/// A model step on a model that thinks by default, ending the turn: a
/// thinking block signed `signature`, then `text`.
pub fn sse_thinking_text(signature: &str, text: &str) -> String {
    let mut events = vec![message_start()];
    events.extend(thinking_block(0, "Reasoning before the answer.", signature));
    events.extend(text_block(1, text));
    events.extend(finish("end_turn"));
    encode_sse(&events)
}

/// A non-streaming reply whose text is `text`.
fn reply_json(text: &str) -> String {
    json!({
        "id": "msg_one_shot",
        "type": "message",
        "role": "assistant",
        "model": "claude-haiku-4-5-20251001",
        "stop_reason": "end_turn",
        "content": [{ "type": "text", "text": text }],
        "usage": { "input_tokens": 1, "output_tokens": 1 }
    })
    .to_string()
}

/// The classifier's answer: `general`, `mashup`, `mix` or `voice`.
pub fn classifier_json(word: &str) -> String {
    reply_json(word)
}

/// The plan request's answer. `text` is the whole reply, so a test can
/// give one with a `<plan>` block, without one, or with an empty one.
pub fn plan_json(text: &str) -> String {
    reply_json(text)
}

/// A chat-completions non-streaming reply whose text is `text`: what a
/// chat-completions provider answers the classifier and the plan with.
pub fn chat_reply_json(text: &str) -> String {
    json!({
        "id": "chatcmpl-one-shot",
        "object": "chat.completion",
        "choices": [{
            "index": 0,
            "message": { "role": "assistant", "content": text },
            "finish_reason": "stop"
        }]
    })
    .to_string()
}

/// One chat-completions stream chunk, framed as server-sent events.
fn chat_chunk(delta: Value, finish_reason: Value) -> String {
    let chunk = json!({
        "id": "chatcmpl-test",
        "object": "chat.completion.chunk",
        "choices": [{ "index": 0, "delta": delta, "finish_reason": finish_reason }]
    });
    format!("data: {}\n\n", serde_json::to_string(&chunk).unwrap())
}

/// A chat-completions streamed step that only speaks, and ends the turn.
pub fn chat_sse_text(text: &str) -> String {
    let mut out = chat_chunk(json!({ "role": "assistant", "content": text }), Value::Null);
    out.push_str(&chat_chunk(json!({}), json!("stop")));
    out.push_str("data: [DONE]\n\n");
    out
}

/// A chat-completions streamed step that makes one tool call.
pub fn chat_sse_tool_call(id: &str, name: &str, args_json: &str) -> String {
    let mut out = chat_chunk(
        json!({
            "role": "assistant",
            "tool_calls": [{
                "index": 0,
                "id": id,
                "type": "function",
                "function": { "name": name, "arguments": args_json }
            }]
        }),
        Value::Null,
    );
    out.push_str(&chat_chunk(json!({}), json!("tool_calls")));
    out.push_str("data: [DONE]\n\n");
    out
}

/// A chat-completions streamed step that says a sentence and then makes
/// one tool call per `(id, name, arguments as JSON text)`, in the same
/// message. Each call's arguments arrive in two fragments, the way a real
/// stream splits them.
pub fn chat_sse_tool_step(calls: &[(&str, &str, &str)]) -> String {
    let mut out = chat_chunk(
        json!({ "role": "assistant", "content": "Working on it." }),
        Value::Null,
    );
    for (index, (id, name, args_json)) in calls.iter().enumerate() {
        let cut = args_json
            .char_indices()
            .nth(args_json.chars().count() / 2)
            .map_or(0, |(i, _)| i);
        let (head, tail) = args_json.split_at(cut);
        out.push_str(&chat_chunk(
            json!({ "tool_calls": [{
                "index": index,
                "id": id,
                "type": "function",
                "function": { "name": name, "arguments": head }
            }] }),
            Value::Null,
        ));
        if !tail.is_empty() {
            out.push_str(&chat_chunk(
                json!({ "tool_calls": [{
                    "index": index,
                    "function": { "arguments": tail }
                }] }),
                Value::Null,
            ));
        }
    }
    out.push_str(&chat_chunk(json!({}), json!("tool_calls")));
    out.push_str("data: [DONE]\n\n");
    out
}

/// One scripted answer: an HTTP status and a body.
pub type Entry = (u16, String);

/// A `200` answer.
pub fn ok(body: String) -> Entry {
    (200, body)
}

/// Answers the i-th request with `responses[i]`, then `500`s.
///
/// Wiremock's built-in matchers have no notion of order, so this rolls
/// its own. A body that starts with `{` is JSON, one that starts with
/// `event:` or `data:` is an event stream, anything else is plain text,
/// which is what a provider answering with an error page looks like.
pub struct SeqResponder {
    counter: Mutex<usize>,
    responses: Vec<Entry>,
}

impl SeqResponder {
    pub fn new(responses: Vec<Entry>) -> Self {
        Self {
            counter: Mutex::new(0),
            responses,
        }
    }
}

impl Respond for SeqResponder {
    fn respond(&self, _req: &wiremock::Request) -> ResponseTemplate {
        let mut counter = self.counter.lock().unwrap();
        let idx = *counter;
        *counter += 1;
        match self.responses.get(idx) {
            Some((status, body)) => {
                let content_type = if body.trim_start().starts_with('{') {
                    "application/json"
                } else if body.trim_start().starts_with("event:")
                    || body.trim_start().starts_with("data:")
                {
                    "text/event-stream"
                } else {
                    "text/plain"
                };
                ResponseTemplate::new(*status)
                    .insert_header("content-type", content_type)
                    .set_body_string(body.clone())
            }
            None => ResponseTemplate::new(500).set_body_string("no more responses"),
        }
    }
}
