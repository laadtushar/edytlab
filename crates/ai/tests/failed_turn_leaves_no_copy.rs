//! A turn that fails before the model answers does not leave the message in
//! the conversation (#736). The window's Retry sends it again, and a copy
//! left behind would be joined onto the new one ("hi\n\nhi").

mod common;

use std::sync::{Arc, Mutex};

use ai::{Agent, LlmConfig};
use common::{classifier_json, ok, sse_text, SeqResponder};
use tools::ToolDispatcher;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer};

#[tokio::test]
async fn a_failed_turn_and_its_retry_leave_the_message_once() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(SeqResponder::new(vec![
            ok(classifier_json("general")),
            (
                500,
                r#"{"type":"error","error":{"type":"api_error","message":"boom"}}"#.into(),
            ),
            ok(classifier_json("general")),
            (
                500,
                r#"{"type":"error","error":{"type":"api_error","message":"boom"}}"#.into(),
            ),
        ]))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().expect("tempdir");
    let mut agent = Agent::new(
        LlmConfig::new_anthropic("k").with_base_url(server.uri()),
        Arc::new(Mutex::new(ToolDispatcher::new())),
        Arc::new(Mutex::new(
            session::Store::open(dir.path()).expect("open store"),
        )),
        Arc::new(Mutex::new(audio_engine::Engine::new())),
        ai::PlanGate::new(),
        Arc::new(Mutex::new(None::<tools::Clipboard>)),
    );

    assert!(agent.turn("hi".to_string(), |_| {}).await.is_err());
    assert!(
        agent.conversation().is_empty(),
        "{:?}",
        agent.conversation()
    );
    assert!(agent.turn("hi".to_string(), |_| {}).await.is_err());
    assert!(
        agent.conversation().is_empty(),
        "{:?}",
        agent.conversation()
    );
}

#[tokio::test]
async fn retry_after_an_unavailable_model_sends_the_user_message_once() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(SeqResponder::new(vec![
            ok(classifier_json("general")),
            (404, r#"{"type":"error","error":{"type":"not_found_error","message":"model: unavailable"}}"#.into()),
            ok(classifier_json("general")),
            ok(sse_text("Ready again.")),
        ]))
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().expect("tempdir");
    let mut agent = Agent::new(
        LlmConfig::new_anthropic("k").with_base_url(server.uri()),
        Arc::new(Mutex::new(ToolDispatcher::new())),
        Arc::new(Mutex::new(
            session::Store::open(dir.path()).expect("open store"),
        )),
        Arc::new(Mutex::new(audio_engine::Engine::new())),
        ai::PlanGate::new(),
        Arc::new(Mutex::new(None::<tools::Clipboard>)),
    );
    let failed = agent.turn("hi".into(), |_| {}).await;
    assert!(
        matches!(failed, Err(ai::Error::ModelUnavailable { .. })),
        "{failed:?}"
    );
    assert!(agent.conversation().is_empty());
    assert!(
        !agent.withdraw_unanswered_text("hi"),
        "automatic cleanup already withdrew it"
    );
    let result = agent
        .turn("hi".into(), |_| {})
        .await
        .expect("retry succeeds");
    assert_eq!(result.text, "Ready again.");
    let requests = server.received_requests().await.expect("recording");
    assert_eq!(requests.len(), 4);
    let sent: serde_json::Value = serde_json::from_slice(&requests[3].body).unwrap();
    assert_eq!(
        sent["messages"],
        serde_json::json!([
            {"role": "user", "content": [{"type": "text", "text": "hi"}]}
        ])
    );
    assert_eq!(agent.conversation().len(), 2);
}
