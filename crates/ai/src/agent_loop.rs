//! Agent tool-calling loop.
//!
//! Renamed from `loop.rs` because `loop` is a Rust keyword and using it
//! as a module name forces `r#loop` everywhere. The behaviour matches
//! the M10 spec, extended in M27 with:
//!
//! * Mode detection: a cheap Haiku call classifies each user message as
//!   `mashup`, `mix`, `voice`, or `general`.
//! * Plan gate (mashup mode, or whenever Plan first is on): the loop emits
//!   a `Plan` event and suspends until the frontend approves by calling
//!   `Agent::approve_plan`.
//!
//! Per [`crate::Agent::turn`] we:
//! 1. Classify the user message.
//! 2. If mashup mode or Plan first, request a `<plan>` from the model and
//!    gate on frontend approval before proceeding. If no plan comes back
//!    and Plan first is on, the gate moves to the first model step that
//!    would change the session (see 7), so that with Plan first on no
//!    edit runs without approval (#415).
//! 3. Append the user's message to the conversation.
//! 4. Open a streaming Anthropic call (system prompt + tools cached). A
//!    provider that asks for the slim tool set (Ollama) is sent a core
//!    plus the tools the message names instead, chosen once per turn
//!    ([`crate::tool_selection`], #395); the others get every permitted
//!    tool, in the dispatcher's order, so Anthropic's cache holds.
//! 5. Forward `text` deltas to the caller's `on_event` sink in order.
//! 6. Reassemble each `tool_use` block, validate args via the
//!    dispatcher's compiled JSON Schema, invoke the tool synchronously,
//!    and append a `tool_result` block to the conversation.
//! 7. If at least one tool was used, loop. The budget of
//!    [`crate::prompt::MAX_TOOL_CALLS_PER_TURN`] applies across all
//!    iterations of the same turn, and the model is told it in the system
//!    prompt. A step whose calls would go past it is not run at all (and
//!    so never shown for approval): each of its calls is answered "not
//!    run", and the model gets one last request with tools off to say what
//!    was done and what is left. That ends the turn normally, not with an
//!    error (#439). Where the gate is still waiting for
//!    its first mutating step, that step is held *before* any of its
//!    calls dispatch and shown as the plan card; approving runs it and
//!    opens the gate for the rest of the turn, declining ends the turn
//!    having run nothing, and revising it runs nothing and goes back to
//!    the model with the user's words. Steps that only read are never
//!    held.
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

use tokio::sync::Notify;

use eventsource_stream::Eventsource;
use futures_util::StreamExt;
use serde_json::Value;
use tools::{ToolContext, ToolDispatcher, ToolResult};

use crate::anthropic::{
    max_tokens_for, ApiError, CacheControl, ContentBlock, ContentBlockDelta, ContentBlockStart,
    Message, MessagesRequest, OutputConfig, Role, StreamEvent, SystemBlock, ToolChoice,
};
use crate::approval::{self, Approval};
use crate::prompt::{
    tool_budget_line, DEFAULT_MAX_TOKENS, MAX_TOOL_CALLS_PER_TURN, PLAN_MAX_TOKENS,
};
use crate::provider::ToolSet;
use crate::session_context::{render_block, SessionContext};
use crate::{context_window, tool_selection};
use crate::{AgentEvent, Effort, Error, LlmConfig, Result, TurnResult, WireFormat};

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

/// The conversation as plain-text messages for a one-shot call (the
/// classifier, the plan), ending with `user_message`, keeping the most
/// recent `last` messages if given.
///
/// A tool-using turn leaves an assistant message with only `tool_use`
/// and a user message with only `tool_result`, which have no text. Sent
/// as `{"role":"user","content":""}` the API rejects the whole request
/// ("user messages must have non-empty content"), so after the first
/// tool-using turn the classifier fell back to `general` and Plan first
/// skipped its plan and ran the tools unapproved (#418). Empty turns are
/// dropped, and what that leaves side by side from one role is merged,
/// so roles still alternate.
fn one_shot_messages(
    conversation: &[Message],
    last: Option<usize>,
    user_message: &str,
) -> Vec<Value> {
    let mut turns: Vec<(&'static str, String)> = Vec::new();
    let all = conversation
        .iter()
        .map(|m| {
            let role = match m.role {
                Role::User => "user",
                Role::Assistant => "assistant",
            };
            (role, message_text(m))
        })
        .chain(std::iter::once(("user", user_message.to_string())));
    for (role, text) in all {
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        match turns.last_mut() {
            Some((r, t)) if *r == role => {
                t.push_str("\n\n");
                t.push_str(text);
            }
            _ => turns.push((role, text.to_string())),
        }
    }
    let keep = last.map_or(turns.len(), |n| n.saturating_add(1).min(turns.len()));
    turns[turns.len() - keep..]
        .iter()
        .map(|(role, text)| serde_json::json!({ "role": role, "content": text }))
        .collect()
}

/// A non-streaming request in the shape `cfg.provider` speaks, so the
/// provider reads its system prompt and its reply can be read back by
/// [`extract_response_text`].
///
/// This used to branch on the provider being OpenAI itself. Groq, Gemini
/// and Ollama speak the same chat-completions API but got Anthropic's
/// body — a top-level `system` their servers ignore, so neither the
/// classifier nor the plan request carried its instructions — and their
/// replies were read as Anthropic's, so every plan came back "carried no
/// text" and every request classified as general (#399).
///
/// `effort` is the reasoning effort for the request, and only the
/// Anthropic wire format has a place for it: chat-completions bodies are
/// built without it whatever is passed. The caller decides whether the
/// request wants it. The plan does, being the main model's own work. The
/// classifier does not: it runs on the cheap model, which answers any
/// `output_config.effort` with a 400 ("This model does not support the
/// effort parameter"), and `classify_mode` swallows errors, so sending it
/// there would quietly turn mode detection off. A set effort also raises
/// `max_tokens` to what that effort needs (see [`Effort::min_max_tokens`]).
///
/// `thinking_off` sends `thinking: {"type": "disabled"}` on the Anthropic
/// wire format, for a request that wants a few words and nothing else. The
/// current Anthropic models think by default and what they write while
/// thinking counts toward `max_tokens`, so a short cap can end before any
/// text. The caller must be on a model that accepts `disabled`, which the
/// Haiku models do and Sonnet 5.5, Opus 5.5 and Fable 5.1 do not (400):
/// that is why the plan, the main model's own work, never sets it. Chat
/// completions bodies are built without it, as without an effort.
fn one_shot_body(
    cfg: &LlmConfig,
    model: String,
    max_tokens: u32,
    system: &[&str],
    messages: Vec<serde_json::Value>,
    effort: Option<Effort>,
    thinking_off: bool,
) -> serde_json::Value {
    match cfg.provider.wire_format() {
        WireFormat::ChatCompletions => {
            // OpenAI's newer models reject `max_tokens`; the compatible
            // servers know only that name.
            let limit_key = if cfg.provider.id() == crate::OPENAI_ID {
                "max_completion_tokens"
            } else {
                "max_tokens"
            };
            serde_json::json!({
                "model": model,
                limit_key: max_tokens,
                "messages": std::iter::once(serde_json::json!({
                        "role": "system",
                        "content": system.join("\n\n"),
                    }))
                    .chain(messages)
                    .collect::<Vec<_>>(),
                "stream": false
            })
        }
        WireFormat::AnthropicMessages => {
            let system = match system {
                [one] => serde_json::json!(one),
                many => serde_json::Value::Array(
                    many.iter()
                        .map(|t| serde_json::json!({ "type": "text", "text": t }))
                        .collect(),
                ),
            };
            let mut body = serde_json::json!({
                "model": model,
                "max_tokens": max_tokens_for(max_tokens, effort),
                "system": system,
                "messages": messages,
                "stream": false
            });
            if let Some(effort) = effort {
                body["output_config"] = serde_json::json!(OutputConfig::effort(effort));
            }
            if thinking_off {
                body["thinking"] = serde_json::json!({ "type": "disabled" });
            }
            body
        }
    }
}

/// `max_tokens` for the classifier. The answer is one word, but the
/// classifier model may think first and that counts toward the cap: at the
/// 10 this used to be, a model that opens with a thinking block ends
/// before any text, `classify_mode` reads no mode out of it, and mode
/// detection is off with nothing to show for it. Anthropic's classifier is
/// sent with thinking disabled as well (see [`one_shot_body`]); this is the
/// room for a provider whose model thinks regardless. A cap is a ceiling,
/// so the usual one-word answer costs what it always did.
const CLASSIFIER_MAX_TOKENS: u32 = 256;

/// Classify the user's request using a cheap single-turn call to the
/// provider's [`classifier_model`](crate::LlmProvider::classifier_model).
/// Passes recent conversation history for context (last 6 messages) so
/// follow-up messages ("actually, change the BPM") classify correctly.
/// Falls back to `Mode::General` on any error so classification failures
/// are never user-visible.
pub(crate) async fn classify_mode(
    cfg: &LlmConfig,
    http: &reqwest::Client,
    user_message: &str,
    conversation: &[Message],
) -> Mode {
    let system_text = "Classify the user's request as one word: mashup, mix, voice, or general. Output only the single word.";

    // The last 6 conversation messages for context, then the new user
    // message so the classifier sees the full intent.
    let messages = one_shot_messages(conversation, Some(6), user_message);

    // No effort (see `one_shot_body`), and no thinking: the answer is one
    // word and nothing here is worth reasoning about.
    let request_body = one_shot_body(
        cfg,
        cfg.wire_classifier_model(),
        CLASSIFIER_MAX_TOKENS,
        &[system_text],
        messages,
        None,
        true,
    );

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

/// The base prompt for `mode`, followed by the line that tells the model
/// its tool budget (#439). Every mode gets it: the budget is the loop's,
/// not a mode's. The line is built here rather than written into the
/// prompt files so that it states the constant the loop enforces.
fn base_prompt_with_budget(mode: Mode) -> String {
    format!(
        "{}\n\n{}",
        select_system_prompt(mode).trim_end(),
        tool_budget_line()
    )
}

/// The per-turn fragments that go into the system prompt.
///
/// A struct rather than five `&str` parameters. Every fragment has the
/// same type, so a call site that passed the memory block where the
/// session context belongs would compile, run, and produce a prompt
/// whose sections are silently in the wrong order — with no test able
/// to see it, because the ordering tests call the assembler directly.
/// Naming the fields puts the association in front of the reader at the
/// one place it can go wrong.
pub(crate) struct SystemPromptParts<'a> {
    /// The mode-selected base prompt.
    pub base: &'a str,
    /// The active agent profile, already wrapped and defanged.
    pub profile: &'a str,
    /// Matched skills, alphabetical.
    pub skills: &'a str,
    /// Global then project memory.
    pub memory: &'a str,
    /// The rendered session context — selection and markers.
    pub context: &'a str,
}

/// Concatenate the per-turn system prompt fragments in canonical
/// order: base prompt → profile → skills (matched, alphabetical) →
/// memory (global, project) → session context. Empty fragments are
/// omitted cleanly so a single section never produces a leading or
/// trailing double newline. Extracted as a free function so the
/// ordering invariant can be unit-tested without booting `run_turn`.
pub(crate) fn assemble_system_prompt(parts: &SystemPromptParts<'_>) -> String {
    let mut out = parts.base.to_string();
    for fragment in [parts.profile, parts.skills, parts.memory, parts.context] {
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

/// Why a plan could not be produced (#267).
///
/// Every one of these used to collapse into a bare `None`, and the
/// caller falls through to the tool loop on `None` — so a user who
/// explicitly turned Plan First on lost the checkpoint they asked for,
/// and it looked exactly like the model deciding no plan was needed.
/// Nothing was logged either, so the failure class was erased before a
/// maintainer could see it.
#[derive(Debug, Clone)]
pub(crate) enum PlanUnavailable {
    /// The request never completed — connection reset, DNS, TLS.
    Transport(String),
    /// The server answered, but not with success.
    Status(u16),
    /// A 2xx whose body would not parse as JSON.
    BodyParse(String),
    /// Valid JSON with no text content where the provider puts it.
    NoResponseText,
    /// Text that carried no well-formed `<plan>` block.
    NoPlanBlock,
    /// A well-formed `<plan>` block listing no steps. Approving it would
    /// approve nothing, so it is no plan at all.
    EmptyPlan,
}

impl std::fmt::Display for PlanUnavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Transport(e) => write!(f, "the planning request failed: {e}"),
            Self::Status(code) => write!(f, "the planning request returned HTTP {code}"),
            Self::BodyParse(e) => write!(f, "the planning response could not be read: {e}"),
            Self::NoResponseText => write!(f, "the planning response carried no text"),
            Self::NoPlanBlock => write!(f, "the model did not return a plan"),
            Self::EmptyPlan => write!(f, "the model returned a plan with no steps"),
        }
    }
}

