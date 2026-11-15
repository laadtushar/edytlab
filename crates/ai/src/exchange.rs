//! The model exchange: one step of a turn, as a port with two adapters
//! (ADR 0007).
//!
//! A turn's policy (the plan gate, the tool budget, the held step, dispatch)
//! needs one thing from a model: send this request, give back what the model
//! said. [`ModelExchange::step`] is that, and everything about how the words
//! travel (the HTTP request, the provider's wire format, the server-sent
//! events, a failed status) is behind it.
//!
//! * [`HttpExchange`] is the real adapter. It wraps what `run_turn` used to
//!   do inline: serialise the request through the provider, post it, read the
//!   stream into a [`Step`]. The bytes on the wire are the same ones.
//! * [`ScriptedExchange`] answers from a list the test wrote, with no HTTP and
//!   no SSE, so a policy rule is tested with plain steps.
//!
//! What is *not* behind the port yet: the one-shot requests (the classifier
//! and the plan), which still match on [`crate::WireFormat`] in `agent_loop`.
//! Moving them into per-wire codecs is the next stage of ADR 0007.
//!
//! The history is not the port's business. A step comes back, and the loop
//! writes it through [`crate::conversation::Conversation`].

use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;
use std::time::Duration;

use eventsource_stream::Eventsource;
use futures_util::StreamExt;
use serde_json::Value;

use crate::agent_loop::error_message;
use crate::anthropic::{ContentBlockDelta, ContentBlockStart, MessagesRequest, StreamEvent};
use crate::provider::{Adaptation, ProviderFailure};
use crate::{context_window, responses, AgentEvent, Error, LlmConfig, Result};

/// Hard upper bound on the number of content blocks we'll allocate for a
/// single streaming message. The server tells us each block's index; without
/// this cap a malicious or buggy server could hand us `u64::MAX` and force a
/// massive `Vec` allocation. Real tool-use messages have well under 10
/// blocks, so 100 is generous.
const MAX_CONTENT_BLOCKS: usize = 100;

/// One block of a reply, as the stream built it. The index in a step's block
/// list is the streaming server's `index`.
#[derive(Debug, Clone, PartialEq)]
pub enum StepBlock {
    /// A block was opened for an index nothing started: later deltas for it
    /// are dropped.
    Pending,
    /// A block of a kind this crate does not model.
    Ignored,
    Text(String),
    ToolUse {
        id: String,
        name: String,
        /// The arguments as the model streamed them, which may not parse.
        args_json: String,
    },
    /// Streamed as thinking deltas and one signature delta. Kept for the
    /// history, never shown.
    Thinking {
        thinking: String,
        signature: String,
    },
    RedactedThinking(String),
}

/// What the model said in one round trip.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Step {
    pub blocks: Vec<StepBlock>,
    pub stop_reason: Option<String>,
    /// The step's text blocks joined, as the caller was shown them.
    pub text: String,
}

impl Step {
    /// A step that says `text` and stops.
    pub fn say(text: &str) -> Self {
        Self {
            blocks: vec![StepBlock::Text(text.to_string())],
            stop_reason: Some("end_turn".to_string()),
            text: text.to_string(),
        }
    }

    /// A step that calls one tool and waits for the result.
    pub fn call(id: &str, name: &str, args: &Value) -> Self {
        Self::calls(&[(id, name, args)])
    }

    /// A step that calls several tools.
    pub fn calls(calls: &[(&str, &str, &Value)]) -> Self {
        Self {
            blocks: calls
                .iter()
                .map(|(id, name, args)| StepBlock::ToolUse {
                    id: (*id).to_string(),
                    name: (*name).to_string(),
                    args_json: args.to_string(),
                })
                .collect(),
            stop_reason: Some("tool_use".to_string()),
            text: String::new(),
        }
    }
}

/// A request the server answered with a failure status.
#[derive(Debug, Clone, PartialEq)]
pub struct Refusal {
    pub status: u16,
    pub body: String,
    /// The request id the route reported, which its support asks for.
    pub request_id: Option<String>,
}

impl Refusal {
    fn as_failure(&self) -> ProviderFailure<'_> {
        ProviderFailure::Http {
            status: self.status,
            body: &self.body,
            request_id: self.request_id.as_deref(),
        }
    }
}

