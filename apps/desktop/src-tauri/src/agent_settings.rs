//! What the agent is built from, saved in one call (ADR 0005, #716).
//!
//! The provider, its key, the chosen model, the base URL and the reasoning
//! effort used to be written by about fifteen commands, each of which had to
//! remember three rules: wait for the startup keychain read, lift a startup
//! the keychain refused after a write that went through, and rebuild the agent
//! with the right one of two functions. This module owns all three.
//!
//! * [`AgentSettings`] is the in-memory copy (the active provider, its key,
//!   every provider's chosen model, what startup found) and the startup state
//!   the reads wait on. `AppState` holds one.
//! * [`save`] is the one write: validate everything, write in the order it
//!   needs, rebuild the agent **once**, answer **once** with
//!   `{ persistent, reason }` ([`KeychainPersistenceDto`], #539).
//! * The reads ([`active_provider`], [`chosen_model`], [`base_url`],
//!   [`effort`], [`has_api_key`], [`has_api_key_for`], [`catalogue_inputs`])
//!   wait inside until startup has settled, and say why instead of answering
//!   from a default when the keychain could not be read (#394, #497).
//! * The agent's configuration has one producer: [`reapply`],
//!   [`reapply_soon`] and [`reapply_if_missing`] are how anything else (a
//!   profile, a project, the ChatGPT sign-in) asks for a new agent.
//! * The slots are behind [`Keychain`]; production is [`OsKeychain`] and the
//!   tests use an in-memory adapter, so nothing here needs Tauri or an OS
//!   keychain to be tested.
//!
//! The ChatGPT plan's rule lives here too: it is the active provider only
//! while its session is signed in, and it has no key or address to save.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use serde::Serialize;

use crate::commands::{CmdResult, CommandError};
use crate::state::AppState;

// ---------------------------------------------------------------------------
// Where a save landed
// ---------------------------------------------------------------------------

/// Where values saved now would end up, as the frontend needs to see it.
///
/// `persistent: false` means Linux with no Secret Service running, so
/// settings sit in the kernel keyring and are gone after the next restart
/// of the machine. `reason` is why, as the keyring library words it.
#[derive(Debug, Clone, Serialize)]
pub struct KeychainPersistenceDto {
    pub persistent: bool,
    pub reason: Option<String>,
}

impl KeychainPersistenceDto {
    /// Where a save's writes landed, taken together: session-only if any of
    /// them was (with the reason of the first), durable otherwise, and
    /// durable for a save that wrote nothing (a clear).
    pub(crate) fn landed(saves: impl IntoIterator<Item = ai::keychain::Persistence>) -> Self {
        saves
            .into_iter()
            .find(|at| matches!(at, ai::keychain::Persistence::SessionOnly { .. }))
            .unwrap_or(ai::keychain::Persistence::Durable)
            .into()
    }
}

impl From<ai::keychain::Persistence> for KeychainPersistenceDto {
    fn from(p: ai::keychain::Persistence) -> Self {
        match p {
            ai::keychain::Persistence::Durable => Self {
                persistent: true,
                reason: None,
            },
            ai::keychain::Persistence::SessionOnly { reason } => Self {
                persistent: false,
                reason: Some(reason),
            },
        }
    }
}

// ---------------------------------------------------------------------------
// The keychain seam
// ---------------------------------------------------------------------------

/// Every slot this module touches, as a trait so a test can say where each
/// write landed and what each read answers without an OS keychain.
/// [`OsKeychain`] is the real one: `ai::keychain`'s own functions, whose
/// `Persistence` is what a save reports.
pub(crate) trait Keychain: Send + Sync {
    fn save_api_key(
        &self,
        provider: &str,
        key: &str,
    ) -> Result<ai::keychain::Persistence, keyring::Error>;
    fn delete_api_key(&self, provider: &str) -> Result<(), keyring::Error>;
    fn save_active_provider(
        &self,
        provider: &str,
    ) -> Result<ai::keychain::Persistence, keyring::Error>;
    fn save_base_url(
        &self,
        provider: &str,
        base_url: &str,
    ) -> Result<ai::keychain::Persistence, keyring::Error>;
    fn delete_base_url(&self, provider: &str) -> Result<(), keyring::Error>;
    fn save_effort(
        &self,
        provider: &str,
        effort: ai::Effort,
    ) -> Result<ai::keychain::Persistence, keyring::Error>;
    fn delete_effort(&self, provider: &str) -> Result<(), keyring::Error>;
    fn save_model(
        &self,
        provider: &str,
        model: &str,
    ) -> Result<ai::keychain::Persistence, keyring::Error>;

    /// A read that cannot fail loudly: an unreadable slot is `None`.
    fn load_api_key(&self, provider: &str) -> Option<String>;
    fn load_base_url(&self, provider: &str) -> Option<String>;
    fn load_effort(&self, provider: &str) -> Option<ai::Effort>;

    /// A read where an unreadable keychain is an error and not `None`.
    fn try_load_active_provider(&self) -> Result<Option<String>, ai::keychain::KeychainError>;
    fn try_load_api_key(
        &self,
        provider: &str,
    ) -> Result<Option<String>, ai::keychain::KeychainError>;
    fn try_load_model(&self, provider: &str)
        -> Result<Option<String>, ai::keychain::KeychainError>;
    fn try_load_base_url(
        &self,
        provider: &str,
    ) -> Result<Option<String>, ai::keychain::KeychainError>;
    fn try_load_effort(
        &self,
        provider: &str,
    ) -> Result<Option<ai::Effort>, ai::keychain::KeychainError>;
}

/// The OS keychain and the settings file beside it (`ai::keychain`).
pub(crate) struct OsKeychain;

impl Keychain for OsKeychain {
    fn save_api_key(
        &self,
        provider: &str,
        key: &str,
    ) -> Result<ai::keychain::Persistence, keyring::Error> {
        ai::keychain::save_api_key(provider, key)
    }
    fn delete_api_key(&self, provider: &str) -> Result<(), keyring::Error> {
        ai::keychain::delete_api_key(provider)
    }
    fn save_active_provider(
        &self,
        provider: &str,
    ) -> Result<ai::keychain::Persistence, keyring::Error> {
        ai::keychain::save_active_provider(provider)
    }
    fn save_base_url(
        &self,
        provider: &str,
        base_url: &str,
    ) -> Result<ai::keychain::Persistence, keyring::Error> {
        ai::keychain::save_base_url(provider, base_url)
    }
    fn delete_base_url(&self, provider: &str) -> Result<(), keyring::Error> {
        ai::keychain::delete_base_url(provider)
    }
    fn save_effort(
        &self,
        provider: &str,
        effort: ai::Effort,
    ) -> Result<ai::keychain::Persistence, keyring::Error> {
        ai::keychain::save_effort(provider, effort)
    }
    fn delete_effort(&self, provider: &str) -> Result<(), keyring::Error> {
        ai::keychain::delete_effort(provider)
    }
    fn save_model(
        &self,
        provider: &str,
        model: &str,
    ) -> Result<ai::keychain::Persistence, keyring::Error> {
        ai::keychain::save_model(provider, model)
    }
    fn load_api_key(&self, provider: &str) -> Option<String> {
        ai::keychain::load_api_key(provider)
    }
    fn load_base_url(&self, provider: &str) -> Option<String> {
        ai::keychain::load_base_url(provider)
    }
    fn load_effort(&self, provider: &str) -> Option<ai::Effort> {
        ai::keychain::load_effort(provider)
    }
    fn try_load_active_provider(&self) -> Result<Option<String>, ai::keychain::KeychainError> {
        ai::keychain::try_load_active_provider()
    }
    fn try_load_api_key(
        &self,
        provider: &str,
    ) -> Result<Option<String>, ai::keychain::KeychainError> {
        ai::keychain::try_load_api_key(provider)
    }
    fn try_load_model(
        &self,
        provider: &str,
    ) -> Result<Option<String>, ai::keychain::KeychainError> {
        ai::keychain::try_load_model(provider)
    }
    fn try_load_base_url(
        &self,
        provider: &str,
    ) -> Result<Option<String>, ai::keychain::KeychainError> {
        ai::keychain::try_load_base_url(provider)
    }
    fn try_load_effort(
        &self,
        provider: &str,
    ) -> Result<Option<ai::Effort>, ai::keychain::KeychainError> {
        ai::keychain::try_load_effort(provider)
    }
}

