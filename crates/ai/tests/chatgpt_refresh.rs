//! The ChatGPT session after sign-in (`ai::chatgpt::session`), against a
//! fake OpenAI issuer: when it refreshes, single-flight refresh in one
//! process and the file lock across processes, the rotated refresh token
//! saved before use, how each refresh failure is handled, restarts, and
//! sign-out with revocation.
//!
//! Time is a clock the tests move; access tokens live an hour.

mod chatgpt_support;
mod oauth_support;

use std::io::{BufRead, BufReader, Write as _};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use ai::chatgpt::session::{
    BearerError, ChatGptSession, EndReason, Revocation, SessionState, TokenStorage,
};
use ai::chatgpt::spec::UNUSABLE_REFRESH_TOKEN_CODES;
use ai::chatgpt::store::{RefreshTokenStore, SecretStoreError, LOCK_FILE, REGISTRATION_FILE};
use ai::chatgpt::SignInIntent;
use ai::oauth::SecretString;
use chatgpt_support::{
    form, Browser, CallbackClient, Fake, MemoryStore, EMAIL, ISSUED_CLIENT, SUBJECT,
};
use serde_json::{json, Value};
use wiremock::matchers::{method, path};
use wiremock::{Match, Mock, MockServer, Request, ResponseTemplate};

const MINUTE: Duration = Duration::from_secs(60);

fn registration_json(dir: &std::path::Path) -> Value {
    let text = std::fs::read_to_string(dir.join("chatgpt").join(REGISTRATION_FILE)).unwrap();
    serde_json::from_str(&text).unwrap()
}

async fn bearer(session: &ChatGptSession) -> String {
    session.bearer().await.unwrap().expose_secret().to_string()
}

/// Matches while the flag is set.
struct While(Arc<AtomicBool>);

impl Match for While {
    fn matches(&self, _: &Request) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// Matches the first `n` requests.
struct FirstN(AtomicUsize);

impl Match for FirstN {
    fn matches(&self, _: &Request) -> bool {
        self.0
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
            .is_ok()
    }
}

fn connection_reset(_: &Request) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::ConnectionReset, "reset")
}

// --- When to refresh ------------------------------------------------------------

#[tokio::test]
async fn the_access_token_is_used_until_five_minutes_before_it_expires() {
    let fake = Fake::start().await;
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::new();
    let session = fake.signed_in(dir.path(), store.clone()).await;

    fake.clock.advance(54 * MINUTE);
    assert_eq!(bearer(&session).await, "AT-1");
    assert!(
        fake.refresh_requests().is_empty(),
        "six minutes left: no refresh"
    );

    fake.clock.advance(MINUTE + Duration::from_secs(1));
    assert_eq!(bearer(&session).await, "AT-2");
    let refreshes = fake.refresh_requests();
    assert_eq!(refreshes.len(), 1);
    let r = &refreshes[0];
    assert_eq!(r["grant_type"], "refresh_token");
    assert_eq!(r["client_id"], ISSUED_CLIENT);
    assert_eq!(r["refresh_token"], "RT-1");
    assert_eq!(r["resource"], "https://api.openai.com/v1");
    assert!(!r.contains_key("scope"), "the grant is kept: {r:?}");
    assert_eq!(r.len(), 4, "{r:?}");
    assert_eq!(
        store.get().as_deref(),
        Some("RT-2"),
        "the rotated token is saved"
    );
    assert_eq!(bearer(&session).await, "AT-2", "and the new one is used");
}

#[tokio::test]
async fn concurrent_callers_refresh_once() {
    let fake = Fake::start().await;
    // The token endpoint is slow, so every caller arrives while it works.
    *fake.on_refresh.lock().unwrap() =
        Some(Box::new(|_| std::thread::sleep(Duration::from_millis(300))));
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::new();
    let session = Arc::new(fake.signed_in(dir.path(), store.clone()).await);
    fake.clock.advance(56 * MINUTE);

    let callers: Vec<_> = (0..16)
        .map(|_| {
            let session = session.clone();
            tokio::spawn(async move { bearer(&session).await })
        })
        .collect();
    for caller in callers {
        assert_eq!(caller.await.unwrap(), "AT-2");
    }
    assert_eq!(
        fake.refresh_requests().len(),
        1,
        "one refresh for sixteen callers"
    );
    assert_eq!(store.saves(), ["save RT-1", "save RT-2"]);
}

#[tokio::test]
async fn earliest_refresh_at_is_honoured_while_the_token_is_still_valid() {
    let fake = Fake::start().await;
    fake.knobs().earliest_refresh_after = Some(58 * 60);
    let dir = tempfile::tempdir().unwrap();
    let session = fake.signed_in(dir.path(), MemoryStore::new()).await;

    fake.clock.advance(56 * MINUTE);
    assert_eq!(
        bearer(&session).await,
        "AT-1",
        "inside the margin, but too early"
    );
    assert!(fake.refresh_requests().is_empty());

    fake.clock.advance(2 * MINUTE);
    assert_eq!(bearer(&session).await, "AT-2");
    assert_eq!(fake.refresh_requests().len(), 1);

    // An expired token is refreshed whatever `earliest_refresh_at` says:
    // there is nothing else to send.
    fake.knobs().earliest_refresh_after = Some(10 * 60 * 60);
    fake.clock.advance(56 * MINUTE);
    assert_eq!(bearer(&session).await, "AT-2", "AT-3's wait starts now");
    fake.clock.advance(61 * MINUTE);
    assert_eq!(bearer(&session).await, "AT-3");
    assert_eq!(fake.refresh_requests().len(), 2);
}

/// A refresh-token store whose `save` waits until the test opens a gate.
struct GatedStore {
    inner: Arc<MemoryStore>,
    gate: Arc<(Mutex<bool>, Condvar)>,
    gated: AtomicBool,
}

impl RefreshTokenStore for GatedStore {
    fn load(&self) -> Result<Option<SecretString>, SecretStoreError> {
        self.inner.load()
    }
    fn save(&self, token: &SecretString) -> Result<(), SecretStoreError> {
        if self.gated.load(Ordering::SeqCst) {
            let (open, cv) = &*self.gate;
            let mut open = open.lock().unwrap();
            while !*open {
                open = cv.wait(open).unwrap();
            }
        }
        self.inner.save(token)
    }
    fn delete(&self) -> Result<(), SecretStoreError> {
        self.inner.delete()
    }
}

/// Opens the gate when dropped, so a failing assertion cannot leave the
/// store's thread waiting (and the test runtime waiting for it).
struct OpenOnDrop(Arc<(Mutex<bool>, Condvar)>);

impl OpenOnDrop {
    fn open(&self) {
        let (open, cv) = &*self.0;
        *open.lock().unwrap() = true;
        cv.notify_all();
    }
}

impl Drop for OpenOnDrop {
    fn drop(&mut self) {
        self.open();
    }
}

