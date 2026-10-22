//! A setup problem the user fixes reaches the window as a card, not as prose
//! (#384).
//!
//! `transcribe` answers "the speech model is not downloaded" with an error that
//! starts with a marker the tool defines. The window is never sent a tool's
//! error text, only `tool-call-end` with a view, so the loop turns the marker
//! into `ToolView::SpeechSetup` and the window draws the card from that. What
//! the assistant then says about it, in whatever words, is text and nothing
//! more: the old card matched the assistant's prose.
//!
//! The turn is driven through the public `Agent` against a mocked provider,
//! with the real tool and a recogniser that is not a model.

mod common;

use std::sync::{Arc, Mutex};

use ai::{Agent, AgentEvent, LlmConfig, SpeechProblem, ToolView};
use common::{classifier_json, ok, sse_text, sse_tool_step, Entry, SeqResponder};
use serde_json::{json, Value};
use tools::tool::transcribe::{
    Audio, ModelRef, SpeechError, SpeechToText, Spoken, TranscribeTool, MARKER_MODEL_NOT_DOWNLOADED,
};
use tools::{ToolContext, ToolDispatcher, ToolResult};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer};

struct Refuses(SpeechError);

impl SpeechToText for Refuses {
    fn locate(&self) -> Result<ModelRef, SpeechError> {
        Err(self.0.clone())
    }
    fn transcribe(
        &self,
        _: &ModelRef,
        _: Audio<'_>,
        _: Option<&str>,
        _: &dyn Fn(usize, usize),
        _: &dyn Fn() -> bool,
    ) -> Result<Spoken, SpeechError> {
        unreachable!("locate refused")
    }
}

/// A one-track session in a folder, as `load` makes it.
fn session_with_a_track() -> (tempfile::TempDir, session::Store) {
    let dir = tempfile::tempdir().expect("tempdir");
    let wav = dir.path().join("talk.wav");
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 8_000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(&wav, spec).expect("wav");
    for n in 0..8_000 {
        w.write_sample((n % 100) as i16).unwrap();
    }
    w.finalize().unwrap();
    let mut store = session::Store::open(dir.path()).expect("store");
    let mut engine = audio_engine::Engine::new();
    let mut clipboard: Option<tools::Clipboard> = None;
    let mut ctx = ToolContext {
        store: &mut store,
        engine: &mut engine,
        user_message: "",
        clipboard: &mut clipboard,
        allowed_tools: None,
    };
    let loaded = ToolDispatcher::default_dispatcher()
        .invoke("load", json!({ "path": wav.to_string_lossy() }), &mut ctx)
        .expect("load dispatches");
    assert!(matches!(loaded, ToolResult::Ok(_)), "{loaded:?}");
    (dir, store)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_missing_speech_model_is_a_view_on_the_tool_call_and_not_on_what_the_assistant_says() {
    let server = MockServer::start().await;
    // The assistant, after the failure, writes the marker itself, as a model
    // quoting a tool result might. It must not make a second card.
    let assistant =
        format!("The tool said {MARKER_MODEL_NOT_DOWNLOADED}: please download the model.");
    let responses: Vec<Entry> = vec![
        ok(classifier_json("general")),
        ok(sse_tool_step(&[("t1", "transcribe", "{\"track\":0}")])),
        ok(sse_text(&assistant)),
    ];
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(SeqResponder::new(responses))
        .mount(&server)
        .await;

    let (_dir, store) = session_with_a_track();
    let mut dispatcher = ToolDispatcher::new();
    dispatcher.register(Box::new(TranscribeTool::with(Arc::new(Refuses(
        SpeechError::ModelNotDownloaded,
    )))));

    let cfg = LlmConfig::new_anthropic("test-key").with_base_url(server.uri());
    let mut agent = Agent::new(
        cfg,
        Arc::new(Mutex::new(dispatcher)),
        Arc::new(Mutex::new(store)),
        Arc::new(Mutex::new(audio_engine::Engine::new())),
        ai::PlanGate::new(),
        Arc::new(Mutex::new(None::<tools::Clipboard>)),
    );
    agent.set_plan_first(false);

    let mut ended: Vec<(String, bool, Option<ToolView>)> = Vec::new();
    let mut text = String::new();
    let result = agent
        .turn("transcribe track 0".to_string(), |event| match event {
            AgentEvent::ToolCallEnd { id, ok, view } => ended.push((id, ok, view)),
            AgentEvent::TextDelta(t) => text.push_str(&t),
            _ => {}
        })
        .await;
    assert!(result.is_ok(), "{result:?}");

    assert_eq!(
        ended,
        vec![(
            "t1".to_string(),
            false,
            Some(ToolView::SpeechSetup {
                problem: SpeechProblem::ModelNotDownloaded
            })
        )],
        "one tool call, failed, carrying the card"
    );
    assert!(
        text.contains(MARKER_MODEL_NOT_DOWNLOADED),
        "the assistant's own words are text: {text}"
    );

    // The model still reads the whole error, including what to tell the user.
    let requests = server.received_requests().await.expect("recording on");
    let last: Value = serde_json::from_slice(&requests.last().expect("a request").body).unwrap();
    let results = last["messages"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|m| m["content"].as_array().cloned().unwrap_or_default())
        .filter(|b| b["type"] == "tool_result")
        .collect::<Vec<_>>();
    assert_eq!(results.len(), 1);
    let content = results[0]["content"].as_str().unwrap_or_default();
    assert!(
        content.starts_with(MARKER_MODEL_NOT_DOWNLOADED),
        "{content}"
    );
    assert!(content.contains("Settings → Account"), "{content}");
    assert_eq!(results[0]["is_error"], true);

    // The view travels as the window reads it.
    let wire = serde_json::to_value(&ended[0].2).unwrap();
    assert_eq!(
        wire,
        json!({ "type": "speech_setup", "problem": "model_not_downloaded" })
    );
}
