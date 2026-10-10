//! Live check of the thinking-block prefix rule (#468), against the real
//! Anthropic API.
//!
//! Anthropic's 5.x models sign each thinking block with the request prefix
//! that produced it (`system`, `tools`, every earlier message) and answer a
//! 400 when a block comes back under a different one. That check is on by
//! default only for accounts created on or after 2026-08-31, so on an older
//! account a regression here would pass unnoticed. This test forces the
//! strict check on any account, with the documented opt-in: the
//! `thinking-binding-controls-2026-08-01` beta header and
//! `thinking.block_binding.prefix_mismatch_behavior: "error"`. That is
//! test-only; the application sends neither.
//!
//! It drives edytlab's own agent loop and Anthropic provider (the provider
//! below only adds those two things on top of [`ai::AnthropicProvider`]) over
//! two turns with a tool call in the first, and the session context, and so
//! `system`, different in the second:
//!
//! * turn 1's second request replays the thinking from its first one under
//!   an unchanged prefix, and must be accepted: this is what keeping a
//!   turn's own thinking relies on;
//! * turn 2 starts under a changed `system`, and must be accepted too,
//!   which it is only if the thinking from turn 1 is not sent. Without
//!   that the API answers 400 `Invalid signature in thinking block ... The
//!   block is bound to a different conversation`.
//!
//! `#[ignore]`d, and a no-op without a key. To run it (the key is read
//! from the environment and goes nowhere else):
//!
//! ```bash
//! ANTHROPIC_E2E_KEY=sk-ant-... cargo test -p ai --test live_thinking_binding -- --ignored --nocapture
//! ```
//!
//! `ANTHROPIC_E2E_MODEL` overrides the model (default `claude-sonnet-5-5`),
//! and `ANTHROPIC_E2E_BASE_URL` the endpoint (default Anthropic's).

use std::sync::{Arc, Mutex};

use ai::anthropic::{ContentBlock, MessagesRequest};
use ai::{Agent, AnthropicProvider, Effort, Error, LlmConfig, LlmProvider, SessionContext};
use serde_json::{json, Value};
use tools::schema::{anthropic_tool, object_schema};
use tools::{Tool, ToolContext, ToolDispatcher, ToolResult};

const STRICT_BETA: &str = "thinking-binding-controls-2026-08-01";

/// Anthropic's provider with the strict prefix check switched on, and a
/// record of every streaming request body it serialised.
#[derive(Debug)]
struct StrictBinding {
    inner: AnthropicProvider,
    streamed: Arc<Mutex<Vec<Value>>>,
}

impl LlmProvider for StrictBinding {
    fn id(&self) -> &'static str {
        self.inner.id()
    }
    fn base_url(&self) -> &str {
        self.inner.base_url()
    }
    fn default_model(&self) -> &str {
        self.inner.default_model()
    }
    fn classifier_model(&self) -> &str {
        self.inner.classifier_model()
    }
    fn translate_model(&self, model: &str) -> String {
        self.inner.translate_model(model)
    }
    fn supports_effort(&self) -> bool {
        self.inner.supports_effort()
    }
    fn apply_auth(&self, req: reqwest::RequestBuilder, api_key: &str) -> reqwest::RequestBuilder {
        self.inner
            .apply_auth(req, api_key)
            .header("anthropic-beta", STRICT_BETA)
    }
    fn serialize_request(&self, req: &MessagesRequest<'_>) -> Value {
        let mut body = self.inner.serialize_request(req);
        body["thinking"] = json!({
            "type": "adaptive",
            "block_binding": { "prefix_mismatch_behavior": "error" }
        });
        self.streamed.lock().unwrap().push(body.clone());
        body
    }
}

/// A read-only tool, so turn 1 has a tool round trip to replay across.
struct CountTracks;

impl Tool for CountTracks {
    fn name(&self) -> &'static str {
        "count_tracks"
    }
    fn schema(&self) -> Value {
        anthropic_tool(
            "count_tracks",
            "Returns how many tracks the session has.",
            object_schema(&[]),
        )
    }
    fn mutates(&self) -> bool {
        false
    }
    fn invoke(&self, _args: Value, _ctx: &mut ToolContext) -> tools::Result<ToolResult> {
        Ok(ToolResult::Ok(json!({ "tracks": 7 })))
    }
}

fn thinking_blocks(messages: &Value) -> usize {
    messages
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|m| m["content"].as_array().into_iter().flatten())
        .filter(|b| matches!(b["type"].as_str(), Some("thinking" | "redacted_thinking")))
        .count()
}

