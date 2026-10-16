//! [`ChatGptSession`]: signing in, the bearer for each request, and signing
//! out (spec §3.4 to §3.6, §4.3 item 4 and §4.9).
//!
//! **Sign-in** ([`ChatGptSession::sign_in`]) follows OpenAI's open-source
//! flow. The host ID is saved before the first attempt. The listener binds
//! `127.0.0.1` before the browser opens, and the redirect URI is the port it
//! got, so a quick retry on another port works. A first registration sends
//! `dynamic_agent_client`, and its callback must hand back the issued client
//! id, which is written to `registration.json` before the code is exchanged.
//! A returning sign-in uses that client, and a callback naming another is
//! refused. The callback's `iss` is checked (RFC 9207) when the issuer
//! promises one. The ID token is checked against the JWKS with `aud` the
//! issued client id and this attempt's nonce, and on a returning sign-in its
//! `sub` must be the saved account's. Plan usage needs the token response to
//! grant `chatgpt.tokens.use.direct`; without it the session is signed in
//! with plan usage disabled. The active session is untouched until all of
//! that holds.
//!
//! **The bearer** ([`ChatGptSession::bearer`]) is the in-memory access
//! token while it has more than [`spec::REFRESH_MARGIN`] (five minutes) to
//! live. Inside that margin it is refreshed, unless `earliest_refresh_at` has
//! not come yet, in which case the token, still valid, is used as it is. An
//! expired token is refreshed whatever `earliest_refresh_at` says, since
//! there is nothing else to send, and so is one the API rejected
//! ([`ChatGptSession::refresh_rejected`], for a 401).
//!
//! **Refresh** is single-flight. In this process, callers queue on one
//! `tokio::sync::Mutex` and the ones behind the first find a fresh token and
//! make no request. Across processes (edytlab is not a single-instance app),
//! the refresh holds an OS lock on `refresh.lock` in the session's directory
//! and re-reads the refresh token from its store under it, so a second
//! process refreshes with the token the first one saved instead of the one
//! the first just used up. The rotated refresh token is saved before the new
//! access token is handed to anyone. A failure is classified
//! ([`spec::classify_refresh_error`]):
//!
//! - an unusable refresh token ends the session. The stored token is removed
//!   only if it is still the one that failed, never one another process saved
//!   since. A `refresh_token_reused` while the store holds a newer token (a
//!   process that did not take the lock refreshed first) is retried once with
//!   that token;
//! - anything temporary (the network, a 5xx, a 429) keeps every credential,
//!   and the session waits before trying again (15 s, doubling to 5 min),
//!   using the current access token meanwhile if it is still valid;
//! - any other refusal (`invalid_client` and the like) also keeps the
//!   credentials and waits, and is reported as it is.
//!
//! **Sign-out** ([`ChatGptSession::sign_out`]) revokes the refresh token at
//! the discovery document's `revocation_endpoint` (which must be on the
//! issuer's origin), retrying a network failure or a 5xx with backoff. It
//! then clears the tokens whatever happened, keeps the registration and the
//! host ID, and reports whether the revocation was confirmed.
//!
//! **Secrets.** Tokens are [`SecretString`]s. None is written to
//! `registration.json`, put in a URL, logged, or shown by `Debug`; the
//! email address is left out of `Debug` too.

use std::fmt;
use std::fs::{File, OpenOptions};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex, MutexGuard as StdMutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use fs4::{FileExt, TryLockError};
use serde_json::{Map, Value};
use tokio::sync::Mutex;

use crate::oauth::loopback::{
    CancelHandle, LoopbackConfig, LoopbackError, LoopbackHost, LoopbackListener, DEFAULT_TIMEOUT,
};
use crate::oauth::oidc::{IdTokenClaims, IdTokenExpectations, OidcClient, OidcError};
use crate::oauth::pkce::{constant_time_eq, Nonce, PkceVerifier, RandomnessError, State};
use crate::oauth::token::{TokenClient, TokenError, TokenResponse, TokenTypeHint};
use crate::oauth::{AuthorizationUrlError, SecretString};

use super::spec::{self, AttemptKind, AuthorizeAttempt, CallbackError, HostId, RefreshFailure};
use super::store::{
    self, ensure_private_dir, ClientRecord, IdTokenReference, RefreshTokenStore, Registration,
    RegistrationFile, SecretStoreError, StoreError,
};
use super::Endpoints;

/// How long a refresh waits for another process's refresh to finish.
pub const DEFAULT_LOCK_TIMEOUT: Duration = Duration::from_secs(60);
/// The waits between revocation attempts: three retries in all.
pub const DEFAULT_REVOKE_RETRY_DELAYS: [Duration; 3] = [
    Duration::from_secs(1),
    Duration::from_secs(2),
    Duration::from_secs(4),
];
/// The first wait after a refresh that failed without ending the session.
const RETRY_BASE: Duration = Duration::from_secs(15);
/// The longest wait between refresh attempts.
const RETRY_MAX: Duration = Duration::from_secs(5 * 60);
/// How often a waiting refresh tries the cross-process lock again.
const LOCK_POLL: Duration = Duration::from_millis(25);

/// The time, for token expiry and `earliest_refresh_at`. Tests use a clock
/// they can move.
pub trait Clock: Send + Sync + fmt::Debug {
    fn now(&self) -> SystemTime;
}

/// The system's wall clock.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> SystemTime {
        SystemTime::now()
    }
}

/// Where the session keeps its files, which issuer it talks to, and its
/// timeouts.
#[derive(Clone, Debug)]
pub struct SessionConfig {
    endpoints: Endpoints,
    dir: std::path::PathBuf,
    clock: Arc<dyn Clock>,
    preferred_port: Option<u16>,
    sign_in_timeout: Duration,
    lock_timeout: Duration,
    revoke_retry_delays: Vec<Duration>,
}

impl SessionConfig {
    /// OpenAI's production issuer, with the session's files in
    /// `<app_config_dir>/chatgpt`.
    pub fn new(app_config_dir: &std::path::Path) -> Self {
        Self {
            endpoints: Endpoints::production(),
            dir: app_config_dir.join(store::DIR_NAME),
            clock: Arc::new(SystemClock),
            preferred_port: Some(spec::PREFERRED_CALLBACK_PORT),
            sign_in_timeout: DEFAULT_TIMEOUT,
            lock_timeout: DEFAULT_LOCK_TIMEOUT,
            revoke_retry_delays: DEFAULT_REVOKE_RETRY_DELAYS.to_vec(),
        }
    }