#[tokio::test]
async fn the_rotated_refresh_token_is_saved_before_the_new_access_token_is_used() {
    let fake = Fake::start().await;
    let dir = tempfile::tempdir().unwrap();
    let memory = MemoryStore::new();
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let store = Arc::new(GatedStore {
        inner: memory.clone(),
        gate: gate.clone(),
        gated: AtomicBool::new(false),
    });
    let session = Arc::new(ChatGptSession::open(fake.config(dir.path()), store.clone()).unwrap());
    fake.sign_in(&session, Browser::default(), SignInIntent::SignIn)
        .await
        .unwrap();
    store.gated.store(true, Ordering::SeqCst);
    let gate = OpenOnDrop(gate);
    fake.clock.advance(61 * MINUTE);

    // The refresh has its answer but the save of RT-2 is held: nobody gets
    // AT-2 meanwhile, neither the caller that refreshed nor another one.
    let first = tokio::spawn({
        let session = session.clone();
        async move { bearer(&session).await }
    });
    while fake.refresh_requests().is_empty() {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let second = tokio::spawn({
        let session = session.clone();
        async move { bearer(&session).await }
    });
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        !first.is_finished() && !second.is_finished(),
        "AT-2 handed out before RT-2 was saved"
    );
    assert_eq!(memory.get().as_deref(), Some("RT-1"));

    gate.open();
    assert_eq!(first.await.unwrap(), "AT-2");
    assert_eq!(second.await.unwrap(), "AT-2");
    assert_eq!(memory.get().as_deref(), Some("RT-2"));
    assert_eq!(fake.refresh_requests().len(), 1);
}

#[tokio::test]
async fn a_rotated_token_the_keychain_refuses_is_kept_in_memory_and_reported() {
    let fake = Fake::start().await;
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::new();
    let session = fake.signed_in(dir.path(), store.clone()).await;
    *store.save_error.lock().unwrap() = Some(SecretStoreError::TooLong);
    fake.clock.advance(56 * MINUTE);
    assert_eq!(bearer(&session).await, "AT-2");
    assert_eq!(
        session.status().token_storage,
        Some(TokenStorage::MemoryOnly(SecretStoreError::TooLong))
    );
    // The spent RT-1 is not left behind to fail at the next launch.
    assert_eq!(store.get(), None);
    // The next refresh uses the token in memory.
    fake.clock.advance(56 * MINUTE);
    assert_eq!(bearer(&session).await, "AT-3");
    assert_eq!(fake.refresh_requests()[1]["refresh_token"], "RT-2");
}

// --- Refresh failures ------------------------------------------------------------

#[tokio::test]
async fn an_unusable_refresh_token_ends_the_session_and_keeps_the_registration() {
    for code in UNUSABLE_REFRESH_TOKEN_CODES {
        for body in [
            json!({ "error": code }),
            json!({ "error": { "code": code } }),
        ] {
            let fake = Fake::start().await;
            let dir = tempfile::tempdir().unwrap();
            let store = MemoryStore::new();
            let session = fake.signed_in(dir.path(), store.clone()).await;
            fake.knobs().refresh_error = Some((400, body.clone()));
            fake.clock.advance(61 * MINUTE);

            let expected = EndReason::RefreshTokenUnusable {
                code: code.to_string(),
            };
            assert_eq!(
                session.bearer().await.unwrap_err(),
                BearerError::SessionEnded(expected.clone()),
                "{body}"
            );
            assert_eq!(session.status().state, SessionState::SessionEnded(expected));
            assert_eq!(store.get(), None, "{body}: the unusable token is cleared");
            let reg = registration_json(dir.path());
            assert_eq!(reg["active_client_id"], ISSUED_CLIENT, "{body}");
            assert_eq!(reg["clients"][0]["subject"], SUBJECT, "{body}");
            assert!(reg["host_id"].as_str().unwrap().starts_with("urn:uuid:"));

            // Recovery is a returning sign-in with the saved client.
            fake.knobs().refresh_error = None;
            fake.sign_in(&session, Browser::default(), SignInIntent::SignIn)
                .await
                .unwrap();
            let auth = fake.authorizations.lock().unwrap()[1].clone();
            assert_eq!(auth["client_id"], ISSUED_CLIENT, "{body}");
            assert!(!auth.contains_key("prompt"));
        }
    }
}

