//! Helpers shared by the integration tests that drive `ai::Agent` against
//! a mocked provider.
//!
//! The mock speaks Anthropic's Messages API. A turn makes, in order: one
//! non-streaming call to classify the request, optionally one more for a
//! plan, then one streaming call per model step. [`SeqResponder`] answers
//! them in that order, and can answer one of them with an error status.

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
/// `event:` is an event stream, anything else is plain text, which is
/// what a provider answering with an error page looks like.
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
                } else if body.trim_start().starts_with("event:") {
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