    /// Another issuer. Outside this crate's tests the only [`Endpoints`] is
    /// [`Endpoints::production`], so this cannot point the app anywhere
    /// else.
    pub fn with_endpoints(mut self, endpoints: Endpoints) -> Self {
        self.endpoints = endpoints;
        self
    }

    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }

    /// The callback port to try first (`None`: any free port).
    pub fn with_preferred_port(mut self, port: Option<u16>) -> Self {
        self.preferred_port = port;
        self
    }

    /// How long a sign-in waits for the browser.
    pub fn with_sign_in_timeout(mut self, timeout: Duration) -> Self {
        self.sign_in_timeout = timeout;
        self
    }

    /// How long a refresh waits for another process's.
    pub fn with_lock_timeout(mut self, timeout: Duration) -> Self {
        self.lock_timeout = timeout;
        self
    }

    /// The waits between revocation attempts on a network failure or a 5xx.
    pub fn with_revoke_retry_delays(mut self, delays: Vec<Duration>) -> Self {
        self.revoke_retry_delays = delays;
        self
    }

    /// The session's directory: `registration.json` and `refresh.lock`.
    pub fn dir(&self) -> &std::path::Path {
        &self.dir
    }

    pub fn endpoints(&self) -> &Endpoints {
        &self.endpoints
    }
}

/// What the user asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignInIntent {
    /// Continue with ChatGPT: a first registration or a returning sign-in.
    SignIn,
    /// Turn plan usage back on after declining it: the full scopes again,
    /// with `prompt=consent`.
    EnablePlanUsage,
}

/// Where the session stands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionState {
    /// No account, or the user signed out of it.
    SignedOut,
    /// A saved session whose refresh token has not been read yet:
    /// [`ChatGptSession::restore`], or the first bearer, reads it.
    NotLoaded,
    /// Signed in, with plan usage granted.
    SignedIn,
    /// Signed in, but plan usage was not granted. "Retain the sign-in but
    /// mark ChatGPT plan usage as disabled"; no inference is made.
    PlanDisabled,
    /// The session ended without a sign-out. A sign-in reuses the
    /// registration, so it shows no consent screen.
    SessionEnded(EndReason),
}

/// Why a session ended.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum EndReason {
    /// The token endpoint said the refresh token cannot be used again.
    #[error("the refresh token was refused ({code})")]
    RefreshTokenUnusable { code: String },
    /// A refresh response's ID token did not verify against the original.
    #[error("a refreshed ID token was not valid: {reason}")]
    IdTokenRejected { reason: String },
    /// No refresh token was issued, and the access token expired.
    #[error("no refresh token was issued and the access token expired")]
    NoRefreshToken,
    /// At launch the store held no refresh token: on Linux with no Secret
    /// Service after a reboot, or after a session kept in memory only.
    #[error("the refresh token was not saved in the keychain")]
    NotStored,
    /// The stored refresh token disappeared while the session ran: another
    /// edytlab process signed out.
    #[error("another edytlab window signed out")]
    SignedOutElsewhere,
}

/// Where the refresh token is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TokenStorage {
    /// In the refresh-token store (the keychain).
    Stored,
    /// In memory only, because the store refused it; the session ends when
    /// the app quits and the user signs in again next launch.
    MemoryOnly(SecretStoreError),
}

/// The session as the UI shows it. Holds no token.
#[derive(Clone, PartialEq, Eq)]
pub struct ChatGptStatus {
    pub state: SessionState,
    /// The account's email, its label. Personal data: not in `Debug`.
    pub email: Option<String>,
    /// Where the refresh token is, when there is one.
    pub token_storage: Option<TokenStorage>,
}

impl fmt::Debug for ChatGptStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChatGptStatus")
            .field("state", &self.state)
            .field("email", &self.email.as_ref().map(|_| "<redacted>"))
            .field("token_storage", &self.token_storage)
            .finish()
    }
}

/// Why there is no bearer.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum BearerError {
    #[error("not signed in to ChatGPT")]
    NotSignedIn,
    #[error("signed in to ChatGPT, but this app is not allowed to use your ChatGPT plan")]
    PlanDisabled,
    #[error("your ChatGPT session ended: {0}")]
    SessionEnded(EndReason),
    /// Temporary. Every credential is kept; the session tries again later.
    #[error("ChatGPT sign-in is unavailable for now: {reason}")]
    Unavailable { reason: String },
    /// The token endpoint refused the refresh for a reason that does not
    /// make the token unusable (`invalid_client`, for one). The credentials
    /// are kept. `status` is 0 when no request was made.
    #[error("OpenAI refused to renew the ChatGPT session (HTTP {status}, {})", .code.as_deref().unwrap_or("no code"))]
    Refused { status: u16, code: Option<String> },
    /// The saved session's refresh token could not be read.
    #[error(transparent)]
    Store(SecretStoreError),
}