#[tokio::test]
async fn a_network_failure_keeps_every_credential() {
    let fake = Fake::start().await;
    let down = Arc::new(AtomicBool::new(true));
    Mock::given(method("POST"))
        .and(path("/api/accounts/oauth/token"))
        .and(While(down.clone()))
        .respond_with_err(connection_reset)
        .with_priority(1)
        .mount(&fake.server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::new();
    down.store(false, Ordering::SeqCst);
    let session = fake.signed_in(dir.path(), store.clone()).await;
    down.store(true, Ordering::SeqCst);

    // Inside the margin, the refresh fails: the token in hand still works.
    fake.clock.advance(56 * MINUTE);
    assert_eq!(bearer(&session).await, "AT-1");
    // Expired: no bearer, but nothing is erased.
    fake.clock.advance(5 * MINUTE);
    let err = session.bearer().await.unwrap_err();
    assert!(matches!(err, BearerError::Unavailable { .. }), "{err:?}");
    assert_eq!(session.status().state, SessionState::SignedIn);
    assert_eq!(store.get().as_deref(), Some("RT-1"));
    assert!(!store.log.lock().unwrap().iter().any(|l| l == "delete"));

    // Back up, and after the wait, the same refresh token works.
    down.store(false, Ordering::SeqCst);
    fake.clock.advance(MINUTE);
    assert_eq!(bearer(&session).await, "AT-2");
    assert_eq!(
        fake.refresh_requests().last().unwrap()["refresh_token"],
        "RT-1"
    );
}

#[tokio::test]
async fn a_temporary_refusal_keeps_the_credentials_and_backs_off() {
    for (status, body) in [
        (503, json!({ "error": "temporarily_unavailable" })),
        (500, json!({ "detail": "upstream" })),
        (502, json!({ "error": "upstream_unavailable" })),
        (429, json!({ "error": "rate_limited" })),
    ] {
        let fake = Fake::start().await;
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::new();
        let session = fake.signed_in(dir.path(), store.clone()).await;
        fake.knobs().refresh_error = Some((status, body.clone()));
        fake.clock.advance(61 * MINUTE);

        let err = session.bearer().await.unwrap_err();
        assert!(
            matches!(err, BearerError::Unavailable { .. }),
            "{body}: {err:?}"
        );
        // Asked again at once: the same answer, and no new request.
        assert_eq!(session.bearer().await.unwrap_err(), err);
        assert_eq!(fake.refresh_requests().len(), 1, "{body}: it waits");
        // After the wait it tries again, and keeps the credentials.
        fake.clock.advance(Duration::from_secs(16));
        assert!(session.bearer().await.is_err());
        assert_eq!(fake.refresh_requests().len(), 2, "{body}");
        assert_eq!(store.get().as_deref(), Some("RT-1"), "{body}");
        assert_eq!(session.status().state, SessionState::SignedIn);

        fake.knobs().refresh_error = None;
        fake.clock.advance(MINUTE);
        assert_eq!(bearer(&session).await, "AT-2", "{body}");
    }
}

#[tokio::test]
async fn invalid_client_is_reported_and_the_credentials_are_kept() {
    // The docs answer `invalid_client` with "fix the client configuration",
    // not with clearing the tokens.
    let fake = Fake::start().await;
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::new();
    let session = fake.signed_in(dir.path(), store.clone()).await;
    fake.knobs().refresh_error = Some((401, json!({ "error": "invalid_client" })));
    fake.clock.advance(61 * MINUTE);
    assert_eq!(
        session.bearer().await.unwrap_err(),
        BearerError::Refused {
            status: 401,
            code: Some("invalid_client".into())
        }
    );
    assert_eq!(store.get().as_deref(), Some("RT-1"));
    assert_eq!(session.status().state, SessionState::SignedIn);
}

#[tokio::test]
async fn a_raced_refresh_is_retried_with_the_newer_stored_token() {
    // Another writer refreshed with RT-1 (and saved RT-2) while this
    // session's request with RT-1 was in flight: the server answers
    // refresh_token_reused. The store holds a newer token, so the session
    // retries once with it instead of ending.
    let fake = Fake::start().await;
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::new();
    let session = fake.signed_in(dir.path(), store.clone()).await;
    {
        let fake_ref = Arc::downgrade(&fake);
        let store = store.clone();
        *fake.on_refresh.lock().unwrap() = Some(Box::new(move |sent| {
            if sent == "RT-1" {
                let fake = fake_ref.upgrade().unwrap();
                let newer = fake.rotate_elsewhere();
                store.set(Some(&newer));
            }
        }));
    }
    fake.clock.advance(61 * MINUTE);
    assert_eq!(bearer(&session).await, "AT-3");
    let sent: Vec<String> = fake
        .refresh_requests()
        .iter()
        .map(|f| f["refresh_token"].clone())
        .collect();
    assert_eq!(sent, ["RT-1", "RT-2"]);
    assert_eq!(store.get().as_deref(), Some("RT-3"));
    assert_eq!(session.status().state, SessionState::SignedIn);
}

#[tokio::test]
async fn a_reused_token_with_nothing_newer_ends_the_session() {
    let fake = Fake::start().await;
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::new();
    let session = fake.signed_in(dir.path(), store.clone()).await;
    fake.rotate_elsewhere(); // RT-1 spent; the store still says RT-1.
    fake.clock.advance(61 * MINUTE);
    assert_eq!(
        session.bearer().await.unwrap_err(),
        BearerError::SessionEnded(EndReason::RefreshTokenUnusable {
            code: "refresh_token_reused".into()
        })
    );
    assert_eq!(fake.refresh_requests().len(), 1);
    assert_eq!(store.get(), None);
}

#[tokio::test]
async fn a_session_never_erases_a_token_another_process_saved() {
    let fake = Fake::start().await;
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::new();
    let session = fake.signed_in(dir.path(), store.clone()).await;
    fake.knobs().refresh_error = Some((400, json!({ "error": "invalid_grant" })));
    {
        let store = store.clone();
        // While this refresh is refused, another process signs in afresh.
        *fake.on_refresh.lock().unwrap() = Some(Box::new(move |_| {
            store.set(Some("RT-from-another-process"))
        }));
    }
    fake.clock.advance(61 * MINUTE);
    assert!(matches!(
        session.bearer().await,
        Err(BearerError::SessionEnded(_))
    ));
    assert_eq!(store.get().as_deref(), Some("RT-from-another-process"));
}

// --- Refreshed ID tokens ----------------------------------------------------------

#[tokio::test]
async fn a_refreshed_id_token_is_checked_against_the_original() {
    let fake = Fake::start().await;
    fake.knobs().refresh_id_token = true;
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::new();
    let session = fake.signed_in(dir.path(), store.clone()).await;

    // Same account, no nonce (OpenID Connect Core §12.2): accepted.
    fake.clock.advance(56 * MINUTE);
    assert_eq!(bearer(&session).await, "AT-2");

    // Another account: the session ends, and the token it just got is
    // removed again.
    fake.knobs().refresh_subject = Some("someone-else".into());
    fake.clock.advance(56 * MINUTE);
    let err = session.bearer().await.unwrap_err();
    assert!(
        matches!(
            err,
            BearerError::SessionEnded(EndReason::IdTokenRejected { .. })
        ),
        "{err:?}"
    );
    assert_eq!(store.get(), None);
    // And revoked, with this session's client, so it is not left live at
    // OpenAI.
    let revoked = fake.revocations.lock().unwrap().clone();
    assert_eq!(revoked.len(), 1);
    assert_eq!(revoked[0]["token"], "RT-3");
    assert_eq!(revoked[0]["client_id"], ISSUED_CLIENT);
}

// --- Restarts ----------------------------------------------------------------------

#[tokio::test]
async fn a_restarted_session_refreshes_from_the_stored_token() {
    let fake = Fake::start().await;
    {
        let mut knobs = fake.knobs();
        // A refreshed ID token with the sign-in's nonce and an `azp`: after a
        // restart it is checked against the claims saved with the account.
        knobs.refresh_id_token = true;
        knobs.refresh_nonce = true;
        knobs.azp = Some(ISSUED_CLIENT.into());
    }
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::new();
    drop(fake.signed_in(dir.path(), store.clone()).await);

    let before_open = store.log.lock().unwrap().len();
    let session = fake.session(dir.path(), store.clone());
    assert_eq!(session.status().state, SessionState::NotLoaded);
    assert_eq!(
        store.log.lock().unwrap().len(),
        before_open,
        "open reads no keychain"
    );
    let status = session.restore().await.unwrap();
    assert_eq!(status.state, SessionState::SignedIn);
    assert_eq!(status.email.as_deref(), Some(EMAIL));
    // No access token survives a restart: the first bearer refreshes.
    assert_eq!(bearer(&session).await, "AT-2");
    assert_eq!(fake.refresh_requests()[0]["refresh_token"], "RT-1");

    // The first bearer restores by itself too.
    let lazy = fake.session(dir.path(), store.clone());
    assert_eq!(bearer(&lazy).await, "AT-3");
}

#[tokio::test]
async fn a_refresh_token_missing_at_launch_is_a_session_ended() {
    // Linux with no Secret Service, after a reboot: the kernel keyring lost
    // the token, registration.json did not.
    let fake = Fake::start().await;
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::new();
    drop(fake.signed_in(dir.path(), store.clone()).await);
    store.set(None);

    let session = fake.session(dir.path(), store.clone());
    let status = session.restore().await.unwrap();
    assert_eq!(
        status.state,
        SessionState::SessionEnded(EndReason::NotStored)
    );
    assert_eq!(status.email.as_deref(), Some(EMAIL));
    assert_eq!(
        session.bearer().await.unwrap_err(),
        BearerError::SessionEnded(EndReason::NotStored)
    );
    fake.sign_in(&session, Browser::default(), SignInIntent::SignIn)
        .await
        .unwrap();
    let auth = fake.authorizations.lock().unwrap()[1].clone();
    assert_eq!(auth["client_id"], ISSUED_CLIENT, "no new registration");
    assert_eq!(auth["login_hint"], EMAIL);
}

#[tokio::test]
async fn a_keychain_that_cannot_be_read_at_launch_leaves_the_session_saved() {
    let fake = Fake::start().await;
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::new();
    drop(fake.signed_in(dir.path(), store.clone()).await);
    *store.load_error.lock().unwrap() = Some(SecretStoreError::Unavailable("locked".into()));

    let session = fake.session(dir.path(), store.clone());
    assert!(session.restore().await.is_err());
    assert_eq!(session.status().state, SessionState::NotLoaded);
    assert!(matches!(session.bearer().await, Err(BearerError::Store(_))));
    assert_eq!(store.get().as_deref(), Some("RT-1"), "nothing erased");

    *store.load_error.lock().unwrap() = None;
    assert_eq!(bearer(&session).await, "AT-2");
}

#[tokio::test]
async fn without_a_refresh_token_the_session_ends_when_the_access_token_expires() {
    let fake = Fake::start().await;
    fake.knobs().issue_refresh_token = false;
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::new();
    let session = fake.signed_in(dir.path(), store.clone()).await;
    assert_eq!(session.status().token_storage, None);
    fake.clock.advance(58 * MINUTE);
    assert_eq!(
        bearer(&session).await,
        "AT-1",
        "nothing to refresh with yet"
    );
    fake.clock.advance(3 * MINUTE);
    assert_eq!(
        session.bearer().await.unwrap_err(),
        BearerError::SessionEnded(EndReason::NoRefreshToken)
    );
    assert!(fake.refresh_requests().is_empty());
}

#[tokio::test]
async fn a_rejected_bearer_is_refreshed_once_for_all_who_saw_it() {
    // For the provider's 401 path: the first caller with the rejected token
    // refreshes; one arriving later with the same rejected token gets the
    // new one without another request.
    let fake = Fake::start().await;
    let dir = tempfile::tempdir().unwrap();
    let session = fake.signed_in(dir.path(), MemoryStore::new()).await;
    let rejected = session.bearer().await.unwrap();
    let first = session.refresh_rejected(&rejected).await.unwrap();
    let second = session.refresh_rejected(&rejected).await.unwrap();
    assert_eq!(first.expose_secret(), "AT-2");
    assert_eq!(second.expose_secret(), "AT-2");
    assert_eq!(fake.refresh_requests().len(), 1);
}

// --- Across processes -----------------------------------------------------------

#[tokio::test]
async fn two_processes_refresh_in_turn_with_the_rotated_token() {
    // Two sessions over one directory and one keychain stand for two
    // edytlab processes: each has its own in-process lock, so only the file
    // lock keeps them apart. The token endpoint is slow, so both refresh at
    // once. Without the lock (or without re-reading the store under it),
    // the second sends the RT-1 the first just spent and is told
    // refresh_token_reused.
    let fake = Fake::start().await;
    *fake.on_refresh.lock().unwrap() =
        Some(Box::new(|_| std::thread::sleep(Duration::from_millis(300))));
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::new();
    let a = Arc::new(fake.signed_in(dir.path(), store.clone()).await);
    let b = Arc::new(fake.session(dir.path(), store.clone()));
    b.restore().await.unwrap();
    fake.clock.advance(61 * MINUTE);

    let (from_a, from_b) = tokio::join!(
        tokio::spawn({
            let a = a.clone();
            async move { a.bearer().await }
        }),
        tokio::spawn({
            let b = b.clone();
            async move { b.bearer().await }
        }),
    );
    let mut got = vec![
        from_a
            .unwrap()
            .expect("A refreshed")
            .expose_secret()
            .to_string(),
        from_b
            .unwrap()
            .expect("B refreshed")
            .expose_secret()
            .to_string(),
    ];
    got.sort();
    assert_eq!(got, ["AT-2", "AT-3"]);
    let sent: Vec<String> = fake
        .refresh_requests()
        .iter()
        .map(|f| f["refresh_token"].clone())
        .collect();
    assert_eq!(
        sent,
        ["RT-1", "RT-2"],
        "the second used the first's rotated token"
    );
    assert_eq!(store.get().as_deref(), Some("RT-3"));
    assert!(dir.path().join("chatgpt").join(LOCK_FILE).exists());
}

/// Not a test of its own: the other process of
/// `another_process_holding_the_lock_makes_a_refresh_wait`. With
/// `CHATGPT_LOCK_HOLDER` set to a lock file it takes the file's lock, says
/// so on stdout, and holds it until its stdin closes.
#[test]
fn lock_holder_child() {
    let Ok(path) = std::env::var("CHATGPT_LOCK_HOLDER") else {
        return;
    };
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .unwrap();
    fs4::FileExt::lock(&file).unwrap();
    println!("LOCKED");
    std::io::stdout().flush().unwrap();
    let mut line = String::new();
    while std::io::stdin().read_line(&mut line).unwrap_or(0) > 0 {}
    drop(file);
}

#[tokio::test]
async fn another_process_holding_the_lock_makes_a_refresh_wait() {
    let fake = Fake::start().await;
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::new();
    let config = fake
        .config(dir.path())
        .with_lock_timeout(Duration::from_millis(300));
    let session = ChatGptSession::open(config, store.clone()).unwrap();
    fake.sign_in(&session, Browser::default(), SignInIntent::SignIn)
        .await
        .unwrap();

    let lock_file = dir.path().join("chatgpt").join(LOCK_FILE);
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "lock_holder_child",
            "--exact",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("CHATGPT_LOCK_HOLDER", &lock_file)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    loop {
        let line = lines
            .next()
            .expect("the child says it holds the lock")
            .unwrap();
        // libtest may print the test's name on the same line.
        if line.contains("LOCKED") {
            break;
        }
    }

    // Still valid: used as it is while the lock is busy.
    fake.clock.advance(56 * MINUTE);
    assert_eq!(bearer(&session).await, "AT-1");
    // Expired: no refresh while the other process holds the lock, and
    // nothing erased.
    fake.clock.advance(5 * MINUTE);
    let err = session.bearer().await.unwrap_err();
    assert!(
        matches!(&err, BearerError::Unavailable { reason } if reason.contains("lock")),
        "{err:?}"
    );
    assert!(fake.refresh_requests().is_empty());
    assert_eq!(store.get().as_deref(), Some("RT-1"));

    // The other process lets go.
    drop(child.stdin.take());
    assert!(child.wait().unwrap().success());
    fake.clock.advance(MINUTE);
    assert_eq!(bearer(&session).await, "AT-2");
    assert_eq!(fake.refresh_requests().len(), 1);
}

// --- Sign-out ---------------------------------------------------------------------

#[tokio::test]
async fn sign_out_revokes_the_refresh_token_with_the_issued_client() {
    let fake = Fake::start().await;
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::new();
    let session = fake.signed_in(dir.path(), store.clone()).await;
    let report = session.sign_out().await;
    assert_eq!(report.revocation, Revocation::Confirmed);
    assert_eq!(report.removed_from_store, Ok(()));

    let revocations = fake.revocations.lock().unwrap().clone();
    assert_eq!(revocations.len(), 1);
    let r = &revocations[0];
    assert_eq!(r["token"], "RT-1");
    assert_eq!(r["token_type_hint"], "refresh_token");
    assert_eq!(r["client_id"], ISSUED_CLIENT);
    assert_eq!(r.len(), 3, "{r:?}");

    assert_eq!(store.get(), None);
    assert_eq!(session.status().state, SessionState::SignedOut);
    assert_eq!(
        session.bearer().await.unwrap_err(),
        BearerError::NotSignedIn
    );
    // The client mapping and the host ID stay for the next sign-in.
    let reg = registration_json(dir.path());
    assert_eq!(reg["active_client_id"], ISSUED_CLIENT);
    assert_eq!(reg["clients"][0]["signed_out"], true);
    assert!(reg["host_id"].as_str().unwrap().starts_with("urn:uuid:"));
    // A restart reads "signed out", not a session to restore.
    let restarted = fake.session(dir.path(), store.clone());
    assert_eq!(restarted.status().state, SessionState::SignedOut);
}

#[tokio::test]
async fn sign_out_retries_a_5xx_or_a_network_failure() {
    let fake = Fake::start().await;
    fake.knobs().revoke_statuses = vec![503];
    Mock::given(method("POST"))
        .and(path("/api/accounts/oauth/revoke"))
        .and(FirstN(AtomicUsize::new(1)))
        .respond_with_err(connection_reset)
        .with_priority(1)
        .mount(&fake.server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let session = fake.signed_in(dir.path(), MemoryStore::new()).await;
    let report = session.sign_out().await;
    assert_eq!(report.revocation, Revocation::Confirmed);
    let attempts = fake
        .server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.url.path() == "/api/accounts/oauth/revoke")
        .count();
    assert_eq!(attempts, 3, "a reset, a 503, then the 200");
}

#[tokio::test]
async fn an_unconfirmed_revocation_still_clears_the_tokens_and_says_so() {
    for (statuses, attempts) in [(vec![503; 10], 4), (vec![400], 1)] {
        let fake = Fake::start().await;
        fake.knobs().revoke_statuses = statuses.clone();
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::new();
        let session = fake.signed_in(dir.path(), store.clone()).await;
        let report = session.sign_out().await;
        assert!(
            matches!(report.revocation, Revocation::NotConfirmed { .. }),
            "{statuses:?}: {report:?}"
        );
        assert_eq!(
            fake.revocations.lock().unwrap().len(),
            attempts,
            "{statuses:?}: a 5xx is retried, a 4xx is not"
        );
        assert_eq!(store.get(), None);
        assert_eq!(session.status().state, SessionState::SignedOut);
    }
}

#[tokio::test]
async fn the_refresh_token_is_never_revoked_anywhere_but_the_issuer() {
    let fake = Fake::start().await;
    let elsewhere = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&elsewhere)
        .await;
    fake.knobs().revocation_endpoint = Some(format!("{}/revoke", elsewhere.uri()));
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::new();
    let session = fake.signed_in(dir.path(), store.clone()).await;
    let report = session.sign_out().await;
    assert!(
        matches!(report.revocation, Revocation::NotConfirmed { .. }),
        "{report:?}"
    );
    assert!(elsewhere.received_requests().await.unwrap().is_empty());
    assert_eq!(store.get(), None);
}

