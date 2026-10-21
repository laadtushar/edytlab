//! Asking the user before something runs (#415).
//!
//! The agent loop has two reasons to stop and ask:
//!
//! * the model wrote a `<plan>` and the user should approve it first, and
//! * with Plan first on, the model wrote **no** plan, so the first step
//!   that would change the session is shown instead, as the concrete
//!   tool calls it is about to make.
//!
//! Both go through [`ask`], so one arming rule, one timeout and one
//! meaning for "revised" serve both. This module also owns what the held
//! step looks like on the card ([`describe_call`]) and the conversation
//! bookkeeping a step that does not run needs ([`not_run`]).
//!
//! Kept out of `agent_loop.rs`, which is already long enough that adding
//! to it would bury the loop it exists to explain.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::FutureExt;
use serde_json::{json, Value};
use tokio::sync::Notify;
use tools::ToolDispatcher;

use crate::agent_loop::parse_tool_args;
use crate::anthropic::{ContentBlock, Message, Role};
use crate::{AgentEvent, Error, Result};

/// The tool result a model gets when the user declined its step.
pub(crate) const DECLINED: &str = "Declined by the user: this call was shown to them for approval and they chose not to run it. Nothing was run and the session is unchanged. Do not call it again unless the user asks.";

/// The tool result a model gets when the user rewrote the step instead of
/// approving it. The revision itself follows in the same message.
pub(crate) const REVISED: &str = "Not run: the user changed this step before approving it, so nothing was run. Their revision follows; do what it says instead.";

/// The tool result a model gets when nobody answered in time.
pub(crate) const TIMED_OUT: &str = "Not run: the user did not answer the approval request within 5 minutes. Nothing was run and the session is unchanged.";

/// How long the gate waits for an answer.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(300);

/// What the user said to a card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Approval {
    /// Run it as shown.
    Approved,
    /// Run it, but not as shown: this is the user's rewrite of the steps.
    /// For a model-written plan the text is appended to the user's
    /// message. For a held step it is guidance only, because running the
    /// held calls would apply the very thing the user just changed.
    Revised(String),
    /// Do not run it.
    Rejected,
}

/// Show `steps` on the approval card and wait for the answer.
///
/// The gate is armed before the card exists. The notifier is a tokio
/// `Notify`, which *stores* a permit when `notify_one` is called with
/// nobody waiting, and the rejection flag and the override slot are plain
/// shared state. Anything left behind by an earlier gate (a timed-out
/// turn, a stray click) would otherwise answer this one before the user
/// has seen it. Arming first is safe because the UI cannot answer a card
/// that has not been emitted yet.
pub(crate) async fn ask(
    on_event: &mut impl FnMut(AgentEvent),
    steps: Vec<Value>,
    notify: &Arc<Notify>,
    rejected: &Arc<AtomicBool>,
    override_slot: &Arc<Mutex<Option<String>>>,
) -> Result<Approval> {
    // Polling a `Notified` once consumes a stored permit. With none
    // stored it registers a waiter that is dropped with the future.
    let _ = notify.notified().now_or_never();
    rejected.store(false, Ordering::SeqCst);
    override_slot
        .lock()
        .expect("plan_steps_override mutex poisoned")
        .take();

    on_event(AgentEvent::Plan { steps });

    if await_answer(notify, rejected).await? {
        return Ok(Approval::Rejected);
    }
    // Consume any step overrides the frontend stored before firing the
    // notifier.
    let revised = override_slot
        .lock()
        .expect("plan_steps_override mutex poisoned")
        .take();
    Ok(match revised {
        Some(text) => Approval::Revised(text),
        None => Approval::Approved,
    })
}

/// Wait for the frontend to answer. Uses `tokio::sync::Notify` so the loop
/// wakes immediately when the user clicks "Run" with zero polling
/// overhead. Times out after 5 minutes.
///
/// The notifier is stored in `AppState` (not behind the agent Mutex), so
/// the `approve_plan` Tauri command can fire it without holding any lock
/// that `send_message` also holds, eliminating the deadlock.
///
/// There used to be exactly two ways out: approve, or wait five minutes.
/// A user who disliked the plan had no way to say so, which made the gate
/// feel like a trap rather than a checkpoint. `rejected` is set by the
/// `reject_plan` command before it fires the same notifier, so a
/// rejection is a normal answer rather than a timeout.
///
/// Returns `true` for a rejection.
async fn await_answer(notify: &Arc<Notify>, rejected: &Arc<AtomicBool>) -> Result<bool> {
    tokio::time::timeout(ANSWER_TIMEOUT, notify.notified())
        .await
        .map_err(|_| Error::PlanTimeout)?;
    Ok(rejected.swap(false, Ordering::SeqCst))
}

/// A model step's tool calls: `(id, name, arguments as streamed)`.
pub(crate) type Calls = [(String, String, String)];