fn status_of<T>(result: &Result<T, Error>) -> String {
    match result {
        Ok(_) => "200".to_string(),
        Err(Error::Api { status, message }) => format!("{status} {message}"),
        Err(other) => format!("failed without an HTTP status: {other}"),
    }
}

#[tokio::test]
#[ignore = "live API; requires ANTHROPIC_E2E_KEY"]
async fn a_second_turn_under_a_changed_system_prompt_passes_the_strict_prefix_check() {
    let Ok(key) = std::env::var("ANTHROPIC_E2E_KEY") else {
        eprintln!("ANTHROPIC_E2E_KEY not set, skipping");
        return;
    };
    let model = std::env::var("ANTHROPIC_E2E_MODEL").unwrap_or_else(|_| "claude-sonnet-5-5".into());

    let streamed = Arc::new(Mutex::new(Vec::new()));
    let provider = StrictBinding {
        inner: AnthropicProvider,
        streamed: Arc::clone(&streamed),
    };
    let mut cfg = LlmConfig::new(Arc::new(provider), key)
        .with_model(model.clone())
        .with_effort(Effort::High);
    // For a gateway, or a forwarder where the client cannot trust the
    // network's certificate authority.
    if let Ok(base) = std::env::var("ANTHROPIC_E2E_BASE_URL") {
        cfg = cfg.with_base_url(base.trim_end_matches('/'));
    }

    let dir = tempfile::tempdir().expect("tempdir");
    let mut dispatcher = ToolDispatcher::new();
    dispatcher.register(Box::new(CountTracks));
    let mut agent = Agent::new(
        cfg,
        Arc::new(Mutex::new(dispatcher)),
        Arc::new(Mutex::new(
            session::Store::open(dir.path()).expect("open store"),
        )),
        Arc::new(Mutex::new(audio_engine::Engine::new())),
        Arc::new(tokio::sync::Notify::new()),
        Arc::new(Mutex::new(None)),
        Arc::new(std::sync::atomic::AtomicBool::new(false)),
        Arc::new(Mutex::new(None::<tools::Clipboard>)),
    );

    let at = |head: &str| SessionContext {
        head: Some(head.to_string()),
        ..SessionContext::default()
    };

    // Turn 1: a tool call, then the answer. Its second request replays
    // the first one's thinking under an unchanged prefix. The model
    // thinks when it is given something to think about before the call
    // (adaptive thinking skipped a plainer prompt at this effort).
    let turn1 = agent
        .turn_with_context(
            "Before calling any tool, reason step by step about what formula counts the ways \
             to choose k non-adjacent items from n in a row. Then call count_tracks to get n, \
             and compute the answer for k=3."
                .to_string(),
            Some(&at("aaaaaaaa")),
            |_| {},
        )
        .await;
    let turn1_requests = streamed.lock().unwrap().len();
    eprintln!(
        "[{model}] turn 1: {} ({turn1_requests} streaming requests)",
        status_of(&turn1)
    );
    turn1.expect(
        "turn 1 must pass the strict check: its requests share one prefix, so replaying \
         a step's own thinking is valid",
    );

    let produced = agent
        .conversation()
        .iter()
        .flat_map(|m| m.content.iter())
        .filter(|b| {
            matches!(
                b,
                ContentBlock::Thinking { .. } | ContentBlock::RedactedThinking { .. }
            )
        })
        .count();
    assert!(
        produced > 0,
        "inconclusive: the model wrote no thinking block in turn 1, so there is nothing \
         for turn 2 to replay or not"
    );

    // Turn 2: the head moved, so `system` is not what it was.
    let turn2 = agent
        .turn_with_context(
            "Use the count_tracks tool again and tell me that number times 17.".to_string(),
            Some(&at("bbbbbbbb")),
            |_| {},
        )
        .await;
    let sent = streamed.lock().unwrap().clone();
    eprintln!(
        "[{model}] turn 2: {} ({} streaming requests in turn 2)",
        status_of(&turn2),
        sent.len() - turn1_requests
    );

    let first_of_turn2 = &sent[turn1_requests];
    assert_ne!(
        sent[0]["system"], first_of_turn2["system"],
        "the premise is that turn 2 sends a different system prompt"
    );
    eprintln!(
        "[{model}] thinking blocks in turn 2's first request: {} (turn 1 left {produced} in the history)",
        thinking_blocks(&first_of_turn2["messages"])
    );

    turn2.expect(
        "turn 2 must pass the strict check: its system prompt changed, so no thinking \
         from turn 1 may be sent with it",
    );
    assert_eq!(thinking_blocks(&first_of_turn2["messages"]), 0);
}