#[tokio::test]
async fn signing_out_of_an_ended_session_revokes_nothing() {
    let fake = Fake::start().await;
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::new();
    let session = fake.signed_in(dir.path(), store.clone()).await;
    fake.knobs().refresh_error = Some((400, json!({ "error": "invalid_grant" })));
    fake.clock.advance(61 * MINUTE);
    assert!(session.bearer().await.is_err());
    let report = session.sign_out().await;
    assert_eq!(report.revocation, Revocation::NothingToRevoke);
    assert!(fake.revocations.lock().unwrap().is_empty());
    assert_eq!(session.status().state, SessionState::SignedOut);
}

#[test]
fn the_session_can_be_shared_and_its_futures_run_on_any_task() {
    // Tauri keeps state in an `Arc` and runs async commands on other
    // threads, and the provider calls `bearer` from the agent's tasks.
    // Checked when this file compiles; nothing runs.
    fn shared<T: Send + Sync>() {}
    fn spawnable<T: Send>(_: &T) {}
    shared::<ChatGptSession>();
    let _ = |s: &ChatGptSession, t: &SecretString| {
        spawnable(&s.bearer());
        spawnable(&s.refresh_rejected(t));
        spawnable(&s.restore());
        spawnable(&s.sign_out());
        spawnable(&s.sign_in(|_: &str| Ok::<(), String>(()), SignInIntent::SignIn));
    };
}