// ---------------------------------------------------------------------------
// The in-memory copy
// ---------------------------------------------------------------------------

/// The in-memory copy of what the agent is built from, and the startup state
/// the reads wait on. Cloning is cheap (`Arc`s), and the clones share
/// everything.
#[derive(Clone)]
pub struct AgentSettings {
    keychain: Arc<dyn Keychain>,
    /// The key of the *currently active* provider, so building an agent does
    /// not read the keychain for it. When the active provider changes the
    /// slot is reloaded from the new provider's entry.
    api_key: Arc<Mutex<Option<String>>>,
    /// Why the keychain could not be read at startup, if it could not. `Some`
    /// is not the same as "no key stored": `has_api_key` rejects with this
    /// instead of answering `false`, so the window shows the reason and not a
    /// first-run prompt for a key that is already there (#394). Cleared by the
    /// next save that goes through.
    read_error: Arc<Mutex<Option<String>>>,
    /// Whether startup was *refused* by the keychain (locked, or an unlock
    /// prompt dismissed), as opposed to merely failing a read. Every further
    /// read would raise its own unlock prompt, so what would otherwise read
    /// the keychain on its own skips instead (#497), and no agent is built.
    /// Cleared with [`Self::read_error`] by the next save that goes through,
    /// and false again on every launch.
    refused: Arc<AtomicBool>,
    /// Whether startup's keychain reads have finished: `true` once they have,
    /// and from the start for a state nothing is reading into. The reads can
    /// wait on the user for as long as they like (a locked Secret Service's
    /// unlock prompt has no time limit), so they run on a thread of their own
    /// and the window comes up meanwhile. What they fill in is not there
    /// until then, so what answers from it or writes over it waits on this
    /// first ([`Self::startup_reads_settled`], #497).
    startup_reads: Arc<tokio::sync::watch::Sender<bool>>,
    /// Stable id of the active provider. Defaults to `"anthropic"` when no
    /// preference is recorded.
    active_provider: Arc<Mutex<String>>,
    /// Per-provider model id the user chose, so the next agent is built with
    /// it (#249).
    models: Arc<Mutex<HashMap<String, String>>>,
    /// One save at a time: a save is a unit of work, and two of them
    /// interleaving their writes would leave a mix of both.
    saving: Arc<tokio::sync::Mutex<()>>,
    /// How many times an agent was (re)built from these settings. A save is
    /// one.
    rebuilds: Arc<AtomicUsize>,
}

impl Default for AgentSettings {
    fn default() -> Self {
        Self::with_keychain(Arc::new(OsKeychain))
    }
}

