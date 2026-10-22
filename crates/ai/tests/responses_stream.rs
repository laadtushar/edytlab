//! The Responses wire format end to end, through `Agent::turn` against a
//! mocked `/v1/responses` (#474).
//!
//! No provider speaks this format yet (the ChatGPT one is a later change),
//! so the provider here is a stand-in that does what that one will: say it is
//! `WireFormat::Responses` and hand its two stream hooks to
//! [`ai::responses::ResponsesCodec`]. What is pinned is the format's stream
//! contract with the agent loop:
//!
//! * text deltas assemble into the reply, in order;
//! * a function call whose arguments arrive in pieces comes out whole, and is
//!   answered with a `function_call_output` of the same `call_id`;
//! * a turn succeeds only at `response.completed`: a stream that stops short
//!   of it, `response.failed`, `response.incomplete` and `error` all end the
//!   turn in an error that keeps the API's words;
//! * the classifier, a one-shot call, is streamed too, since the route takes
//!   nothing else.

mod common;

use std::sync::{Arc, Mutex};

use ai::anthropic::{MessagesRequest, StreamEvent};
use ai::provider::{ProviderError, WireFormat};
use ai::responses::{ResponsesCodec, ToolGrouping};
use ai::{Agent, AgentEvent, Error, LlmConfig, LlmProvider};
use common::{ok, Entry, SeqResponder};
use serde_json::{json, Value};
use tools::ToolDispatcher;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer};

const NAMESPACE: &str = "edytlab";

/// What the ChatGPT provider will be, minus the credentials.
#[derive(Debug)]
struct ResponsesProvider {
    codec: ResponsesCodec,
}

impl ResponsesProvider {
    fn new() -> Self {
        Self {
            codec: ResponsesCodec::new(ToolGrouping::Namespace(NAMESPACE.to_string())),
        }
    }
}

impl LlmProvider for ResponsesProvider {
    fn id(&self) -> &'static str {
        "responses-test"
    }
    fn base_url(&self) -> &str {
        "http://127.0.0.1:9"
    }
    fn default_model(&self) -> &str {
        "gpt-test"
    }
    fn classifier_model(&self) -> &str {
        "gpt-test-mini"
    }
    fn translate_model(&self, model: &str) -> String {
        model.to_string()
    }
    fn apply_auth(&self, req: reqwest::RequestBuilder, api_key: &str) -> reqwest::RequestBuilder {
        req.header("authorization", format!("Bearer {api_key}"))
            .header("content-type", "application/json")
    }
    fn endpoint_path(&self) -> &str {
        "/v1/responses"
    }
    fn wire_format(&self) -> WireFormat {
        WireFormat::Responses
    }
    fn serialize_request(&self, req: &MessagesRequest<'_>) -> Value {
        self.codec.serialize_request(req)
    }
    fn parse_stream_chunk(&self, raw: &str) -> Result<Vec<StreamEvent>, ProviderError> {
        self.codec.parse_stream_chunk(raw)
    }
}

// ---------------------------------------------------------------------
// The server's side: Responses events, framed as server-sent events
// ---------------------------------------------------------------------

/// `event: <type>` / `data: <json>` frames, as the API sends them.
fn sse(events: &[Value]) -> String {
    let mut out = String::new();
    for event in events {
        out.push_str("event: ");
        out.push_str(event["type"].as_str().expect("an event has a type"));
        out.push_str("\ndata: ");
        out.push_str(&serde_json::to_string(event).unwrap());
        out.push_str("\n\n");
    }
    out
}

fn created() -> Value {
    json!({
        "type": "response.created",
        "response": { "id": "resp_1", "model": "gpt-test", "status": "in_progress", "output": [] }
    })
}

fn completed() -> Value {
    json!({
        "type": "response.completed",
        "response": {
            "id": "resp_1",
            "status": "completed",
            "output": [],
            "usage": { "input_tokens": 10, "output_tokens": 5, "total_tokens": 15 }
        }
    })
}

/// A whole assistant message streamed as `pieces`, ending at `completed`.
fn text_reply(pieces: &[&str]) -> String {
    let mut events = text_events(pieces);
    events.push(completed());
    sse(&events)
}