#[test]
fn form_helper_reads_what_the_session_sends() {
    let f = form(b"a=1&b=x%20y");
    assert_eq!(f["a"], "1");
    assert_eq!(f["b"], "x y");
}

// --- Several windows, and refreshes cut short --------------------------------------

#[tokio::test]
async fn a_window_with_stale_claims_does_not_end_a_newer_sign_in() {
    // Windows A and B share the keychain slot and registration.json. In A
    // the user signs out and back in (a new refresh token and a new
    // `auth_time`). B still holds the old sign-in's claims; under the lock
    // it must take the account as registration.json now has it, not judge
    // the newer session's refreshed ID token against its stale copy and
    // erase it.
    let fake = Fake::start().await;
    fake.knobs().refresh_id_token = true;
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::new();
    let a = fake.signed_in(dir.path(), store.clone()).await;
    let b = fake.session(dir.path(), store.clone());
    b.restore().await.unwrap();

    fake.clock.advance(10 * MINUTE);
    assert_eq!(a.sign_out().await.revocation, Revocation::Confirmed);
    fake.sign_in(&a, Browser::default(), SignInIntent::SignIn)
        .await
        .unwrap();
    assert_eq!(store.get().as_deref(), Some("RT-2"));

    // B's first request refreshes the session A now holds.
    assert_eq!(bearer(&b).await, "AT-3");
    assert_eq!(store.get().as_deref(), Some("RT-3"));
    assert_eq!(b.status().state, SessionState::SignedIn);
    // And A, an hour on, refreshes from what B saved.
    fake.clock.advance(61 * MINUTE);
    assert_eq!(bearer(&a).await, "AT-4");
    assert_eq!(a.status().state, SessionState::SignedIn);
    assert_eq!(store.get().as_deref(), Some("RT-4"));
}