/// How a round trip came out.
#[derive(Debug)]
pub enum Exchanged {
    /// The model answered.
    Step(Step),
    /// The server refused the request. The loop may change the request and
    /// ask again ([`ModelExchange::adapt`]); if it does not,
    /// [`ModelExchange::refusal_error`] names the failure.
    Refused(Refusal),
    /// The stream failed in a way worth one more try, before any of the step
    /// was shown or run. Carries the error to end the turn with when the try
    /// has already been spent.
    Interrupted(Error),
}

/// Where a step reports the text and tool calls as they stream in. A channel
/// rather than a callback, so the callback the caller holds need not be
/// `Send`.
pub type EventSink = tokio::sync::mpsc::UnboundedSender<AgentEvent>;

/// A boxed future, so the port is object safe.
pub type ExchangeFuture<'a> = Pin<Box<dyn Future<Output = Result<Exchanged>> + Send + 'a>>;

/// The seam between a turn's policy and the model.
pub trait ModelExchange: Send + Sync {
    /// The JSON the request goes out as. The loop compares two of them to see
    /// that a refused request was really changed.
    fn request_body(&self, request: &MessagesRequest<'_>) -> Value;

    /// Send `body`, a request as [`Self::request_body`] serialised it (once,
    /// by the caller), and read the reply, sending `events` for the text and
    /// tool calls as they arrive, in order. Use [`step_streaming`] to call it
    /// with a plain callback.
    fn step<'a>(&'a self, body: &'a Value, events: EventSink) -> ExchangeFuture<'a>;

    /// Whether the exchange can ask for something else after `refusal`
    /// (the ChatGPT plan's ladder of tool layouts). If it answers, the same
    /// request built again is a different one.
    fn adapt(&self, refusal: &Refusal) -> Option<Adaptation>;

    /// The error a refusal ends the turn with, once nothing more can be
    /// tried.
    fn refusal_error(&self, refusal: &Refusal) -> Error;

    /// Whether the model's thinking blocks go back into the history. Only
    /// for a provider whose server issued them.
    fn replays_thinking(&self) -> bool;

    /// How long to wait before the second try of an interrupted step.
    fn retry_delay(&self) -> Duration;
}

/// Run one step, handing each event to `on_event` as it arrives and in
/// order, before the step's result is returned.
pub(crate) async fn step_streaming(
    exchange: &dyn ModelExchange,
    body: &Value,
    on_event: &mut impl FnMut(AgentEvent),
) -> Result<Exchanged> {
    let (sink, mut events) = tokio::sync::mpsc::unbounded_channel();
    let mut step = exchange.step(body, sink);
    let exchanged = loop {
        tokio::select! {
            biased;
            Some(event) = events.recv() => on_event(event),
            exchanged = &mut step => break exchanged,
        }
    };
    // The step is over and its sender gone: what it sent last is still here.
    while let Ok(event) = events.try_recv() {
        on_event(event);
    }
    exchanged
}

fn grow_to(v: &mut Vec<StepBlock>, idx: usize) -> Result<()> {
    if idx >= MAX_CONTENT_BLOCKS {
        return Err(Error::Protocol(format!(
            "content_block_index {idx} exceeds MAX_CONTENT_BLOCKS ({MAX_CONTENT_BLOCKS})"
        )));
    }
    while v.len() <= idx {
        v.push(StepBlock::Pending);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// The HTTP adapter
// ---------------------------------------------------------------------------

/// The real model: the provider's endpoint, over HTTP, streamed.
pub struct HttpExchange<'a> {
    cfg: &'a LlmConfig,
    http: &'a reqwest::Client,
}

impl<'a> HttpExchange<'a> {
    pub fn new(cfg: &'a LlmConfig, http: &'a reqwest::Client) -> Self {
        Self { cfg, http }
    }

    async fn exchange(&self, body: &Value, events: EventSink) -> Result<Exchanged> {
        let cfg = self.cfg;
        let resp = cfg.post_json(self.http, body).await?;

        if !resp.status().is_success() {
            let status = resp.status();
            // The route's errors page asks for the request ID to be kept with
            // the status and the code: it is what OpenAI is asked about.
            let request_id = responses::request_id_of(resp.headers());
            let body = resp.text().await.unwrap_or_default();
            return Ok(Exchanged::Refused(Refusal {
                status: status.as_u16(),
                body,
                request_id,
            }));
        }

        let stream_request_id = responses::request_id_of(resp.headers());
        let stream = resp
            .bytes_stream()
            .map(|chunk| chunk.map_err(std::io::Error::other));
        let mut sse = stream.eventsource();

        // Per-message accumulators, indexed by content-block index.
        let mut blocks: Vec<StepBlock> = Vec::new();
        let mut stop_reason: Option<String> = None;
        let mut text_this_message = String::new();
        // Flag flipped by `MessageStop` inside the (provider-translated)
        // event sequence so we can break out of the SSE loop cleanly.
        let mut stream_finished = false;

        while let Some(ev) = sse.next().await {
            let event = ev.map_err(Error::Sse)?;
            // `eventsource-stream` skips comment lines and reassembles
            // multi-line `data:` payloads automatically. An empty data
            // payload (server-side keepalive) is ignored.
            if event.data.is_empty() {
                continue;
            }
            // Provider-specific stream parsing. Anthropic + OpenRouter
            // deserialise the chunk straight into `StreamEvent`; OpenAI
            // translates a chat-completions delta into one or more
            // canonical events.
            let parsed_events = cfg
                .provider
                .parse_stream_chunk(&event.data)
                .map_err(|e| Error::Protocol(e.to_string()))?;
            for parsed in parsed_events {
                match parsed {
                    StreamEvent::MessageStart { .. } => {
                        blocks.clear();
                        text_this_message.clear();
                    }
                    StreamEvent::ContentBlockStart {
                        index,
                        content_block,
                    } => {
                        grow_to(&mut blocks, index as usize)?;
                        match content_block {
                            ContentBlockStart::Text { .. } => {
                                blocks[index as usize] = StepBlock::Text(String::new());
                            }
                            ContentBlockStart::ToolUse { id, name, .. } => {
                                let _ = events.send(AgentEvent::ToolCallStart {
                                    name: name.clone(),
                                    id: id.clone(),
                                });
                                blocks[index as usize] = StepBlock::ToolUse {
                                    id,
                                    name,
                                    args_json: String::new(),
                                };
                            }
                            ContentBlockStart::Thinking {
                                thinking,
                                signature,
                            } => {
                                blocks[index as usize] = StepBlock::Thinking {
                                    thinking,
                                    signature,
                                };
                            }
                            ContentBlockStart::RedactedThinking { data } => {
                                blocks[index as usize] = StepBlock::RedactedThinking(data);
                            }
                            ContentBlockStart::Other => {
                                blocks[index as usize] = StepBlock::Ignored;
                            }
                        }
                    }
                    StreamEvent::ContentBlockDelta { index, delta } => {
                        let slot = blocks.get_mut(index as usize).ok_or_else(|| {
                            Error::Protocol("delta for unknown block index".into())
                        })?;
                        match (slot, delta) {
                            (StepBlock::Text(buf), ContentBlockDelta::TextDelta { text }) => {
                                buf.push_str(&text);
                                text_this_message.push_str(&text);
                                let _ = events.send(AgentEvent::TextDelta(text));
                            }
                            (
                                StepBlock::ToolUse { args_json, .. },
                                ContentBlockDelta::InputJsonDelta { partial_json },
                            ) => {
                                args_json.push_str(&partial_json);
                            }
                            (
                                StepBlock::Thinking { thinking, .. },
                                ContentBlockDelta::ThinkingDelta { thinking: more },
                            ) => thinking.push_str(&more),
                            (
                                StepBlock::Thinking { signature, .. },
                                ContentBlockDelta::SignatureDelta { signature: more },
                            ) => signature.push_str(&more),
                            // Mismatched delta kind for the block — ignore;
                            // the server occasionally emits unrelated deltas
                            // we don't model yet.
                            _ => {}
                        }
                    }
                    StreamEvent::ContentBlockStop { .. } => {
                        // Nothing to do; we'll consume `blocks` after stop.
                    }
                    StreamEvent::MessageDelta { delta } => {
                        if let Some(reason) = delta.stop_reason {
                            stop_reason = Some(reason);
                        }
                    }
                    StreamEvent::MessageStop => {
                        stream_finished = true;
                        break;
                    }
                    StreamEvent::Ping | StreamEvent::Other => {}
                    StreamEvent::Error { error } => {
                        // The error's `kind` (on the Responses format, the
                        // API's code, such as
                        // `subscription_sharing_usage_limit_exceeded`) and
                        // `param` go to the provider first: one whose codes
                        // call for different handling names the failure,
                        // and nothing is tried on another provider. Any
                        // other keeps only the message.
                        let failure = cfg
                            .provider
                            .classify_failure(&ProviderFailure::Stream {
                                kind: &error.kind,
                                message: &error.message,
                                param: error.param.as_deref(),
                                request_id: stream_request_id.as_deref(),
                            })
                            .unwrap_or_else(|| Error::ApiStream(error_message(&error)));
                        // A transient failure the provider says is worth one
                        // more try, before any of the step has been shown or
                        // run (a retry after text went out would say it
                        // twice). Whether the try is still allowed is the
                        // loop's to say. Each try spends the person's
                        // allowance, so only the codes that blame the
                        // service qualify.
                        if blocks.is_empty() && cfg.provider.retries_stream_failure(&error.kind) {
                            return Ok(Exchanged::Interrupted(failure));
                        }
                        return Err(failure);
                    }
                }
            }
            if stream_finished {
                break;
            }
        }
        // A stream that ran out without its end marker. The Responses
        // format succeeds only at `response.completed`; a step cut short is
        // an error, not a shorter reply, and nothing of it is kept or run.
        if !stream_finished && cfg.provider.wire_format().needs_terminal_event() {
            return Err(responses::cut_off_error());
        }
        Ok(Exchanged::Step(Step {
            blocks,
            stop_reason,
            text: text_this_message,
        }))
    }
}

impl ModelExchange for HttpExchange<'_> {
    fn request_body(&self, request: &MessagesRequest<'_>) -> Value {
        // Provider-specific wire serialisation. Anthropic + OpenRouter
        // pass `MessagesRequest` through verbatim; OpenAI translates to
        // chat-completions JSON.
        self.cfg.provider.serialize_request(request)
    }

    fn step<'a>(&'a self, body: &'a Value, events: EventSink) -> ExchangeFuture<'a> {
        Box::pin(self.exchange(body, events))
    }

    fn adapt(&self, refusal: &Refusal) -> Option<Adaptation> {
        self.cfg.provider.adapt_after_refusal(&refusal.as_failure())
    }

    fn refusal_error(&self, refusal: &Refusal) -> Error {
        let cfg = self.cfg;
        // A failure the provider has its own words and recovery for (the
        // ChatGPT plan's usage limit, an account that cannot use it, a
        // session that ended). Nothing is retried on another provider.
        if let Some(error) = cfg.provider.classify_failure(&refusal.as_failure()) {
            // Worked out here, not in the macro, which skips its fields when
            // nothing listens.
            let code = match &error {
                Error::PlanUsage(e) => e.code(),
                _ => "-",
            };
            tracing::warn!(
                status = refusal.status,
                request_id = refusal.request_id.as_deref().unwrap_or("-"),
                code = %code,
                "the provider refused the request"
            );
            return error;
        }
        // A model the provider no longer has is its own error, naming
        // the model and the default to switch to (#468).
        if crate::provider::is_model_unavailable(
            cfg.provider.as_ref(),
            refusal.status,
            &refusal.body,
            &cfg.wire_model(),
        ) {
            // The provider's words are for the log; the person is shown
            // the model. A response body never carries the key.
            tracing::warn!(
                status = refusal.status,
                model = %cfg.model,
                body = %refusal.body,
                "the provider has no such model"
            );
            return Error::ModelUnavailable {
                model: cfg.model.clone(),
                default_model: cfg.provider.default_model().to_string(),
                status: refusal.status,
            };
        }
        // A request that did not fit the model's context is its own
        // error, with what to do about it (#395).
        context_window::api_error(refusal.status, refusal.body.clone())
    }

    fn replays_thinking(&self) -> bool {
        self.cfg.provider.replays_thinking()
    }

    fn retry_delay(&self) -> Duration {
        self.cfg.provider.stream_retry_delay()
    }
}

// ---------------------------------------------------------------------------
// The scripted adapter
// ---------------------------------------------------------------------------

/// One answer a [`ScriptedExchange`] gives.
#[derive(Debug)]
pub enum Scripted {
    Step(Step),
    Refused(Refusal),
    Interrupted(Error),
    /// The exchange fails outright, as a transport error would.
    Fail(Error),
}

impl From<Step> for Scripted {
    fn from(step: Step) -> Self {
        Self::Step(step)
    }
}

/// A model that answers from a list, for tests of a turn's policy. Each
/// call takes the next answer; an empty list is a protocol error. It records
/// the body of every request it was sent.
pub struct ScriptedExchange {
    script: Mutex<VecDeque<Scripted>>,
    requests: Mutex<Vec<Value>>,
    adaptations: Mutex<VecDeque<Adaptation>>,
    replays_thinking: bool,
}

impl ScriptedExchange {
    pub fn new(script: impl IntoIterator<Item = Scripted>) -> Self {
        Self {
            script: Mutex::new(script.into_iter().collect()),
            requests: Mutex::new(Vec::new()),
            adaptations: Mutex::new(VecDeque::new()),
            replays_thinking: false,
        }
    }