/// The events of a streamed message, up to but not including a terminal one.
fn text_events(pieces: &[&str]) -> Vec<Value> {
    let mut events = vec![
        created(),
        json!({
            "type": "response.output_item.added",
            "output_index": 0,
            "item": { "type": "message", "id": "msg_1", "role": "assistant", "status": "in_progress", "content": [] }
        }),
        json!({
            "type": "response.content_part.added",
            "item_id": "msg_1", "output_index": 0, "content_index": 0,
            "part": { "type": "output_text", "text": "" }
        }),
    ];
    for piece in pieces {
        events.push(json!({
            "type": "response.output_text.delta",
            "item_id": "msg_1", "output_index": 0, "content_index": 0, "delta": piece
        }));
    }
    let whole = pieces.concat();
    events.push(json!({
        "type": "response.output_text.done",
        "item_id": "msg_1", "output_index": 0, "content_index": 0, "text": whole
    }));
    events.push(json!({
        "type": "response.output_item.done",
        "output_index": 0,
        "item": {
            "type": "message", "id": "msg_1", "role": "assistant", "status": "completed",
            "content": [{ "type": "output_text", "text": whole }]
        }
    }));
    events
}

/// A step that calls one function, its arguments streamed in `pieces`.
fn call_events(call_id: &str, name: &str, pieces: &[&str]) -> Vec<Value> {
    let whole = pieces.concat();
    let mut events = vec![
        created(),
        json!({
            "type": "response.output_item.added",
            "output_index": 0,
            "item": {
                "type": "function_call", "id": "fc_1", "call_id": call_id,
                "name": name, "arguments": "", "status": "in_progress"
            }
        }),
    ];
    for piece in pieces {
        events.push(json!({
            "type": "response.function_call_arguments.delta",
            "item_id": "fc_1", "output_index": 0, "delta": piece
        }));
    }
    events.push(json!({
        "type": "response.function_call_arguments.done",
        "item_id": "fc_1", "output_index": 0, "arguments": whole
    }));
    events.push(json!({
        "type": "response.output_item.done",
        "output_index": 0,
        "item": {
            "type": "function_call", "id": "fc_1", "call_id": call_id,
            "name": name, "arguments": whole, "status": "completed"
        }
    }));
    events
}

// ---------------------------------------------------------------------
// The agent's side
// ---------------------------------------------------------------------

struct Run {
    result: ai::Result<ai::TurnResult>,
    events: Vec<AgentEvent>,
    /// The bodies the server was sent, in order.
    requests: Vec<Value>,
}

/// One turn against a server that answers `/v1/responses` with `responses`,
/// in order. The classifier is the first request.
async fn run(responses: Vec<Entry>) -> Run {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(SeqResponder::new(responses))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().expect("tempdir");
    let cfg =
        LlmConfig::new(Arc::new(ResponsesProvider::new()), "token").with_base_url(server.uri());
    let mut agent = Agent::new(
        cfg,
        Arc::new(Mutex::new(ToolDispatcher::default_dispatcher())),
        Arc::new(Mutex::new(
            session::Store::open(dir.path()).expect("open store"),
        )),
        Arc::new(Mutex::new(audio_engine::Engine::new())),
        ai::PlanGate::new(),
        Arc::new(Mutex::new(None::<tools::Clipboard>)),
    );
    let mut events = Vec::new();
    let result = agent
        .turn("make the drums louder".to_string(), |e| events.push(e))
        .await;
    let requests = server
        .received_requests()
        .await
        .expect("recording")
        .iter()
        .map(|r| serde_json::from_slice(&r.body).expect("a JSON body"))
        .collect();
    Run {
        result,
        events,
        requests,
    }
}

fn classifier_says_general() -> Entry {
    ok(text_reply(&["general"]))
}

fn streamed_text(events: &[AgentEvent]) -> Vec<String> {
    events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::TextDelta(t) => Some(t.clone()),
            _ => None,
        })
        .collect()
}

/// The text of an error the turn ended in.
fn failure(run: &Run) -> String {
    match &run.result {
        Err(e) => e.to_string(),
        Ok(r) => panic!("the turn succeeded with {:?}", r.text),
    }
}

// ---------------------------------------------------------------------
// Text
// ---------------------------------------------------------------------

