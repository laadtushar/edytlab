//! How big the agent's first request is, per provider, and what a
//! small-context local model is sent instead (#395).
//!
//! The native end-to-end run sent "Make track 0 louder by 6 dB." to a
//! local model and got back
//!
//! ```text
//! request (15157 tokens) exceeds the available context size (8192 tokens)
//! ```
//!
//! Nothing recorded how big a request was, so the report could only give
//! the total. These tests drive the real `Agent` against a mocked server,
//! take the first streaming request it makes, and split it into the parts
//! a fix has to be aimed at: the system prompt, the tool schemas and the
//! conversation. Each test prints one line of that split. Read them with
//!
//! ```text
//! cargo test -p ai --test request_size -- --nocapture
//! ```
//!
//! The split is what showed where the size is. The system prompt is
//! small (about 1.2 KB, session block included); the tool schemas are
//! nearly all of it (about 54 KB of 55 KB for Anthropic, 57 KB of 58 KB
//! as chat-completions JSON). So the fix is not a shorter prompt but
//! fewer, terser tools, and only for a provider that needs it:
//!
//! * Anthropic keeps the whole list. Its prompt cache keys on the full
//!   tools-plus-system prefix, so sending a different subset per message
//!   would make every message a cache miss. The ratchets below only make
//!   growth visible.
//! * Ollama sends a core set plus the tools the message (or the
//!   assistant's last message, or an earlier one of the user's) names,
//!   with shortened descriptions, and names every other tool it may use
//!   in one line of the system prompt. The request must fit half of an
//!   8,192-token context. The bound is bytes, at an assumed three bytes
//!   per token.
//!
//! Where three bytes per token comes from: the issue's llama.cpp run
//! counted 15,157 tokens for a request of the whole-list kind, and
//! `openai_first_request_reports_the_full_chat_completions_size` builds
//! that kind of request today. Divided by 15,157 it is 3.84 bytes per
//! token. The registry has grown since the issue, so today's request has
//! more tokens than were counted, and 3.84 is an upper bound on the true
//! ratio, not an estimate of it. Three is a margin under that bound,
//! chosen by judgement. Nothing here measures the true ratio: no
//! tokenizer is run, and the count in that division is fixed, so the
//! assertion there only fails if the whole-list request shrinks below the
//! size the issue measured. That is why the bound is half the context and
//! not all of it.

mod common;

use std::collections::BTreeSet;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use ai::provider::provider_from_id;
use ai::{Agent, AgentEvent, Error, LlmConfig, SessionContext, TrackBrief};
use common::{
    chat_reply_json, chat_sse_text, chat_sse_tool_call, classifier_json, ok, sse_text, Entry,
    SeqResponder,
};
use serde_json::{json, Value};
use tools::ToolDispatcher;
use wiremock::matchers::method;
use wiremock::{Mock, MockServer};

/// The message from the issue's end-to-end run.
const CANONICAL: &str = "Make track 0 louder by 6 dB.";

/// A message that names a dozen and a half tools, to show that naming
/// tools cannot push a request past the bound.
const NAMES_MANY_TOOLS: &str = "add reverb, echo, tremolo, phaser and distortion, pitch shift it, \
     time stretch it, reverse it, split by speaker, add a noise gate, a stereo widener and a \
     de-esser";

/// Bytes per token assumed when turning a context size into a byte
/// budget. An assumption, not a measurement: the top of this file says
/// what is known (an upper bound of 3.84) and what is not.
const BYTES_PER_TOKEN: usize = 3;

/// A first request must fit in half of an 8,192-token context, leaving
/// the other half for the reply and the tool round trips.
const SLIM_FIRST_REQUEST_MAX_BYTES: usize = 4_096 * BYTES_PER_TOKEN;

/// The issue's measured token count for the first request.
const ISSUE_MEASURED_TOKENS: usize = 15_157;

