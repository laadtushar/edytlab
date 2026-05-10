//! Agent tool-calling loop.
//!
//! Renamed from `loop.rs` because `loop` is a Rust keyword and using it
//! as a module name forces `r#loop` everywhere. The behaviour matches
//! the M10 spec, extended in M27 with:
//!
//! * Mode detection: a cheap Haiku call classifies each user message as
//!   `mashup`, `mix`, `voice`, or `general`.
//! * Plan gate (mashup mode only): the loop emits a `Plan` event and
//!   suspends until the frontend approves by calling `Agent::approve_plan`.
//!
//! Per [`crate::Agent::turn`] we:
//! 1. Classify the user message.
//! 2. If mashup mode, request a `<plan>` from the model and gate on
//!    frontend approval before proceeding.
//! 3. Append the user's message to the conversation.
//! 4. Open a streaming Anthropic call (system prompt + tools cached).
//! 5. Forward `text` deltas to the caller's `on_event` sink in order.
//! 6. Reassemble each `tool_use` block, validate args via the
//!    dispatcher's compiled JSON Schema, invoke the tool synchronously,
//!    and append a `tool_result` block to the conversation.
//! 7. If at least one tool was used, loop. The hard cap of
//!    [`crate::prompt::MAX_TOOL_CALLS_PER_TURN`] applies across all
//!    iterations of the same turn.
//! 8. If the model emits a malformed `tool_use` (e.g. unparseable JSON
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
use std::time::Duration;

use tokio::sync::Notify;

use eventsource_stream::Eventsource;
use futures_util::StreamExt;
use serde_json::Value;
use tools::{ToolContext, ToolDispatcher, ToolResult};

use crate::anthropic::{
    ApiError, CacheControl, ContentBlock, ContentBlockDelta, ContentBlockStart, Message,
    MessagesRequest, Role, StreamEvent, SystemBlock, ToolChoice,
};
use crate::prompt::{DEFAULT_MAX_TOKENS, MAX_TOOL_CALLS_PER_TURN};
use crate::session_context::{render_block, SessionContext};
use crate::{AgentEvent, Error, LlmConfig, Result, TurnResult};

// ---------------------------------------------------------------------------
// Mode detection (M27)
// ---------------------------------------------------------------------------

/// Conversation mode as classified by the cheap Haiku call.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Mode {
    Mashup,
    Mix,
    Voice,
    General,
}

/// Classify the user's request using a cheap single-turn call to
/// `claude-haiku-4-5-20251001`. Passes recent conversation history for
/// context (last 6 messages) so follow-up messages ("actually, change
/// the BPM") classify correctly. Falls back to `Mode::General` on any
/// error so classification failures are never user-visible.
pub(crate) async fn classify_mode(
    cfg: &LlmConfig,
    http: &reqwest::Client,
    user_message: &str,
    conversation: &[Message],
) -> Mode {
    let system_text = "Classify the user's request as one word: mashup, mix, voice, or general. Output only the single word.";

    // Include the last 6 conversation messages for context, then the new
    // user message so the classifier sees the full intent.
    let mut messages: Vec<serde_json::Value> = conversation
        .iter()
        .rev()
        .take(6)
        .rev()
        .map(|m| {
            serde_json::json!({
                "role": match m.role { Role::User => "user", Role::Assistant => "assistant" },
                "content": m.content.iter().filter_map(|b| {
                    if let ContentBlock::Text { text } = b { Some(text.as_str()) } else { None }
                }).collect::<Vec<_>>().join(" ")
            })
        })
        .collect();
    messages.push(serde_json::json!({ "role": "user", "content": user_message }));

    // Build a provider-shaped non-streaming body. OpenAI's chat-completions
    // returns `choices[0].message.content`; Anthropic returns
    // `content[0].text`; we handle both shapes after we get the response.
    let request_body = if cfg.provider.id() == crate::OPENAI_ID {
        serde_json::json!({
            "model": cfg.wire_classifier_model(),
            "max_completion_tokens": 10,
            "messages": std::iter::once(serde_json::json!({"role":"system","content":system_text}))
                .chain(messages.iter().cloned())
                .collect::<Vec<_>>(),
            "stream": false
        })
    } else {
        serde_json::json!({
            "model": cfg.wire_classifier_model(),
            "max_tokens": 10,
            "system": system_text,
            "messages": messages,
            "stream": false
        })
    };

    let req = http.post(format!(
        "{}{}",
        cfg.base_url(),
        cfg.provider.endpoint_path()
    ));
    let req = cfg.provider.apply_auth(req, &cfg.api_key);
    let resp = match req.json(&request_body).send().await {
        Ok(r) => r,
        Err(_) => return Mode::General,
    };

    if !resp.status().is_success() {
        return Mode::General;
    }

    let body: Value = match resp.json().await {
        Ok(v) => v,
        Err(_) => return Mode::General,
    };

    let text = extract_response_text(cfg, &body)
        .unwrap_or_default()
        .trim()
        .to_lowercase();

    if text.contains("mashup") {
        Mode::Mashup
    } else if text.contains("mix") {
        Mode::Mix
    } else if text.contains("voice") {
        Mode::Voice
    } else {
        Mode::General
    }
}

