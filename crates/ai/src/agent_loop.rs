//! Agent tool-calling loop.
//!
//! Renamed from `loop.rs` because `loop` is a Rust keyword and using it
//! as a module name forces `r#loop` everywhere. The behaviour matches
//! the M10 spec.
//!
//! Per [`crate::Agent::turn`] we:
//! 1. Append the user's message to the conversation.
//! 2. Open a streaming Anthropic call (system prompt + tools cached).
//! 3. Forward `text` deltas to the caller's `on_event` sink in order.
//! 4. Reassemble each `tool_use` block, validate args via the
//!    dispatcher's compiled JSON Schema, invoke the tool synchronously,
//!    and append a `tool_result` block to the conversation.
//! 5. If at least one tool was used, loop. The hard cap of
//!    [`crate::prompt::MAX_TOOL_CALLS_PER_TURN`] applies across all
//!    iterations of the same turn.
//! 6. If the model emits a malformed `tool_use` (e.g. unparseable JSON
//!    args, or args that fail schema validation), we send back a
//!    `tool_result` with `is_error: true` and let the model retry once;
//!    if it errors a second time on the same tool call, the loop bails
//!    with [`crate::Error::ToolValidation`].
//!
//! `on_event` is `FnMut(AgentEvent)` and synchronous: this keeps the API
//! ergonomic for the Tauri command layer that just pushes each event
//! into a channel. The agent itself is `async` because the HTTP/SSE
//! work is.

use std::sync::{Arc, Mutex};

use eventsource_stream::Eventsource;
use futures_util::StreamExt;
use serde_json::Value;
use tools::{ToolContext, ToolDispatcher, ToolResult};

use crate::anthropic::{
    ApiError, CacheControl, ContentBlock, ContentBlockDelta, ContentBlockStart, Message,
    MessagesRequest, Role, StreamEvent, SystemBlock, ToolChoice,
};
use crate::prompt::{ANTHROPIC_VERSION, DEFAULT_MAX_TOKENS, MAX_TOOL_CALLS_PER_TURN};
use crate::{AgentEvent, AnthropicConfig, Error, Result, TurnResult};

/// Hard upper bound on the number of content blocks we'll allocate for
/// a single streaming Anthropic message. The server tells us each
/// block's index; without this cap a malicious or buggy server could
/// hand us `u64::MAX` and force a massive `Vec` allocation. Anthropic's
/// real tool-use messages have well under 10 blocks, so 100 is generous.
const MAX_CONTENT_BLOCKS: usize = 100;

