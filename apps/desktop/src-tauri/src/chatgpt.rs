//! The ChatGPT plan in the desktop app (#474, PR 5): the session the window
//! signs in with, and everything around it that the commands share.
//!
//! `ai::chatgpt` does the sign-in, the refresh and the requests and knows
//! nothing of Tauri. This module is the app's side of it:
//!
//! * [`KeychainTokenStore`]: the session's refresh token in the keychain slot
//!   `chatgpt_refresh_token`, through the same plumbing as the API keys, so
//!   the Linux kernel-keyring fallback (#394) and a startup the keychain
//!   refused (#497) behave the same way. It reads nothing after a refusal.
//! * [`ChatGptHandle`]: the one session, opened at startup; the status the
//!   window reads and is sent on [`crate::events::CHATGPT_STATUS`] whenever it
//!   changes; what the agent's provider has learned about the route, carried
//!   across rebuilds; and the message a turn could not send because the
//!   session ended.
//! * The status DTO ([`ChatGptStatusDto`]), which holds no token, and the
//!   typed error text a failed turn is reported as ([`plan_usage_text`]).
//!
//! # The provider the agent runs on is the agent's alone
//!
//! The provider holds state that changes a request: the tool rung, the
//! reasoning choice, the reasoning levels, and the stream parser. Test and the
//! model list run beside a turn, and a request built on the agent's own
//! provider would step its ladder, replace its levels or restart its parser
//! under the turn in flight (the review of #695, finding 7). So the agent's
//! provider is built here ([`ChatGptHandle::new_agent_provider`]) and nothing
//! else touches it while a turn runs: Test and the model list each get a
//! **copy** ([`ChatGptHandle::list_models`], [`ChatGptHandle::probe`]) made
//! from what the app remembers, and what a copy learns (a rung the route
//! took, the models' reasoning levels) is kept in that memory on purpose. It
//! reaches the running agent only at a turn boundary
//! ([`ChatGptHandle::turn_boundary`], with the agent's lock held and no turn
//! running), and the next agent built starts from it. A rung the agent itself
//! stepped to is read back when its turn ends
//! ([`ChatGptHandle::end_of_turn`]) and when it is replaced.
//!
//! # Account generation
//!
//! `ChatGptHandle::account_generation` counts the times the account changed
//! (a sign-out, a start over, another account, a sign-in whose email
//! differs). A request that runs beside a turn takes it with its copy of the
//! provider, and a turn takes it at its start: what a copy learned for an
//! older generation is thrown away, and a message a turn could not send is
//! kept only if the generation is still the one it started under and the
//! session ended (it was not signed out of). Otherwise an in-flight turn could
//! teach, or hand a message to, the next account.
//!
//! # Locks
//!
//! None is held across an `.await`, a call into the session that does I/O, or
//! a notification. `hooks`, `session` and `published` are leaves, read and
//! cloned out. The order where two are held is `memory` and then the
//! provider's own state lock (inside `ai`, itself a leaf), and `pending` and
//! then a read of the session's in-memory status (no I/O). The agent's lock
//! (`AppState::agent`) comes before `memory`: `turn_boundary` and the rebuilds
//! run with it held, and nothing takes the agent's lock while holding
//! `memory`.

use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use ai::chatgpt::provider::ToolRung;
use ai::chatgpt::store::{RefreshTokenStore, SecretStoreError, StoreError};
use ai::chatgpt::{
    ChatGptProvider, ChatGptSession, ChatGptStatus, EndReason, ForgetError, PlanUsageError,
    Revocation, SessionState, SignInError, SignInIntent, SignOutReport, TokenStorage,
};
use ai::keychain::{KeychainError, Persistence};
use ai::oauth::loopback::LoopbackError;
use ai::oauth::SecretString;
use serde::Serialize;

// ---------------------------------------------------------------------------
// The refresh token's keychain slot
// ---------------------------------------------------------------------------

/// The one keychain slot the ChatGPT plan has, `chatgpt_refresh_token`, as
/// the store sees it. A trait so a test can stand in for the OS.
pub trait TokenSlot: Send + Sync + 'static {
    fn load(&self) -> Result<Option<String>, KeychainError>;
    fn save(&self, token: &str) -> Result<Persistence, keyring::Error>;
    fn delete(&self) -> Result<(), keyring::Error>;
    /// Where a value saved now would end up (see `ai::keychain::persistence`).
    fn persistence(&self) -> Persistence;
}

/// The OS keychain, through `ai::keychain`'s one slot for the refresh token.
pub struct OsTokenSlot;

impl TokenSlot for OsTokenSlot {
    fn load(&self) -> Result<Option<String>, KeychainError> {
        ai::keychain::try_load_chatgpt_refresh_token()
    }
    fn save(&self, token: &str) -> Result<Persistence, keyring::Error> {
        ai::keychain::save_chatgpt_refresh_token(token)
    }
    fn delete(&self) -> Result<(), keyring::Error> {
        ai::keychain::delete_chatgpt_refresh_token()
    }
    fn persistence(&self) -> Persistence {
        ai::keychain::persistence()
    }
}

/// Why the keychain is not asked, if startup found it refusing access: the
/// note `commands::refusal_note` words for every other read.
pub type Refusal = Arc<dyn Fn() -> Option<String> + Send + Sync>;

/// The session's [`RefreshTokenStore`], on the keychain.
///
/// * After a startup the keychain refused ([`Refusal`]) a **read** is not made
///   and answers [`SecretStoreError::Unavailable`] with the refusal's note,
///   as an API key's read is not (#497): each one would raise another unlock
///   prompt, right after the user dismissed the first. A save and a delete are
///   the user's own act (signing in, signing out) and are made.
/// * It remembers where the last save or read landed, so the status can say a
///   token will not survive a reboot (the kernel keyring, #394) as the API
///   key's banner does.
/// * No error carries the token: they are built from the keychain error's
///   `Display`, which `ai` keeps free of values.
pub struct KeychainTokenStore {
    slot: Box<dyn TokenSlot>,
    refusal: Refusal,
    persistence: Mutex<Option<Persistence>>,
}

impl KeychainTokenStore {
    pub fn new(slot: impl TokenSlot, refusal: Refusal) -> Self {
        Self {
            slot: Box::new(slot),
            refusal,
            persistence: Mutex::new(None),
        }
    }

    /// Where the token landed the last time it was saved or read here.
    pub fn persistence(&self) -> Option<Persistence> {
        lock(&self.persistence).clone()
    }

    fn remember(&self, persistence: Persistence) {
        *lock(&self.persistence) = Some(persistence);
    }
}

impl RefreshTokenStore for KeychainTokenStore {
    fn load(&self) -> Result<Option<SecretString>, SecretStoreError> {
        if let Some(why) = (self.refusal)() {
            return Err(SecretStoreError::Unavailable(why));
        }
        match self.slot.load() {
            Ok(Some(token)) => {
                // A token that was read is where the slot keeps tokens.
                if lock(&self.persistence).is_none() {
                    self.remember(self.slot.persistence());
                }
                Ok(Some(SecretString::new(token)))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(SecretStoreError::Unavailable(e.reason().to_string())),
        }
    }

    fn save(&self, token: &SecretString) -> Result<(), SecretStoreError> {
        let persistence = self
            .slot
            .save(token.expose_secret())
            .map_err(SecretStoreError::from)?;
        self.remember(persistence);
        Ok(())
    }

    fn delete(&self) -> Result<(), SecretStoreError> {
        self.slot.delete().map_err(SecretStoreError::from)?;
        *lock(&self.persistence) = None;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// What the window is told
// ---------------------------------------------------------------------------

/// Where the refresh token is kept, as the window shows it: `persistent` is
/// whether it survives a restart of the computer, the same words as
/// [`crate::commands::KeychainPersistenceDto`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenStorageDto {
    /// `keychain` (the system keychain), `kernel_keyring` (Linux with no
    /// Secret Service: gone at the next reboot) or `memory` (the keychain
    /// refused the token: gone when edytlab quits).
    pub kind: &'static str,
    pub persistent: bool,
    /// Why it is not persistent, in words for the banner.
    pub reason: Option<String>,
}

/// Why the state is what it is, for the states that have a reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReasonDto {
    /// A stable word to switch on; see [`ChatGptStatusDto::state`].
    pub code: &'static str,
    /// The sentence the window can show.
    pub message: String,
}

/// The ChatGPT sign-in as the window reads it, on `chatgpt_status` and on
/// every [`crate::events::CHATGPT_STATUS`]. Holds no token and no path.
#[derive(Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatGptStatusDto {
    /// One of:
    ///
    /// * `signed_out`: no account, or the user signed out of it.
    /// * `signing_in`: the browser is open and edytlab is waiting for it.
    ///   Cancel with `chatgpt_cancel_sign_in`.
    /// * `signed_in`: ready to use the plan.
    /// * `plan_disabled`: signed in, but plan usage was declined (the
    ///   `reason`); signing in again with `enable_plan_usage` asks again.
    /// * `session_ended`: the sign-in is over without a sign-out (the `reason`
    ///   code says why). A new sign-in needs no consent screen.
    /// * `unread`: a sign-in is saved, but its token has not been read from
    ///   the keychain (it refused access at launch, or could not be read).
    /// * `unavailable`: this build could not open the session (no config
    ///   folder). `chatgpt_sign_in` answers with an error.
    pub state: &'static str,
    /// The account's email, when the session has one and the state is not
    /// `signed_out`. Personal data: never logged.
    pub email: Option<String>,
    /// Present for `plan_disabled`, `session_ended`, `unread` and
    /// `unavailable`. The codes are `plan_usage_declined`,
    /// `refresh_token_unusable`, `id_token_rejected`, `no_refresh_token`,
    /// `not_stored`, `signed_out_elsewhere`, `replaced_elsewhere`,
    /// `keychain_unread` and `no_config_dir`.
    pub reason: Option<ReasonDto>,
    /// Where the refresh token is, while there is a session.
    pub token_storage: Option<TokenStorageDto>,
    /// A message could not be sent because the session ended, and is kept
    /// (`chatgpt_take_pending_message`) so the window can offer to send it
    /// again once the person has signed in.
    pub pending_message: bool,
}

impl std::fmt::Debug for ChatGptStatusDto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChatGptStatusDto")
            .field("state", &self.state)
            .field("email", &self.email.as_ref().map(|_| "<redacted>"))
            .field("reason", &self.reason)
            .field("token_storage", &self.token_storage)
            .field("pending_message", &self.pending_message)
            .finish()
    }
}

