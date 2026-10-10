//! A model the provider no longer has fails the turn with a message that
//! says so (#468).
//!
//! Providers retire models, and a person who chose one keeps it chosen:
//! nothing rewrites the saved model. The request then fails, and what the
//! chat showed was "the model provider returned an error (404)" followed by
//! the raw JSON, which says nothing a person can act on. A turn on a retired
//! model now fails with `Error::ModelUnavailable`, which names the model and
//! the provider's default and no provider (#405).
//!
//! These drive whole turns through the public `Agent` API against a mocked
//! provider. The Anthropic body is the real answer for a retired model,
//! captured from the API; the others are illustrative (see
//! `provider::tests::model_unavailable_is_recognised_per_wire_format`).

mod common;

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use ai::{Agent, Error, LlmConfig};
use common::{chat_reply_json, classifier_json, ok, Entry, SeqResponder};
use tokio::sync::Notify;
use tools::ToolDispatcher;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer};

const RETIRED_ANTHROPIC_BODY: &str = r#"{"type":"error","error":{"type":"not_found_error","message":"model: claude-opus-4-1-20250805"},"request_id":"req_011Cftywc6xsQRBrAxHK2RgM"}"#;

/// Run one turn of `cfg` against a server that answers `/endpoint` with
/// `responses` in order, and give back the turn's result and how many
/// requests it made.
async fn turn(
    cfg: LlmConfig,
    endpoint: &str,
    responses: Vec<Entry>,
) -> (ai::Result<ai::TurnResult>, usize) {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(endpoint))
        .respond_with(SeqResponder::new(responses))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().expect("tempdir");
    let mut agent = Agent::new(
        cfg.with_base_url(server.uri()),
        Arc::new(Mutex::new(ToolDispatcher::new())),
        Arc::new(Mutex::new(
            session::Store::open(dir.path()).expect("open store"),
        )),
        Arc::new(Mutex::new(audio_engine::Engine::new())),
        Arc::new(Notify::new()),
        Arc::new(Mutex::new(None)),
        Arc::new(AtomicBool::new(false)),
        Arc::new(Mutex::new(None::<tools::Clipboard>)),
    );
    let result = agent.turn("make it louder".to_string(), |_| {}).await;
    let requests = server.received_requests().await.expect("recording").len();
    (result, requests)
}

#[tokio::test]
async fn a_retired_model_fails_the_turn_with_model_unavailable() {
    let cfg = LlmConfig::new_anthropic("k").with_model("claude-opus-4-1-20250805");
    let (result, requests) = turn(
        cfg,
        "/v1/messages",
        vec![
            // The classifier runs on its own model and is answered.
            ok(classifier_json("general")),
            (404, RETIRED_ANTHROPIC_BODY.to_string()),
        ],
    )
    .await;

    match result {
        Err(Error::ModelUnavailable {
            model,
            default_model,
            status,
        }) => {
            assert_eq!(model, "claude-opus-4-1-20250805");
            assert_eq!(default_model, ai::DEFAULT_MODEL);
            assert_eq!(status, 404);
        }
        other => panic!("expected ModelUnavailable, got {other:?}"),
    }
    assert_eq!(requests, 2, "the classifier, then the one refused step");
}

#[tokio::test]
async fn a_model_the_openai_api_does_not_have_fails_the_same_way() {
    let body = r#"{"error":{"message":"The model `gpt-4o-2024-05-13` does not exist or you do not have access to it.","type":"invalid_request_error","param":null,"code":"model_not_found"}}"#;
    let cfg = LlmConfig::new_openai("k").with_model("gpt-4o-2024-05-13");
    let (result, _) = turn(
        cfg,
        "/v1/chat/completions",
        vec![ok(chat_reply_json("general")), (404, body.to_string())],
    )
    .await;

    match result {
        Err(Error::ModelUnavailable {
            model,
            default_model,
            status,
        }) => {
            assert_eq!(model, "gpt-4o-2024-05-13");
            assert_eq!(default_model, "gpt-6-luna");
            assert_eq!(status, 404);
        }
        other => panic!("expected ModelUnavailable, got {other:?}"),
    }
}

/// Anything that is not a missing model stays the plain API error, with the
/// server's own words: a rate limit, a key problem, a 404 from a proxy with
/// the wrong base path, and a 400 that merely mentions the model.
#[tokio::test]
async fn other_failures_stay_api_errors() {
    for (status, body) in [
        (
            429,
            r#"{"type":"error","error":{"type":"rate_limit_error","message":"slow down"}}"#,
        ),
        (404, "<html>404 Not Found</html>"),
        (
            400,
            r#"{"type":"error","error":{"type":"invalid_request_error","message":"'claude-opus-5-5' does not support this request"}}"#,
        ),
    ] {
        let cfg = LlmConfig::new_anthropic("k").with_model("claude-opus-5-5");
        let (result, _) = turn(
            cfg,
            "/v1/messages",
            vec![ok(classifier_json("general")), (status, body.to_string())],
        )
        .await;
        match result {
            Err(Error::Api {
                status: got,
                message,
            }) => {
                assert_eq!(got, status);
                assert_eq!(message, body);
            }
            other => panic!("{status}: expected Api, got {other:?}"),
        }
    }
}

/// Ollama's own "not found, try pulling it first" is the useful message for
/// a missing model; "it may have been retired" would be wrong for a model the
/// person simply has not pulled.
#[tokio::test]
async fn a_missing_local_model_keeps_ollamas_own_words() {
    let body = r#"{"error":{"message":"model \"llama3.2\" not found, try pulling it first","type":"api_error","param":null,"code":null}}"#;
    let cfg = LlmConfig::new(ai::provider::provider_from_id(ai::provider::OLLAMA_ID), "");
    let (result, _) = turn(
        cfg,
        "/chat/completions",
        vec![ok(chat_reply_json("general")), (404, body.to_string())],
    )
    .await;
    match result {
        Err(Error::Api { status, message }) => {
            assert_eq!(status, 404);
            assert!(message.contains("try pulling it first"), "{message}");
        }
        other => panic!("expected Api, got {other:?}"),
    }
}
