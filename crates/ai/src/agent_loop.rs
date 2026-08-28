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
                "content": message_text(m),
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

/// Concatenate the per-turn system prompt fragments in canonical
/// order: base prompt → skills (matched, alphabetical) → memory
/// (global, project) → session context. Empty fragments are omitted
/// cleanly so a single section never produces a leading or trailing
/// double newline. Extracted as a free function so the ordering
/// invariant can be unit-tested without booting `run_turn`.
pub(crate) fn assemble_system_prompt(
    base: &str,
    profile_block: &str,
    skills_block: &str,
    memory_block: &str,
    ctx_block: &str,
) -> String {
    let mut out = base.to_string();
    for fragment in [profile_block, skills_block, memory_block, ctx_block] {
        if !fragment.is_empty() {
            out.push_str("\n\n");
            out.push_str(fragment);
        }
    }
    out
}

/// Flatten a message's content blocks into a single space-joined
/// string of its text blocks. Used by classify_mode / fetch_plan /
/// run_turn — keeps the three call sites consistent and avoids
/// silently dropping a block kind one path knows about and another
/// doesn't.
pub(crate) fn message_text(m: &Message) -> String {
    m.content
        .iter()
        .filter_map(|b| match b {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Stringified conversation mode for the skill trigger context. Kept
/// stable so frontmatter `modes: […]` matchers can rely on the same
/// labels we surface elsewhere.
/// Keep only schemas whose `name` is in `whitelist`. An empty
/// whitelist hides every tool — that's the deliberate "no tools
/// for this profile" case. A `null` whitelist (i.e. `None` from the
/// caller) means "all tools" and is handled at the call site.
pub(crate) fn filter_tool_schemas(schemas: Value, whitelist: &[String]) -> Value {
    let arr = match schemas.as_array() {
        Some(a) => a,
        None => return schemas,
    };
    let kept: Vec<Value> = arr
        .iter()
        .filter(|s| {
            s.get("name")
                .and_then(|v| v.as_str())
                .map(|n| whitelist.iter().any(|w| w == n))
                .unwrap_or(false)
        })
        .cloned()
        .collect();
    Value::Array(kept)
}

fn mode_as_str(mode: Mode) -> &'static str {
    match mode {
        Mode::General => "general",
        Mode::Mashup => "mashup",
        Mode::Mix => "mix",
        Mode::Voice => "voice",
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
                "content": message_text(m),
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
/// Block until the frontend answers the plan gate.
///
/// There used to be exactly two ways out: approve, or wait five minutes.
/// A user who disliked the plan had no way to say so, which made the
/// gate feel like a trap rather than a checkpoint. `rejected` is set by
/// the `reject_plan` command before it fires the same notifier, so a
/// rejection is a normal answer rather than a timeout.
async fn await_plan_approval(
    notify: &Arc<Notify>,
    rejected: &Arc<std::sync::atomic::AtomicBool>,
) -> Result<bool> {
    tokio::time::timeout(Duration::from_secs(300), notify.notified())
        .await
        .map_err(|_| Error::PlanTimeout)?;
    Ok(rejected.swap(false, std::sync::atomic::Ordering::SeqCst))
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
    plan_steps_override: &Arc<std::sync::Mutex<Option<String>>>,
    plan_rejected: &Arc<std::sync::atomic::AtomicBool>,
    plan_first: bool,
    user_message: String,
    session_ctx: Option<&SessionContext>,
    memory_store: Option<&memory::MemoryStore>,
    skill_library: Option<&Mutex<skills::SkillLibrary>>,
    profile_body: Option<&str>,
    tool_whitelist: Option<&[String]>,
    mut on_event: F,
) -> Result<TurnResult>
where
    F: FnMut(AgentEvent),
{
    // M27: classify the user message to select the system prompt and
    // decide whether to gate on plan approval.
    let mode = classify_mode(cfg, http, &user_message, conversation).await;
    let base_prompt = select_system_prompt(mode);

    let memory_block = memory_store.map(|m| m.render()).unwrap_or_default();
    let skills_block = skill_library
        .map(|lib| {
            // Pull prior user turns out of `conversation` to form the
            // history haystack. Skills stay sticky across follow-up
            // turns even when the trigger keyword isn't repeated.
            let history: Vec<String> = conversation
                .iter()
                .filter(|m| m.role == Role::User)
                .map(message_text)
                .collect();
            let ctx = skills::TriggerContext {
                user_message: &user_message,
                history: &history,
                mode: Some(mode_as_str(mode)),
            };
            let guard = lib.lock().expect("skill library mutex poisoned");
            guard.render(&ctx)
        })
        .unwrap_or_default();
    let ctx_block = session_ctx.map(render_block).unwrap_or_default();
    let profile_block = profile_body
        .map(|b| {
            let defanged = b.replace("</agent-profile", "</\u{200B}agent-profile");
            format!("<agent-profile>\n{}\n</agent-profile>", defanged.trim_end())
        })
        .unwrap_or_default();
    let combined_prompt = assemble_system_prompt(
        base_prompt,
        &profile_block,
        &skills_block,
        &memory_block,
        &ctx_block,
    );
    let system_prompt: &str = &combined_prompt;

    // Plan first when the user asked for it, or when the request
    // classified as a mashup — those are historically plan-gated and
    // stay that way.
    //
    // The gate was previously reachable *only* through `Mode::Mashup`,
    // so whether you got a plan depended on how a classifier read your
    // sentence. From outside that is indistinguishable from arbitrary:
    // the same phrasing sometimes planned and sometimes just acted, with
    // nothing to explain why. `plan_first` makes it a choice.
    let mut step_override: Option<String> = None;
    if plan_first || mode == Mode::Mashup {
        if let Some(steps) = fetch_plan(cfg, http, system_prompt, conversation, &user_message).await
        {
            on_event(AgentEvent::Plan {
                steps: steps.clone(),
            });
            // Block until the frontend answers via `approve_plan` or
            // `reject_plan`, or time out after 5 minutes.
            if await_plan_approval(plan_notify, plan_rejected).await? {
                on_event(AgentEvent::PlanRejected);
                return Ok(TurnResult::default());
            }
            // Consume any step overrides the frontend stored before
            // firing the notifier.
            step_override = plan_steps_override
                .lock()
                .expect("plan_steps_override mutex poisoned")
                .take();
        }
        // If fetch_plan returns None (e.g. parse failure) we continue
        // without gating — graceful degradation.
    }

    // 1. Push the user turn onto the running conversation.
    // Save a copy before `user_message` is consumed by the ContentBlock move.
    let user_msg_saved = user_message.clone();
    // If the user edited the plan steps before approving, merge the override
    // into the same user message to avoid consecutive Role::User turns, which
    // the Anthropic API rejects with 400 Bad Request.
    let user_text = if let Some(override_text) = step_override {
        format!("{user_message}\n\n{override_text}")
    } else {
        user_message
    };
    conversation.push(Message {
        role: Role::User,
        content: vec![ContentBlock::Text { text: user_text }],
    });

    let tool_schemas = {
        let d = dispatcher.lock().expect("dispatcher mutex poisoned");
        let all = d.tool_schemas();
        match tool_whitelist {
            Some(whitelist) => filter_tool_schemas(all, whitelist),
            None => all,
        }
    };

    // The same whitelist, as a set, for the dispatch-time check (#238).
    //
    // Trimming the schema list is a hint: it tells a well-behaved model
    // what to ask for. It is not a control. A model on an
    // OpenAI-compatible or Ollama endpoint that names a filtered-out
    // tool anyway used to get it executed, and meta-tools that build
    // their own dispatcher bypassed the restriction outright — so
    // unticking `render_final` did not stop `batch_apply` from calling
    // it with an unconstrained absolute `out_path`.
    let allowed_tools: Option<std::collections::HashSet<String>> =
        tool_whitelist.map(|w| w.iter().cloned().collect());

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
                        view: None,
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
                    allowed_tools: allowed_tools.as_ref(),
                };
                d.invoke(&name, args, &mut ctx)
            };

            match result {
                Err(tools::DispatchError::NotPermitted(tool)) => {
                    // A disabled capability, not a malformed call (#238).
                    //
                    // Deliberately outside the validation-retry budget:
                    // that budget exists because a model repeating the
                    // *same* bad arguments will not fix itself, and two
                    // strikes ends the turn. A refusal is different —
                    // the right response is to choose another tool, and
                    // a model that tried two disabled ones would
                    // otherwise have the whole turn aborted. The
                    // per-turn call budget still bounds a model that
                    // keeps asking.
                    on_event(AgentEvent::ToolCallEnd {
                        id: id.clone(),
                        ok: false,
                        view: None,
                    });
                    tool_results.push(ContentBlock::ToolResult {
                        tool_use_id: id,
                        content: format!(
                            "`{tool}` is turned off for this turn. Do not try it again; \
                             use one of the tools you were given, or say what you would \
                             need enabled."
                        ),
                        is_error: Some(true),
                    });
                }
                Err(dispatch_err) => {
                    // Schema validation failure or unknown tool. Surface
                    // to the model as a tool_result error and let it
                    // retry once.
                    consecutive_validation_errors += 1;
                    let is_unrecoverable = consecutive_validation_errors > 1;
                    on_event(AgentEvent::ToolCallEnd {
                        id: id.clone(),
                        ok: false,
                        view: None,
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
                Ok(ToolResult::Ok(mut value)) => {
                    consecutive_validation_errors = 0;
                    if let Some(node_id) = extract_node_id(&value) {
                        on_event(AgentEvent::NodeCreated(node_id));
                        node_ids_emitted.push(node_id);
                    }
                    on_event(AgentEvent::ToolCallEnd {
                        id: id.clone(),
                        ok: true,
                        view: extract_tool_view(&value),
                    });
                    // Order matters: the view has its copy now, so the
                    // chart's bulk can come out of the model's copy.
                    strip_view_only_fields(&mut value);
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
                        view: None,
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

/// Pull the drawable part out of a tool result, for the tools that have
/// one. See [`crate::ToolView`].
///
/// The tag is checked before the parse so that the overwhelming majority
/// of tool results — which are not drawable — don't pay to clone
/// themselves into a deserialise that was always going to fail.
fn extract_tool_view(value: &Value) -> Option<crate::ToolView> {
    match value.get("type").and_then(Value::as_str) {
        Some("spectrum") => serde_json::from_value(value.clone()).ok(),
        _ => None,
    }
}

/// Fields that exist for the chart and are worthless to the model.
///
/// Keyed by the result's `type` tag, same as [`extract_tool_view`].
const VIEW_ONLY_FIELDS: &[(&str, &[&str])] = &[("spectrum", &["points"])];

/// Drop the chart's payload from the copy the model reads.
///
/// A tool result is one document serving two audiences. `plot_spectrum`
/// returns 2048 `{hz, db}` pairs because the chart needs every bin; at
/// 44.1 kHz that is ~83 KB of JSON, about 24k tokens, and the model
/// cannot read a spectrum out of it. Worse, a tool result stays in the
/// conversation, so the cost is paid again on every later round trip.
///
/// The tool emits the analysis a model can actually use — peak, band
/// energies, centroid, rolloff, noise floor — alongside the curve. This
/// removes the curve once [`extract_tool_view`] has taken its copy.
///
/// Called only after the view has been extracted; doing it in the other
/// order would strip the data out from under the chart.
fn strip_view_only_fields(value: &mut Value) {
    let Some(tag) = value.get("type").and_then(Value::as_str) else {
        return;
    };
    let Some((_, fields)) = VIEW_ONLY_FIELDS.iter().find(|(t, _)| *t == tag) else {
        return;
    };
    let Some(obj) = value.as_object_mut() else {
        return;
    };
    for field in *fields {
        obj.remove(*field);
    }
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

    // ------------------------------------------------------------------
    // extract_tool_view
    // ------------------------------------------------------------------

    /// The shape `plot_spectrum` emits and the shape the UI draws are
    /// declared in two different crates, and nothing used to hold them
    /// together — the chart component sat unreachable for exactly that
    /// reason. So this drives the real tool rather than a JSON literal:
    /// a literal would keep passing after the tool's output changed.
    #[test]
    fn plot_spectrum_result_becomes_a_drawable_view() {
        use hound::{SampleFormat, WavSpec, WavWriter};

        let tmp = tempfile::TempDir::new().expect("tempdir");
        let src = tmp.path().join("tone.wav");
        let sr = 8_000u32;
        let spec = WavSpec {
            channels: 1,
            sample_rate: sr,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };
        let mut w = WavWriter::create(&src, spec).expect("wav writer");
        for n in 0..sr {
            let t = n as f32 / sr as f32;
            let s = (2.0 * std::f32::consts::PI * 440.0 * t).sin() * 0.4;
            w.write_sample((s * 32_767.0) as i16).unwrap();
        }
        w.finalize().unwrap();

        let mut store = session::Store::open(tmp.path()).expect("open store");
        let mut engine = audio_engine::Engine::new();
        let dispatcher = ToolDispatcher::default_dispatcher();
        let mut clipboard: Option<Vec<f32>> = None;
        let mut ctx = ToolContext {
            store: &mut store,
            engine: &mut engine,
            user_message: "",
            clipboard: &mut clipboard,
            allowed_tools: None,
        };

        let load = dispatcher
            .invoke("load", json!({ "path": src.to_string_lossy() }), &mut ctx)
            .expect("load dispatches");
        assert!(matches!(load, ToolResult::Ok(_)), "load failed: {load:?}");

        let result = dispatcher
            .invoke(
                "plot_spectrum",
                json!({ "track": 0, "start_sec": 0.0, "end_sec": 0.5 }),
                &mut ctx,
            )
            .expect("plot_spectrum dispatches");
        let value = match result {
            ToolResult::Ok(v) => v,
            ToolResult::Error(msg) => panic!("plot_spectrum errored: {msg}"),
        };

        let view = extract_tool_view(&value)
            .expect("plot_spectrum's result must survive the trip to the UI as a ToolView");
        let crate::ToolView::Spectrum { points, summary } = view;
        assert!(
            !points.is_empty(),
            "a spectrum with no points draws nothing"
        );
        assert!(
            points.windows(2).all(|w| w[1].hz > w[0].hz),
            "the chart plots points in array order, so they must ascend in frequency"
        );
        assert!(
            summary.is_some(),
            "the caption under the chart came back empty"
        );
    }

    /// The chart keeps the curve; the model gets the analysis instead.
    ///
    /// `plot_spectrum` returns 2048 `{hz, db}` pairs — ~83 KB at 44.1
    /// kHz, about 24k tokens — which the chart needs and a model cannot
    /// read. This drives the real tool and checks the split both ways,
    /// because getting it backwards would either blank the chart or put
    /// the curve back in the context.
    #[test]
    fn the_model_gets_the_analysis_and_the_chart_gets_the_curve() {
        use hound::{SampleFormat, WavSpec, WavWriter};

        let tmp = tempfile::TempDir::new().expect("tempdir");
        let src = tmp.path().join("tone.wav");
        let sr = 8_000u32;
        let spec = WavSpec {
            channels: 1,
            sample_rate: sr,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };
        let mut w = WavWriter::create(&src, spec).expect("wav writer");
        for n in 0..sr {
            let t = n as f32 / sr as f32;
            let s = (2.0 * std::f32::consts::PI * 440.0 * t).sin() * 0.4;
            w.write_sample((s * 32_767.0) as i16).unwrap();
        }
        w.finalize().unwrap();

        let mut store = session::Store::open(tmp.path()).expect("open store");
        let mut engine = audio_engine::Engine::new();
        let dispatcher = ToolDispatcher::default_dispatcher();
        let mut clipboard: Option<Vec<f32>> = None;
        let mut ctx = ToolContext {
            store: &mut store,
            engine: &mut engine,
            user_message: "",
            clipboard: &mut clipboard,
            allowed_tools: None,
        };
        dispatcher
            .invoke("load", json!({ "path": src.to_string_lossy() }), &mut ctx)
            .expect("load dispatches");
        let result = dispatcher
            .invoke(
                "plot_spectrum",
                json!({ "track": 0, "start_sec": 0.0, "end_sec": 0.5 }),
                &mut ctx,
            )
            .expect("plot_spectrum dispatches");
        let mut value = match result {
            ToolResult::Ok(v) => v,
            ToolResult::Error(msg) => panic!("plot_spectrum errored: {msg}"),
        };

        // The chart's half, taken first.
        let view = extract_tool_view(&value).expect("the chart must still get its curve");
        let crate::ToolView::Spectrum { points, .. } = &view;
        assert!(
            points.len() > 100,
            "the curve was gutted: {} points",
            points.len()
        );

        // The model's half, after the split.
        strip_view_only_fields(&mut value);
        assert!(
            value.get("points").is_none(),
            "the curve is still in the model's copy"
        );

        // What replaced it has to be worth reading.
        for field in [
            "peak_hz",
            "peak_db",
            "centroid_hz",
            "rolloff_hz",
            "noise_floor_db",
            "bands_dbfs",
            "summary",
        ] {
            assert!(
                value.get(field).is_some(),
                "the model lost the curve and got no {field} in exchange"
            );
        }
        let peak = value["peak_hz"].as_f64().expect("peak_hz is a number");
        assert!(
            (peak - 440.0).abs() < 30.0,
            "peak_hz {peak} should be ~440 for a 440 Hz tone"
        );

        let serialised = serde_json::to_string(&value).unwrap();
        assert!(
            serialised.len() < 1_000,
            "the model's copy is {} bytes; the point of this split was to \
             stop sending it kilobytes of float pairs",
            serialised.len()
        );
    }

    /// Stripping must not touch results that carry no view.
    #[test]
    fn stripping_leaves_ordinary_tool_results_alone() {
        let mut value = json!({ "node_id": "ab12", "summary": "gain applied", "points": 3 });
        let before = value.clone();
        strip_view_only_fields(&mut value);
        assert_eq!(
            value, before,
            "an untagged result must survive the strip untouched"
        );
    }

    /// Every other tool has to stay off this path: a `ToolView` for a
    /// tool the UI can't draw would be a wasted IPC payload at best.
    #[test]
    fn ordinary_tool_results_produce_no_view() {
        assert!(
            extract_tool_view(&json!({ "node_id": "ab12", "summary": "gain applied" })).is_none()
        );
        assert!(extract_tool_view(&json!({ "type": "waveform", "points": [] })).is_none());
        // Tagged as a spectrum but shaped wrong — better to draw nothing
        // than to hand the canvas a malformed curve.
        assert!(extract_tool_view(&json!({ "type": "spectrum", "points": "lots" })).is_none());
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
    // System-prompt assembly order: base → memory → session context.
    // ------------------------------------------------------------------

    #[test]
    fn assemble_no_extras_passes_base_through_unchanged() {
        assert_eq!(assemble_system_prompt("BASE", "", "", "", ""), "BASE");
    }

    #[test]
    fn assemble_orders_base_profile_skills_memory_ctx() {
        let out = assemble_system_prompt("BASE", "PRO", "SKL", "MEM", "CTX");
        let base = out.find("BASE").expect("missing base");
        let pro = out.find("PRO").expect("missing profile");
        let skl = out.find("SKL").expect("missing skills");
        let mem = out.find("MEM").expect("missing memory");
        let ctx = out.find("CTX").expect("missing ctx");
        assert!(base < pro, "profile must come after base");
        assert!(pro < skl, "skills must come after profile");
        assert!(skl < mem, "memory must come after skills");
        assert!(mem < ctx, "session ctx must come after memory");
    }

    #[test]
    fn assemble_skips_empty_blocks_cleanly() {
        let out = assemble_system_prompt("BASE", "", "", "", "CTX");
        assert!(out.contains("BASE"));
        assert!(out.contains("CTX"));
        assert!(!out.contains("\n\n\n"), "must not double-blank-line");
    }

    #[test]
    fn assemble_separates_with_blank_line() {
        let out = assemble_system_prompt("BASE", "", "", "MEM", "");
        assert!(
            out.contains("BASE\n\nMEM"),
            "base + memory should be separated by a blank line; got {out:?}"
        );
    }

    #[test]
    fn filter_tool_schemas_keeps_whitelisted_only() {
        use serde_json::json;
        let schemas = json!([
            { "name": "load", "description": "" },
            { "name": "gain", "description": "" },
            { "name": "fade", "description": "" },
        ]);
        let out = filter_tool_schemas(schemas, &["load".into(), "gain".into()]);
        let names: Vec<&str> = out
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|s| s.get("name").and_then(|v| v.as_str()))
            .collect();
        assert_eq!(names, vec!["load", "gain"]);
    }

    #[test]
    fn filter_tool_schemas_excludes_blacklisted() {
        use serde_json::json;
        let schemas = json!([
            { "name": "load", "description": "" },
            { "name": "gain", "description": "" },
            { "name": "fade", "description": "" },
        ]);
        let all_names = vec!["load".to_string(), "gain".to_string(), "fade".to_string()];
        let blacklist = ["gain".to_string()];
        let remaining: Vec<String> = all_names
            .into_iter()
            .filter(|t| !blacklist.contains(t))
            .collect();
        let out = filter_tool_schemas(schemas, &remaining);
        let names: Vec<&str> = out
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, vec!["load", "fade"]);
    }

    #[test]
    fn filter_tool_schemas_empty_whitelist_hides_everything() {
        use serde_json::json;
        let schemas = json!([{ "name": "load" }]);
        let out = filter_tool_schemas(schemas, &[]);
        assert!(out.as_array().unwrap().is_empty());
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