impl AgentSettings {
    /// Settings over `keychain`: the OS one in the app, an in-memory one in a
    /// test.
    pub(crate) fn with_keychain(keychain: Arc<dyn Keychain>) -> Self {
        Self {
            keychain,
            api_key: Arc::new(Mutex::new(None)),
            read_error: Arc::new(Mutex::new(None)),
            refused: Arc::new(AtomicBool::new(false)),
            startup_reads: Arc::new(tokio::sync::watch::channel(true).0),
            active_provider: Arc::new(Mutex::new(ai::ANTHROPIC_ID.to_string())),
            models: Arc::new(Mutex::new(HashMap::new())),
            saving: Arc::new(tokio::sync::Mutex::new(())),
            rebuilds: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub(crate) fn keychain(&self) -> &dyn Keychain {
        &*self.keychain
    }

    /// The active provider id. Defaults to `"anthropic"` when no preference
    /// is recorded.
    pub fn active_provider_id(&self) -> String {
        self.active_provider
            .lock()
            .expect("active_provider mutex poisoned")
            .clone()
    }

    /// Replace the active provider id (in memory; [`save`] writes it).
    pub fn set_active_provider(&self, id: String) {
        *self
            .active_provider
            .lock()
            .expect("active_provider mutex poisoned") = id;
    }

    /// The cached key of the active provider, if any.
    pub fn api_key_snapshot(&self) -> Option<String> {
        self.api_key.lock().expect("api_key mutex poisoned").clone()
    }

    /// Replace the cached key.
    pub fn set_api_key_cache(&self, key: Option<String>) {
        *self.api_key.lock().expect("api_key mutex poisoned") = key;
    }

    /// The model id chosen for `provider_id`, if any.
    pub fn model_for(&self, provider_id: &str) -> Option<String> {
        self.models
            .lock()
            .expect("models mutex poisoned")
            .get(provider_id)
            .cloned()
    }

    /// Remember the model id chosen for `provider_id`. In memory only: the
    /// durable copy is written by [`save`] (#249).
    pub fn set_model_for(&self, provider_id: String, model: String) {
        self.models
            .lock()
            .expect("models mutex poisoned")
            .insert(provider_id, model);
    }

    /// Record why the keychain could not be read, or `None` once it can be.
    pub fn set_keychain_read_error(&self, error: Option<String>) {
        *self.read_error.lock().expect("read_error mutex poisoned") = error;
    }

    /// Why the keychain could not be read at startup, if it could not.
    pub fn keychain_read_error(&self) -> Option<String> {
        self.read_error
            .lock()
            .expect("read_error mutex poisoned")
            .clone()
    }

    /// Record whether the keychain refused access at startup.
    pub fn set_keychain_refused(&self, refused: bool) {
        self.refused.store(refused, Ordering::SeqCst);
    }

    /// Whether the keychain refused access at startup and nothing has worked
    /// since.
    pub fn keychain_refused(&self) -> bool {
        self.refused.load(Ordering::SeqCst)
    }

    /// Mark startup's keychain reads as pending, before anything can ask for
    /// what they fill in. Pair it with [`Self::startup_reads_guard`], or what
    /// waits for them waits for good.
    pub fn begin_startup_reads(&self) {
        self.startup_reads.send_replace(false);
    }

    /// A guard that marks startup's keychain reads as finished when it is
    /// dropped, however the thread holding it ends: a read that panicked must
    /// not leave `has_api_key` pending forever.
    pub fn startup_reads_guard(&self) -> StartupReadsGuard {
        StartupReadsGuard(Arc::clone(&self.startup_reads))
    }

    /// Wait until startup's keychain reads have finished. Returns at once
    /// when they have (or were never begun).
    pub async fn startup_reads_settled(&self) {
        let mut settled = self.startup_reads.subscribe();
        // The sender is `self`'s own, so it outlives the wait; an error here
        // could only mean it is gone, and then nothing is pending.
        let _ = settled.wait_for(|done| *done).await;
    }

    /// Whether startup's keychain reads are still going.
    pub fn startup_reads_pending(&self) -> bool {
        !*self.startup_reads.borrow()
    }

    /// How many agents were built from these settings. A save builds one.
    pub fn rebuilds(&self) -> usize {
        self.rebuilds.load(Ordering::SeqCst)
    }

    /// Why the keychain is not asked, if startup found it refusing access;
    /// `None` while it is not known to.
    ///
    /// Each further read of a locked Secret Service raises its own unlock
    /// prompt, right after the user dismissed the first, so what is read on
    /// the user's behalf only because a panel opened is refused up front with
    /// this instead (#497). The reason alone, in words for Settings' "Save
    /// leaves it as it is" note.
    pub fn refusal_note(&self) -> Option<String> {
        keychain_refusal_note(self.keychain_refused(), self.keychain_read_error())
    }

    /// [`Self::refusal_note`] as a closure the ChatGPT sign-in can hold, so
    /// its keychain reads follow the same refusal.
    pub(crate) fn refusal(&self) -> crate::chatgpt::Refusal {
        let settings = self.clone();
        Arc::new(move || settings.refusal_note())
    }

    /// The key a Model Context Protocol server's `<keychain:slot>` names, if
    /// one is stored.
    pub(crate) fn mcp_secret(&self, slot: &str) -> Option<String> {
        self.keychain.load_api_key(slot)
    }
}

/// Why the keychain is not asked again, after a startup it refused; `None`
/// while it is not known to. One wording for every read that is skipped, the
/// API keys' and the ChatGPT refresh token's.
pub fn keychain_refusal_note(refused: bool, read_error: Option<String>) -> Option<String> {
    if !refused {
        return None;
    }
    let why = read_error
        .map(|reason| format!(" ({reason})"))
        .unwrap_or_default();
    Some(format!(
        "the keychain refused access at launch{why}, so it was not asked again"
    ))
}

/// Marks startup's keychain reads as finished when dropped. See
/// [`AgentSettings::startup_reads_guard`].
pub struct StartupReadsGuard(Arc<tokio::sync::watch::Sender<bool>>);

impl Drop for StartupReadsGuard {
    fn drop(&mut self) {
        self.0.send_replace(true);
    }
}

// ---------------------------------------------------------------------------
// The one write
// ---------------------------------------------------------------------------

/// What to change, for one provider. A field that is `None` is left as it is;
/// an empty `base_url` or `effort` clears the setting (the provider's own
/// address, the model's own effort), which is what the window sends for an
/// emptied field. Saving settings for a provider also makes it the active one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SettingsChange {
    pub provider: String,
    pub key: Option<String>,
    pub model: Option<String>,
    pub base_url: Option<String>,
    pub effort: Option<String>,
}

/// A [`SettingsChange`] that passed every check, in the form it is written.
struct Validated {
    provider: String,
    /// A key to store; `None` also for a keyless provider's empty key.
    key: Option<String>,
    model: Option<String>,
    /// `Some(None)` clears the override.
    base_url: Option<Option<String>>,
    /// `Some(None)` clears the level.
    effort: Option<Option<ai::Effort>>,
}

/// Every check a save makes before it writes anything, so a save that is
/// refused changes nothing.
fn validate(change: &SettingsChange) -> Result<Validated, String> {
    let provider = change.provider.as_str();
    if !ai::SUPPORTED_PROVIDER_IDS.contains(&provider) {
        return Err(format!("unsupported provider id: {provider}"));
    }
    if is_session_provider(provider) && (change.key.is_some() || change.base_url.is_some()) {
        return Err(session_provider_refusal());
    }
    let key = match change.key.as_deref() {
        // Selecting a keyless provider is a save with no key: an empty one is
        // nothing to store, not a blank credential.
        Some(k) if k.trim().is_empty() => {
            if ai::validate::provider_for(provider).requires_api_key() {
                return Err("api key must not be empty".into());
            }
            None
        }
        Some(k) => Some(k.to_string()),
        None => None,
    };
    let model = match change.model.as_deref().map(str::trim) {
        Some("") => return Err("model id must not be empty".into()),
        Some(m) => Some(m.to_string()),
        None => None,
    };
    let base_url = change
        .base_url
        .as_deref()
        .map(|raw| parse_base_url(raw.trim()))
        .transpose()?;
    let effort = change
        .effort
        .as_deref()
        .map(|raw| parse_effort_setting(provider, Some(raw)))
        .transpose()?;
    Ok(Validated {
        provider: provider.to_string(),
        key,
        model,
        base_url,
        effort,
    })
}

/// A base URL as it is stored: `None` for an empty one (clear the override),
/// else the URL without a trailing slash, which turns `{base}/v1/models` into
/// `{base}//v1/models` on some servers. Validated here rather than at request
/// time: a typo saved now would otherwise surface much later as a connection
/// error with no hint that the cause was a malformed setting.
fn parse_base_url(trimmed: &str) -> Result<Option<String>, String> {
    if trimmed.is_empty() {
        return Ok(None);
    }
    let rest = trimmed
        .strip_prefix("http://")
        .or_else(|| trimmed.strip_prefix("https://"))
        .ok_or_else(|| format!("base URL must start with http:// or https:// — got `{trimmed}`"))?;
    if rest.is_empty() || rest.starts_with('/') {
        return Err(format!("base URL has no host — got `{trimmed}`"));
    }
    Ok(Some(trimmed.trim_end_matches('/').to_string()))
}

/// Validate a requested effort for `provider`. `None` (or blank) means "the
/// model's default" and is always valid; a level is valid only for a provider
/// whose endpoint takes one and only if it is exactly one of the five the API
/// accepts. The API answers anything else with a 400 on every request, so a
/// typo saved now would break chat later with no hint that a setting caused
/// it.
pub(crate) fn parse_effort_setting(
    provider: &str,
    effort: Option<&str>,
) -> Result<Option<ai::Effort>, String> {
    let Some(raw) = effort.map(str::trim).filter(|e| !e.is_empty()) else {
        return Ok(None);
    };
    if !ai::validate::provider_for(provider).supports_effort() {
        return Err(format!(
            "{provider} has no reasoning effort setting; it applies to Anthropic and the ChatGPT plan only"
        ));
    }
    ai::Effort::parse(raw).map(Some).ok_or_else(|| {
        let levels: Vec<&str> = ai::Effort::ALL.iter().map(|e| e.as_str()).collect();
        format!(
            "unknown reasoning effort `{raw}` (expected one of: {})",
            levels.join(", ")
        )
    })
}

/// Whether `provider_id` is paid for by a sign-in session and not a key (the
/// ChatGPT plan, #474). It has no key slot and no address: a saved key would
/// be sent as the "plan" bearer, and bill the API under a label that says
/// plan. So no key or address is ever saved or read for one; the session
/// ([`crate::chatgpt::ChatGptHandle`]) is its credential, and an agent is
/// built for it only while that session is signed in.
pub(crate) fn is_session_provider(provider_id: &str) -> bool {
    ai::validate::provider_for(provider_id).credential_kind() == ai::CredentialKind::ChatGptOAuth
}

/// The refusal for a save of a key or an address for a session provider:
/// there is none to save, whether or not anyone is signed in.
fn session_provider_refusal() -> String {
    "the ChatGPT plan is used by signing in with a ChatGPT account; \
     no key or address can be saved for it"
        .into()
}

/// Makes the active provider the ChatGPT plan only while its session is
/// signed in (with plan usage granted); otherwise says why not.
///
/// A saved sign-in that has not been read yet is read first, unless the
/// keychain refused access at launch (the same rule as every other read,
/// #497): then it stays unread and the answer says so.
async fn require_signed_in_session(state: &AppState) -> CmdResult<()> {
    state.chatgpt.restore().await;
    if state.chatgpt.is_signed_in() {
        return Ok(());
    }
    Err(crate::chatgpt::not_signed_in_error(&state.chatgpt.status()))
}

/// Save settings for one provider, make it the active one and rebuild the
/// agent once.
///
/// Everything is checked before anything is written, so a refused save
/// changes nothing. The writes then go in the order the agent's next build
/// needs them: the key, the address, the level, the choice of provider, the
/// model. A write that fails stops the rest, and the agent is still rebuilt
/// from what did go through, so it matches what is stored.
///
/// Answers where the writes landed, as one answer (see
/// [`KeychainPersistenceDto`]): session-only if any of them fell back to the
/// kernel keyring because no Secret Service could be reached, so it will not
/// survive a reboot. The values are saved either way (#497, #539).
///
/// The rebuild waits for the agent's turn to end, so a save during a turn is
/// applied after it; the turn carries on with the agent it started with.
///
/// Waits for startup's keychain reads first: they fill the memory this writes
/// over, and would write over this in turn if they finished after it.
pub async fn save(state: &AppState, change: SettingsChange) -> CmdResult<KeychainPersistenceDto> {
    let settings = &state.agent_settings;
    settings.startup_reads_settled().await;
    let change = validate(&change)?;
    // The ChatGPT plan can be the active provider only with a session to
    // build its agent from.
    if is_session_provider(&change.provider) {
        require_signed_in_session(state).await?;
    }
    let _one_save_at_a_time = settings.saving.lock().await;
    let mut write = Write::default();
    let written = write_all(state, &change, &mut write);
    if write.applied {
        // Once, for the whole save, and also for one that stopped part way:
        // the writes before the failure are in force.
        rebuild_agent(state).await?;
    }
    written?;
    match write.key_read_failure {
        // The switch itself worked, so say what did not instead of failing
        // it: the agent is built for the new provider with no key.
        Some(e) => Err(format!(
            "switched to {}, but its saved key could not be read: {e}",
            change.provider
        )),
        None => Ok(KeychainPersistenceDto::landed(write.landed)),
    }
}

/// What a save has done so far.
#[derive(Default)]
struct Write {
    landed: Vec<ai::keychain::Persistence>,
    /// Whether anything changed, in memory or stored, that the agent is built
    /// from.
    applied: bool,
    /// Why the new provider's saved key could not be read, if it could not.
    key_read_failure: Option<String>,
}

fn write_all(state: &AppState, change: &Validated, write: &mut Write) -> CmdResult<()> {
    let settings = &state.agent_settings;
    let keychain = settings.keychain();
    let provider = change.provider.as_str();
    if let Some(key) = &change.key {
        write.landed.push(
            keychain
                .save_api_key(provider, key)
                .map_err(CommandError::from)?,
        );
    }
    match &change.base_url {
        Some(Some(url)) => write.landed.push(
            keychain
                .save_base_url(provider, url)
                .map_err(CommandError::from)?,
        ),
        // Clearing writes nothing, so it is always durable: there is no value
        // to lose.
        Some(None) => keychain
            .delete_base_url(provider)
            .map_err(CommandError::from)?,
        None => {}
    }
    match change.effort {
        Some(Some(level)) => write.landed.push(
            keychain
                .save_effort(provider, level)
                .map_err(CommandError::from)?,
        ),
        Some(None) => keychain
            .delete_effort(provider)
            .map_err(CommandError::from)?,
        None => {}
    }
    // Stored values the agent reads on its next build are in force from here.
    write.applied = true;
    write.landed.push(
        keychain
            .save_active_provider(provider)
            .map_err(CommandError::from)?,
    );
    // The key and the provider went through, so the keychain is readable and
    // writable now whatever it was at launch.
    keychain_readable_again(state);
    let was_active = settings.active_provider_id() == provider;
    settings.set_active_provider(provider.to_string());
    match &change.key {
        // The key just typed is the one in use.
        Some(key) => settings.set_api_key_cache(Some(key.clone())),
        // Already the active provider with its key in hand: nothing to reread.
        None if was_active && settings.api_key_snapshot().is_some() => {}
        // Another provider's key must not be sent to this one, so it is
        // replaced by this one's, or by nothing when that cannot be read.
        None => match key_for_switch(provider, |p| keychain.try_load_api_key(p)) {
            Ok(key) => settings.set_api_key_cache(key),
            Err(e) => {
                settings.set_api_key_cache(None);
                write.key_read_failure = Some(e.reason().to_string());
            }
        },
    }
    if let Some(model) = &change.model {
        // Persisted, not only cached (#249). The choice applies to this
        // session whether or not it could be stored, but a store that failed
        // is reported rather than swallowed: it will not survive a restart.
        let stored = keychain.save_model(provider, model);
        settings.set_model_for(provider.to_string(), model.clone());
        match stored {
            Ok(at) => write.landed.push(at),
            Err(e) => {
                return Err(format!(
                    "model selected for this session, but could not be saved: {e}"
                ))
            }
        }
    }
    Ok(())
}

/// The key to cache when switching to `provider`.
///
/// A provider that needs none (a local Ollama daemon) has nothing to read, so
/// the keychain is not asked: on a locked keyring that read is an unlock
/// prompt for a key that does not exist, and an error the switch would then
/// report.
fn key_for_switch(
    provider: &str,
    load: impl FnOnce(&str) -> Result<Option<String>, ai::keychain::KeychainError>,
) -> Result<Option<String>, ai::keychain::KeychainError> {
    if is_session_provider(provider) || !ai::validate::provider_for(provider).requires_api_key() {
        return Ok(None);
    }
    load(provider)
}

/// Remove the stored key of `provider`. When it is the active provider the
/// agent is torn down: the app behaves as if it had just launched with no key
/// for it (M13).
pub async fn clear_key(state: &AppState, provider: &str) -> CmdResult<()> {
    state.agent_settings.startup_reads_settled().await;
    state
        .agent_settings
        .keychain()
        .delete_api_key(provider)
        .map_err(CommandError::from)?;
    if state.agent_settings.active_provider_id() == provider {
        state.agent_settings.set_api_key_cache(None);
        rebuild_agent(state).await?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// The reads
// ---------------------------------------------------------------------------

/// A stored setting as the window sees it: the value, or the reason the
/// keychain could not be read, never `None` for a failure.
///
/// The reason alone: Settings says the rest.
fn stored_setting<T>(read: Result<Option<T>, ai::keychain::KeychainError>) -> CmdResult<Option<T>> {
    read.map_err(|e| e.reason().to_string())
}

/// [`stored_setting`], except that `read` is not made at all once startup
/// found the keychain refusing access: the answer is then the refusal's own
/// note ([`AgentSettings::refusal_note`]).
///
/// What Settings reads when it opens would otherwise raise another unlock
/// prompt for each field, on a keyring the user has just declined to unlock.
/// The prompts end with the refusal, which the first save that goes through
/// lifts ([`keychain_readable_again`]).
fn stored_setting_unless_refused<T>(
    state: &AppState,
    read: impl FnOnce() -> Result<Option<T>, ai::keychain::KeychainError>,
) -> CmdResult<Option<T>> {
    match state.agent_settings.refusal_note() {
        Some(why) => Err(why),
        None => stored_setting(read()),
    }
}

/// A provider's base-URL override, or `None` when it uses the built-in one.
///
/// Rejects when the keychain could not be read, which is not the same as
/// `None`: Settings saves what it showed, and a blank it took for "no
/// override" is sent back as a clear (#497). Also rejects, without reading,
/// after a startup the keychain refused.
pub async fn base_url(state: &AppState, provider: &str) -> CmdResult<Option<String>> {
    state.agent_settings.startup_reads_settled().await;
    stored_setting_unless_refused(state, || {
        state.agent_settings.keychain().try_load_base_url(provider)
    })
}

/// A provider's reasoning effort as its wire string (`"low"`, `"medium"`,
/// `"high"`, `"xhigh"` or `"max"`), or `None` for the model's own default.
/// Rejects for the reasons [`base_url`] gives.
pub async fn effort(state: &AppState, provider: &str) -> CmdResult<Option<String>> {
    state.agent_settings.startup_reads_settled().await;
    stored_setting_unless_refused(state, || {
        state.agent_settings.keychain().try_load_effort(provider)
    })
    .map(|level| level.map(|e| e.as_str().to_string()))
}

/// The active provider id.
///
/// Rejects while startup's keychain read is still unaccounted for
/// ([`AgentSettings::keychain_read_error`]): the default is then what was
/// never read, not an answer. Settings would show it as the stored choice and
/// its Save would write it over the real one (#497).
pub async fn active_provider(state: &AppState) -> CmdResult<String> {
    state.agent_settings.startup_reads_settled().await;
    active_provider_answer(
        state.agent_settings.keychain_read_error(),
        state.agent_settings.active_provider_id(),
    )
}

/// The answer to [`active_provider`], given what startup found.
///
/// The reason alone, as [`has_key_answer`] gives it: the banner that shows it
/// already says the keychain could not be read.
fn active_provider_answer(startup_error: Option<String>, active: String) -> CmdResult<String> {
    match startup_error {
        Some(e) => Err(e),
        None => Ok(active),
    }
}

/// The model id chosen for `provider`; empty when nothing has been chosen.
///
/// Rejects when startup could not read the keychain and nothing is known for
/// `provider` (see [`active_model_answer`]): "nothing chosen" is then what the
/// map lacks, not what the keychain says, and Settings would save what its
/// field shows over the stored model (#497).
pub async fn chosen_model(state: &AppState, provider: &str) -> CmdResult<String> {
    state.agent_settings.startup_reads_settled().await;
    active_model_answer(
        models_unread_reason(state),
        state.agent_settings.model_for(provider),
    )
}

/// The answer to [`chosen_model`]: the model known for the provider, else
/// "none chosen" if startup read the keychain cleanly, else the reason it
/// could not.
///
/// A model that is known is an answer whatever startup found. It was restored
/// before the refusal ended the reads, or chosen since.
fn active_model_answer(unread: Option<String>, known: Option<String>) -> CmdResult<String> {
    match (known, unread) {
        (Some(model), _) => Ok(model),
        (None, Some(why)) => Err(why),
        (None, None) => Ok(String::new()),
    }
}

/// Why startup's keychain reads left the chosen models unrestored, if they
/// did. Startup stops at the first refusal, so the models after it were never
/// read; a read that failed some other way may have been one of the models'.
/// Either way an empty map entry proves nothing.
fn models_unread_reason(state: &AppState) -> Option<String> {
    state.agent_settings.refusal_note().or_else(|| {
        state
            .agent_settings
            .keychain_read_error()
            .map(|reason| format!("the keychain could not be read at launch ({reason})"))
    })
}

/// Whether the active provider has what it needs (a stored key, or none
/// required).
///
/// Rejects when the keychain could not be read (locked, access denied, no
/// service). That is not the same as `false`: `false` sends the user to enter
/// a key, which on a locked keyring they already have (#394).
pub async fn has_api_key(state: &AppState) -> CmdResult<bool> {
    // Pending while startup's keychain reads are (#497): the window is up
    // before they are done, and it says it is waiting for the keyring until
    // this answers.
    state.agent_settings.startup_reads_settled().await;
    let provider_id = state.agent_settings.active_provider_id();
    has_key_answer(state.agent_settings.keychain_read_error(), || {
        provider_is_configured(state, &provider_id)
    })
}

/// Whether `provider` is ready to use, for Settings' provider picker.
/// Rejects when the keychain could not be read, which is not the same as
/// `false`.
pub async fn has_api_key_for(state: &AppState, provider: &str) -> CmdResult<bool> {
    state.agent_settings.startup_reads_settled().await;
    provider_is_configured(state, provider).map_err(|e| e.to_string())
}

/// The answer to [`has_api_key`], given what startup found.
///
/// A keychain that could not be read at launch means the active provider and
/// key were never restored, so an answer computed from the defaults would be
/// about the wrong provider. Report the failure instead, and only ask
/// `configured` when startup read cleanly.
///
/// Both failures carry the reason alone (`KeychainError::reason`): the banner
/// that shows it already says the keychain could not be read, and the full
/// message would say so twice.
fn has_key_answer(
    startup_error: Option<String>,
    configured: impl FnOnce() -> Result<bool, ai::keychain::KeychainError>,
) -> CmdResult<bool> {
    match startup_error {
        Some(e) => Err(e),
        None => configured().map_err(|e| e.reason().to_string()),
    }
}

/// Whether `provider_id` is ready to use.
///
/// For a keyless provider that is unconditionally true: there is no
/// credential to store, so asking the keychain would answer "no" forever and
/// leave first launch stuck behind a key prompt it can never satisfy.
///
/// An unreadable keychain is an error rather than `false`.
fn provider_is_configured(
    state: &AppState,
    provider_id: &str,
) -> Result<bool, ai::keychain::KeychainError> {
    // A provider paid for by a session is configured when its session is
    // signed in, and no key slot is read.
    if is_session_provider(provider_id) {
        let status = state.chatgpt.status();
        // A saved sign-in the keychain could not be asked for is not "not
        // signed in": the answer `false` sends the person to sign in again
        // over a session that may well be there (#394).
        if status.state == "unread" {
            return Err(ai::keychain::KeychainError::new(
                status
                    .reason
                    .map(|r| r.message)
                    .unwrap_or_else(|| "the saved ChatGPT sign-in could not be read".into()),
            ));
        }
        return Ok(status.state == "signed_in");
    }
    if !ai::validate::provider_for(provider_id).requires_api_key() {
        return Ok(true);
    }
    state
        .agent_settings
        .keychain()
        .try_load_api_key(provider_id)
        .map(|k| k.is_some())
}

/// What the model catalogue is fetched with: the key, and the endpoint the
/// agent will use.
///
/// Both come from the keychain unless the key was typed into the form, and
/// after a startup the keychain refused neither is read: that is two more
/// unlock prompts every time Settings opens or the provider changes (#497).
/// The list is refused with the refusal's note instead. Reading only the key
/// would leave the endpoint unknown, and a list from the built-in endpoint is
/// not the list of the server chat talks to, so a key typed for a gateway
/// would go to the wrong host.
pub fn catalogue_inputs(
    state: &AppState,
    provider: &str,
    typed: Option<String>,
) -> CmdResult<(Option<String>, Option<String>)> {
    let keychain = state.agent_settings.keychain();
    catalogue_inputs_from(
        state,
        provider,
        typed,
        |p| keychain.load_api_key(p),
        |p| keychain.load_base_url(p),
    )
}

/// [`catalogue_inputs`] with the two keychain reads passed in.
fn catalogue_inputs_from(
    state: &AppState,
    provider: &str,
    typed: Option<String>,
    load_key: impl Fn(&str) -> Option<String>,
    load_base_url: impl Fn(&str) -> Option<String>,
) -> CmdResult<(Option<String>, Option<String>)> {
    if let Some(why) = state.agent_settings.refusal_note() {
        return Err(why);
    }
    Ok((
        catalogue_key(provider, typed, load_key),
        load_base_url(provider),
    ))
}

/// The key the catalogue is fetched with: the one typed into the form if
/// there is one, else the one saved for `provider`.
///
/// Settings sends only what was typed, and after a key is saved the form is
/// empty, so without the fallback the normal case, a saved key, never got a
/// live list. The saved key is read here and stays in this process; it is not
/// sent to the webview.
fn catalogue_key(
    provider: &str,
    typed: Option<String>,
    load: impl Fn(&str) -> Option<String>,
) -> Option<String> {
    typed
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty())
        .or_else(|| load(provider))
}

/// Whether the agent that is missing is the ChatGPT plan's: it is the active
/// provider, or the active profile pins it. There is no key to set for it.
pub(crate) fn effective_provider_is_session(state: &AppState) -> bool {
    let pinned = state
        .active_profile_overrides()
        .2
        .map(|(provider, _)| provider);
    is_session_provider(&pinned.unwrap_or_else(|| state.agent_settings.active_provider_id()))
}

// ---------------------------------------------------------------------------
// Startup
// ---------------------------------------------------------------------------

/// At app startup, restore the active provider from the keychain (if
/// recorded) and load that provider's stored key into the in-memory cache,
/// then every provider's chosen model. If nothing is stored, this is a no-op
/// and the window's first action is to surface Settings.
///
/// A keychain that cannot be read is not "nothing stored": the reason is kept
/// and `has_api_key` rejects with it, so the window can tell the user instead
/// of asking for a key they already entered (#394).
///
/// Blocks for as long as the keychain does, which for a locked Secret Service
/// is as long as the user takes to answer its prompt, so `run` calls it off
/// the setup thread and what depends on it waits for it
/// ([`AgentSettings::startup_reads_settled`], #497).
pub fn load_at_startup(state: &AppState) {
    let keychain = state.agent_settings.keychain();
    load_startup_settings(
        state,
        || keychain.try_load_active_provider(),
        |id| keychain.try_load_api_key(id),
        |id| keychain.try_load_model(id),
    );
}

/// [`load_at_startup`] with the keychain reads passed in, so the failure paths
/// can be tested one slot at a time: the active provider, its key, then every
/// provider's chosen model.
///
/// A locked keychain stops it at the first refusal. Each read of a locked
/// Secret Service raises its own unlock prompt, so carrying on would ask the
/// user to dismiss the same prompt once per slot before the window is up.
pub(crate) fn load_startup_settings(
    state: &AppState,
    load_active: impl Fn() -> Result<Option<String>, ai::keychain::KeychainError>,
    load_key: impl Fn(&str) -> Result<Option<String>, ai::keychain::KeychainError>,
    load_model: impl Fn(&str) -> Result<Option<String>, ai::keychain::KeychainError>,
) {
    let settings = &state.agent_settings;
    let mut first_error: Option<String> = None;
    let mut refused = false;
    match load_active() {
        Ok(Some(active)) => settings.set_active_provider(active),
        Ok(None) => {}
        Err(e) => refused = note_startup_failure("active provider", &e, &mut first_error),
    }
    if !refused {
        match load_key(&settings.active_provider_id()) {
            Ok(Some(key)) => settings.set_api_key_cache(Some(key)),
            Ok(None) => {}
            Err(e) => refused = note_startup_failure("API key", &e, &mut first_error),
        }
    }
    settings.set_keychain_read_error(first_error);
    if !refused {
        refused = restore_models(state, load_model);
    }
    // What the MCP autostart asks before it reads the keychain for a server of
    // its own (#497).
    settings.set_keychain_refused(refused);
}

/// A save went through, so the keychain is readable and writable now whatever
/// it was at launch: drop the failure startup kept.
///
/// If there was one, startup stopped reading there and never restored the
/// chosen models, so read them now. Without that the agent would run each
/// provider's default model while Settings showed the stored pick (#249). A
/// provider with a pick already in this session is left alone: it was made
/// after the failure and may be newer than what the keychain holds.
///
/// With nothing kept this reads nothing, so an ordinary save costs no extra
/// keychain round trips.
///
/// The refusal startup recorded goes with the failure: MCP servers that were
/// skipped for it can be restarted from Settings now.
pub(crate) fn keychain_readable_again(state: &AppState) {
    let keychain = state.agent_settings.keychain();
    keychain_readable_again_with(state, |id| keychain.try_load_model(id));
}

/// [`keychain_readable_again`] with the model read passed in.
pub(crate) fn keychain_readable_again_with(
    state: &AppState,
    load_model: impl Fn(&str) -> Result<Option<String>, ai::keychain::KeychainError>,
) {
    let settings = &state.agent_settings;
    if settings.keychain_read_error().is_none() && !settings.keychain_refused() {
        return;
    }
    settings.set_keychain_read_error(None);
    settings.set_keychain_refused(false);
    restore_models(state, |id| {
        if settings.model_for(id).is_some() {
            Ok(None)
        } else {
            load_model(id)
        }
    });
}

/// Log a startup read that failed and keep the first one's reason. Returns
/// whether the keychain refused access, which ends startup's reads.
///
/// Only the reason is kept (`KeychainError::reason`): the banner built from it
/// already says the keychain could not be read.
fn note_startup_failure(
    what: &str,
    e: &ai::keychain::KeychainError,
    first_error: &mut Option<String>,
) -> bool {
    tracing::warn!(error = %e, "could not read the {what} from the keychain at startup");
    first_error.get_or_insert_with(|| e.reason().to_string());
    e.is_access_denied()
}

/// Repopulate the in-memory model map from `load`.
///
/// The logic worth pinning is that *every* provider's choice comes back, not
/// just the active one's. Switching provider must not lose a choice made
/// earlier, and this map is what the agent's build reads after a switch.
///
/// A slot that cannot be read is skipped, except that a keychain which
/// refused access ends the loop: the rest would be refused too, each with an
/// unlock prompt of its own. Returns whether it did.
pub(crate) fn restore_models(
    state: &AppState,
    load: impl Fn(&str) -> Result<Option<String>, ai::keychain::KeychainError>,
) -> bool {
    for id in ai::SUPPORTED_PROVIDER_IDS {
        match load(id) {
            Ok(Some(model)) => state.agent_settings.set_model_for((*id).to_string(), model),
            Ok(None) => {}
            Err(e) => {
                tracing::warn!(provider = %id, error = %e, "could not read the chosen model from the keychain at startup");
                if e.is_access_denied() {
                    return true;
                }
            }
        }
    }
    false
}

// ---------------------------------------------------------------------------
// The agent's configuration: the only producer
// ---------------------------------------------------------------------------

/// Build the agent anew from the settings as they are now, after something
/// else changed what it is built from (a profile, a project). Waits for the
/// agent's turn to end, as [`rebuild_agent`] does, so a command in the
/// session's line must use [`reapply_soon`].
pub async fn reapply(state: &AppState) -> Result<(), CommandError> {
    rebuild_agent(state).await
}

/// [`reapply`] without waiting for a turn to end (#498): see
/// [`rebuild_agent_soon`].
pub fn reapply_soon(state: &AppState) {
    rebuild_agent_soon(state);
}

/// Builds the agent if there is none and nothing is using the slot, without
/// touching a running one: its conversation is the person's. A person who had
/// the ChatGPT plan as the active provider and no agent (signed out, then in)
/// gets one.
pub fn reapply_if_missing(state: &AppState) {
    if let Ok(mut slot) = state.agent.try_lock() {
        if slot.is_none() {
            rebuild_agent_into(state, &mut slot);
        }
    }
}

/// The ChatGPT sign-in finished: the token was written to the keychain when
/// `token_in_keychain`, so the keychain can be used again whatever it was at
/// launch, as after a save. An agent is built if there is none, and the
/// account's models are read.
pub fn chatgpt_signed_in(state: &AppState, token_in_keychain: bool) {
    if token_in_keychain {
        keychain_readable_again(state);
    }
    reapply_if_missing(state);
    refresh_chatgpt_models_soon(state);
}

/// The agent that ran on the plan has no session to run on any more.
pub fn chatgpt_session_ended(state: &AppState) {
    if state.chatgpt.has_agent() {
        rebuild_agent_soon(state);
    }
}

/// Build the agent after startup's reads settled, so it is ready for the very
/// first chat turn with no manual save. After them, not alongside: the store
/// is in place by now, and the key may not be.
pub async fn build_after_startup(state: &AppState) -> Result<(), CommandError> {
    state.agent_settings.startup_reads_settled().await;
    rebuild_agent(state).await
}

/// Resolve the `(provider_id, api_key, model)` triple the agent should be
/// built with.
///
/// A profile that pins `model: { provider, id }` overrides *both* the provider
/// and the model id. When the pinned provider differs from the globally active
/// one, the cached API key belongs to the wrong provider — sending it would
/// 401 against the pinned provider's endpoint — so the key is re-read from
/// that provider's keychain slot via `load_key`.
///
/// A pure function so the provider/key pairing can be unit-tested without a
/// keychain or a live `AppState`.
fn resolve_effective_llm(
    global_provider_id: String,
    global_key: Option<String>,
    global_model: Option<String>,
    profile_model: Option<(String, String)>,
    load_key: impl Fn(&str) -> Option<String>,
) -> (String, Option<String>, Option<String>) {
    match profile_model {
        // Profile pins a provider that is not the active one: the cached key
        // is for a different service.
        Some((provider, id)) if provider != global_provider_id => {
            let key = load_key(&provider);
            (provider, key, Some(id))
        }
        // Profile pins a model on the provider that is already active.
        Some((provider, id)) => (provider, global_key, Some(id)),
        // No profile override: the global picker wins.
        None => (global_provider_id, global_key, global_model),
    }
}

/// Wait for the agent's mutex, then rebuild the agent.
///
/// The mutex is held by `send_message` for a whole turn, which can be minutes
/// (a plan card waits up to five), so this waits that long too. A command that
/// is in the session's line (`sessionQueue.ts`) must not call it: everything
/// queued after that command would wait for the turn. [`rebuild_agent_soon`]
/// is the one for those.
///
/// What the agent is built from is read *after* the mutex is held, not
/// before: a caller that waited behind a turn builds from the state of the
/// moment it runs, not of the moment it was asked.
async fn rebuild_agent(state: &AppState) -> Result<(), CommandError> {
    {
        let mut slot = state.agent.lock().await;
        rebuild_agent_into(state, &mut slot);
    }
    refresh_chatgpt_models_soon(state);
    Ok(())
}

/// Reads the signed-in ChatGPT account's model list in the background when an
/// agent was just built on the plan, so the reasoning levels each model takes
/// are known ([`crate::chatgpt::ChatGptHandle::refresh_models_with`]). Nothing
/// is waited for, and a state that does not read it (tests) makes no request.
fn refresh_chatgpt_models_soon(state: &AppState) {
    if !state.chatgpt.refreshes_models() || !state.chatgpt.has_agent() {
        return;
    }
    let state = state.clone();
    // Called by another name: `tests/main_thread_commands.rs` reads the plain
    // call as the reclaimer's, which takes the locks, and would take this for
    // a command that waits on them.
    let in_background = tauri::async_runtime::spawn;
    in_background(async move {
        state
            .chatgpt
            .refresh_models_with(crate::chatgpt::read_models)
            .await;
    });
}

/// Rebuild the agent without waiting for a turn to end (#498).
///
/// Opening a project and Save As call this, and the app sends both in the
/// session's line, so they must answer even while the agent is mid-turn.
///
/// - Nobody holds the agent (the usual case): rebuilt here and now, so the
///   agent is on the new store when the command answers.
/// - A turn holds it: the rebuild runs as its own task, which waits its turn
///   on the mutex and then reads the state as it is *then*. Several of these
///   queued behind one turn each build from the latest state, so an older
///   open cannot be applied after a newer one. The turn itself carries on
///   with the agent and the store it started with (#501).
fn rebuild_agent_soon(state: &AppState) {
    if let Ok(mut slot) = state.agent.try_lock() {
        rebuild_agent_into(state, &mut slot);
        drop(slot);
        refresh_chatgpt_models_soon(state);
        return;
    }
    let state = state.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = rebuild_agent(&state).await {
            tracing::warn!(error = %e, "could not rebuild the agent after a turn");
        }
    });
}