    /// A script of steps only.
    pub fn of_steps(steps: impl IntoIterator<Item = Step>) -> Self {
        Self::new(steps.into_iter().map(Scripted::Step))
    }

    /// Whether the model's thinking goes back into the history (the default
    /// is no, like a provider that does not issue thinking).
    pub fn replaying_thinking(mut self) -> Self {
        self.replays_thinking = true;
        self
    }

    /// The answers [`ModelExchange::adapt`] gives to refusals, one per call,
    /// in order; once they are spent it answers `None` (the default: a model
    /// that never adapts).
    pub fn adapting(self, adaptations: impl IntoIterator<Item = Adaptation>) -> Self {
        *self.adaptations.lock().expect("script mutex poisoned") =
            adaptations.into_iter().collect();
        self
    }

    /// The body of every request sent so far, in order.
    pub fn requests(&self) -> Vec<Value> {
        self.requests.lock().expect("script mutex poisoned").clone()
    }

    /// How many answers are left.
    pub fn remaining(&self) -> usize {
        self.script.lock().expect("script mutex poisoned").len()
    }
}

impl ModelExchange for ScriptedExchange {
    fn request_body(&self, request: &MessagesRequest<'_>) -> Value {
        serde_json::to_value(request).expect("MessagesRequest serialisation must not fail")
    }