/// Run a single agent turn. See [`crate::Agent::turn`] for behaviour.
///
/// This is a free function (rather than an `Agent` method) so the
/// borrow checker can hold a `&mut Vec<Message>` for the conversation
/// while keeping the dispatcher / store / engine in the
/// `Arc<Mutex<_>>`s the spec mandates. Mixing `&mut self` on `Agent`
/// with the mutex guards leads to awkward lifetimes.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn run_turn<F>(
    cfg: &AnthropicConfig,
    http: &reqwest::Client,
    dispatcher: &Arc<Mutex<ToolDispatcher>>,
    store: &Arc<Mutex<session::Store>>,
    engine: &Arc<Mutex<audio_engine::Engine>>,
    conversation: &mut Vec<Message>,
    user_message: String,
    mut on_event: F,
) -> Result<TurnResult>
where
    F: FnMut(AgentEvent),
{
    // 1. Push the user turn onto the running conversation.
    conversation.push(Message {
        role: Role::User,
        content: vec![ContentBlock::Text { text: user_message }],
    });

    let tool_schemas = {
        let d = dispatcher.lock().expect("dispatcher mutex poisoned");
        d.tool_schemas()
    };

    let mut total_tool_calls = 0usize;
    // Track consecutive validation failures *for the same call site* so
    // the model gets exactly one retry per tool_use before we bail.
    let mut consecutive_validation_errors = 0usize;
    let mut node_ids_emitted: Vec<session::NodeId> = Vec::new();
    // Accumulates assistant text across all loop iterations. The
    // "final" assistant text the user sees is the LAST iteration's
    // text (after all tool calls), but earlier iterations may also
    // emit text the caller streamed; we concatenate so `TurnResult`
    // matches the rebuilt-from-events reconstruction.
    let mut accumulated_text = String::new();

    loop {
        // 2. Build and send the streaming request.
        let request_body = build_request(cfg, &tool_schemas, conversation);
        let resp = http
            .post(format!("{}/v1/messages", cfg.base_url))
            .header("x-api-key", &cfg.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json")
            .json(&request_body)
            .send()
            .await
            .map_err(Error::Http)?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(Error::Api {
                status: status.as_u16(),
                message: body,
            });
        }

        // 3. Drain the SSE stream, accumulating text deltas + tool_use
        //    blocks. The Anthropic stream emits one `message_start`
        //    followed by a sequence of `content_block_*` events per
        //    block, then `message_delta` + `message_stop`.
        let stream = resp
            .bytes_stream()
            .map(|chunk| chunk.map_err(std::io::Error::other));
        let mut sse = stream.eventsource();

        // Per-message accumulators, indexed by content-block index.
        let mut blocks: Vec<PartialBlock> = Vec::new();
        let mut stop_reason: Option<String> = None;
        let mut text_this_message = String::new();

        while let Some(ev) = sse.next().await {
            let event = ev.map_err(Error::Sse)?;
            // `eventsource-stream` skips comment lines and reassembles
            // multi-line `data:` payloads automatically. An empty data
            // payload (server-side keepalive) is ignored.
            if event.data.is_empty() {
                continue;
            }
            let parsed: StreamEvent = serde_json::from_str(&event.data).map_err(Error::Json)?;
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
                            blocks[index as usize] = PartialBlock::Text(String::new());
                        }
                        ContentBlockStart::ToolUse { id, name, .. } => {
                            on_event(AgentEvent::ToolCallStart {
                                name: name.clone(),
                                id: id.clone(),
                            });
                            blocks[index as usize] = PartialBlock::ToolUse {
                                id,
                                name,
                                args_json: String::new(),
                            };
                        }
                        ContentBlockStart::Other => {
                            blocks[index as usize] = PartialBlock::Ignored;
                        }
                    }
                }
                StreamEvent::ContentBlockDelta { index, delta } => {
                    let slot = blocks
                        .get_mut(index as usize)
                        .ok_or_else(|| Error::Protocol("delta for unknown block index".into()))?;
                    match (slot, delta) {
                        (PartialBlock::Text(buf), ContentBlockDelta::TextDelta { text }) => {
                            buf.push_str(&text);
                            text_this_message.push_str(&text);
                            on_event(AgentEvent::TextDelta(text));
                        }
                        (
                            PartialBlock::ToolUse { args_json, .. },
                            ContentBlockDelta::InputJsonDelta { partial_json },
                        ) => {
                            args_json.push_str(&partial_json);
                        }
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
                StreamEvent::MessageStop => break,
                StreamEvent::Ping | StreamEvent::Other => {}
                StreamEvent::Error { error } => {
                    return Err(Error::ApiStream(error_message(&error)));
                }
            }
        }

        // 4. Append the assistant turn (with all its blocks) to history
        //    so the next API call sees it.
        let assistant_blocks: Vec<ContentBlock> = blocks
            .iter()
            .filter_map(|b| match b {
                PartialBlock::Text(t) if !t.is_empty() => {
                    Some(ContentBlock::Text { text: t.clone() })
                }
                PartialBlock::ToolUse {
                    id,
                    name,
                    args_json,
                } => Some(ContentBlock::ToolUse {
                    id: id.clone(),
                    name: name.clone(),
                    // We re-serialise the parsed args (or empty object)
                    // so the conversation stores valid JSON regardless
                    // of what the model streamed.
                    input: serde_json::from_str::<Value>(args_json)
                        .unwrap_or_else(|_| Value::Object(Default::default())),
                }),
                _ => None,
            })
            .collect();
        if !assistant_blocks.is_empty() {
            conversation.push(Message {
                role: Role::Assistant,
                content: assistant_blocks,
            });
        }
        accumulated_text.push_str(&text_this_message);

        // 5. If the model used tools, dispatch them and iterate.
        let tool_uses: Vec<(String, String, String)> = blocks
            .iter()
            .filter_map(|b| match b {
                PartialBlock::ToolUse {
                    id,
                    name,
                    args_json,
                } => Some((id.clone(), name.clone(), args_json.clone())),
                _ => None,
            })
            .collect();

        if tool_uses.is_empty() {
            on_event(AgentEvent::Done);
            return Ok(TurnResult {
                text: accumulated_text,
                stop_reason,
                node_ids: node_ids_emitted,
            });
        }

        // The cap is enforced *before* dispatching this batch, so a
        // model that requests 11 tools in a single turn never gets the
        // 11th invoked. Checking the whole batch up-front (rather than
        // per-call inside the loop) keeps the conversation history
        // consistent: if the batch would push us over the cap we bail
        // *before* appending any tool_result blocks, so we never end up
        // with an assistant tool_use missing its matching tool_result.
        if total_tool_calls + tool_uses.len() > MAX_TOOL_CALLS_PER_TURN {
            return Err(Error::ToolBudgetExceeded(MAX_TOOL_CALLS_PER_TURN));
        }
        let mut tool_results: Vec<ContentBlock> = Vec::with_capacity(tool_uses.len());
        for (id, name, args_json) in tool_uses {
            total_tool_calls += 1;

            let args: Value = match serde_json::from_str(&args_json) {
                Ok(v) => v,
                Err(e) => {
                    consecutive_validation_errors += 1;
                    if consecutive_validation_errors > 1 {
                        return Err(Error::ToolValidation(format!(
                            "tool {name} args malformed twice; bailing: {e}"
                        )));
                    }
                    on_event(AgentEvent::ToolCallEnd {
                        id: id.clone(),
                        ok: false,
                    });
                    tool_results.push(ContentBlock::ToolResult {
                        tool_use_id: id,
                        content: format!(
                            "tool args were not valid JSON: {e}. retry once with a valid JSON object."
                        ),
                        is_error: Some(true),
                    });
                    continue;
                }
            };

            // Dispatch under a single lock acquisition. The dispatcher
            // holds tool implementations + compiled schema validators.
            let result = {
                let d = dispatcher.lock().expect("dispatcher mutex poisoned");
                let mut store_g = store.lock().expect("store mutex poisoned");
                let mut engine_g = engine.lock().expect("engine mutex poisoned");
                let mut ctx = ToolContext {
                    store: &mut store_g,
                    engine: &mut engine_g,
                };
                d.invoke(&name, args, &mut ctx)
            };

            match result {
                Err(dispatch_err) => {
                    // Schema validation failure or unknown tool. Surface
                    // to the model as a tool_result error and let it
                    // retry once.
                    consecutive_validation_errors += 1;
                    let is_unrecoverable = consecutive_validation_errors > 1;
                    on_event(AgentEvent::ToolCallEnd {
                        id: id.clone(),
                        ok: false,
                    });
                    if is_unrecoverable {
                        return Err(Error::ToolValidation(format!(
                            "tool {name} failed validation twice: {dispatch_err}"
                        )));
                    }
                    tool_results.push(ContentBlock::ToolResult {
                        tool_use_id: id,
                        content: format!("{dispatch_err}"),
                        is_error: Some(true),
                    });
                }
                Ok(ToolResult::Ok(value)) => {
                    consecutive_validation_errors = 0;
                    if let Some(node_id) = extract_node_id(&value) {
                        on_event(AgentEvent::NodeCreated(node_id));
                        node_ids_emitted.push(node_id);
                    }
                    on_event(AgentEvent::ToolCallEnd {
                        id: id.clone(),
                        ok: true,
                    });
                    tool_results.push(ContentBlock::ToolResult {
                        tool_use_id: id,
                        content: serde_json::to_string(&value).unwrap_or_else(|_| "{}".to_string()),
                        is_error: None,
                    });
                }
                Ok(ToolResult::Error(msg)) => {
                    consecutive_validation_errors = 0;
                    on_event(AgentEvent::ToolCallEnd {
                        id: id.clone(),
                        ok: false,
                    });
                    tool_results.push(ContentBlock::ToolResult {
                        tool_use_id: id,
                        content: msg,
                        is_error: Some(true),
                    });
                }
            }
        }

        // The user-role message carrying tool_result blocks: per
        // Anthropic's conversation shape, tool_result lives in a `user`
        // message that immediately follows the assistant's tool_use.
        conversation.push(Message {
            role: Role::User,
            content: tool_results,
        });
        // Iterate: the next round-trip lets the model react to the
        // tool results.
    }
}