#[tokio::test]
async fn text_deltas_assemble_into_the_reply() {
    let run = run(vec![
        classifier_says_general(),
        ok(text_reply(&["Hello", ", ", "world", "!"])),
    ])
    .await;

    let result = run.result.as_ref().expect("the turn succeeds");
    assert_eq!(result.text, "Hello, world!");
    assert_eq!(
        streamed_text(&run.events),
        ["Hello", ", ", "world", "!"],
        "each delta reaches the caller as it arrives"
    );
    assert!(matches!(run.events.last(), Some(AgentEvent::Done)));
    assert_eq!(result.stop_reason.as_deref(), Some("end_turn"));
    assert_eq!(run.requests.len(), 2, "the classifier, then one step");

    // Both requests are in the format's shape.
    for body in &run.requests {
        assert_eq!(body["store"], json!(false));
        assert_eq!(body["stream"], json!(true));
        assert!(body["input"].is_array());
        assert!(body["instructions"].as_str().is_some_and(|s| !s.is_empty()));
        assert!(body.get("messages").is_none());
        assert!(body.get("max_tokens").is_none());
        assert!(body.get("max_completion_tokens").is_none());
    }
    // The classifier's request is a one-shot call made to stream.
    assert!(
        run.requests[0].get("tools").is_none(),
        "the classifier offers no tools"
    );
}

// ---------------------------------------------------------------------
// Function calls
// ---------------------------------------------------------------------

#[tokio::test]
async fn a_function_call_with_arguments_in_pieces_comes_out_whole_and_is_answered() {
    let args = ["{\"tra", "ck\": 0, \"d", "b\": -3.5}"];
    let mut step = call_events("call_9", "set_track_gain", &args);
    step.push(completed());

    let run = run(vec![
        classifier_says_general(),
        ok(sse(&step)),
        ok(text_reply(&["Done."])),
    ])
    .await;

    let result = run.result.as_ref().expect("the turn succeeds");
    assert_eq!(result.text, "Done.");
    assert!(
        run.events.iter().any(|e| matches!(
            e,
            AgentEvent::ToolCallStart { name, id } if name == "set_track_gain" && id == "call_9"
        )),
        "the call is announced under its call_id: {:?}",
        run.events
    );

    // The step after the tool call carries the call, whole, and its answer.
    assert_eq!(run.requests.len(), 3, "classifier, call step, answer step");
    let input = run.requests[2]["input"].as_array().expect("input");
    let call = input
        .iter()
        .find(|i| i["type"] == "function_call")
        .expect("the call is echoed back");
    assert_eq!(call["call_id"], "call_9");
    assert_eq!(call["name"], "set_track_gain");
    assert_eq!(call["namespace"], NAMESPACE);
    let echoed: Value = serde_json::from_str(call["arguments"].as_str().unwrap()).unwrap();
    assert_eq!(
        echoed,
        json!({ "track": 0, "db": -3.5 }),
        "the pieces were joined before the call was run"
    );
    let output = input
        .iter()
        .find(|i| i["type"] == "function_call_output")
        .expect("the call is answered");
    assert_eq!(output["call_id"], "call_9", "the answer names the call");
    assert!(output["output"].is_string());
    // The call comes before its answer.
    let at = |kind: &str| input.iter().position(|i| i["type"] == kind).unwrap();
    assert!(at("function_call") < at("function_call_output"));
}