    fn step<'a>(&'a self, body: &'a Value, events: EventSink) -> ExchangeFuture<'a> {
        self.requests
            .lock()
            .expect("script mutex poisoned")
            .push(body.clone());
        let next = self
            .script
            .lock()
            .expect("script mutex poisoned")
            .pop_front();
        Box::pin(async move {
            match next {
                None => Err(Error::Protocol(
                    "the scripted model ran out of steps".into(),
                )),
                Some(Scripted::Fail(error)) => Err(error),
                Some(Scripted::Refused(refusal)) => Ok(Exchanged::Refused(refusal)),
                Some(Scripted::Interrupted(error)) => Ok(Exchanged::Interrupted(error)),
                Some(Scripted::Step(step)) => {
                    // The events a streamed step would have sent, in order.
                    for block in &step.blocks {
                        match block {
                            StepBlock::ToolUse { id, name, .. } => {
                                let _ = events.send(AgentEvent::ToolCallStart {
                                    name: name.clone(),
                                    id: id.clone(),
                                });
                            }
                            StepBlock::Text(text) if !text.is_empty() => {
                                let _ = events.send(AgentEvent::TextDelta(text.clone()));
                            }
                            _ => {}
                        }
                    }
                    Ok(Exchanged::Step(step))
                }
            }
        })
    }

