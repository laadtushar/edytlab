//! OpenAI's Responses API as a third wire format (#474).
//!
//! `POST /v1/responses` is the one route Sign in with ChatGPT's plan usage
//! accepts: Chat Completions and Realtime are refused for those tokens. It
//! differs from the other two formats in ways that reach every layer, so the
//! format's own code lives here and [`crate::agent_loop`] only dispatches to
//! it:
//!
//! * the request ([`request_body`]): the system prompt is `instructions`,
//!   never a system item; `input` is an array of items, where an assistant
//!   turn's tool call is a `function_call` item and its answer a
//!   `function_call_output` item with the same `call_id`; function tools are
//!   grouped in a `namespace` tool; `store` is `false` and `stream` is `true`
//!   whatever the caller asked, because the route takes nothing else;
//! * the fields the preview refuses ([`UNSUPPORTED_FIELDS`]), none of which
//!   is ever sent. The output cap and the sampling knobs the other formats
//!   carry have no place here;
//! * the reply ([`ResponsesStream`]): typed events instead of chat chunks,
//!   which the state machine turns into the canonical [`StreamEvent`]s the
//!   agent loop already reads from the other two formats. A turn succeeds
//!   **only** at `response.completed`; `response.failed`,
//!   `response.incomplete` and `error` are errors with the API's own words,
//!   and a stream that stops before any of them is one too;
//! * one-shot calls (the classifier, the plan): the route streams those as
//!   well, so [`one_shot_body`] asks for a stream and [`collect_text`]
//!   gathers the text from it.
//!
//! The references are the plan-usage pages on `developers.openai.com`
//! (`/siwc/token-sharing-open-source/models-and-inference`, `.../preview-limitations`
//! and `.../errors-and-recovery`) and the function-calling guide
//! (<https://developers.openai.com/api/docs/guides/function-calling>) for the
//! item and event shapes.
//!
//! The ChatGPT plan provider ([`crate::chatgpt::provider`]) speaks this format.
//! A provider that does holds a [`ResponsesCodec`] and hands it
//! `serialize_request` and `parse_stream_chunk`, the way Groq hands them to
//! `OpenAIProvider`.

use std::collections::HashMap;
use std::sync::Mutex;

use eventsource_stream::Eventsource;
use futures_util::StreamExt;
use serde_json::{json, Map, Value};

use crate::anthropic::{
    ApiError, ContentBlock, ContentBlockDelta, ContentBlockStart, Message, MessageDeltaInner,
    MessageMeta, MessagesRequest, Role, StreamEvent, ToolChoice,
};
use crate::provider::ProviderError;
use crate::Error;

/// The top-level request fields the plan-usage preview refuses.
///
/// From the preview-limitations page: "Omit `background`, `conversation`,
/// `max_output_tokens`, `max_tool_calls`, `metadata`, `moderation`,
/// `multi_agent`, `prompt`, `prompt_cache_retention`, `safety_identifier`,
/// `temperature`, `top_logprobs`, `top_p`, `truncation`, and `user`", and
/// "Omit `previous_response_id` over HTTP and send the required history in
/// `input`". A request carrying one is refused.
///
/// Nothing in [`request_body`] writes any of these, and the tests check the
/// body against this list, which is the only place the names are written
/// down. When the preview adds one, add it here.
pub const UNSUPPORTED_FIELDS: &[&str] = &[
    "background",
    "conversation",
    "max_output_tokens",
    "max_tool_calls",
    "metadata",
    "moderation",
    "multi_agent",
    "prompt",
    "prompt_cache_retention",
    "safety_identifier",
    // Not on the preview page's list but refused on the route all the same
    // (other clients hit it): the service tier is the account's, not ours.
    "service_tier",
    "temperature",
    "top_logprobs",
    "top_p",
    "truncation",
    "user",
    "previous_response_id",
];

/// The `include` value that asks for a reasoning item's encrypted content.
const REASONING_INCLUDE: &str = "reasoning.encrypted_content";

/// What a namespace says it is. The name is a parameter and what the tools do
/// is in each tool's own description, so this says only what they are.
const NAMESPACE_DESCRIPTION: &str = "Functions the assistant can call.";

/// Said when a stream stops without a terminal event.
const CUT_OFF: &str = "the stream ended before the response completed";

/// How function tools are laid out in the request.
///
/// On the Sign in with ChatGPT route the preview page names two ways to offer
/// function tools: "group function/custom tools in namespaces or supply them
/// through `additional_tools` input items". It does not take `tool_search`.
/// The agent has about ninety tools, so [`ToolGrouping::Namespace`] puts them
/// all in one namespace, and whether an account accepts a namespace of that
/// size is for a real account to say.
///
/// The other layouts exist because of that. Third-party reports say plain
/// top-level function tools also work on the live route, and the page names
/// `additional_tools`, so a provider can step down a ladder when the route
/// refuses a layout, driven by the `param` of its 400 (the ChatGPT plan
/// provider's `ToolRung`). Nothing here chooses; the provider does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolGrouping {
    /// One `{"type":"namespace","name":<this>,"description":...,"tools":[...]}`
    /// holds every function. A call the model makes from it is echoed back in
    /// `input` with the same `namespace`.
    Namespace(String),
    /// No `tools` at all: one `{"type":"additional_tools","tools":[...]}` item
    /// leads `input` and holds every function. **The item's exact shape is
    /// not documented** (the page names the item and nothing more), so this is
    /// the obvious reading and a rung to try, not a fact; a route that
    /// refuses it answers with a 400 and the ladder moves on.
    AdditionalTools,
    /// Plain `{"type":"function"}` tools at the top level: the ordinary
    /// Responses shape, for a provider on the API-key Responses endpoint
    /// (moving the `openai` provider onto this format is a candidate), and the
    /// last rung of the plan route's ladder.
    Flat,
}

/// What a request body is built with, beyond the conversation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestOptions {
    /// How the tools are laid out.
    pub grouping: ToolGrouping,
    /// Whether reasoning items are asked for and echoed (see
    /// [`request_body_with`]).
    pub reasoning: bool,
    /// The `reasoning.effort` to send, if any. The caller has already checked
    /// that the model lists it: the route refuses an effort a model does not
    /// support.
    pub effort: Option<crate::anthropic::Effort>,
}

// ---------------------------------------------------------------------
// Request
// ---------------------------------------------------------------------

/// The Responses body for `req`.
///
/// * `store: false` and `stream: true`, always. `req.stream` is not read.
/// * The system blocks, joined by blank lines, are `instructions`. They are
///   left out when empty. Anthropic's `cache_control` has no meaning here and
///   is not carried over.
/// * `input` holds one item per piece of the conversation, in order. User
///   text is a `message` of `input_text`, assistant text a `message` of
///   `output_text`; a `tool_use` is a `function_call` and a `tool_result` a
///   `function_call_output` of the same `call_id` (a failed one is prefixed
///   `error: `, as on Chat Completions). A user turn's results come before
///   its words. Thinking blocks are dropped: they are Anthropic's, signed for
///   its own server.
/// * **Reasoning items** are the caller's choice (`reasoning`). The
///   function-calling guide says that for reasoning models "any reasoning
///   items returned in model responses with tool calls must also be passed
///   back with tool call outputs", and the reasoning guide that in stateless
///   mode (`store: false`) "reasoning items in the response's `output` array
///   include an `encrypted_content` property by default", the legacy
///   `include: ["reasoning.encrypted_content"]` being accepted "for
///   compatibility, but doesn't require it"
///   (<https://developers.openai.com/api/docs/guides/reasoning>). With
///   `reasoning` on, the body asks for the encrypted content anyway (the
///   field is not among the ones the preview refuses, and asking is the
///   documented way) and a `reasoning` item the parser kept
///   ([`ResponsesStream::with_reasoning`]) goes back in `input`, in its place
///   before the calls it led to, without its `id`: nothing is stored on this
///   route, so the id names nothing the server has. Off, the body has no
///   `include` and the items are left out, as before. Whether the route
///   accepts the field, and takes the items back, is for a real account to
///   say (spec §4.10).
///
///   The items are kept in the history as `redacted_thinking` blocks holding
///   the item's JSON, so the agent loop's rule for thinking applies to them
///   as it is: they are replayed within the turn they were produced in and
///   dropped by [`crate::agent_loop::strip_thinking`] at the start of the next,
///   because the encrypted content is tied to the `system` and `tools` it was
///   produced under (CLAUDE.md, the thinking rule).
/// * Tools are grouped as `grouping` says, each with `strict: false`: the
///   Responses API otherwise tries to turn a schema strict, which these
///   schemas are not written for. `tool_choice` is `"auto"`, or `"none"` for
///   the one request after the tool budget is spent, and is sent only with
///   tools.
/// * No output cap and no `output_config`. A reasoning effort is sent only
///   through [`request_body_for`], as the Responses API's own
///   `reasoning: {"effort"}`.
pub fn request_body(req: &MessagesRequest<'_>, grouping: &ToolGrouping) -> Value {
    request_body_with(req, grouping, false)
}