/// What a sign-out or a start-over did, for the window to tell the person.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatGptEndDto {
    pub status: ChatGptStatusDto,
    /// `confirmed`, `not_confirmed` or `nothing_to_revoke`.
    pub revocation: &'static str,
    /// Whether the saved refresh token was taken out of the keychain.
    pub token_removed: bool,
    /// For a start-over: whether the registration was forgotten. Absent for
    /// a sign-out.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub registration_forgotten: Option<bool>,
    /// What the person should know and may do, in words: that OpenAI could
    /// not be told to revoke the sign-in, that the old entry may remain in
    /// ChatGPT's settings and can be disconnected there. `None` when there
    /// is nothing to add.
    pub notice: Option<String>,
}

/// A message a turn could not send because the ChatGPT session ended, kept so
/// the window can offer **Continue with ChatGPT** and send it again (#474,
/// G9). In memory only: it is gone when edytlab quits.
#[derive(Clone, PartialEq, Eq)]
pub struct PendingMessage {
    /// What the person typed.
    pub text: String,
    /// What to send again: `text`, when the turn got no further than
    /// sending it (it was taken back off the conversation, so it is not
    /// there twice); a fixed line asking the model to carry on when tools had
    /// already run, because the conversation then holds the request and its
    /// results and sending the text again would repeat it.
    pub resume_text: String,
    /// The tools the person had switched off for that turn.
    pub disabled_tools: Vec<String>,
    /// Whether the turn had got as far as running tools or hearing the
    /// model's answer before the session ended.
    pub progressed: bool,
}

/// The line sent in place of the person's text when the interrupted turn had
/// already made progress ([`PendingMessage::resume_text`]).
pub const CONTINUE_TEXT: &str = "Please carry on with my last request from where you stopped. \
     The connection to my ChatGPT account ended part-way and I have signed in again.";

/// [`PendingMessage`] as the window reads it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingMessageDto {
    pub text: String,
    pub resume_text: String,
    pub disabled_tools: Vec<String>,
    pub progressed: bool,
}

impl From<PendingMessage> for PendingMessageDto {
    fn from(p: PendingMessage) -> Self {
        Self {
            text: p.text,
            resume_text: p.resume_text,
            disabled_tools: p.disabled_tools,
            progressed: p.progressed,
        }
    }
}

fn end_code(reason: &EndReason) -> &'static str {
    match reason {
        EndReason::RefreshTokenUnusable { .. } => "refresh_token_unusable",
        EndReason::IdTokenRejected { .. } => "id_token_rejected",
        EndReason::NoRefreshToken => "no_refresh_token",
        EndReason::NotStored => "not_stored",
        EndReason::SignedOutElsewhere => "signed_out_elsewhere",
        EndReason::ReplacedElsewhere => "replaced_elsewhere",
    }
}

fn token_storage_dto(storage: &TokenStorage, persistence: Option<&Persistence>) -> TokenStorageDto {
    match storage {
        TokenStorage::Stored => match persistence {
            Some(Persistence::SessionOnly { reason }) => TokenStorageDto {
                kind: "kernel_keyring",
                persistent: false,
                reason: Some(reason.clone()),
            },
            _ => TokenStorageDto {
                kind: "keychain",
                persistent: true,
                reason: None,
            },
        },
        TokenStorage::MemoryOnly(why) => TokenStorageDto {
            kind: "memory",
            persistent: false,
            reason: Some(why.to_string()),
        },
    }
}

/// The status the window reads, from the session's own and the app's.
/// `refusal` is the keychain's note after a startup it refused.
fn status_dto(
    status: &ChatGptStatus,
    signing_in: bool,
    persistence: Option<&Persistence>,
    refusal: Option<String>,
    pending_message: bool,
) -> ChatGptStatusDto {
    let (state, reason) = if signing_in {
        ("signing_in", None)
    } else {
        match &status.state {
            SessionState::SignedOut => ("signed_out", None),
            SessionState::SignedIn => ("signed_in", None),
            SessionState::NotLoaded => (
                "unread",
                Some(ReasonDto {
                    code: "keychain_unread",
                    message: refusal.unwrap_or_else(|| {
                        "the saved ChatGPT sign-in has not been read from the keychain".into()
                    }),
                }),
            ),
            SessionState::PlanDisabled => (
                "plan_disabled",
                Some(ReasonDto {
                    code: "plan_usage_declined",
                    message: "ChatGPT plan usage was not allowed for edytlab. Turn it on to \
                              use your plan, or choose another provider."
                        .into(),
                }),
            ),
            SessionState::SessionEnded(why) => (
                "session_ended",
                Some(ReasonDto {
                    code: end_code(why),
                    message: why.to_string(),
                }),
            ),
        }
    };
    ChatGptStatusDto {
        state,
        // A person who signed out does not see the account they left.
        email: status
            .email
            .clone()
            .filter(|_| state != "signed_out" && state != "unavailable"),
        reason,
        token_storage: status
            .token_storage
            .as_ref()
            .map(|s| token_storage_dto(s, persistence)),
        pending_message,
    }
}

fn unavailable_dto() -> ChatGptStatusDto {
    ChatGptStatusDto {
        state: "unavailable",
        email: None,
        reason: Some(ReasonDto {
            code: "no_config_dir",
            message: "ChatGPT sign-in is not available: edytlab could not find a folder to \
                      keep it in."
                .into(),
        }),
        token_storage: None,
        pending_message: false,
    }
}

// ---------------------------------------------------------------------------
// Typed errors
// ---------------------------------------------------------------------------

/// A failed turn on the ChatGPT plan reaches the window as the command's error
/// text, as every failure does, and this is what makes it readable by code: the
/// text **starts with a marker line** the window parses and strips
/// (`parsePlanUsageError` in `lib/chat-errors.ts`; this format and that parser
/// change together, and `plan_usage_text_has_the_form_the_window_parses` pins
/// the Rust side):
///
/// ```text
/// [plan-usage code=usage_limit request-id=req_abc123] <the sentence to show>
/// ```
///
/// `code` is [`PlanUsageError::code`]: `usage_limit`, `not_eligible`,
/// `plan_disabled`, `session_ended`, `unavailable`, `unsupported_request` or
/// `refused`. `request-id` is OpenAI's, when the response had one and it is
/// plain; the sentence already carries it. Neither the marker nor the
/// sentence names a key, a status or any word `chat-errors.ts` reads as a
/// refused key, so the chat does not send the person to Settings to paste
/// one.
pub fn plan_usage_text(error: &PlanUsageError) -> String {
    let request = error.request_id().filter(|id| {
        !id.is_empty()
            && id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "._:-".contains(c))
    });
    match request {
        Some(id) => format!(
            "[plan-usage code={} request-id={}] {}",
            error.code(),
            id,
            error
        ),
        None => format!("[plan-usage code={}] {}", error.code(), error),
    }
}

/// Cuts text from OpenAI's callback down to something safe to show: no
/// control characters, one line, a bounded length.
fn shown(text: &str) -> String {
    let one_line: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let trimmed = one_line.split_whitespace().collect::<Vec<_>>().join(" ");
    trimmed.chars().take(200).collect()
}

/// The words for a sign-in that did not finish. The sentence for a browser
/// that came back with an error includes OpenAI's own description of it, so
/// that "this app is unavailable" or "your organisation has not enabled
/// access" is what the person reads (G6).
pub fn sign_in_error_text(error: &SignInError) -> String {
    match error {
        SignInError::Loopback(LoopbackError::AccessDenied {
            description: Some(d),
        }) if !shown(d).is_empty() => format!("access was denied: {}", shown(d)),
        SignInError::Loopback(LoopbackError::Authorization {
            error,
            description: Some(d),
        }) if !shown(d).is_empty() => {
            format!("the sign-in failed: {} ({})", shown(error), shown(d))
        }
        SignInError::Loopback(LoopbackError::Authorization { error, .. }) => {
            format!("the sign-in failed: {}", shown(error))
        }
        // These name the folder edytlab keeps its sign-in files in, and a
        // local path is not for the window: a fixed sentence, and the detail
        // in the log.
        SignInError::Lock(_) => {
            tracing::warn!(error = %error, "chatgpt: could not lock the sign-in files");
            "edytlab could not use its saved ChatGPT sign-in files: they are locked or cannot \
             be written. Close any other edytlab window that is signing in and try again."
                .to_string()
        }
        SignInError::Registration(why) => {
            tracing::warn!(error = %why, "chatgpt: the saved registration could not be used");
            match why {
                StoreError::Corrupt(_) => "edytlab's saved ChatGPT registration is damaged. Use \
                     Start over to register again."
                    .to_string(),
                StoreError::UnsupportedVersion { .. } => "edytlab's saved ChatGPT registration \
                     was written by a newer edytlab. Update edytlab, or use Start over to \
                     register again."
                    .to_string(),
                StoreError::Io { .. } => "edytlab could not read or write its saved ChatGPT \
                     registration."
                    .to_string(),
            }
        }
        other => other.to_string(),
    }
}

const CHATGPT_SETTINGS: &str = "ChatGPT settings, under Security and login, Sign in with ChatGPT";

fn revocation_word(revocation: &Revocation) -> &'static str {
    match revocation {
        Revocation::Confirmed => "confirmed",
        Revocation::NotConfirmed { .. } => "not_confirmed",
        Revocation::NothingToRevoke => "nothing_to_revoke",
    }
}

/// What the person should be told about a sign-out's loose ends.
fn sign_out_notes(report: &SignOutReport) -> Vec<String> {
    let mut notes = Vec::new();
    if let Revocation::NotConfirmed { reason } = &report.revocation {
        notes.push(format!(
            "edytlab could not confirm that OpenAI ended the sign-in ({}). To be sure, \
             disconnect edytlab in {CHATGPT_SETTINGS}.",
            shown(reason)
        ));
    }
    if let Err(why) = &report.removed_from_store {
        notes.push(format!(
            "The saved sign-in could not be removed from the keychain ({why})."
        ));
    }
    notes
}

fn join_notes(notes: Vec<String>) -> Option<String> {
    (!notes.is_empty()).then(|| notes.join(" "))
}

/// The error for a command that needs the session when this build has none.
pub fn unavailable_error() -> String {
    "ChatGPT sign-in is not available: edytlab could not find a folder to keep it in".into()
}