/// The guide's field table puts the finished call in an `item` on
/// `arguments.done`, where its example has `arguments` on the event. A call
/// finished in the table's shape must run with its whole arguments, not the
/// prefix the pieces made.
#[tokio::test]
async fn a_call_finished_in_the_item_shape_runs_with_its_whole_arguments() {
    let whole = "{\"track\": 0, \"db\": -3.5}";
    let events = vec![
        created(),
        json!({
            "type": "response.output_item.added", "output_index": 0,
            "item": { "type": "function_call", "id": "fc_1", "call_id": "call_5",
                      "name": "set_track_gain", "arguments": "" }
        }),
        json!({ "type": "response.function_call_arguments.delta",
                "item_id": "fc_1", "output_index": 0, "delta": "{\"track\": 0, " }),
        json!({
            "type": "response.function_call_arguments.done",
            "item_id": "fc_1", "output_index": 0,
            "item": { "type": "function_call", "id": "fc_1", "call_id": "call_5",
                      "name": "set_track_gain", "arguments": whole }
        }),
        json!({
            "type": "response.output_item.done", "output_index": 0,
            "item": { "type": "function_call", "id": "fc_1", "call_id": "call_5",
                      "name": "set_track_gain", "arguments": whole }
        }),
        completed(),
    ];
    let run = run(vec![
        classifier_says_general(),
        ok(sse(&events)),
        ok(text_reply(&["Done."])),
    ])
    .await;
    run.result.as_ref().expect("the turn succeeds");

    let input = run.requests[2]["input"].as_array().expect("input");
    let call = input
        .iter()
        .find(|i| i["type"] == "function_call")
        .expect("the call is echoed back");
    let echoed: Value = serde_json::from_str(call["arguments"].as_str().unwrap()).unwrap();
    assert_eq!(echoed, json!({ "track": 0, "db": -3.5 }));
    // The call ran: an unparseable argument string would have been answered
    // as malformed JSON.
    let output = input
        .iter()
        .find(|i| i["type"] == "function_call_output")
        .expect("answered");
    assert!(
        !output["output"]
            .as_str()
            .unwrap()
            .contains("not valid JSON"),
        "{output}"
    );
}

#[tokio::test]
async fn text_and_two_calls_in_one_step_all_arrive() {
    let mut events = text_events(&["Raising both."]);
    // Second item: calls at output indexes 1 and 2.
    for (index, (call_id, name, args)) in [
        ("call_a", "set_track_gain", "{\"track\":0,\"db\":1}"),
        ("call_b", "set_track_gain", "{\"track\":1,\"db\":1}"),
    ]
    .iter()
    .enumerate()
    {
        let output_index = index + 1;
        events.push(json!({
            "type": "response.output_item.added", "output_index": output_index,
            "item": { "type": "function_call", "id": format!("fc_{index}"), "call_id": call_id, "name": name, "arguments": "" }
        }));
        let (head, tail) = args.split_at(7);
        for piece in [head, tail] {
            events.push(json!({
                "type": "response.function_call_arguments.delta",
                "item_id": format!("fc_{index}"), "output_index": output_index, "delta": piece
            }));
        }
        events.push(json!({
            "type": "response.function_call_arguments.done",
            "item_id": format!("fc_{index}"), "output_index": output_index, "arguments": args
        }));
        events.push(json!({
            "type": "response.output_item.done", "output_index": output_index,
            "item": { "type": "function_call", "id": format!("fc_{index}"), "call_id": call_id, "name": name, "arguments": args }
        }));
    }
    events.push(completed());

    let run = run(vec![
        classifier_says_general(),
        ok(sse(&events)),
        ok(text_reply(&["Done."])),
    ])
    .await;
    run.result.as_ref().expect("the turn succeeds");

    let input = run.requests[2]["input"].as_array().expect("input");
    let ids = |kind: &str| -> Vec<String> {
        input
            .iter()
            .filter(|i| i["type"] == kind)
            .map(|i| i["call_id"].as_str().unwrap().to_string())
            .collect()
    };
    assert_eq!(ids("function_call"), ["call_a", "call_b"]);
    assert_eq!(ids("function_call_output"), ["call_a", "call_b"]);
    let first: Value = input
        .iter()
        .find(|i| i["call_id"] == "call_a" && i["type"] == "function_call")
        .map(|i| serde_json::from_str(i["arguments"].as_str().unwrap()).unwrap())
        .unwrap();
    assert_eq!(first, json!({ "track": 0, "db": 1 }));
}

// ---------------------------------------------------------------------
// A turn succeeds only at `response.completed`
// ---------------------------------------------------------------------

#[tokio::test]
async fn a_stream_cut_before_completed_is_an_error() {
    // Everything but the last event: the text is all there, and the turn
    // still did not finish.
    let cut = sse(&text_events(&["Hello, ", "wor"]));
    let run = run(vec![classifier_says_general(), ok(cut)]).await;

    let message = failure(&run);
    assert!(
        message.contains("ended before the response completed"),
        "{message}"
    );
    assert!(
        matches!(run.result, Err(Error::ApiStream(_))),
        "{:?}",
        run.result
    );
}

