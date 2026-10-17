//! Signing in with ChatGPT (`ai::chatgpt`), against a fake OpenAI issuer:
//! registration and the callback rules, the scope check, the ID token's
//! audience, what is stored where, and that no secret leaks.
//!
//! See `chatgpt_support` for the fake server and the browser it drives.

mod chatgpt_support;
mod oauth_support;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ai::chatgpt::session::{BearerError, SessionState, SignInError, TokenStorage};
use ai::chatgpt::spec::{CallbackError, HostId};
use ai::chatgpt::store::{RegistrationFile, SecretStoreError, REGISTRATION_FILE};
use ai::chatgpt::{ChatGptSession, ChatGptStatus, SignInIntent};
use ai::oauth::loopback::LoopbackError;
use ai::oauth::oidc::OidcError;
use chatgpt_support::{
    Browser, CallbackClient, Fake, MemoryStore, CODE, EMAIL, ISSUED_CLIENT, SUBJECT,
};
use serde_json::{json, Value};

const SCOPE: &str = "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct";

fn registration_json(dir: &std::path::Path) -> Value {
    let text = std::fs::read_to_string(dir.join("chatgpt").join(REGISTRATION_FILE)).unwrap();
    serde_json::from_str(&text).unwrap()
}

#[tokio::test]
async fn a_first_sign_in_registers_and_saves_the_issued_client_before_the_exchange() {
    let fake = Fake::start().await;
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::new();
    let session = fake.session(dir.path(), store.clone());
    assert_eq!(session.status().state, SessionState::SignedOut);

    // What registration.json holds when the code is exchanged.
    let at_exchange: Arc<Mutex<Option<Value>>> = Arc::default();
    {
        let seen = at_exchange.clone();
        let path = dir.path().join("chatgpt").join(REGISTRATION_FILE);
        *fake.on_exchange.lock().unwrap() = Some(Box::new(move || {
            let text = std::fs::read_to_string(&path).unwrap();
            *seen.lock().unwrap() = Some(serde_json::from_str(&text).unwrap());
        }));
    }

    let status = fake
        .sign_in(&session, Browser::default(), SignInIntent::SignIn)
        .await
        .expect("signs in");
    assert_eq!(status.state, SessionState::SignedIn);
    assert_eq!(status.email.as_deref(), Some(EMAIL));
    assert_eq!(status.token_storage, Some(TokenStorage::Stored));

    // The authorization request: a first registration.
    let auth = fake.authorizations.lock().unwrap()[0].clone();
    assert_eq!(auth["client_id"], "dynamic_agent_client");
    assert_eq!(auth["agent_name_hint"], "edytlab");
    let host_id = auth["ext_agent_host_id"].clone();
    HostId::try_from(host_id.clone()).expect("a urn:uuid host ID");
    assert_eq!(auth["scope"], SCOPE);
    assert_eq!(auth["resource"], "https://api.openai.com/v1");
    assert_eq!(auth["response_type"], "code");
    assert_eq!(auth["code_challenge_method"], "S256");
    let redirect = url::Url::parse(&auth["redirect_uri"]).unwrap();
    assert_eq!(redirect.scheme(), "http");
    assert_eq!(redirect.host_str(), Some("127.0.0.1"));
    assert_eq!(redirect.path(), "/auth/callback");
    for absent in ["login_hint", "prompt", "id_token_hint"] {
        assert!(!auth.contains_key(absent), "{absent}: {auth:?}");
    }

    // The issued client id was on disk before the exchange, with the host
    // ID, and the account was not yet.
    let before = at_exchange
        .lock()
        .unwrap()
        .clone()
        .expect("the exchange ran");
    assert_eq!(before["host_id"], host_id.as_str());
    assert_eq!(before["clients"][0]["client_id"], ISSUED_CLIENT);
    assert_eq!(before["clients"][0]["subject"], Value::Null);
    assert_eq!(before["active_client_id"], Value::Null);

    // The exchange: the issued client, the same redirect URI and resource.
    let exchanges = fake.exchange_requests();
    assert_eq!(exchanges.len(), 1);
    let ex = &exchanges[0];
    assert_eq!(ex["client_id"], ISSUED_CLIENT);
    assert_eq!(ex["code"], CODE);
    assert_eq!(ex["redirect_uri"], auth["redirect_uri"]);
    assert_eq!(ex["resource"], "https://api.openai.com/v1");
    assert!(ex["code_verifier"].len() >= 43);
    assert_eq!(ex.len(), 6, "nothing else: {ex:?}");

    // After: the verified account is active; the refresh token is in the
    // store, the access token in memory.
    let after = registration_json(dir.path());
    assert_eq!(after["version"], 1);
    assert_eq!(after["host_id"], host_id.as_str());
    assert_eq!(after["active_client_id"], ISSUED_CLIENT);
    let client = &after["clients"][0];
    assert_eq!(after["clients"].as_array().unwrap().len(), 1);
    assert_eq!(client["issuer"], fake.issuer());
    assert_eq!(client["subject"], SUBJECT);
    assert_eq!(client["email"], EMAIL);
    assert_eq!(client["signed_out"], false);
    assert!(client["scopes"]
        .as_array()
        .unwrap()
        .contains(&json!("chatgpt.tokens.use.direct")));
    assert_eq!(store.get().as_deref(), Some("RT-1"));
    assert_eq!(session.bearer().await.unwrap().expose_secret(), "AT-1");
    assert!(fake.refresh_requests().is_empty());
}