#[tokio::test]
async fn a_window_whose_account_was_replaced_elsewhere_ends_without_deleting() {
    // In window A the user signs in to another ChatGPT account. The keychain
    // slot now holds that account's token: window B's session is over, and
    // it must neither refresh with that token nor delete it.
    let fake = Fake::start().await;
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::new();
    let a = fake.signed_in(dir.path(), store.clone()).await;
    let b = fake.session(dir.path(), store.clone());
    b.restore().await.unwrap();
    fake.knobs().subject = "a-second-account".into();
    fake.sign_in(
        &a,
        Browser {
            client_id: CallbackClient::Send("oaiapp_second".into()),
            ..Browser::default()
        },
        SignInIntent::AnotherAccount,
    )
    .await
    .unwrap();
    assert_eq!(store.get().as_deref(), Some("RT-2"));

    fake.clock.advance(61 * MINUTE);
    assert_eq!(
        b.bearer().await.unwrap_err(),
        BearerError::SessionEnded(EndReason::ReplacedElsewhere)
    );
    assert!(fake.refresh_requests().is_empty(), "B sent nothing");
    assert_eq!(store.get().as_deref(), Some("RT-2"), "and deleted nothing");
    // A carries on with its account.
    assert_eq!(bearer(&a).await, "AT-3");
    assert_eq!(fake.refresh_requests()[0]["client_id"], "oaiapp_second");
}

#[tokio::test]
async fn a_sign_out_in_another_window_ends_this_one_without_a_refresh() {
    let fake = Fake::start().await;
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::new();
    let a = fake.signed_in(dir.path(), store.clone()).await;
    let b = fake.session(dir.path(), store.clone());
    b.restore().await.unwrap();
    a.sign_out().await;
    fake.clock.advance(61 * MINUTE);
    assert_eq!(
        b.bearer().await.unwrap_err(),
        BearerError::SessionEnded(EndReason::SignedOutElsewhere)
    );
    assert!(
        fake.refresh_requests().is_empty(),
        "B must not refresh with the token A signed out"
    );
}

#[tokio::test]
async fn a_5xx_with_an_unusable_code_keeps_the_credentials() {
    // A server error decides nothing about the token, whatever code it
    // carries.
    for (status, body) in [
        (503, json!({ "error": "invalid_grant" })),
        (
            502,
            json!({ "error": { "code": "refresh_token_expired", "message": "x" } }),
        ),
        (500, json!({ "error": "refresh_token_reused" })),
    ] {
        let fake = Fake::start().await;
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::new();
        let session = fake.signed_in(dir.path(), store.clone()).await;
        fake.knobs().refresh_error = Some((status, body.clone()));
        fake.clock.advance(61 * MINUTE);
        let err = session.bearer().await.unwrap_err();
        assert!(
            matches!(err, BearerError::Unavailable { .. }),
            "{status} {body}: {err:?}"
        );
        assert_eq!(store.get().as_deref(), Some("RT-1"), "{status} {body}");
        assert_eq!(session.status().state, SessionState::SignedIn);
    }
}

#[tokio::test]
async fn signing_out_of_an_ended_window_revokes_the_stored_session() {
    // Window A shows "Session ended"; window B has since signed in afresh.
    // Signing out in A signs the account out: B's stored token is revoked
    // before it is cleared, not just deleted.
    let fake = Fake::start().await;
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::new();
    let a = fake.signed_in(dir.path(), store.clone()).await;
    fake.knobs().refresh_error = Some((400, json!({ "error": "invalid_grant" })));
    fake.clock.advance(61 * MINUTE);
    assert!(matches!(
        a.bearer().await,
        Err(BearerError::SessionEnded(_))
    ));
    fake.knobs().refresh_error = None;
    let b = fake.session(dir.path(), store.clone());
    fake.sign_in(&b, Browser::default(), SignInIntent::SignIn)
        .await
        .unwrap();
    assert_eq!(store.get().as_deref(), Some("RT-2"));

    let report = a.sign_out().await;
    assert_eq!(report.revocation, Revocation::Confirmed);
    let revoked = fake.revocations.lock().unwrap().clone();
    assert_eq!(revoked.len(), 1);
    assert_eq!(revoked[0]["token"], "RT-2");
    assert_eq!(revoked[0]["client_id"], ISSUED_CLIENT);
    assert_eq!(store.get(), None);
}

#[tokio::test]
async fn signing_out_of_a_session_not_yet_loaded_revokes_its_stored_token() {
    let fake = Fake::start().await;
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::new();
    drop(fake.signed_in(dir.path(), store.clone()).await);
    let restarted = fake.session(dir.path(), store.clone());
    assert_eq!(restarted.status().state, SessionState::NotLoaded);
    let report = restarted.sign_out().await;
    assert_eq!(report.revocation, Revocation::Confirmed);
    assert_eq!(fake.revocations.lock().unwrap()[0]["token"], "RT-1");
    assert_eq!(store.get(), None);
    assert_eq!(restarted.status().state, SessionState::SignedOut);
}