    fn adapt(&self, _refusal: &Refusal) -> Option<Adaptation> {
        self.adaptations
            .lock()
            .expect("script mutex poisoned")
            .pop_front()
    }

    fn refusal_error(&self, refusal: &Refusal) -> Error {
        context_window::api_error(refusal.status, refusal.body.clone())
    }

    fn replays_thinking(&self) -> bool {
        self.replays_thinking
    }

    fn retry_delay(&self) -> Duration {
        Duration::ZERO
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::json;
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;
    use crate::provider::{AnthropicProvider, LlmProvider};

    fn sse(events: &[(&str, Value)]) -> String {
        events
            .iter()
            .map(|(name, data)| format!("event: {name}\ndata: {data}\n\n"))
            .collect()
    }

    fn message_start() -> (&'static str, Value) {
        (
            "message_start",
            json!({
                "type": "message_start",
                "message": {
                    "id": "msg_test", "type": "message", "role": "assistant",
                    "content": [], "model": "m", "stop_reason": null,
                    "stop_sequence": null,
                    "usage": { "input_tokens": 1, "output_tokens": 0 }
                }
            }),
        )
    }

    fn message_stop() -> (&'static str, Value) {
        ("message_stop", json!({ "type": "message_stop" }))
    }

    /// Run one HTTP step against a server that answers with `stream`.
    async fn step_over(stream: String) -> (Result<Exchanged>, Vec<AgentEvent>) {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(stream),
            )
            .mount(&server)
            .await;
        let cfg = LlmConfig::new_anthropic("key").with_base_url(server.uri());
        let http = reqwest::Client::new();
        let exchange = HttpExchange::new(&cfg, &http);
        let mut events = Vec::new();
        let result = step_streaming(&exchange, &json!({}), &mut |e| events.push(e)).await;
        (result, events)
    }