#[tokio::test]
async fn a_stream_cut_in_the_middle_of_a_call_is_an_error_and_runs_nothing() {
    let mut events = call_events("call_1", "set_track_gain", &["{\"track\":", "0}"]);
    // Cut after the first argument piece.
    events.truncate(3);
    let run = run(vec![classifier_says_general(), ok(sse(&events))]).await;

    let message = failure(&run);
    assert!(
        message.contains("ended before the response completed"),
        "{message}"
    );
    assert_eq!(
        run.requests.len(),
        2,
        "nothing went back to the model: the call was never run"
    );
    assert!(
        !run.events
            .iter()
            .any(|e| matches!(e, AgentEvent::ToolCallEnd { .. })),
        "the call was not dispatched: {:?}",
        run.events
    );
}

#[tokio::test]
async fn an_empty_stream_is_an_error_too() {
    let run = run(vec![
        classifier_says_general(),
        ok("data: \n\n".to_string()),
    ])
    .await;
    assert!(
        failure(&run).contains("ended before the response completed"),
        "{:?}",
        run.result
    );
}

#[tokio::test]
async fn response_failed_is_an_error_that_keeps_the_apis_message() {
    let mut events = text_events(&["Starting"]);
    events.pop(); // no `output_item.done`: it died mid-reply
    events.push(json!({
        "type": "response.failed",
        "response": {
            "id": "resp_1", "status": "failed",
            "error": {
                "code": "subscription_sharing_usage_limit_exceeded",
                "message": "You have reached your ChatGPT usage limit."
            }
        }
    }));
    let run = run(vec![classifier_says_general(), ok(sse(&events))]).await;

    let message = failure(&run);
    assert!(
        message.contains("You have reached your ChatGPT usage limit."),
        "{message}"
    );
    assert!(matches!(run.result, Err(Error::ApiStream(_))));
    // What streamed before the failure was shown; the turn is still a failure.
    assert_eq!(streamed_text(&run.events), ["Starting"]);
}

#[tokio::test]
async fn response_incomplete_is_an_error_that_says_why() {
    let mut events = text_events(&["Half an"]);
    events.push(json!({
        "type": "response.incomplete",
        "response": {
            "id": "resp_1", "status": "incomplete",
            "incomplete_details": { "reason": "content_filter" }
        }
    }));
    let run = run(vec![classifier_says_general(), ok(sse(&events))]).await;

    let message = failure(&run);
    assert!(message.contains("content_filter"), "{message}");
    assert!(matches!(run.result, Err(Error::ApiStream(_))));
}

#[tokio::test]
async fn an_error_event_is_an_error_that_keeps_the_apis_message() {
    let events = vec![
        created(),
        json!({
            "type": "error",
            "code": "server_error",
            "message": "The server had an error while processing your request.",
            "param": "input"
        }),
    ];
    let run = run(vec![classifier_says_general(), ok(sse(&events))]).await;

    let message = failure(&run);
    assert!(
        message.contains("The server had an error while processing your request."),
        "{message}"
    );
    // The errors page says to use `error.param` too.
    assert!(message.contains("(param: input)"), "{message}");
}

// ---------------------------------------------------------------------
// The classifier is a one-shot call and is streamed
// ---------------------------------------------------------------------

/// A classifier whose stream fails is no reason to fail the turn: the mode
/// is `general`, as on every other format. The turn's own request says which
/// mode it ran in: it is the one a classifier that answered `general` gets,
/// instruction for instruction, and a mashup would have asked for a plan
/// first.
#[tokio::test]
async fn a_classifier_stream_that_fails_leaves_the_turn_on_general() {
    let general = run(vec![classifier_says_general(), ok(text_reply(&["Fine."]))]).await;
    let failed = sse(&[
        created(),
        json!({
            "type": "response.failed",
            "response": { "error": { "code": "server_error", "message": "boom" } }
        }),
    ]);
    let run = run(vec![ok(failed), ok(text_reply(&["Fine."]))]).await;

    assert_eq!(run.result.as_ref().expect("the turn goes on").text, "Fine.");
    assert_eq!(run.requests[0]["stream"], json!(true));
    assert_eq!(
        run.requests.len(),
        2,
        "the classifier, then the step: no plan"
    );
    assert_eq!(
        run.requests[1]["instructions"], general.requests[1]["instructions"],
        "the general prompt, as when the classifier says general"
    );
}