/// The step to put in front of the user, or `None` when nothing in it
/// would change anything.
///
/// A step is held when at least one of its calls would reach a tool that
/// mutates. A call that would be refused before it ran (not permitted,
/// unknown, arguments that fail the schema) runs nothing and does not
/// count. [`ToolDispatcher::would_mutate`] shares its pre-dispatch checks
/// with `invoke`, so the gate and the dispatch cannot disagree about which
/// calls those are.
///
/// When it is held, **every** call in the step is listed, in order,
/// including the read-only ones: approving runs all of them, so the card
/// shows all of them.
pub(crate) fn held_step(
    d: &ToolDispatcher,
    calls: &Calls,
    allowed: Option<&HashSet<String>>,
    user_message: &str,
) -> Option<Vec<Value>> {
    let mutates = calls.iter().any(|(_, name, args_json)| {
        parse_tool_args(args_json)
            .map(|args| d.would_mutate(name, &args, allowed))
            .unwrap_or(false)
    });
    if !mutates {
        return None;
    }
    Some(
        calls
            .iter()
            .enumerate()
            .map(|(i, (_, name, args_json))| {
                // The wire name from the model, never `Tool::name()`,
                // which leaks a `String` per call for an MCP tool.
                let schema = d.get(name).map(|t| t.schema());
                json!({
                    "step": i + 1,
                    "tool": name,
                    "description": describe_call(schema.as_ref(), name, args_json, user_message),
                })
            })
            .collect(),
    )
}

/// Longest value shown on the card before it is cut.
const VALUE_CAP: usize = 60;
/// Longest description of a call's own arguments. The steps of a recipe it
/// replays are listed after that, in full (see [`recipe_steps`]).
const DESCRIPTION_CAP: usize = 300;
/// Longest array shown inline as `[a, b, c]`.
const INLINE_ARRAY_MAX: usize = 8;

/// The tools that replay a recipe file, and the argument that names it.
///
/// Their arguments say where the steps are, not what they are: the steps
/// are in the file, and the card is meant to show what will run. The test
/// `the_recipe_tools_are_real_and_take_their_file_as_a_required_string`
/// checks these names against the real tools.
const RECIPE_RUNNERS: [(&str, &str); 2] =
    [("apply_recipe", "path"), ("batch_apply", "recipe_path")];

/// Largest file the gate reads to find a recipe's steps. A recipe is a few
/// kilobytes of JSON; a path the model chose that is bigger than this is
/// not one, and is not read into memory before the user has said yes.
const RECIPE_READ_CAP: u64 = 4 * 1024 * 1024;