// Ratchets on the request Anthropic gets, which stays the whole tool
// list. They are the measured size plus headroom, not targets. A failure
// means the system prompt or the tool schemas grew: check the growth was
// intended, then raise the constant deliberately and say why in the
// commit. The headroom absorbs one or two new tools.
const ANTHROPIC_SYSTEM_MAX_BYTES: usize = 1_400; // measured 1,203
const FULL_TOOLS_MAX_BYTES: usize = 62_500; // measured 54,133

// ---------------------------------------------------------------------
// Fixture
// ---------------------------------------------------------------------

/// One streaming request the agent sent.
struct Sent {
    body: Value,
    /// The body as it went over the wire, which is what a context window
    /// counts.
    raw_len: usize,
}

struct Turn {
    /// Every streaming request, in order. The classifier's non-streaming
    /// call is left out.
    streaming: Vec<Sent>,
    result: ai::Result<ai::TurnResult>,
}

/// What the session looks like to the model in a native run: a head node
/// and one 30-second track.
fn native_run_context() -> SessionContext {
    SessionContext {
        head: Some("a".repeat(64)),
        tracks: vec![TrackBrief {
            name: "voice".into(),
            clips: vec![(0.0, 30.0)],
            gain_db: 0.0,
            pan: 0.0,
            muted: false,
            soloed: false,
            effects: vec![],
        }],
        ..SessionContext::default()
    }
}

fn config_for(provider_id: &str, base_url: String) -> LlmConfig {
    if provider_id == "anthropic" {
        LlmConfig::new_anthropic("test-key").with_base_url(base_url)
    } else {
        LlmConfig::new(provider_from_id(provider_id), "test-key").with_base_url(base_url)
    }
}

/// Run one turn of `message` against `provider_id`'s wire format, with
/// the mock answering `responses` in order.
async fn run(provider_id: &str, message: &str, responses: Vec<Entry>) -> Turn {
    run_with(provider_id, message, responses, false).await
}

/// [`run`], optionally with Plan first on. A plan card is approved as soon
/// as it is shown, the way the UI's Run button does.
async fn run_with(
    provider_id: &str,
    message: &str,
    responses: Vec<Entry>,
    plan_first: bool,
) -> Turn {
    run_conversation(
        provider_id,
        &[message],
        responses,
        Options {
            plan_first,
            whitelist: None,
        },
    )
    .await
}

/// What a test can set on the agent beyond the provider.
#[derive(Default)]
struct Options {
    plan_first: bool,
    /// The tools the agent profile permits. `None` permits all.
    whitelist: Option<Vec<String>>,
}

/// Run `messages` as consecutive turns of one conversation, with the mock
/// answering `responses` in order across all of them. `Turn::streaming`
/// has every streaming request of every turn, in order, and
/// `Turn::result` is the last turn's.
async fn run_conversation(
    provider_id: &str,
    messages: &[&str],
    responses: Vec<Entry>,
    options: Options,
) -> Turn {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(SeqResponder::new(responses))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().expect("tempdir");
    let notify = Arc::new(tokio::sync::Notify::new());
    let mut agent = Agent::new(
        config_for(provider_id, server.uri()),
        Arc::new(Mutex::new(ToolDispatcher::default_dispatcher())),
        Arc::new(Mutex::new(
            session::Store::open(dir.path()).expect("open store"),
        )),
        Arc::new(Mutex::new(audio_engine::Engine::new())),
        Arc::clone(&notify),
        Arc::new(Mutex::new(None)),
        Arc::new(AtomicBool::new(false)),
        Arc::new(Mutex::new(None::<tools::Clipboard>)),
    );
    agent.set_plan_first(options.plan_first);
    if let Some(whitelist) = options.whitelist {
        agent = agent.with_tool_whitelist(whitelist);
    }

    let ctx = native_run_context();
    let mut result = None;
    for message in messages {
        result = Some(
            agent
                .turn_with_context((*message).to_string(), Some(&ctx), |event| {
                    if matches!(event, AgentEvent::Plan { .. }) {
                        notify.notify_one();
                    }
                })
                .await,
        );
    }
    let result = result.expect("at least one message to send");

    let streaming = server
        .received_requests()
        .await
        .expect("recording is on")
        .iter()
        .filter_map(|r| {
            let body: Value = serde_json::from_slice(&r.body).ok()?;
            (body["stream"] == json!(true)).then_some(Sent {
                body,
                raw_len: r.body.len(),
            })
        })
        .collect();
    Turn { streaming, result }
}