/// Build the outgoing request. Keeps the system prompt + tool schemas
/// `cache_control: ephemeral` so they're cached server-side across the
/// many round trips a tool-using turn makes.
fn build_request<'a>(
    cfg: &'a AnthropicConfig,
    tool_schemas: &Value,
    conversation: &'a [Message],
) -> MessagesRequest<'a> {
    MessagesRequest {
        model: &cfg.model,
        max_tokens: DEFAULT_MAX_TOKENS,
        system: vec![SystemBlock {
            kind: "text",
            text: crate::prompt::SYSTEM_PROMPT,
            cache_control: Some(CacheControl::EPHEMERAL),
        }],
        messages: conversation,
        tools: Some(attach_cache_control_to_tools(tool_schemas.clone())),
        tool_choice: Some(ToolChoice::AUTO),
        stream: true,
    }
}

/// Decorate the LAST tool entry with `cache_control: ephemeral`. The
/// Anthropic cache key extends from the start of the request through
/// the final block tagged `cache_control`, so marking the last tool
/// covers the prompt + entire tool list as one cacheable prefix.
fn attach_cache_control_to_tools(mut tools: Value) -> Value {
    if let Some(arr) = tools.as_array_mut() {
        if let Some(last) = arr.last_mut() {
            if let Some(obj) = last.as_object_mut() {
                obj.insert(
                    "cache_control".to_string(),
                    serde_json::json!({ "type": "ephemeral" }),
                );
            }
        }
    }
    tools
}

