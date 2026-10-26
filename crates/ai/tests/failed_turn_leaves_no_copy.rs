//! A turn that fails before the model answers does not leave the message in
//! the conversation (#736). The window's Retry sends it again, and a copy
//! left behind would be joined onto the new one ("hi\n\nhi").

mod common;

use std::sync::{Arc, Mutex};

use ai::{Agent, LlmConfig};
use common::{classifier_json, ok, SeqResponder};
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