/// The classifier's answer and one spoken step, in Anthropic's shape.
fn anthropic_script() -> Vec<Entry> {
    vec![ok(classifier_json("general")), ok(sse_text("Done."))]
}

/// The same, in chat-completions shape.
fn chat_script() -> Vec<Entry> {
    vec![ok(chat_reply_json("general")), ok(chat_sse_text("Done."))]
}

/// A request split into the parts a fix is aimed at.
#[derive(Debug)]
struct Split {
    system: usize,
    tools: usize,
    n_tools: usize,
    messages: usize,
    total: usize,
}

fn len_of(v: &Value) -> usize {
    serde_json::to_string(v).unwrap().len()
}

/// Anthropic's shape: `system` and `tools` are top-level.
fn split_anthropic(sent: &Sent) -> Split {
    Split {
        system: len_of(&sent.body["system"]),
        tools: len_of(&sent.body["tools"]),
        n_tools: sent.body["tools"].as_array().map_or(0, Vec::len),
        messages: len_of(&sent.body["messages"]),
        total: sent.raw_len,
    }
}

/// Chat-completions: the system prompt is the first message.
fn split_chat(sent: &Sent) -> Split {
    let messages = sent.body["messages"].as_array().expect("messages");
    assert_eq!(messages[0]["role"], "system", "{:?}", messages[0]);
    Split {
        system: len_of(&messages[0]),
        tools: len_of(&sent.body["tools"]),
        n_tools: sent.body["tools"].as_array().map_or(0, Vec::len),
        messages: messages[1..].iter().map(len_of).sum(),
        total: sent.raw_len,
    }
}

fn report(label: &str, s: &Split) {
    println!(
        "first request, {label}: system {} B, tools {} B ({} tools), messages {} B, total {} B \
         (about {} tokens at {BYTES_PER_TOKEN} B/token)",
        s.system,
        s.tools,
        s.n_tools,
        s.messages,
        s.total,
        s.total.div_ceil(BYTES_PER_TOKEN)
    );
}

fn registered_tool_names() -> BTreeSet<String> {
    ToolDispatcher::default_dispatcher()
        .tool_names()
        .into_iter()
        .collect()
}

fn tool_names_in(tools: &Value) -> BTreeSet<String> {
    tools
        .as_array()
        .expect("tools array")
        .iter()
        .map(|t| {
            // Anthropic: {name}. Chat-completions: {function: {name}}.
            t.get("name")
                .or_else(|| t.get("function").and_then(|f| f.get("name")))
                .and_then(Value::as_str)
                .expect("a tool name")
                .to_string()
        })
        .collect()
}

/// The tools that describe themselves as doing nothing in this build
/// (stem separation, transcription). A small model is not offered them.
fn not_implemented_tools() -> BTreeSet<String> {
    ToolDispatcher::default_dispatcher()
        .tool_schemas()
        .as_array()
        .expect("tool schemas")
        .iter()
        .filter(|t| {
            t["description"]
                .as_str()
                .is_some_and(|d| d.starts_with("NOT IMPLEMENTED"))
        })
        .map(|t| t["name"].as_str().expect("a name").to_string())
        .collect()
}