/// [`request_body`], with reasoning items asked for and replayed when
/// `reasoning` is set (see above).
pub fn request_body_with(
    req: &MessagesRequest<'_>,
    grouping: &ToolGrouping,
    reasoning: bool,
) -> Value {
    request_body_for(
        req,
        &RequestOptions {
            grouping: grouping.clone(),
            reasoning,
            effort: None,
        },
    )
}

/// The body for `req` as `options` say: the layout of the tools, reasoning
/// items, and a reasoning effort (`reasoning: {"effort": ...}`, the Responses
/// API's own field; the request's Anthropic-shaped `output_config` is never
/// read).
pub fn request_body_for(req: &MessagesRequest<'_>, options: &RequestOptions) -> Value {
    let grouping = &options.grouping;
    let instructions = req
        .system
        .iter()
        .map(|b| b.text)
        .collect::<Vec<_>>()
        .join("\n\n");
    let mut input = input_items(req.messages, grouping, options.reasoning);
    let mut body_tools = None;
    if let Some(tools) = req.tools.as_ref().and_then(|t| tools_value(t, grouping)) {
        if *grouping == ToolGrouping::AdditionalTools {
            input.insert(0, json!({ "type": "additional_tools", "tools": tools }));
        }
        body_tools = Some(tools);
    }
    let mut body = base_body(req.model, &instructions, input);
    if options.reasoning {
        body.insert("include".into(), json!([REASONING_INCLUDE]));
    }
    if let Some(effort) = options.effort {
        body.insert("reasoning".into(), json!({ "effort": effort.as_str() }));
    }
    if let Some(tools) = body_tools {
        if *grouping != ToolGrouping::AdditionalTools {
            body.insert("tools".into(), tools);
        }
        let choice = if req.tool_choice == Some(ToolChoice::NONE) {
            "none"
        } else {
            "auto"
        };
        body.insert("tool_choice".into(), Value::from(choice));
    }
    Value::Object(body)
}

/// The fields every body has: the ones the route requires, and nothing else.
fn base_body(model: &str, instructions: &str, input: Vec<Value>) -> Map<String, Value> {
    let mut body = Map::new();
    body.insert("model".into(), Value::from(model));
    if !instructions.is_empty() {
        body.insert("instructions".into(), Value::from(instructions));
    }
    body.insert("input".into(), Value::Array(input));
    body.insert("store".into(), Value::Bool(false));
    body.insert("stream".into(), Value::Bool(true));
    body
}

/// A message item. The part type differs by who is speaking: the API
/// rejects `input_text` in an assistant message.
fn message_item(role: &str, text: &str) -> Value {
    let part = if role == "assistant" {
        "output_text"
    } else {
        "input_text"
    };
    json!({
        "type": "message",
        "role": role,
        "content": [{ "type": part, "text": text }],
    })
}

fn input_items(messages: &[Message], grouping: &ToolGrouping, reasoning: bool) -> Vec<Value> {
    let mut items = Vec::with_capacity(messages.len());
    for message in messages {
        match message.role {
            Role::User => {
                let mut text = String::new();
                for block in &message.content {
                    match block {
                        ContentBlock::Text { text: t } => text.push_str(t),
                        ContentBlock::ToolResult {
                            tool_use_id,
                            content,
                            is_error,
                        } => {
                            let output = if matches!(is_error, Some(true)) {
                                format!("error: {content}")
                            } else {
                                content.clone()
                            };
                            items.push(json!({
                                "type": "function_call_output",
                                "call_id": tool_use_id,
                                "output": output,
                            }));
                        }
                        // The user's turn holds neither, and nothing here
                        // could make sense of them.
                        ContentBlock::ToolUse { .. }
                        | ContentBlock::Thinking { .. }
                        | ContentBlock::RedactedThinking { .. } => {}
                    }
                }
                if !text.is_empty() {
                    items.push(message_item("user", &text));
                }
            }
            Role::Assistant => {
                // Text before a call is its own message, and text after
                // another, so the items keep the order the blocks had.
                let mut text = String::new();
                for block in &message.content {
                    match block {
                        ContentBlock::Text { text: t } => text.push_str(t),
                        ContentBlock::ToolUse { id, name, input } => {
                            if !text.is_empty() {
                                items.push(message_item("assistant", &text));
                                text.clear();
                            }
                            let mut call = Map::new();
                            call.insert("type".into(), Value::from("function_call"));
                            call.insert("call_id".into(), Value::from(id.as_str()));
                            call.insert("name".into(), Value::from(name.as_str()));
                            call.insert(
                                "arguments".into(),
                                Value::from(
                                    serde_json::to_string(input)
                                        .unwrap_or_else(|_| "{}".to_string()),
                                ),
                            );
                            // Every tool is in the one namespace, so that is
                            // where each call came from.
                            if let ToolGrouping::Namespace(namespace) = grouping {
                                call.insert("namespace".into(), Value::from(namespace.as_str()));
                            }
                            items.push(Value::Object(call));
                        }
                        // A reasoning item the model produced earlier in this
                        // turn, in the order it came.
                        ContentBlock::RedactedThinking { data } if reasoning => {
                            if let Some(item) = reasoning_item(data) {
                                if !text.is_empty() {
                                    items.push(message_item("assistant", &text));
                                    text.clear();
                                }
                                items.push(item);
                            }
                        }
                        ContentBlock::ToolResult { .. }
                        | ContentBlock::Thinking { .. }
                        | ContentBlock::RedactedThinking { .. } => {}
                    }
                }
                if !text.is_empty() {
                    items.push(message_item("assistant", &text));
                }
            }
        }
    }
    items
}

/// A kept reasoning item as an `input` item: the item the server sent, without
/// the `id` that names nothing it stored. `None` for anything that is not a
/// `reasoning` item (the history holds `redacted_thinking` blocks from no
/// other source on this format, but a stray one is not guessed at).
fn reasoning_item(data: &str) -> Option<Value> {
    let mut item: Value = serde_json::from_str(data).ok()?;
    let object = item.as_object_mut()?;
    if object.get("type").and_then(Value::as_str) != Some("reasoning") {
        return None;
    }
    object.remove("id");
    Some(item)
}

/// `tools` for the Anthropic-shaped schema array, or `None` when there is no
/// tool to offer (the API rejects an empty array).
fn tools_value(tools: &Value, grouping: &ToolGrouping) -> Option<Value> {
    let list = tools.as_array().filter(|a| !a.is_empty())?;
    let functions: Vec<Value> = list.iter().map(function_tool).collect();
    Some(match grouping {
        ToolGrouping::Flat | ToolGrouping::AdditionalTools => Value::Array(functions),
        ToolGrouping::Namespace(name) => json!([{
            "type": "namespace",
            "name": name,
            "description": NAMESPACE_DESCRIPTION,
            "tools": functions,
        }]),
    })
}

fn function_tool(tool: &Value) -> Value {
    let mut function = Map::new();
    function.insert("type".into(), Value::from("function"));
    function.insert(
        "name".into(),
        tool.get("name").cloned().unwrap_or(Value::Null),
    );
    if let Some(description) = tool.get("description") {
        function.insert("description".into(), description.clone());
    }
    function.insert(
        "parameters".into(),
        tool.get("input_schema")
            .cloned()
            .unwrap_or_else(|| json!({ "type": "object", "properties": {} })),
    );
    // Left out, the API tries to normalise the schema to strict mode, which
    // makes every optional parameter required. Ours are written for the
    // non-strict reading.
    function.insert("strict".into(), Value::Bool(false));
    Value::Object(function)
}

/// The body of a one-shot request (the classifier, the plan): the same
/// fields as a step, no tools, and a stream, because the route takes no
/// other. `messages` are the `{"role", "content": <text>}` pairs the
/// one-shot callers build for every format.
pub(crate) fn one_shot_body(
    model: &str,
    system: &[&str],
    messages: &[Value],
    effort: Option<crate::anthropic::Effort>,
) -> Value {
    let input = messages
        .iter()
        .filter_map(|m| {
            let role = m.get("role")?.as_str()?;
            let text = m.get("content")?.as_str()?;
            Some(message_item(role, text))
        })
        .collect();
    let mut body = base_body(model, &system.join("\n\n"), input);
    if let Some(effort) = effort {
        body.insert("reasoning".into(), json!({ "effort": effort.as_str() }));
    }
    Value::Object(body)
}

// ---------------------------------------------------------------------
// Stream
// ---------------------------------------------------------------------

/// What `response.completed` reports of the tokens spent, when it does.
///
/// The canonical stream the agent loop reads has no place for usage (no
/// format forwards it), so it is kept on the parser for whoever wants it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

/// A function call in progress.
#[derive(Debug)]
struct Call {
    /// The canonical content block it was given.
    block: u32,
    /// The item's own id (`fc_...`), which the argument events name it by
    /// as well as by `output_index`.
    item_id: Option<String>,
    /// The arguments so far.
    args: String,
    /// Whether its block has been closed.
    done: bool,
}

/// A text block in progress.
#[derive(Debug)]
struct Text {
    block: u32,
    done: bool,
}