/// The error for something that needs a signed-in session, with why there is
/// none.
pub fn not_signed_in_error(status: &ChatGptStatusDto) -> String {
    match &status.reason {
        Some(reason) => format!(
            "not signed in with ChatGPT ({}); sign in with ChatGPT first",
            reason.message
        ),
        None => "not signed in with ChatGPT; sign in with ChatGPT first".into(),
    }
}

// ---------------------------------------------------------------------------
// The handle
// ---------------------------------------------------------------------------

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

type Notifier = Arc<dyn Fn(&ChatGptStatusDto) + Send + Sync>;
type PersistenceOf = Arc<dyn Fn() -> Option<Persistence> + Send + Sync>;

#[derive(Default)]
struct Hooks {
    persistence: Option<PersistenceOf>,
    refusal: Option<Refusal>,
    notifier: Option<Notifier>,
    /// Whether the model list is read after a sign-in and when an agent is
    /// built. On in the app; off in a state nothing installed into, so a test
    /// makes no request.
    refresh_models: bool,
    /// The folder the session keeps its files in. Its path is not for the
    /// window: anything shown is scrubbed of it.
    private_dir: Option<String>,
}

/// What the app remembers of the route, between agents and between turns.
#[derive(Default)]
struct Memory {
    /// The deepest rung any provider of this account has been on.
    rung: Option<ToolRung>,
    /// The reasoning levels of each model, from the last catalogue read.
    levels: HashMap<String, Vec<String>>,
    /// The provider the running agent was built with. Nothing but the agent's
    /// turn uses it; see the module documentation.
    agent: Option<Arc<ChatGptProvider>>,
    /// The account generation the running agent's provider has been brought
    /// to. When it is behind the current one, the account changed since: the
    /// provider starts again at its next turn boundary.
    agent_generation: u64,
}

/// A provider of its own for a request that runs beside a turn, and the
/// account generation it was made under.
struct Fork {
    provider: Arc<ChatGptProvider>,
    generation: u64,
}

fn deeper(a: Option<ToolRung>, b: ToolRung) -> Option<ToolRung> {
    match a {
        Some(a) if a.depth() >= b.depth() => Some(a),
        _ if b == ToolRung::NamespaceFull => a,
        _ => Some(b),
    }
}

struct Inner {
    session: Mutex<Option<Arc<ChatGptSession>>>,
    hooks: Mutex<Hooks>,
    memory: Mutex<Memory>,
    pending: Mutex<Option<PendingMessage>>,
    published: Mutex<Option<ChatGptStatusDto>>,
    refreshing_models: AtomicBool,
    /// See "Account generation" in the module documentation.
    generation: AtomicU64,
    /// A start over is running: the session counts it as a sign-in in
    /// progress (it takes the same guard), and the window must not read that
    /// as a browser being waited for.
    starting_over: AtomicBool,
}

/// The ChatGPT sign-in of one app: see the module documentation. Cloning is
/// cheap and shares everything.
#[derive(Clone)]
pub struct ChatGptHandle {
    inner: Arc<Inner>,
}

impl Default for ChatGptHandle {
    fn default() -> Self {
        Self {
            inner: Arc::new(Inner {
                session: Mutex::new(None),
                hooks: Mutex::new(Hooks::default()),
                memory: Mutex::new(Memory::default()),
                pending: Mutex::new(None),
                published: Mutex::new(None),
                refreshing_models: AtomicBool::new(false),
                generation: AtomicU64::new(0),
                starting_over: AtomicBool::new(false),
            }),
        }
    }
}

/// What a handle needs from the app around it.
pub struct Hooking {
    /// Where the refresh token landed (the store's), for the status.
    pub persistence: PersistenceOf,
    /// Why the keychain is not asked, after a startup it refused.
    pub refusal: Refusal,
    /// Whether to read the model list after a sign-in and on an agent build.
    pub refresh_models: bool,
    /// The folder the session keeps its files in (the app config folder), so
    /// no text shown to the window can carry its path.
    pub private_dir: Option<std::path::PathBuf>,
}

impl ChatGptHandle {
    /// Puts the opened session in. Once, at startup.
    pub fn install(&self, session: Arc<ChatGptSession>, hooking: Hooking) {
        *lock(&self.inner.session) = Some(session);
        let mut hooks = lock(&self.inner.hooks);
        hooks.persistence = Some(hooking.persistence);
        hooks.refusal = Some(hooking.refusal);
        hooks.refresh_models = hooking.refresh_models;
        hooks.private_dir = hooking
            .private_dir
            .map(|d| d.to_string_lossy().into_owned())
            .filter(|d| !d.is_empty());
    }

    /// Text that is going to the window, without the folder the session's
    /// files are in.
    fn scrubbed(&self, text: String) -> String {
        match &lock(&self.inner.hooks).private_dir {
            Some(dir) => text.replace(dir.as_str(), "the ChatGPT folder"),
            None => text,
        }
    }

    /// How many times the account has changed. See the module documentation.
    pub fn account_generation(&self) -> u64 {
        self.inner.generation.load(Ordering::SeqCst)
    }

    /// Where status changes are sent: the app's event.
    pub fn set_notifier(&self, notifier: impl Fn(&ChatGptStatusDto) + Send + Sync + 'static) {
        lock(&self.inner.hooks).notifier = Some(Arc::new(notifier));
    }

    /// The session, if this build opened one.
    pub fn session(&self) -> Option<Arc<ChatGptSession>> {
        lock(&self.inner.session).clone()
    }

    /// Whether the model list is read after a sign-in and on an agent build.
    pub fn refreshes_models(&self) -> bool {
        lock(&self.inner.hooks).refresh_models
    }

    fn refusal(&self) -> Option<String> {
        let refusal = lock(&self.inner.hooks).refusal.clone();
        refusal.and_then(|r| r())
    }

    // --- Status -------------------------------------------------------------------

    /// The status now, from what the session holds. No I/O.
    pub fn status(&self) -> ChatGptStatusDto {
        let Some(session) = self.session() else {
            return unavailable_dto();
        };
        let persistence = lock(&self.inner.hooks).persistence.clone();
        let persistence = persistence.and_then(|p| p());
        status_dto(
            &session.status(),
            session.is_signing_in() && !self.inner.starting_over.load(Ordering::SeqCst),
            persistence.as_ref(),
            self.refusal(),
            lock(&self.inner.pending).is_some(),
        )
    }

    /// Whether the session is signed in with plan usage granted. (It does not
    /// look at whether another sign-in is in progress: the session in use is
    /// the one that holds until that sign-in replaces it.)
    pub fn is_signed_in(&self) -> bool {
        self.session()
            .is_some_and(|s| s.status().state == SessionState::SignedIn)
    }

    /// The session, if it is signed in with plan usage granted.
    pub fn signed_in_session(&self) -> Option<Arc<ChatGptSession>> {
        self.session()
            .filter(|s| s.status().state == SessionState::SignedIn)
    }

    /// Reads the status, and sends it on the event if it is not what was sent
    /// last. Call it after anything that can have changed the session: a
    /// command, a turn (a refresh can end the session inside one), the
    /// startup restore.
    pub fn publish(&self) -> ChatGptStatusDto {
        let dto = self.status();
        let changed = {
            let mut published = lock(&self.inner.published);
            if published.as_ref() == Some(&dto) {
                false
            } else {
                *published = Some(dto.clone());
                true
            }
        };
        if changed {
            let notifier = lock(&self.inner.hooks).notifier.clone();
            if let Some(notify) = notifier {
                notify(&dto);
            }
        }
        dto
    }

    /// Reads the saved session's refresh token from the keychain, once, when
    /// there is one to read ([`SessionState::NotLoaded`]) and the keychain was
    /// not refused. Startup calls it; a failure leaves the state as it was.
    pub async fn restore(&self) {
        let Some(session) = self.session() else {
            return;
        };
        if session.status().state != SessionState::NotLoaded || self.refusal().is_some() {
            return;
        }
        if let Err(e) = session.restore().await {
            tracing::warn!(error = %e, "could not read the saved ChatGPT sign-in");
        }
        self.publish();
    }

    // --- Signing in and out -----------------------------------------------------------

    /// Signs in: `browser_opener` is given the authorization URL and opens the system
    /// browser at it. Answers when the sign-in is over. A cancel is not an
    /// error: it answers with the status as it then stands.
    ///
    /// The status is sent as `signing_in` once the browser is about to open
    /// and again when the sign-in ends, however it ends.
    pub async fn sign_in(
        &self,
        intent: SignInIntent,
        browser_opener: impl FnOnce(&str) -> Result<(), String>,
    ) -> Result<ChatGptStatusDto, String> {
        let session = self.session().ok_or_else(unavailable_error)?;
        let before = session.status().email;
        let this = self.clone();
        let result = session
            .sign_in(
                move |url: &str| {
                    this.publish();
                    browser_opener(url)
                },
                intent,
            )
            .await;
        match result {
            Ok(status) => {
                if intent == SignInIntent::AnotherAccount || status.email != before {
                    self.account_changed();
                }
                Ok(self.publish())
            }
            Err(SignInError::Loopback(LoopbackError::Cancelled)) => Ok(self.publish()),
            Err(e) => {
                self.publish();
                Err(self.scrubbed(sign_in_error_text(&e)))
            }
        }
    }

    /// Ends the sign-in in progress, if there is one.
    pub fn cancel_sign_in(&self) {
        if let Some(session) = self.session() {
            session.cancel_sign_in();
        }
    }

    /// Signs out: the refresh token is revoked and removed, and what was
    /// learned about the account's route and any kept message go with it (the
    /// account generation moves on, so a turn that is still running cannot
    /// keep a message for whoever signs in next).
    pub async fn sign_out(&self) -> Result<ChatGptEndDto, String> {
        let session = self.session().ok_or_else(unavailable_error)?;
        let report = session.sign_out().await;
        self.account_changed();
        Ok(ChatGptEndDto {
            status: self.publish(),
            revocation: revocation_word(&report.revocation),
            token_removed: report.removed_from_store.is_ok(),
            registration_forgotten: None,
            notice: self.join_notes(sign_out_notes(&report)),
        })
    }

