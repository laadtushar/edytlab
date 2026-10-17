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
//! refused. [`SignInIntent::AnotherAccount`] registers a new client instead,
//! for a different ChatGPT account. The callback's `iss` is checked (RFC
//! 9207) when the issuer promises one. The ID token is checked against the
//! JWKS with `aud` the issued client id and this attempt's nonce, and on a
//! returning sign-in its `sub` must be the saved account's. Plan usage needs
//! the token response to grant `chatgpt.tokens.use.direct`; without it the
//! session is signed in with plan usage disabled. The active session is
//! untouched until all of that holds, and tokens the session refuses (another
//! account, a bad ID token, a registration it cannot save) are revoked.
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
//! and re-reads both the refresh token and `registration.json` under it. A
//! token this process did not write was written by another window: if the
//! registration still names this process's client, signed in, the session
//! takes the account as the registration now has it (another window may have
//! signed out and in again, with new ID-token claims) and refreshes with that
//! token; if it names another client, or says signed out, this window's
//! session ends in memory and nothing is deleted. The registration is
//! checked on every refresh, this process's own token included. The refresh runs on a task
//! of its own, so a caller that stops waiting (a timeout, a Stop button)
//! cannot leave the server's rotated token unsaved. The rotated refresh token
//! is saved before the new access token is handed to anyone. A failure is
//! classified ([`spec::classify_refresh_error`]):
//!
//! - an unusable refresh token (one of the documented codes, on a 4xx) ends
//!   the session. The stored token is removed only if it is still the one
//!   that failed, never one another process saved since. A
//!   `refresh_token_reused` while the store holds a newer token (a process
//!   that did not take the lock refreshed first) is retried once with that
//!   token;
//! - anything temporary (the network, a 5xx, a 429, a 408, whatever code it
//!   carries) keeps every credential, and the session waits before trying
//!   again (15 s, doubling to 5 min), using the current access token
//!   meanwhile if it is still valid;
//! - any other refusal (`invalid_client` and the like) also keeps the
//!   credentials and waits, and is reported as it is.
//!
//! **Sign-out** ([`ChatGptSession::sign_out`]) signs the account out of
//! every window. Under the lock it reads the stored refresh token whatever
//! this window's state, clears it, marks the account signed out (keeping the
//! registration and the host ID) and lets go of the lock. Then it revokes
//! that token (and this window's own, if that differs) at the discovery
//! document's `revocation_endpoint`, which must be on the issuer's origin,
//! retrying a network failure or a 5xx with backoff, and reports whether the
//! revocation was confirmed. A keychain that cannot be read is reported as
//! an unconfirmed revocation, and its token is left in place for a later
//! sign-out to revoke rather than deleted unrevoked.
//!
//! **Another account** signed in while one is signed in replaces it: the
//! previous account's record is marked signed out and its refresh token
//! revoked, since the keychain has one slot and another account's
//! credentials are never overwritten silently.
//!
//! **The file lock** falls back to a process-wide lock for the session
//! directory, with a warning, where the OS refuses file locks (`ENOLCK` on
//! some network file systems, a read-only file system, a lock file this user
//! may not open). Every session in the process over that directory still
//! takes turns; two edytlab processes are then not kept apart, as before this
//! lock existed.
//!
//! **Secrets.** Tokens are [`SecretString`]s. None is written to
//! `registration.json`, put in a URL, logged, or shown by `Debug`; the
//! email address is left out of `Debug` too.

use std::fmt;
use std::fs::{File, OpenOptions};
use std::future::Future;
use std::io;
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
/// How long revoking a token the session refused may take. It is best
/// effort: the user is not waiting on it to succeed.
const QUIET_REVOKE_TIMEOUT: Duration = Duration::from_secs(10);

/// `ENOLCK`: the file system offers no locks (some NFS mounts).
#[cfg(any(target_os = "linux", target_os = "android"))]
const ENOLCK: Option<i32> = Some(37);
#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "dragonfly"
))]
const ENOLCK: Option<i32> = Some(77);
#[cfg(not(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "dragonfly"
)))]
const ENOLCK: Option<i32> = None;

/// Whether `error`, from opening or locking `refresh.lock`, means file locks
/// cannot be had here at all, rather than that one is taken: the session
/// then makes do with its in-process lock.
pub fn file_lock_unsupported(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::PermissionDenied
            | io::ErrorKind::ReadOnlyFilesystem
            | io::ErrorKind::Unsupported
    ) || (ENOLCK.is_some() && error.raw_os_error() == ENOLCK)
}

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
    #[cfg(feature = "test-endpoints")]
    failing_registration_saves: Option<Arc<AtomicBool>>,
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
            #[cfg(feature = "test-endpoints")]
            failing_registration_saves: None,
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

    /// Tests only (the `test-endpoints` feature): every `registration.json`
    /// save fails while `switch` is set.
    #[cfg(feature = "test-endpoints")]
    pub fn with_failing_registration_saves(mut self, switch: Arc<AtomicBool>) -> Self {
        self.failing_registration_saves = Some(switch);
        self
    }

    /// The session's directory: `registration.json` and `refresh.lock`.
    pub fn dir(&self) -> &std::path::Path {
        &self.dir
    }

    pub fn endpoints(&self) -> &Endpoints {
        &self.endpoints
    }

    fn registration_file(&self) -> RegistrationFile {
        let file = RegistrationFile::new(self.dir.clone());
        #[cfg(feature = "test-endpoints")]
        let file = match &self.failing_registration_saves {
            Some(switch) => file.with_failing_saves(switch.clone()),
            None => file,
        };
        file
    }
}