/// Request a plan from the model in a single non-streaming call and
/// return the parsed steps. Includes conversation history so follow-up
/// requests can be planned in context.
///
/// `Err` names the failure class rather than erasing it. Whatever the
/// class, the caller proceeds without a plan; with Plan first on it then
/// holds the first edit for approval instead (#415).
async fn fetch_plan(
    cfg: &LlmConfig,
    http: &reqwest::Client,
    system_prompt: &str,
    conversation: &[Message],
    user_message: &str,
) -> std::result::Result<Vec<Value>, PlanUnavailable> {
    let messages = one_shot_messages(conversation, None, user_message);

    let plan_instruction =
        "Output only a <plan>...</plan> XML block listing the steps as JSON. No other text.";
    let request_body = one_shot_body(
        cfg,
        cfg.wire_model(),
        PLAN_MAX_TOKENS,
        &[system_prompt, plan_instruction],
        messages,
        cfg.effective_effort(),
        false,
    );

    let req = http.post(format!(
        "{}{}",
        cfg.base_url(),
        cfg.provider.endpoint_path()
    ));
    let req = cfg.provider.apply_auth(req, &cfg.api_key);
    let resp = req
        .json(&request_body)
        .send()
        .await
        .map_err(|e| PlanUnavailable::Transport(e.to_string()))?;

    let status = resp.status();
    if !status.is_success() {
        return Err(PlanUnavailable::Status(status.as_u16()));
    }

    let body: Value = resp
        .json()
        .await
        .map_err(|e| PlanUnavailable::BodyParse(e.to_string()))?;
    let text = extract_response_text(cfg, &body).ok_or(PlanUnavailable::NoResponseText)?;

    let steps = parse_plan(&text).ok_or(PlanUnavailable::NoPlanBlock)?;
    // `<plan>[]</plan>` used to reach the user as a card with no steps,
    // whose Run approved nothing and then let every edit through.
    if steps.is_empty() {
        return Err(PlanUnavailable::EmptyPlan);
    }
    Ok(steps)
}

/// Hard upper bound on the number of content blocks we'll allocate for
/// a single streaming Anthropic message. The server tells us each
/// block's index; without this cap a malicious or buggy server could
/// hand us `u64::MAX` and force a massive `Vec` allocation. Anthropic's
/// real tool-use messages have well under 10 blocks, so 100 is generous.
const MAX_CONTENT_BLOCKS: usize = 100;

/// Where a turn stands on approving edits (#415).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Gate {
    /// Nothing is waiting to be approved: no gate was asked for, the
    /// approved plan covers the turn, or the held step has been approved.
    Open,
    /// Plan first is on and the model wrote no plan. The first step with a
    /// call that would change the session is held for approval.
    HoldFirstEdit,
}