    /// Starts over (G13): signs out, then forgets the registered client, so
    /// the next sign-in registers edytlab again. The old entry may remain in
    /// ChatGPT's settings, which the notice says.
    ///
    /// While it runs the status is whatever the session is, not `signing_in`:
    /// the session takes the sign-in guard to keep a sign-in from starting
    /// meanwhile, and no browser is being waited for.
    pub async fn forget_registration(&self) -> Result<ChatGptEndDto, String> {
        let session = self.session().ok_or_else(unavailable_error)?;
        let _starting_over = StartingOver::begin(&self.inner.starting_over);
        let report = session.forget_registration().await.map_err(|e| match e {
            ForgetError::SignInInProgress => {
                "A ChatGPT sign-in is in progress. Cancel it first, then start over.".to_string()
            }
        })?;
        self.account_changed();
        let forgotten = report.registration_removed.is_ok();
        let mut notes = sign_out_notes(&report.sign_out);
        match &report.registration_removed {
            Ok(()) => notes.push(format!(
                "edytlab has forgotten its registration, and the next sign-in registers it \
                 again. The old entry may still be listed in {CHATGPT_SETTINGS}; you can \
                 disconnect it there."
            )),
            Err(why) => {
                // The detail names the folder; the log has it.
                tracing::warn!(error = %why, "chatgpt: could not forget the registration");
                notes.push("edytlab could not update its saved ChatGPT registration.".to_string())
            }
        }
        Ok(ChatGptEndDto {
            status: self.publish(),
            revocation: revocation_word(&report.sign_out.revocation),
            token_removed: report.sign_out.removed_from_store.is_ok(),
            registration_forgotten: Some(forgotten),
            notice: self.join_notes(notes),
        })
    }

    fn join_notes(&self, notes: Vec<String>) -> Option<String> {
        join_notes(notes).map(|n| self.scrubbed(n))
    }

    // --- The message a dead session could not send (G9) --------------------------------

    /// Keeps `message` for the window to offer to send again, if it still
    /// can be: the account has not changed since the turn began under
    /// `generation`, and the session **ended** (it was not signed out of: a
    /// person who signed out chose to leave). Says whether it was kept.
    pub fn keep_pending(&self, generation: u64, message: PendingMessage) -> bool {
        let ended = self
            .session()
            .is_some_and(|s| matches!(s.status().state, SessionState::SessionEnded(_)));
        {
            let mut pending = lock(&self.inner.pending);
            // Checked with the lock held: `account_changed` moves the
            // generation first and clears the message after, so a message
            // set here is either cleared by it or refused by this.
            if !ended || generation != self.account_generation() {
                return false;
            }
            *pending = Some(message);
        }
        self.publish();
        true
    }

    /// Takes the kept message, if there is one, and forgets it.
    pub fn take_pending(&self) -> Option<PendingMessage> {
        let taken = lock(&self.inner.pending).take();
        if taken.is_some() {
            self.publish();
        }
        taken
    }

    /// Forgets the kept message, and says so on the status if there was one
    /// (the Continue card has nothing to send any more): a new message
    /// supersedes it.
    pub fn clear_pending(&self) {
        let had = lock(&self.inner.pending).take().is_some();
        if had {
            self.publish();
        }
    }

    // --- What the agent's provider has learned --------------------------------------

    /// A provider for a new agent, starting where the account's route is
    /// known to stand: the rung and the reasoning levels the app remembers
    /// (nothing, after an account change). The agent's alone; see the module
    /// documentation.
    pub fn new_agent_provider(&self) -> Arc<ChatGptProvider> {
        let mut memory = lock(&self.inner.memory);
        harvest_locked(&mut memory, self.account_generation());
        let provider = Arc::new(provider_from(&memory));
        memory.agent = Some(Arc::clone(&provider));
        memory.agent_generation = self.account_generation();
        provider
    }

    /// An agent that is not a ChatGPT one replaced the running one: what it
    /// learned is kept, and it is no longer the agent's provider.
    pub fn agent_replaced_by_another_provider(&self) {
        let mut memory = lock(&self.inner.memory);
        harvest_locked(&mut memory, self.account_generation());
        memory.agent = None;
    }

    /// Whether the running agent was built on the ChatGPT plan.
    pub fn has_agent(&self) -> bool {
        lock(&self.inner.memory).agent.is_some()
    }

    /// The agent's turn has ended: the rung it stepped to is kept.
    pub fn end_of_turn(&self) {
        harvest_locked(&mut lock(&self.inner.memory), self.account_generation());
    }

    /// A turn is about to start, with the agent's lock held and none running:
    /// the running agent's provider takes on what was learned since (a rung a
    /// copy found, the reasoning levels), after starting again if the account
    /// changed since it was built or last brought up to date. This is the
    /// only time anything reaches the agent's provider.
    pub fn turn_boundary(&self) {
        let mut memory = lock(&self.inner.memory);
        let Some(agent) = memory.agent.clone() else {
            return;
        };
        let generation = self.account_generation();
        if memory.agent_generation != generation {
            agent.reset_adaptations();
            agent.adopt_levels(HashMap::new());
            memory.agent_generation = generation;
        }
        if let Some(rung) = memory.rung {
            agent.adopt_rung(rung);
        }
        agent.adopt_levels(memory.levels.clone());
    }

    /// The account is not the one the route was learned on (another account,
    /// a sign-out, a start over): what was learned is dropped, for the next
    /// agent now and for the running one at its next turn boundary; copies
    /// that began under the old account teach nothing; and a kept message
    /// goes, so it cannot be offered to whoever signs in next.
    pub fn account_changed(&self) {
        {
            let mut memory = lock(&self.inner.memory);
            self.inner.generation.fetch_add(1, Ordering::SeqCst);
            memory.rung = None;
            memory.levels.clear();
        }
        self.clear_pending();
    }

    /// What the app remembers, for a test.
    #[cfg(test)]
    pub(crate) fn remembered_rung(&self) -> Option<ToolRung> {
        lock(&self.inner.memory).rung
    }

    /// The provider the running agent was built with, for a test.
    #[cfg(test)]
    pub(crate) fn agent_provider(&self) -> Option<Arc<ChatGptProvider>> {
        lock(&self.inner.memory).agent.clone()
    }

    /// A provider of its own for a request that runs beside a turn (Test, the
    /// model list), made from what the app remembers and nothing the running
    /// agent's provider holds right now, with the account generation it was
    /// made under.
    fn fork(&self) -> Fork {
        let memory = lock(&self.inner.memory);
        Fork {
            provider: Arc::new(provider_from(&memory)),
            // Read with `memory` held, as `account_changed` moves it.
            generation: self.account_generation(),
        }
    }

    /// Keeps what a copy learned: a deeper rung than the one remembered, and
    /// the reasoning levels it read, if it read any. The running agent has
    /// them from its next turn boundary on. A copy made under an older
    /// account generation learned that account's route and is dropped.
    fn learned(&self, fork: &Fork) {
        let mut memory = lock(&self.inner.memory);
        if fork.generation != self.account_generation() {
            return;
        }
        memory.rung = deeper(memory.rung, fork.provider.tool_rung());
        let levels = fork.provider.levels_snapshot();
        if !levels.is_empty() {
            memory.levels = levels;
        }
    }

    // --- Requests that run beside a turn -------------------------------------------------

    /// The account's models, read by `read_catalogue` with a config built on a copy of
    /// the provider ([`Self::fork`]). Keeps what the copy learned.
    pub async fn list_models<F, Fut>(&self, read_catalogue: F) -> Result<ai::Catalogue, String>
    where
        F: FnOnce(ai::LlmConfig) -> Fut,
        Fut: Future<Output = Result<ai::Catalogue, String>>,
    {
        let session = self
            .signed_in_session()
            .ok_or_else(|| self.not_signed_in())?;
        let fork = self.fork();
        let cfg = ai::LlmConfig::with_credential(
            fork.provider.clone() as Arc<dyn ai::LlmProvider>,
            ai::Credential::ChatGpt(session),
        );
        let result = read_catalogue(cfg).await;
        if let Ok(catalogue) = &result {
            // The levels each model takes, on the copy. (The real fetch has
            // told it already; this is what makes them reach `learned`
            // whatever `read_catalogue` is.)
            ai::LlmProvider::note_catalogue(fork.provider.as_ref(), &catalogue.models);
        }
        self.learned(&fork);
        // A refresh inside the request can end the session.
        self.publish();
        result
    }

    /// TEST: `probe_with` is given a config built on a copy of the provider and the
    /// model to try. Keeps what the copy learned.
    pub async fn probe<F, Fut>(
        &self,
        model: Option<String>,
        probe_with: F,
    ) -> Result<ai::validate::ProbeReport, String>
    where
        F: FnOnce(ai::LlmConfig, Option<String>) -> Fut,
        Fut: Future<Output = Result<ai::validate::ProbeReport, String>>,
    {
        let session = self
            .signed_in_session()
            .ok_or_else(|| self.not_signed_in())?;
        let fork = self.fork();
        let cfg = ai::LlmConfig::with_credential(
            fork.provider.clone() as Arc<dyn ai::LlmProvider>,
            ai::Credential::ChatGpt(session),
        );
        let result = probe_with(cfg, model).await;
        self.learned(&fork);
        self.publish();
        result
    }

    /// Reads the model list in the background, once at a time, unless this
    /// handle does not read it ([`Hooking::refresh_models`]). The levels are
    /// what the reasoning picker and a request's effort need.
    pub async fn refresh_models_with<F, Fut>(&self, read_catalogue: F)
    where
        F: FnOnce(ai::LlmConfig) -> Fut,
        Fut: Future<Output = Result<ai::Catalogue, String>>,
    {
        if !self.refreshes_models() || !self.is_signed_in() {
            return;
        }
        if self.inner.refreshing_models.swap(true, Ordering::SeqCst) {
            return;
        }
        if let Err(e) = self.list_models(read_catalogue).await {
            tracing::warn!(error = %e, "could not read the ChatGPT model list");
        }
        self.inner.refreshing_models.store(false, Ordering::SeqCst);
    }

    fn not_signed_in(&self) -> String {
        if self.session().is_none() {
            return unavailable_error();
        }
        not_signed_in_error(&self.status())
    }
}

/// Marks a start over as running, and unmarks it however the call ends.
struct StartingOver<'a>(&'a AtomicBool);

impl<'a> StartingOver<'a> {
    fn begin(flag: &'a AtomicBool) -> Self {
        flag.store(true, Ordering::SeqCst);
        Self(flag)
    }
}

impl Drop for StartingOver<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