/// What the user asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignInIntent {
    /// Continue with ChatGPT: the saved account's client if there is one
    /// (signed out or not, so no new client appears in the user's ChatGPT
    /// settings), else a first registration.
    SignIn,
    /// Turn plan usage back on after declining it: the full scopes again,
    /// with `prompt=consent`.
    EnablePlanUsage,
    /// Use a different ChatGPT account: a new registration, whatever is
    /// saved. The other account's record stays, so switching back can reuse
    /// it once there is an account picker.
    AnotherAccount,
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
    /// The stored refresh token disappeared, or the registration says signed
    /// out, while the session ran: another edytlab window signed out.
    #[error("another edytlab window signed out")]
    SignedOutElsewhere,
    /// Another edytlab window signed in to a different ChatGPT account,
    /// whose token now holds the keychain slot.
    #[error("another edytlab window signed in to a different ChatGPT account")]
    ReplacedElsewhere,
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
    /// client is registered for. [`SignInIntent::AnotherAccount`] signs in
    /// to that one.
    #[error(
        "you signed in to a different ChatGPT account than the one saved for this app; \
         choose to use another account to sign in to it"
    )]
    AccountChanged,
    /// The sign-in's last steps stopped before they finished.
    #[error("the sign-in stopped before it finished")]
    Interrupted,
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

/// A ChatGPT plan session. See the module documentation. The state is
/// behind an `Arc`, which the tasks that must finish whatever the caller
/// does (a refresh, the end of a sign-in, a sign-out) hold too.
pub struct ChatGptSession {
    core: Arc<Core>,
}

impl fmt::Debug for ChatGptSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChatGptSession")
            .field("issuer", &self.core.config.endpoints.issuer())
            .field("status", &self.core.status())
            .finish_non_exhaustive()
    }
}

struct Core {
    config: SessionConfig,
    oidc: OidcClient,
    file: RegistrationFile,
    secrets: Arc<dyn RefreshTokenStore>,
    inner: StdMutex<Inner>,
    /// Single-flight: held for every refresh, and around every write of
    /// tokens or of the registration. Taken before the file lock, always.
    flight: Mutex<()>,
    cancel: StdMutex<Cancel>,
    /// Whether the in-process fallback for the file lock was announced.
    warned_no_file_lock: AtomicBool,
}