/// Return the appropriate system prompt for the detected mode.
fn select_system_prompt(mode: Mode) -> &'static str {
    match mode {
        Mode::Mashup => include_str!("../prompts/mashup_mode.md"),
        _ => include_str!("../prompts/system.md"),
    }
}

// ---------------------------------------------------------------------------
// Plan parsing helpers (M27)
// ---------------------------------------------------------------------------

/// Extract the JSON array from a `<plan>[...]</plan>` block and
/// deserialise it. Returns `None` if the block is missing or malformed.
pub(crate) fn parse_plan(text: &str) -> Option<Vec<Value>> {
    let start = text.find("<plan>")?;
    let end = text.find("</plan>")?;
    if end <= start {
        return None;
    }
    let inner = text[start + "<plan>".len()..end].trim();
    serde_json::from_str(inner).ok()
}

/// Request a plan from the model in a single non-streaming call and
/// return the parsed steps. Includes conversation history so follow-up
/// requests can be planned in context. Returns `None` on failure.
async fn fetch_plan(
    cfg: &LlmConfig,
    http: &reqwest::Client,
    system_prompt: &str,
    conversation: &[Message],
    user_message: &str,
) -> Option<Vec<Value>> {
    let mut messages: Vec<serde_json::Value> = conversation
        .iter()
        .map(|m| {
            serde_json::json!({
                "role": match m.role { Role::User => "user", Role::Assistant => "assistant" },
                "content": m.content.iter().filter_map(|b| {
                    if let ContentBlock::Text { text } = b { Some(text.as_str()) } else { None }
                }).collect::<Vec<_>>().join(" ")
            })
        })
        .collect();
    messages.push(serde_json::json!({ "role": "user", "content": user_message }));

    let plan_instruction =
        "Output only a <plan>...</plan> XML block listing the steps as JSON. No other text.";
    let request_body = if cfg.provider.id() == crate::OPENAI_ID {
        let combined_system = format!("{system_prompt}\n\n{plan_instruction}");
        serde_json::json!({
            "model": cfg.wire_model(),
            "max_completion_tokens": 1024,
            "messages": std::iter::once(serde_json::json!({"role":"system","content":combined_system}))
                .chain(messages.iter().cloned())
                .collect::<Vec<_>>(),
            "stream": false
        })
    } else {
        serde_json::json!({
            "model": cfg.wire_model(),
            "max_tokens": 1024,
            "system": [
                { "type": "text", "text": system_prompt },
                { "type": "text", "text": plan_instruction }
            ],
            "messages": messages,
            "stream": false
        })
    };

    let req = http.post(format!(
        "{}{}",
        cfg.base_url(),
        cfg.provider.endpoint_path()
    ));
    let req = cfg.provider.apply_auth(req, &cfg.api_key);
    let resp = req.json(&request_body).send().await.ok()?;

    if !resp.status().is_success() {
        return None;
    }

    let body: Value = resp.json().await.ok()?;
    let text = extract_response_text(cfg, &body)?;

    parse_plan(&text)
}