/// A provider made from what is remembered.
fn provider_from(memory: &Memory) -> ChatGptProvider {
    let provider = ChatGptProvider::default();
    let provider = match memory.rung {
        Some(rung) => provider.with_tool_rung(rung),
        None => provider,
    };
    provider.adopt_levels(memory.levels.clone());
    provider
}

/// Reads the running agent's rung into memory (the ladder is walked in one
/// direction, so what it is on now is never shallower than what was kept),
/// unless the account changed since the agent's provider was brought up to
/// date: what it learned is the old account's.
fn harvest_locked(memory: &mut Memory, generation: u64) {
    if memory.agent_generation != generation {
        return;
    }
    if let Some(agent) = &memory.agent {
        memory.rung = deeper(memory.rung, agent.tool_rung());
    }
}

/// The model list as the app reads it in the background and for Settings:
/// `GET /v1/models` with the copy's bearer.
pub async fn read_models(cfg: ai::LlmConfig) -> Result<ai::Catalogue, String> {
    ai::models::list_chatgpt_models(&cfg, &reqwest::Client::new()).await
}

/// TEST for the ChatGPT plan: one streamed tool call.
pub async fn run_probe(
    cfg: ai::LlmConfig,
    model: Option<String>,
) -> Result<ai::validate::ProbeReport, String> {
    ai::validate::probe_chatgpt(&cfg, model.as_deref()).await
}

#[cfg(test)]
pub(crate) mod test_support {
    //! A signed-in session without a network: the registration a sign-in
    //! would have written, and a token in a store that lives in memory.

    use std::path::Path;

    use ai::chatgpt::spec::{HostId, ISSUER, PLAN_USAGE_SCOPE};
    use ai::chatgpt::store::{
        ClientRecord, IdTokenReference, Registration, RegistrationFile, REFRESH_TOKEN_SLOT,
    };
    use ai::chatgpt::SessionConfig;

    use super::*;

    /// A refresh-token store in memory.
    #[derive(Default)]
    pub struct MemoryStore {
        pub token: Mutex<Option<String>>,
    }

    impl RefreshTokenStore for MemoryStore {
        fn load(&self) -> Result<Option<SecretString>, SecretStoreError> {
            Ok(lock(&self.token).clone().map(SecretString::new))
        }
        fn save(&self, token: &SecretString) -> Result<(), SecretStoreError> {
            *lock(&self.token) = Some(token.expose_secret().to_string());
            Ok(())
        }
        fn delete(&self) -> Result<(), SecretStoreError> {
            *lock(&self.token) = None;
            Ok(())
        }
    }

    /// Writes the registration of a signed-in account in `dir` and opens a
    /// session over it, with a token in `store`.
    pub fn open_session(
        dir: &Path,
        store: Arc<dyn RefreshTokenStore>,
        signed_in: bool,
    ) -> Arc<ChatGptSession> {
        assert_eq!(REFRESH_TOKEN_SLOT, "chatgpt_refresh_token");
        if signed_in {
            let mut registration = Registration::new(HostId::generate().unwrap());
            registration.active_client_id = Some("oaiapp_test".to_string());
            registration.clients.push(ClientRecord {
                client_id: "oaiapp_test".to_string(),
                issuer: ISSUER.to_string(),
                subject: Some("user-test".to_string()),
                email: Some("person@example.test".to_string()),
                scopes: vec!["openid".to_string(), PLAN_USAGE_SCOPE.to_string()],
                id_token_reference: IdTokenReference::default(),
                saved_at: 1,
                signed_out: false,
            });
            RegistrationFile::new(dir.join("chatgpt"))
                .save(&registration)
                .unwrap();
            store.save(&SecretString::new("RT-test")).unwrap();
        }
        Arc::new(ChatGptSession::open(SessionConfig::new(dir), store).unwrap())
    }

    /// A handle whose session has **ended**: another window signed out, so
    /// the token is gone from the store and the next bearer says so (offline).
    pub fn ended_handle(dir: &Path) -> (ChatGptHandle, Arc<MemoryStore>) {
        let (handle, store) = signed_in_handle(dir);
        *lock(&store.token) = None;
        let session = handle.session().expect("a session");
        let err = tauri::async_runtime::block_on(session.bearer()).expect_err("it ended");
        assert!(
            matches!(err, ai::chatgpt::BearerError::SessionEnded(_)),
            "{err:?}"
        );
        (handle, store)
    }

