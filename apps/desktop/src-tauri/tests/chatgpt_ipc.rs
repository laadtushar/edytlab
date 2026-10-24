//! The ChatGPT plan's commands over IPC (#474, PR 5), the way the window calls
//! them, with no network: a signed-in session is one whose registration is on
//! disk and whose refresh token is in a store in memory, which is what a
//! restart finds.
//!
//! What it checks that unit tests cannot: that the commands are registered and
//! take the arguments the bridge sends, the shape of every answer, that a
//! status change arrives on `chatgpt://status`, and that a turn that ends
//! because the session did comes back as a typed error and leaves the message
//! to be sent again (G9).
//!
//! Gated off Windows for the reason `commands_mock.rs` gives.

#![cfg(not(target_os = "windows"))]

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ai::chatgpt::spec::{HostId, ISSUER, PLAN_USAGE_SCOPE};
use ai::chatgpt::store::{
    ClientRecord, IdTokenReference, Registration, RegistrationFile, SecretStoreError,
};
use ai::chatgpt::{ChatGptSession, SessionConfig};
use ai::oauth::SecretString;
use edytlab_desktop_lib::chatgpt::Hooking;
use edytlab_desktop_lib::commands;
use edytlab_desktop_lib::events::CHATGPT_STATUS;
use edytlab_desktop_lib::state::AppState;
use serde_json::{json, Value};
use tauri::ipc::{CallbackFn, InvokeBody};
use tauri::test::{mock_builder, mock_context, noop_assets, MockRuntime, INVOKE_KEY};
use tauri::webview::InvokeRequest;
use tauri::{Emitter, Listener, WebviewWindow, WebviewWindowBuilder};

fn make_request(cmd: &str, body: Value) -> InvokeRequest {
    InvokeRequest {
        cmd: cmd.into(),
        callback: CallbackFn(0),
        error: CallbackFn(1),
        url: "tauri://localhost".parse().unwrap(),
        body: InvokeBody::Json(body),
        headers: Default::default(),
        invoke_key: INVOKE_KEY.to_string(),
    }
}

/// A refresh-token store in memory, shared with the test so it can take the
/// token away, the way another window signing out does.
#[derive(Default)]
struct MemoryStore {
    token: Mutex<Option<String>>,
}

impl ai::chatgpt::store::RefreshTokenStore for MemoryStore {
    fn load(&self) -> Result<Option<SecretString>, SecretStoreError> {
        Ok(self.token.lock().unwrap().clone().map(SecretString::new))
    }
    fn save(&self, token: &SecretString) -> Result<(), SecretStoreError> {
        *self.token.lock().unwrap() = Some(token.expose_secret().to_string());
        Ok(())
    }
    fn delete(&self) -> Result<(), SecretStoreError> {
        *self.token.lock().unwrap() = None;
        Ok(())
    }
}

struct Rig {
    app: tauri::App<MockRuntime>,
    webview: WebviewWindow<MockRuntime>,
    state: AppState,
    store: Arc<MemoryStore>,
    events: mpsc::Receiver<Value>,
    _config: tempfile::TempDir,
}

/// An app whose ChatGPT session is signed in (`true`) or never was.
fn new_rig(signed_in: bool) -> Rig {
    let config = tempfile::tempdir().expect("config dir");
    let store = Arc::new(MemoryStore::default());
    if signed_in {
        let mut registration = Registration::new(HostId::generate().unwrap());
        registration.active_client_id = Some("oaiapp_test".into());
        registration.clients.push(ClientRecord {
            client_id: "oaiapp_test".into(),
            issuer: ISSUER.into(),
            subject: Some("user-test".into()),
            email: Some("person@example.test".into()),
            scopes: vec!["openid".into(), PLAN_USAGE_SCOPE.into()],
            id_token_reference: IdTokenReference::default(),
            saved_at: 1,
            signed_out: false,
        });
        RegistrationFile::new(config.path().join("chatgpt"))
            .save(&registration)
            .unwrap();
        *store.token.lock().unwrap() = Some("RT-test".into());
    }
    let session =
        Arc::new(ChatGptSession::open(SessionConfig::new(config.path()), store.clone()).unwrap());

    let state = AppState::new();
    state.chatgpt.install(
        session,
        Hooking {
            persistence: Arc::new(|| None),
            refusal: Arc::new(|| None),
            // No request in a test.
            refresh_models: false,
            private_dir: None,
        },
    );
    tauri::async_runtime::block_on(state.chatgpt.restore());

    let app = mock_builder()
        .manage(state.clone())
        .invoke_handler(tauri::generate_handler![
            commands::open_project,
            commands::send_message,
            commands::has_api_key_for,
            commands::set_active_provider,
            commands::list_models_for,
            commands::test_api_key_for,
            commands::chatgpt_status,
            commands::chatgpt_sign_in,
            commands::chatgpt_cancel_sign_in,
            commands::chatgpt_sign_out,
            commands::chatgpt_forget_registration,
            commands::chatgpt_take_pending_message,
        ])
        .build(mock_context(noop_assets()))
        .expect("mock app");
    // As `lib.rs` sends it.
    let handle = app.handle().clone();
    state.chatgpt.set_notifier(move |status| {
        let _ = handle.emit(CHATGPT_STATUS, status);
    });
    let (tx, events) = mpsc::channel();
    app.listen(CHATGPT_STATUS, move |event| {
        let _ = tx.send(serde_json::from_str::<Value>(event.payload()).expect("a JSON payload"));
    });
    let webview = WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .expect("webview");
    Rig {
        app,
        webview,
        state,
        store,
        events,
        _config: config,
    }
}