    #[test]
    fn a_block_index_past_the_limit_is_refused_before_anything_is_allocated() {
        let mut blocks = Vec::new();
        let err = grow_to(&mut blocks, MAX_CONTENT_BLOCKS).unwrap_err();
        assert!(
            err.to_string().contains("exceeds MAX_CONTENT_BLOCKS"),
            "{err}"
        );
        assert!(blocks.is_empty());
        grow_to(&mut blocks, MAX_CONTENT_BLOCKS - 1).expect("the last index fits");
        assert_eq!(blocks.len(), MAX_CONTENT_BLOCKS);
    }

    #[tokio::test]
    async fn a_stream_opening_a_block_past_the_limit_is_an_error() {
        let (result, _) = step_over(sse(&[
            message_start(),
            (
                "content_block_start",
                json!({
                    "type": "content_block_start",
                    "index": MAX_CONTENT_BLOCKS,
                    "content_block": { "type": "text", "text": "" }
                }),
            ),
        ]))
        .await;
        assert!(
            matches!(&result, Err(Error::Protocol(m)) if m.contains("MAX_CONTENT_BLOCKS")),
            "{result:?}"
        );
    }

    #[tokio::test]
    async fn a_delta_for_a_block_never_opened_is_an_error() {
        let (result, events) = step_over(sse(&[
            message_start(),
            (
                "content_block_delta",
                json!({
                    "type": "content_block_delta",
                    "index": 3,
                    "delta": { "type": "text_delta", "text": "stray" }
                }),
            ),
        ]))
        .await;
        assert!(
            matches!(&result, Err(Error::Protocol(m)) if m.contains("unknown block index")),
            "{result:?}"
        );
        assert!(events.is_empty(), "{events:?}");
    }

    #[tokio::test]
    async fn a_ping_in_the_stream_changes_nothing() {
        let (result, events) = step_over(sse(&[
            message_start(),
            ("ping", json!({ "type": "ping" })),
            (
                "content_block_start",
                json!({
                    "type": "content_block_start",
                    "index": 0,
                    "content_block": { "type": "text", "text": "" }
                }),
            ),
            ("ping", json!({ "type": "ping" })),
            (
                "content_block_delta",
                json!({
                    "type": "content_block_delta",
                    "index": 0,
                    "delta": { "type": "text_delta", "text": "Hi." }
                }),
            ),
            message_stop(),
        ]))
        .await;
        match result {
            Ok(Exchanged::Step(step)) => {
                assert_eq!(step.text, "Hi.");
                assert_eq!(step.blocks, [StepBlock::Text("Hi.".into())]);
            }
            other => panic!("{other:?}"),
        }
        assert!(
            matches!(events.as_slice(), [AgentEvent::TextDelta(t)] if t == "Hi."),
            "{events:?}"
        );
    }

    /// Anthropic's endpoint, with a provider's own name for every refusal
    /// that is not a ChatGPT plan error.
    #[derive(Debug)]
    struct NamesItsFailures;

    impl LlmProvider for NamesItsFailures {
        fn id(&self) -> &'static str {
            "names-its-failures"
        }
        fn base_url(&self) -> &str {
            AnthropicProvider.base_url()
        }
        fn default_model(&self) -> &str {
            AnthropicProvider.default_model()
        }
        fn classifier_model(&self) -> &str {
            AnthropicProvider.classifier_model()
        }
        fn translate_model(&self, model: &str) -> String {
            model.to_string()
        }
        fn apply_auth(&self, req: reqwest::RequestBuilder, _: &str) -> reqwest::RequestBuilder {
            req
        }
        fn classify_failure(&self, _: &ProviderFailure<'_>) -> Option<Error> {
            Some(Error::Api {
                status: 418,
                message: "named by the provider".into(),
            })
        }
    }

    #[test]
    fn a_refusal_the_provider_names_is_its_error() {
        let cfg = LlmConfig::new(Arc::new(NamesItsFailures), "key");
        let http = reqwest::Client::new();
        let refusal = Refusal {
            status: 500,
            body: "boom".into(),
            request_id: Some("req_1".into()),
        };
        let error = HttpExchange::new(&cfg, &http).refusal_error(&refusal);
        assert!(
            matches!(&error, Error::Api { status: 418, message } if message == "named by the provider"),
            "{error:?}"
        );
    }
}