/// Wait for the frontend to approve the pending plan. Uses
/// `tokio::sync::Notify` so the loop wakes immediately when the user
/// clicks "Run" with zero polling overhead. Times out after 5 minutes.
///
/// The notifier is stored in `AppState` (not behind the agent Mutex),
/// so the `approve_plan` Tauri command can fire it without holding any
/// lock that `send_message` also holds, eliminating the deadlock.
async fn await_plan_approval(notify: &Arc<Notify>) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(300), notify.notified())
        .await
        .map_err(|_| Error::PlanTimeout)
}

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
    cfg: &LlmConfig,
    http: &reqwest::Client,
    dispatcher: &Arc<Mutex<ToolDispatcher>>,
    store: &Arc<Mutex<session::Store>>,
    engine: &Arc<Mutex<audio_engine::Engine>>,
    clipboard: &Arc<Mutex<Option<Vec<f32>>>>,
    conversation: &mut Vec<Message>,
    plan_notify: &Arc<Notify>,
    user_message: String,
    session_ctx: Option<&SessionContext>,
    mut on_event: F,
) -> Result<TurnResult>
where
    F: FnMut(AgentEvent),
{
    // M27: classify the user message to select the system prompt and
    // decide whether to gate on plan approval.
    let mode = classify_mode(cfg, http, &user_message, conversation).await;
    let base_prompt = select_system_prompt(mode);

    // Splice the per-turn session context (selection + markers) into
    // the system prompt. `render_block` returns "" for an empty context
    // so the splice is a no-op when the frontend hasn't pushed anything.
    let ctx_block = session_ctx.map(render_block).unwrap_or_default();
    let combined_prompt = if ctx_block.is_empty() {
        base_prompt.to_string()
    } else {
        format!("{base_prompt}\n\n{ctx_block}")
    };
    let system_prompt: &str = &combined_prompt;

    // M27: if mashup mode, request a plan from the model and wait for
    // the frontend to approve before executing any tools.
    if mode == Mode::Mashup {
        if let Some(steps) = fetch_plan(cfg, http, system_prompt, conversation, &user_message).await
        {
            on_event(AgentEvent::Plan {
                steps: steps.clone(),
            });
            // Block until the frontend fires plan_notify via the
            // `approve_plan` command, or time out after 5 minutes.
            await_plan_approval(plan_notify).await?;
        }
        // If fetch_plan returns None (e.g. parse failure) we continue
        // without gating — graceful degradation.
    }

    // 1. Push the user turn onto the running conversation.
    // Save a copy before `user_message` is consumed by the ContentBlock move.
    let user_msg_saved = user_message.clone();
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
        let wire_model = cfg.wire_model();
        let request_struct = build_request(&wire_model, system_prompt, &tool_schemas, conversation);
        // Provider-specific wire serialisation. Anthropic + OpenRouter
        // pass `MessagesRequest` through verbatim; OpenAI translates to
        // chat-completions JSON.
        let request_body = cfg.provider.serialize_request(&request_struct);
        let req = http.post(format!(
            "{}{}",
            cfg.base_url(),
            cfg.provider.endpoint_path()
        ));
        let req = cfg.provider.apply_auth(req, &cfg.api_key);
        let resp = req.json(&request_body).send().await.map_err(Error::Http)?;

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
                        let slot = blocks.get_mut(index as usize).ok_or_else(|| {
                            Error::Protocol("delta for unknown block index".into())
                        })?;
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
                    StreamEvent::MessageStop => {
                        stream_finished = true;
                        break;
                    }
                    StreamEvent::Ping | StreamEvent::Other => {}
                    StreamEvent::Error { error } => {
                        return Err(Error::ApiStream(error_message(&error)));
                    }
                }
            }
            if stream_finished {
                break;
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
                let mut clipboard_g = clipboard.lock().expect("clipboard mutex poisoned");
                let mut ctx = ToolContext {
                    store: &mut store_g,
                    engine: &mut engine_g,
                    user_message: &user_msg_saved,
                    clipboard: &mut clipboard_g,
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
///
/// `wire_model` is the provider-translated model id (Anthropic uses the
/// canonical id as-is; OpenRouter prepends `anthropic/`). The caller
/// computes it once per turn iteration via [`LlmConfig::wire_model`].
fn build_request<'a>(
    wire_model: &'a str,
    system_prompt: &'a str,
    tool_schemas: &Value,
    conversation: &'a [Message],
) -> MessagesRequest<'a> {
    MessagesRequest {
        model: wire_model,
        max_tokens: DEFAULT_MAX_TOKENS,
        system: vec![SystemBlock {
            kind: "text",
            text: system_prompt,
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

/// Extract the assistant's text reply from a non-streaming response
/// body, handling both Anthropic-shape (`content[0].text`) and
/// OpenAI-shape (`choices[0].message.content`).
fn extract_response_text(cfg: &LlmConfig, body: &Value) -> Option<String> {
    if cfg.provider.id() == crate::OPENAI_ID {
        body.get("choices")
            .and_then(|c| c.as_array())
            .and_then(|a| a.first())
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|t| t.as_str())
            .map(|s| s.to_string())
    } else {
        body.get("content")
            .and_then(|c| c.as_array())
            .and_then(|arr| arr.first())
            .and_then(|b| b.get("text"))
            .and_then(|t| t.as_str())
            .map(|s| s.to_string())
    }
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

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ------------------------------------------------------------------
    // parse_plan
    // ------------------------------------------------------------------

    #[test]
    fn parse_plan_extracts_steps_from_valid_block() {
        let input = r#"<plan>
[
  {"step": 1, "tool": "analyze_track", "description": "Analyse A BPM"},
  {"step": 2, "tool": "separate_stems", "description": "Separate stems"}
]
</plan>"#;
        let steps = parse_plan(input).expect("should parse");
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].get("step").and_then(|v| v.as_u64()), Some(1));
        assert_eq!(
            steps[0].get("tool").and_then(|v| v.as_str()),
            Some("analyze_track")
        );
        assert_eq!(steps[1].get("step").and_then(|v| v.as_u64()), Some(2));
    }

    #[test]
    fn parse_plan_returns_none_when_no_block() {
        assert!(parse_plan("some text without plan tags").is_none());
    }

    #[test]
    fn parse_plan_returns_none_for_malformed_json() {
        let input = "<plan>not json at all</plan>";
        assert!(parse_plan(input).is_none());
    }

    #[test]
    fn parse_plan_handles_surrounding_text() {
        let input = r#"Here is your plan:
<plan>[{"step": 1, "tool": "analyze_track", "description": "BPM check"}]</plan>
No other text."#;
        let steps = parse_plan(input).expect("should parse");
        assert_eq!(steps.len(), 1);
        assert_eq!(
            steps[0].get("description").and_then(|v| v.as_str()),
            Some("BPM check")
        );
    }

    #[test]
    fn parse_plan_single_step_array() {
        let input =
            "<plan>[{\"step\":1,\"tool\":\"render_final\",\"description\":\"Render\"}]</plan>";
        let steps = parse_plan(input).expect("should parse");
        assert_eq!(steps.len(), 1);
    }

    // ------------------------------------------------------------------
    // classify_mode (live API — marked #[ignore] for CI)
    // ------------------------------------------------------------------

    /// This test requires a real ANTHROPIC_API_KEY and makes a network
    /// call. Run manually with:
    ///   ANTHROPIC_API_KEY=sk-... cargo test -p ai -- --ignored
    #[tokio::test]
    #[ignore = "requires live Anthropic API key"]
    async fn classify_mode_returns_mashup_for_mashup_request() {
        let key = std::env::var("ANTHROPIC_API_KEY").expect("ANTHROPIC_API_KEY must be set");
        let cfg = crate::LlmConfig::new_anthropic(key);
        let http = reqwest::Client::new();
        let mode = classify_mode(&cfg, &http, "make a mashup of these two tracks", &[]).await;
        assert_eq!(
            mode,
            Mode::Mashup,
            "expected Mashup mode for mashup request"
        );
    }

    // ------------------------------------------------------------------
    // Mode selection
    // ------------------------------------------------------------------

    #[test]
    fn select_system_prompt_returns_mashup_prompt_for_mashup_mode() {
        let prompt = select_system_prompt(Mode::Mashup);
        assert!(
            prompt.contains("Mashup Mode"),
            "expected mashup prompt; got: {prompt:.80}"
        );
    }

    #[test]
    fn select_system_prompt_returns_default_for_general_mode() {
        let prompt = select_system_prompt(Mode::General);
        // system.md should NOT contain the mashup header
        assert!(
            !prompt.contains("Mashup Mode"),
            "expected default system prompt; got mashup prompt"
        );
    }

    // ------------------------------------------------------------------
    // Misc: json round-trip through parse_plan
    // ------------------------------------------------------------------

    #[test]
    fn parse_plan_values_are_objects_not_nulls() {
        let steps = vec![
            json!({"step": 1, "tool": "analyze_track", "description": "A BPM"}),
            json!({"step": 2, "tool": "time_stretch", "description": "Stretch B"}),
        ];
        let serialised = serde_json::to_string(&steps).unwrap();
        let wrapped = format!("<plan>{serialised}</plan>");
        let parsed = parse_plan(&wrapped).expect("round-trip must succeed");
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0]["tool"], json!("analyze_track"));
        assert_eq!(parsed[1]["description"], json!("Stretch B"));
    }
}