/// `text`, cut to at most `max` characters, ending in `…` if it was cut.
/// Counts and cuts on characters, never inside a multi-byte one.
fn shorten(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// One argument value as the card shows it.
fn render_value(v: &Value) -> String {
    match v {
        Value::String(s) => serde_json::to_string(&shorten(s, VALUE_CAP))
            .unwrap_or_else(|_| format!("\"{}\"", shorten(s, VALUE_CAP))),
        Value::Number(_) | Value::Bool(_) => v.to_string(),
        Value::Null => "null".to_string(),
        Value::Array(items)
            if items.len() <= INLINE_ARRAY_MAX
                && items.iter().all(|i| !i.is_array() && !i.is_object()) =>
        {
            let inner: Vec<String> = items.iter().map(render_value).collect();
            format!("[{}]", inner.join(", "))
        }
        other => shorten(&other.to_string(), VALUE_CAP),
    }
}

/// A tool call's concrete arguments as readable text, for the approval
/// card. The point of showing a held step is that the user sees what will
/// actually run, so this is the arguments themselves, never a paraphrase
/// of them.
///
/// Required parameters come first, in the order the schema lists them
/// (the only reliable order: `serde_json`'s map order depends on its
/// features), then the rest alphabetically.
///
/// A call that replays a recipe file ([`RECIPE_RUNNERS`]) is followed by
/// the steps in that file ([`recipe_steps`]).
pub(crate) fn describe_call(
    schema: Option<&Value>,
    name: &str,
    args_json: &str,
    user_message: &str,
) -> String {
    let args = match parse_tool_args(args_json) {
        Ok(v) => v,
        Err(_) => {
            return "arguments are not valid JSON; this call will be refused".to_string();
        }
    };
    let Value::Object(map) = &args else {
        return shorten(&args.to_string(), DESCRIPTION_CAP);
    };

    // A tool that reads its range from the `[apply to …]` prefix of the
    // chat message acts on the user's selection when the call names no
    // range. Without saying so, the card would read "reverse — track: 0"
    // while only the selection is reversed.
    let selection = (tools::READS_RANGE_FROM_MESSAGE.contains(&name)
        && map.get("range").is_none_or(Value::is_null))
    .then(|| tools::util::range_resolver::from_message(user_message))
    .flatten();

    let mut keys: Vec<&str> = Vec::with_capacity(map.len());
    let required = schema
        .and_then(|s| s.get("input_schema"))
        .and_then(|s| s.get("required"))
        .and_then(Value::as_array);
    for key in required.into_iter().flatten().filter_map(Value::as_str) {
        if map.contains_key(key) && !keys.contains(&key) {
            keys.push(key);
        }
    }
    let mut rest: Vec<&str> = map
        .keys()
        .map(String::as_str)
        .filter(|k| !keys.contains(k))
        .collect();
    rest.sort_unstable();
    keys.extend(rest);

    let mut parts: Vec<String> = keys
        .into_iter()
        // A null `range` is the same as none; the selection below says
        // what is actually used.
        .filter(|k| !(selection.is_some() && *k == "range"))
        .map(|k| format!("{k}: {}", render_value(&map[k])))
        .collect();
    if let Some(r) = selection {
        parts.push(format!(
            "range: {:.2}–{:.2} s (your selection)",
            r.start_sec, r.end_sec
        ));
    }

    let own = if parts.is_empty() {
        "no arguments".to_string()
    } else {
        shorten(&parts.join(", "), DESCRIPTION_CAP)
    };
    match recipe_steps(name, map) {
        Some(steps) => format!("{own}; {steps}"),
        None => own,
    }
}

/// What a recipe-replaying call will run, read from the recipe file its
/// arguments name: `recipe steps (3): normalize → reverb → fade`.
///
/// `apply_recipe` and `batch_apply` carry only the path, so the card used
/// to show a path and nothing of the edits the user was approving. Every
/// step's tool is listed and none is cut: neither [`VALUE_CAP`] nor
/// [`DESCRIPTION_CAP`] applies, as they keep one argument from filling the
/// card and these are the call's whole effect. The parameters of each step
/// stay in the file.
///
/// `None` for any other tool, and for a call that names no file. A file
/// that cannot be read says so and that the call will fail, which is what
/// the tool itself will answer: showing a list of steps for a call that
/// then fails would be worse than showing none.
fn recipe_steps(name: &str, args: &serde_json::Map<String, Value>) -> Option<String> {
    let (_, arg) = RECIPE_RUNNERS.iter().find(|(tool, _)| *tool == name)?;
    let path = args.get(*arg)?.as_str()?;
    Some(match read_recipe(path) {
        Ok(recipe) => {
            let tools: Vec<&str> = recipe.steps.iter().map(|s| s.tool.as_str()).collect();
            format!("recipe steps ({}): {}", tools.len(), tools.join(" → "))
        }
        Err(why) => format!(
            "the recipe could not be read ({}); this call will fail",
            shorten(&why, VALUE_CAP * 2)
        ),
    })
}

/// The recipe at `path`, or why there is not one.
///
/// This runs at the approval gate, on a path the model chose, before the
/// user has said yes. So it opens only a regular file of a size a recipe
/// could be: a device, a pipe or a gigabyte of audio is not read, which
/// would stall the turn or fill memory.
fn read_recipe(path: &str) -> std::result::Result<tools::Recipe, String> {
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if !meta.is_file() {
        return Err("it is not a file".to_string());
    }
    if meta.len() > RECIPE_READ_CAP {
        return Err("it is too large to be a recipe".to_string());
    }
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    tools::recipe::parse(&text)
}

/// Answer a step without running it: one the user declined, revised or
/// did not answer, one that would have gone past the tool budget, or the
/// rest of one that a failing call ended the turn in the middle of.
///
/// Emits `ToolCallNotRun` for every held id, so no badge is left reading
/// "running" and none reads as a failure, and records one user message holding a
/// `tool_result` for every `tool_use` in the step, plus the user's own
/// words after them when there are any. The pairing has to hold on every
/// path: an assistant `tool_use` with no `tool_result` makes the next
/// request a 400.
pub(crate) fn not_run(
    on_event: &mut impl FnMut(AgentEvent),
    conversation: &mut Vec<Message>,
    calls: &Calls,
    content: &str,
    guidance: Option<String>,
) {
    not_run_after(on_event, conversation, Vec::new(), calls, content, guidance);
}

/// [`not_run`] for a step that was cut short part way through, where the
/// calls before the cut already have their answers.
///
/// `answered` are those answers, in the order the calls were made: the
/// results of the calls that ran, and of the one that failed. They stay
/// first and are not touched, because they are true: a call that ran and
/// changed the session must not be reported as "not run". `calls` are the
/// ones the step never reached, and each is answered `content`. All of it
/// is one user message, so the step's `tool_use`s are answered together,
/// in order, in the message right after them.
pub(crate) fn not_run_after(
    on_event: &mut impl FnMut(AgentEvent),
    conversation: &mut Vec<Message>,
    answered: Vec<ContentBlock>,
    calls: &Calls,
    content: &str,
    guidance: Option<String>,
) {
    let mut blocks = answered;
    blocks.reserve(calls.len() + 1);
    for (id, _, _) in calls {
        on_event(AgentEvent::ToolCallNotRun { id: id.clone() });
        blocks.push(ContentBlock::ToolResult {
            tool_use_id: id.clone(),
            content: content.to_string(),
            is_error: Some(true),
        });
    }
    if let Some(text) = guidance {
        blocks.push(ContentBlock::Text { text });
    }
    conversation.push(Message {
        role: Role::User,
        content: blocks,
    });
}

/// Add the user's words to the conversation without ever putting two
/// user messages side by side.
///
/// After a declined or timed-out step the conversation ends in a user
/// message of `tool_result`s. The next turn's text joins it, as
/// `[tool_result…, text]`, instead of starting a second user message that
/// providers reject. It repairs the same shape after any error exit,
/// which left the conversation ending in a user message it never
/// answered.
pub(crate) fn push_user_text(conversation: &mut Vec<Message>, text: String) {
    if let Some(last) = conversation.last_mut() {
        if last.role == Role::User {
            match last.content.last_mut() {
                // One text block rather than two: chat-completions
                // providers join a message's text blocks with nothing in
                // between.
                Some(ContentBlock::Text { text: prev }) => {
                    prev.push_str("\n\n");
                    prev.push_str(&text);
                }
                _ => last.content.push(ContentBlock::Text { text }),
            }
            return;
        }
    }
    conversation.push(Message {
        role: Role::User,
        content: vec![ContentBlock::Text { text }],
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use tools::schema::{anthropic_tool, object_schema};
    use tools::{Tool, ToolContext, ToolResult};

    // ------------------------------------------------------------------
    // ask
    // ------------------------------------------------------------------

    fn gate() -> (Arc<Notify>, Arc<AtomicBool>, Arc<Mutex<Option<String>>>) {
        (
            Arc::new(Notify::new()),
            Arc::new(AtomicBool::new(false)),
            Arc::new(Mutex::new(None)),
        )
    }

    /// A permit left by an earlier gate must not answer this one.
    ///
    /// Without the drain in [`ask`], the stale `notify_one` is consumed by
    /// the wait and the result is `Approved` at once, before the user has
    /// seen the card.
    #[tokio::test]
    async fn ask_ignores_a_stale_permit() {
        let (notify, rejected, slot) = gate();
        notify.notify_one(); // a stray click from some earlier turn

        let (n, r) = (Arc::clone(&notify), Arc::clone(&rejected));
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            r.store(true, Ordering::SeqCst);
            n.notify_one();
        });

        let mut events = Vec::new();
        let answer = ask(&mut |e| events.push(e), vec![], &notify, &rejected, &slot)
            .await
            .expect("answered");
        assert_eq!(answer, Approval::Rejected);
        assert!(matches!(events.as_slice(), [AgentEvent::Plan { .. }]));
    }

    #[tokio::test]
    async fn ask_returns_revised_when_the_override_is_set() {
        let (notify, rejected, slot) = gate();
        let (n, s) = (Arc::clone(&notify), Arc::clone(&slot));
        let answer = ask(
            &mut |event| {
                // The UI answers the card it was just shown.
                if matches!(event, AgentEvent::Plan { .. }) {
                    *s.lock().unwrap() = Some("1. track: 1".to_string());
                    n.notify_one();
                }
            },
            vec![],
            &notify,
            &rejected,
            &slot,
        )
        .await
        .expect("answered");
        assert_eq!(answer, Approval::Revised("1. track: 1".to_string()));
        assert!(slot.lock().unwrap().is_none(), "the override was consumed");
    }

    #[tokio::test]
    async fn ask_approves_when_nothing_was_changed() {
        let (notify, rejected, slot) = gate();
        let n = Arc::clone(&notify);
        let answer = ask(&mut |_| n.notify_one(), vec![], &notify, &rejected, &slot)
            .await
            .expect("answered");
        assert_eq!(answer, Approval::Approved);
    }

    /// An override left in the slot by an abandoned gate is not this
    /// card's revision.
    #[tokio::test]
    async fn ask_clears_a_stale_override_before_emitting() {
        let (notify, rejected, slot) = gate();
        *slot.lock().unwrap() = Some("left over".to_string());
        let (n, s) = (Arc::clone(&notify), Arc::clone(&slot));
        let mut seen_at_emit = None;
        let answer = ask(
            &mut |event| {
                if matches!(event, AgentEvent::Plan { .. }) {
                    seen_at_emit = Some(s.lock().unwrap().clone());
                    n.notify_one();
                }
            },
            vec![],
            &notify,
            &rejected,
            &slot,
        )
        .await
        .expect("answered");
        assert_eq!(seen_at_emit, Some(None), "the slot was not cleared first");
        assert_eq!(answer, Approval::Approved);
    }

    // ------------------------------------------------------------------
    // describe_call
    // ------------------------------------------------------------------

    fn schema_requiring(required: &[&str]) -> Value {
        json!({
            "name": "t",
            "description": "",
            "input_schema": { "type": "object", "required": required },
        })
    }

    fn describe(schema: Option<&Value>, name: &str, args: &str) -> String {
        describe_call(schema, name, args, "")
    }

    #[test]
    fn required_keys_come_first_then_the_rest_alphabetically() {
        let schema = schema_requiring(&["track", "db"]);
        assert_eq!(
            describe(
                Some(&schema),
                "t",
                r#"{"zeta":1,"db":3,"alpha":true,"track":0}"#
            ),
            "track: 0, db: 3, alpha: true, zeta: 1"
        );
        // Without a schema everything is "the rest".
        assert_eq!(
            describe(None, "t", r#"{"track":0,"db":3}"#),
            "db: 3, track: 0"
        );
    }

    #[test]
    fn a_simple_call_reads_as_its_arguments() {
        let schema = schema_requiring(&["track"]);
        assert_eq!(
            describe(Some(&schema), "reverse", r#"{"track":0}"#),
            "track: 0"
        );
    }

    #[test]
    fn strings_are_quoted_and_long_ones_cut() {
        assert_eq!(
            describe(None, "t", r#"{"kind":"low_pass"}"#),
            r#"kind: "low_pass""#
        );
        let long = "x".repeat(200);
        let out = describe(None, "t", &format!(r#"{{"text":"{long}"}}"#));
        assert!(out.ends_with("…\""), "{out}");
        assert!(out.chars().count() < 80, "{out}");
    }

    #[test]
    fn nested_values_are_compact_and_cut() {
        assert_eq!(
            describe(None, "t", r#"{"params":{"cutoff_hz":1000}}"#),
            r#"params: {"cutoff_hz":1000}"#
        );
        let many: Vec<u32> = (0..100).collect();
        let out = describe(None, "t", &json!({ "points": many }).to_string());
        assert!(out.starts_with("points: [0,1,2"), "{out}");
        assert!(out.ends_with('…'), "{out}");
    }

    #[test]
    fn a_short_scalar_array_is_inline() {
        assert_eq!(
            describe(None, "t", r#"{"tracks":[0,1,2]}"#),
            "tracks: [0, 1, 2]"
        );
        assert_eq!(
            describe(None, "t", r#"{"tags":["a","b"]}"#),
            r#"tags: ["a", "b"]"#
        );
        // Nine elements is too many to call short.
        let out = describe(None, "t", r#"{"n":[1,2,3,4,5,6,7,8,9]}"#);
        assert_eq!(out, "n: [1,2,3,4,5,6,7,8,9]");
    }

    #[test]
    fn no_arguments_says_so() {
        assert_eq!(describe(None, "t", "{}"), "no arguments");
        // A call with no arguments streams as nothing at all (#409).
        assert_eq!(describe(None, "t", ""), "no arguments");
    }

    #[test]
    fn malformed_arguments_say_they_will_be_refused() {
        let out = describe(None, "t", r#"{"track":"#);
        assert!(out.contains("not valid JSON"), "{out}");
    }

    #[test]
    fn arguments_that_are_not_an_object_are_shown_compactly() {
        assert_eq!(describe(None, "t", "[1,2]"), "[1,2]");
    }

    /// `reverse` with no `range` acts on the selection the message names.
    #[test]
    fn a_selection_taken_from_the_message_is_shown() {
        let schema = schema_requiring(&["track"]);
        let message = "[apply to 0:01-0:03.5] reverse this";
        let out = describe_call(Some(&schema), "reverse", r#"{"track":0}"#, message);
        assert_eq!(out, "track: 0, range: 1.00–3.50 s (your selection)");

        // A null range is no range.
        let out = describe_call(
            Some(&schema),
            "reverse",
            r#"{"track":0,"range":null}"#,
            message,
        );
        assert_eq!(out, "track: 0, range: 1.00–3.50 s (your selection)");
    }

    #[test]
    fn an_explicit_range_wins_and_is_not_relabelled() {
        let out = describe_call(
            None,
            "reverse",
            r#"{"track":0,"range":{"start_sec":2,"end_sec":4}}"#,
            "[apply to 0:01-0:03] reverse this",
        );
        assert!(!out.contains("your selection"), "{out}");
        assert!(out.contains("start_sec"), "{out}");
    }

    #[test]
    fn a_tool_that_ignores_the_message_gets_no_selection() {
        let out = describe_call(
            None,
            "set_track_gain",
            r#"{"track":0,"db":3}"#,
            "[apply to 0:01-0:03]",
        );
        assert!(!out.contains("your selection"), "{out}");
    }

    #[test]
    fn a_selection_with_no_other_arguments_still_shows() {
        let out = describe_call(None, "reverse", "{}", "[apply to 0:00-0:02]");
        assert_eq!(out, "range: 0.00–2.00 s (your selection)");
    }

    #[test]
    fn the_description_never_exceeds_its_cap_or_splits_a_character() {
        // 'é' is two bytes, '日' three, '🎵' four: a byte-indexed cut
        // would panic or split one.
        let args = json!({
            "a": "é".repeat(500),
            "b": "日本語".repeat(100),
            "c": "🎵".repeat(500),
            "d": "é".repeat(500),
            "e": "日".repeat(500),
            "f": "🎵".repeat(500),
        })
        .to_string();
        let out = describe(None, "t", &args);
        assert!(
            out.chars().count() <= DESCRIPTION_CAP,
            "{}",
            out.chars().count()
        );
        assert!(out.ends_with('…'));
    }

    // ------------------------------------------------------------------
    // describe_call: the steps of a recipe (#438)
    // ------------------------------------------------------------------

    /// A recipe file holding steps for `names`, in order. Returns its path.
    fn recipe_file(dir: &std::path::Path, names: &[&str]) -> String {
        let steps: Vec<Value> = names
            .iter()
            .map(|tool| json!({ "tool": tool, "params": {}, "replayable": true }))
            .collect();
        let recipe = json!({
            "format_version": 1,
            "engine_version": "test",
            "steps": steps,
        });
        let path = dir.join("chain.json");
        std::fs::write(&path, recipe.to_string()).expect("write recipe");
        path.to_string_lossy().into_owned()
    }

    /// `apply_recipe` takes a path, not the steps, so what it will do is in
    /// the file. The card said `path: "…"` and nothing else.
    #[test]
    fn a_recipe_call_lists_the_steps_it_will_run() {
        let dir = tempfile::tempdir().unwrap();
        let path = recipe_file(dir.path(), &["normalize", "low_pass_filter", "fade"]);
        let out = describe(None, "apply_recipe", &json!({ "path": path }).to_string());
        // The path is cut to the value cap like any argument, and a temp
        // dir's length depends on the OS (macOS's is past the cap), so only
        // its start is pinned; the steps after it are never cut.
        assert!(out.starts_with("path: \""), "{out}");
        assert!(
            out.ends_with("; recipe steps (3): normalize → low_pass_filter → fade"),
            "{out}"
        );
    }

    #[test]
    fn a_batch_call_lists_the_steps_of_its_recipe_too() {
        let dir = tempfile::tempdir().unwrap();
        let path = recipe_file(dir.path(), &["trim_silence", "normalize"]);
        let args = json!({ "recipe_path": path, "input_dir": "/takes" }).to_string();
        let out = describe(None, "batch_apply", &args);
        assert!(
            out.ends_with("; recipe steps (2): trim_silence → normalize"),
            "{out}"
        );
        assert!(out.contains("input_dir: \"/takes\""), "{out}");
    }

    /// Neither cap applies to the steps: both exist to keep one argument
    /// from filling the card, and the steps are what the call does.
    #[test]
    fn the_steps_are_shown_in_full_past_both_caps() {
        // A folder name far longer than a value is shown at: the file is
        // read from the argument, not from its shortened rendering.
        let dir = tempfile::tempdir().unwrap();
        let deep = dir.path().join("d".repeat(VALUE_CAP * 2));
        std::fs::create_dir(&deep).unwrap();
        let names: Vec<String> = (0..60).map(|i| format!("effect_number_{i}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let path = recipe_file(&deep, &refs);

        let out = describe(None, "apply_recipe", &json!({ "path": path }).to_string());
        assert!(out.chars().count() > DESCRIPTION_CAP, "{out}");
        assert!(
            out.ends_with(&format!("recipe steps (60): {}", names.join(" → "))),
            "{out}"
        );
        for name in &names {
            assert!(out.contains(name.as_str()), "{name} was cut: {out}");
        }
    }

    #[test]
    fn a_recipe_that_cannot_be_read_says_the_call_will_fail() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("none.json").to_string_lossy().into_owned();
        let out = describe(
            None,
            "apply_recipe",
            &json!({ "path": missing }).to_string(),
        );
        assert!(out.contains("recipe could not be read"), "{out}");
        assert!(out.ends_with("this call will fail"), "{out}");

        // A folder is not a recipe.
        let folder = dir.path().to_string_lossy().into_owned();
        let out = describe(None, "apply_recipe", &json!({ "path": folder }).to_string());
        assert!(out.contains("recipe could not be read"), "{out}");

        // Not a recipe, and a recipe with nothing in it.
        std::fs::write(dir.path().join("text.json"), "not json").unwrap();
        let text = dir.path().join("text.json").to_string_lossy().into_owned();
        let out = describe(
            None,
            "batch_apply",
            &json!({ "recipe_path": text }).to_string(),
        );
        assert!(out.contains("not a readable recipe"), "{out}");
        let empty = recipe_file(dir.path(), &[]);
        let out = describe(None, "apply_recipe", &json!({ "path": empty }).to_string());
        assert!(out.contains("no steps"), "{out}");
    }

    /// The approval gate reads a path the model chose, before the user has
    /// said yes. A file that is plainly not a recipe is not read whole.
    #[test]
    fn a_file_too_large_to_be_a_recipe_is_not_read() {
        let dir = tempfile::tempdir().unwrap();
        let big = dir.path().join("big.json");
        std::fs::write(&big, vec![b' '; RECIPE_READ_CAP as usize + 1]).unwrap();
        let out = describe(
            None,
            "apply_recipe",
            &json!({ "path": big.to_string_lossy() }).to_string(),
        );
        assert!(out.contains("too large"), "{out}");
    }

    /// Only the two tools that replay a recipe have steps to show. A tool
    /// that merely takes a `path` is not opened.
    #[test]
    fn other_tools_with_a_path_are_not_read() {
        let dir = tempfile::tempdir().unwrap();
        let path = recipe_file(dir.path(), &["normalize"]);
        let args = json!({ "path": path }).to_string();
        let out = describe(None, "load", &args);
        assert!(!out.contains("recipe steps"), "{out}");
        // Nor is a call that names no file.
        assert_eq!(describe(None, "apply_recipe", "{}"), "no arguments");
        assert_eq!(describe(None, "apply_recipe", r#"{"path":7}"#), "path: 7");
    }

    /// The table above names tools and arguments by string. If one is
    /// renamed, the card would quietly go back to showing a bare path.
    #[test]
    fn the_recipe_tools_are_real_and_take_their_file_as_a_required_string() {
        let d = ToolDispatcher::default_dispatcher();
        for (tool, arg) in RECIPE_RUNNERS {
            let schema = d
                .get(tool)
                .unwrap_or_else(|| panic!("{tool} is not a registered tool"))
                .schema();
            let input = &schema["input_schema"];
            assert_eq!(input["properties"][arg]["type"], "string", "{tool}.{arg}");
            let required: Vec<&str> = input["required"]
                .as_array()
                .expect("required")
                .iter()
                .filter_map(Value::as_str)
                .collect();
            assert!(required.contains(&arg), "{tool}.{arg} is not required");
        }
    }

    // ------------------------------------------------------------------
    // held_step
    // ------------------------------------------------------------------

    /// A tool that mutates (the default) and one that does not.
    struct Edit;
    struct Peek;

    impl Tool for Edit {
        fn name(&self) -> &'static str {
            "edit"
        }
        fn schema(&self) -> Value {
            anthropic_tool(
                "edit",
                "edits",
                object_schema(&[("track", "integer", true)]),
            )
        }
        fn invoke(&self, _: Value, _: &mut ToolContext) -> tools::Result<ToolResult> {
            Ok(ToolResult::Ok(Value::Null))
        }
    }

    impl Tool for Peek {
        fn name(&self) -> &'static str {
            "peek"
        }
        fn schema(&self) -> Value {
            anthropic_tool(
                "peek",
                "reads",
                object_schema(&[("track", "integer", true)]),
            )
        }
        fn mutates(&self) -> bool {
            false
        }
        fn invoke(&self, _: Value, _: &mut ToolContext) -> tools::Result<ToolResult> {
            Ok(ToolResult::Ok(Value::Null))
        }
    }

    fn dispatcher() -> ToolDispatcher {
        let mut d = ToolDispatcher::new();
        d.register(Box::new(Edit));
        d.register(Box::new(Peek));
        d
    }

    fn call(id: &str, name: &str, args: &str) -> (String, String, String) {
        (id.to_string(), name.to_string(), args.to_string())
    }

    #[test]
    fn held_step_is_none_without_a_mutating_call() {
        let d = dispatcher();
        let step = [call("a", "peek", r#"{"track":0}"#)];
        assert!(held_step(&d, &step, None, "").is_none());

        // Calls that would be refused run nothing.
        let step = [
            call("a", "no_such_tool", "{}"),
            call("b", "edit", r#"{"track":"zero"}"#), // fails the schema
            call("c", "edit", r#"{"track":"#),        // not JSON
        ];
        assert!(held_step(&d, &step, None, "").is_none());

        // Not permitted this turn.
        let only_peek: HashSet<String> = ["peek".to_string()].into();
        let step = [call("a", "edit", r#"{"track":0}"#)];
        assert!(held_step(&d, &step, Some(&only_peek), "").is_none());
    }

    /// `list_session` is how the model reaches the part of the session the
    /// block cut (#618), and it is called page after page. Plan first holds a
    /// step only for a call that can change the session, so a step of these
    /// runs without the user being asked, on the real registry and not on a
    /// stand-in: the tool must really be registered read-only.
    #[test]
    fn a_step_of_list_session_calls_is_not_held() {
        let d = ToolDispatcher::default_dispatcher();
        assert!(
            d.get("list_session").is_some(),
            "list_session is not registered"
        );
        let step = [
            call("a", "list_session", r#"{"what":"tracks"}"#),
            call(
                "b",
                "list_session",
                r#"{"what":"markers","offset":100,"limit":50}"#,
            ),
        ];
        assert!(held_step(&d, &step, None, "").is_none());

        // The control: the same step with an edit in it is held, and then
        // every call is listed, `list_session` among them.
        let with_edit = [
            call("a", "list_session", r#"{"what":"buses"}"#),
            call("b", "set_track_gain", r#"{"track":0,"db":3.0}"#),
        ];
        let held = held_step(&d, &with_edit, None, "").expect("an edit is held");
        assert_eq!(held.len(), 2);
    }

    #[test]
    fn held_step_lists_every_call_in_order() {
        let d = dispatcher();
        let step = [
            call("a", "peek", r#"{"track":0}"#),
            call("b", "edit", r#"{"track":1}"#),
        ];
        let steps = held_step(&d, &step, None, "").expect("held");
        assert_eq!(
            steps,
            vec![
                json!({"step": 1, "tool": "peek", "description": "track: 0"}),
                json!({"step": 2, "tool": "edit", "description": "track: 1"}),
            ]
        );
    }

    /// Through the real registry, which is what the agent loop holds: the
    /// card for a replayed recipe carries the steps in its description.
    #[test]
    fn a_held_recipe_call_carries_its_steps_to_the_card() {
        let dir = tempfile::tempdir().unwrap();
        let path = recipe_file(dir.path(), &["normalize", "reverb", "fade"]);
        let d = ToolDispatcher::default_dispatcher();
        let step = [call(
            "r1",
            "apply_recipe",
            &json!({ "path": path }).to_string(),
        )];

        let steps = held_step(&d, &step, None, "").expect("a recipe edits the session");
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0]["tool"], "apply_recipe");
        let description = steps[0]["description"].as_str().expect("text");
        assert!(
            description.ends_with("recipe steps (3): normalize → reverb → fade"),
            "{description}"
        );
    }

    // ------------------------------------------------------------------
    // not_run, push_user_text
    // ------------------------------------------------------------------

    fn three_calls() -> Vec<(String, String, String)> {
        vec![
            call("a", "edit", "{}"),
            call("b", "edit", "{}"),
            call("c", "peek", "{}"),
        ]
    }

    #[test]
    fn not_run_answers_every_tool_use_id() {
        let calls = three_calls();
        let mut events = Vec::new();
        let mut conversation = Vec::new();
        not_run(
            &mut |e| events.push(e),
            &mut conversation,
            &calls,
            DECLINED,
            None,
        );

        let not_run_ids: Vec<&str> = events
            .iter()
            .map(|e| match e {
                AgentEvent::ToolCallNotRun { id } => id.as_str(),
                other => panic!("unexpected event {other:?}"),
            })
            .collect();
        assert_eq!(not_run_ids, vec!["a", "b", "c"]);

        assert_eq!(conversation.len(), 1);
        assert_eq!(conversation[0].role, Role::User);
        let ids: Vec<&str> = conversation[0]
            .content
            .iter()
            .map(|b| match b {
                ContentBlock::ToolResult {
                    tool_use_id,
                    content,
                    is_error,
                } => {
                    assert_eq!(content, DECLINED);
                    assert_eq!(*is_error, Some(true));
                    tool_use_id.as_str()
                }
                other => panic!("unexpected block {other:?}"),
            })
            .collect();
        assert_eq!(ids, vec!["a", "b", "c"]);
    }

    #[test]
    fn not_run_appends_guidance_after_the_results() {
        let calls = three_calls();
        let mut conversation = Vec::new();
        not_run(
            &mut |_| {},
            &mut conversation,
            &calls,
            REVISED,
            Some("do this instead".to_string()),
        );
        let blocks = &conversation[0].content;
        assert_eq!(blocks.len(), 4);
        assert!(blocks[..3]
            .iter()
            .all(|b| matches!(b, ContentBlock::ToolResult { .. })));
        assert!(matches!(
            &blocks[3],
            ContentBlock::Text { text } if text == "do this instead"
        ));
    }

    #[test]
    fn push_user_text_merges_into_a_trailing_user_message() {
        let mut conversation = vec![Message {
            role: Role::User,
            content: vec![ContentBlock::ToolResult {
                tool_use_id: "a".into(),
                content: DECLINED.into(),
                is_error: Some(true),
            }],
        }];
        push_user_text(&mut conversation, "next request".to_string());
        assert_eq!(conversation.len(), 1, "two user messages in a row");
        assert_eq!(conversation[0].content.len(), 2);
        assert!(matches!(
            &conversation[0].content[1],
            ContentBlock::Text { text } if text == "next request"
        ));

        // Text already there joins into one block, not two.
        let mut conversation = vec![Message {
            role: Role::User,
            content: vec![ContentBlock::Text {
                text: "first".into(),
            }],
        }];
        push_user_text(&mut conversation, "second".to_string());
        assert!(matches!(
            conversation[0].content.as_slice(),
            [ContentBlock::Text { text }] if text == "first\n\nsecond"
        ));
    }

    #[test]
    fn push_user_text_pushes_after_an_assistant_message() {
        let mut conversation = vec![Message {
            role: Role::Assistant,
            content: vec![ContentBlock::Text { text: "hi".into() }],
        }];
        push_user_text(&mut conversation, "hello".to_string());
        assert_eq!(conversation.len(), 2);
        assert_eq!(conversation[1].role, Role::User);

        let mut empty = Vec::new();
        push_user_text(&mut empty, "hello".to_string());
        assert_eq!(empty.len(), 1);
    }
}