fn error_message(err: &ApiError) -> String {
    err.message.clone()
}

fn grow_to(v: &mut Vec<PartialBlock>, idx: usize) -> Result<()> {
    if idx >= MAX_CONTENT_BLOCKS {
        return Err(Error::Protocol(format!(
            "content_block_index {idx} exceeds MAX_CONTENT_BLOCKS ({MAX_CONTENT_BLOCKS})"
        )));
    }
    while v.len() <= idx {
        v.push(PartialBlock::Pending);
    }
    Ok(())
}

/// Tool results commonly include `node_id` (a hex string). We surface
/// these as [`AgentEvent::NodeCreated`] for the UI without forcing the
/// caller to re-parse tool output.
fn extract_node_id(value: &Value) -> Option<session::NodeId> {
    value
        .get("node_id")
        .and_then(|v| v.as_str())
        .and_then(|s| session::NodeId::from_hex(s).ok())
}

/// Per-block accumulator. Index in the block array matches the
/// streaming server's `index` field.
#[derive(Debug)]
enum PartialBlock {
    /// `content_block_start` arrived for an index we don't model, so
    /// subsequent deltas for it are silently dropped.
    Pending,
    Ignored,
    Text(String),
    ToolUse {
        id: String,
        name: String,
        args_json: String,
    },
}
