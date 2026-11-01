//! A turn that fails before the model answers does not leave the message in
//! the conversation (#736). The window's Retry sends it again, and a copy
//! left behind would be joined onto the new one ("hi\n\nhi").

mod common;

use ai::LlmConfig;
use common::{classifier_json, ok};
use tools::ToolDispatcher;

#[tokio::test]
async fn a_failed_turn_and_its_retry_leave_the_message_once() {
    let server = common::serve_messages(vec![
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
    ])
    .await;

    let dir = tempfile::tempdir().expect("tempdir");
    let mut agent = common::agent_over(
        LlmConfig::new_anthropic("k").with_base_url(server.uri()),
        ToolDispatcher::new(),
        common::store_in(dir.path()),
        ai::PlanGate::new(),
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