/// The system prompt as the model reads it, in either wire shape:
/// Anthropic's `system` blocks, or chat-completions' first message.
fn system_text(body: &Value) -> String {
    if let Some(blocks) = body["system"].as_array() {
        return blocks
            .iter()
            .filter_map(|b| b["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n");
    }
    body["messages"][0]["content"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

/// How the line that names the tools a small model was not sent begins.
const NAMES_LINE_START: &str = "More tools you can call by exact name";

/// The words of the system prompt's names line, or none if it has none.
/// Only that line: a tool name that happens to be an ordinary word
/// elsewhere in the prompt (`gain`, `fade`) must not stand in for it.
fn names_line_words(system: &str) -> BTreeSet<String> {
    system
        .split("\n\n")
        .find(|paragraph| paragraph.starts_with(NAMES_LINE_START))
        .map(|line| {
            line.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .filter(|w| !w.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Every word of the system prompt, for "this appears nowhere in it".
fn all_words(system: &str) -> BTreeSet<String> {
    system
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

// ---------------------------------------------------------------------
// Anthropic: the whole list, cached
// ---------------------------------------------------------------------

/// Anthropic is sent every tool, with the cache breakpoints that make
/// that cheap, and its size is ratcheted so growth shows.
///
/// The system prompt and the tool schemas are bounded separately: one
/// number for the total would let a long prompt hide behind a short tool
/// list, or the reverse.
#[tokio::test]
async fn anthropic_first_request_is_the_full_cached_set() {
    let turn = run("anthropic", CANONICAL, anthropic_script()).await;
    turn.result.as_ref().expect("the turn succeeds");
    let sent = &turn.streaming[0];
    let split = split_anthropic(sent);
    report("anthropic", &split);

    assert_eq!(
        tool_names_in(&sent.body["tools"]),
        registered_tool_names(),
        "Anthropic is sent every registered tool, so its prompt cache stays valid"
    );
    assert!(
        !system_text(&sent.body).contains(NAMES_LINE_START),
        "the names line is for a provider that is not sent every tool; Anthropic's system prompt \
         is part of its cached prefix and must not vary with it"
    );
    assert_eq!(
        sent.body["system"][0]["cache_control"]["type"], "ephemeral",
        "the system prompt is a cache breakpoint"
    );
    let tools = sent.body["tools"].as_array().unwrap();
    assert_eq!(
        tools.last().unwrap()["cache_control"]["type"],
        "ephemeral",
        "the last tool is a cache breakpoint, covering the whole list"
    );

    assert!(
        split.system <= ANTHROPIC_SYSTEM_MAX_BYTES,
        "the system prompt is {} B, over the ratchet of {ANTHROPIC_SYSTEM_MAX_BYTES} B. If it \
         grew on purpose, raise ANTHROPIC_SYSTEM_MAX_BYTES deliberately.",
        split.system
    );
    assert!(
        split.tools <= FULL_TOOLS_MAX_BYTES,
        "the tool schemas are {} B, over the ratchet of {FULL_TOOLS_MAX_BYTES} B. If they grew \
         on purpose, raise FULL_TOOLS_MAX_BYTES deliberately.",
        split.tools
    );
}

/// What a chat-completions provider that is sent the whole list gets, as
/// Ollama was before it had a smaller one. Printed for the record, and
/// used to check the bytes-per-token figure the bound relies on.
#[tokio::test]
async fn openai_first_request_reports_the_full_chat_completions_size() {
    let turn = run("openai", CANONICAL, chat_script()).await;
    turn.result.as_ref().expect("the turn succeeds");
    let split = split_chat(&turn.streaming[0]);
    report("openai (the whole tool list)", &split);

    let bytes_per_token = split.total as f64 / ISSUE_MEASURED_TOKENS as f64;
    println!(
        "the issue's 15,157 tokens over this request is {bytes_per_token:.2} bytes per token \
         (the request has grown since the issue was written, so this is an upper bound)"
    );
    // Not a check of the tokenizer, which is not run: the token count is
    // the issue's, fixed. This fails only if the whole-list request has
    // become smaller than the one the issue measured, and then the figure
    // above says nothing about today's requests.
    assert!(
        bytes_per_token >= BYTES_PER_TOKEN as f64,
        "the issue's 15,157 tokens over this request is {bytes_per_token:.2} bytes per token. \
         The whole-list request has shrunk below the size the issue measured, so that count no \
         longer bounds the ratio: re-measure before trusting BYTES_PER_TOKEN \
         ({BYTES_PER_TOKEN})."
    );
    assert_eq!(
        split.n_tools,
        registered_tool_names().len(),
        "OpenAI is sent every registered tool"
    );
    assert!(
        !system_text(&turn.streaming[0].body).contains(NAMES_LINE_START),
        "a provider that is sent every tool has nothing to be told about"
    );
    assert!(
        split.tools > split.system * 10,
        "the tool schemas are the bulk of the request: tools {} B, system {} B",
        split.tools,
        split.system
    );
}

// ---------------------------------------------------------------------
// Ollama: the slim set
// ---------------------------------------------------------------------

fn assert_fits(label: &str, split: &Split) {
    assert!(
        split.total <= SLIM_FIRST_REQUEST_MAX_BYTES,
        "{label}: the first request is {} B (about {} tokens), over {SLIM_FIRST_REQUEST_MAX_BYTES} \
         B, which is half of an 8,192-token context. system {} B, tools {} B ({} tools), \
         messages {} B.",
        split.total,
        split.total.div_ceil(BYTES_PER_TOKEN),
        split.system,
        split.tools,
        split.n_tools,
        split.messages,
    );
}

/// The issue's acceptance criterion: a request fits in 8,192 tokens.
/// Half of it is the most the first request may take, because the model
/// must still answer, and a tool round trip adds a call and its result.
#[tokio::test]
async fn ollama_first_request_fits_half_an_8k_context() {
    let turn = run("ollama", CANONICAL, chat_script()).await;
    turn.result.as_ref().expect("the turn succeeds");
    let split = split_chat(&turn.streaming[0]);
    report("ollama", &split);

    assert_fits("ollama", &split);
    assert!(
        split.n_tools < registered_tool_names().len(),
        "ollama is sent a smaller tool set, not all {}",
        split.n_tools
    );
    let names = tool_names_in(&turn.streaming[0].body["tools"]);
    assert!(
        names.contains("gain"),
        "the common edits stay available: {names:?}"
    );
}

/// Naming tools adds them, and the budget stops it adding too many.
#[tokio::test]
async fn ollama_stays_within_the_bound_when_the_message_names_many_tools() {
    let turn = run("ollama", NAMES_MANY_TOOLS, chat_script()).await;
    turn.result.as_ref().expect("the turn succeeds");
    let sent = &turn.streaming[0];
    let split = split_chat(sent);
    report("ollama, a message naming many tools", &split);

    assert_fits("many mentions", &split);
    let names = tool_names_in(&sent.body["tools"]);
    for wanted in ["reverb", "echo", "tremolo"] {
        assert!(
            names.contains(wanted),
            "{wanted} was named in the message but not sent: {names:?}"
        );
    }
}

/// Every request of a turn carries the same tools, in the same order.
///
/// llama.cpp reuses the prompt it has already processed when the start of
/// the next request matches, and the tools come before the conversation.
/// A set recomputed per round trip would invalidate that on each one, and
/// on a CPU that is minutes.
#[tokio::test]
async fn ollama_round_trips_in_a_turn_send_identical_tools() {
    let turn = run(
        "ollama",
        CANONICAL,
        vec![
            ok(chat_reply_json("general")),
            // No head in the empty store, so this is a tool error, which
            // goes back to the model like any other result.
            ok(chat_sse_tool_call(
                "call_1",
                "gain",
                r#"{"track":0,"db":6}"#,
            )),
            ok(chat_sse_text("Done.")),
        ],
    )
    .await;
    turn.result.as_ref().expect("the turn succeeds");

    assert_eq!(turn.streaming.len(), 2, "one call, then the answer");
    assert_eq!(
        turn.streaming[0].body["tools"], turn.streaming[1].body["tools"],
        "the tools changed between round trips"
    );
    assert!(turn.streaming[0].body["tools"].is_array());
    // The names line is part of what precedes the conversation, so it is
    // held to the same rule.
    let (first, second) = (
        system_text(&turn.streaming[0].body),
        system_text(&turn.streaming[1].body),
    );
    assert!(first.contains(NAMES_LINE_START), "no names line: {first}");
    assert_eq!(
        first, second,
        "the system prompt changed between round trips"
    );
}

/// The tool results of a request, as chat-completions messages with
/// `role: "tool"`.
fn tool_results(body: &Value) -> Vec<String> {
    body["messages"]
        .as_array()
        .expect("messages")
        .iter()
        .filter(|m| m["role"] == "tool")
        .map(|m| m["content"].as_str().unwrap_or_default().to_string())
        .collect()
}

/// What the model is shown is not what it is allowed. `echo` is permitted
/// but is not in the slim set for this message; a model that calls it
/// anyway still gets it run, which is what the permission check is for
/// (#238). Without this, slimming would also have taken tools away.
#[tokio::test]
async fn ollama_runs_a_permitted_tool_it_was_not_offered() {
    let turn = run(
        "ollama",
        CANONICAL,
        vec![
            ok(chat_reply_json("general")),
            ok(chat_sse_tool_call(
                "call_1",
                "echo",
                r#"{"track":0,"delay_ms":100}"#,
            )),
            ok(chat_sse_text("Done.")),
        ],
    )
    .await;
    turn.result.as_ref().expect("the turn succeeds");

    let offered = tool_names_in(&turn.streaming[0].body["tools"]);
    assert!(!offered.contains("echo"), "echo was offered: {offered:?}");
    // It was not sent in full, but the model was told it exists, which is
    // what makes calling it by name something a model can do.
    assert!(
        names_line_words(&system_text(&turn.streaming[0].body)).contains("echo"),
        "echo was neither sent nor named"
    );
    let results = tool_results(&turn.streaming[1].body);
    assert_eq!(results.len(), 1, "{results:?}");
    assert!(
        results[0].contains("no session loaded"),
        "echo was refused instead of run (with no session it answers that): {}",
        results[0]
    );
}

/// An approved plan names the tools it will use, and the model must be
/// shown them. The message here names none.
#[tokio::test]
async fn ollama_is_shown_the_tools_its_approved_plan_names() {
    let message = "make it sound spacious";

    // A step names its tool as the model wrote it, which is not always a
    // registered name: "add_reverb" is about the `reverb` tool.
    for step_tool in ["reverb", "add_reverb"] {
        let plan = format!(
            r#"I will add a reverb. <plan>[{{"step":1,"tool":"{step_tool}","description":"Add reverb"}}]</plan>"#
        );
        let planned = run_with(
            "ollama",
            message,
            vec![
                ok(chat_reply_json("general")),
                ok(chat_reply_json(&plan)),
                ok(chat_sse_text("Done.")),
            ],
            true,
        )
        .await;
        planned.result.as_ref().expect("the turn succeeds");
        let with_plan = tool_names_in(&planned.streaming[0].body["tools"]);
        assert!(
            with_plan.contains("reverb"),
            "a plan step for {step_tool:?} did not bring in reverb: {with_plan:?}"
        );
    }

    let unplanned = run("ollama", message, chat_script()).await;
    unplanned.result.as_ref().expect("the turn succeeds");
    let without = tool_names_in(&unplanned.streaming[0].body["tools"]);
    assert!(
        !without.contains("reverb"),
        "the message alone must not name reverb, or this test proves nothing: {without:?}"
    );
}

// ---------------------------------------------------------------------
// Telling the model which tools it was not sent
// ---------------------------------------------------------------------

/// A model that cannot see a tool cannot know it exists. So every tool
/// the turn permits is either sent or named in the system prompt, for any
/// message, and the request still fits the bound with the names in it.
#[tokio::test]
async fn ollama_every_permitted_tool_is_sent_or_named_and_the_bound_holds() {
    let stubs = not_implemented_tools();
    assert!(
        !stubs.is_empty(),
        "no tool says it is not implemented: has the marker changed?"
    );
    let permitted: BTreeSet<String> = registered_tool_names()
        .difference(&stubs)
        .cloned()
        .collect();

    for message in [CANONICAL, NAMES_MANY_TOOLS, "hello"] {
        let turn = run("ollama", message, chat_script()).await;
        turn.result.as_ref().expect("the turn succeeds");
        let sent = &turn.streaming[0];
        let split = split_chat(sent);
        report("ollama, tools and the names line", &split);
        assert_fits(message, &split);

        let in_tools = tool_names_in(&sent.body["tools"]);
        let in_line = names_line_words(&system_text(&sent.body));
        let neither: Vec<_> = permitted
            .iter()
            .filter(|n| !in_tools.contains(*n) && !in_line.contains(*n))
            .collect();
        assert!(
            neither.is_empty(),
            "{message:?}: these permitted tools are neither sent nor named: {neither:?}"
        );
        // A tool sent in full is not also listed by name.
        let both: Vec<_> = in_tools.iter().filter(|n| in_line.contains(*n)).collect();
        assert!(both.is_empty(), "{message:?}: sent and listed: {both:?}");

        // A tool that does nothing in this build is offered by neither.
        for stub in &stubs {
            assert!(
                !in_tools.contains(stub) && !all_words(&system_text(&sent.body)).contains(stub),
                "{message:?}: {stub} is not implemented and was offered"
            );
        }
    }
}

/// What the profile or the Capabilities menu took away is not brought
/// back by being named in the message, and not mentioned to the model.
#[tokio::test]
async fn ollama_a_tool_the_profile_does_not_permit_is_in_neither_tools_nor_names() {
    let removed = ["limiter", "echo"];
    let whitelist: Vec<String> = registered_tool_names()
        .into_iter()
        .filter(|n| !removed.contains(&n.as_str()))
        .collect();
    let turn = run_conversation(
        "ollama",
        &["add a limiter and an echo"],
        chat_script(),
        Options {
            whitelist: Some(whitelist),
            ..Options::default()
        },
    )
    .await;
    turn.result.as_ref().expect("the turn succeeds");
    let sent = &turn.streaming[0];
    assert_fits("whitelist", &split_chat(sent));

    let in_tools = tool_names_in(&sent.body["tools"]);
    let words = all_words(&system_text(&sent.body));
    for gone in removed {
        assert!(!in_tools.contains(gone), "{gone} was sent");
        assert!(
            !words.contains(gone),
            "{gone} was named in the system prompt"
        );
    }
    // The control: a tool that is permitted, and not sent, is named.
    assert!(names_line_words(&system_text(&sent.body)).contains("reverb"));
}

/// "Yes, do it" names no tool. After the assistant proposed "a limiter or
/// a de-esser" it means those two, and the model has to be shown them.
#[tokio::test]
async fn ollama_a_yes_after_a_proposal_is_shown_the_tools_proposed() {
    let turn = run_conversation(
        "ollama",
        &[
            "how could this voice sound more professional?",
            "yes, do it",
        ],
        vec![
            ok(chat_reply_json("general")),
            ok(chat_sse_text(
                "I could add a limiter or a de-esser. Want me to?",
            )),
            ok(chat_reply_json("general")),
            ok(chat_sse_text("Done.")),
        ],
        Options::default(),
    )
    .await;
    turn.result.as_ref().expect("the turn succeeds");
    assert_eq!(turn.streaming.len(), 2, "one reply per message");

    // Neither message names the tools, so the first request has neither.
    let first = tool_names_in(&turn.streaming[0].body["tools"]);
    assert!(
        !first.contains("limiter") && !first.contains("de_esser"),
        "the question must not name them, or this proves nothing: {first:?}"
    );

    let second = &turn.streaming[1];
    report("ollama, the follow-up", &split_chat(second));
    assert_fits("follow-up", &split_chat(second));
    let names = tool_names_in(&second.body["tools"]);
    for wanted in ["limiter", "de_esser"] {
        assert!(
            names.contains(wanted),
            "{wanted} was proposed, not sent: {names:?}"
        );
    }

    // The same two words with nothing proposed before them send neither.
    let alone = run("ollama", "yes, do it", chat_script()).await;
    alone.result.as_ref().expect("the turn succeeds");
    let alone_names = tool_names_in(&alone.streaming[0].body["tools"]);
    assert!(
        !alone_names.contains("limiter") && !alone_names.contains("de_esser"),
        "{alone_names:?}"
    );
}

/// A tool the user asked for stays in the set when the next message is
/// about something else, so the set does not churn from message to
/// message (each change costs a local server the prompt it had processed).
#[tokio::test]
async fn ollama_a_tool_named_earlier_stays_and_the_set_only_grows() {
    let turn = run_conversation(
        "ollama",
        &["add some reverb", "now make it a bit louder"],
        vec![
            ok(chat_reply_json("general")),
            ok(chat_sse_text("Done.")),
            ok(chat_reply_json("general")),
            ok(chat_sse_text("Done.")),
        ],
        Options::default(),
    )
    .await;
    turn.result.as_ref().expect("the turn succeeds");
    assert_eq!(turn.streaming.len(), 2);

    let first = tool_names_in(&turn.streaming[0].body["tools"]);
    let second = tool_names_in(&turn.streaming[1].body["tools"]);
    assert!(first.contains("reverb"), "{first:?}");
    assert!(
        second.contains("reverb"),
        "reverb was asked for and not yet used, then dropped: {second:?}"
    );
    assert_eq!(
        first, second,
        "a message that named nothing changed the set"
    );
    assert_fits("second message", &split_chat(&turn.streaming[1]));
}

/// Phrasings that name no tool by a word of its name still reach it, and
/// none of them offers a tool that does nothing in this build.
#[tokio::test]
async fn ollama_loose_phrasings_are_shown_the_tool_they_ask_for() {
    for (message, wanted) in [
        ("remove the reverb from track 1", "remove_effect"),
        ("make the vocals less harsh", "de_esser"),
        ("export each track separately", "export_multiple"),
    ] {
        let turn = run("ollama", message, chat_script()).await;
        turn.result.as_ref().expect("the turn succeeds");
        let sent = &turn.streaming[0];
        assert_fits(message, &split_chat(sent));

        let names = tool_names_in(&sent.body["tools"]);
        assert!(
            names.contains(wanted),
            "{message:?} should send {wanted}: {names:?}"
        );
        for stub in not_implemented_tools() {
            assert!(
                !names.contains(&stub) && !all_words(&system_text(&sent.body)).contains(&stub),
                "{message:?} offered {stub}, which does nothing in this build"
            );
        }
    }
}

// ---------------------------------------------------------------------
// Saying so when the model's context is still too small
// ---------------------------------------------------------------------

/// The server's own words, from the issue, are explained instead of being
/// shown as a generic provider error.
#[tokio::test]
async fn an_overflow_from_the_model_server_is_explained() {
    let body = json!({
        "error": {
            "code": 400,
            "message": "request (15157 tokens) exceeds the available context size (8192 tokens), \
                        try increasing it",
            "type": "exceed_context_size_error",
            "n_prompt_tokens": 15157,
            "n_ctx": 8192
        }
    })
    .to_string();
    let turn = run(
        "ollama",
        CANONICAL,
        vec![ok(chat_reply_json("general")), (400, body)],
    )
    .await;

    match turn.result {
        Err(Error::ContextTooSmall {
            needed: Some(15157),
            available: Some(8192),
            ..
        }) => {}
        other => panic!("expected the context error with both numbers, got {other:?}"),
    }
}