/// The state machine behind one response stream: Responses events in, the
/// canonical [`StreamEvent`]s the agent loop reads out.
///
/// A message's text is a block per `(output_index, content_index)`, opened
/// by the first delta. A function call is a `tool_use` block opened by its
/// `response.output_item.added` and given its arguments **whole**, in one
/// `input_json_delta`, once the API says they are complete
/// (`response.function_call_arguments.done` in either of its shapes, or the
/// finished item): the loop reads arguments only when a step ends, so nothing
/// is lost by not forwarding the pieces, and the call is never run on a
/// prefix of them. The finished event is the authority over the deltas if the
/// two disagree; the pieces are used only when no event brings the whole.
///
/// The stream ends the turn successfully at `response.completed` and
/// nowhere else (`MessageStop`). `response.failed`, `response.incomplete`
/// and `error` become one [`StreamEvent::Error`] carrying the API's message,
/// with its code as the error's `kind`. Whatever follows a terminal event is
/// ignored. A stream that stops with none seen is not detectable here, only
/// by the reader at the end of its input; [`is_complete`](Self::is_complete)
/// says whether `response.completed` came.
///
/// Events it does not model (`response.created`, `content_part.*`,
/// `reasoning*` unless [`ResponsesStream::with_reasoning`], `output_text.done`,
/// ...) produce nothing.
#[derive(Debug, Default)]
pub struct ResponsesStream {
    started: bool,
    terminated: bool,
    completed: bool,
    /// The next content block index.
    next_block: u32,
    calls: HashMap<u64, Call>,
    texts: HashMap<(u64, u64), Text>,
    /// Whether the response called any function: its stop reason.
    called: bool,
    usage: Option<Usage>,
    /// Whether `reasoning` items are kept (see [`Self::with_reasoning`]).
    reasoning: bool,
    /// The `output_index` of every reasoning item already kept.
    reasoned: std::collections::HashSet<u64>,
}

impl ResponsesStream {
    pub fn new() -> Self {
        Self::default()
    }

    /// A parser that keeps the response's `reasoning` items: each finished one
    /// becomes a `redacted_thinking` block whose data is the item's JSON, in
    /// the order the response gave it, for the request builder to put back
    /// ([`request_body_with`]). Without it they produce nothing.
    pub fn with_reasoning(reasoning: bool) -> Self {
        Self {
            reasoning,
            ..Self::default()
        }
    }

    /// Whether `response.completed` has been seen: the only success.
    pub fn is_complete(&self) -> bool {
        self.completed
    }

    /// The usage `response.completed` reported, if it did.
    pub fn usage(&self) -> Option<Usage> {
        self.usage
    }

    /// Take one SSE `data:` payload and return the canonical events it
    /// stands for.
    pub fn feed(&mut self, raw: &str) -> Result<Vec<StreamEvent>, ProviderError> {
        // Once a terminal event has been seen nothing else is read, so data
        // that would not parse is no error either.
        if self.terminated {
            return Ok(Vec::new());
        }
        let raw = raw.trim();
        if raw.is_empty() || raw == "[DONE]" {
            return Ok(Vec::new());
        }
        let event: Value =
            serde_json::from_str(raw).map_err(|e| ProviderError::Parse(e.to_string()))?;

        let mut out = Vec::new();
        if !self.started {
            self.started = true;
            out.push(StreamEvent::MessageStart {
                message: MessageMeta {
                    id: event
                        .pointer("/response/id")
                        .and_then(Value::as_str)
                        .unwrap_or("responses-msg")
                        .to_string(),
                    model: event
                        .pointer("/response/model")
                        .and_then(Value::as_str)
                        .unwrap_or("responses")
                        .to_string(),
                },
            });
        }

        match event.get("type").and_then(Value::as_str).unwrap_or("") {
            "response.output_item.added" => self.item_added(&event, &mut out)?,
            "response.output_item.done" => self.item_done(&event, &mut out)?,
            "response.output_text.delta" | "response.refusal.delta" => {
                // A refusal is what the model says in place of an answer.
                self.text_delta(&event, &mut out)
            }
            "response.function_call_arguments.delta" => self.arguments_delta(&event)?,
            "response.function_call_arguments.done" => self.arguments_done(&event, &mut out)?,
            "response.completed" => self.complete(&event, &mut out),
            "response.failed" => {
                let error = event.pointer("/response/error");
                let code = error.and_then(|e| e.get("code")).and_then(Value::as_str);
                let message = match (error.and_then(|e| e.get("message")), code) {
                    (Some(Value::String(m)), _) if !m.is_empty() => m.clone(),
                    (_, Some(code)) => format!("the response failed ({code})"),
                    _ => "the response failed".to_string(),
                };
                let param = error.and_then(|e| e.get("param")).and_then(Value::as_str);
                self.fail(code.unwrap_or("response_failed"), message, param, &mut out);
            }
            "response.incomplete" => {
                let message = match event
                    .pointer("/response/incomplete_details/reason")
                    .and_then(Value::as_str)
                {
                    Some(reason) => format!("the response ended early: {reason}"),
                    None => "the response ended early".to_string(),
                };
                self.fail("incomplete", message, None, &mut out);
            }
            "error" => {
                // The API puts `code`, `message` and `param` on the event
                // itself; an `error` object is the shape of every other error.
                let source = event.get("error").unwrap_or(&event);
                let code = source.get("code").and_then(Value::as_str);
                let message = source
                    .get("message")
                    .and_then(Value::as_str)
                    .filter(|m| !m.is_empty())
                    .unwrap_or("the API sent an error")
                    .to_string();
                let param = source.get("param").and_then(Value::as_str);
                self.fail(code.unwrap_or("error"), message, param, &mut out);
            }
            _ => {}
        }
        Ok(out)
    }

    fn alloc_block(&mut self) -> u32 {
        let block = self.next_block;
        self.next_block += 1;
        block
    }

    /// End the stream in one error. The errors page says to use the exact
    /// `error.code` and `error.param`: they are the error's `kind` and
    /// `param`, and the param is also in the message, because the message is
    /// all that reaches the user today.
    fn fail(
        &mut self,
        kind: &str,
        message: String,
        param: Option<&str>,
        out: &mut Vec<StreamEvent>,
    ) {
        self.terminated = true;
        let param = param.filter(|p| !p.is_empty());
        let message = match param {
            Some(param) => format!("{message} (param: {param})"),
            None => message,
        };
        out.push(StreamEvent::Error {
            error: ApiError {
                kind: kind.to_string(),
                message,
                param: param.map(str::to_string),
            },
        });
    }

    fn item_added(
        &mut self,
        event: &Value,
        out: &mut Vec<StreamEvent>,
    ) -> Result<(), ProviderError> {
        let item = item_of(event)?;
        if item.get("type").and_then(Value::as_str) == Some("function_call") {
            self.open_call(output_index(event)?, item, out)?;
        }
        Ok(())
    }

    fn item_done(
        &mut self,
        event: &Value,
        out: &mut Vec<StreamEvent>,
    ) -> Result<(), ProviderError> {
        let item = item_of(event)?;
        match item.get("type").and_then(Value::as_str) {
            Some("function_call") => {
                // The finished item carries the whole call, so a call whose
                // start was never seen can still be made.
                let index = output_index(event)?;
                self.open_call(index, item, out)?;
                self.finish_call(index, item.get("arguments").and_then(Value::as_str), out);
            }
            Some("reasoning") if self.reasoning => {
                let index = output_index(event)?;
                if self.reasoned.insert(index) {
                    let block = self.alloc_block();
                    out.push(StreamEvent::ContentBlockStart {
                        index: block,
                        content_block: ContentBlockStart::RedactedThinking {
                            data: item.to_string(),
                        },
                    });
                    out.push(StreamEvent::ContentBlockStop { index: block });
                }
            }
            Some("message") => {
                let index = output_index(event)?;
                // Text that was not streamed is in the finished item.
                if !self.texts.keys().any(|(i, _)| *i == index) {
                    let parts = item.get("content").and_then(Value::as_array);
                    for (at, part) in parts.into_iter().flatten().enumerate() {
                        let text = part
                            .get("text")
                            .or_else(|| part.get("refusal"))
                            .and_then(Value::as_str)
                            .unwrap_or("");
                        self.push_text(index, at as u64, text, out);
                    }
                }
                let mut open: Vec<u32> = self
                    .texts
                    .iter_mut()
                    .filter(|((i, _), t)| *i == index && !t.done)
                    .map(|(_, t)| {
                        t.done = true;
                        t.block
                    })
                    .collect();
                open.sort_unstable();
                out.extend(
                    open.into_iter()
                        .map(|index| StreamEvent::ContentBlockStop { index }),
                );
            }
            _ => {}
        }
        Ok(())
    }