/// Why a sign-in did not finish. The active session, if any, is unchanged.
#[derive(Debug, thiserror::Error)]
pub enum SignInError {
    #[error("a ChatGPT sign-in is already in progress")]
    AlreadyInProgress,
    #[error("could not take the ChatGPT session lock: {0}")]
    Lock(String),
    #[error(transparent)]
    Registration(#[from] StoreError),
    #[error(transparent)]
    Randomness(#[from] RandomnessError),
    #[error("could not read OpenAI's sign-in configuration: {0}")]
    Discovery(#[source] OidcError),
    #[error(transparent)]
    AuthorizationUrl(#[from] AuthorizationUrlError),
    /// Cancelled, timed out, access denied, another issuer's response, or
    /// the browser could not be opened.
    #[error(transparent)]
    Loopback(#[from] LoopbackError),
    #[error(transparent)]
    Callback(#[from] CallbackError),
    /// "If code exchange returns invalid_grant, discard that code and start
    /// a fresh authorization."
    #[error("the sign-in code was refused; start a new sign-in")]
    CodeRejected,
    #[error("could not finish signing in: {0}")]
    Exchange(#[source] TokenError),
    #[error("the sign-in returned no ID token")]
    NoIdToken,
    #[error("the sign-in's ID token is not valid: {0}")]
    IdToken(#[source] OidcError),
    /// A returning sign-in verified as another account than the one this
    /// client is registered for.
    #[error("you signed in to a different ChatGPT account than the one saved for this app")]
    AccountChanged,
}

/// Why the session could not be opened.
#[derive(Debug, thiserror::Error)]
pub enum OpenError {
    #[error(transparent)]
    Issuer(#[from] OidcError),
    #[error(transparent)]
    Registration(#[from] StoreError),
}

/// Whether the refresh token was revoked at OpenAI.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Revocation {
    /// The revocation endpoint answered 2xx (an empty 200, "including for
    /// an already-invalid token").
    Confirmed,
    /// Not confirmed: the user can disconnect the app in ChatGPT Settings →
    /// Security and login → Sign in with ChatGPT.
    NotConfirmed { reason: String },
    /// There was no refresh token to revoke.
    NothingToRevoke,
}

/// What a sign-out did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignOutReport {
    pub revocation: Revocation,
    /// Whether the stored refresh token was removed. The in-memory tokens are
    /// cleared either way.
    pub removed_from_store: Result<(), SecretStoreError>,
}

/// A ChatGPT plan session. See the module documentation.
pub struct ChatGptSession {
    config: SessionConfig,
    oidc: OidcClient,
    file: RegistrationFile,
    secrets: Arc<dyn RefreshTokenStore>,
    inner: StdMutex<Inner>,
    /// Single-flight: held for every refresh, and around every write of
    /// tokens or of the registration. Taken before the file lock, always.
    flight: Mutex<()>,
    signing_in: AtomicBool,
    cancel: StdMutex<Cancel>,
}

/// The sign-in in progress's cancel handle, once its listener is bound, and
/// whether a cancel came before that.
#[derive(Default)]
struct Cancel {
    handle: Option<CancelHandle>,
    requested: bool,
}

impl fmt::Debug for ChatGptSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChatGptSession")
            .field("issuer", &self.config.endpoints.issuer())
            .field("status", &self.status())
            .finish_non_exhaustive()
    }
}

struct Inner {
    registration: Option<Registration>,
    phase: Phase,
}

enum Phase {
    SignedOut,
    NotLoaded,
    Active(Box<Active>),
    Ended(EndReason),
}

struct Active {
    client_id: String,
    /// The claims a refreshed ID token is compared with.
    original: IdTokenClaims,
    scopes: Vec<String>,
    access: Option<AccessToken>,
    /// The refresh token, also in memory: the only copy when the store
    /// refused it, and the fallback when the store cannot be read.
    refresh_token: Option<SecretString>,
    storage: Option<TokenStorage>,
    retry: Option<Retry>,
}

struct AccessToken {
    token: SecretString,
    expires_at: SystemTime,
    earliest_refresh_at: Option<SystemTime>,
}

/// A refresh that failed without ending the session, and when to try
/// again.
struct Retry {
    not_before: SystemTime,
    failures: u32,
    error: BearerError,
}

/// The OS lock on `refresh.lock`. Released on drop.
struct FileLock(File);

impl Drop for FileLock {
    fn drop(&mut self) {
        // Closing releases it too; unlocking first makes it immediate.
        let _ = FileExt::unlock(&self.0);
    }
}

/// Clears the in-progress flag and the cancel handle when a sign-in ends.
struct Attempt<'a> {
    session: &'a ChatGptSession,
}

impl Drop for Attempt<'_> {
    fn drop(&mut self) {
        *lock(&self.session.cancel) = Cancel::default();
        self.session.signing_in.store(false, Ordering::Release);
    }
}

fn lock<T>(m: &StdMutex<T>) -> StdMutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn unix_secs(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// An ID-token check that failed for a reason that may pass on a later
/// try: the JWKS could not be fetched, or names no key for the token yet.
fn oidc_error_is_transient(e: &OidcError) -> bool {
    match e {
        OidcError::Network(_) | OidcError::Client(_) | OidcError::TooLarge(_) => true,
        OidcError::Status { status, .. } => *status >= 500 || *status == 429,
        OidcError::UnknownKey => true,
        _ => false,
    }
}

impl ChatGptSession {
    /// Opens the session: reads `registration.json`, and nothing else. A
    /// saved session is [`SessionState::NotLoaded`] until [`Self::restore`]
    /// (or the first bearer) reads its refresh token, since that is a
    /// keychain read and can prompt. A `registration.json` that does not
    /// parse is set aside: the next sign-in registers afresh and overwrites
    /// it.
    pub fn open(
        config: SessionConfig,
        secrets: Arc<dyn RefreshTokenStore>,
    ) -> Result<Self, OpenError> {
        let oidc = OidcClient::new(config.endpoints.issuer())?;
        let file = RegistrationFile::new(config.dir.clone());
        let registration = read_registration(&file)?;
        let phase = match registration
            .as_ref()
            .and_then(|r| r.active(config.endpoints.issuer()))
        {
            Some(c) if c.subject.is_some() && !c.signed_out => Phase::NotLoaded,
            _ => Phase::SignedOut,
        };
        Ok(Self {
            config,
            oidc,
            file,
            secrets,
            inner: StdMutex::new(Inner {
                registration,
                phase,
            }),
            flight: Mutex::new(()),
            signing_in: AtomicBool::new(false),
            cancel: StdMutex::new(Cancel::default()),
        })
    }

    /// The session as it stands, without any I/O.
    pub fn status(&self) -> ChatGptStatus {
        let inner = lock(&self.inner);
        let email = inner
            .registration
            .as_ref()
            .and_then(|r| r.active(self.config.endpoints.issuer()))
            .and_then(|c| c.email.clone());
        let (state, token_storage) = match &inner.phase {
            Phase::SignedOut => (SessionState::SignedOut, None),
            Phase::NotLoaded => (SessionState::NotLoaded, None),
            Phase::Ended(reason) => (SessionState::SessionEnded(reason.clone()), None),
            Phase::Active(a) => (
                if spec::grants_plan_usage(&a.scopes) {
                    SessionState::SignedIn
                } else {
                    SessionState::PlanDisabled
                },
                a.storage.clone(),
            ),
        };
        ChatGptStatus {
            state,
            email,
            token_storage,
        }
    }

    /// Whether a sign-in is waiting for the browser.
    pub fn is_signing_in(&self) -> bool {
        self.signing_in.load(Ordering::Acquire)
    }

    /// Reads a saved session's refresh token from its store. Nothing to do
    /// unless the state is [`SessionState::NotLoaded`]. With no token
    /// stored, the session has ended ([`EndReason::NotStored`]); a store that
    /// cannot be read leaves it as it was, and returns the error.
    pub async fn restore(&self) -> Result<ChatGptStatus, SecretStoreError> {
        let _flight = self.flight.lock().await;
        self.restore_locked().await?;
        Ok(self.status())
    }

    async fn restore_locked(&self) -> Result<(), SecretStoreError> {
        let record = {
            let inner = lock(&self.inner);
            if !matches!(inner.phase, Phase::NotLoaded) {
                return Ok(());
            }
            inner
                .registration
                .as_ref()
                .and_then(|r| r.active(self.config.endpoints.issuer()))
                .cloned()
        };
        let Some(record) = record.filter(|r| r.subject.is_some()) else {
            lock(&self.inner).phase = Phase::SignedOut;
            return Ok(());
        };
        let stored = self.load_secret().await?;
        let mut inner = lock(&self.inner);
        if !matches!(inner.phase, Phase::NotLoaded) {
            return Ok(());
        }
        inner.phase = match stored {
            Some(token) => Phase::Active(Box::new(Active {
                original: original_claims(&record),
                client_id: record.client_id.clone(),
                scopes: record.scopes.clone(),
                access: None,
                refresh_token: Some(token),
                storage: Some(TokenStorage::Stored),
                retry: None,
            })),
            None => Phase::Ended(EndReason::NotStored),
        };
        Ok(())
    }

    /// The access token to send as `Authorization: Bearer`, refreshed first
    /// when it is about to expire. See the module documentation.
    pub async fn bearer(&self) -> Result<SecretString, BearerError> {
        if let Some(answer) = self.current_bearer() {
            return answer;
        }
        let _flight = self.flight.lock().await;
        self.restore_locked().await.map_err(BearerError::Store)?;
        // Whoever held the lock before may have refreshed already.
        if let Some(answer) = self.current_bearer() {
            return answer;
        }
        match self.refresh_locked().await {
            // A refresh that failed for now: the token in hand still works
            // until it expires.
            Err(
                e @ (BearerError::Unavailable { .. }
                | BearerError::Refused { .. }
                | BearerError::Store(_)),
            ) => self.unexpired_access_token().ok_or(e),
            other => other,
        }
    }

    /// The in-memory access token, if it has not expired.
    fn unexpired_access_token(&self) -> Option<SecretString> {
        let now = self.config.clock.now();
        match &lock(&self.inner).phase {
            Phase::Active(a) => a
                .access
                .as_ref()
                .filter(|t| now < t.expires_at)
                .map(|t| t.token.clone()),
            _ => None,
        }
    }

    /// A bearer to replace `rejected`, which the API refused (a 401):
    /// refreshed now, unless another caller already replaced it. Honours the
    /// wait after a failed refresh, not `earliest_refresh_at`.
    pub async fn refresh_rejected(
        &self,
        rejected: &SecretString,
    ) -> Result<SecretString, BearerError> {
        let _flight = self.flight.lock().await;
        self.restore_locked().await.map_err(BearerError::Store)?;
        let now = self.config.clock.now();
        {
            let inner = lock(&self.inner);
            if let Phase::Active(a) = &inner.phase {
                if let Some(t) = &a.access {
                    if now < t.expires_at
                        && !constant_time_eq(t.token.expose_secret(), rejected.expose_secret())
                    {
                        return Ok(t.token.clone());
                    }
                }
                if let Some(r) = a.retry.as_ref().filter(|r| now < r.not_before) {
                    return Err(r.error.clone());
                }
            }
        }
        match self.current_bearer() {
            Some(Err(e)) => Err(e),
            _ => self.refresh_locked().await,
        }
    }

    /// The answer the in-memory state gives, or `None` when a refresh (or
    /// a restore) is due.
    fn current_bearer(&self) -> Option<Result<SecretString, BearerError>> {
        let now = self.config.clock.now();
        let inner = lock(&self.inner);
        let a = match &inner.phase {
            Phase::SignedOut => return Some(Err(BearerError::NotSignedIn)),
            Phase::NotLoaded => return None,
            Phase::Ended(reason) => return Some(Err(BearerError::SessionEnded(reason.clone()))),
            Phase::Active(a) => a,
        };
        if !spec::grants_plan_usage(&a.scopes) {
            return Some(Err(BearerError::PlanDisabled));
        }
        let waiting = a.retry.as_ref().filter(|r| now < r.not_before);
        // With no refresh token there is nothing to refresh with: the access
        // token is used until it expires, and then the session ends.
        let can_refresh = a.refresh_token.is_some() || a.storage == Some(TokenStorage::Stored);
        match a.access.as_ref().filter(|t| now < t.expires_at) {
            Some(t) => {
                let fresh = now + spec::REFRESH_MARGIN < t.expires_at;
                let too_early = t.earliest_refresh_at.is_some_and(|at| now < at);
                (fresh || too_early || waiting.is_some() || !can_refresh)
                    .then(|| Ok(t.token.clone()))
            }
            None => waiting.map(|r| Err(r.error.clone())),
        }
    }

    /// Refreshes, holding `flight`. See the module documentation.
    async fn refresh_locked(&self) -> Result<SecretString, BearerError> {
        let (client_id, original, memory_token, storage) = {
            let inner = lock(&self.inner);
            match &inner.phase {
                Phase::Active(a) => (
                    a.client_id.clone(),
                    a.original.clone(),
                    a.refresh_token.clone(),
                    a.storage.clone(),
                ),
                Phase::SignedOut => return Err(BearerError::NotSignedIn),
                Phase::NotLoaded => return Err(BearerError::NotSignedIn),
                Phase::Ended(r) => return Err(BearerError::SessionEnded(r.clone())),
            }
        };
        let file_lock = match self.lock_file().await {
            Ok(l) => l,
            Err(reason) => return Err(self.failed(BearerError::Unavailable { reason })),
        };
        // The store is the truth when the token lives there: another process
        // may have rotated it since this one last looked.
        let mut sent = match (&storage, memory_token) {
            (Some(TokenStorage::Stored), memory) => match self.load_secret().await {
                Ok(Some(stored)) => stored,
                Ok(None) => {
                    self.end(EndReason::SignedOutElsewhere, None).await;
                    return Err(BearerError::SessionEnded(EndReason::SignedOutElsewhere));
                }
                Err(e) => match memory {
                    Some(memory) => memory,
                    None => return Err(self.failed(BearerError::Store(e))),
                },
            },
            (_, Some(memory)) => memory,
            (_, None) => {
                self.end(EndReason::NoRefreshToken, None).await;
                return Err(BearerError::SessionEnded(EndReason::NoRefreshToken));
            }
        };
        let client =
            match TokenClient::new(self.config.endpoints.token_endpoint().clone(), &client_id) {
                Ok(c) => c,
                Err(e) => {
                    return Err(self.failed(BearerError::Unavailable {
                        reason: e.to_string(),
                    }))
                }
            };
        let mut retried = false;
        loop {
            let error = match client.refresh(&sent, &[("resource", spec::RESOURCE)]).await {
                Ok(response) => {
                    return self
                        .accept_refresh(response, sent, &client_id, &original, file_lock)
                        .await
                }
                Err(e) => e,
            };
            match spec::classify_refresh_error(&error) {
                RefreshFailure::Unusable { code } => {
                    if !retried && code == spec::REFRESH_TOKEN_REUSED {
                        // Raced: someone else refreshed with this token. If the
                        // store holds what they got, use that.
                        if let Ok(Some(newer)) = self.load_secret().await {
                            if !constant_time_eq(newer.expose_secret(), sent.expose_secret()) {
                                tracing::info!(
                                    "chatgpt: refresh token already used; retrying with the newer stored one"
                                );
                                sent = newer;
                                retried = true;
                                continue;
                            }
                        }
                    }
                    let reason = EndReason::RefreshTokenUnusable { code };
                    tracing::info!("chatgpt: session ended: {reason}");
                    self.end(reason.clone(), Some(&sent)).await;
                    return Err(BearerError::SessionEnded(reason));
                }
                RefreshFailure::Transient { reason } => {
                    tracing::warn!("chatgpt: refresh failed, will retry: {reason}");
                    return Err(self.failed(BearerError::Unavailable { reason }));
                }
                RefreshFailure::Refused { status, code } => {
                    tracing::warn!(
                        "chatgpt: refresh refused (HTTP {status}, {})",
                        code.as_deref().unwrap_or("no code")
                    );
                    return Err(self.failed(BearerError::Refused { status, code }));
                }
            }
        }
    }

    /// A successful refresh: the rotated refresh token is saved first, then
    /// the refreshed ID token is checked, then the access token is put in
    /// place and returned.
    async fn accept_refresh(
        &self,
        response: TokenResponse,
        sent: SecretString,
        client_id: &str,
        original: &IdTokenClaims,
        file_lock: FileLock,
    ) -> Result<SecretString, BearerError> {
        let now = self.config.clock.now();
        // 1. The new refresh token, before anything uses the new access
        //    token: the old one is spent now.
        let rotated = response.refresh_token.clone().filter(|t| !t.is_empty());
        let storage = match &rotated {
            Some(token) => Some(match self.save_secret(token).await {
                Ok(()) => TokenStorage::Stored,
                Err(e) => {
                    tracing::warn!(
                        "chatgpt: the renewed refresh token is kept in memory only: {e}"
                    );
                    // The stored one is spent: leave nothing that only fails.
                    let _ = self.delete_secret().await;
                    TokenStorage::MemoryOnly(e)
                }
            }),
            None => None,
        };
        let refresh_token = rotated.unwrap_or(sent);
        {
            let mut inner = lock(&self.inner);
            if let Phase::Active(a) = &mut inner.phase {
                a.refresh_token = Some(refresh_token.clone());
                if storage.is_some() {
                    a.storage = storage;
                }
            }
        }

        // 2. A refreshed ID token is for the same issuer, account and client
        //    as the original (OpenID Connect Core §12.2).
        if let Some(id_token) = response.id_token.as_ref().filter(|t| !t.is_empty()) {
            let checked = self
                .oidc
                .verify_id_token_at(
                    id_token,
                    &IdTokenExpectations::refresh(client_id, original),
                    now,
                )
                .await;
            match checked {
                Ok(_) => {}
                Err(e) if oidc_error_is_transient(&e) => {
                    return Err(self.failed(BearerError::Unavailable {
                        reason: e.to_string(),
                    }));
                }
                Err(e) => {
                    let reason = EndReason::IdTokenRejected {
                        reason: e.to_string(),
                    };
                    tracing::warn!("chatgpt: session ended: {reason}");
                    self.end(reason.clone(), Some(&refresh_token)).await;
                    return Err(BearerError::SessionEnded(reason));
                }
            }
        }

        // 3. The scopes the response grants; one that names none keeps the
        //    grant (no `scope` is sent, so it is unchanged).
        let scopes = spec::granted_scopes(&response);
        let lifetime = response
            .expires_in
            .map(Duration::from_secs)
            .unwrap_or(spec::DEFAULT_ACCESS_TOKEN_LIFETIME);
        let token = response.access_token.clone();
        let changed_scopes = {
            let mut inner = lock(&self.inner);
            let Phase::Active(a) = &mut inner.phase else {
                return Err(BearerError::NotSignedIn);
            };
            a.access = Some(AccessToken {
                token: token.clone(),
                expires_at: now + lifetime,
                earliest_refresh_at: spec::earliest_refresh_at(&response),
            });
            a.retry = None;
            match scopes {
                Some(scopes) if scopes != a.scopes => {
                    a.scopes = scopes.clone();
                    Some(scopes)
                }
                _ => None,
            }
        };
        if let Some(scopes) = changed_scopes {
            // Best effort: the registration's scopes are for the next launch.
            let _ = self.update_registration(|r| {
                if let Some(c) = r.client_mut(client_id) {
                    c.scopes = scopes;
                    c.saved_at = unix_secs(now);
                }
            });
        }
        drop(file_lock);
        match self.current_bearer() {
            Some(Err(BearerError::PlanDisabled)) => Err(BearerError::PlanDisabled),
            _ => Ok(token),
        }
    }

    /// Records a refresh that failed without ending the session, so the next
    /// attempt waits, and returns `error`.
    fn failed(&self, error: BearerError) -> BearerError {
        let now = self.config.clock.now();
        let mut inner = lock(&self.inner);
        if let Phase::Active(a) = &mut inner.phase {
            let failures = a.retry.as_ref().map_or(0, |r| r.failures) + 1;
            let wait = RETRY_BASE
                .saturating_mul(1 << (failures - 1).min(8))
                .min(RETRY_MAX);
            a.retry = Some(Retry {
                not_before: now + wait,
                failures,
                error: error.clone(),
            });
        }
        error
    }

    /// Ends the session, holding the file lock. `spent`, the refresh token
    /// that failed, is removed from the store only if it is still the one
    /// there, so a token another process saved since is never erased.
    async fn end(&self, reason: EndReason, spent: Option<&SecretString>) {
        if let Some(spent) = spent {
            if let Ok(Some(stored)) = self.load_secret().await {
                if constant_time_eq(stored.expose_secret(), spent.expose_secret()) {
                    if let Err(e) = self.delete_secret().await {
                        tracing::warn!("chatgpt: could not remove the unusable refresh token: {e}");
                    }
                }
            }
        }
        lock(&self.inner).phase = Phase::Ended(reason);
    }

    /// Signs in through the system browser, which `open_url` opens at the
    /// authorization URL (the desktop app passes its opener; tests pass a
    /// closure that plays the browser). Waits up to the sign-in timeout, or
    /// until [`Self::cancel_sign_in`]. See the module documentation.
    pub async fn sign_in<F, E>(
        &self,
        open_url: F,
        intent: SignInIntent,
    ) -> Result<ChatGptStatus, SignInError>
    where
        F: FnOnce(&str) -> Result<(), E>,
        E: fmt::Display,
    {
        if self
            .signing_in
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(SignInError::AlreadyInProgress);
        }
        let _attempt = Attempt { session: self };
        let endpoints = &self.config.endpoints;
        let issuer = endpoints.issuer();

        // The host ID, saved before the first sign-in, and the client to use.
        let (host_id, kind, login_hint) = {
            let _flight = self.flight.lock().await;
            let _file = self.lock_file().await.map_err(SignInError::Lock)?;
            let registration = match read_registration(&self.file)? {
                Some(r) => r,
                None => {
                    let r = Registration::new(HostId::generate()?);
                    self.file.save(&r)?;
                    r
                }
            };
            let client = registration.client_for_sign_in(issuer).cloned();
            let host_id = registration.host_id.clone();
            lock(&self.inner).registration = Some(registration);
            let kind = match &client {
                Some(c) => AttemptKind::Reauthorize {
                    client_id: c.client_id.clone(),
                },
                None => AttemptKind::Register,
            };
            let login_hint = client.filter(|c| c.subject.is_some()).and_then(|c| c.email);
            (host_id, kind, login_hint)
        };

        // Discovery: the JWKS for the ID token, and whether `iss` comes back.
        let metadata = self.oidc.metadata().await.map_err(SignInError::Discovery)?;

        let mut loopback = LoopbackConfig::new(LoopbackHost::Ipv4Loopback, spec::CALLBACK_PATH)
            .with_timeout(self.config.sign_in_timeout);
        if let Some(port) = self.config.preferred_port {
            loopback = loopback.with_preferred_port(port);
        }
        let listener = LoopbackListener::bind(loopback).await?;
        {
            // A cancel that came while discovery ran counts: the wait ends
            // before the browser is opened.
            let mut cancel = lock(&self.cancel);
            let handle = listener.cancel_handle();
            if cancel.requested {
                handle.cancel();
            }
            cancel.handle = Some(handle);
        }
        // This attempt's port: a quick retry may not get the last one's.
        let redirect_uri = listener.redirect_uri();
        let verifier = PkceVerifier::generate()?;
        let state = State::generate()?;
        let nonce = Nonce::generate()?;
        let url = spec::authorize_url(
            endpoints,
            &AuthorizeAttempt {
                kind: &kind,
                host_id: &host_id,
                redirect_uri: &redirect_uri,
                state: &state,
                nonce: &nonce,
                code_challenge: &verifier.challenge(),
                login_hint: login_hint.as_deref(),
                consent: intent == SignInIntent::EnablePlanUsage,
            },
        )?;
        let callback = listener.authorize(url.as_str(), open_url, &state).await?;
        callback.check_issuer(
            &metadata.issuer,
            metadata.authorization_response_iss_parameter_supported,
        )?;
        let client_id = spec::issued_client_id(&kind, &callback)?;
        if kind == AttemptKind::Register {
            // Saved before the exchange, so a failed exchange does not
            // register another client next time.
            self.record_issued_client(&client_id).await?;
        }

        let tokens = TokenClient::new(endpoints.token_endpoint().clone(), &client_id)
            .map_err(SignInError::Exchange)?
            .exchange_code(
                &callback.code,
                &verifier,
                &redirect_uri,
                &[("resource", spec::RESOURCE)],
            )
            .await
            .map_err(|e| match e.oauth_error() {
                Some(body) if body.error == "invalid_grant" => SignInError::CodeRejected,
                _ => SignInError::Exchange(e),
            })?;
        let id_token = tokens
            .id_token
            .as_ref()
            .filter(|t| !t.is_empty())
            .ok_or(SignInError::NoIdToken)?;
        let claims = self
            .oidc
            .verify_id_token_at(
                id_token,
                &IdTokenExpectations::sign_in(&client_id, &nonce),
                self.config.clock.now(),
            )
            .await
            .map_err(SignInError::IdToken)?;
        self.finish_sign_in(client_id, claims, tokens).await
    }

    /// Ends the sign-in in progress with [`LoopbackError::Cancelled`]: at
    /// once if it is waiting for the browser, or before the browser opens if
    /// it has not got that far. A sign-in already exchanging its code
    /// finishes. Nothing happens when no sign-in is in progress.
    pub fn cancel_sign_in(&self) {
        let mut cancel = lock(&self.cancel);
        if !self.is_signing_in() {
            return;
        }
        cancel.requested = true;
        if let Some(handle) = cancel.handle.as_ref() {
            handle.cancel();
        }
    }

    /// Adds the client a registration callback issued, unverified, so the
    /// next sign-in uses it even if this one goes no further.
    async fn record_issued_client(&self, client_id: &str) -> Result<(), SignInError> {
        let _flight = self.flight.lock().await;
        let _file = self.lock_file().await.map_err(SignInError::Lock)?;
        let now = unix_secs(self.config.clock.now());
        let issuer = self.config.endpoints.issuer().to_string();
        self.update_registration(|r| {
            if r.client(client_id).is_none() {
                r.clients.push(ClientRecord {
                    client_id: client_id.to_string(),
                    issuer,
                    subject: None,
                    email: None,
                    scopes: Vec::new(),
                    id_token_reference: IdTokenReference::default(),
                    saved_at: now,
                    signed_out: false,
                });
            }
        })?;
        Ok(())
    }

    /// The tokens of a verified sign-in: the account checked, the refresh
    /// token stored, the registration updated, and only then the session
    /// replaced.
    async fn finish_sign_in(
        &self,
        client_id: String,
        claims: IdTokenClaims,
        tokens: TokenResponse,
    ) -> Result<ChatGptStatus, SignInError> {
        let _flight = self.flight.lock().await;
        let _file = self.lock_file().await.map_err(SignInError::Lock)?;
        let now = self.config.clock.now();
        let issuer = self.config.endpoints.issuer().to_string();

        let mut registration = read_registration(&self.file)?
            .or_else(|| lock(&self.inner).registration.clone())
            .ok_or_else(|| SignInError::Registration(StoreError::Corrupt(self.file.path())))?;
        // "For a returning account, confirm the new ID token's verified
        // identity matches the selected account before replacing
        // credentials."
        if let Some(saved) = registration
            .client(&client_id)
            .and_then(|c| c.subject.as_deref())
        {
            if saved != claims.subject {
                return Err(SignInError::AccountChanged);
            }
        }

        // The refresh token first: if the store refuses it, the session
        // runs from memory and says so.
        let refresh_token = tokens.refresh_token.clone().filter(|t| !t.is_empty());
        let storage = match &refresh_token {
            Some(token) => Some(match self.save_secret(token).await {
                Ok(()) => TokenStorage::Stored,
                Err(e) => {
                    tracing::warn!("chatgpt: the refresh token is kept in memory only: {e}");
                    // Leave no older session's token behind it.
                    let _ = self.delete_secret().await;
                    TokenStorage::MemoryOnly(e)
                }
            }),
            None => {
                let _ = self.delete_secret().await;
                None
            }
        };

        let scopes = spec::granted_scopes(&tokens).unwrap_or_default();
        let reference = IdTokenReference {
            azp: claims
                .claims
                .get("azp")
                .and_then(Value::as_str)
                .map(str::to_string),
            auth_time: match claims.claims.get("auth_time") {
                Some(Value::Number(n)) => Some(n.clone()),
                _ => None,
            },
            nonce: claims
                .claims
                .get("nonce")
                .and_then(Value::as_str)
                .map(str::to_string),
        };
        let record = ClientRecord {
            client_id: client_id.clone(),
            issuer,
            subject: Some(claims.subject.clone()),
            email: claims.email.clone(),
            scopes: scopes.clone(),
            id_token_reference: reference,
            saved_at: unix_secs(now),
            signed_out: false,
        };
        match registration.client_mut(&client_id) {
            Some(existing) => *existing = record,
            None => registration.clients.push(record),
        }
        registration.active_client_id = Some(client_id.clone());
        if let Err(e) = self.file.save(&registration) {
            if refresh_token.is_some() && storage == Some(TokenStorage::Stored) {
                let _ = self.delete_secret().await;
            }
            return Err(e.into());
        }

        let lifetime = tokens
            .expires_in
            .map(Duration::from_secs)
            .unwrap_or(spec::DEFAULT_ACCESS_TOKEN_LIFETIME);
        {
            let mut inner = lock(&self.inner);
            inner.registration = Some(registration);
            inner.phase = Phase::Active(Box::new(Active {
                client_id,
                original: claims,
                scopes,
                access: Some(AccessToken {
                    token: tokens.access_token.clone(),
                    expires_at: now + lifetime,
                    earliest_refresh_at: spec::earliest_refresh_at(&tokens),
                }),
                refresh_token,
                storage,
                retry: None,
            }));
        }
        Ok(self.status())
    }

    /// Signs out: revokes the refresh token, then clears the tokens whether
    /// or not the revocation was confirmed. The registration (the issued
    /// client and its account) and the host ID are kept for the next
    /// sign-in. See the module documentation.
    pub async fn sign_out(&self) -> SignOutReport {
        let _flight = self.flight.lock().await;
        // Signing out must not hang on another process: without the lock it
        // goes ahead.
        let _file = self.lock_file().await.ok();
        let (client_id, memory_token, read_store) = {
            let inner = lock(&self.inner);
            match &inner.phase {
                Phase::Active(a) => (
                    Some(a.client_id.clone()),
                    a.refresh_token.clone(),
                    a.storage == Some(TokenStorage::Stored),
                ),
                Phase::NotLoaded => (
                    inner
                        .registration
                        .as_ref()
                        .and_then(|r| r.active(self.config.endpoints.issuer()))
                        .map(|c| c.client_id.clone()),
                    None,
                    true,
                ),
                Phase::SignedOut | Phase::Ended(_) => (None, None, false),
            }
        };
        let token = if read_store {
            self.load_secret().await.ok().flatten().or(memory_token)
        } else {
            memory_token
        };
        let revocation = match (&client_id, &token) {
            (Some(client_id), Some(token)) => self.revoke(client_id, token).await,
            _ => Revocation::NothingToRevoke,
        };
        if let Revocation::NotConfirmed { reason } = &revocation {
            tracing::warn!("chatgpt: sign-out could not confirm the revocation: {reason}");
        }
        let removed_from_store = self.delete_secret().await;
        let issuer = self.config.endpoints.issuer().to_string();
        let now = unix_secs(self.config.clock.now());
        if let Err(e) = self.update_registration(|r| {
            if let Some(id) = r.active_client_id.clone() {
                if let Some(c) = r.client_mut(&id).filter(|c| c.issuer == issuer) {
                    c.signed_out = true;
                    c.saved_at = now;
                }
            }
        }) {
            tracing::warn!("chatgpt: could not record the sign-out: {e}");
        }
        lock(&self.inner).phase = Phase::SignedOut;
        SignOutReport {
            revocation,
            removed_from_store,
        }
    }

    /// Revokes `token`, retrying a network failure or a 5xx.
    async fn revoke(&self, client_id: &str, token: &SecretString) -> Revocation {
        let issuer = self.config.endpoints.issuer();
        let mut last = String::new();
        let waits = std::iter::once(None).chain(self.config.revoke_retry_delays.iter().map(Some));
        for wait in waits {
            if let Some(wait) = wait {
                tokio::time::sleep(*wait).await;
            }
            let metadata = match self.oidc.metadata().await {
                Ok(m) => m,
                Err(e) if oidc_error_is_transient(&e) => {
                    last = e.to_string();
                    continue;
                }
                Err(e) => {
                    return Revocation::NotConfirmed {
                        reason: e.to_string(),
                    }
                }
            };
            let Some(endpoint) = metadata.revocation_endpoint.clone() else {
                return Revocation::NotConfirmed {
                    reason: "OpenAI's sign-in configuration names no revocation endpoint".into(),
                };
            };
            if !spec::same_origin(&endpoint, issuer) {
                return Revocation::NotConfirmed {
                    reason: "the revocation endpoint is not on OpenAI's sign-in server".into(),
                };
            }
            let client =
                match TokenClient::new(self.config.endpoints.token_endpoint().clone(), client_id) {
                    Ok(c) => c,
                    Err(e) => {
                        return Revocation::NotConfirmed {
                            reason: e.to_string(),
                        }
                    }
                };
            match client
                .revoke(&endpoint, token, TokenTypeHint::RefreshToken)
                .await
            {
                Ok(()) => return Revocation::Confirmed,
                Err(e) => {
                    let retry = match &e {
                        TokenError::Network(_) | TokenError::TooLarge(_) => true,
                        TokenError::Status { status } | TokenError::OAuth { status, .. } => {
                            *status >= 500
                        }
                        _ => false,
                    };
                    last = e.to_string();
                    if !retry {
                        break;
                    }
                }
            }
        }
        Revocation::NotConfirmed { reason: last }
    }

    /// Reads, changes and saves `registration.json` (re-read from disk, so
    /// another process's changes are kept), then caches it. The caller holds
    /// the file lock.
    fn update_registration(
        &self,
        change: impl FnOnce(&mut Registration),
    ) -> Result<(), StoreError> {
        let mut registration = match read_registration(&self.file)? {
            Some(r) => r,
            None => match lock(&self.inner).registration.clone() {
                Some(r) => r,
                None => return Ok(()),
            },
        };
        change(&mut registration);
        self.file.save(&registration)?;
        lock(&self.inner).registration = Some(registration);
        Ok(())
    }

    /// The OS lock on `refresh.lock`, waited for up to the lock timeout. The
    /// wait polls, so it never blocks a runtime thread.
    async fn lock_file(&self) -> Result<FileLock, String> {
        let dir = &self.config.dir;
        let path = dir.join(store::LOCK_FILE);
        let failed = |e: std::io::Error| format!("could not lock {}: {e}", path.display());
        ensure_private_dir(dir).map_err(failed)?;
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(&path).map_err(failed)?;
        let deadline = tokio::time::Instant::now() + self.config.lock_timeout;
        loop {
            // Not `file.try_lock()`: std has a method of that name from Rust
            // 1.89, with another error type, and it would win the call.
            match FileExt::try_lock(&file) {
                Ok(()) => return Ok(FileLock(file)),
                Err(TryLockError::WouldBlock) => {
                    if tokio::time::Instant::now() >= deadline {
                        return Err(
                            "another edytlab process held the ChatGPT session lock for too long"
                                .into(),
                        );
                    }
                    tokio::time::sleep(LOCK_POLL).await;
                }
                Err(TryLockError::Error(e)) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(TryLockError::Error(e)) => return Err(failed(e)),
            }
        }
    }

    async fn load_secret(&self) -> Result<Option<SecretString>, SecretStoreError> {
        let store = self.secrets.clone();
        blocking(move || store.load()).await
    }

    async fn save_secret(&self, token: &SecretString) -> Result<(), SecretStoreError> {
        let store = self.secrets.clone();
        let token = token.clone();
        blocking(move || store.save(&token)).await
    }

    async fn delete_secret(&self) -> Result<(), SecretStoreError> {
        let store = self.secrets.clone();
        blocking(move || store.delete()).await
    }
}

/// Runs a store call on a blocking thread: a keychain can wait on an
/// unlock prompt.
async fn blocking<T: Send + 'static>(
    call: impl FnOnce() -> Result<T, SecretStoreError> + Send + 'static,
) -> Result<T, SecretStoreError> {
    tokio::task::spawn_blocking(call).await.unwrap_or_else(|_| {
        Err(SecretStoreError::Unavailable(
            "the keychain call failed".into(),
        ))
    })
}

/// `registration.json`, with a file that does not parse read as none (and
/// replaced by the next save).
fn read_registration(file: &RegistrationFile) -> Result<Option<Registration>, StoreError> {
    match file.load() {
        Err(StoreError::Corrupt(_)) => {
            tracing::warn!(
                "chatgpt: registration.json is not valid; the next sign-in registers again"
            );
            Ok(None)
        }
        other => other,
    }
}

/// The claims of the sign-in's ID token, as far as a refreshed one is
/// compared with them, rebuilt from the registration after a restart.
fn original_claims(record: &ClientRecord) -> IdTokenClaims {
    let subject = record.subject.clone().unwrap_or_default();
    let mut claims = Map::new();
    claims.insert("iss".into(), Value::String(record.issuer.clone()));
    claims.insert("sub".into(), Value::String(subject.clone()));
    claims.insert("aud".into(), Value::String(record.client_id.clone()));
    let reference = &record.id_token_reference;
    if let Some(azp) = &reference.azp {
        claims.insert("azp".into(), Value::String(azp.clone()));
    }
    if let Some(auth_time) = &reference.auth_time {
        claims.insert("auth_time".into(), Value::Number(auth_time.clone()));
    }
    if let Some(nonce) = &reference.nonce {
        claims.insert("nonce".into(), Value::String(nonce.clone()));
    }
    IdTokenClaims {
        issuer: record.issuer.clone(),
        subject,
        audience: record.client_id.clone(),
        expires_at: 0,
        issued_at: 0,
        email: record.email.clone(),
        email_verified: None,
        claims,
    }
}