impl Rig {
    fn call(&self, cmd: &str, body: Value) -> Result<Value, String> {
        tauri::test::get_ipc_response(&self.webview, make_request(cmd, body))
            .map(|r| r.deserialize::<Value>().expect("json"))
            .map_err(|e| e.as_str().expect("an error is a string").to_string())
    }

    /// The next status event, within a few seconds.
    fn next_event(&self) -> Value {
        self.events
            .recv_timeout(Duration::from_secs(10))
            .expect("a chatgpt://status event")
    }

    fn open_project(&self) -> tempfile::TempDir {
        let project = tempfile::tempdir().expect("project");
        self.call("open_project", json!({ "path": project.path() }))
            .expect("opened");
        project
    }
}

#[test]
fn the_status_has_the_shape_the_bridge_types_and_holds_no_token() {
    let rig = new_rig(true);
    let status = rig.call("chatgpt_status", json!({})).expect("a status");
    assert_eq!(
        status,
        json!({
            "state": "signed_in",
            "email": "person@example.test",
            "reason": null,
            "tokenStorage": { "kind": "keychain", "persistent": true, "reason": null },
            "pendingMessage": false
        })
    );
    assert!(!status.to_string().contains("RT-test"));

    let out = new_rig(false);
    let status = out.call("chatgpt_status", json!({})).expect("a status");
    assert_eq!(status["state"], "signed_out");
    assert_eq!(status["email"], Value::Null);
    assert_eq!(status["pendingMessage"], false);
}

#[test]
fn a_build_with_no_session_says_unavailable_and_refuses_to_sign_in() {
    let app = mock_builder()
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            commands::chatgpt_status,
            commands::chatgpt_sign_in,
            commands::chatgpt_sign_out,
            commands::chatgpt_forget_registration,
            commands::chatgpt_cancel_sign_in,
            commands::chatgpt_take_pending_message,
        ])
        .build(mock_context(noop_assets()))
        .expect("mock app");
    let webview = WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .expect("webview");
    let call = |cmd: &str, body: Value| {
        tauri::test::get_ipc_response(&webview, make_request(cmd, body))
            .map(|r| r.deserialize::<Value>().expect("json"))
            .map_err(|e| e.as_str().expect("an error is a string").to_string())
    };
    let status = call("chatgpt_status", json!({})).unwrap();
    assert_eq!(status["state"], "unavailable");
    assert_eq!(status["reason"]["code"], "no_config_dir");
    // The intent travels as a snake_case word, and nothing is opened.
    let err = call("chatgpt_sign_in", json!({ "intent": "sign_in" })).expect_err("no session");
    assert!(err.contains("not available"), "{err}");
    assert!(call("chatgpt_sign_in", json!({ "intent": "nonsense" })).is_err());
    assert!(call("chatgpt_sign_out", json!({})).is_err());
    assert!(call("chatgpt_forget_registration", json!({})).is_err());
    assert_eq!(call("chatgpt_cancel_sign_in", json!({})), Ok(Value::Null));
    assert_eq!(
        call("chatgpt_take_pending_message", json!({})),
        Ok(Value::Null)
    );
}

#[test]
fn the_plan_is_configured_and_selectable_only_while_signed_in() {
    let out = new_rig(false);
    assert_eq!(
        out.call("has_api_key_for", json!({ "provider": "chatgpt" })),
        Ok(json!(false))
    );
    let err = out
        .call("set_active_provider", json!({ "provider": "chatgpt" }))
        .expect_err("refused while signed out");
    assert!(err.contains("sign in with ChatGPT"), "{err}");

    let signed_in = new_rig(true);
    assert_eq!(
        signed_in.call("has_api_key_for", json!({ "provider": "chatgpt" })),
        Ok(json!(true))
    );
    // The model list and Test need the session too, and make no request
    // without one.
    let err = out
        .call("list_models_for", json!({ "provider": "chatgpt" }))
        .expect_err("not signed in");
    assert!(err.contains("not signed in"), "{err}");
    let err = out
        .call(
            "test_api_key_for",
            json!({ "provider": "chatgpt", "key": "", "baseUrl": null, "model": null }),
        )
        .expect_err("not signed in");
    assert!(err.contains("not signed in"), "{err}");
}