/// The sign-in in progress, if any: its cancel handle once its listener is
/// bound, and whether a cancel came before that. One mutex covers all three,
/// so a cancel can never land between one attempt ending and the next
/// starting.
#[derive(Default)]
struct Cancel {
    signing_in: bool,
    handle: Option<CancelHandle>,
    requested: bool,
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

/// The cross-process lock: the OS lock on `refresh.lock`, or, where the file
/// system offers no locks, the process-wide lock for the session directory
/// ([`process_lock`]). Released on drop.
enum FileLock {
    Os(File),
    /// Held for its drop, which lets the next session in.
    Process {
        _guard: tokio::sync::OwnedMutexGuard<()>,
    },
}

impl Drop for FileLock {
    fn drop(&mut self) {
        // Closing releases it too; unlocking first makes it immediate.
        if let FileLock::Os(file) = self {
            let _ = FileExt::unlock(file);
        }
    }
}

/// The in-process stand-in for `refresh.lock` where the file system has no
/// locks: one per session directory, shared by every [`ChatGptSession`] in
/// the process, so two sessions over one directory are still kept apart
/// (each session's own lock covers only its own callers).
fn process_lock(dir: &std::path::Path) -> Arc<Mutex<()>> {
    static LOCKS: std::sync::OnceLock<
        StdMutex<std::collections::HashMap<std::path::PathBuf, Arc<Mutex<()>>>>,
    > = std::sync::OnceLock::new();
    let key = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    let mut locks = lock(LOCKS.get_or_init(Default::default));
    Arc::clone(locks.entry(key).or_default())
}

/// Ends a sign-in attempt: no longer in progress, no cancel handle, no
/// pending cancel, all in one step. It owns the core, so the task that
/// finishes a sign-in whose caller stopped waiting can hold it: the sign-in
/// counts as in progress until that task is done.
struct Attempt {
    core: Arc<Core>,
}

impl Drop for Attempt {
    fn drop(&mut self) {
        *lock(&self.core.cancel) = Cancel::default();
    }
}

/// What the registration on disk says about the refresh token a refresh is
/// about to send.
enum Owner {
    /// The registration's active account is still this process's client,
    /// signed in: the token is that account's, perhaps from a newer sign-in.
    Same(ClientRecord),
    /// Signed out, or another client: this window's session is over.
    Gone(EndReason),
    /// `registration.json` could not be read.
    Unknown(String),
}

fn lock<T>(m: &StdMutex<T>) -> StdMutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn unix_secs(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn same_secret(a: &SecretString, b: &SecretString) -> bool {
    constant_time_eq(a.expose_secret(), b.expose_secret())
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

/// Runs `task` on a task of its own and waits for it. If the caller stops
/// waiting, the task still runs to the end: a refresh the server has
/// answered must save its rotated token. `if_lost` is the answer when the
/// task itself fails (it panicked).
async fn detached<T: Send + 'static>(
    task: impl Future<Output = T> + Send + 'static,
    if_lost: impl FnOnce() -> T,
) -> T {
    tokio::spawn(task).await.unwrap_or_else(|_| if_lost())
}

impl ChatGptSession {
    /// Opens the session: reads `registration.json`, and nothing else. A
    /// saved session is [`SessionState::NotLoaded`] until [`Self::restore`]
    /// (or the first bearer) reads its refresh token, since that is a
    /// keychain read. A `registration.json` that does not parse is read as
    /// none; the next sign-in, under the file lock, sets it aside as
    /// [`store::CORRUPT_REGISTRATION_FILE`], registers afresh and clears the
    /// keychain slot, whose token nothing can use or revoke without the
    /// client id the file held.
    pub fn open(
        config: SessionConfig,
        secrets: Arc<dyn RefreshTokenStore>,
    ) -> Result<Self, OpenError> {
        let oidc = OidcClient::new(config.endpoints.issuer())?;
        let file = config.registration_file();
        // No file lock here: a file that does not parse is read as none,
        // and set aside only by a sign-in, under the lock.
        let registration = read_registration(&file, SetAside::No)?;
        let phase = match registration
            .as_ref()
            .and_then(|r| r.active(config.endpoints.issuer()))
        {
            Some(c) if c.subject.is_some() && !c.signed_out => Phase::NotLoaded,
            _ => Phase::SignedOut,
        };
        Ok(Self {
            core: Arc::new(Core {
                config,
                oidc,
                file,
                secrets,
                inner: StdMutex::new(Inner {
                    registration,
                    phase,
                }),
                flight: Mutex::new(()),
                cancel: StdMutex::new(Cancel::default()),
                warned_no_file_lock: AtomicBool::new(false),
            }),
        })
    }

    /// The session as it stands, without any I/O.
    pub fn status(&self) -> ChatGptStatus {
        self.core.status()
    }

    /// Whether a sign-in is in progress.
    pub fn is_signing_in(&self) -> bool {
        lock(&self.core.cancel).signing_in
    }

    /// Reads a saved session's refresh token from its store. Nothing to do
    /// unless the state is [`SessionState::NotLoaded`]. With no token
    /// stored, the session has ended ([`EndReason::NotStored`]); a store that
    /// cannot be read leaves it as it was, and returns the error.
    pub async fn restore(&self) -> Result<ChatGptStatus, SecretStoreError> {
        let _flight = self.core.flight.lock().await;
        self.core.restore_locked().await?;
        Ok(self.core.status())
    }

    /// The access token to send as `Authorization: Bearer`, refreshed first
    /// when it is about to expire. See the module documentation.
    pub async fn bearer(&self) -> Result<SecretString, BearerError> {
        if let Some(answer) = self.core.current_bearer() {
            return answer;
        }
        let core = Arc::clone(&self.core);
        detached(async move { core.bearer_slow().await }, lost_refresh).await
    }

    /// A bearer to replace `rejected`, which the API refused (a 401):
    /// refreshed now, unless another caller already replaced it. Honours the
    /// wait after a failed refresh, not `earliest_refresh_at`.
    pub async fn refresh_rejected(
        &self,
        rejected: &SecretString,
    ) -> Result<SecretString, BearerError> {
        let core = Arc::clone(&self.core);
        let rejected = rejected.clone();
        detached(
            async move { core.refresh_rejected(&rejected).await },
            lost_refresh,
        )
        .await
    }

    /// Signs in through the system browser, which `open_url` opens at the
    /// authorization URL (the desktop app passes its opener; tests pass a
    /// closure that plays the browser). Waits up to the sign-in timeout, or
    /// until [`Self::cancel_sign_in`]. Once the browser has come back with a
    /// code, the rest (the exchange, the checks, saving) runs to the end even
    /// if the caller stops waiting. See the module documentation.
    pub async fn sign_in<F, E>(
        &self,
        open_url: F,
        intent: SignInIntent,
    ) -> Result<ChatGptStatus, SignInError>
    where
        F: FnOnce(&str) -> Result<(), E>,
        E: fmt::Display,
    {
        let attempt = Core::begin_attempt(&self.core)?;
        let core = &self.core;
        let endpoints = &core.config.endpoints;

        let (host_id, kind, login_hint) = core.prepare_sign_in(intent).await?;

        // Discovery: the JWKS for the ID token, and whether `iss` comes back.
        let metadata = core.oidc.metadata().await.map_err(SignInError::Discovery)?;

        let mut loopback = LoopbackConfig::new(LoopbackHost::Ipv4Loopback, spec::CALLBACK_PATH)
            .with_timeout(core.config.sign_in_timeout);
        if let Some(port) = core.config.preferred_port {
            loopback = loopback.with_preferred_port(port);
        }
        let listener = LoopbackListener::bind(loopback).await?;
        {
            // A cancel that came while discovery ran counts: the wait ends
            // before the browser is opened.
            let mut cancel = lock(&core.cancel);
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

        // From here the code is spent whatever happens: finish on a task of
        // its own, so tokens are never left half-saved or unrevoked.
        let core = Arc::clone(&self.core);
        let registering = kind == AttemptKind::Register;
        let code = callback.code;
        detached(
            async move {
                let _attempt = attempt;
                core.complete_sign_in(client_id, registering, code, verifier, nonce, redirect_uri)
                    .await
            },
            || Err(SignInError::Interrupted),
        )
        .await
    }

    /// Ends the sign-in in progress with [`LoopbackError::Cancelled`]: at
    /// once if it is waiting for the browser, or before the browser opens if
    /// it has not got that far. A sign-in already exchanging its code
    /// finishes. Nothing happens when no sign-in is in progress.
    pub fn cancel_sign_in(&self) {
        let mut cancel = lock(&self.core.cancel);
        if !cancel.signing_in {
            return;
        }
        cancel.requested = true;
        if let Some(handle) = cancel.handle.as_ref() {
            handle.cancel();
        }
    }

    /// Signs out of the account, in every window: clears the stored refresh
    /// token and marks the account signed out, then revokes that token (and
    /// this window's, if it differs) and reports whether the revocation was
    /// confirmed. The registration (the issued client and its account) and
    /// the host ID are kept for the next sign-in. It runs to the end even if
    /// the caller stops waiting. See the module documentation.
    pub async fn sign_out(&self) -> SignOutReport {
        let core = Arc::clone(&self.core);
        detached(async move { core.sign_out().await }, || SignOutReport {
            revocation: Revocation::NotConfirmed {
                reason: "the sign-out stopped before it finished".into(),
            },
            removed_from_store: Err(SecretStoreError::Unavailable(
                "the sign-out stopped before it finished".into(),
            )),
        })
        .await
    }
}

fn lost_refresh() -> Result<SecretString, BearerError> {
    Err(BearerError::Unavailable {
        reason: "the refresh stopped before it finished".into(),
    })
}

impl Core {
    fn status(&self) -> ChatGptStatus {
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

    fn issuer(&self) -> &str {
        self.config.endpoints.issuer()
    }

    // --- Restore and the bearer --------------------------------------------------

    async fn restore_locked(&self) -> Result<(), SecretStoreError> {
        if !matches!(lock(&self.inner).phase, Phase::NotLoaded) {
            return Ok(());
        }
        // The registration as it is now, not as it was at `open`: another
        // window may have signed out or in since.
        if let Ok(Some(registration)) = read_registration(&self.file, SetAside::No) {
            lock(&self.inner).registration = Some(registration);
        }
        let record = lock(&self.inner)
            .registration
            .as_ref()
            .and_then(|r| r.active(self.issuer()))
            .cloned()
            .filter(|r| r.subject.is_some() && !r.signed_out);
        let Some(record) = record else {
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

    async fn bearer_slow(&self) -> Result<SecretString, BearerError> {
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

    async fn refresh_rejected(&self, rejected: &SecretString) -> Result<SecretString, BearerError> {
        let _flight = self.flight.lock().await;
        self.restore_locked().await.map_err(BearerError::Store)?;
        let now = self.config.clock.now();
        {
            let inner = lock(&self.inner);
            if let Phase::Active(a) = &inner.phase {
                if let Some(t) = &a.access {
                    if now < t.expires_at && !same_secret(&t.token, rejected) {
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

    // --- Refresh -----------------------------------------------------------------

    /// Refreshes, holding `flight`. See the module documentation.
    async fn refresh_locked(&self) -> Result<SecretString, BearerError> {
        let (mut client_id, mut original, memory_token, storage) = {
            let inner = lock(&self.inner);
            match &inner.phase {
                Phase::Active(a) => (
                    a.client_id.clone(),
                    a.original.clone(),
                    a.refresh_token.clone(),
                    a.storage.clone(),
                ),
                Phase::SignedOut | Phase::NotLoaded => return Err(BearerError::NotSignedIn),
                Phase::Ended(r) => return Err(BearerError::SessionEnded(r.clone())),
            }
        };
        let file_lock = match self.lock_file().await {
            Ok(l) => l,
            Err(reason) => return Err(self.failed(BearerError::Unavailable { reason })),
        };
        // The store is the truth when the token lives there: another process
        // may have rotated it, or signed out or in, since this one looked.
        let mut sent = match (&storage, memory_token.clone()) {
            (Some(TokenStorage::Stored), memory) => match self.load_secret().await {
                Ok(Some(stored)) => stored,
                Ok(None) => {
                    // Another window signed out. End here only: there is
                    // nothing of this window's left to delete.
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
        let mut retried = false;
        loop {
            // The registration as it is on disk now, on every refresh: a
            // window that signed out may have failed to delete the token,
            // so even this process's own token is used only while the
            // registration still names its client, signed in.
            let ours = memory_token.as_ref().is_some_and(|m| same_secret(m, &sent));
            match self.owner_of_stored_token(&client_id) {
                Owner::Same(record) if !ours => {
                    // Another window wrote this token, perhaps after signing
                    // out and in again: take the account as it now is.
                    client_id = record.client_id.clone();
                    original = original_claims(&record);
                    let mut inner = lock(&self.inner);
                    if let Phase::Active(a) = &mut inner.phase {
                        a.client_id = record.client_id.clone();
                        a.original = original.clone();
                        a.scopes = record.scopes.clone();
                        a.refresh_token = Some(sent.clone());
                        a.access = None;
                    }
                }
                Owner::Same(_) => {}
                Owner::Gone(reason) => {
                    tracing::info!("chatgpt: this window's session ended: {reason}");
                    self.end(reason.clone(), None).await;
                    return Err(BearerError::SessionEnded(reason));
                }
                Owner::Unknown(reason) => {
                    return Err(self.failed(BearerError::Unavailable { reason }));
                }
            }
            let client = match TokenClient::new(
                self.config.endpoints.token_endpoint().clone(),
                &client_id,
            ) {
                Ok(c) => c,
                Err(e) => {
                    return Err(self.failed(BearerError::Unavailable {
                        reason: e.to_string(),
                    }))
                }
            };
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
                            if !same_secret(&newer, &sent) {
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

    /// Whose the refresh token about to be sent is, from `registration.json`
    /// as it is on disk now. The caller holds the file lock.
    fn owner_of_stored_token(&self, client_id: &str) -> Owner {
        let registration = match read_registration(&self.file, SetAside::Yes) {
            Ok(r) => r,
            Err(e) => return Owner::Unknown(e.to_string()),
        };
        let active = registration
            .as_ref()
            .and_then(|r| r.active(self.issuer()))
            .cloned();
        if let Some(registration) = registration {
            lock(&self.inner).registration = Some(registration);
        }
        match active {
            Some(record) if record.signed_out => Owner::Gone(EndReason::SignedOutElsewhere),
            Some(record) if record.client_id != client_id => {
                Owner::Gone(EndReason::ReplacedElsewhere)
            }
            Some(record) if record.subject.is_some() => Owner::Same(record),
            _ => Owner::Gone(EndReason::SignedOutElsewhere),
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
                    // Refused, so not left live at OpenAI either.
                    self.revoke_quietly(client_id, &refresh_token).await;
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
    /// there, so a token another process saved since is never erased; with
    /// no `spent`, nothing is removed.
    async fn end(&self, reason: EndReason, spent: Option<&SecretString>) {
        if let Some(spent) = spent {
            if let Ok(Some(stored)) = self.load_secret().await {
                if same_secret(&stored, spent) {
                    if let Err(e) = self.delete_secret().await {
                        tracing::warn!("chatgpt: could not remove the unusable refresh token: {e}");
                    }
                }
            }
        }
        lock(&self.inner).phase = Phase::Ended(reason);
    }

    // --- Sign-in -----------------------------------------------------------------

    fn begin_attempt(core: &Arc<Core>) -> Result<Attempt, SignInError> {
        let mut cancel = lock(&core.cancel);
        if cancel.signing_in {
            return Err(SignInError::AlreadyInProgress);
        }
        *cancel = Cancel {
            signing_in: true,
            handle: None,
            requested: false,
        };
        Ok(Attempt {
            core: Arc::clone(core),
        })
    }

    /// The host ID (saved before the first sign-in) and the client to use.
    async fn prepare_sign_in(
        &self,
        intent: SignInIntent,
    ) -> Result<(HostId, AttemptKind, Option<String>), SignInError> {
        let _flight = self.flight.lock().await;
        let _file = self.lock_file().await.map_err(SignInError::Lock)?;
        let registration = match read_registration(&self.file, SetAside::Yes)? {
            Some(r) => r,
            None => {
                // No registration (a first sign-in, or one set aside as
                // corrupt): a token in the keychain slot belongs to nothing
                // that can use it, and cannot be revoked without its client
                // id. Clear it.
                if let Ok(Some(_)) = self.load_secret().await {
                    tracing::warn!(
                        "chatgpt: clearing a refresh token no registration accounts for"
                    );
                    let _ = self.delete_secret().await;
                }
                let r = Registration::new(HostId::generate()?);
                self.file.save(&r)?;
                r
            }
        };
        let client = match intent {
            SignInIntent::AnotherAccount => None,
            SignInIntent::SignIn | SignInIntent::EnablePlanUsage => {
                registration.client_for_sign_in(self.issuer()).cloned()
            }
        };
        let host_id = registration.host_id.clone();
        lock(&self.inner).registration = Some(registration);
        let kind = match &client {
            Some(c) => AttemptKind::Reauthorize {
                client_id: c.client_id.clone(),
            },
            None => AttemptKind::Register,
        };
        let login_hint = client.filter(|c| c.subject.is_some()).and_then(|c| c.email);
        Ok((host_id, kind, login_hint))
    }

    /// The sign-in after the browser came back with a code: the issued
    /// client saved, the code exchanged, the ID token checked, the session
    /// replaced. Tokens it refuses are revoked.
    async fn complete_sign_in(
        &self,
        client_id: String,
        registering: bool,
        code: SecretString,
        verifier: PkceVerifier,
        nonce: Nonce,
        redirect_uri: String,
    ) -> Result<ChatGptStatus, SignInError> {
        if registering {
            // Saved before the exchange, so a failed exchange does not
            // register another client next time.
            self.record_issued_client(&client_id).await?;
        }
        let tokens = TokenClient::new(self.config.endpoints.token_endpoint().clone(), &client_id)
            .map_err(SignInError::Exchange)?
            .exchange_code(
                &code,
                &verifier,
                &redirect_uri,
                &[("resource", spec::RESOURCE)],
            )
            .await
            .map_err(|e| match e.oauth_error() {
                Some(body) if body.error.eq_ignore_ascii_case("invalid_grant") => {
                    SignInError::CodeRejected
                }
                _ => SignInError::Exchange(e),
            })?;
        let outcome = self.accept_sign_in(&client_id, &tokens, &nonce).await;
        if outcome.is_err() {
            if let Some(refresh_token) = tokens.refresh_token.as_ref().filter(|t| !t.is_empty()) {
                self.revoke_quietly(&client_id, refresh_token).await;
            }
        }
        outcome
    }

    async fn accept_sign_in(
        &self,
        client_id: &str,
        tokens: &TokenResponse,
        nonce: &Nonce,
    ) -> Result<ChatGptStatus, SignInError> {
        let id_token = tokens
            .id_token
            .as_ref()
            .filter(|t| !t.is_empty())
            .ok_or(SignInError::NoIdToken)?;
        let claims = self
            .oidc
            .verify_id_token_at(
                id_token,
                &IdTokenExpectations::sign_in(client_id, nonce),
                self.config.clock.now(),
            )
            .await
            .map_err(SignInError::IdToken)?;
        self.finish_sign_in(client_id.to_string(), claims, tokens)
            .await
    }

    /// Adds the client a registration callback issued, unverified, so the
    /// next sign-in uses it even if this one goes no further.
    async fn record_issued_client(&self, client_id: &str) -> Result<(), SignInError> {
        let _flight = self.flight.lock().await;
        let _file = self.lock_file().await.map_err(SignInError::Lock)?;
        let now = unix_secs(self.config.clock.now());
        let issuer = self.issuer().to_string();
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

    /// The tokens of a verified sign-in: the account checked, the
    /// registration saved, then the refresh token stored, and only then the
    /// session replaced. If the registration cannot be saved, nothing else
    /// has changed: the session in use, and its stored token, are as they
    /// were.
    ///
    /// A sign-in to another account while one is signed in replaces it: the
    /// keychain has one slot, and "never overwrite another account
    /// registration's credentials" rules out doing that silently. The
    /// previous account's record is marked signed out in the same write, and
    /// its refresh token (read from the slot before it is overwritten, and
    /// this window's own if that differs) is revoked once the locks are let
    /// go.
    async fn finish_sign_in(
        &self,
        client_id: String,
        claims: IdTokenClaims,
        tokens: &TokenResponse,
    ) -> Result<ChatGptStatus, SignInError> {
        let (status, superseded) = {
            let _flight = self.flight.lock().await;
            let _file = self.lock_file().await.map_err(SignInError::Lock)?;
            let now = self.config.clock.now();

            let mut registration = read_registration(&self.file, SetAside::Yes)?
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

            // Another account still signed in, which this one replaces.
            let previous = registration
                .active(self.issuer())
                .filter(|c| c.client_id != client_id && !c.signed_out)
                .map(|c| c.client_id.clone());
            let mut superseded: Vec<(String, SecretString)> = Vec::new();
            if let Some(previous) = &previous {
                match self.load_secret().await {
                    Ok(Some(token)) => superseded.push((previous.clone(), token)),
                    Ok(None) => {}
                    Err(e) => tracing::warn!(
                        "chatgpt: the previous account's refresh token could not be read to \
                         revoke it: {e}"
                    ),
                }
                if let Phase::Active(a) = &lock(&self.inner).phase {
                    if a.client_id == *previous {
                        if let Some(token) = &a.refresh_token {
                            if !superseded.iter().any(|(_, t)| same_secret(t, token)) {
                                superseded.push((previous.clone(), token.clone()));
                            }
                        }
                    }
                }
                if let Some(c) = registration.client_mut(previous) {
                    c.signed_out = true;
                    c.saved_at = unix_secs(now);
                }
            }

            // The registration first: if it cannot be written, nothing else
            // changes.
            let scopes = spec::granted_scopes(tokens).unwrap_or_default();
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
                issuer: self.issuer().to_string(),
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
            self.file.save(&registration)?;

            // Then the refresh token: if the store refuses it, the session
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
                        earliest_refresh_at: spec::earliest_refresh_at(tokens),
                    }),
                    refresh_token,
                    storage,
                    retry: None,
                }));
            }
            (self.status(), superseded)
        };
        for (previous, token) in &superseded {
            if let Revocation::NotConfirmed { reason } = self.revoke(previous, token, true).await {
                tracing::warn!(
                    "chatgpt: the account signed in before could not be revoked: {reason}"
                );
            }
        }
        Ok(status)
    }

    // --- Sign-out ----------------------------------------------------------------

    async fn sign_out(&self) -> SignOutReport {
        let issuer = self.issuer().to_string();
        // Under the locks: what to revoke, then the local sign-out. The
        // revocations, retries and all, run after the locks are let go, so
        // no other window waits on them.
        let (to_revoke, unreadable, removed_from_store) = {
            let _flight = self.flight.lock().await;
            // Signing out must not hang on another process: without the
            // lock it goes ahead.
            let _file = self.lock_file().await.ok();
            let (window_client, window_token) = {
                let inner = lock(&self.inner);
                match &inner.phase {
                    Phase::Active(a) => (Some(a.client_id.clone()), a.refresh_token.clone()),
                    _ => (None, None),
                }
            };
            // The account as registration.json has it now, and whatever
            // token the keychain holds, whatever this window's state: an
            // ended or not-yet-loaded window still signs the account out.
            let disk_client = read_registration(&self.file, SetAside::Yes)
                .ok()
                .flatten()
                .or_else(|| lock(&self.inner).registration.clone())
                .and_then(|r| r.active(&issuer).map(|c| c.client_id.clone()));
            let mut to_revoke: Vec<(Option<String>, SecretString)> = Vec::new();
            let mut unreadable = None;
            match self.load_secret().await {
                Ok(Some(stored)) => {
                    to_revoke.push((disk_client.clone().or(window_client.clone()), stored))
                }
                Ok(None) => {}
                Err(e) => unreadable = Some(e),
            }
            if let Some(token) = window_token {
                if !to_revoke.iter().any(|(_, t)| same_secret(t, &token)) {
                    to_revoke.push((window_client.or(disk_client), token));
                }
            }
            // A keychain that cannot be read holds a token nobody has seen:
            // it is left where it is, for a later sign-out to revoke, rather
            // than deleted unrevoked. The registration still says signed
            // out, so no window uses it meanwhile.
            let removed_from_store = match &unreadable {
                Some(e) => Err(e.clone()),
                None => self.delete_secret().await,
            };
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
            (to_revoke, unreadable, removed_from_store)
        };

        let mut revocation = match &unreadable {
            Some(e) => Revocation::NotConfirmed {
                reason: format!(
                    "the keychain could not be read, so the saved session was not revoked: {e}"
                ),
            },
            None => Revocation::NothingToRevoke,
        };
        for (client_id, token) in &to_revoke {
            let outcome = match client_id {
                Some(client_id) => self.revoke(client_id, token, true).await,
                None => Revocation::NotConfirmed {
                    reason: "no client id is saved for this session".into(),
                },
            };
            revocation = match (revocation, outcome) {
                (Revocation::NotConfirmed { reason }, _) => Revocation::NotConfirmed { reason },
                (_, outcome) => outcome,
            };
        }
        if let Revocation::NotConfirmed { reason } = &revocation {
            tracing::warn!("chatgpt: sign-out could not confirm the revocation: {reason}");
        }
        SignOutReport {
            revocation,
            removed_from_store,
        }
    }

    /// Revokes `token`, retrying a network failure or a 5xx when `retry`.
    async fn revoke(&self, client_id: &str, token: &SecretString, retry: bool) -> Revocation {
        let issuer = self.issuer();
        let mut last = String::new();
        let delays: &[Duration] = if retry {
            &self.config.revoke_retry_delays
        } else {
            &[]
        };
        let waits = std::iter::once(None).chain(delays.iter().map(Some));
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
                    let retryable = match &e {
                        TokenError::Network(_) | TokenError::TooLarge(_) => true,
                        TokenError::Status { status } | TokenError::OAuth { status, .. } => {
                            *status >= 500
                        }
                        _ => false,
                    };
                    last = e.to_string();
                    if !retryable {
                        break;
                    }
                }
            }
        }
        Revocation::NotConfirmed { reason: last }
    }

    /// Revokes a token the session refused, once, best effort.
    async fn revoke_quietly(&self, client_id: &str, token: &SecretString) {
        match tokio::time::timeout(QUIET_REVOKE_TIMEOUT, self.revoke(client_id, token, false)).await
        {
            Ok(Revocation::Confirmed) | Ok(Revocation::NothingToRevoke) => {}
            Ok(Revocation::NotConfirmed { reason }) => {
                tracing::warn!("chatgpt: could not revoke a refused token: {reason}");
            }
            Err(_) => tracing::warn!("chatgpt: revoking a refused token timed out"),
        }
    }

    // --- Files and the keychain --------------------------------------------------

    /// Reads, changes and saves `registration.json` (re-read from disk, so
    /// another process's changes are kept), then caches it. The caller holds
    /// the file lock.
    fn update_registration(
        &self,
        change: impl FnOnce(&mut Registration),
    ) -> Result<(), StoreError> {
        let mut registration = match read_registration(&self.file, SetAside::Yes)? {
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
    /// wait polls, so it never blocks a runtime thread. Where the OS offers
    /// no file locks, the in-process lock stands alone, with a warning.
    async fn lock_file(&self) -> Result<FileLock, String> {
        let dir = &self.config.dir;
        let path = dir.join(store::LOCK_FILE);
        let failed = |e: &io::Error| format!("could not lock {}: {e}", path.display());
        let deadline = tokio::time::Instant::now() + self.config.lock_timeout;
        if let Err(e) = ensure_private_dir(dir) {
            return if file_lock_unsupported(&e) {
                self.process_wide_lock(&e, deadline).await
            } else {
                Err(failed(&e))
            };
        }
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = match options.open(&path) {
            Ok(file) => file,
            Err(e) if file_lock_unsupported(&e) => {
                return self.process_wide_lock(&e, deadline).await
            }
            Err(e) => return Err(failed(&e)),
        };
        loop {
            // Not `file.try_lock()`: std has a method of that name from Rust
            // 1.89, with another error type, and it would win the call.
            match FileExt::try_lock(&file) {
                Ok(()) => return Ok(FileLock::Os(file)),
                Err(TryLockError::WouldBlock) => {
                    if tokio::time::Instant::now() >= deadline {
                        return Err(
                            "another edytlab process held the ChatGPT session lock for too long"
                                .into(),
                        );
                    }
                    tokio::time::sleep(LOCK_POLL).await;
                }
                Err(TryLockError::Error(e)) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(TryLockError::Error(e)) if file_lock_unsupported(&e) => {
                    return self.process_wide_lock(&e, deadline).await
                }
                Err(TryLockError::Error(e)) => return Err(failed(&e)),
            }
        }
    }

    /// The fallback where the OS gives no file lock: the process-wide lock
    /// for this directory, with one warning that other processes are not
    /// kept out.
    async fn process_wide_lock(
        &self,
        why: &io::Error,
        deadline: tokio::time::Instant,
    ) -> Result<FileLock, String> {
        if !self.warned_no_file_lock.swap(true, Ordering::Relaxed) {
            tracing::warn!(
                "chatgpt: file locks are not available here ({why}); another edytlab process \
                 could refresh the ChatGPT session at the same time"
            );
        }
        match tokio::time::timeout_at(deadline, process_lock(&self.config.dir).lock_owned()).await {
            Ok(guard) => Ok(FileLock::Process { _guard: guard }),
            Err(_) => Err("another ChatGPT session held the session lock for too long".into()),
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

/// Runs a store call on a blocking thread: a keychain call is blocking I/O.
async fn blocking<T: Send + 'static>(
    call: impl FnOnce() -> Result<T, SecretStoreError> + Send + 'static,
) -> Result<T, SecretStoreError> {
    tokio::task::spawn_blocking(call).await.unwrap_or_else(|_| {
        Err(SecretStoreError::Unavailable(
            "the keychain call failed".into(),
        ))
    })
}

/// Whether a `registration.json` that does not parse may be moved aside:
/// only by a caller holding the file lock, so a window that opens while
/// another replaces the file never moves it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum SetAside {
    Yes,
    No,
}

/// `registration.json`, with a file that does not parse read as none and,
/// under the lock, set aside as `registration.json.corrupt`.
fn read_registration(
    file: &RegistrationFile,
    set_aside: SetAside,
) -> Result<Option<Registration>, StoreError> {
    match file.load() {
        Err(StoreError::Corrupt(_)) if set_aside == SetAside::No => Ok(None),
        Err(StoreError::Corrupt(_)) => {
            match file.set_aside() {
                Ok(_) => tracing::warn!(
                    "chatgpt: registration.json is not valid; it was set aside as \
                     registration.json.corrupt, and the next sign-in registers again"
                ),
                Err(e) => tracing::warn!(
                    "chatgpt: registration.json is not valid, and could not be set aside: {e}"
                ),
            }
            Ok(None)
        }
        other => other,
    }
}

/// The claims of the sign-in's ID token, as far as a refreshed one is
/// compared with them, rebuilt from the registration.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_system_without_locks_falls_back_to_the_process_lock() {
        for unsupported in [
            io::Error::from(io::ErrorKind::PermissionDenied),
            io::Error::from(io::ErrorKind::ReadOnlyFilesystem),
            io::Error::from(io::ErrorKind::Unsupported),
        ] {
            assert!(file_lock_unsupported(&unsupported), "{unsupported:?}");
        }
        if let Some(enolck) = ENOLCK {
            assert!(file_lock_unsupported(&io::Error::from_raw_os_error(enolck)));
        }
        // A lock someone holds, or any other failure, is not that.
        for other in [
            io::Error::from(io::ErrorKind::WouldBlock),
            io::Error::from(io::ErrorKind::NotFound),
            io::Error::from(io::ErrorKind::Interrupted),
            io::Error::other("disk on fire"),
        ] {
            assert!(!file_lock_unsupported(&other), "{other:?}");
        }
    }

    #[tokio::test]
    async fn the_fallback_lock_is_one_per_directory_for_the_whole_process() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        // Every session over one directory gets the same lock, however the
        // path is spelled; another directory gets its own.
        let first = process_lock(a.path());
        let spelled_otherwise = process_lock(&a.path().join("."));
        assert!(Arc::ptr_eq(&first, &spelled_otherwise));
        assert!(!Arc::ptr_eq(&first, &process_lock(b.path())));
        // And it excludes: while one holds it, another waits.
        let held = first.clone().lock_owned().await;
        let waiting = tokio::time::timeout(
            Duration::from_millis(50),
            process_lock(a.path()).lock_owned(),
        )
        .await;
        assert!(
            waiting.is_err(),
            "a second session over the directory waits"
        );
        drop(held);
        process_lock(a.path()).lock_owned().await;
    }
}