    fn open_call(
        &mut self,
        index: u64,
        item: &Value,
        out: &mut Vec<StreamEvent>,
    ) -> Result<(), ProviderError> {
        if self.calls.contains_key(&index) {
            return Ok(());
        }
        let call_id = item
            .get("call_id")
            .and_then(Value::as_str)
            .ok_or_else(|| ProviderError::Parse("a function_call item has no call_id".into()))?;
        let name = item
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| ProviderError::Parse("a function_call item has no name".into()))?;
        let block = self.alloc_block();
        out.push(StreamEvent::ContentBlockStart {
            index: block,
            content_block: ContentBlockStart::ToolUse {
                id: call_id.to_string(),
                name: name.to_string(),
                input: json!({}),
            },
        });
        self.calls.insert(
            index,
            Call {
                block,
                item_id: item.get("id").and_then(Value::as_str).map(str::to_string),
                args: item
                    .get("arguments")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                done: false,
            },
        );
        Ok(())
    }

    /// The `output_index` of the call an argument event is about: by index
    /// when that names a call, else by the item's id.
    fn call_index(&self, event: &Value) -> Option<u64> {
        if let Some(index) = event.get("output_index").and_then(Value::as_u64) {
            if self.calls.contains_key(&index) {
                return Some(index);
            }
        }
        let id = event.get("item_id")?.as_str()?;
        self.calls
            .iter()
            .find(|(_, call)| call.item_id.as_deref() == Some(id))
            .map(|(index, _)| *index)
    }

    fn arguments_delta(&mut self, event: &Value) -> Result<(), ProviderError> {
        let index = self.call_index(event).ok_or_else(unknown_call)?;
        if let Some(delta) = event.get("delta").and_then(Value::as_str) {
            if let Some(call) = self.calls.get_mut(&index) {
                call.args.push_str(delta);
            }
        }
        Ok(())
    }

    fn arguments_done(
        &mut self,
        event: &Value,
        out: &mut Vec<StreamEvent>,
    ) -> Result<(), ProviderError> {
        let index = self.call_index(event).ok_or_else(unknown_call)?;
        // The guide's example puts `arguments` on the event; its field table
        // says the event carries the call as an `item`. Either is the whole.
        let whole = event
            .get("arguments")
            .or_else(|| event.pointer("/item/arguments"))
            .and_then(Value::as_str)
            .filter(|arguments| !arguments.is_empty());
        // An event with neither leaves the call open: the finished item may
        // still bring the whole, and `response.completed` closes the call on
        // the pieces if nothing does.
        if let Some(whole) = whole {
            self.finish_call(index, Some(whole), out);
        }
        Ok(())
    }

    /// Hand the loop a call's arguments, whole, and close its block. `whole`
    /// is what the API says they are; the pieces seen so far stand in when it
    /// says nothing. Once only: both `arguments.done` and the finished item
    /// report the same call.
    fn finish_call(&mut self, index: u64, whole: Option<&str>, out: &mut Vec<StreamEvent>) {
        let Some(call) = self.calls.get_mut(&index) else {
            return;
        };
        if call.done {
            return;
        }
        call.done = true;
        let args = match whole {
            Some(whole) if !whole.is_empty() => whole.to_string(),
            _ => std::mem::take(&mut call.args),
        };
        if !args.is_empty() {
            out.push(StreamEvent::ContentBlockDelta {
                index: call.block,
                delta: ContentBlockDelta::InputJsonDelta { partial_json: args },
            });
        }
        out.push(StreamEvent::ContentBlockStop { index: call.block });
        self.called = true;
    }

    fn text_delta(&mut self, event: &Value, out: &mut Vec<StreamEvent>) {
        let index = event
            .get("output_index")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let part = event
            .get("content_index")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        if let Some(delta) = event.get("delta").and_then(Value::as_str) {
            self.push_text(index, part, delta, out);
        }
    }

    fn push_text(&mut self, index: u64, part: u64, text: &str, out: &mut Vec<StreamEvent>) {
        if text.is_empty() {
            return;
        }
        let block = match self.texts.get(&(index, part)) {
            Some(open) => open.block,
            None => {
                let block = self.alloc_block();
                out.push(StreamEvent::ContentBlockStart {
                    index: block,
                    content_block: ContentBlockStart::Text {
                        text: String::new(),
                    },
                });
                self.texts
                    .insert((index, part), Text { block, done: false });
                block
            }
        };
        out.push(StreamEvent::ContentBlockDelta {
            index: block,
            delta: ContentBlockDelta::TextDelta {
                text: text.to_string(),
            },
        });
    }

    fn complete(&mut self, event: &Value, out: &mut Vec<StreamEvent>) {
        // Anything the API did not close is closed now, calls with the
        // arguments they have.
        let mut calls: Vec<u64> = self
            .calls
            .iter()
            .filter(|(_, c)| !c.done)
            .map(|(i, _)| *i)
            .collect();
        calls.sort_unstable();
        for index in calls {
            self.finish_call(index, None, out);
        }
        let mut texts: Vec<u32> = self
            .texts
            .values_mut()
            .filter(|t| !t.done)
            .map(|t| {
                t.done = true;
                t.block
            })
            .collect();
        texts.sort_unstable();
        out.extend(
            texts
                .into_iter()
                .map(|index| StreamEvent::ContentBlockStop { index }),
        );

        if let Some(usage) = event.pointer("/response/usage") {
            self.usage = Some(Usage {
                input_tokens: usage
                    .get("input_tokens")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
                output_tokens: usage
                    .get("output_tokens")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
            });
        }

        let stop_reason = if self.called { "tool_use" } else { "end_turn" };
        out.push(StreamEvent::MessageDelta {
            delta: MessageDeltaInner {
                stop_reason: Some(stop_reason.to_string()),
            },
        });
        out.push(StreamEvent::MessageStop);
        self.completed = true;
        self.terminated = true;
    }
}

fn unknown_call() -> ProviderError {
    ProviderError::Parse("an argument event names a function call that was never started".into())
}

fn item_of(event: &Value) -> Result<&Value, ProviderError> {
    event
        .get("item")
        .ok_or_else(|| ProviderError::Parse("an output_item event has no item".into()))
}

fn output_index(event: &Value) -> Result<u64, ProviderError> {
    event
        .get("output_index")
        .and_then(Value::as_u64)
        .ok_or_else(|| ProviderError::Parse("an output_item event has no output_index".into()))
}

// ---------------------------------------------------------------------
// The two hooks a provider needs
// ---------------------------------------------------------------------

/// A provider's half of the format: the tool grouping it was set up with and
/// the parser state for the response in flight. A provider speaking
/// [`WireFormat::Responses`](crate::provider::WireFormat::Responses) holds one and hands it
/// [`LlmProvider::serialize_request`](crate::LlmProvider::serialize_request)
/// and [`LlmProvider::parse_stream_chunk`](crate::LlmProvider::parse_stream_chunk).
///
/// Like `OpenAIProvider`'s, the stream state is reset by each request: the
/// loop makes one request per round trip, and the stream that follows is the
/// one the state is for.
#[derive(Debug)]
pub struct ResponsesCodec {
    grouping: ToolGrouping,
    reasoning: bool,
    stream: Mutex<ResponsesStream>,
}

impl ResponsesCodec {
    pub fn new(grouping: ToolGrouping) -> Self {
        Self::with_reasoning(grouping, false)
    }

    /// A codec that asks for reasoning items, keeps them and replays them
    /// within a turn (see [`request_body_with`]).
    pub fn with_reasoning(grouping: ToolGrouping, reasoning: bool) -> Self {
        Self {
            grouping,
            reasoning,
            stream: Mutex::new(ResponsesStream::with_reasoning(reasoning)),
        }
    }

    /// The body for `req` as `options` say (see [`request_body_for`]), and a
    /// fresh parser, keeping reasoning items or not as the options say, for
    /// the stream it will get. For a provider whose layout changes while it
    /// runs.
    pub fn serialize_with(&self, req: &MessagesRequest<'_>, options: &RequestOptions) -> Value {
        if let Ok(mut stream) = self.stream.lock() {
            *stream = ResponsesStream::with_reasoning(options.reasoning);
        }
        request_body_for(req, options)
    }

    /// The body for `req` (see [`request_body`]), and a fresh parser for the
    /// stream it will get.
    pub fn serialize_request(&self, req: &MessagesRequest<'_>) -> Value {
        if let Ok(mut stream) = self.stream.lock() {
            *stream = ResponsesStream::with_reasoning(self.reasoning);
        }
        request_body_with(req, &self.grouping, self.reasoning)
    }

    /// One SSE payload of the stream (see [`ResponsesStream::feed`]).
    pub fn parse_stream_chunk(&self, raw: &str) -> Result<Vec<StreamEvent>, ProviderError> {
        self.stream
            .lock()
            .map_err(|_| ProviderError::Parse("responses stream state poisoned".into()))?
            .feed(raw)
    }

    /// The usage the response in flight reported at `response.completed`, if
    /// it has and did (see [`ResponsesStream::usage`]). The canonical stream
    /// carries none, so this is where a provider reads it from.
    pub fn usage(&self) -> Option<Usage> {
        self.stream.lock().ok().and_then(|stream| stream.usage())
    }
}

impl Clone for ResponsesCodec {
    /// A copy starts with no response in flight.
    fn clone(&self) -> Self {
        Self::with_reasoning(self.grouping.clone(), self.reasoning)
    }
}