/// The case G9 is for. The session ends under a turn (another window signed
/// out; the same path a refused refresh takes). The turn fails with an error
/// the window can read by code, the status event says the session ended and
/// that a message is kept, and the message comes back to be sent again,
/// once, without the conversation holding it twice.
#[test]
fn a_session_that_ends_in_a_turn_is_a_typed_error_and_keeps_the_message() {
    let rig = new_rig(true);
    rig.state.set_active_provider("chatgpt".to_string());
    let _project = rig.open_project();
    assert!(
        rig.state.chatgpt.has_agent(),
        "the signed-in plan has an agent"
    );
    // Drain what the start published.
    while rig.events.try_recv().is_ok() {}

    // Another window signs out: the token is gone.
    *rig.store.token.lock().unwrap() = None;

    let err = rig
        .call(
            "send_message",
            json!({ "text": "cut the intro", "disabledTools": ["render_final"] }),
        )
        .expect_err("the session ended");
    assert!(
        err.starts_with("[plan-usage code=session_ended] "),
        "the window reads the code from the marker: {err}"
    );
    let lower = err.to_lowercase();
    assert!(
        !lower.contains("api key") && !lower.contains("authenticat"),
        "the chat must not send the person to Settings for a key: {err}"
    );

    // The status said so, on the event.
    let ended = rig.next_event();
    assert_eq!(ended["state"], "session_ended");
    assert_eq!(ended["reason"]["code"], "signed_out_elsewhere");
    assert_eq!(ended["pendingMessage"], true);

    // The agent's conversation does not hold the message: sending it again
    // is not joined onto its own copy.
    let slot = tauri::async_runtime::block_on(rig.state.agent.lock());
    assert_eq!(slot.as_ref().expect("agent").conversation().len(), 0);
    drop(slot);

    // The Continue card takes it, once.
    let kept = rig
        .call("chatgpt_take_pending_message", json!({}))
        .expect("answers");
    assert_eq!(
        kept,
        json!({
            "text": "cut the intro",
            "resumeText": "cut the intro",
            "disabledTools": ["render_final"],
            "progressed": false
        })
    );
    assert_eq!(rig.next_event()["pendingMessage"], false);
    assert_eq!(
        rig.call("chatgpt_take_pending_message", json!({})),
        Ok(Value::Null)
    );

    // A new message supersedes a kept one: the first is kept and left there,
    // then another is sent. The status says the kept message went when the
    // new turn began (the Continue card has nothing to send any more), and
    // the message kept at the end is the new one.
    let _ = rig.call(
        "send_message",
        json!({ "text": "first", "disabledTools": [] }),
    );
    assert_eq!(
        rig.call("chatgpt_status", json!({})).unwrap()["pendingMessage"],
        true,
        "precondition: the first message is kept"
    );
    while rig.events.try_recv().is_ok() {}
    let _ = rig.call(
        "send_message",
        json!({ "text": "second", "disabledTools": [] }),
    );
    let first_event = rig.next_event();
    assert_eq!(
        first_event["pendingMessage"], false,
        "the new turn began by letting the kept message go: {first_event}"
    );
    assert_eq!(rig.next_event()["pendingMessage"], true, "and kept its own");
    let kept = rig.call("chatgpt_take_pending_message", json!({})).unwrap();
    assert_eq!(kept["text"], "second", "not the first message");
    drop(rig.app);
}

#[test]
fn sign_out_and_start_over_finish_and_say_what_the_person_may_still_have_to_do() {
    let rig = new_rig(true);
    rig.state.set_active_provider("chatgpt".to_string());
    let _project = rig.open_project();
    assert!(rig.state.chatgpt.has_agent());
    // The session ends under a message (another window signed out), so there
    // is no token left to revoke and the test makes no request to OpenAI.
    *rig.store.token.lock().unwrap() = None;
    let _ = rig.call("send_message", json!({ "text": "hi", "disabledTools": [] }));
    assert_eq!(
        rig.call("chatgpt_status", json!({})).unwrap()["pendingMessage"],
        true
    );
    while rig.events.try_recv().is_ok() {}

    let out = rig.call("chatgpt_sign_out", json!({})).expect("signed out");
    assert_eq!(out["status"]["state"], "signed_out");
    assert_eq!(
        out["status"]["pendingMessage"], false,
        "a sign-out is a choice"
    );
    assert_eq!(out["tokenRemoved"], true);
    assert_eq!(out["revocation"], "nothing_to_revoke");
    assert!(out.get("registrationForgotten").is_none(), "{out}");
    // The agent that ran on the plan is dropped: nothing to run it on.
    assert!(!rig.state.chatgpt.has_agent());
    assert!(tauri::async_runtime::block_on(rig.state.agent.lock()).is_none());
    assert_eq!(rig.next_event()["state"], "signed_out");
    let forgotten = rig
        .call("chatgpt_forget_registration", json!({}))
        .expect("started over");
    assert_eq!(forgotten["registrationForgotten"], true);
    assert!(forgotten["notice"]
        .as_str()
        .unwrap()
        .contains("may still be listed"));
}