/// Remove every `thinking` and `redacted_thinking` block from
/// `conversation`; text, `tool_use` and `tool_result` blocks stay, in order.
///
/// A thinking block's signature is bound to the request that produced it:
/// the top-level `system`, the `tools` set, and every message before it.
/// On Anthropic's 5.x models (Fable 5.1, Opus 5.5, Sonnet 5.5, Haiku 5.5)
/// a block replayed under a different prefix is a 400
/// (`Invalid signature in thinking block ... bound to a different
/// conversation`) for accounts created on or after 2026-08-31, and for any
/// request that opts in. Removing the blocks is the documented way out:
/// "Remove `thinking` blocks from the start of the history, from the end,
/// or all of them" is valid, and the model only loses that reasoning.
///
/// [`run_turn`] calls this once at the start of every turn, because every
/// turn rebuilds the prefix: `system` is assembled from the classifier's
/// mode, the matched skills, the memory, the agent profile and a session
/// context that carries the head node id and the selection, and `tools` is
/// filtered by that turn's whitelist. Almost no two turns send the same
/// `system`, so a block kept from an earlier turn would not be valid.
///
/// Within a turn the prefix cannot change: `run_turn` builds `system` and
/// `tools` once before its request loop and sends those same values on every
/// step, so the thinking a step produced is replayed, unmodified, with the
/// tool results that follow it. `tests/prior_turn_thinking.rs` pins that;
/// anything that makes `system` or `tools` change between a turn's steps
/// has to strip there too.
///
/// A provider that does not keep thinking in the first place (everything
/// but Anthropic's own API) has none of these blocks, so this is a no-op
/// for it. No message is left empty: an assistant message is stored only if
/// it carries a block other than thinking.
///
/// Follow-up, not done here: an append-only history with a frozen `system`
/// (the session context sent as part of the user message or as a
/// mid-conversation system message) would keep the reasoning across turns.
pub(crate) fn strip_thinking(conversation: &mut [Message]) {
    for message in conversation {
        message.content.retain(|block| {
            !matches!(
                block,
                ContentBlock::Thinking { .. } | ContentBlock::RedactedThinking { .. }
            )
        });
    }
}

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
    clipboard: &Arc<Mutex<Option<tools::Clipboard>>>,
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
    let base_prompt = base_prompt_with_budget(mode);

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
    let combined_prompt = assemble_system_prompt(&SystemPromptParts {
        base: &base_prompt,
        profile: &profile_block,
        skills: &skills_block,
        memory: &memory_block,
        context: &ctx_block,
    });
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
    let mut gate = Gate::Open;
    // The tools an approved plan names, for the slim tool set: a plan that
    // says to use `reverb` is no use if the model is then not shown it.
    let mut plan_tool_names = String::new();
    if plan_first || mode == Mode::Mashup {
        match fetch_plan(cfg, http, system_prompt, conversation, &user_message).await {
            Ok(steps) => {
                plan_tool_names = steps
                    .iter()
                    .filter_map(|step| step.get("tool").and_then(Value::as_str))
                    .collect::<Vec<_>>()
                    .join(" ");
                // Block until the frontend answers via `approve_plan` or
                // `reject_plan`, or time out after 5 minutes.
                match approval::ask(
                    &mut on_event,
                    steps,
                    plan_notify,
                    plan_rejected,
                    plan_steps_override,
                )
                .await?
                {
                    Approval::Rejected => {
                        on_event(AgentEvent::PlanRejected);
                        return Ok(TurnResult::default());
                    }
                    Approval::Approved => {}
                    Approval::Revised(text) => step_override = Some(text),
                }
            }
            // No plan, for any reason. With Plan first on, the user asked
            // for no edit to run unapproved, so the gate moves to the
            // first step that would change the session and shows its
            // concrete tool calls instead (#415). A mashup request with
            // Plan first off keeps the notice and no gate: that guarantee
            // is tied to the user's explicit choice.
            //
            // Every class lands here: a transport failure, a non-2xx, a
            // body that would not parse, a response with no text, a
            // response with no `<plan>` block, and a plan with no steps.
            // Only the last two are the model choosing not to plan; the
            // rest are faults. Either way the checkpoint the user asked
            // for is kept, and neither is silent (#267).
            Err(reason) => {
                let first_edit_held = plan_first;
                tracing::warn!(
                    reason = %reason,
                    plan_first,
                    first_edit_held,
                    mode = mode_as_str(mode),
                    "no plan was produced"
                );
                on_event(AgentEvent::PlanUnavailable {
                    reason: reason.to_string(),
                    first_edit_held,
                });
                if first_edit_held {
                    gate = Gate::HoldFirstEdit;
                }
            }
        }
    }

    // 1. Push the user turn onto the running conversation.
    //
    // First, drop the thinking that earlier turns left in it. `system` and
    // `tools` were rebuilt above for this turn, and a thinking block only
    // stays valid under the prefix it was produced with: replayed under
    // this one it is a 400 on the 5.x models. See `strip_thinking`.
    strip_thinking(conversation);
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
    // What the slim tool set reads to see which tools were asked for: the
    // words the user sent, an edited plan if they changed one, and the
    // tools an approved plan named.
    let mention_text = format!("{user_text} {plan_tool_names}");
    // Joins a trailing user message rather than following it: after a
    // declined step the conversation ends in the `tool_result`s that
    // answered it, and a second user message in a row is rejected.
    approval::push_user_text(conversation, user_text);

    let tool_schemas = {
        let d = dispatcher.lock().expect("dispatcher mutex poisoned");
        let all = d.tool_schemas();
        match tool_whitelist {
            Some(whitelist) => filter_tool_schemas(all, whitelist),
            None => all,
        }
    };
    // A provider for small-context models is sent a core plus the tools
    // this message points at, in a shorter form (#395). Worked out once
    // per turn, here, so every round trip carries the same tools in the
    // same order: a local server then keeps the prompt it has already
    // processed. The full path is untouched, which is what keeps
    // Anthropic's prompt cache valid.
    //
    // The tools it was not sent are named in one line at the end of the
    // system prompt, from the same selection, so the model knows they
    // exist and can call them by name. Appended here, after the prompt
    // the plan request used, because only now is the selection made.
    let (tool_schemas, tools_not_sent_note) = match cfg.provider.tool_set() {
        ToolSet::Full => (tool_schemas, None),
        ToolSet::Slim => {
            let selection = tool_selection::slim_tool_schemas(
                tool_schemas,
                &mention_text,
                // Not the memory or session blocks: track names and effect
                // kinds there would pull in tools nobody asked for.
                &[
                    base_prompt.as_str(),
                    profile_block.as_str(),
                    skills_block.as_str(),
                ],
                conversation,
            );
            let note = selection.names_note();
            (selection.tools, note)
        }
    };
    let request_prompt = match tools_not_sent_note {
        Some(note) => format!("{combined_prompt}\n\n{note}"),
        None => combined_prompt.clone(),
    };
    let system_prompt: &str = &request_prompt;

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
    // Set once a step has been refused for going past the tool budget.
    // The next request is the last of the turn: tools off, so the model
    // can only say what it did and what is left (#439).
    let mut summarising = false;

    loop {
        // 2. Build and send the streaming request.
        let wire_model = cfg.wire_model();
        let tool_choice = if summarising {
            ToolChoice::NONE
        } else {
            ToolChoice::AUTO
        };
        let request_struct = build_request(
            &wire_model,
            system_prompt,
            &tool_schemas,
            conversation,
            tool_choice,
            cfg.effective_effort(),
        );
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
            // A request that did not fit the model's context is its own
            // error, with what to do about it (#395).
            return Err(context_window::api_error(status.as_u16(), body));
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
                            ContentBlockStart::Thinking {
                                thinking,
                                signature,
                            } => {
                                blocks[index as usize] = PartialBlock::Thinking {
                                    thinking,
                                    signature,
                                };
                            }
                            ContentBlockStart::RedactedThinking { data } => {
                                blocks[index as usize] = PartialBlock::RedactedThinking(data);
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
                            (
                                PartialBlock::Thinking { thinking, .. },
                                ContentBlockDelta::ThinkingDelta { thinking: more },
                            ) => thinking.push_str(&more),
                            (
                                PartialBlock::Thinking { signature, .. },
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
        //
        //    The model's thinking goes in with them, first where it
        //    came first, because a request that replays a tool call
        //    without the thinking that led to it loses that reasoning
        //    — and the API asks for the blocks back unmodified. Only
        //    for a provider whose server issued them, and only a
        //    thinking block that has a signature: without one the
        //    server cannot verify it.
        let keep_thinking = cfg.provider.supports_effort();
        let assistant_blocks: Vec<ContentBlock> = blocks
            .iter()
            .filter_map(|b| match b {
                PartialBlock::Text(t) if !t.is_empty() => {
                    Some(ContentBlock::Text { text: t.clone() })
                }
                PartialBlock::Thinking {
                    thinking,
                    signature,
                } if keep_thinking && !signature.is_empty() => Some(ContentBlock::Thinking {
                    thinking: thinking.clone(),
                    signature: signature.clone(),
                }),
                PartialBlock::RedactedThinking(data) if keep_thinking && !data.is_empty() => {
                    Some(ContentBlock::RedactedThinking { data: data.clone() })
                }
                // The last request offers no tools. A provider that
                // ignores that and calls one anyway has the call left
                // out of the history, since nothing will answer it.
                PartialBlock::ToolUse {
                    id,
                    name,
                    args_json,
                } if !summarising => Some(ContentBlock::ToolUse {
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
        // A message that is nothing but thinking (a reply cut off at
        // `max_tokens` before it said anything) is not an assistant turn
        // worth replaying: it has no content for the next request to
        // follow.
        if assistant_blocks.iter().any(|b| {
            !matches!(
                b,
                ContentBlock::Thinking { .. } | ContentBlock::RedactedThinking { .. }
            )
        }) {
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

        // The last request of a turn that spent its tool budget. Its
        // text is the summary; this is where the turn ends.
        if summarising {
            // Tools were off, so a call here means the provider ignored
            // that. It was announced as it streamed, so its badge is
            // resolved; it is not run and not in the history.
            for (id, _, _) in &tool_uses {
                on_event(AgentEvent::ToolCallNotRun { id: id.clone() });
            }
            // A reply with no words would leave the user with neither
            // the work they asked for nor a reason why.
            if text_this_message.is_empty() {
                let notice = budget_notice();
                on_event(AgentEvent::TextDelta(notice.clone()));
                accumulated_text.push_str(&notice);
                conversation.push(Message {
                    role: Role::Assistant,
                    content: vec![ContentBlock::Text { text: notice }],
                });
            }
            on_event(AgentEvent::Done);
            return Ok(TurnResult {
                text: accumulated_text,
                stop_reason,
                node_ids: node_ids_emitted,
            });
        }

        if tool_uses.is_empty() {
            on_event(AgentEvent::Done);
            return Ok(TurnResult {
                text: accumulated_text,
                stop_reason,
                node_ids: node_ids_emitted,
            });
        }

        // The budget is checked on the whole step *before* anything in
        // it dispatches, so a model that proposes a step that would go
        // past it never gets the call that crosses it invoked. A step
        // is refused whole rather than run part way: its calls were
        // written to go together, and the budget is runaway protection,
        // not a meter to spend to the last unit.
        //
        // This is not an error. The edits already made stay as the
        // undoable nodes they are, so ending the turn in `Err` after
        // them told the user their request failed when most of it had
        // been done, and left the model's reply unsaid (#439). Each call
        // is answered "not run", so the history stays valid (a `tool_use`
        // with no `tool_result` is a 400) and every badge resolves, and
        // the next request, with tools off, is the model's account of
        // what was done and what is left.
        //
        // It sits before the held-step gate on purpose: a step that will
        // not run is not put to the user for approval.
        if total_tool_calls + tool_uses.len() > MAX_TOOL_CALLS_PER_TURN {
            approval::not_run(
                &mut on_event,
                conversation,
                &tool_uses,
                &budget_reached_result(),
                None,
            );
            summarising = true;
            continue;
        }

        // With Plan first on and no plan, the first step with a call that
        // would change the session is held here: after the budget check,
        // before anything in it dispatches, with the assistant message
        // already recorded so the `tool_result`s below can pair with it.
        // The calls were announced as they streamed, so their badges
        // already exist; every path out answers each one.
        if gate == Gate::HoldFirstEdit {
            // Decided under the dispatcher lock, which is dropped before
            // the await that waits for the user.
            let held = {
                let d = dispatcher.lock().expect("dispatcher mutex poisoned");
                approval::held_step(&d, &tool_uses, allowed_tools.as_ref(), &user_msg_saved)
            };
            if let Some(steps) = held {
                match approval::ask(
                    &mut on_event,
                    steps,
                    plan_notify,
                    plan_rejected,
                    plan_steps_override,
                )
                .await
                {
                    // This step runs, and so does the rest of the turn,
                    // as after an approved plan.
                    Ok(Approval::Approved) => gate = Gate::Open,
                    Ok(Approval::Rejected) => {
                        approval::not_run(
                            &mut on_event,
                            conversation,
                            &tool_uses,
                            approval::DECLINED,
                            None,
                        );
                        on_event(AgentEvent::PlanRejected);
                        return Ok(TurnResult {
                            text: accumulated_text,
                            stop_reason,
                            node_ids: node_ids_emitted,
                        });
                    }
                    // The user rewrote the step. Running the held calls
                    // would apply the very thing they just changed, so
                    // nothing runs: the model gets their words and
                    // proposes again, and that proposal is held in turn.
                    // Every edit that runs is one the user saw as shown.
                    Ok(Approval::Revised(guidance)) => {
                        approval::not_run(
                            &mut on_event,
                            conversation,
                            &tool_uses,
                            approval::REVISED,
                            Some(guidance),
                        );
                        continue;
                    }
                    Err(e) => {
                        approval::not_run(
                            &mut on_event,
                            conversation,
                            &tool_uses,
                            approval::TIMED_OUT,
                            None,
                        );
                        return Err(e);
                    }
                }
            }
        }

        let mut tool_results: Vec<ContentBlock> = Vec::with_capacity(tool_uses.len());
        for (id, name, args_json) in tool_uses {
            total_tool_calls += 1;

            let args: Value = match parse_tool_args(&args_json) {
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

/// A tool call's arguments, from the JSON the stream delivered.
///
/// A call with no arguments arrives with no `input_json_delta` at all, or
/// with an empty one, so the accumulated text is empty. That is `{}`, not
/// malformed JSON. Parsing it as JSON failed with "EOF while parsing", the
/// call was reported as malformed, and a second such call ended the whole
/// turn with an error (#409).
pub(crate) fn parse_tool_args(args_json: &str) -> serde_json::Result<Value> {
    if args_json.trim().is_empty() {
        return Ok(Value::Object(serde_json::Map::new()));
    }
    serde_json::from_str(args_json)
}

/// Build the outgoing request. Keeps the system prompt + tool schemas
/// `cache_control: ephemeral` so they're cached server-side across the
/// many round trips a tool-using turn makes.
///
/// `wire_model` is the provider-translated model id (Anthropic uses the
/// canonical id as-is; OpenRouter prepends `anthropic/`). The caller
/// computes it once per turn iteration via [`LlmConfig::wire_model`].
///
/// `tool_choice` is `AUTO` for a turn's steps and `NONE` for the last
/// request after the tool budget is spent. The tools are sent either way:
/// the history holds tool calls and their results, which Anthropic
/// rejects when no tools are defined.
fn build_request<'a>(
    wire_model: &'a str,
    system_prompt: &'a str,
    tool_schemas: &Value,
    conversation: &'a [Message],
    tool_choice: ToolChoice,
    effort: Option<Effort>,
) -> MessagesRequest<'a> {
    MessagesRequest {
        model: wire_model,
        max_tokens: max_tokens_for(DEFAULT_MAX_TOKENS, effort),
        system: vec![SystemBlock {
            kind: "text",
            text: system_prompt,
            cache_control: Some(CacheControl::EPHEMERAL),
        }],
        messages: conversation,
        tools: Some(attach_cache_control_to_tools(tool_schemas.clone())),
        tool_choice: Some(tool_choice),
        stream: true,
        output_config: effort.map(OutputConfig::effort),
    }
}

/// What the model is told for each call of a step that went past the tool
/// budget: it was not run, nothing changed, and the next thing to do is
/// tell the user where things stand.
fn budget_reached_result() -> String {
    format!(
        "Not run: this request has used its budget of {MAX_TOOL_CALLS_PER_TURN} tool calls, so \
         this call was not made and changed nothing. Make no more tool calls in this reply. \
         Tell the user what has been done and what is left, so they can ask you to continue."
    )
}

/// What the user is shown when the model, asked to summarise after the
/// budget ran out, said nothing.
fn budget_notice() -> String {
    format!(
        "I stopped after reaching the limit of {MAX_TOOL_CALLS_PER_TURN} tool calls for one \
         request. What already ran is applied and can be undone; ask me to continue to do the rest."
    )
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
    if cfg.provider.wire_format() == WireFormat::ChatCompletions {
        body.get("choices")
            .and_then(|c| c.as_array())
            .and_then(|a| a.first())
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|t| t.as_str())
            .map(|s| s.to_string())
    } else {
        // The first block carrying text, not the first block: a reply to
        // a request with a reasoning effort leads with a `thinking`
        // block, which has no `text`.
        body.get("content")
            .and_then(|c| c.as_array())
            .and_then(|arr| arr.iter().find_map(|b| b.get("text")))
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
        Some("spectrum") | Some("audition") => serde_json::from_value(value.clone()).ok(),
        _ => None,
    }
}

/// Fields that exist for the view and are worthless to the model.
///
/// Keyed by the result's `type` tag, same as [`extract_tool_view`].
///
/// `audition`'s `path` is here for a different reason than `spectrum`'s
/// `points`: not because it is large, but because it is *misdirected*.
/// An absolute path to an excerpt WAV is something the UI can open and
/// the model can only read aloud into the transcript — which is exactly
/// what #258 reports. The model keeps the summary, which tells it what
/// was auditioned and that nothing was committed.
const VIEW_ONLY_FIELDS: &[(&str, &[&str])] = &[("spectrum", &["points"]), ("audition", &["path"])];

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
    /// Streamed as `thinking_delta`s and one `signature_delta`. Kept for
    /// the history, never shown.
    Thinking {
        thinking: String,
        signature: String,
    },
    RedactedThinking(String),
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
    // One-shot requests: the classifier and the plan (#399)
    // ------------------------------------------------------------------

    /// Whether a provider's server speaks chat-completions, judged by the
    /// endpoint it is called on — independent of `wire_format`, which is
    /// what is under test. A provider that posts to `/chat/completions`
    /// and does not say so is the fault this guards.
    fn speaks_chat_completions(provider_id: &str) -> bool {
        crate::provider::provider_from_id(provider_id)
            .endpoint_path()
            .ends_with("/chat/completions")
    }

    /// What a server of `provider_id` answers a one-shot call with: the
    /// text, in the shape that server's API uses.
    fn reply_in_providers_shape(provider_id: &str, text: &str) -> Value {
        if speaks_chat_completions(provider_id) {
            json!({ "choices": [{ "message": { "role": "assistant", "content": text } }] })
        } else {
            json!({ "content": [{ "type": "text", "text": text }] })
        }
    }

    #[test]
    fn a_provider_on_chat_completions_declares_it() {
        for id in crate::SUPPORTED_PROVIDER_IDS {
            let declared = crate::provider::provider_from_id(id).wire_format();
            let expected = if speaks_chat_completions(id) {
                WireFormat::ChatCompletions
            } else {
                WireFormat::AnthropicMessages
            };
            assert_eq!(
                declared, expected,
                "{id}: its endpoint and its wire_format disagree"
            );
        }
    }

    /// Run `call` against a mock that answers `text` in the shape of
    /// `provider_id`, and return what the server was sent.
    async fn serve_one_shot<F, Fut, T>(provider_id: &str, text: &str, call: F) -> (T, Value)
    where
        F: FnOnce(LlmConfig, reqwest::Client) -> Fut,
        Fut: std::future::Future<Output = T>,
    {
        use wiremock::matchers::method;
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(reply_in_providers_shape(provider_id, text)),
            )
            .mount(&server)
            .await;
        let cfg = LlmConfig::new(crate::provider::provider_from_id(provider_id), "key")
            .with_base_url(server.uri());
        let out = call(cfg, reqwest::Client::new()).await;
        let sent = server.received_requests().await.expect("recorded");
        assert_eq!(sent.len(), 1, "{provider_id}: exactly one request");
        let body: Value = serde_json::from_slice(&sent[0].body).expect("a JSON body");
        (out, body)
    }

    /// Every provider, so a new one is covered the day it is added: its
    /// plan request carries the plan instruction where its server reads
    /// instructions, and a plan in its own reply shape is read back.
    #[tokio::test]
    async fn a_plan_round_trips_for_every_provider() {
        let plan = r#"<plan>[{"step":1,"tool":"set_track_gain","description":"Louder"}]</plan>"#;
        for id in crate::SUPPORTED_PROVIDER_IDS {
            let (steps, body) = serve_one_shot(id, plan, |cfg, http| async move {
                fetch_plan(&cfg, &http, "SYSTEM-PROMPT", &[], "make it louder").await
            })
            .await;

            let steps = steps.unwrap_or_else(|e| panic!("{id}: no plan read back: {e:?}"));
            assert_eq!(steps.len(), 1, "{id}");
            let everything = body.to_string();
            assert!(
                everything.contains("SYSTEM-PROMPT"),
                "{id}: the system prompt was not sent"
            );
            assert!(
                everything.contains("<plan>"),
                "{id}: the plan instruction was not sent"
            );
            match crate::provider::provider_from_id(id).wire_format() {
                WireFormat::ChatCompletions => {
                    assert!(
                        body.get("system").is_none(),
                        "{id}: a top-level `system` is ignored by this API"
                    );
                    assert_eq!(body["messages"][0]["role"], "system", "{id}");
                    assert_eq!(body["messages"][1]["content"], "make it louder", "{id}");
                }
                WireFormat::AnthropicMessages => {
                    assert_eq!(body["system"].as_array().map(Vec::len), Some(2), "{id}");
                    assert_eq!(body["messages"][0]["role"], "user", "{id}");
                }
            }
        }
    }

    /// `<plan>[]</plan>` is no plan: approving it approves nothing, and
    /// the card it made had no steps and a Run button that let every edit
    /// through (#415).
    #[tokio::test]
    async fn an_empty_plan_block_is_no_plan() {
        for id in crate::SUPPORTED_PROVIDER_IDS {
            let (result, _) = serve_one_shot(id, "<plan>[]</plan>", |cfg, http| async move {
                fetch_plan(&cfg, &http, "SYSTEM-PROMPT", &[], "make it louder").await
            })
            .await;
            assert!(
                matches!(result, Err(PlanUnavailable::EmptyPlan)),
                "{id}: an empty plan was accepted: {result:?}"
            );
        }
    }

    #[tokio::test]
    async fn the_classifier_reads_its_answer_for_every_provider() {
        for id in crate::SUPPORTED_PROVIDER_IDS {
            let (mode, body) = serve_one_shot(id, "mashup", |cfg, http| async move {
                classify_mode(&cfg, &http, "mash these two songs up", &[]).await
            })
            .await;

            assert_eq!(mode, Mode::Mashup, "{id}: the answer was not read");
            assert!(
                body.to_string().contains("Classify the user's request"),
                "{id}: the classifier instruction was not sent"
            );
        }
    }

    /// OpenAI's own models reject `max_tokens`; the compatible servers
    /// know only that name.
    #[test]
    fn the_token_limit_is_named_as_each_api_expects() {
        let body_for = |id: &str| {
            let cfg = LlmConfig::new(crate::provider::provider_from_id(id), "k");
            one_shot_body(&cfg, "m".into(), 7, &["s"], vec![], None, false)
        };
        assert_eq!(body_for(crate::OPENAI_ID)["max_completion_tokens"], 7);
        for id in [
            crate::provider::OLLAMA_ID,
            crate::provider::GROQ_ID,
            crate::provider::GEMINI_ID,
        ] {
            assert_eq!(body_for(id)["max_tokens"], 7, "{id}");
            assert!(body_for(id).get("max_completion_tokens").is_none(), "{id}");
        }
    }

    // ------------------------------------------------------------------
    // Reasoning effort
    // ------------------------------------------------------------------

    /// What the effort setting puts on each request, per provider.
    fn config_with_effort(id: &str, effort: Effort) -> LlmConfig {
        LlmConfig::new(crate::provider::provider_from_id(id), "k").with_effort(effort)
    }

    #[test]
    fn a_one_shot_body_carries_the_effort_in_the_anthropic_shape_only() {
        for id in crate::SUPPORTED_PROVIDER_IDS {
            let cfg = config_with_effort(id, Effort::XHigh);
            let body = one_shot_body(
                &cfg,
                "m".into(),
                1024,
                &["s"],
                vec![],
                Some(Effort::XHigh),
                false,
            );
            match cfg.provider.wire_format() {
                WireFormat::AnthropicMessages => {
                    assert_eq!(body["output_config"], json!({ "effort": "xhigh" }), "{id}");
                    assert!(body.get("effort").is_none(), "{id}");
                    // 1024 is not enough room to think in at this level.
                    assert_eq!(body["max_tokens"], 16384, "{id}");
                }
                WireFormat::ChatCompletions => {
                    let text = body.to_string();
                    assert!(!text.contains("output_config"), "{id}: {text}");
                    assert!(!text.contains("effort"), "{id}: {text}");
                    // And the cap is the caller's own.
                    let cap = body
                        .get("max_tokens")
                        .or_else(|| body.get("max_completion_tokens"));
                    assert_eq!(cap, Some(&json!(1024)), "{id}");
                }
            }
        }
    }

    /// Unset is the body this function always built.
    #[test]
    fn a_one_shot_body_with_no_effort_is_unchanged() {
        let cfg = LlmConfig::new_anthropic("k");
        let body = one_shot_body(&cfg, "m".into(), 1024, &["s"], vec![], None, false);
        assert_eq!(
            body,
            json!({
                "model": "m",
                "max_tokens": 1024,
                "system": "s",
                "messages": [],
                "stream": false
            })
        );
    }

    /// The streaming turn's request, per provider, as the loop builds it:
    /// `output_config` for Anthropic when set, nowhere else, and not at
    /// all when unset.
    #[test]
    fn the_streaming_request_carries_the_effort_for_anthropic_only() {
        let conversation = [Message {
            role: Role::User,
            content: vec![ContentBlock::Text { text: "hi".into() }],
        }];
        let tools = json!([{ "name": "t", "description": "d", "input_schema": {"type":"object"} }]);
        for id in crate::SUPPORTED_PROVIDER_IDS {
            let cfg = config_with_effort(id, Effort::High);
            let req = build_request(
                "m",
                "sys",
                &tools,
                &conversation,
                ToolChoice::AUTO,
                cfg.effective_effort(),
            );
            let body = cfg.provider.serialize_request(&req);
            let text = body.to_string();
            if *id == crate::ANTHROPIC_ID {
                assert_eq!(body["output_config"], json!({ "effort": "high" }));
                assert_eq!(body["max_tokens"], 8192);
            } else {
                assert!(!text.contains("output_config"), "{id}: {text}");
                assert!(!text.contains("\"effort\""), "{id}: {text}");
            }

            let unset = LlmConfig::new(crate::provider::provider_from_id(id), "k");
            let req = build_request(
                "m",
                "sys",
                &tools,
                &conversation,
                ToolChoice::AUTO,
                unset.effective_effort(),
            );
            let text = unset.provider.serialize_request(&req).to_string();
            assert!(!text.contains("output_config"), "{id}: {text}");
        }
        // Unset also keeps the token cap it always had.
        let req = build_request("m", "sys", &tools, &conversation, ToolChoice::AUTO, None);
        assert_eq!(req.max_tokens, DEFAULT_MAX_TOKENS);
    }

    /// The cheap classifier model rejects `effort` with a 400 and
    /// `classify_mode` swallows errors, so it must never be sent: it
    /// would turn mode detection off without a word.
    #[tokio::test]
    async fn the_classifier_request_never_carries_an_effort() {
        for id in crate::SUPPORTED_PROVIDER_IDS {
            let (mode, body) = serve_one_shot(id, "mashup", |cfg, http| async move {
                let cfg = cfg.with_effort(Effort::Max);
                classify_mode(&cfg, &http, "mash these two songs up", &[]).await
            })
            .await;
            assert_eq!(mode, Mode::Mashup, "{id}");
            let text = body.to_string();
            assert!(!text.contains("output_config"), "{id}: {text}");
            assert!(!text.contains("effort"), "{id}: {text}");
        }
    }

    /// The plan is the main model's own work, so it carries the effort:
    /// on Anthropic, with room to think; nowhere else.
    #[tokio::test]
    async fn the_plan_request_carries_the_effort_on_anthropic_only() {
        let plan = r#"<plan>[{"step":1,"tool":"set_track_gain","description":"Louder"}]</plan>"#;
        for id in crate::SUPPORTED_PROVIDER_IDS {
            let (steps, body) = serve_one_shot(id, plan, |cfg, http| async move {
                let cfg = cfg.with_effort(Effort::XHigh);
                fetch_plan(&cfg, &http, "SYSTEM-PROMPT", &[], "make it louder").await
            })
            .await;
            assert!(steps.is_ok(), "{id}: {steps:?}");
            if *id == crate::ANTHROPIC_ID {
                assert_eq!(body["output_config"], json!({ "effort": "xhigh" }));
                assert_eq!(body["max_tokens"], 16384);
            } else {
                let text = body.to_string();
                assert!(!text.contains("output_config"), "{id}: {text}");
                assert!(!text.contains("\"effort\""), "{id}: {text}");
            }
        }
    }

    /// With no effort the plan request adds no `output_config`, and it
    /// asks for `PLAN_MAX_TOKENS`: on a model that thinks by default the
    /// old 1024 came back as a thinking block with no text, which reads
    /// as "the model returned no plan". It does not turn thinking off
    /// either: the plan is the main model's own work, and the main model
    /// may be one that rejects `disabled`.
    #[tokio::test]
    async fn the_plan_request_with_no_effort_adds_no_output_config() {
        let plan = r#"<plan>[{"step":1,"tool":"set_track_gain","description":"Louder"}]</plan>"#;
        for id in crate::SUPPORTED_PROVIDER_IDS {
            let (steps, body) = serve_one_shot(id, plan, |cfg, http| async move {
                fetch_plan(&cfg, &http, "SYSTEM-PROMPT", &[], "make it louder").await
            })
            .await;
            assert!(steps.is_ok(), "{id}: {steps:?}");
            assert!(body.get("output_config").is_none(), "{id}");
            assert!(body.get("thinking").is_none(), "{id}");
            let cap = body
                .get("max_tokens")
                .or_else(|| body.get("max_completion_tokens"));
            assert_eq!(cap, Some(&json!(PLAN_MAX_TOKENS)), "{id}");
            // The literal is deliberate: a lowered constant must not pass.
            assert!(
                cap.and_then(Value::as_u64).is_some_and(|c| c >= 8192),
                "{id}: no room to think before a plan: {cap:?}"
            );
        }
    }

    /// A model step needs the same room: thinking counts toward the cap,
    /// and the 5.x models think by default. The literal is deliberate.
    #[test]
    fn a_step_of_a_turn_has_room_to_think_before_it_answers() {
        let conversation = [Message {
            role: Role::User,
            content: vec![ContentBlock::Text { text: "hi".into() }],
        }];
        let req = build_request(
            "m",
            "sys",
            &json!([]),
            &conversation,
            ToolChoice::AUTO,
            None,
        );
        assert!(req.max_tokens >= 8192, "{}", req.max_tokens);
        assert_eq!(req.max_tokens, DEFAULT_MAX_TOKENS);
    }

    /// The classifier asks for one word, and a model that thinks by
    /// default spends the cap on thinking first, so an answer of no text
    /// at all (read as `general`, silently) was one short cap away.
    /// Anthropic's classifier is sent with thinking disabled and room to
    /// spare; a chat-completions body has no `thinking` field to set and
    /// gets the room.
    #[tokio::test]
    async fn the_classifier_turns_thinking_off_and_has_headroom() {
        for id in crate::SUPPORTED_PROVIDER_IDS {
            let (mode, body) = serve_one_shot(id, "mashup", |cfg, http| async move {
                classify_mode(&cfg, &http, "mash these two songs up", &[]).await
            })
            .await;
            assert_eq!(mode, Mode::Mashup, "{id}");
            match crate::provider::provider_from_id(id).wire_format() {
                WireFormat::AnthropicMessages => {
                    assert_eq!(body["thinking"], json!({ "type": "disabled" }), "{id}");
                }
                WireFormat::ChatCompletions => {
                    assert!(body.get("thinking").is_none(), "{id}: {body}");
                }
            }
            // Room for a model that thinks before its one word. The
            // literal is deliberate: a lowered constant must not pass.
            let cap = body
                .get("max_tokens")
                .or_else(|| body.get("max_completion_tokens"))
                .and_then(Value::as_u64);
            assert_eq!(cap, Some(u64::from(CLASSIFIER_MAX_TOKENS)), "{id}");
            assert!(cap.is_some_and(|c| c >= 256), "{id}: {cap:?}");
        }
    }

    /// `thinking_off` is what puts the field on the wire, and only on the
    /// Anthropic format.
    #[test]
    fn thinking_off_is_a_request_for_the_anthropic_body_only() {
        for id in crate::SUPPORTED_PROVIDER_IDS {
            let cfg = LlmConfig::new(crate::provider::provider_from_id(id), "k");
            let off = one_shot_body(&cfg, "m".into(), 7, &["s"], vec![], None, true);
            let on = one_shot_body(&cfg, "m".into(), 7, &["s"], vec![], None, false);
            assert!(on.get("thinking").is_none(), "{id}");
            match cfg.provider.wire_format() {
                WireFormat::AnthropicMessages => {
                    assert_eq!(off["thinking"], json!({ "type": "disabled" }), "{id}")
                }
                WireFormat::ChatCompletions => assert!(off.get("thinking").is_none(), "{id}"),
            }
        }
    }

    // ------------------------------------------------------------------
    // strip_thinking (#468)
    // ------------------------------------------------------------------

    fn thinking(signature: &str) -> ContentBlock {
        ContentBlock::Thinking {
            thinking: format!("reasoning {signature}"),
            signature: signature.into(),
        }
    }

    /// `role:kind,kind` per message, to compare a conversation's shape.
    fn shape_of(conversation: &[Message]) -> Vec<String> {
        conversation
            .iter()
            .map(|m| {
                let kinds: Vec<&str> = m
                    .content
                    .iter()
                    .map(|b| match b {
                        ContentBlock::Text { .. } => "text",
                        ContentBlock::ToolUse { .. } => "tool_use",
                        ContentBlock::ToolResult { .. } => "tool_result",
                        ContentBlock::Thinking { .. } => "thinking",
                        ContentBlock::RedactedThinking { .. } => "redacted_thinking",
                    })
                    .collect();
                format!("{:?}:{}", m.role, kinds.join(","))
            })
            .collect()
    }

    #[test]
    fn strip_thinking_removes_both_kinds_and_keeps_the_rest_in_order() {
        let mut conversation = vec![
            Message {
                role: Role::User,
                content: vec![ContentBlock::Text { text: "go".into() }],
            },
            Message {
                role: Role::Assistant,
                content: vec![
                    thinking("A"),
                    ContentBlock::RedactedThinking { data: "R".into() },
                    ContentBlock::Text {
                        text: "on it".into(),
                    },
                    thinking("B"),
                    ContentBlock::ToolUse {
                        id: "t1".into(),
                        name: "gain".into(),
                        input: json!({ "db": 3 }),
                    },
                ],
            },
            Message {
                role: Role::User,
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "t1".into(),
                    content: "{}".into(),
                    is_error: None,
                }],
            },
            Message {
                role: Role::Assistant,
                content: vec![
                    thinking("C"),
                    ContentBlock::Text {
                        text: "done".into(),
                    },
                ],
            },
        ];
        strip_thinking(&mut conversation);

        assert_eq!(
            shape_of(&conversation),
            [
                "User:text",
                "Assistant:text,tool_use",
                "User:tool_result",
                "Assistant:text"
            ]
        );
        // The survivors are untouched.
        assert!(matches!(
            &conversation[1].content[0],
            ContentBlock::Text { text } if text == "on it"
        ));
        assert!(matches!(
            &conversation[1].content[1],
            ContentBlock::ToolUse { id, input, .. } if id == "t1" && input == &json!({ "db": 3 })
        ));
        assert!(matches!(
            &conversation[2].content[0],
            ContentBlock::ToolResult { tool_use_id, .. } if tool_use_id == "t1"
        ));
    }

    #[test]
    fn strip_thinking_leaves_a_conversation_without_any_alone() {
        let mut conversation = tool_turn("make it louder", "Done.");
        let before = shape_of(&conversation);
        strip_thinking(&mut conversation);
        assert_eq!(shape_of(&conversation), before);
        strip_thinking(&mut []);
    }

    /// A reply to a request with an effort can lead with a `thinking`
    /// block, which has no text. Reading `content[0]` found nothing and
    /// reported the plan unavailable.
    #[test]
    fn the_reply_text_is_found_past_a_leading_thinking_block() {
        let cfg = LlmConfig::new_anthropic("k");
        let body = json!({
            "content": [
                { "type": "thinking", "thinking": "", "signature": "SIG" },
                { "type": "text", "text": "<plan>[]</plan>" }
            ]
        });
        assert_eq!(
            extract_response_text(&cfg, &body).as_deref(),
            Some("<plan>[]</plan>")
        );
        // Thinking and nothing after it (cut off at max_tokens) is no text.
        let cut_off =
            json!({ "content": [{ "type": "thinking", "thinking": "", "signature": "S" }] });
        assert_eq!(extract_response_text(&cfg, &cut_off), None);
    }

    // ------------------------------------------------------------------
    // one_shot_messages (#418)
    // ------------------------------------------------------------------

    /// A turn that used a tool: the request, the model's tool call (no
    /// text), the tool's result (no text), and the model's reply.
    fn tool_turn(request: &str, reply: &str) -> Vec<Message> {
        vec![
            Message {
                role: Role::User,
                content: vec![ContentBlock::Text {
                    text: request.into(),
                }],
            },
            Message {
                role: Role::Assistant,
                content: vec![ContentBlock::ToolUse {
                    id: "t1".into(),
                    name: "gain".into(),
                    input: json!({}),
                }],
            },
            Message {
                role: Role::User,
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "t1".into(),
                    content: "{\"gain_db\":3}".into(),
                    is_error: None,
                }],
            },
            Message {
                role: Role::Assistant,
                content: vec![ContentBlock::Text { text: reply.into() }],
            },
        ]
    }

    fn assert_sendable(messages: &[Value]) {
        assert!(!messages.is_empty());
        for (i, m) in messages.iter().enumerate() {
            let text = m["content"].as_str().expect("text content");
            assert!(
                !text.trim().is_empty(),
                "message {i} is empty: {messages:?}"
            );
            if i > 0 {
                assert_ne!(
                    m["role"],
                    messages[i - 1]["role"],
                    "roles repeat at {i}: {messages:?}"
                );
            }
        }
        assert_eq!(messages.last().unwrap()["role"], "user");
    }

    #[test]
    fn a_tool_using_turn_leaves_no_empty_message() {
        let conversation = tool_turn("make it louder", "Done, +3 dB.");
        let messages = one_shot_messages(&conversation, None, "do that again");
        assert_sendable(&messages);
        assert_eq!(messages.len(), 3, "{messages:?}");
        assert_eq!(messages[0]["content"], "make it louder");
        assert_eq!(messages[2]["content"], "do that again");
    }

    /// A turn whose reply was tool calls only leaves two user messages
    /// side by side once the empty ones go; they are merged.
    #[test]
    fn user_messages_left_side_by_side_are_merged() {
        let mut conversation = tool_turn("make it louder", "");
        conversation.pop();
        let messages = one_shot_messages(&conversation, None, "and fade it out");
        assert_sendable(&messages);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["content"], "make it louder\n\nand fade it out");
    }

    #[test]
    fn the_classifier_keeps_the_most_recent_turns() {
        let mut conversation = Vec::new();
        for i in 0..5 {
            conversation.extend(tool_turn(&format!("request {i}"), &format!("reply {i}")));
        }
        let messages = one_shot_messages(&conversation, Some(6), "the new one");
        assert_sendable(&messages);
        assert_eq!(messages.len(), 7, "six of history and the new message");
        assert_eq!(messages.last().unwrap()["content"], "the new one");
        assert_eq!(messages[0]["content"], "request 2");
    }

    // ------------------------------------------------------------------
    // parse_tool_args (#409)
    // ------------------------------------------------------------------

    #[test]
    fn a_call_with_no_arguments_is_an_empty_object() {
        assert_eq!(parse_tool_args("").unwrap(), json!({}));
        assert_eq!(parse_tool_args("  \n").unwrap(), json!({}));
    }

    #[test]
    fn arguments_are_parsed_as_json() {
        assert_eq!(
            parse_tool_args(r#"{"track":0}"#).unwrap(),
            json!({"track": 0})
        );
    }

    #[test]
    fn truncated_arguments_are_still_malformed() {
        assert!(parse_tool_args(r#"{"track":"#).is_err());
    }

    // ------------------------------------------------------------------
    // extract_tool_view
    // ------------------------------------------------------------------

    /// `audition_effect` renders an excerpt and used to return only its
    /// path, as JSON, into the chat — a full render nobody could hear
    /// without leaving the app (#258).
    ///
    /// This drives the real tool for the same reason the spectrum test
    /// below does: the tool's result and this enum are declared in two
    /// different crates, and a JSON literal here would keep passing
    /// after the tool stopped emitting the tag. That is precisely how
    /// the audition shipped unreachable in the first place.
    #[test]
    fn audition_result_becomes_a_playable_view() {
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
        let mut clipboard: Option<tools::Clipboard> = None;
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
                "audition_effect",
                json!({
                    "track": 0,
                    "kind": "low_pass_filter",
                    "params": { "cutoff_hz": 1_000.0 },
                    "start_sec": 0.0,
                    "end_sec": 0.5,
                }),
                &mut ctx,
            )
            .expect("audition_effect dispatches");
        let mut value = match result {
            ToolResult::Ok(v) => v,
            ToolResult::Error(msg) => panic!("audition_effect errored: {msg}"),
        };

        let view = extract_tool_view(&value)
            .expect("audition_effect's result must survive the trip to the UI as a ToolView");
        let crate::ToolView::Audition {
            path,
            kind,
            track,
            start_sec,
            end_sec,
            summary,
        } = view
        else {
            panic!("audition_effect must project to an Audition view, got {view:?}");
        };

        // The player loads this path. A relative or empty one plays
        // nothing, and it fails silently in a webview.
        assert!(
            std::path::Path::new(&path).is_absolute(),
            "the player needs an absolute path, got {path:?}"
        );
        assert!(
            std::path::Path::new(&path).exists(),
            "the view names {path:?}, which does not exist — the excerpt was never rendered"
        );
        assert_eq!(kind, "low_pass_filter");
        assert_eq!(track, 0);
        assert_eq!(start_sec, 0.0);
        assert_eq!(end_sec, 0.5);
        assert!(
            summary.is_some_and(|s| !s.is_empty()),
            "the caption under the player comes from the tool's summary"
        );

        // And the model's copy loses the path it could only recite.
        // This is the other half of #258: the path stopped being chat
        // text at the same moment it became a player.
        strip_view_only_fields(&mut value);
        assert!(
            value.get("path").is_none(),
            "the model still receives the excerpt's path as text: {value}"
        );
        assert!(
            value.get("summary").is_some(),
            "the model must keep the summary — it is how it learns nothing was committed"
        );
    }

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
        let mut clipboard: Option<tools::Clipboard> = None;
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
        let crate::ToolView::Spectrum { points, summary } = view else {
            panic!("plot_spectrum must project to a Spectrum view, got {view:?}");
        };
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
        let mut clipboard: Option<tools::Clipboard> = None;
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
        let crate::ToolView::Spectrum { points, .. } = &view else {
            panic!("plot_spectrum must project to a Spectrum view, got {view:?}");
        };
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

    /// Every mode's prompt says how many tool calls the model has (#439),
    /// after the mode's own text, with the mode's file untouched.
    #[test]
    fn every_mode_s_base_prompt_states_the_tool_budget() {
        for mode in [Mode::Mashup, Mode::Mix, Mode::Voice, Mode::General] {
            let base = base_prompt_with_budget(mode);
            let line = tool_budget_line();
            assert!(
                base.ends_with(&line),
                "{} mode: the budget line is not at the end of the base prompt",
                mode_as_str(mode)
            );
            assert!(
                base.starts_with(select_system_prompt(mode).trim_end()),
                "{} mode: the mode's own prompt was changed",
                mode_as_str(mode)
            );
            assert!(
                base.contains(&format!("at most {MAX_TOOL_CALLS_PER_TURN} tool calls")),
                "{} mode: {base:?}",
                mode_as_str(mode)
            );
        }
    }

    /// What the model reads for a call it was not allowed to make has to
    /// say why, and what to do next, in the words it can act on.
    #[test]
    fn the_not_run_answer_names_the_budget_and_asks_for_a_summary() {
        let answer = budget_reached_result();
        assert!(
            answer.contains(&MAX_TOOL_CALLS_PER_TURN.to_string()),
            "{answer:?}"
        );
        assert!(answer.starts_with("Not run"), "{answer:?}");
        assert!(answer.contains("what is left"), "{answer:?}");
        assert!(budget_notice().contains(&MAX_TOOL_CALLS_PER_TURN.to_string()));
    }

    // ------------------------------------------------------------------
    // System-prompt assembly order: base → memory → session context.
    // ------------------------------------------------------------------

    /// `SystemPromptParts` with every fragment empty, to be filled in
    /// by the field the test is about.
    fn parts<'a>(base: &'a str) -> SystemPromptParts<'a> {
        SystemPromptParts {
            base,
            profile: "",
            skills: "",
            memory: "",
            context: "",
        }
    }

    #[test]
    fn assemble_no_extras_passes_base_through_unchanged() {
        assert_eq!(assemble_system_prompt(&parts("BASE")), "BASE");
    }

    #[test]
    fn assemble_orders_base_profile_skills_memory_ctx() {
        let out = assemble_system_prompt(&SystemPromptParts {
            base: "BASE",
            profile: "PRO",
            skills: "SKL",
            memory: "MEM",
            context: "CTX",
        });
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
        let out = assemble_system_prompt(&SystemPromptParts {
            context: "CTX",
            ..parts("BASE")
        });
        assert!(out.contains("BASE"));
        assert!(out.contains("CTX"));
        assert!(!out.contains("\n\n\n"), "must not double-blank-line");
    }

    #[test]
    fn assemble_separates_with_blank_line() {
        let out = assemble_system_prompt(&SystemPromptParts {
            memory: "MEM",
            ..parts("BASE")
        });
        assert!(
            out.contains("BASE\n\nMEM"),
            "base + memory should be separated by a blank line; got {out:?}"
        );
    }

    /// Every failure class has to render something a user can act on.
    ///
    /// Against the real `Display` impl, not a list of strings retyped
    /// into the test — the whole defect was that these classes were
    /// erased, so a test that asserts on its own copies of the messages
    /// would prove nothing about what a user is shown.
    #[test]
    fn every_plan_failure_class_names_itself() {
        use super::PlanUnavailable;

        let cases = [
            PlanUnavailable::Transport("connection reset by peer".into()),
            PlanUnavailable::Status(503),
            PlanUnavailable::BodyParse("expected value at line 1".into()),
            PlanUnavailable::NoResponseText,
            PlanUnavailable::NoPlanBlock,
            PlanUnavailable::EmptyPlan,
        ];

        for case in &cases {
            let rendered = case.to_string();
            assert!(
                rendered.len() > 20,
                "{case:?} renders as {rendered:?}, too terse to tell a user \
                 what happened"
            );
            // The reason is appended to "Plan step skipped — …", so it
            // must not merely restate the event.
            assert!(
                !rendered.to_lowercase().contains("plan unavailable"),
                "{rendered:?} restates the event instead of naming the cause"
            );
        }

        // The two that carry detail must keep it: a bare "the request
        // failed" is the silent version with extra words.
        assert!(
            PlanUnavailable::Status(503).to_string().contains("503"),
            "the HTTP status was dropped"
        );
        assert!(
            PlanUnavailable::Transport("connection reset by peer".into())
                .to_string()
                .contains("connection reset by peer"),
            "the transport error was dropped"
        );

        // And each class has to be distinguishable from the others, or
        // logging them separately buys nothing.
        let rendered: std::collections::BTreeSet<String> =
            cases.iter().map(|c| c.to_string()).collect();
        assert_eq!(
            rendered.len(),
            cases.len(),
            "two failure classes render identically: {rendered:?}"
        );
    }

    /// The wiring `session_context.rs` could not reach.
    ///
    /// Its `agent_loop_accepts_session_context` was a compile-test over
    /// a re-export: it never named `agent_loop`, never called anything,
    /// and would have passed with the context dropped from the prompt
    /// entirely. What actually has to hold is that a `SessionContext`
    /// reaches the assembler through `render_block`, and lands in the
    /// context slot rather than one of the four fragments beside it.
    #[test]
    fn a_session_context_reaches_the_prompt_in_the_context_slot() {
        use crate::session_context::{render_block, SessionContext};

        let ctx = SessionContext {
            selection: Some(tools::Range {
                start_sec: 1.0,
                end_sec: 2.5,
            }),
            ..Default::default()
        };
        let rendered = render_block(&ctx);
        assert!(
            rendered.contains("current_selection"),
            "render_block produced nothing to look for: {rendered:?}"
        );

        let out = assemble_system_prompt(&SystemPromptParts {
            base: "BASE",
            profile: "",
            skills: "",
            memory: "MEM",
            context: &rendered,
        });

        assert!(
            out.contains("current_selection"),
            "the session context never made it into the prompt"
        );
        let mem = out.find("MEM").expect("missing memory");
        let sel = out.find("current_selection").expect("missing selection");
        assert!(
            mem < sel,
            "the session context is ahead of memory — the two fragments are \
             swapped"
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