// ---------------------------------------------------------------------
// What the agent loop asks of the format
// ---------------------------------------------------------------------

/// The error for a step's stream that ran out before `response.completed`.
/// Only a format that [`needs_terminal_event`](crate::provider::WireFormat::needs_terminal_event)
/// ends a step in it; the other two have always taken the end of the stream
/// as the end of the step.
pub(crate) fn cut_off_error() -> Error {
    Error::ApiStream(CUT_OFF.to_string())
}

/// Why a stream could not be read to `response.completed`.
///
/// Keeps what the API said apart from the words: [`kind`](Self::kind) is the
/// error's code (`subscription_sharing_usage_limit_exceeded`, ...) and
/// [`param`](Self::param) the request field it is about, which a provider
/// that must tell its failures apart reads through
/// [`LlmProvider::classify_failure`](crate::LlmProvider::classify_failure).
/// A stream that failed to be read at all, or stopped early, has neither.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamFailure {
    pub kind: Option<String>,
    pub message: String,
    pub param: Option<String>,
    /// The `x-request-id` (or `openai-request-id`) of the response the stream
    /// came on.
    pub request_id: Option<String>,
}

impl StreamFailure {
    fn plain(message: impl Into<String>, request_id: Option<String>) -> Self {
        Self {
            kind: None,
            message: message.into(),
            param: None,
            request_id,
        }
    }
}

/// The request ID a response carries: `x-request-id`, else
/// `openai-request-id`. The errors page asks for it to be preserved.
pub(crate) fn request_id_of(headers: &reqwest::header::HeaderMap) -> Option<String> {
    ["x-request-id", "openai-request-id"]
        .iter()
        .filter_map(|name| headers.get(*name))
        .filter_map(|v| v.to_str().ok())
        .map(str::trim)
        .find(|v| !v.is_empty())
        .map(str::to_string)
}

impl std::fmt::Display for StreamFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// What a completed response said: its text, and the names of the functions it
/// called, in order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StreamRead {
    pub text: String,
    pub tool_calls: Vec<String>,
}

/// Read a response's stream through `response.completed`.
///
/// `Err` is a response that did not complete: the stream failed (with the
/// API's code and param), was cut off, or could not be read.
pub(crate) async fn read_stream(resp: reqwest::Response) -> Result<StreamRead, StreamFailure> {
    let request_id = request_id_of(resp.headers());
    let mut parser = ResponsesStream::new();
    let mut sse = resp
        .bytes_stream()
        .map(|chunk| chunk.map_err(std::io::Error::other))
        .eventsource();
    let mut read = StreamRead::default();

    while let Some(event) = sse.next().await {
        let event = event.map_err(|e| StreamFailure::plain(e.to_string(), request_id.clone()))?;
        if event.data.is_empty() {
            continue;
        }
        let parsed = parser
            .feed(&event.data)
            .map_err(|e| StreamFailure::plain(e.to_string(), request_id.clone()))?;
        for parsed in parsed {
            match parsed {
                StreamEvent::ContentBlockDelta {
                    delta: ContentBlockDelta::TextDelta { text: more },
                    ..
                } => read.text.push_str(&more),
                StreamEvent::ContentBlockStart {
                    content_block: ContentBlockStart::ToolUse { name, .. },
                    ..
                } => read.tool_calls.push(name),
                StreamEvent::Error { error } => {
                    return Err(StreamFailure {
                        kind: Some(error.kind),
                        message: error.message,
                        param: error.param,
                        request_id,
                    })
                }
                _ => {}
            }
        }
        if parser.is_complete() {
            return Ok(read);
        }
    }
    Err(StreamFailure::plain(CUT_OFF, request_id))
}

/// The text of a one-shot reply, read from its stream: every text delta, in
/// order, up to `response.completed`.
///
/// `Ok(None)` is a response that completed without saying anything. `Err`
/// is one that did not complete: the stream failed, was cut off, or could
/// not be read, and the failure says how, with the API's code and param when
/// it gave them.
pub(crate) async fn collect_text(resp: reqwest::Response) -> Result<Option<String>, StreamFailure> {
    let read = read_stream(resp).await?;
    Ok((!read.text.is_empty()).then_some(read.text))
}

#[cfg(test)]
pub(crate) mod testing {
    //! A stand-in provider on this format, and server-side stream builders,
    //! for the tests of the pieces that sit above the parser.

    use super::*;
    use crate::provider::WireFormat;
    use crate::LlmProvider;

    /// What a provider on this format looks like to the agent loop.
    #[derive(Debug)]
    pub(crate) struct ResponsesProvider {
        codec: ResponsesCodec,
    }