#[tokio::test]
async fn a_refresh_the_caller_stops_waiting_for_still_saves_the_rotated_token() {
    // A Stop button or a timeout drops the bearer future while the refresh
    // is in flight. The server has rotated the token by then; the refresh
    // must still save the new one, or the next call sends the spent one.
    let fake = Fake::start().await;
    *fake.on_refresh.lock().unwrap() =
        Some(Box::new(|_| std::thread::sleep(Duration::from_millis(500))));
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::new();
    let session = Arc::new(fake.signed_in(dir.path(), store.clone()).await);
    fake.clock.advance(61 * MINUTE);
    let cut_short = tokio::time::timeout(Duration::from_millis(150), session.bearer()).await;
    assert!(cut_short.is_err(), "the caller gave up first");
    // Give the refresh time to finish on its own.
    for _ in 0..100 {
        if store.get().as_deref() == Some("RT-2") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(store.get().as_deref(), Some("RT-2"));
    *fake.on_refresh.lock().unwrap() = None;
    assert_eq!(bearer(&session).await, "AT-2");
    assert_eq!(fake.refresh_requests().len(), 1);
}

// --- Gaps the review's mutations found ---------------------------------------------

#[tokio::test]
async fn refresh_rejected_waits_out_a_failed_refresh() {
    let fake = Fake::start().await;
    let dir = tempfile::tempdir().unwrap();
    let session = fake.signed_in(dir.path(), MemoryStore::new()).await;
    let rejected = session.bearer().await.unwrap();
    fake.knobs().refresh_error = Some((503, json!({ "error": "temporarily_unavailable" })));
    // Inside the margin the refresh fails; AT-1, still valid, is used.
    fake.clock.advance(56 * MINUTE);
    assert_eq!(bearer(&session).await, "AT-1");
    assert_eq!(fake.refresh_requests().len(), 1);
    // Then the API refuses AT-1 (a 401). Inside the wait that follows a
    // failed refresh, asking again makes no new request.
    let err = session.refresh_rejected(&rejected).await.unwrap_err();
    assert!(matches!(err, BearerError::Unavailable { .. }), "{err:?}");
    assert_eq!(fake.refresh_requests().len(), 1);
}

#[tokio::test]
async fn a_refresh_that_drops_the_plan_scope_hands_out_no_bearer() {
    let fake = Fake::start().await;
    let dir = tempfile::tempdir().unwrap();
    let session = fake.signed_in(dir.path(), MemoryStore::new()).await;
    fake.knobs().scope = Some("openid profile email offline_access resource.invoke".into());
    fake.clock.advance(56 * MINUTE);
    assert_eq!(
        session.bearer().await.unwrap_err(),
        BearerError::PlanDisabled
    );
    assert_eq!(session.status().state, SessionState::PlanDisabled);
    assert_eq!(fake.refresh_requests().len(), 1);
}

#[tokio::test]
async fn an_unknown_signing_key_on_a_refreshed_id_token_is_temporary() {
    // A key OpenAI has just rotated in is not a bad token: the session keeps
    // the rotated refresh token and tries again later.
    let fake = Fake::start().await;
    {
        let mut knobs = fake.knobs();
        knobs.refresh_id_token = true;
        knobs.refresh_kid = Some("k2".into());
    }
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::new();
    let session = fake.signed_in(dir.path(), store.clone()).await;
    fake.clock.advance(56 * MINUTE);
    assert_eq!(
        bearer(&session).await,
        "AT-1",
        "the token in hand, still valid"
    );
    assert_eq!(
        store.get().as_deref(),
        Some("RT-2"),
        "the rotated token is kept"
    );
    assert_eq!(session.status().state, SessionState::SignedIn);

    fake.knobs().refresh_kid = None;
    fake.clock.advance(MINUTE);
    assert_eq!(bearer(&session).await, "AT-3");
    assert_eq!(fake.refresh_requests()[1]["refresh_token"], "RT-2");
}

#[tokio::test]
async fn the_wait_between_failed_refreshes_is_capped_at_five_minutes() {
    let fake = Fake::start().await;
    let dir = tempfile::tempdir().unwrap();
    let session = fake.signed_in(dir.path(), MemoryStore::new()).await;
    fake.knobs().refresh_error = Some((503, json!({ "error": "temporarily_unavailable" })));
    fake.clock.advance(61 * MINUTE);
    for attempt in 1..=8 {
        assert!(session.bearer().await.is_err());
        assert_eq!(
            fake.refresh_requests().len(),
            attempt,
            "attempt {attempt}: five minutes is the longest wait"
        );
        fake.clock.advance(5 * MINUTE + Duration::from_secs(1));
    }
}

// --- Second review (#605) ------------------------------------------------------------

#[tokio::test]
async fn signing_out_while_the_keychain_cannot_be_read_does_not_claim_a_clean_sign_out() {
    // A window that cannot read the keychain cannot know what to revoke. It
    // says so, keeps the token where it is for a later sign-out to revoke,
    // and still marks the account signed out so no window uses it.
    let fake = Fake::start().await;
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::new();
    drop(fake.signed_in(dir.path(), store.clone()).await);
    let restarted = fake.session(dir.path(), store.clone());
    *store.load_error.lock().unwrap() = Some(SecretStoreError::Unavailable("locked".into()));

    let report = restarted.sign_out().await;
    match &report.revocation {
        Revocation::NotConfirmed { reason } => {
            assert!(reason.contains("keychain could not be read"), "{reason}")
        }
        other => panic!("{other:?}"),
    }
    assert!(report.removed_from_store.is_err(), "{report:?}");
    assert_eq!(
        store.get().as_deref(),
        Some("RT-1"),
        "left for a later sign-out"
    );
    assert_eq!(
        registration_json(dir.path())["clients"][0]["signed_out"],
        true
    );
    assert_eq!(restarted.status().state, SessionState::SignedOut);

    // Once the keychain answers, signing out again revokes it.
    *store.load_error.lock().unwrap() = None;
    let again = restarted.sign_out().await;
    assert_eq!(again.revocation, Revocation::Confirmed);
    assert_eq!(fake.revocations.lock().unwrap()[0]["token"], "RT-1");
    assert_eq!(store.get(), None);
}

#[tokio::test]
async fn another_account_while_signed_in_revokes_and_signs_out_the_first() {
    // "Never overwrite another account registration's credentials": the
    // keychain slot is single, so the first account's refresh token is
    // revoked and its record marked signed out, not silently overwritten.
    let fake = Fake::start().await;
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::new();
    let session = fake.signed_in(dir.path(), store.clone()).await;
    fake.knobs().subject = "a-second-account".into();
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
    assert_eq!(store.get().as_deref(), Some("RT-2"));
    let revoked = fake.revocations.lock().unwrap().clone();
    assert_eq!(revoked.len(), 1, "{revoked:?}");
    assert_eq!(revoked[0]["token"], "RT-1");
    assert_eq!(revoked[0]["client_id"], ISSUED_CLIENT);
    let reg = registration_json(dir.path());
    assert_eq!(reg["active_client_id"], "oaiapp_second");
    assert_eq!(reg["clients"][0]["signed_out"], true, "the first account");
    assert_eq!(reg["clients"][1]["signed_out"], false);
}

#[tokio::test]
async fn a_window_restored_after_a_sign_out_elsewhere_is_signed_out() {
    // B was opened, but not restored, before A signed out: B's restore reads
    // the registration as it is now.
    let fake = Fake::start().await;
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::new();
    let a = fake.signed_in(dir.path(), store.clone()).await;
    let b = fake.session(dir.path(), store.clone());
    assert_eq!(b.status().state, SessionState::NotLoaded);
    a.sign_out().await;
    assert_eq!(b.restore().await.unwrap().state, SessionState::SignedOut);
    assert_eq!(b.bearer().await.unwrap_err(), BearerError::NotSignedIn);
}

#[tokio::test]
async fn a_window_restored_after_a_sign_out_and_in_elsewhere_keeps_the_newer_session() {
    // The review's restore leg: B opened, not restored; A signs out and back
    // in; B's first bearer must not empty the store.
    let fake = Fake::start().await;
    fake.knobs().refresh_id_token = true;
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::new();
    let a = fake.signed_in(dir.path(), store.clone()).await;
    let b = fake.session(dir.path(), store.clone());
    fake.clock.advance(10 * MINUTE);
    a.sign_out().await;
    fake.sign_in(&a, Browser::default(), SignInIntent::SignIn)
        .await
        .unwrap();
    assert_eq!(bearer(&b).await, "AT-3");
    assert_eq!(store.get().as_deref(), Some("RT-3"));
    assert_eq!(b.status().state, SessionState::SignedIn);
}

#[tokio::test]
async fn a_window_whose_own_token_outlived_a_sign_out_does_not_use_it() {
    // A signs out, but the revocation is not confirmed and the keychain
    // delete did not take: the stored token is still B's own. The
    // registration says signed out, and B checks it on every refresh.
    let fake = Fake::start().await;
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::new();
    let a = fake.signed_in(dir.path(), store.clone()).await;
    let b = fake.session(dir.path(), store.clone());
    b.restore().await.unwrap();
    fake.knobs().revoke_statuses = vec![500; 4];
    let report = a.sign_out().await;
    assert!(matches!(report.revocation, Revocation::NotConfirmed { .. }));
    store.set(Some("RT-1"));
    fake.clock.advance(61 * MINUTE);
    assert_eq!(
        b.bearer().await.unwrap_err(),
        BearerError::SessionEnded(EndReason::SignedOutElsewhere)
    );
    assert!(fake.refresh_requests().is_empty(), "B sent nothing");
}

#[tokio::test]
async fn a_sign_out_whose_caller_stops_waiting_still_revokes() {
    // Dropped during the wait between two revocation attempts, the sign-out
    // still makes its next attempt, and the tokens are cleared either way.
    let fake = Fake::start().await;
    fake.knobs().revoke_statuses = vec![503];
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::new();
    let config = fake
        .config(dir.path())
        .with_revoke_retry_delays(vec![Duration::from_millis(400); 3]);
    let session = ChatGptSession::open(config, store.clone()).unwrap();
    fake.sign_in(&session, Browser::default(), SignInIntent::SignIn)
        .await
        .unwrap();
    let cut_short = tokio::time::timeout(Duration::from_millis(150), session.sign_out()).await;
    assert!(cut_short.is_err(), "the caller gave up first");
    for _ in 0..100 {
        if fake.revocations.lock().unwrap().len() == 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(
        fake.revocations.lock().unwrap().len(),
        2,
        "the retry still ran"
    );
    assert_eq!(store.get(), None);
    assert_eq!(session.status().state, SessionState::SignedOut);
    assert_eq!(
        registration_json(dir.path())["clients"][0]["signed_out"],
        true
    );
}

#[tokio::test]
async fn a_slow_revocation_holds_up_no_other_window() {
    // Sign-out clears and marks under the lock, then revokes after letting
    // go of it: another window's refresh is not kept waiting behind the
    // revocation's retries.
    let fake = Fake::start().await;
    fake.knobs().revoke_statuses = vec![503; 4];
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::new();
    let config = fake
        .config(dir.path())
        .with_revoke_retry_delays(vec![Duration::from_millis(500); 3]);
    let a = Arc::new(ChatGptSession::open(config, store.clone()).unwrap());
    fake.sign_in(&a, Browser::default(), SignInIntent::SignIn)
        .await
        .unwrap();
    let b = fake.session(dir.path(), store.clone());
    b.restore().await.unwrap();
    fake.clock.advance(61 * MINUTE);
    let signing_out = tokio::spawn({
        let a = a.clone();
        async move { a.sign_out().await }
    });
    while fake.revocations.lock().unwrap().is_empty() {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let from_b = tokio::time::timeout(Duration::from_millis(300), b.bearer())
        .await
        .expect("B is not held up by A's revocation retries");
    assert_eq!(
        from_b.unwrap_err(),
        BearerError::SessionEnded(EndReason::SignedOutElsewhere)
    );
    let report = signing_out.await.unwrap();
    assert!(matches!(report.revocation, Revocation::NotConfirmed { .. }));
}

#[tokio::test]
async fn dropping_the_session_mid_refresh_still_saves_the_rotated_token() {
    // The review's probe: the session itself is dropped while its refresh
    // is in flight; the refresh finishes on its own task.
    let fake = Fake::start().await;
    *fake.on_refresh.lock().unwrap() =
        Some(Box::new(|_| std::thread::sleep(Duration::from_millis(500))));
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::new();
    let session = fake.signed_in(dir.path(), store.clone()).await;
    fake.clock.advance(61 * MINUTE);
    let _ = tokio::time::timeout(Duration::from_millis(100), session.bearer()).await;
    drop(session);
    for _ in 0..100 {
        if store.get().as_deref() == Some("RT-2") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(store.get().as_deref(), Some("RT-2"));
    *fake.on_refresh.lock().unwrap() = None;
    let reopened = fake.session(dir.path(), store.clone());
    assert_eq!(bearer(&reopened).await, "AT-3");
    let sent: Vec<String> = fake
        .refresh_requests()
        .iter()
        .map(|f| f["refresh_token"].clone())
        .collect();
    assert_eq!(sent, ["RT-1", "RT-2"]);
}

#[tokio::test]
async fn signing_out_while_a_dropped_refresh_is_in_flight_revokes_the_rotated_token() {
    // The review's probe: sign-out waits for the refresh in flight, then
    // revokes the token that refresh saved.
    let fake = Fake::start().await;
    *fake.on_refresh.lock().unwrap() =
        Some(Box::new(|_| std::thread::sleep(Duration::from_millis(500))));
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::new();
    let session = fake.signed_in(dir.path(), store.clone()).await;
    fake.clock.advance(61 * MINUTE);
    let _ = tokio::time::timeout(Duration::from_millis(100), session.bearer()).await;
    let report = session.sign_out().await;
    assert_eq!(report.revocation, Revocation::Confirmed);
    let revoked: Vec<String> = fake
        .revocations
        .lock()
        .unwrap()
        .iter()
        .map(|f| f["token"].clone())
        .collect();
    assert_eq!(revoked, ["RT-2"]);
    assert_eq!(store.get(), None);
    assert_eq!(session.status().state, SessionState::SignedOut);
}

#[tokio::test]
async fn a_keychain_slot_emptied_elsewhere_ends_the_session_without_a_refresh() {
    // The stored token is gone while the registration still says signed in
    // (the keychain was cleared, or a sign-out could not record itself).
    // The window does not fall back to the copy in its memory.
    let fake = Fake::start().await;
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::new();
    let session = fake.signed_in(dir.path(), store.clone()).await;
    store.set(None);
    fake.clock.advance(61 * MINUTE);
    assert_eq!(
        session.bearer().await.unwrap_err(),
        BearerError::SessionEnded(EndReason::SignedOutElsewhere)
    );
    assert!(fake.refresh_requests().is_empty());
}