/// Build the agent from the state as it is now and put it in `slot`, which is
/// the agent's mutex, held by the caller. Nothing here waits for a turn; see
/// [`rebuild_agent`] and [`rebuild_agent_soon`] for the callers.
///
/// After a startup the keychain refused, nothing is built and the keychain is
/// not read ([`rebuild_agent_from`]).
fn rebuild_agent_into(state: &AppState, slot: &mut Option<ai::Agent>) {
    let keychain = state.agent_settings.keychain();
    rebuild_agent_from(
        state,
        slot,
        |provider| keychain.load_api_key(provider),
        |provider| keychain.load_base_url(provider),
        |provider| keychain.load_effort(provider),
    );
}

/// [`rebuild_agent_into`] with the three keychain reads it makes passed in, so
/// a test can count them without an OS keychain: another provider's key (for a
/// profile that pins one), and the base URL and the level of the provider the
/// agent is built for.
pub(crate) fn rebuild_agent_from(
    state: &AppState,
    slot: &mut Option<ai::Agent>,
    load_key: impl Fn(&str) -> Option<String>,
    load_base_url: impl Fn(&str) -> Option<String>,
    load_effort: impl Fn(&str) -> Option<ai::Effort>,
) {
    state.agent_settings.rebuilds.fetch_add(1, Ordering::SeqCst);
    // After a startup the keychain refused (#497), no agent can be built
    // without asking it again: every agent reads its provider's base URL and
    // level, and a profile that pins another provider reads that provider's
    // key. On a locked Secret Service each read is an unlock prompt, raised
    // right after the user declined the first, and this runs at startup
    // behind the reads. Building without them would be a guess, an agent
    // pointed at the provider's own host when the stored URL is a gateway's
    // (the reason `list_models_for` is refused as well). So the agent is left
    // unbuilt, and `send_message` says why (`no_agent_error`). The first save
    // that goes through lifts the refusal ([`keychain_readable_again`]) and
    // builds it.
    // Whatever agent was running is replaced. If it ran on the ChatGPT plan,
    // the rung its provider ended on is kept for the next one built.
    state.chatgpt.agent_replaced_by_another_provider();
    if state.agent_settings.keychain_refused() {
        *slot = None;
        return;
    }
    let store_handle = state.store_handle();
    let global_provider_id = state.agent_settings.active_provider_id();
    let global_chosen_model = state.agent_settings.model_for(&global_provider_id);
    let (profile_body, tool_whitelist, profile_model) = state.active_profile_overrides();

    let (effective_provider_id, api_key, effective_model) = resolve_effective_llm(
        global_provider_id,
        state.agent_settings.api_key_snapshot(),
        global_chosen_model,
        profile_model,
        // A provider paid for by a session has no key slot to read.
        |provider| {
            if is_session_provider(provider) {
                None
            } else {
                load_key(provider)
            }
        },
    );
    // The config the agent is built from, or none.
    //
    // A provider paid for by a session (the ChatGPT plan, #474) has an agent
    // only while its session is signed in with plan usage granted, whether it
    // is the active provider or a profile pins it. It is built on the session,
    // never a key (the cached key is another provider's or none at all, and
    // its slots are not read), and with no address: the bearer goes to one
    // host. A signed-out or ended session builds none, and `send_message`
    // says there is no agent.
    let cfg = if is_session_provider(&effective_provider_id) {
        match (state.chatgpt.signed_in_session(), store_handle.is_some()) {
            (Some(session), true) => {
                // Starts where the account's route is known to stand: the rung
                // and the reasoning levels the last agent and the model list
                // found out.
                let provider = state.chatgpt.new_agent_provider();
                let mut cfg = ai::LlmConfig::with_credential(
                    provider as Arc<dyn ai::LlmProvider>,
                    ai::Credential::ChatGpt(session),
                );
                if let Some(m) = effective_model {
                    if !m.trim().is_empty() {
                        cfg = cfg.with_model(m);
                    }
                }
                if let Some(effort) = load_effort(&effective_provider_id) {
                    cfg = cfg.with_effort(effort);
                }
                Some(cfg)
            }
            _ => None,
        }
    } else {
        let provider = ai::validate::provider_for(&effective_provider_id);

        // A provider that needs no key (a local Ollama daemon) still has to
        // produce an agent. Before `requires_api_key` existed, the match below
        // was the hard stop: no key meant no agent, so a keyless provider
        // could never be used at all.
        let api_key = match api_key {
            Some(k) => Some(k),
            None if !provider.requires_api_key() => Some(String::new()),
            None => None,
        };
        api_key.map(|key| {
            let mut cfg = ai::LlmConfig::new(provider, key);
            if let Some(m) = effective_model {
                if !m.trim().is_empty() {
                    cfg = cfg.with_model(m);
                }
            }
            // Point at a local server, a gateway, or a proxy. Absent, the
            // provider's own URL applies.
            if let Some(base) = load_base_url(&effective_provider_id) {
                cfg = cfg.with_base_url(base);
            }
            // Absent, the model's own default applies. A provider that cannot
            // take one drops it in `effective_effort`.
            if let Some(effort) = load_effort(&effective_provider_id) {
                cfg = cfg.with_effort(effort);
            }
            cfg
        })
    };

    *slot = match (cfg, store_handle) {
        (Some(cfg), Some(store)) => {
            let plan_first = state.plan_first.load(Ordering::SeqCst);
            let mut agent = ai::Agent::new(
                cfg,
                Arc::clone(&state.dispatcher),
                store,
                Arc::clone(&state.engine),
                state.plan_gate.clone(),
                state.clipboard_handle(),
            );
            agent.set_plan_first(plan_first);
            let agent = match state.memory_handle() {
                Some(mem) => agent.with_memory(mem),
                None => agent,
            };
            let agent = agent.with_skills(state.skills_handle());
            let agent = match profile_body {
                Some(body) => agent.with_profile_body(body),
                None => agent,
            };
            let agent = match tool_whitelist {
                Some(list) => agent.with_tool_whitelist(list),
                None => agent,
            };
            Some(agent)
        }
        _ => None,
    };
}