    impl ResponsesProvider {
        pub(crate) fn new() -> Self {
            Self {
                codec: ResponsesCodec::new(ToolGrouping::Namespace("edytlab".into())),
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
        fn apply_auth(
            &self,
            req: reqwest::RequestBuilder,
            api_key: &str,
        ) -> reqwest::RequestBuilder {
            req.header("authorization", format!("Bearer {api_key}"))
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

    /// `event:`/`data:` frames for `events`.
    pub(crate) fn sse(events: &[Value]) -> String {
        events
            .iter()
            .map(|e| format!("event: {}\ndata: {e}\n\n", e["type"].as_str().unwrap()))
            .collect()
    }

    /// A whole streamed message of `pieces`, ending at `response.completed`.
    pub(crate) fn text_stream(pieces: &[&str]) -> String {
        let mut events = vec![
            json!({ "type": "response.created", "response": { "id": "resp_1" } }),
            json!({
                "type": "response.output_item.added", "output_index": 0,
                "item": { "type": "message", "id": "msg_1", "role": "assistant", "content": [] }
            }),
        ];
        for piece in pieces {
            events.push(json!({
                "type": "response.output_text.delta",
                "item_id": "msg_1", "output_index": 0, "content_index": 0, "delta": piece
            }));
        }
        events.push(completed());
        sse(&events)
    }

    pub(crate) fn completed() -> Value {
        json!({ "type": "response.completed", "response": { "id": "resp_1", "status": "completed" } })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::WireFormat;

    /// One line per canonical event, so a sequence reads at a glance.
    fn render(events: &[StreamEvent]) -> Vec<String> {
        events
            .iter()
            .map(|e| match e {
                StreamEvent::MessageStart { message } => format!("start {}", message.id),
                StreamEvent::ContentBlockStart {
                    index,
                    content_block: ContentBlockStart::Text { .. },
                } => format!("text# {index}"),
                StreamEvent::ContentBlockStart {
                    index,
                    content_block: ContentBlockStart::ToolUse { id, name, .. },
                } => format!("tool# {index} {id} {name}"),
                StreamEvent::ContentBlockDelta {
                    index,
                    delta: ContentBlockDelta::TextDelta { text },
                } => format!("text {index} {text:?}"),
                StreamEvent::ContentBlockDelta {
                    index,
                    delta: ContentBlockDelta::InputJsonDelta { partial_json },
                } => format!("args {index} {partial_json}"),
                StreamEvent::ContentBlockStop { index } => format!("stop {index}"),
                StreamEvent::MessageDelta { delta } => {
                    format!("reason {}", delta.stop_reason.as_deref().unwrap_or("-"))
                }
                StreamEvent::MessageStop => "message_stop".to_string(),
                StreamEvent::Error { error } => format!("error {}: {}", error.kind, error.message),
                other => format!("{other:?}"),
            })
            .collect()
    }

    /// Feed `events` one by one; everything the parser returned.
    fn feed_all(stream: &mut ResponsesStream, events: &[Value]) -> Vec<String> {
        let mut out = Vec::new();
        for event in events {
            out.extend(render(&stream.feed(&event.to_string()).expect("parses")));
        }
        out
    }

    fn call_added(index: u64, call_id: &str, name: &str) -> Value {
        json!({
            "type": "response.output_item.added", "output_index": index,
            "item": { "type": "function_call", "id": format!("fc_{index}"), "call_id": call_id, "name": name, "arguments": "" }
        })
    }

    fn args_delta(index: u64, delta: &str) -> Value {
        json!({
            "type": "response.function_call_arguments.delta",
            "item_id": format!("fc_{index}"), "output_index": index, "delta": delta
        })
    }

    fn args_done(index: u64, arguments: &str) -> Value {
        json!({
            "type": "response.function_call_arguments.done",
            "item_id": format!("fc_{index}"), "output_index": index, "arguments": arguments
        })
    }

    fn text_delta(index: u64, part: u64, delta: &str) -> Value {
        json!({
            "type": "response.output_text.delta",
            "output_index": index, "content_index": part, "delta": delta
        })
    }

    fn completed() -> Value {
        testing::completed()
    }

    #[test]
    fn text_deltas_become_one_text_block() {
        let mut stream = ResponsesStream::new();
        let out = feed_all(
            &mut stream,
            &[
                json!({ "type": "response.created", "response": { "id": "resp_7", "model": "m" } }),
                json!({ "type": "response.output_item.added", "output_index": 0,
                        "item": { "type": "message", "role": "assistant", "content": [] } }),
                text_delta(0, 0, "Hel"),
                text_delta(0, 0, "lo"),
                json!({ "type": "response.output_text.done", "output_index": 0, "text": "Hello" }),
                json!({ "type": "response.output_item.done", "output_index": 0,
                        "item": { "type": "message", "content": [{ "type": "output_text", "text": "Hello" }] } }),
                completed(),
            ],
        );
        assert_eq!(
            out,
            [
                "start resp_7",
                "text# 0",
                "text 0 \"Hel\"",
                "text 0 \"lo\"",
                "stop 0",
                "reason end_turn",
                "message_stop",
            ]
        );
        assert!(stream.is_complete());
    }

    #[test]
    fn a_function_call_is_started_then_given_its_arguments_whole() {
        let mut stream = ResponsesStream::new();
        let out = feed_all(
            &mut stream,
            &[
                call_added(0, "call_1", "set_track_gain"),
                args_delta(0, "{\"tra"),
                args_delta(0, "ck\":0,\"d"),
                args_delta(0, "b\":-3}"),
                args_done(0, "{\"track\":0,\"db\":-3}"),
                json!({ "type": "response.output_item.done", "output_index": 0,
                        "item": { "type": "function_call", "id": "fc_0", "call_id": "call_1",
                                  "name": "set_track_gain", "arguments": "{\"track\":0,\"db\":-3}" } }),
                completed(),
            ],
        );
        assert_eq!(
            out,
            [
                "start responses-msg",
                "tool# 0 call_1 set_track_gain",
                // Once, whole; the pieces are not forwarded, and the finished
                // item does not repeat what `arguments.done` gave.
                "args 0 {\"track\":0,\"db\":-3}",
                "stop 0",
                "reason tool_use",
                "message_stop",
            ]
        );
    }

    /// The pieces are used when the API's own whole is not there: a call
    /// can finish by its item alone, or by neither.
    #[test]
    fn the_pieces_stand_in_when_no_whole_arguments_come() {
        let mut stream = ResponsesStream::new();
        let out = feed_all(
            &mut stream,
            &[
                call_added(0, "call_1", "t"),
                args_delta(0, "{\"a\":"),
                args_delta(0, "1}"),
                json!({ "type": "response.output_item.done", "output_index": 0,
                        "item": { "type": "function_call", "call_id": "call_1", "name": "t", "arguments": "" } }),
                // A second call that is never closed before `completed`.
                call_added(1, "call_2", "u"),
                args_delta(1, "{}"),
                completed(),
            ],
        );
        assert_eq!(
            out[1..],
            [
                "tool# 0 call_1 t",
                "args 0 {\"a\":1}",
                "stop 0",
                "tool# 1 call_2 u",
                "args 1 {}",
                "stop 1",
                "reason tool_use",
                "message_stop",
            ]
        );
    }

    /// If the pieces and the whole disagree, the whole is what the model
    /// called the tool with.
    #[test]
    fn the_finished_arguments_win_over_the_pieces() {
        let mut stream = ResponsesStream::new();
        let out = feed_all(
            &mut stream,
            &[
                call_added(0, "c", "t"),
                args_delta(0, "{\"a\":1"),
                args_done(0, "{\"a\":1}"),
            ],
        );
        assert_eq!(out[2], "args 0 {\"a\":1}");
    }

    /// The guide's field table says `arguments.done` carries the call as an
    /// `item`, and its example puts `arguments` on the event. Both shapes are
    /// the whole, and either beats the pieces.
    #[test]
    fn the_finished_arguments_win_in_the_item_shape_too() {
        let mut stream = ResponsesStream::new();
        let out = feed_all(
            &mut stream,
            &[
                call_added(0, "c", "t"),
                args_delta(0, "{\"a\":1"),
                json!({
                    "type": "response.function_call_arguments.done",
                    "item_id": "fc_0", "output_index": 0,
                    "item": { "type": "function_call", "id": "fc_0", "call_id": "c",
                              "name": "t", "arguments": "{\"a\":1}" }
                }),
                // And the finished item that follows adds nothing.
                json!({ "type": "response.output_item.done", "output_index": 0,
                        "item": { "type": "function_call", "id": "fc_0", "call_id": "c",
                                  "name": "t", "arguments": "{\"a\":1}" } }),
            ],
        );
        assert_eq!(
            out[1..],
            ["tool# 0 c t", "args 0 {\"a\":1}", "stop 0"],
            "the whole, once, and not the prefix the pieces made"
        );
    }

    /// An `arguments.done` that says nothing about the arguments does not
    /// close the call on the pieces: the whole may still come in the finished
    /// item, and that is the one to run with.
    #[test]
    fn a_done_event_with_no_arguments_leaves_the_call_for_its_finished_item() {
        let mut stream = ResponsesStream::new();
        let out = feed_all(
            &mut stream,
            &[
                call_added(0, "c", "t"),
                args_delta(0, "{\"a\":1"),
                json!({ "type": "response.function_call_arguments.done",
                        "item_id": "fc_0", "output_index": 0 }),
            ],
        );
        assert_eq!(out[1..], ["tool# 0 c t"], "nothing is closed yet");

        let out = feed_all(
            &mut stream,
            &[
                json!({ "type": "response.output_item.done", "output_index": 0,
                      "item": { "type": "function_call", "call_id": "c", "name": "t",
                                "arguments": "{\"a\":1}" } }),
            ],
        );
        assert_eq!(out, ["args 0 {\"a\":1}", "stop 0"]);
    }

    /// Two calls whose argument pieces arrive mixed together each keep their
    /// own, whether the events name them by `output_index` or by `item_id`.
    #[test]
    fn interleaved_argument_deltas_stay_with_their_own_call() {
        let by_item = |index: u64, delta: &str| {
            json!({
                "type": "response.function_call_arguments.delta",
                "item_id": format!("fc_{index}"), "delta": delta
            })
        };
        let mut stream = ResponsesStream::new();
        let out = feed_all(
            &mut stream,
            &[
                call_added(0, "call_a", "one"),
                call_added(1, "call_b", "two"),
                args_delta(0, "{\"a\":"),
                args_delta(1, "{\"b\":"),
                by_item(1, "2}"),
                args_delta(0, "1}"),
                json!({ "type": "response.output_item.done", "output_index": 1,
                        "item": { "type": "function_call", "call_id": "call_b", "name": "two" } }),
                json!({ "type": "response.output_item.done", "output_index": 0,
                        "item": { "type": "function_call", "call_id": "call_a", "name": "one" } }),
                completed(),
            ],
        );
        assert_eq!(
            out[1..],
            [
                "tool# 0 call_a one",
                "tool# 1 call_b two",
                "args 1 {\"b\":2}",
                "stop 1",
                "args 0 {\"a\":1}",
                "stop 0",
                "reason tool_use",
                "message_stop",
            ]
        );
    }

    #[test]
    fn a_call_known_only_from_its_finished_item_is_still_made() {
        let mut stream = ResponsesStream::new();
        let out = feed_all(
            &mut stream,
            &[
                json!({ "type": "response.output_item.done", "output_index": 3,
                        "item": { "type": "function_call", "id": "fc_9", "call_id": "call_9",
                                  "name": "t", "arguments": "{\"x\":2}" } }),
                completed(),
            ],
        );
        assert_eq!(
            out,
            [
                "start responses-msg",
                "tool# 0 call_9 t",
                "args 0 {\"x\":2}",
                "stop 0",
                "reason tool_use",
                "message_stop"
            ]
        );
    }

    #[test]
    fn a_calls_arguments_can_be_matched_by_item_id() {
        let mut stream = ResponsesStream::new();
        let out = feed_all(
            &mut stream,
            &[
                call_added(2, "call_x", "t"),
                // No `output_index` on the delta, only the item's id.
                json!({ "type": "response.function_call_arguments.delta", "item_id": "fc_2", "delta": "{}" }),
                json!({ "type": "response.function_call_arguments.done", "item_id": "fc_2", "arguments": "{}" }),
            ],
        );
        assert_eq!(out[2], "args 0 {}");
    }

    #[test]
    fn text_that_was_never_streamed_is_taken_from_the_finished_item_once() {
        let mut stream = ResponsesStream::new();
        let done = json!({
            "type": "response.output_item.done", "output_index": 0,
            "item": { "type": "message", "content": [{ "type": "output_text", "text": "Whole." }] }
        });
        let out = feed_all(&mut stream, &[done.clone(), done]);
        assert_eq!(
            out,
            [
                "start responses-msg",
                "text# 0",
                "text 0 \"Whole.\"",
                "stop 0"
            ],
            "the second finished item adds nothing"
        );

        // And text that was streamed is not repeated from the item.
        let mut stream = ResponsesStream::new();
        let out = feed_all(
            &mut stream,
            &[
                text_delta(0, 0, "Hi"),
                json!({ "type": "response.output_item.done", "output_index": 0,
                        "item": { "type": "message", "content": [{ "type": "output_text", "text": "Hi" }] } }),
            ],
        );
        assert_eq!(out[1..], ["text# 0", "text 0 \"Hi\"", "stop 0"]);
    }

    #[test]
    fn text_parts_and_calls_get_blocks_in_the_order_they_appear() {
        let mut stream = ResponsesStream::new();
        let out = feed_all(
            &mut stream,
            &[
                text_delta(0, 0, "a"),
                text_delta(0, 1, "b"),
                call_added(1, "c", "t"),
                args_done(1, "{}"),
                text_delta(2, 0, "z"),
                completed(),
            ],
        );
        assert_eq!(
            out[1..],
            [
                "text# 0",
                "text 0 \"a\"",
                "text# 1",
                "text 1 \"b\"",
                "tool# 2 c t",
                "args 2 {}",
                "stop 2",
                "text# 3",
                "text 3 \"z\"",
                // `completed` closes the three text blocks nobody did.
                "stop 0",
                "stop 1",
                "stop 3",
                "reason tool_use",
                "message_stop",
            ]
        );
    }

    #[test]
    fn a_refusal_is_the_reply() {
        let mut stream = ResponsesStream::new();
        let out = feed_all(
            &mut stream,
            &[
                json!({ "type": "response.refusal.delta", "output_index": 0, "content_index": 0, "delta": "I can't." }),
            ],
        );
        assert_eq!(out[1..], ["text# 0", "text 0 \"I can't.\""]);
    }

    #[test]
    fn completed_reports_the_usage_it_carries() {
        let mut stream = ResponsesStream::new();
        assert_eq!(stream.usage(), None);
        feed_all(
            &mut stream,
            &[json!({ "type": "response.completed", "response": {
                "usage": { "input_tokens": 120, "output_tokens": 34, "total_tokens": 154 } } })],
        );
        assert_eq!(
            stream.usage(),
            Some(Usage {
                input_tokens: 120,
                output_tokens: 34
            })
        );

        // And none is made up when the event has none.
        let mut stream = ResponsesStream::new();
        feed_all(&mut stream, &[completed()]);
        assert_eq!(stream.usage(), None);
        assert!(stream.is_complete());
    }

    #[test]
    fn a_stream_is_not_complete_until_response_completed() {
        let mut stream = ResponsesStream::new();
        feed_all(&mut stream, &[text_delta(0, 0, "x")]);
        assert!(!stream.is_complete());
    }

    #[test]
    fn failed_incomplete_and_error_each_end_the_stream_in_one_error() {
        let cases = [
            (
                json!({ "type": "response.failed", "response": { "error": {
                    "code": "subscription_sharing_usage_limit_exceeded",
                    "message": "You hit your limit." } } }),
                "error subscription_sharing_usage_limit_exceeded: You hit your limit.",
            ),
            (
                json!({ "type": "response.failed", "response": { "error": { "code": "server_error" } } }),
                "error server_error: the response failed (server_error)",
            ),
            (
                json!({ "type": "response.failed", "response": {} }),
                "error response_failed: the response failed",
            ),
            (
                json!({ "type": "response.incomplete", "response": {
                    "incomplete_details": { "reason": "max_output_tokens" } } }),
                "error incomplete: the response ended early: max_output_tokens",
            ),
            (
                json!({ "type": "response.incomplete", "response": {} }),
                "error incomplete: the response ended early",
            ),
            (
                json!({ "type": "error", "code": "rate_limit_exceeded", "message": "Slow down.", "param": null }),
                "error rate_limit_exceeded: Slow down.",
            ),
            (
                json!({ "type": "error", "error": { "code": "x", "message": "Nested." } }),
                "error x: Nested.",
            ),
            (
                json!({ "type": "error" }),
                "error error: the API sent an error",
            ),
        ];
        for (event, expected) in cases {
            let mut stream = ResponsesStream::new();
            let out = feed_all(&mut stream, &[event.clone()]);
            assert_eq!(out, ["start responses-msg", expected], "{event}");
            assert!(!stream.is_complete(), "{event}");
            // Nothing after a terminal event counts, a `completed` included,
            // and not even data that would not parse.
            assert!(
                feed_all(&mut stream, &[text_delta(0, 0, "late"), completed()]).is_empty(),
                "{event}"
            );
            assert!(stream.feed("not json").expect("ignored").is_empty());
            assert!(!stream.is_complete(), "{event}");
        }
    }

    /// The errors page says to use the exact `error.code` and `error.param`;
    /// the message is all that reaches the user today, so the param is in it.
    #[test]
    fn an_errors_param_is_kept_in_its_message() {
        let cases = [
            (
                json!({ "type": "response.failed", "response": { "error": {
                    "code": "subscription_sharing_unsupported_capability",
                    "message": "Unsupported input.", "param": "input[3].type" } } }),
                "error subscription_sharing_unsupported_capability: Unsupported input. (param: input[3].type)",
            ),
            (
                json!({ "type": "response.failed", "response": { "error": {
                    "code": "server_error", "param": "tools" } } }),
                "error server_error: the response failed (server_error) (param: tools)",
            ),
            (
                json!({ "type": "error", "code": "invalid_value", "message": "Bad value.", "param": "model" }),
                "error invalid_value: Bad value. (param: model)",
            ),
            (
                json!({ "type": "error", "error": { "code": "x", "message": "Nested.", "param": "p" } }),
                "error x: Nested. (param: p)",
            ),
            // A null or empty param is none.
            (
                json!({ "type": "error", "code": "x", "message": "Plain.", "param": "" }),
                "error x: Plain.",
            ),
        ];
        for (event, expected) in cases {
            let mut stream = ResponsesStream::new();
            let out = feed_all(&mut stream, &[event.clone()]);
            assert_eq!(out[1], expected, "{event}");
        }
    }

    #[test]
    fn events_it_does_not_model_say_nothing() {
        let mut stream = ResponsesStream::new();
        let out = feed_all(
            &mut stream,
            &[
                json!({ "type": "response.in_progress", "response": { "id": "r" } }),
                json!({ "type": "response.output_item.added", "output_index": 0,
                        "item": { "type": "reasoning", "id": "rs_1", "summary": [] } }),
                json!({ "type": "response.reasoning_summary_text.delta", "delta": "thinking" }),
                json!({ "type": "response.content_part.added", "output_index": 1, "part": {} }),
                json!({ "type": "response.output_item.done", "output_index": 0,
                        "item": { "type": "reasoning", "id": "rs_1" } }),
                json!({ "type": "something.new" }),
                json!({}),
            ],
        );
        assert_eq!(out, ["start r"]);
        let mut stream = ResponsesStream::new();
        assert!(stream.feed("").unwrap().is_empty());
        assert!(stream.feed("[DONE]").unwrap().is_empty());
        assert!(!stream.is_complete(), "[DONE] is not a completion");
    }

    #[test]
    fn a_malformed_stream_is_a_parse_error() {
        let mut stream = ResponsesStream::new();
        assert!(stream.feed("not json").is_err());
        // A call without the id its answer must carry.
        let no_id = json!({ "type": "response.output_item.added", "output_index": 0,
                            "item": { "type": "function_call", "name": "t" } });
        assert!(stream.feed(&no_id.to_string()).is_err());
        let no_name = json!({ "type": "response.output_item.added", "output_index": 0,
                              "item": { "type": "function_call", "call_id": "c" } });
        assert!(stream.feed(&no_name.to_string()).is_err());
        // Arguments for a call nobody started.
        assert!(stream.feed(&args_delta(5, "{}").to_string()).is_err());
        assert!(stream.feed(&args_done(5, "{}").to_string()).is_err());
        // An item event with no item or no index.
        let no_item = json!({ "type": "response.output_item.added", "output_index": 0 });
        assert!(stream.feed(&no_item.to_string()).is_err());
        let no_index = json!({ "type": "response.output_item.added",
                               "item": { "type": "function_call", "call_id": "c", "name": "t" } });
        assert!(stream.feed(&no_index.to_string()).is_err());
    }

    #[test]
    fn a_request_starts_the_codecs_parser_afresh() {
        let codec = ResponsesCodec::new(ToolGrouping::Flat);
        let first = codec
            .parse_stream_chunk(&text_delta(0, 0, "cut here").to_string())
            .unwrap();
        assert_eq!(render(&first)[0], "start responses-msg");
        // Mid-stream, the same parser does not start again.
        let more = codec
            .parse_stream_chunk(&text_delta(0, 0, "more").to_string())
            .unwrap();
        assert_eq!(render(&more), ["text 0 \"more\""]);

        // The next request is a new response: it starts over, block 0 again.
        let messages: [Message; 0] = [];
        let request = MessagesRequest {
            model: "m",
            max_tokens: 1,
            system: Vec::new(),
            messages: &messages,
            tools: None,
            tool_choice: None,
            stream: true,
            output_config: None,
        };
        codec.serialize_request(&request);
        let again = codec
            .parse_stream_chunk(&text_delta(0, 0, "fresh").to_string())
            .unwrap();
        assert_eq!(
            render(&again),
            ["start responses-msg", "text# 0", "text 0 \"fresh\""]
        );
        // A clone has no response in flight either.
        let copy = codec.clone();
        let copied = copy
            .parse_stream_chunk(&text_delta(0, 0, "x").to_string())
            .unwrap();
        assert_eq!(render(&copied)[0], "start responses-msg");
    }

    #[test]
    fn the_one_shot_body_streams_and_stores_nothing() {
        let body = one_shot_body(
            "m",
            &["first", "second"],
            &[
                json!({ "role": "user", "content": "hi" }),
                json!({ "role": "assistant", "content": "hello" }),
                json!({ "role": "user", "content": "louder" }),
            ],
            None,
        );
        assert_eq!(body["instructions"], "first\n\nsecond");
        assert_eq!(body["store"], json!(false));
        assert_eq!(body["stream"], json!(true));
        assert_eq!(body["input"][1]["content"][0]["type"], "output_text");
        assert_eq!(body["input"][2]["content"][0]["text"], "louder");
        assert!(body.get("tools").is_none());
        let keys: Vec<&str> = body
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        for field in UNSUPPORTED_FIELDS {
            assert!(!keys.contains(field), "{field}");
        }
    }

    #[test]
    fn only_this_format_needs_a_terminal_event() {
        assert!(WireFormat::Responses.needs_terminal_event());
        assert!(!WireFormat::ChatCompletions.needs_terminal_event());
        assert!(!WireFormat::AnthropicMessages.needs_terminal_event());
    }

    /// For whoever maps the error to something typed (the `subscription_sharing_*`
    /// codes): the code is the `kind` and the param is a field, not only text.
    #[test]
    fn an_errors_code_and_param_are_fields_as_well_as_words() {
        let mut stream = ResponsesStream::new();
        let events = stream
            .feed(
                &json!({ "type": "response.failed", "response": { "error": {
                    "code": "subscription_sharing_unsupported_capability",
                    "message": "Unsupported.", "param": "tools" } } })
                .to_string(),
            )
            .unwrap();
        let Some(StreamEvent::Error { error }) = events.last() else {
            panic!("an error event: {events:?}");
        };
        assert_eq!(error.kind, "subscription_sharing_unsupported_capability");
        assert_eq!(error.param.as_deref(), Some("tools"));

        let events = stream
            .feed(&json!({ "type": "error", "message": "m" }).to_string())
            .unwrap();
        assert!(events.is_empty(), "the stream had ended");
        let mut stream = ResponsesStream::new();
        let events = stream
            .feed(&json!({ "type": "error", "code": "c", "message": "m" }).to_string())
            .unwrap();
        let Some(StreamEvent::Error { error }) = events.last() else {
            panic!("an error event: {events:?}");
        };
        assert_eq!(error.param, None);
    }

    #[test]
    fn the_codec_reports_the_usage_of_the_response_in_flight() {
        let codec = ResponsesCodec::new(ToolGrouping::Flat);
        assert_eq!(codec.usage(), None);
        codec
            .parse_stream_chunk(
                &json!({ "type": "response.completed", "response": {
                    "usage": { "input_tokens": 7, "output_tokens": 3 } } })
                .to_string(),
            )
            .unwrap();
        assert_eq!(
            codec.usage(),
            Some(Usage {
                input_tokens: 7,
                output_tokens: 3
            })
        );
        // The next request starts a new response, with none yet.
        let messages: [Message; 0] = [];
        codec.serialize_request(&MessagesRequest {
            model: "m",
            max_tokens: 1,
            system: Vec::new(),
            messages: &messages,
            tools: None,
            tool_choice: None,
            stream: true,
            output_config: None,
        });
        assert_eq!(codec.usage(), None);
    }

    // ------------------------------------------------------------------
    // Reasoning items (#474, PR 4)
    // ------------------------------------------------------------------

    fn reasoning_done(index: u64, encrypted: &str) -> Value {
        json!({
            "type": "response.output_item.done", "output_index": index,
            "item": { "type": "reasoning", "id": "rs_1", "summary": [], "encrypted_content": encrypted }
        })
    }

    /// The parser keeps a finished reasoning item as a block, in order, once,
    /// and only when asked to.
    #[test]
    fn a_reasoning_item_is_kept_as_a_block_when_asked_to() {
        let events = [
            reasoning_done(0, "ENC"),
            reasoning_done(0, "ENC"),
            call_added(1, "call_1", "echo"),
            args_done(1, "{}"),
            json!({ "type": "response.completed", "response": {} }),
        ];

        let mut off = ResponsesStream::new();
        let rendered = feed_all(&mut off, &events);
        assert!(
            rendered.iter().all(|l| !l.contains("RedactedThinking")),
            "{rendered:?}"
        );
        assert_eq!(rendered[1], "tool# 0 call_1 echo", "{rendered:?}");

        let mut on = ResponsesStream::with_reasoning(true);
        let mut kept = Vec::new();
        for event in &events {
            kept.extend(on.feed(&event.to_string()).expect("parses"));
        }
        let blocks: Vec<(u32, String)> = kept
            .iter()
            .filter_map(|e| match e {
                StreamEvent::ContentBlockStart {
                    index,
                    content_block: ContentBlockStart::RedactedThinking { data },
                } => Some((*index, data.clone())),
                _ => None,
            })
            .collect();
        assert_eq!(blocks.len(), 1, "a repeated item is kept once: {kept:?}");
        assert_eq!(blocks[0].0, 0, "before the call it led to");
        let item: Value = serde_json::from_str(&blocks[0].1).unwrap();
        assert_eq!(item["encrypted_content"], "ENC");
        // The call after it is the next block.
        assert!(kept.iter().any(|e| matches!(
            e,
            StreamEvent::ContentBlockStart {
                index: 1,
                content_block: ContentBlockStart::ToolUse { .. }
            }
        )));
    }

    fn assistant_with_reasoning() -> Vec<Message> {
        vec![
            Message {
                role: Role::User,
                content: vec![ContentBlock::Text { text: "go".into() }],
            },
            Message {
                role: Role::Assistant,
                content: vec![
                    ContentBlock::Text {
                        text: "Looking.".into(),
                    },
                    ContentBlock::RedactedThinking {
                        data: json!({ "type": "reasoning", "id": "rs_1", "summary": [],
                                      "encrypted_content": "ENC" })
                        .to_string(),
                    },
                    ContentBlock::ToolUse {
                        id: "call_1".into(),
                        name: "echo".into(),
                        input: json!({ "track": 1 }),
                    },
                ],
            },
            Message {
                role: Role::User,
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "call_1".into(),
                    content: "{}".into(),
                    is_error: None,
                }],
            },
        ]
    }

    fn body_for(messages: &[Message], reasoning: bool) -> Value {
        request_body_with(
            &MessagesRequest {
                model: "m",
                max_tokens: 1,
                system: Vec::new(),
                messages,
                tools: None,
                tool_choice: None,
                stream: true,
                output_config: None,
            },
            &ToolGrouping::Namespace("edytlab".into()),
            reasoning,
        )
    }

    /// The item goes back where it came from (after the words before it,
    /// ahead of the call), without the `id` of a response nothing stored, and
    /// the body asks for the encrypted content.
    #[test]
    fn a_kept_reasoning_item_is_echoed_in_place_without_its_id() {
        let body = body_for(&assistant_with_reasoning(), true);
        assert_eq!(body["include"], json!(["reasoning.encrypted_content"]));
        let kinds: Vec<&str> = body["input"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["type"].as_str().unwrap())
            .collect();
        assert_eq!(
            kinds,
            [
                "message",
                "message",
                "reasoning",
                "function_call",
                "function_call_output"
            ]
        );
        let item = &body["input"][2];
        assert_eq!(item["encrypted_content"], "ENC");
        assert!(item.get("id").is_none(), "{item}");
    }

    /// Off, nothing about reasoning is in the request.
    #[test]
    fn with_reasoning_off_the_body_has_neither_the_field_nor_the_items() {
        let body = body_for(&assistant_with_reasoning(), false);
        assert!(body.get("include").is_none(), "{body}");
        assert!(!body.to_string().contains("ENC"), "{body}");
        // The same body the format had before this existed.
        assert_eq!(
            body,
            request_body(
                &MessagesRequest {
                    model: "m",
                    max_tokens: 1,
                    system: Vec::new(),
                    messages: &assistant_with_reasoning(),
                    tools: None,
                    tool_choice: None,
                    stream: true,
                    output_config: None,
                },
                &ToolGrouping::Namespace("edytlab".into())
            )
        );
    }

    /// A `redacted_thinking` block that is not a reasoning item (Anthropic's
    /// own data in a history that came from another provider) is never sent.
    #[test]
    fn a_block_that_is_not_a_reasoning_item_is_not_echoed() {
        let mut messages = assistant_with_reasoning();
        messages[1].content[1] = ContentBlock::RedactedThinking {
            data: "opaque-anthropic-data".into(),
        };
        let body = body_for(&messages, true);
        assert!(
            !body.to_string().contains("opaque-anthropic-data"),
            "{body}"
        );
        messages[1].content[1] = ContentBlock::RedactedThinking {
            data: json!({ "type": "message", "id": "msg_1" }).to_string(),
        };
        let body = body_for(&messages, true);
        assert!(!body.to_string().contains("msg_1"), "{body}");
    }
}