#[tokio::test]
async fn a_returning_sign_in_reuses_the_client_and_the_host_id() {
    let fake = Fake::start().await;
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::new();
    let session = fake.signed_in(dir.path(), store.clone()).await;
    session.sign_out().await;

    // In this process, and after a restart.
    let restarted = fake.session(dir.path(), store.clone());
    for s in [&session, &restarted] {
        let status = fake
            .sign_in(s, Browser::default(), SignInIntent::SignIn)
            .await
            .expect("signs in again");
        assert_eq!(status.state, SessionState::SignedIn);
    }
    let auths = fake.authorizations.lock().unwrap().clone();
    assert_eq!(auths.len(), 3);
    for again in &auths[1..] {
        assert_eq!(again["client_id"], ISSUED_CLIENT);
        assert_eq!(again["ext_agent_host_id"], auths[0]["ext_agent_host_id"]);
        assert_eq!(again["login_hint"], EMAIL);
        assert_eq!(again["scope"], SCOPE);
        assert!(!again.contains_key("agent_name_hint"), "{again:?}");
        assert!(!again.contains_key("prompt"), "{again:?}");
    }
    // Each exchange used its own attempt's redirect URI.
    let exchanges = fake.exchange_requests();
    for (ex, auth) in exchanges.iter().zip(&auths) {
        assert_eq!(ex["client_id"], ISSUED_CLIENT);
        assert_eq!(ex["redirect_uri"], auth["redirect_uri"]);
    }
    // Still one client: signing in again registered nothing new.
    let reg = registration_json(dir.path());
    assert_eq!(reg["clients"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn the_callback_port_may_change_between_attempts() {
    // The preferred port is taken (by Codex CLI, or by the last attempt's
    // connections in TIME_WAIT): the listener takes another, and the
    // request and the exchange both name that one.
    let fake = Fake::start().await;
    let dir = tempfile::tempdir().unwrap();
    let squatter = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let taken = squatter.local_addr().unwrap().port();
    let config = fake.config(dir.path()).with_preferred_port(Some(taken));
    let session = ChatGptSession::open(config, MemoryStore::new()).unwrap();
    fake.sign_in(&session, Browser::default(), SignInIntent::SignIn)
        .await
        .unwrap();
    drop(squatter);
    let auth = fake.authorizations.lock().unwrap()[0].clone();
    let port = url::Url::parse(&auth["redirect_uri"])
        .unwrap()
        .port()
        .unwrap();
    assert_ne!(port, taken);
    assert_eq!(
        fake.exchange_requests()[0]["redirect_uri"],
        auth["redirect_uri"]
    );

    // And a returning sign-in on whatever port it gets is accepted.
    session.sign_out().await;
    fake.sign_in(&session, Browser::default(), SignInIntent::SignIn)
        .await
        .unwrap();
    let auth = fake.authorizations.lock().unwrap()[1].clone();
    assert_eq!(
        fake.exchange_requests()[1]["redirect_uri"],
        auth["redirect_uri"]
    );
}

#[tokio::test]
async fn a_registration_without_an_issued_client_id_is_incomplete() {
    for (name, client_id) in [
        ("none", CallbackClient::Omit),
        ("empty", CallbackClient::Send(String::new())),
        (
            "the entry point",
            CallbackClient::Send("dynamic_agent_client".into()),
        ),
    ] {
        let fake = Fake::start().await;
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::new();
        let session = fake.session(dir.path(), store.clone());
        let err = fake
            .sign_in(
                &session,
                Browser {
                    client_id,
                    ..Browser::default()
                },
                SignInIntent::SignIn,
            )
            .await
            .unwrap_err();
        assert!(
            matches!(
                err,
                SignInError::Callback(CallbackError::RegistrationIncomplete)
            ),
            "{name}: {err:?}"
        );
        assert!(
            fake.exchange_requests().is_empty(),
            "{name}: no code exchange"
        );
        let reg = registration_json(dir.path());
        assert_eq!(
            reg["clients"],
            json!([]),
            "{name}: nothing saved as a client"
        );
        assert!(!reg.to_string().contains("dynamic_agent_client"), "{name}");
        assert_eq!(session.status().state, SessionState::SignedOut);
        assert_eq!(store.get(), None);
    }
}

#[tokio::test]
async fn a_returning_sign_in_that_names_another_client_is_refused() {
    let fake = Fake::start().await;
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::new();
    let session = fake.signed_in(dir.path(), store.clone()).await;

    let err = fake
        .sign_in(
            &session,
            Browser {
                client_id: CallbackClient::Send("oaiapp_someone_else".into()),
                ..Browser::default()
            },
            SignInIntent::SignIn,
        )
        .await
        .unwrap_err();
    assert!(
        matches!(err, SignInError::Callback(CallbackError::ClientIdChanged)),
        "{err:?}"
    );
    assert_eq!(fake.exchange_requests().len(), 1, "no second exchange");
    // The session in use is untouched.
    assert_eq!(session.status().state, SessionState::SignedIn);
    assert_eq!(session.bearer().await.unwrap().expose_secret(), "AT-1");
    assert_eq!(store.get().as_deref(), Some("RT-1"));
    assert_eq!(
        registration_json(dir.path())["active_client_id"],
        ISSUED_CLIENT
    );
}

#[tokio::test]
async fn access_denied_stops_the_attempt_without_an_exchange() {
    let fake = Fake::start().await;
    let dir = tempfile::tempdir().unwrap();
    let session = fake.session(dir.path(), MemoryStore::new());
    let err = fake
        .sign_in(
            &session,
            Browser {
                error: Some("access_denied".into()),
                ..Browser::default()
            },
            SignInIntent::SignIn,
        )
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            SignInError::Loopback(LoopbackError::AccessDenied { .. })
        ),
        "{err:?}"
    );
    assert!(fake.exchange_requests().is_empty());
    assert_eq!(session.status().state, SessionState::SignedOut);
}

#[tokio::test]
async fn the_callback_issuer_is_checked_when_discovery_promises_one() {
    // RFC 9207: an issuer that advertises
    // `authorization_response_iss_parameter_supported` must send `iss`.
    let fake = Fake::start().await;
    fake.knobs().iss_parameter_supported = true;
    let dir = tempfile::tempdir().unwrap();
    let session = fake.session(dir.path(), MemoryStore::new());
    for iss in [None, Some("https://auth.openai.com".to_string())] {
        let err = fake
            .sign_in(
                &session,
                Browser {
                    iss: iss.clone(),
                    ..Browser::default()
                },
                SignInIntent::SignIn,
            )
            .await
            .unwrap_err();
        assert!(
            matches!(err, SignInError::Loopback(LoopbackError::IssuerMismatch)),
            "{iss:?}: {err:?}"
        );
    }
    assert!(fake.exchange_requests().is_empty());
    let status = fake
        .sign_in(
            &session,
            Browser {
                iss: Some(fake.issuer()),
                ..Browser::default()
            },
            SignInIntent::SignIn,
        )
        .await
        .unwrap();
    assert_eq!(status.state, SessionState::SignedIn);
}

#[tokio::test]
async fn without_the_plan_scope_the_sign_in_is_kept_with_plan_usage_disabled() {
    for granted in [
        Some("openid profile email offline_access resource.invoke"),
        None,
    ] {
        let fake = Fake::start().await;
        fake.knobs().scope = granted.map(str::to_string);
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::new();
        let session = fake.signed_in(dir.path(), store.clone()).await;
        assert_eq!(
            session.status().state,
            SessionState::PlanDisabled,
            "{granted:?}"
        );
        assert_eq!(session.status().email.as_deref(), Some(EMAIL));
        assert_eq!(
            session.bearer().await.unwrap_err(),
            BearerError::PlanDisabled
        );

        // Turning it on: the saved client, the full scopes, and consent.
        fake.knobs().scope = Some(chatgpt_support::FULL_SCOPE.into());
        let status = fake
            .sign_in(&session, Browser::default(), SignInIntent::EnablePlanUsage)
            .await
            .unwrap();
        assert_eq!(status.state, SessionState::SignedIn);
        let auth = fake.authorizations.lock().unwrap()[1].clone();
        assert_eq!(auth["client_id"], ISSUED_CLIENT);
        assert_eq!(auth["prompt"], "consent");
        assert_eq!(auth["scope"], SCOPE);
        assert_eq!(session.bearer().await.unwrap().expose_secret(), "AT-2");
    }
}

#[tokio::test]
async fn an_id_token_for_another_audience_is_refused() {
    // `aud` must be the issued client id: not the registration entry point,
    // not another client.
    for audience in ["dynamic_agent_client", "oaiapp_other"] {
        let fake = Fake::start().await;
        fake.knobs().audience = Some(audience.into());
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::new();
        let session = fake.session(dir.path(), store.clone());
        let err = fake
            .sign_in(&session, Browser::default(), SignInIntent::SignIn)
            .await
            .unwrap_err();
        assert!(
            matches!(err, SignInError::IdToken(OidcError::WrongAudience)),
            "{audience}: {err:?}"
        );
        assert_eq!(session.status().state, SessionState::SignedOut);
        assert_eq!(store.get(), None, "{audience}: no token stored");
        let reg = registration_json(dir.path());
        assert_eq!(reg["active_client_id"], Value::Null);
        assert_eq!(reg["clients"][0]["subject"], Value::Null);
        // The tokens it refused are revoked, not left live at OpenAI.
        let revoked = fake.revocations.lock().unwrap().clone();
        assert_eq!(revoked.len(), 1, "{audience}");
        assert_eq!(revoked[0]["token"], "RT-1");
        assert_eq!(revoked[0]["client_id"], ISSUED_CLIENT);
    }
}

#[tokio::test]
async fn a_returning_sign_in_as_another_account_is_refused() {
    let fake = Fake::start().await;
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::new();
    let session = fake.signed_in(dir.path(), store.clone()).await;
    fake.knobs().subject = "someone-else".into();
    let err = fake
        .sign_in(&session, Browser::default(), SignInIntent::SignIn)
        .await
        .unwrap_err();
    assert!(matches!(err, SignInError::AccountChanged), "{err:?}");
    assert_eq!(session.status().state, SessionState::SignedIn);
    assert_eq!(session.bearer().await.unwrap().expose_secret(), "AT-1");
    assert_eq!(
        store.get().as_deref(),
        Some("RT-1"),
        "the credentials are not replaced"
    );
    assert_eq!(
        registration_json(dir.path())["clients"][0]["subject"],
        SUBJECT
    );
    // The other account's tokens are revoked, not left live at OpenAI.
    let revoked = fake.revocations.lock().unwrap().clone();
    assert_eq!(revoked.len(), 1);
    assert_eq!(revoked[0]["token"], "RT-2");
}

#[tokio::test]
async fn an_invalid_grant_in_any_case_on_the_exchange_means_a_fresh_start() {
    // Refresh codes are matched without regard to case; so is this one.
    let fake = Fake::start().await;
    fake.knobs().exchange_error = Some((400, json!({ "error": "INVALID_GRANT" })));
    let dir = tempfile::tempdir().unwrap();
    let session = fake.session(dir.path(), MemoryStore::new());
    let err = fake
        .sign_in(&session, Browser::default(), SignInIntent::SignIn)
        .await
        .unwrap_err();
    assert!(matches!(err, SignInError::CodeRejected), "{err:?}");
}

#[tokio::test]
async fn an_invalid_grant_on_the_exchange_means_a_fresh_authorization_with_the_same_client() {
    let fake = Fake::start().await;
    fake.knobs().exchange_error = Some((400, json!({ "error": "invalid_grant" })));
    let dir = tempfile::tempdir().unwrap();
    let session = fake.session(dir.path(), MemoryStore::new());
    let err = fake
        .sign_in(&session, Browser::default(), SignInIntent::SignIn)
        .await
        .unwrap_err();
    assert!(matches!(err, SignInError::CodeRejected), "{err:?}");
    assert_eq!(session.status().state, SessionState::SignedOut);

    fake.knobs().exchange_error = None;
    fake.sign_in(&session, Browser::default(), SignInIntent::SignIn)
        .await
        .unwrap();
    let auths = fake.authorizations.lock().unwrap().clone();
    assert_eq!(auths[1]["client_id"], ISSUED_CLIENT, "the retained client");
    assert!(!auths[1].contains_key("agent_name_hint"));
    assert_eq!(
        registration_json(dir.path())["clients"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn a_registration_that_cannot_be_saved_leaves_the_session_in_use_as_it_was() {
    // registration.json is written before the refresh token: if it cannot
    // be, the session in use and its stored token are untouched, and the new
    // tokens are revoked.
    let fake = Fake::start().await;
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::new();
    let failing = Arc::new(AtomicBool::new(false));
    let config = fake
        .config(dir.path())
        .with_failing_registration_saves(failing.clone());
    let session = ChatGptSession::open(config, store.clone()).unwrap();
    fake.sign_in(&session, Browser::default(), SignInIntent::SignIn)
        .await
        .unwrap();
    {
        let failing = failing.clone();
        *fake.on_exchange.lock().unwrap() =
            Some(Box::new(move || failing.store(true, Ordering::SeqCst)));
    }
    let err = fake
        .sign_in(&session, Browser::default(), SignInIntent::SignIn)
        .await
        .unwrap_err();
    assert!(matches!(err, SignInError::Registration(_)), "{err:?}");
    assert_eq!(
        store.get().as_deref(),
        Some("RT-1"),
        "the stored token is still the session in use's"
    );
    assert_eq!(store.saves(), ["save RT-1"]);
    assert_eq!(session.status().state, SessionState::SignedIn);
    assert_eq!(session.bearer().await.unwrap().expose_secret(), "AT-1");
    let revoked = fake.revocations.lock().unwrap().clone();
    assert_eq!(revoked.len(), 1);
    assert_eq!(revoked[0]["token"], "RT-2");
}

#[tokio::test]
async fn another_account_can_be_signed_in_after_a_sign_out() {
    let fake = Fake::start().await;
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::new();
    let session = fake.signed_in(dir.path(), store.clone()).await;
    session.sign_out().await;
    fake.knobs().subject = "a-second-account".into();

    // Continue with ChatGPT reuses the saved account's client, as the docs
    // say signing back in should; another account is refused there...
    let err = fake
        .sign_in(&session, Browser::default(), SignInIntent::SignIn)
        .await
        .unwrap_err();
    assert!(matches!(err, SignInError::AccountChanged), "{err:?}");

    // ...and "use another account" registers a new client for it.
    let status = fake
        .sign_in(
            &session,
            Browser {
                client_id: CallbackClient::Send("oaiapp_second".into()),
                ..Browser::default()
            },
            SignInIntent::AnotherAccount,
        )
        .await
        .unwrap();
    assert_eq!(status.state, SessionState::SignedIn);
    let auths = fake.authorizations.lock().unwrap().clone();
    let another = &auths[2];
    assert_eq!(another["client_id"], "dynamic_agent_client");
    assert_eq!(another["agent_name_hint"], "edytlab");
    assert_eq!(another["ext_agent_host_id"], auths[0]["ext_agent_host_id"]);
    assert!(!another.contains_key("login_hint"), "{another:?}");

    let reg = registration_json(dir.path());
    assert_eq!(reg["active_client_id"], "oaiapp_second");
    let clients = reg["clients"].as_array().unwrap();
    assert_eq!(clients.len(), 2, "the first account's record is kept");
    assert_eq!(clients[0]["subject"], SUBJECT);
    assert_eq!(clients[1]["subject"], "a-second-account");
    assert_eq!(store.get().as_deref(), Some("RT-3"));
    assert_eq!(session.bearer().await.unwrap().expose_secret(), "AT-3");
    // The tokens of the refused attempt were revoked.
    assert!(fake
        .revocations
        .lock()
        .unwrap()
        .iter()
        .any(|r| r["token"] == "RT-2"));
}

#[tokio::test]
async fn a_refresh_token_the_keychain_refuses_keeps_the_session_in_memory() {
    for refusal in [
        SecretStoreError::TooLong,
        SecretStoreError::Unavailable("locked".into()),
    ] {
        let fake = Fake::start().await;
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::new();
        *store.save_error.lock().unwrap() = Some(refusal.clone());
        let session = fake.signed_in(dir.path(), store.clone()).await;
        let status = session.status();
        assert_eq!(status.state, SessionState::SignedIn);
        assert_eq!(
            status.token_storage,
            Some(TokenStorage::MemoryOnly(refusal.clone())),
            "it says so"
        );
        assert_eq!(store.get(), None);

        // It refreshes from memory.
        fake.clock.advance(Duration::from_secs(56 * 60));
        assert_eq!(session.bearer().await.unwrap().expose_secret(), "AT-2");
        assert_eq!(fake.refresh_requests()[0]["refresh_token"], "RT-1");

        // And next launch, the session has ended; the registration has not.
        let restarted = fake.session(dir.path(), store.clone());
        let status = restarted.restore().await.unwrap();
        assert!(
            matches!(status.state, SessionState::SessionEnded(_)),
            "{refusal:?}: {status:?}"
        );
        assert_eq!(status.email.as_deref(), Some(EMAIL));
    }
}

#[tokio::test]
async fn only_one_sign_in_at_a_time_and_cancel_ends_the_wait() {
    let fake = Fake::start().await;
    let dir = tempfile::tempdir().unwrap();
    let session = Arc::new(fake.session(dir.path(), MemoryStore::new()));
    let (opened, browser_opened) = tokio::sync::oneshot::channel::<()>();
    let waiting = {
        let session = session.clone();
        tokio::spawn(async move {
            // A browser that opens and never comes back.
            session
                .sign_in(
                    move |_url: &str| -> Result<(), String> {
                        let _ = opened.send(());
                        Ok(())
                    },
                    SignInIntent::SignIn,
                )
                .await
        })
    };
    browser_opened.await.expect("the browser was opened");
    assert!(session.is_signing_in());
    let second = fake
        .sign_in(&session, Browser::default(), SignInIntent::SignIn)
        .await
        .unwrap_err();
    assert!(
        matches!(second, SignInError::AlreadyInProgress),
        "{second:?}"
    );

    session.cancel_sign_in();
    let first = tokio::time::timeout(Duration::from_secs(10), waiting)
        .await
        .expect("cancel ends the wait")
        .unwrap()
        .unwrap_err();
    assert!(
        matches!(first, SignInError::Loopback(LoopbackError::Cancelled)),
        "{first:?}"
    );
    assert!(!session.is_signing_in());
    // A new attempt can start.
    fake.sign_in(&session, Browser::default(), SignInIntent::SignIn)
        .await
        .unwrap();
}

#[tokio::test]
async fn a_cancel_before_the_browser_opens_still_counts() {
    // Cancel pressed while discovery is still loading: the attempt ends
    // without opening the browser.
    let fake = Fake::start().await;
    fake.knobs().discovery_delay = Duration::from_millis(400);
    let dir = tempfile::tempdir().unwrap();
    let session = Arc::new(fake.session(dir.path(), MemoryStore::new()));
    let opened = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let attempt = {
        let session = session.clone();
        let opened = opened.clone();
        tokio::spawn(async move {
            session
                .sign_in(
                    move |_url: &str| -> Result<(), String> {
                        opened.store(true, std::sync::atomic::Ordering::SeqCst);
                        Ok(())
                    },
                    SignInIntent::SignIn,
                )
                .await
        })
    };
    while !session.is_signing_in() {
        tokio::task::yield_now().await;
    }
    session.cancel_sign_in();
    let err = tokio::time::timeout(Duration::from_secs(10), attempt)
        .await
        .expect("the attempt ends")
        .unwrap()
        .unwrap_err();
    assert!(
        matches!(err, SignInError::Loopback(LoopbackError::Cancelled)),
        "{err:?}"
    );
    assert!(
        !opened.load(std::sync::atomic::Ordering::SeqCst),
        "the browser was opened"
    );

    // A cancel with no sign-in in progress does not carry over to the next.
    session.cancel_sign_in();
    fake.sign_in(&session, Browser::default(), SignInIntent::SignIn)
        .await
        .unwrap();
}

#[tokio::test]
async fn a_sign_in_whose_caller_stops_waiting_still_finishes() {
    // The caller is dropped during the code exchange. The code is spent by
    // then, so the sign-in finishes on its own task, and it counts as in
    // progress until it has.
    let fake = Fake::start().await;
    let exchanging = Arc::new(AtomicBool::new(false));
    {
        let exchanging = exchanging.clone();
        *fake.on_exchange.lock().unwrap() = Some(Box::new(move || {
            exchanging.store(true, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(600));
        }));
    }
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::new();
    let session = fake.session(dir.path(), store.clone());
    {
        let attempt = fake.sign_in(&session, Browser::default(), SignInIntent::SignIn);
        tokio::pin!(attempt);
        loop {
            tokio::select! {
                done = &mut attempt => panic!("finished before the drop: {:?}", done.map(|s| s.state)),
                _ = tokio::time::sleep(Duration::from_millis(10)) => {
                    if exchanging.load(Ordering::SeqCst) {
                        tokio::time::sleep(Duration::from_millis(50)).await;
                        break;
                    }
                }
            }
        }
    }
    assert!(session.is_signing_in(), "still finishing");
    for _ in 0..100 {
        if !session.is_signing_in() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(!session.is_signing_in());
    assert_eq!(session.status().state, SessionState::SignedIn);
    assert_eq!(store.get().as_deref(), Some("RT-1"));
    assert_eq!(
        registration_json(dir.path())["active_client_id"],
        ISSUED_CLIENT
    );
}

#[tokio::test]
async fn a_returning_sign_in_whose_token_the_keychain_refuses_leaves_no_older_token() {
    // The new refresh token cannot be saved over the old one: the old one is
    // spent for this account's new session, so it is removed rather than
    // left to be restored at the next launch.
    let fake = Fake::start().await;
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::new();
    let session = fake.signed_in(dir.path(), store.clone()).await;
    assert_eq!(store.get().as_deref(), Some("RT-1"));
    *store.save_error.lock().unwrap() = Some(SecretStoreError::TooLong);
    let status = fake
        .sign_in(&session, Browser::default(), SignInIntent::SignIn)
        .await
        .unwrap();
    assert_eq!(
        status.token_storage,
        Some(TokenStorage::MemoryOnly(SecretStoreError::TooLong))
    );
    assert_eq!(store.get(), None, "RT-1 is not left behind");
    assert_eq!(session.bearer().await.unwrap().expose_secret(), "AT-2");
}

#[tokio::test]
async fn an_unreadable_registration_is_replaced_by_the_next_sign_in() {
    let fake = Fake::start().await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("chatgpt");
    std::fs::create_dir_all(&path).unwrap();
    std::fs::write(path.join(REGISTRATION_FILE), b"{ not json").unwrap();
    // A token from the session the file described, which nothing can use
    // or revoke now: its client id was in the file.
    let store = MemoryStore::new();
    store.set(Some("RT-stale"));
    let session = fake.session(dir.path(), store.clone());
    assert_eq!(session.status().state, SessionState::SignedOut);
    // Opening reads it as none but leaves it alone: it is set aside only
    // under the file lock, so a window that opens while another writes it
    // never moves a file that is being replaced.
    assert_eq!(
        std::fs::read(path.join(REGISTRATION_FILE)).unwrap(),
        b"{ not json"
    );
    assert!(!path.join("registration.json.corrupt").exists());
    fake.sign_in(&session, Browser::default(), SignInIntent::SignIn)
        .await
        .unwrap();
    // Set aside, not overwritten, so it can still be looked at.
    assert_eq!(
        std::fs::read(path.join("registration.json.corrupt")).unwrap(),
        b"{ not json"
    );
    let reg = RegistrationFile::new(&path).load().unwrap().unwrap();
    assert_eq!(reg.active_client_id.as_deref(), Some(ISSUED_CLIENT));
    // The stale token was cleared before the new one was saved.
    let log = store.log.lock().unwrap().clone();
    let deleted = log.iter().position(|l| l == "delete");
    let saved = log.iter().position(|l| l == "save RT-1");
    assert!(
        deleted.is_some() && deleted < saved,
        "the stale slot is cleared at the next sign-in: {log:?}"
    );

    // A newer build's file is not overwritten: opening refuses it.
    std::fs::write(
        path.join(REGISTRATION_FILE),
        br#"{"version": 2, "host_id": "urn:uuid:x"}"#,
    )
    .unwrap();
    assert!(ChatGptSession::open(fake.config(dir.path()), MemoryStore::new()).is_err());
}

// --- Secrets ----------------------------------------------------------------------

/// Fails to compile if `$ty` implements any of the traits: the trick
/// `tests/oauth_client.rs` explains.
macro_rules! assert_not_impl {
    ($ty:ty: $($trait:path),+ $(,)?) => {
        const _: fn() = || {
            trait Probe<M> {
                fn probe() {}
            }
            impl<T: ?Sized> Probe<()> for T {}
            $({
                #[allow(dead_code)]
                struct Implements;
                impl<T: ?Sized + $trait> Probe<Implements> for T {}
            })+
            let _ = <$ty as Probe<_>>::probe;
        };
    };
}

#[test]
fn the_session_and_its_status_cannot_be_displayed_serialized_or_compared() {
    use std::fmt::Display;
    // The session holds every token; the status holds the email address.
    assert_not_impl!(ChatGptSession: Display, serde::Serialize, PartialEq);
    assert_not_impl!(ChatGptStatus: Display, serde::Serialize);
}

#[tokio::test]
async fn no_secret_reaches_the_registration_debug_output_errors_or_urls() {
    let fake = Fake::start().await;
    fake.knobs().refresh_id_token = true;
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::new();
    let session = fake.signed_in(dir.path(), store.clone()).await;
    fake.clock.advance(Duration::from_secs(56 * 60));
    session.bearer().await.unwrap();
    // A second sign-in, so a URL is built while tokens exist.
    fake.sign_in(&session, Browser::default(), SignInIntent::SignIn)
        .await
        .unwrap();
    let ended = {
        fake.knobs().refresh_error = Some((400, json!({ "error": "invalid_grant" })));
        fake.clock.advance(Duration::from_secs(61 * 60));
        session.bearer().await.unwrap_err()
    };

    let verifier = fake.exchange_requests()[0]["code_verifier"].clone();
    let id_token = fake.id_token(ISSUED_CLIENT, None, SUBJECT);
    let mut secrets = vec![
        "AT-1".to_string(),
        "RT-1".into(),
        "AT-2".into(),
        "RT-2".into(),
        "RT-3".into(),
        CODE.into(),
        verifier,
        // Any ID token the fake signs starts with the same header.
        id_token.split('.').next().unwrap().to_string(),
    ];
    secrets.retain(|s| !s.is_empty());

    let registration =
        std::fs::read_to_string(dir.path().join("chatgpt").join(REGISTRATION_FILE)).unwrap();
    let debug = format!(
        "{session:?} {:?} {:#?} {ended:?}",
        session.status(),
        session.status()
    );
    let shown = format!("{ended}");
    let urls = format!("{:?}", fake.authorizations.lock().unwrap());
    for secret in &secrets {
        assert!(
            !registration.contains(secret.as_str()),
            "{secret} in registration.json"
        );
        assert!(!debug.contains(secret.as_str()), "{secret} in {debug}");
        assert!(!shown.contains(secret.as_str()), "{secret} in {shown}");
        assert!(
            !urls.contains(secret.as_str()),
            "{secret} in an authorization URL"
        );
    }
    // The email is a label in the file and a login hint, never in Debug.
    assert!(registration.contains(EMAIL));
    assert!(!debug.contains(EMAIL), "{debug}");
}