    /// A handle with a signed-in session installed, as startup leaves it.
    pub fn signed_in_handle(dir: &Path) -> (ChatGptHandle, Arc<MemoryStore>) {
        let store = Arc::new(MemoryStore::default());
        let session = open_session(dir, store.clone(), true);
        tauri::async_runtime::block_on(session.restore()).unwrap();
        let handle = ChatGptHandle::default();
        handle.install(
            session,
            Hooking {
                persistence: Arc::new(|| None),
                refusal: Arc::new(|| None),
                refresh_models: false,
                private_dir: None,
            },
        );
        (handle, store)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;

    use ai::chatgpt::PlanUsageKind;
    use ai::{ModelInfo, ProviderFailure};
    use tempfile::tempdir;

    use super::test_support::*;
    use super::*;

    // ---- The keychain slot ---------------------------------------------------

    #[derive(Default)]
    struct FakeSlot {
        token: Mutex<Option<String>>,
        loads: AtomicUsize,
        durable: bool,
        fail_save: Mutex<Option<fn() -> keyring::Error>>,
    }

    impl FakeSlot {
        fn durable() -> Arc<Self> {
            Arc::new(Self {
                durable: true,
                ..Default::default()
            })
        }
        fn kernel_keyring() -> Arc<Self> {
            Arc::new(Self::default())
        }
    }

    impl TokenSlot for Arc<FakeSlot> {
        fn load(&self) -> Result<Option<String>, KeychainError> {
            self.loads.fetch_add(1, Ordering::SeqCst);
            Ok(lock(&self.token).clone())
        }
        fn save(&self, token: &str) -> Result<Persistence, keyring::Error> {
            if let Some(fail) = *lock(&self.fail_save) {
                return Err(fail());
            }
            *lock(&self.token) = Some(token.to_string());
            Ok(self.persistence())
        }
        fn delete(&self) -> Result<(), keyring::Error> {
            *lock(&self.token) = None;
            Ok(())
        }
        fn persistence(&self) -> Persistence {
            if self.durable {
                Persistence::Durable
            } else {
                Persistence::SessionOnly {
                    reason: "no Secret Service".into(),
                }
            }
        }
    }

    fn never_refused() -> Refusal {
        Arc::new(|| None)
    }

    #[test]
    fn the_refresh_token_round_trips_through_its_slot() {
        let slot = FakeSlot::durable();
        let store = KeychainTokenStore::new(slot.clone(), never_refused());
        assert!(store.load().unwrap().is_none());
        store.save(&SecretString::new("RT-1")).unwrap();
        assert_eq!(store.load().unwrap().unwrap().expose_secret(), "RT-1");
        assert_eq!(store.persistence(), Some(Persistence::Durable));
        store.delete().unwrap();
        assert!(store.load().unwrap().is_none());
        assert_eq!(store.persistence(), None, "nothing is stored any more");
    }

    /// The Linux kernel-keyring fallback (#394) is reported, not hidden.
    #[test]
    fn a_token_kept_in_the_kernel_keyring_is_not_persistent() {
        let slot = FakeSlot::kernel_keyring();
        let store = KeychainTokenStore::new(slot, never_refused());
        store.save(&SecretString::new("RT-1")).unwrap();
        let dto = token_storage_dto(&TokenStorage::Stored, store.persistence().as_ref());
        assert_eq!(dto.kind, "kernel_keyring");
        assert!(!dto.persistent);
        assert_eq!(dto.reason.as_deref(), Some("no Secret Service"));

        let durable = token_storage_dto(&TokenStorage::Stored, Some(&Persistence::Durable));
        assert!(durable.persistent && durable.kind == "keychain" && durable.reason.is_none());

        let memory = token_storage_dto(
            &TokenStorage::MemoryOnly(SecretStoreError::TooLong),
            Some(&Persistence::Durable),
        );
        assert_eq!(memory.kind, "memory");
        assert!(!memory.persistent);
        assert!(memory.reason.unwrap().contains("too long for this system"));
    }

    /// A startup the keychain refused: no read, so no unlock prompt for a
    /// slot nobody is waiting on (#497), and the reason is the note every
    /// other read gives.
    #[test]
    fn a_refused_keychain_is_not_read_for_the_refresh_token() {
        let slot = FakeSlot::durable();
        *lock(&slot.token) = Some("RT-1".into());
        let refused = Arc::new(AtomicBool::new(true));
        let flag = refused.clone();
        let store = KeychainTokenStore::new(
            slot.clone(),
            Arc::new(move || {
                flag.load(Ordering::SeqCst)
                    .then(|| "the keychain refused access at launch".to_string())
            }),
        );
        let err = store.load().expect_err("refused");
        assert_eq!(
            err,
            SecretStoreError::Unavailable("the keychain refused access at launch".into())
        );
        assert_eq!(
            slot.loads.load(Ordering::SeqCst),
            0,
            "the slot was not read"
        );
        // A save is the user's own act and goes through; so does a read once
        // the refusal is lifted.
        store.save(&SecretString::new("RT-2")).unwrap();
        refused.store(false, Ordering::SeqCst);
        assert_eq!(store.load().unwrap().unwrap().expose_secret(), "RT-2");
    }

    #[test]
    fn a_token_too_long_for_the_keychain_is_reported_as_such_and_never_echoed() {
        let slot = FakeSlot::durable();
        *lock(&slot.fail_save) = Some(|| keyring::Error::TooLong("password".into(), 2560));
        let store = KeychainTokenStore::new(slot, never_refused());
        let err = store
            .save(&SecretString::new("RT-secret-value"))
            .expect_err("too long");
        assert_eq!(err, SecretStoreError::TooLong);
        assert!(!format!("{err:?} {err}").contains("RT-secret-value"));
    }

    // ---- The status the window reads ----------------------------------------------------

    fn status(
        state: SessionState,
        email: Option<&str>,
        storage: Option<TokenStorage>,
    ) -> ChatGptStatus {
        ChatGptStatus {
            state,
            email: email.map(str::to_string),
            token_storage: storage,
        }
    }

    #[test]
    fn every_state_has_a_word_and_a_reason_where_there_is_one() {
        let dto = |s: &ChatGptStatus, signing: bool| status_dto(s, signing, None, None, false);
        let signed_out = dto(
            &status(SessionState::SignedOut, Some("a@b.test"), None),
            false,
        );
        assert_eq!(signed_out.state, "signed_out");
        assert_eq!(signed_out.email, None, "the account left is not shown");
        assert!(signed_out.reason.is_none() && signed_out.token_storage.is_none());

        let signed_in = dto(
            &status(
                SessionState::SignedIn,
                Some("a@b.test"),
                Some(TokenStorage::Stored),
            ),
            false,
        );
        assert_eq!(signed_in.state, "signed_in");
        assert_eq!(signed_in.email.as_deref(), Some("a@b.test"));
        assert!(signed_in.token_storage.unwrap().persistent);

        let declined = dto(
            &status(SessionState::PlanDisabled, None, Some(TokenStorage::Stored)),
            false,
        );
        assert_eq!(declined.state, "plan_disabled");
        assert_eq!(declined.reason.unwrap().code, "plan_usage_declined");

        for (why, code) in [
            (
                EndReason::RefreshTokenUnusable {
                    code: "invalid_grant".into(),
                },
                "refresh_token_unusable",
            ),
            (
                EndReason::IdTokenRejected { reason: "x".into() },
                "id_token_rejected",
            ),
            (EndReason::NoRefreshToken, "no_refresh_token"),
            (EndReason::NotStored, "not_stored"),
            (EndReason::SignedOutElsewhere, "signed_out_elsewhere"),
            (EndReason::ReplacedElsewhere, "replaced_elsewhere"),
        ] {
            let ended = dto(
                &status(
                    SessionState::SessionEnded(why.clone()),
                    Some("a@b.test"),
                    None,
                ),
                false,
            );
            assert_eq!(ended.state, "session_ended");
            let reason = ended.reason.unwrap();
            assert_eq!(reason.code, code);
            assert_eq!(reason.message, why.to_string());
            assert_eq!(ended.email.as_deref(), Some("a@b.test"));
        }

        // Signing in wins over what was there before.
        let signing = dto(&status(SessionState::SignedOut, None, None), true);
        assert_eq!(signing.state, "signing_in");

        // A saved sign-in the keychain was not asked for.
        let unread = status_dto(
            &status(SessionState::NotLoaded, None, None),
            false,
            None,
            Some("the keychain refused access at launch".into()),
            false,
        );
        assert_eq!(unread.state, "unread");
        let reason = unread.reason.unwrap();
        assert_eq!(reason.code, "keychain_unread");
        assert!(
            reason.message.contains("refused access"),
            "{}",
            reason.message
        );

        assert_eq!(unavailable_dto().state, "unavailable");
    }

    #[test]
    fn the_status_serialises_in_the_shape_the_bridge_types_and_holds_no_token() {
        let dto = status_dto(
            &status(
                SessionState::SignedIn,
                Some("a@b.test"),
                Some(TokenStorage::MemoryOnly(SecretStoreError::TooLong)),
            ),
            false,
            None,
            None,
            true,
        );
        let json = serde_json::to_value(&dto).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "state": "signed_in",
                "email": "a@b.test",
                "reason": null,
                "tokenStorage": {
                    "kind": "memory",
                    "persistent": false,
                    "reason": "the sign-in is too long for this system's credential store; if other keys are saved, removing one you do not use may make room"
                },
                "pendingMessage": true
            })
        );
        // The debug form leaves the email out.
        assert!(!format!("{dto:?}").contains("a@b.test"));
    }

    // ---- The handle: events ------------------------------------------------------------

    #[test]
    fn a_status_change_is_sent_once_and_an_unchanged_one_is_not() {
        let dir = tempdir().unwrap();
        let (handle, _store) = ended_handle(dir.path());
        let sent: Arc<Mutex<Vec<ChatGptStatusDto>>> = Arc::default();
        let log = sent.clone();
        handle.set_notifier(move |dto| lock(&log).push(dto.clone()));

        handle.publish();
        handle.publish();
        assert_eq!(lock(&sent).len(), 1, "the same status is not sent twice");
        assert_eq!(lock(&sent)[0].state, "session_ended");

        // A kept message changes what the window reads.
        assert!(handle.keep_pending(handle.account_generation(), pending("cut the intro")));
        assert_eq!(lock(&sent).len(), 2);
        assert!(lock(&sent)[1].pending_message);
        assert!(handle.take_pending().is_some());
        assert_eq!(lock(&sent).len(), 3);
        assert!(!lock(&sent)[2].pending_message);
        assert!(handle.take_pending().is_none());
        assert_eq!(lock(&sent).len(), 3, "nothing was taken, nothing changed");
    }

    #[test]
    fn a_handle_with_no_session_is_unavailable_and_says_so() {
        let handle = ChatGptHandle::default();
        assert_eq!(handle.status().state, "unavailable");
        assert!(!handle.is_signed_in());
        let err = tauri::async_runtime::block_on(handle.sign_in(SignInIntent::SignIn, |_| Ok(())))
            .expect_err("no session");
        assert!(err.contains("not available"), "{err}");
        assert!(tauri::async_runtime::block_on(handle.sign_out()).is_err());
        assert!(tauri::async_runtime::block_on(handle.forget_registration()).is_err());
        handle.cancel_sign_in();
    }

    #[test]
    fn a_saved_session_is_read_once_and_not_after_a_refusal() {
        let dir = tempdir().unwrap();
        let store = Arc::new(MemoryStore::default());
        let session = open_session(dir.path(), store, true);
        assert_eq!(session.status().state, SessionState::NotLoaded);
        let refused = Arc::new(AtomicBool::new(true));
        let flag = refused.clone();
        let handle = ChatGptHandle::default();
        handle.install(
            session,
            Hooking {
                persistence: Arc::new(|| None),
                refusal: Arc::new(move || flag.load(Ordering::SeqCst).then(|| "refused".into())),
                refresh_models: false,
                private_dir: None,
            },
        );
        tauri::async_runtime::block_on(handle.restore());
        assert_eq!(
            handle.status().state,
            "unread",
            "a refusal is not read through"
        );
        assert_eq!(handle.status().reason.unwrap().message, "refused");

        refused.store(false, Ordering::SeqCst);
        tauri::async_runtime::block_on(handle.restore());
        assert_eq!(handle.status().state, "signed_in");
    }

    // ---- Finding 7: Test and the model list do not touch the agent's provider -------------

    fn tools_refused() -> String {
        r#"{"error":{"param":"tools","message":"no"}}"#.to_string()
    }

    fn catalogue(levels: &[&str]) -> ai::Catalogue {
        ai::Catalogue {
            models: vec![ModelInfo {
                id: "gpt-x".into(),
                display_name: "gpt-x".into(),
                reasoning_levels: levels.iter().map(|l| l.to_string()).collect(),
                ..ModelInfo::default()
            }],
            source: ai::CatalogueSource::Live,
        }
    }

    fn request<'a>(
        messages: &'a [ai::anthropic::Message],
        effort: Option<ai::Effort>,
    ) -> ai::anthropic::MessagesRequest<'a> {
        ai::anthropic::MessagesRequest {
            model: "gpt-x",
            max_tokens: 1,
            system: Vec::new(),
            messages,
            tools: None,
            tool_choice: None,
            stream: true,
            output_config: effort.map(|e| ai::anthropic::OutputConfig { effort: e }),
        }
    }

    /// The bug it pins (finding 7): the model list ran on the agent's own
    /// provider, so the catalogue replaced the levels a turn's requests read
    /// their effort from, and a copy's ladder step moved the turn's tools,
    /// under a turn in flight.
    #[test]
    fn a_model_list_during_a_turn_does_not_change_that_turns_tools_or_reasoning_layout() {
        let dir = tempdir().unwrap();
        let (handle, _store) = signed_in_handle(dir.path());
        let agent = handle.new_agent_provider();
        let agents_levels = [ModelInfo {
            id: "gpt-x".into(),
            display_name: "gpt-x".into(),
            reasoning_levels: vec!["low".into(), "high".into()],
            ..ModelInfo::default()
        }];
        ai::LlmProvider::note_catalogue(agent.as_ref(), &agents_levels);
        let messages: Vec<ai::anthropic::Message> = Vec::new();
        let before = ai::LlmProvider::serialize_request(
            agent.as_ref(),
            &request(&messages, Some(ai::Effort::High)),
        );

        // The model list runs during the turn. On its own provider it steps
        // the ladder, reads another catalogue and builds a request.
        let listed = tauri::async_runtime::block_on(handle.list_models(|cfg| async move {
            let failure = ProviderFailure::Http {
                status: 400,
                body: &tools_refused(),
                request_id: None,
            };
            assert!(cfg.provider.adapt_after_refusal(&failure).is_some());
            let messages: Vec<ai::anthropic::Message> = Vec::new();
            cfg.provider.serialize_request(&request(&messages, None));
            cfg.provider.note_catalogue(&catalogue(&["max"]).models);
            Ok(catalogue(&["max"]))
        }))
        .expect("listed");
        assert_eq!(listed.models.len(), 1);

        // The turn's provider is as it was.
        assert_eq!(agent.tool_rung(), ToolRung::NamespaceFull);
        assert_eq!(agent.reasoning_levels("gpt-x"), ["low", "high"]);
        assert_eq!(
            ai::LlmProvider::serialize_request(
                agent.as_ref(),
                &request(&messages, Some(ai::Effort::High))
            ),
            before,
            "the next step of the turn is the same request"
        );
        assert_eq!(ai::LlmProvider::tool_set(agent.as_ref()), ai::ToolSet::Full);

        // What the copy learned is kept, on purpose, and reaches the agent at
        // the next turn boundary.
        assert_eq!(handle.remembered_rung(), Some(ToolRung::NamespaceSlim));
        handle.turn_boundary();
        assert_eq!(agent.tool_rung(), ToolRung::NamespaceSlim);
        assert_eq!(agent.reasoning_levels("gpt-x"), ["max"]);

        // And an agent built later starts there.
        let next = handle.new_agent_provider();
        assert_eq!(next.tool_rung(), ToolRung::NamespaceSlim);
        assert_eq!(next.reasoning_levels("gpt-x"), ["max"]);
    }

    #[test]
    fn test_runs_on_a_copy_too_and_a_rung_the_agent_found_is_carried_to_the_next_agent() {
        let dir = tempdir().unwrap();
        let (handle, _store) = signed_in_handle(dir.path());
        let agent = handle.new_agent_provider();

        // Test steps its own ladder; the agent's provider does not move.
        let report = tauri::async_runtime::block_on(handle.probe(
            Some("gpt-x".into()),
            |cfg, model| async move {
                assert_eq!(model.as_deref(), Some("gpt-x"));
                let failure = ProviderFailure::Http {
                    status: 400,
                    body: &tools_refused(),
                    request_id: None,
                };
                cfg.provider.adapt_after_refusal(&failure);
                cfg.provider.adapt_after_refusal(&failure);
                Ok(ai::validate::ProbeReport {
                    model: "gpt-x".into(),
                    tools_ok: true,
                    detail: None,
                    model_unavailable: false,
                })
            },
        ))
        .expect("probed");
        assert!(report.tools_ok);
        assert_eq!(agent.tool_rung(), ToolRung::NamespaceFull);
        assert_eq!(handle.remembered_rung(), Some(ToolRung::AdditionalTools));

        // The agent finds a deeper rung of its own in a turn: kept at the end
        // of the turn, so a rebuild carries it (tool_rung across rebuild_agent).
        let failure = ProviderFailure::Http {
            status: 400,
            body: &tools_refused(),
            request_id: None,
        };
        agent.adopt_rung(ToolRung::AdditionalTools);
        ai::LlmProvider::adapt_after_refusal(agent.as_ref(), &failure);
        assert_eq!(agent.tool_rung(), ToolRung::Flat);
        handle.end_of_turn();
        assert_eq!(handle.remembered_rung(), Some(ToolRung::Flat));
        assert_eq!(handle.new_agent_provider().tool_rung(), ToolRung::Flat);
    }

    #[test]
    fn a_copy_that_read_no_levels_does_not_wipe_the_ones_known() {
        let dir = tempdir().unwrap();
        let (handle, _store) = signed_in_handle(dir.path());
        let _ = tauri::async_runtime::block_on(
            handle.list_models(|_| async { Ok(catalogue(&["low"])) }),
        );
        // A list that failed learned nothing.
        let failed =
            tauri::async_runtime::block_on(handle.list_models(|_| async {
                Err::<ai::Catalogue, _>("the service is down".to_string())
            }));
        assert!(failed.is_err());
        assert_eq!(
            handle.new_agent_provider().reasoning_levels("gpt-x"),
            ["low"]
        );
    }

    #[test]
    fn an_account_change_starts_the_route_over_for_the_next_agent_and_the_running_one() {
        let dir = tempdir().unwrap();
        let (handle, _store) = signed_in_handle(dir.path());
        let agent = handle.new_agent_provider();
        agent.adopt_rung(ToolRung::Flat);
        agent.adopt_levels(HashMap::from([(
            "gpt-x".to_string(),
            vec!["high".to_string()],
        )]));
        handle.end_of_turn();
        assert_eq!(handle.remembered_rung(), Some(ToolRung::Flat));

        handle.account_changed();
        assert_eq!(handle.remembered_rung(), None);
        // The running agent is not touched until a turn boundary.
        assert_eq!(agent.tool_rung(), ToolRung::Flat);
        handle.turn_boundary();
        assert_eq!(agent.tool_rung(), ToolRung::NamespaceFull);
        assert!(agent.reasoning_levels("gpt-x").is_empty());
        // And a new agent starts fresh.
        let fresh = handle.new_agent_provider();
        assert_eq!(fresh.tool_rung(), ToolRung::NamespaceFull);
    }

    /// The bug: with an agent running, `learned` returned early whenever a
    /// reset was waiting for the next turn, so the catalogue read **for the
    /// new account** was thrown away and no effort was ever sent for it.
    #[test]
    fn a_catalogue_read_after_an_account_change_reaches_the_running_agent() {
        let dir = tempdir().unwrap();
        let (handle, _store) = signed_in_handle(dir.path());
        let agent = handle.new_agent_provider();
        agent.adopt_rung(ToolRung::Flat);
        agent.adopt_levels(HashMap::from([(
            "gpt-x".to_string(),
            vec!["low".to_string()],
        )]));
        handle.end_of_turn();

        // Another account signs in; its models are read before the next turn.
        handle.account_changed();
        let listed = tauri::async_runtime::block_on(
            handle.list_models(|_| async { Ok(catalogue(&["medium", "max"])) }),
        );
        assert!(listed.is_ok());

        handle.turn_boundary();
        assert_eq!(
            agent.reasoning_levels("gpt-x"),
            ["medium", "max"],
            "the new account's levels, not the old ones and not none"
        );
        assert_eq!(
            agent.tool_rung(),
            ToolRung::NamespaceFull,
            "the old rung is gone"
        );
        // And an agent built next has them too.
        assert_eq!(
            handle.new_agent_provider().reasoning_levels("gpt-x"),
            ["medium", "max"]
        );
    }

    /// The other direction (review finding 6): a request that began for
    /// account A and ends after B signed in learned A's route, and must not
    /// teach it to B.
    #[test]
    fn a_copy_made_for_the_old_account_teaches_the_new_one_nothing() {
        let dir = tempdir().unwrap();
        let (handle, _store) = signed_in_handle(dir.path());
        let during = handle.clone();
        let listed = tauri::async_runtime::block_on(handle.list_models(move |cfg| async move {
            let failure = ProviderFailure::Http {
                status: 400,
                body: &tools_refused(),
                request_id: None,
            };
            cfg.provider.adapt_after_refusal(&failure);
            // B signs in while A's request is in flight.
            during.account_changed();
            Ok(catalogue(&["low"]))
        }));
        assert!(listed.is_ok(), "the caller still gets its answer");
        assert_eq!(handle.remembered_rung(), None);
        let next = handle.new_agent_provider();
        assert_eq!(next.tool_rung(), ToolRung::NamespaceFull);
        assert!(next.reasoning_levels("gpt-x").is_empty());
    }

    #[test]
    fn the_model_list_and_test_need_a_signed_in_session() {
        let dir = tempdir().unwrap();
        let store = Arc::new(MemoryStore::default());
        let session = open_session(dir.path(), store, false);
        let handle = ChatGptHandle::default();
        handle.install(
            session,
            Hooking {
                persistence: Arc::new(|| None),
                refusal: Arc::new(|| None),
                refresh_models: false,
                private_dir: None,
            },
        );
        let err = tauri::async_runtime::block_on(
            handle.list_models(|_| async { panic!("no request is made without a session") }),
        )
        .map(|_| ())
        .expect_err("not signed in");
        assert!(err.contains("not signed in"), "{err}");
        let err = tauri::async_runtime::block_on(handle.probe(None, |_, _| async {
            panic!("no request is made without a session")
        }))
        .map(|_| ())
        .expect_err("not signed in");
        assert!(err.contains("not signed in"), "{err}");
    }

    // ---- Typed errors ---------------------------------------------------------------------

    #[test]
    fn plan_usage_text_has_the_form_the_window_parses() {
        let error =
            PlanUsageError::new(PlanUsageKind::UsageLimit).with_request_id(Some("req_Ab-1.2:3"));
        let text = plan_usage_text(&error);
        assert!(
            text.starts_with("[plan-usage code=usage_limit request-id=req_Ab-1.2:3] "),
            "{text}"
        );
        assert!(text.ends_with(&error.to_string()), "{text}");

        let no_id = plan_usage_text(&PlanUsageError::new(PlanUsageKind::SessionEnded {
            detail: None,
        }));
        assert!(
            no_id.starts_with("[plan-usage code=session_ended] "),
            "{no_id}"
        );

        // An id with anything odd in it stays out of the marker, where it
        // would end the bracket early.
        let odd = PlanUsageError::new(PlanUsageKind::NotEligible).with_request_id(Some("a] b"));
        assert!(plan_usage_text(&odd).starts_with("[plan-usage code=not_eligible] "));

        // Every kind: a code the window switches on, and no word that the
        // chat reads as a refused key.
        for kind in [
            PlanUsageKind::UsageLimit,
            PlanUsageKind::NotEligible,
            PlanUsageKind::PlanDisabled,
            PlanUsageKind::SessionEnded { detail: None },
            PlanUsageKind::Unavailable { detail: "x".into() },
            PlanUsageKind::UnsupportedRequest {
                param: None,
                detail: "x".into(),
            },
            PlanUsageKind::Refused {
                code: None,
                detail: "x".into(),
            },
        ] {
            let error = PlanUsageError::new(kind).with_request_id(Some("req_1"));
            let text = plan_usage_text(&error);
            let lower = text.to_lowercase();
            assert!(
                !lower.contains("api key") && !lower.contains("api-key"),
                "{text}"
            );
            assert!(!lower.contains("authenticat"), "{text}");
            assert!(text.contains(&format!("code={}", error.code())), "{text}");
        }
    }

    #[test]
    fn a_callback_error_is_shown_with_openais_description_on_one_short_line() {
        let denied = SignInError::Loopback(LoopbackError::AccessDenied {
            description: Some("This app is unavailable\nfor your workspace".into()),
        });
        assert_eq!(
            sign_in_error_text(&denied),
            "access was denied: This app is unavailable for your workspace"
        );
        let plain = SignInError::Loopback(LoopbackError::AccessDenied { description: None });
        assert_eq!(sign_in_error_text(&plain), "access was denied");
        let other = SignInError::Loopback(LoopbackError::Authorization {
            error: "3p_login_workspace_scope_denied".into(),
            description: Some("x".repeat(500)),
        });
        let text = sign_in_error_text(&other);
        assert!(text.starts_with("the sign-in failed: 3p_login_workspace_scope_denied ("));
        assert!(text.len() < 300, "{}", text.len());
        assert_eq!(
            sign_in_error_text(&SignInError::AlreadyInProgress),
            "a ChatGPT sign-in is already in progress"
        );
    }

    #[test]
    fn the_notes_say_what_the_person_may_still_have_to_do() {
        let clean = SignOutReport {
            revocation: Revocation::Confirmed,
            removed_from_store: Ok(()),
        };
        assert!(sign_out_notes(&clean).is_empty());
        let loose = SignOutReport {
            revocation: Revocation::NotConfirmed {
                reason: "network down".into(),
            },
            removed_from_store: Err(SecretStoreError::Unavailable("locked".into())),
        };
        let notes = join_notes(sign_out_notes(&loose)).unwrap();
        assert!(
            notes.contains("disconnect edytlab in ChatGPT settings"),
            "{notes}"
        );
        assert!(notes.contains("network down"), "{notes}");
        assert!(
            notes.contains("could not be removed from the keychain"),
            "{notes}"
        );
    }

    #[test]
    fn signing_out_drops_what_the_route_taught_and_the_kept_message() {
        let dir = tempdir().unwrap();
        // An ended session with no token left: nothing to revoke, and a test
        // makes no request to OpenAI.
        let (handle, _store) = ended_handle(dir.path());
        let agent = handle.new_agent_provider();
        agent.adopt_rung(ToolRung::Flat);
        handle.end_of_turn();
        assert!(handle.keep_pending(handle.account_generation(), pending("t")));
        let end = tauri::async_runtime::block_on(handle.sign_out()).expect("signed out");
        assert_eq!(end.status.state, "signed_out");
        assert!(end.token_removed);
        assert_eq!(end.revocation, "nothing_to_revoke");
        assert!(end.notice.is_none());
        assert_eq!(handle.remembered_rung(), None);
        assert!(!end.status.pending_message);
        assert!(handle.take_pending().is_none());
    }

    fn pending(text: &str) -> PendingMessage {
        PendingMessage {
            text: text.into(),
            resume_text: text.into(),
            disabled_tools: vec![],
            progressed: false,
        }
    }

    /// The bug: a turn still running when the person signed out (or started
    /// over, or changed account) ended with the session's end and kept its
    /// message after the sign-out had cleared it, so it was offered to
    /// whoever signed in next.
    #[test]
    fn a_turn_that_ends_after_the_account_changed_keeps_no_message() {
        let dir = tempdir().unwrap();
        let (handle, _store) = ended_handle(dir.path());
        let started_under = handle.account_generation();

        // Under the same account, with the session ended: kept, and taken once.
        assert!(handle.keep_pending(started_under, pending("cut the intro")));
        assert_eq!(handle.take_pending().unwrap().text, "cut the intro");

        // The person signs out while the turn runs; the turn then ends.
        tauri::async_runtime::block_on(handle.sign_out()).expect("signed out");
        assert!(handle.account_generation() > started_under);
        assert!(!handle.keep_pending(started_under, pending("late")));
        assert!(handle.take_pending().is_none());
        assert!(!handle.status().pending_message);
    }

    #[test]
    fn every_way_the_account_changes_moves_the_generation_and_clears_the_message() {
        let dir = tempdir().unwrap();
        let (handle, _store) = ended_handle(dir.path());
        let g0 = handle.account_generation();
        assert!(handle.keep_pending(g0, pending("one")));
        handle.account_changed();
        let g1 = handle.account_generation();
        assert!(g1 > g0);
        assert!(handle.take_pending().is_none(), "cleared by the change");

        // A start over.
        assert!(handle.keep_pending(g1, pending("two")));
        tauri::async_runtime::block_on(handle.forget_registration()).expect("started over");
        assert!(handle.account_generation() > g1);
        assert!(handle.take_pending().is_none());
    }

    /// A person who signed out chose to leave: the state is signed out, not
    /// ended, and a message is not kept for it. Nor for a session still signed in.
    #[test]
    fn a_message_is_kept_only_for_a_session_that_ended() {
        let dir = tempdir().unwrap();
        let (signed_in, _store) = signed_in_handle(dir.path());
        assert!(!signed_in.keep_pending(signed_in.account_generation(), pending("x")));

        let other = tempdir().unwrap();
        let out = {
            let handle = ChatGptHandle::default();
            let session = open_session(other.path(), Arc::new(MemoryStore::default()), false);
            handle.install(
                session,
                Hooking {
                    persistence: Arc::new(|| None),
                    refusal: Arc::new(|| None),
                    refresh_models: false,
                    private_dir: None,
                },
            );
            handle
        };
        assert_eq!(out.status().state, "signed_out");
        assert!(!out.keep_pending(out.account_generation(), pending("x")));
        assert!(out.take_pending().is_none());
        assert!(!ChatGptHandle::default().keep_pending(0, pending("x")));
    }

    // ---- No local path reaches the window ----------------------------------------------

    /// A handle over `dir` that knows its files are there, whose saved
    /// registration has been rewritten by `damage`.
    fn handle_with_damage(
        dir: &std::path::Path,
        damage: impl FnOnce(&std::path::Path),
    ) -> ChatGptHandle {
        let store = Arc::new(MemoryStore::default());
        let session = open_session(dir, store.clone(), true);
        // No token: nothing to revoke, and a test makes no request to OpenAI.
        *lock(&store.token) = None;
        damage(&dir.join("chatgpt"));
        let handle = ChatGptHandle::default();
        handle.install(
            session,
            Hooking {
                persistence: Arc::new(|| None),
                refusal: Arc::new(|| None),
                refresh_models: false,
                private_dir: Some(dir.to_path_buf()),
            },
        );
        handle
    }

    fn registration_from_a_newer_edytlab(folder: &std::path::Path) {
        let file = folder.join("registration.json");
        let mut json: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        json["version"] = serde_json::json!(999);
        std::fs::write(file, json.to_string()).unwrap();
    }

    fn no_path(text: &str, dir: &std::path::Path) {
        let dir = dir.to_string_lossy();
        assert!(!text.contains(dir.as_ref()), "the folder is in: {text}");
    }

    /// The bug: `sign_in_error_text` passed `SignInError::Lock` and
    /// `Registration(..)` through as they print, and they name the config
    /// folder, so the window was handed a local path.
    #[test]
    fn a_sign_in_that_cannot_use_its_files_does_not_show_where_they_are() {
        // A registration written by a newer build.
        let dir = tempdir().unwrap();
        let handle = handle_with_damage(dir.path(), registration_from_a_newer_edytlab);
        // The session's own text does name the folder: the test is about
        // something.
        let raw = tauri::async_runtime::block_on(
            handle
                .session()
                .unwrap()
                .sign_in(|_: &str| Ok::<(), String>(()), SignInIntent::SignIn),
        )
        .expect_err("refused")
        .to_string();
        assert!(raw.contains(dir.path().to_string_lossy().as_ref()), "{raw}");
        let err = tauri::async_runtime::block_on(handle.sign_in(SignInIntent::SignIn, |_| Ok(())))
            .expect_err("refused");
        no_path(&err, dir.path());
        assert!(err.contains("newer edytlab"), "{err}");

        // A registration that cannot be read at all: the file is a folder.
        let dir = tempdir().unwrap();
        let handle = handle_with_damage(dir.path(), |folder| {
            let file = folder.join("registration.json");
            std::fs::remove_file(&file).unwrap();
            std::fs::create_dir(&file).unwrap();
        });
        let err = tauri::async_runtime::block_on(handle.sign_in(SignInIntent::SignIn, |_| Ok(())))
            .expect_err("refused");
        no_path(&err, dir.path());

        // The lock file cannot be taken. A folder in its place stops the
        // lock on unix; Windows opens a folder for locking and goes on to
        // wait for the browser, so this case is not reachable that way there.
        // The scrubbing it checks is the same code on every platform.
        #[cfg(unix)]
        {
            let dir = tempdir().unwrap();
            let handle = handle_with_damage(dir.path(), |folder| {
                std::fs::create_dir(folder.join("refresh.lock")).unwrap();
            });
            let err =
                tauri::async_runtime::block_on(handle.sign_in(SignInIntent::SignIn, |_| Ok(())))
                    .expect_err("refused");
            no_path(&err, dir.path());
            assert!(err.contains("locked or cannot be written"), "{err}");
        }
    }

    #[test]
    fn a_start_over_that_cannot_update_the_registration_does_not_show_where_it_is() {
        let dir = tempdir().unwrap();
        let handle = handle_with_damage(dir.path(), registration_from_a_newer_edytlab);
        // The token is gone already, so there is nothing to revoke and a
        // test makes no request to OpenAI.
        let end = tauri::async_runtime::block_on(handle.forget_registration());
        let end = match end {
            Ok(end) => end,
            Err(e) => panic!("{e}"),
        };
        assert_eq!(end.registration_forgotten, Some(false));
        let all = serde_json::to_string(&end).unwrap();
        no_path(&all, dir.path());
        let notice = end.notice.expect("it says so");
        assert!(
            notice.contains("could not update its saved ChatGPT registration"),
            "{notice}"
        );

        // Nothing else the window can ask for carries it either.
        no_path(
            &serde_json::to_string(&handle.status()).unwrap(),
            dir.path(),
        );
        let out = tauri::async_runtime::block_on(handle.sign_out()).expect("signed out");
        no_path(&serde_json::to_string(&out).unwrap(), dir.path());
    }

    #[test]
    fn text_for_the_window_is_scrubbed_of_the_folder_whatever_made_it() {
        let dir = tempdir().unwrap();
        let handle = handle_with_damage(dir.path(), |_| {});
        let text = format!("failed at {}/chatgpt/x", dir.path().display());
        let shown = handle.scrubbed(text);
        no_path(&shown, dir.path());
        assert!(shown.contains("the ChatGPT folder"), "{shown}");
    }

    // ---- A start over is not a browser being waited for -------------------------------

    /// The session takes its sign-in guard for the length of a start over, so
    /// that none can begin meanwhile, and the status read it as `signing_in`:
    /// a card would draw "finish signing in in your browser, Cancel".
    #[test]
    fn a_start_over_is_not_reported_as_signing_in_while_it_runs() {
        let dir = tempdir().unwrap();
        let (handle, _store) = ended_handle(dir.path());
        let session = handle.session().unwrap();
        tauri::async_runtime::block_on(async {
            let mut start_over = std::pin::pin!(handle.forget_registration());
            // Polled once: the session has taken its guard and the work has
            // not finished.
            let first =
                std::future::poll_fn(|cx| std::task::Poll::Ready(start_over.as_mut().poll(cx)))
                    .await;
            assert!(
                first.is_pending(),
                "it finished before it could be looked at"
            );
            assert!(session.is_signing_in(), "the session holds its guard");
            assert_ne!(handle.status().state, "signing_in");
            assert_eq!(handle.status().state, "session_ended");
            let end = start_over.await.expect("started over");
            assert_eq!(end.status.state, "signed_out");
        });
        // And a real sign-in still reads as one.
        assert!(!session.is_signing_in());
    }
}
