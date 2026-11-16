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

#[cfg(test)]
mod tests {
    //! Against an in-memory keychain and no Tauri window: what a save writes,
    //! in what order, how often the agent is built, and how the startup reads
    //! and refusals behave (ADR 0005).

    use super::*;
    use ai::keychain::KeychainError;
    use session::Store;
    use std::cell::{Cell, RefCell};
    use tempfile::tempdir;

    // -----------------------------------------------------------------
    // resolve_effective_llm — provider / key / model pairing
    // -----------------------------------------------------------------

    /// A profile pinning a model on a DIFFERENT provider must pull that
    /// provider's key from the keychain. Using the cached key (which
    /// belongs to the globally active provider) 401s at runtime.
    #[test]
    fn profile_on_other_provider_reloads_that_providers_key() {
        let (provider, key, model) = super::resolve_effective_llm(
            "anthropic".into(),
            Some("anthropic-key".into()),
            Some("claude-sonnet-5-5".into()),
            Some(("openai".into(), "gpt-4o-mini".into())),
            |p| {
                assert_eq!(p, "openai", "must look up the pinned provider");
                Some("openai-key".to_string())
            },
        );
        assert_eq!(provider, "openai");
        assert_eq!(key.as_deref(), Some("openai-key"));
        assert_eq!(model.as_deref(), Some("gpt-4o-mini"));
    }

    /// Same provider as the active one: the cached key is already
    /// correct, so no keychain round-trip is needed.
    #[test]
    fn profile_on_same_provider_keeps_cached_key() {
        let (provider, key, model) = super::resolve_effective_llm(
            "anthropic".into(),
            Some("anthropic-key".into()),
            Some("claude-sonnet-5-5".into()),
            Some(("anthropic".into(), "claude-opus-4-7".into())),
            |_| panic!("must not hit the keychain for the active provider"),
        );
        assert_eq!(provider, "anthropic");
        assert_eq!(key.as_deref(), Some("anthropic-key"));
        assert_eq!(model.as_deref(), Some("claude-opus-4-7"));
    }

    /// No profile override: the global picker's provider, key, and
    /// model all pass through untouched.
    #[test]
    fn no_profile_uses_global_picker() {
        let (provider, key, model) = super::resolve_effective_llm(
            "openrouter".into(),
            Some("or-key".into()),
            Some("anthropic/claude-sonnet-5.5".into()),
            None,
            |_| panic!("must not hit the keychain when no profile is active"),
        );
        assert_eq!(provider, "openrouter");
        assert_eq!(key.as_deref(), Some("or-key"));
        assert_eq!(model.as_deref(), Some("anthropic/claude-sonnet-5.5"));
    }

    /// A pinned provider with no stored key yields `None` rather than
    /// silently falling back to the wrong provider's key. `rebuild_agent`
    /// then leaves the agent unbuilt, which surfaces as the familiar
    /// "no agent configured" error instead of a confusing 401.
    #[test]
    fn pinned_provider_without_key_yields_none() {
        let (provider, key, _) = super::resolve_effective_llm(
            "anthropic".into(),
            Some("anthropic-key".into()),
            None,
            Some(("openai".into(), "gpt-4o-mini".into())),
            |_| None,
        );
        assert_eq!(provider, "openai");
        assert!(
            key.is_none(),
            "must not fall back to the active provider's key"
        );
    }

    // ---- The chosen model survives a restart (#249) ---------------

    /// The map lived only in memory and nothing re-read it at startup,
    /// so after a restart `rebuild_agent` saw no model and silently
    /// used `provider.default_model()` — while Settings went on
    /// showing the user's pick from localStorage.
    ///
    /// `restore_models` is the startup path with the keychain read
    /// injected, so this exercises the real wiring rather than a copy.
    #[test]
    fn a_restart_restores_the_last_selected_model() {
        let state = AppState::default();

        // Before: a fresh process knows nothing.
        assert_eq!(state.agent_settings.model_for("anthropic"), None);

        // What the keychain would hand back after the user picked one.
        restore_models(&state, |id| {
            Ok((id == "anthropic").then(|| "claude-opus-5".to_string()))
        });

        assert_eq!(
            state.agent_settings.model_for("anthropic").as_deref(),
            Some("claude-opus-5"),
            "the selection did not survive the restart, so rebuild_agent \
             would fall back to the provider default"
        );
    }

    /// Every provider's choice, not just the active one's: switching
    /// provider after a restart must not lose a model chosen earlier.
    #[test]
    fn a_restart_restores_every_provider_not_only_the_active_one() {
        let state = AppState::default();
        restore_models(&state, |id| Ok(Some(format!("{id}-chosen"))));

        for id in ai::SUPPORTED_PROVIDER_IDS {
            assert_eq!(
                state.agent_settings.model_for(id).as_deref(),
                Some(format!("{id}-chosen").as_str()),
                "provider {id} lost its model across the restart"
            );
        }
    }

    /// A provider the user never chose for must stay unset, so the
    /// agent uses that provider's own default rather than inheriting
    /// something arbitrary.
    #[test]
    fn a_provider_with_no_stored_choice_stays_unset() {
        let state = AppState::default();
        restore_models(&state, |_| Ok(None));

        for id in ai::SUPPORTED_PROVIDER_IDS {
            assert_eq!(state.agent_settings.model_for(id), None);
        }
    }

    // ---- A keychain that cannot be read is not a fresh install (#394) --

    /// Nothing stored, and every read answered: the Welcome flow is the
    /// right thing to show, so `has_api_key` answers `false` and does not
    /// reject.
    #[test]
    fn startup_with_nothing_stored_is_a_fresh_install() {
        let state = AppState::default();
        load_startup_settings(&state, || Ok(None), |_| Ok(None), |_| Ok(None));

        assert_eq!(state.agent_settings.active_provider_id(), ai::ANTHROPIC_ID);
        assert_eq!(state.agent_settings.api_key_snapshot(), None);
        assert_eq!(state.agent_settings.keychain_read_error(), None);
        assert_eq!(
            has_key_answer(state.agent_settings.keychain_read_error(), || Ok(false)),
            Ok(false)
        );
    }

    /// The bug: on a locked keyring every read failed, the failure was
    /// collapsed to "no key", and a user with a stored key was shown the
    /// first-run Welcome. The failure has to reach `has_api_key`.
    #[test]
    fn startup_read_failure_is_reported_not_treated_as_a_fresh_install() {
        let state = AppState::default();
        load_startup_settings(
            &state,
            || Err(KeychainError::new("locked")),
            |_| Ok(None),
            |_| Ok(None),
        );

        let stashed = state
            .agent_settings
            .keychain_read_error()
            .expect("the failure is kept");
        assert_eq!(stashed, "locked", "the reason alone, not the whole message");
        // Even though a live read would say "no key", `has_api_key` must
        // not: that read is about the wrong provider.
        let answer = has_key_answer(state.agent_settings.keychain_read_error(), || Ok(false));
        let err = answer.expect_err("must reject, not answer false");
        assert!(err.contains("locked"), "{err}");
    }

    #[test]
    fn startup_key_read_failure_is_reported() {
        let state = AppState::default();
        load_startup_settings(
            &state,
            || Ok(Some("openrouter".to_string())),
            |_| Err(KeychainError::new("dismissed the unlock prompt")),
            |_| Ok(None),
        );

        assert_eq!(state.agent_settings.active_provider_id(), "openrouter");
        assert_eq!(state.agent_settings.api_key_snapshot(), None);
        let stashed = state
            .agent_settings
            .keychain_read_error()
            .expect("the failure is kept");
        assert!(stashed.contains("dismissed the unlock prompt"), "{stashed}");
    }

    /// The first failure is the one reported, and a later one does not
    /// replace it.
    #[test]
    fn the_first_startup_failure_is_the_one_kept() {
        let state = AppState::default();
        load_startup_settings(
            &state,
            || Err(KeychainError::new("first")),
            |_| Err(KeychainError::new("second")),
            |_| Ok(None),
        );
        let stashed = state.agent_settings.keychain_read_error().unwrap();
        assert!(
            stashed.contains("first") && !stashed.contains("second"),
            "{stashed}"
        );
    }

    #[test]
    fn startup_restores_provider_and_key() {
        let state = AppState::default();
        load_startup_settings(
            &state,
            || Ok(Some("openrouter".to_string())),
            |id| Ok((id == "openrouter").then(|| "sk-or-1".to_string())),
            |id| Ok((id == "openrouter").then(|| "or-model".to_string())),
        );

        assert_eq!(state.agent_settings.active_provider_id(), "openrouter");
        assert_eq!(
            state.agent_settings.api_key_snapshot().as_deref(),
            Some("sk-or-1")
        );
        assert_eq!(
            state.agent_settings.model_for("openrouter").as_deref(),
            Some("or-model")
        );
        assert_eq!(state.agent_settings.keychain_read_error(), None);
        assert_eq!(
            has_key_answer(state.agent_settings.keychain_read_error(), || Ok(true)),
            Ok(true)
        );
    }

    /// A clean read after a failed one (the user saved a key, so the
    /// keychain is evidently working) clears the stash, or `has_api_key`
    /// would go on rejecting.
    #[test]
    fn a_later_clean_startup_read_clears_the_stash() {
        let state = AppState::default();
        state
            .agent_settings
            .set_keychain_read_error(Some("locked".to_string()));
        load_startup_settings(&state, || Ok(None), |_| Ok(None), |_| Ok(None));
        assert_eq!(state.agent_settings.keychain_read_error(), None);
    }

    // ---- Settings' reads after a refused startup (#497) ----------------

    /// A keychain that was asked and answered. The tests below pass it as
    /// the read, and count how often it ran.
    fn stored_url(asked: &Cell<u32>) -> Result<Option<String>, KeychainError> {
        asked.set(asked.get() + 1);
        Ok(Some("http://localhost:1234/v1".to_string()))
    }

    /// The bug: opening Settings read the base URL and the level from the
    /// keychain whatever startup had found, and on a locked keyring each
    /// read is an unlock prompt the user had just declined. After a refused
    /// startup the read is not made, and the answer says why.
    #[test]
    fn a_stored_setting_is_not_read_after_a_refused_startup() {
        let state = AppState::default();
        state.agent_settings.set_keychain_refused(true);
        state
            .agent_settings
            .set_keychain_read_error(Some("locked".into()));
        let asked = Cell::new(0);

        let err = stored_setting_unless_refused(&state, || stored_url(&asked))
            .expect_err("a keychain that refused is not asked again");

        assert_eq!(asked.get(), 0, "the keychain was not asked");
        assert!(
            err.contains("refused access") && err.contains("locked") && err.contains("not asked"),
            "{err}"
        );
    }

    /// Everything else reads as before: a clean startup, a failed read that
    /// was not a refusal (no prompt to avoid, and the read may well work),
    /// and the read after the refusal was lifted.
    #[test]
    fn a_stored_setting_is_read_unless_startup_was_refused() {
        let asked = Cell::new(0);
        let state = AppState::default();
        assert_eq!(
            stored_setting_unless_refused(&state, || stored_url(&asked)),
            Ok(Some("http://localhost:1234/v1".to_string()))
        );

        state
            .agent_settings
            .set_keychain_read_error(Some("no secret service".into()));
        assert!(stored_setting_unless_refused(&state, || stored_url(&asked)).is_ok());
        assert_eq!(asked.get(), 2);

        state.agent_settings.set_keychain_refused(true);
        assert!(stored_setting_unless_refused(&state, || stored_url(&asked)).is_err());
        keychain_readable_again_with(&state, |_| Ok(None));
        assert!(stored_setting_unless_refused(&state, || stored_url(&asked)).is_ok());
        assert_eq!(asked.get(), 3, "asked again once a save went through");

        // And a read that fails when it is made is still the keychain's reason.
        let failing = AppState::default();
        assert_eq!(
            stored_setting_unless_refused::<String>(&failing, || Err(KeychainError::new(
                "ambiguous"
            ))),
            Err("ambiguous".to_string())
        );
    }

    /// The model list reads two things (the saved key and the saved base
    /// URL), so two more prompts. After a refused startup it reads neither,
    /// even though a key typed into the form would make the first read
    /// unnecessary: the base URL would still be a guess.
    #[test]
    fn the_model_list_reads_neither_key_nor_base_url_after_a_refused_startup() {
        let state = AppState::default();
        state.agent_settings.set_keychain_refused(true);
        let asked = RefCell::new(Vec::new());
        let load = |what: &'static str| {
            let asked = &asked;
            move |provider: &str| {
                asked.borrow_mut().push(format!("{what}:{provider}"));
                Some(what.to_string())
            }
        };

        for typed in [None, Some("sk-typed".to_string())] {
            let err = catalogue_inputs_from(&state, "anthropic", typed, load("key"), load("base"))
                .expect_err("refused");
            assert!(err.contains("refused access"), "{err}");
        }
        assert!(asked.borrow().is_empty(), "asked {:?}", asked.borrow());
    }

    /// Without a refusal the inputs are what they were: the typed key wins
    /// and spares the keychain read of the key, and the base URL is read.
    #[test]
    fn the_model_list_reads_its_inputs_when_startup_was_not_refused() {
        let state = AppState::default();
        let asked = RefCell::new(Vec::new());
        let load = |what: &'static str| {
            let asked = &asked;
            move |provider: &str| {
                asked.borrow_mut().push(format!("{what}:{provider}"));
                Some(format!("stored-{what}"))
            }
        };

        assert_eq!(
            catalogue_inputs_from(&state, "groq", None, load("key"), load("base")),
            Ok((Some("stored-key".into()), Some("stored-base".into())))
        );
        assert_eq!(*asked.borrow(), ["key:groq", "base:groq"]);

        asked.borrow_mut().clear();
        assert_eq!(
            catalogue_inputs_from(
                &state,
                "groq",
                Some(" gsk-typed ".into()),
                load("key"),
                load("base")
            ),
            Ok((Some("gsk-typed".into()), Some("stored-base".into())))
        );
        assert_eq!(
            *asked.borrow(),
            ["base:groq"],
            "the typed key is not looked up"
        );
    }

    /// The bug: a read that failed came back as `None`, which Settings
    /// shows as an empty field and saves as "delete the override".
    #[test]
    fn a_stored_setting_that_could_not_be_read_is_an_error_not_none() {
        let err = stored_setting::<String>(Err(KeychainError::new("locked")))
            .expect_err("a failed read must reject");
        assert_eq!(err, "locked", "the reason alone: Settings says the rest");
        assert_eq!(stored_setting::<String>(Ok(None)), Ok(None));
        assert_eq!(
            stored_setting(Ok(Some("http://localhost:1234".to_string()))),
            Ok(Some("http://localhost:1234".to_string()))
        );
    }

    #[test]
    fn the_active_provider_is_not_answered_while_startup_was_unreadable() {
        let err = active_provider_answer(Some("locked".into()), "anthropic".into())
            .expect_err("the default is not what is stored");
        assert_eq!(err, "locked");
        assert_eq!(
            active_provider_answer(None, "openrouter".into()),
            Ok("openrouter".to_string())
        );
    }

    /// The bug: after a startup the keychain refused, the models were never
    /// restored, and "no model" came back as the empty answer Settings reads
    /// as "none chosen". A model that is known is an answer all the same.
    #[test]
    fn a_model_is_not_answered_as_none_chosen_while_startup_left_it_unread() {
        let why = || Some("the keychain refused access at launch".to_string());
        assert_eq!(
            active_model_answer(why(), None),
            Err("the keychain refused access at launch".to_string())
        );
        assert_eq!(
            active_model_answer(why(), Some("claude-opus-5".into())),
            Ok("claude-opus-5".to_string()),
            "restored before the refusal, or chosen since"
        );
        assert_eq!(active_model_answer(None, None), Ok(String::new()));
        assert_eq!(
            active_model_answer(None, Some("claude-opus-5".into())),
            Ok("claude-opus-5".to_string())
        );
    }

    /// Both ways startup can leave the models unread count: a refusal ends
    /// the reads, and a failed read may have been a model's own.
    #[test]
    fn the_models_are_unread_after_a_refusal_or_after_any_failed_read() {
        let clean = AppState::default();
        assert_eq!(models_unread_reason(&clean), None);

        let failed = AppState::default();
        failed
            .agent_settings
            .set_keychain_read_error(Some("ambiguous".into()));
        let reason = models_unread_reason(&failed).expect("a failed read");
        assert!(
            reason.contains("could not be read") && reason.contains("ambiguous"),
            "{reason}"
        );

        let refused = AppState::default();
        refused.agent_settings.set_keychain_refused(true);
        let reason = models_unread_reason(&refused).expect("a refusal");
        assert!(
            reason.contains("refused access") && !reason.contains("()"),
            "{reason}"
        );
        refused
            .agent_settings
            .set_keychain_read_error(Some("locked".into()));
        assert!(models_unread_reason(&refused).unwrap().contains("(locked)"));
    }

    #[test]
    fn has_key_answer_passes_a_live_read_failure_through() {
        let answer = has_key_answer(None, || Err(KeychainError::new("locked")));
        let err = answer.expect_err("a read failure is not `false`");
        assert_eq!(
            err, "locked",
            "the reason alone: the banner supplies the rest"
        );
    }

    // ---- A locked keychain is asked once, not once per slot (#394) -----

    /// The user dismissed the unlock prompt. Every further read would
    /// raise it again, so startup must stop at the first refusal instead
    /// of asking for the active provider, its key and six models in a row.
    #[test]
    fn a_refused_active_provider_read_ends_startup_there() {
        let state = AppState::default();
        let reads = Cell::new(0u32);
        let counted = || reads.set(reads.get() + 1);
        load_startup_settings(
            &state,
            || {
                counted();
                Err(KeychainError::access_denied("prompt dismissed"))
            },
            |_| {
                counted();
                Ok(Some("sk-x".to_string()))
            },
            |_| {
                counted();
                Ok(Some("a-model".to_string()))
            },
        );

        assert_eq!(reads.get(), 1, "nothing was read after the refusal");
        assert_eq!(
            state.agent_settings.keychain_read_error().as_deref(),
            Some("prompt dismissed")
        );
        assert_eq!(state.agent_settings.api_key_snapshot(), None);
        assert_eq!(state.agent_settings.model_for("anthropic"), None);
    }

    #[test]
    fn a_refused_key_read_skips_the_model_reads() {
        let state = AppState::default();
        let model_reads = Cell::new(0u32);
        load_startup_settings(
            &state,
            || Ok(None),
            |_| Err(KeychainError::access_denied("prompt dismissed")),
            |_| {
                model_reads.set(model_reads.get() + 1);
                Ok(None)
            },
        );

        assert_eq!(model_reads.get(), 0);
        assert_eq!(
            state.agent_settings.keychain_read_error().as_deref(),
            Some("prompt dismissed")
        );
    }

    /// Refused partway through the models: the ones already read stay,
    /// and the rest are left alone.
    #[test]
    fn a_refused_model_read_ends_the_loop_and_keeps_what_was_read() {
        let state = AppState::default();
        let asked = std::cell::RefCell::new(Vec::new());
        restore_models(&state, |id| {
            asked.borrow_mut().push(id.to_string());
            if asked.borrow().len() == 3 {
                Err(KeychainError::access_denied("prompt dismissed"))
            } else {
                Ok(Some(format!("{id}-chosen")))
            }
        });

        let asked = asked.into_inner();
        assert_eq!(asked.len(), 3, "stopped at the refusal: {asked:?}");
        assert_eq!(
            state.agent_settings.model_for(&asked[0]).as_deref(),
            Some(format!("{}-chosen", asked[0]).as_str())
        );
        assert_eq!(state.agent_settings.model_for(&asked[2]), None);
    }

    /// Any other failure is about one slot, so the next one is still
    /// worth reading, and the failure is not reported as the keychain
    /// being unreadable (the key and provider came back fine).
    #[test]
    fn a_model_slot_that_cannot_be_decoded_does_not_end_startup() {
        let state = AppState::default();
        load_startup_settings(
            &state,
            || Ok(None),
            |_| Ok(Some("sk-x".to_string())),
            |id| {
                if id == "anthropic" {
                    Err(KeychainError::new("not UTF-8"))
                } else {
                    Ok(Some(format!("{id}-chosen")))
                }
            },
        );

        assert_eq!(state.agent_settings.keychain_read_error(), None);
        assert_eq!(
            state.agent_settings.api_key_snapshot().as_deref(),
            Some("sk-x")
        );
        assert_eq!(state.agent_settings.model_for("anthropic"), None);
        assert_eq!(
            state.agent_settings.model_for("openrouter").as_deref(),
            Some("openrouter-chosen")
        );
    }

    /// A failure that is not a refusal does not end startup's reads: the
    /// models are still restored after a key slot that would not decode.
    #[test]
    fn an_unreadable_key_slot_that_is_not_a_refusal_still_restores_models() {
        let state = AppState::default();
        load_startup_settings(
            &state,
            || Ok(None),
            |_| Err(KeychainError::new("not UTF-8")),
            |id| Ok((id == "anthropic").then(|| "m".to_string())),
        );

        assert_eq!(
            state.agent_settings.keychain_read_error().as_deref(),
            Some("not UTF-8")
        );
        assert_eq!(
            state.agent_settings.model_for("anthropic").as_deref(),
            Some("m")
        );
    }

    // ---- Models come back once the keychain does (#394) ----------------

    /// Startup was refused, so the chosen models were never read, and the
    /// first save that worked cleared the failure without reading them.
    /// The agent then ran the provider's default model while Settings went
    /// on showing the stored pick.
    #[test]
    fn a_save_after_a_refused_startup_restores_the_models_startup_skipped() {
        let state = AppState::default();
        load_startup_settings(
            &state,
            || Err(KeychainError::access_denied("prompt dismissed")),
            |_| Ok(None),
            |_| Ok(Some("never read".to_string())),
        );
        assert_eq!(
            state.agent_settings.model_for("anthropic"),
            None,
            "startup skipped them"
        );

        keychain_readable_again_with(&state, |id| {
            Ok((id == "anthropic").then(|| "claude-opus-5".to_string()))
        });

        assert_eq!(
            state.agent_settings.keychain_read_error(),
            None,
            "the stash is cleared"
        );
        assert_eq!(
            state.agent_settings.model_for("anthropic").as_deref(),
            Some("claude-opus-5"),
            "the stored pick is back, so rebuild_agent uses it"
        );
    }

    /// A model picked after the failure may be newer than the stored one
    /// (the pick could not be saved while the keychain was locked), so
    /// recovery fills gaps and overwrites nothing.
    #[test]
    fn recovery_does_not_overwrite_a_model_picked_since() {
        let state = AppState::default();
        state
            .agent_settings
            .set_keychain_read_error(Some("locked".to_string()));
        state
            .agent_settings
            .set_model_for("anthropic".to_string(), "picked-today".to_string());

        keychain_readable_again_with(&state, |id| Ok(Some(format!("{id}-stored"))));

        assert_eq!(
            state.agent_settings.model_for("anthropic").as_deref(),
            Some("picked-today")
        );
        assert_eq!(
            state.agent_settings.model_for("openrouter").as_deref(),
            Some("openrouter-stored")
        );
    }

    /// Every successful save passes through here, and almost none follow a
    /// failed startup. Those must not read eight model slots each.
    #[test]
    fn a_save_with_no_failure_kept_reads_no_models() {
        let state = AppState::default();
        let reads = Cell::new(0u32);
        keychain_readable_again_with(&state, |_| {
            reads.set(reads.get() + 1);
            Ok(None)
        });
        assert_eq!(reads.get(), 0);
        assert_eq!(state.agent_settings.keychain_read_error(), None);
    }

    // ---- Switching to a provider that needs no key ---------------------

    /// Ollama has no key to read. Asking the keychain anyway raised an
    /// unlock prompt on a locked keyring, and a failure the switch then
    /// reported over a provider that works with nothing stored.
    #[test]
    fn switching_to_a_keyless_provider_does_not_read_the_keychain() {
        let reads = Cell::new(0u32);
        let got = key_for_switch("ollama", |_| {
            reads.set(reads.get() + 1);
            Err(KeychainError::access_denied("locked"))
        });
        assert_eq!(got, Ok(None));
        assert_eq!(reads.get(), 0);
    }

    #[test]
    fn switching_to_a_provider_that_needs_a_key_reads_it() {
        let got = key_for_switch("openrouter", |id| {
            assert_eq!(id, "openrouter");
            Ok(Some("sk-or-1".to_string()))
        });
        assert_eq!(got, Ok(Some("sk-or-1".to_string())));

        let err = key_for_switch("openrouter", |_| {
            Err(KeychainError::access_denied("locked"))
        })
        .expect_err("a read failure is reported, not turned into no key");
        assert_eq!(err.reason(), "locked");
    }

    #[test]
    fn the_persistence_dto_says_why_it_is_not_persistent() {
        let durable: KeychainPersistenceDto = ai::keychain::Persistence::Durable.into();
        assert!(durable.persistent && durable.reason.is_none());
        let session: KeychainPersistenceDto = ai::keychain::Persistence::SessionOnly {
            reason: "no secret service".to_string(),
        }
        .into();
        assert!(!session.persistent);
        assert_eq!(session.reason.as_deref(), Some("no secret service"));
    }

    // ---- Reasoning effort setting -------------------------------------

    #[test]
    fn every_level_is_accepted_for_anthropic() {
        for level in ai::Effort::ALL {
            assert_eq!(
                parse_effort_setting("anthropic", Some(level.as_str())),
                Ok(Some(level))
            );
        }
        // Stray whitespace around a real level is the level.
        assert_eq!(
            parse_effort_setting("anthropic", Some("  xhigh ")),
            Ok(Some(ai::Effort::XHigh))
        );
    }

    /// "Default" in the UI: no value, or a blank one, clears the slot.
    /// Valid for every provider, so a stale slot can always be cleared.
    #[test]
    fn no_value_or_a_blank_one_means_the_models_default() {
        for id in ai::SUPPORTED_PROVIDER_IDS {
            assert_eq!(parse_effort_setting(id, None), Ok(None), "{id}");
            assert_eq!(parse_effort_setting(id, Some("")), Ok(None), "{id}");
            assert_eq!(parse_effort_setting(id, Some("  ")), Ok(None), "{id}");
        }
    }

    #[test]
    fn a_level_the_api_would_reject_is_refused_at_save_time() {
        for bad in ["extra high", "XHigh", "ultra", "0", "low;"] {
            let err = parse_effort_setting("anthropic", Some(bad)).unwrap_err();
            assert!(err.contains("unknown reasoning effort"), "{bad}: {err}");
            assert!(err.contains("xhigh"), "the error lists the levels: {err}");
        }
    }

    #[test]
    fn a_provider_with_no_such_setting_refuses_a_level() {
        for id in ai::SUPPORTED_PROVIDER_IDS
            .iter()
            .filter(|id| !ai::validate::provider_for(id).supports_effort())
        {
            let err = parse_effort_setting(id, Some("high")).unwrap_err();
            assert!(err.contains("no reasoning effort setting"), "{id}: {err}");
        }
    }

    /// A saved key is the normal case: the form is empty once a key is
    /// saved, and a catalogue fetched with no key is a catalogue that
    /// cannot be live.
    #[test]
    fn the_catalogue_uses_the_saved_key_when_none_is_typed() {
        let saved = |p: &str| {
            assert_eq!(p, "anthropic", "the saved key of the provider on screen");
            Some("saved".to_string())
        };
        assert_eq!(
            super::catalogue_key("anthropic", None, saved).as_deref(),
            Some("saved")
        );
        assert_eq!(
            super::catalogue_key("anthropic", Some("  ".into()), saved).as_deref(),
            Some("saved"),
            "a blank field is no key"
        );
        assert_eq!(
            super::catalogue_key("anthropic", Some("t".into()), |_| {
                panic!("a typed key must not read the keychain")
            })
            .as_deref(),
            Some("t")
        );
        assert_eq!(
            super::catalogue_key("anthropic", Some(" t ".into()), |_| None).as_deref(),
            Some("t"),
            "the typed key is trimmed"
        );
        assert_eq!(super::catalogue_key("ollama", None, |_| None), None);
    }

    // ---- The agent after a refused startup (#497) ----------------------

    /// A state an agent can be built in: the default project's store is
    /// open, as `run` leaves it. Without one nothing is ever built whatever
    /// the keychain says, and the tests below would prove nothing about it.
    fn state_with_a_store(dir: &std::path::Path) -> AppState {
        let state = AppState::default();
        // Through the registry, as the real opens are: `set_open_project`
        // takes nothing else (#566).
        let store = Store::open_shared(dir).expect("store");
        state.set_open_project(store, dir.to_path_buf());
        state
    }

    /// An agent profile that pins `openai`'s model, active in `state`.
    fn pin_openai_profile(state: &AppState, dir: &std::path::Path) {
        std::fs::write(
            dir.join("pinned.md"),
            "---\ndescription: pins another provider\nmodel:\n  provider: openai\n  id: gpt-4o-mini\n---\nbody\n",
        )
        .expect("profile");
        state.install_agent_profile_library(dir.to_path_buf());
        state
            .set_active_agent_profile(Some("pinned".to_string()))
            .expect("select it");
    }

    /// What one rebuild asked the keychain for ("key:openai",
    /// "base_url:ollama", "effort:ollama"), and whether it built an agent.
    fn rebuild_and_record(state: &AppState) -> (Vec<String>, bool) {
        let asked = RefCell::new(Vec::new());
        let mut slot = None;
        rebuild_agent_from(
            state,
            &mut slot,
            |provider| {
                asked.borrow_mut().push(format!("key:{provider}"));
                Some("stored-key".to_string())
            },
            |provider| {
                asked.borrow_mut().push(format!("base_url:{provider}"));
                None
            },
            |provider| {
                asked.borrow_mut().push(format!("effort:{provider}"));
                None
            },
        );
        (asked.into_inner(), slot.is_some())
    }

    /// The bug: `rebuild_agent_into` reads the base URL and the level of
    /// whatever provider it builds for, and it runs at startup right after
    /// the reads. After a startup the keychain refused, a keyless active
    /// provider (a local Ollama) was therefore built, and read the
    /// keychain twice more, each an unlock prompt on a keyring the user had
    /// just declined to unlock.
    ///
    /// The agent is left unbuilt instead. It cannot be built from guesses:
    /// the stored URL may be a gateway's, and the built-in one is not.
    #[test]
    fn a_refused_startup_leaves_the_agent_unbuilt_and_the_keychain_unasked() {
        let dir = tempdir().unwrap();
        let state = state_with_a_store(dir.path());
        state
            .agent_settings
            .set_active_provider("ollama".to_string());
        state.agent_settings.set_keychain_refused(true);
        state
            .agent_settings
            .set_keychain_read_error(Some("locked".into()));

        let (asked, built) = rebuild_and_record(&state);

        assert!(asked.is_empty(), "the keychain was asked for {asked:?}");
        assert!(!built, "an agent built without its stored endpoint");
    }

    /// The same for a profile that pins another provider's model, which
    /// reads that provider's key as well.
    #[test]
    fn a_refused_startup_does_not_read_the_key_of_a_provider_a_profile_pins() {
        let dir = tempdir().unwrap();
        let agents = tempdir().unwrap();
        let state = state_with_a_store(dir.path());
        state
            .agent_settings
            .set_api_key_cache(Some("anthropic-key".to_string()));
        pin_openai_profile(&state, agents.path());
        state.agent_settings.set_keychain_refused(true);

        let (asked, built) = rebuild_and_record(&state);

        assert!(asked.is_empty(), "the keychain was asked for {asked:?}");
        assert!(!built);
    }

    /// And it is the refusal that does it, not the setup: the same states,
    /// readable, build an agent and read what they always read. Without
    /// this the two tests above would pass for a rebuild that never built
    /// anything.
    #[test]
    fn without_a_refusal_the_rebuild_reads_the_keychain_and_builds() {
        let dir = tempdir().unwrap();
        let state = state_with_a_store(dir.path());
        state
            .agent_settings
            .set_active_provider("ollama".to_string());
        let (asked, built) = rebuild_and_record(&state);
        assert_eq!(asked, ["base_url:ollama", "effort:ollama"]);
        assert!(built, "a keyless provider needs no key to be built");

        let agents = tempdir().unwrap();
        let state = state_with_a_store(dir.path());
        state
            .agent_settings
            .set_api_key_cache(Some("anthropic-key".to_string()));
        pin_openai_profile(&state, agents.path());
        let (asked, built) = rebuild_and_record(&state);
        assert_eq!(asked, ["key:openai", "base_url:openai", "effort:openai"]);
        assert!(built);

        // A failed read that was not a refusal raises no prompt to avoid,
        // so it does not stop the build either.
        let state = state_with_a_store(dir.path());
        state
            .agent_settings
            .set_active_provider("ollama".to_string());
        state
            .agent_settings
            .set_keychain_read_error(Some("no secret service".into()));
        assert!(rebuild_and_record(&state).1);
    }

    /// The refusal ends with the first save that goes through, and the
    /// rebuild after it reads and builds again.
    #[test]
    fn the_rebuild_after_a_lifted_refusal_builds_the_agent() {
        let dir = tempdir().unwrap();
        let state = state_with_a_store(dir.path());
        state
            .agent_settings
            .set_active_provider("ollama".to_string());
        state.agent_settings.set_keychain_refused(true);
        assert!(!rebuild_and_record(&state).1, "precondition");

        keychain_readable_again_with(&state, |_| Ok(None));

        let (asked, built) = rebuild_and_record(&state);
        assert_eq!(asked, ["base_url:ollama", "effort:ollama"]);
        assert!(built);
    }

    // ---- The ChatGPT plan has a session, not a key (#474) -----------------
    //
    // Nothing here builds an agent for `chatgpt`, makes it the active
    // provider or says it is configured unless its session is signed in, and
    // no key or address is ever stored or read for it, signed in or not: a
    // stored key would be sent as the "plan" bearer and bill the API.

    use crate::chatgpt::test_support::{open_session, signed_in_handle};

    fn pin_profile(state: &AppState, dir: &std::path::Path, provider: &str) {
        std::fs::write(
            dir.join("pinned.md"),
            format!(
                "---\ndescription: pins a provider\nmodel:\n  provider: {provider}\n  id: some-model\n---\nbody\n"
            ),
        )
        .expect("profile");
        state.install_agent_profile_library(dir.to_path_buf());
        state
            .set_active_agent_profile(Some("pinned".to_string()))
            .expect("select it");
    }

    /// An `AppState` with its ChatGPT session, and the folder the session's
    /// files are in, which lives as long as the rig does.
    struct ChatGptRig {
        state: AppState,
        _config: tempfile::TempDir,
    }

    impl std::ops::Deref for ChatGptRig {
        type Target = AppState;
        fn deref(&self) -> &AppState {
            &self.state
        }
    }

    /// A state whose ChatGPT session is signed in (`true`) or signed out.
    fn state_with_chatgpt(dir: &std::path::Path, signed_in: bool) -> ChatGptRig {
        let state = state_with_a_store(dir);
        let config = tempdir().unwrap();
        let (handle, _store) = if signed_in {
            signed_in_handle(config.path())
        } else {
            let handle = crate::chatgpt::ChatGptHandle::default();
            let session = open_session(
                config.path(),
                Arc::new(crate::chatgpt::test_support::MemoryStore::default()),
                false,
            );
            handle.install(
                session,
                crate::chatgpt::Hooking {
                    persistence: Arc::new(|| None),
                    refusal: Arc::new(|| None),
                    refresh_models: false,
                    private_dir: None,
                },
            );
            (handle, Arc::default())
        };
        ChatGptRig {
            state: AppState {
                chatgpt: handle,
                ..state
            },
            _config: config,
        }
    }

    /// A state with no ChatGPT session at all.
    fn bare_rig() -> ChatGptRig {
        ChatGptRig {
            state: AppState::default(),
            _config: tempdir().unwrap(),
        }
    }

    /// A state whose settings are over `keychain`, as `rig` made it.
    fn rig_over(rig: ChatGptRig, keychain: &Arc<FakeKeychain>) -> ChatGptRig {
        ChatGptRig {
            state: AppState {
                agent_settings: AgentSettings::with_keychain(keychain.clone()),
                ..rig.state
            },
            _config: rig._config,
        }
    }

    #[test]
    fn the_chatgpt_plan_is_refused_as_the_active_provider_unless_signed_in() {
        for (what, rig) in [
            ("no session at all", bare_rig()),
            (
                "signed out",
                state_with_chatgpt(tempdir().unwrap().path(), false),
            ),
        ] {
            let keychain = Arc::new(FakeKeychain::default());
            let state = rig_over(rig, &keychain);
            state
                .agent_settings
                .set_active_provider("anthropic".to_string());
            let err = run(&state, change("chatgpt")).expect_err("refused");
            assert!(err.contains("sign in with ChatGPT"), "{what}: {err}");
            assert!(
                keychain.wrote().is_empty(),
                "{what}: wrote {:?}",
                keychain.wrote()
            );
            assert_eq!(
                state.agent_settings.active_provider_id(),
                "anthropic",
                "{what}"
            );
            assert_eq!(state.agent_settings.rebuilds(), 0, "{what}");
        }
    }

    #[test]
    fn the_chatgpt_plan_can_be_the_active_provider_when_signed_in_and_builds_its_agent() {
        let dir = tempdir().unwrap();
        let keychain = Arc::new(FakeKeychain::default());
        let state = rig_over(state_with_chatgpt(dir.path(), true), &keychain);
        state
            .agent_settings
            .set_active_provider("anthropic".to_string());
        state
            .agent_settings
            .set_api_key_cache(Some("anthropic-key".to_string()));
        let at = run(&state, change("chatgpt")).expect("allowed");
        assert!(at.persistent);
        assert_eq!(
            keychain.wrote(),
            ["save active_provider"],
            "and nothing else"
        );
        assert_eq!(state.agent_settings.active_provider_id(), "chatgpt");
        assert_eq!(
            state.agent_settings.api_key_snapshot(),
            None,
            "the other provider's key is not carried over"
        );
        // The agent was built, on the session.
        let slot = tauri::async_runtime::block_on(state.agent.lock());
        assert!(slot.is_some(), "an agent is built for the signed-in plan");
        assert!(state.chatgpt.has_agent());
    }

    /// A model and a level are saved for the plan like any provider's, in the
    /// same one call; a key or an address never is.
    #[test]
    fn the_chatgpt_plan_saves_its_model_and_level_but_never_a_key_or_an_address() {
        for signed_in in [false, true] {
            let keychain = Arc::new(FakeKeychain::default());
            let state = rig_over(
                state_with_chatgpt(tempdir().unwrap().path(), signed_in),
                &keychain,
            );
            for (what, refused) in [
                (
                    "a key",
                    SettingsChange {
                        key: Some("sk-would-be-the-bearer".into()),
                        ..change("chatgpt")
                    },
                ),
                (
                    "an address",
                    SettingsChange {
                        base_url: Some("http://evil.example/v1".into()),
                        ..change("chatgpt")
                    },
                ),
                (
                    "clearing an address",
                    SettingsChange {
                        base_url: Some("".into()),
                        ..change("chatgpt")
                    },
                ),
            ] {
                let err = run(&state, refused).expect_err(what);
                assert!(err.contains("ChatGPT"), "{what}: {err}");
            }
            assert!(keychain.wrote().is_empty(), "wrote {:?}", keychain.wrote());
            assert_eq!(state.agent_settings.api_key_snapshot(), None);
        }

        let keychain = Arc::new(FakeKeychain::default());
        let state = rig_over(
            state_with_chatgpt(tempdir().unwrap().path(), true),
            &keychain,
        );
        run(
            &state,
            SettingsChange {
                model: Some("gpt-5".into()),
                effort: Some("high".into()),
                ..change("chatgpt")
            },
        )
        .expect("saved");
        assert_eq!(
            keychain.wrote(),
            ["save effort", "save active_provider", "save model"]
        );
    }

    /// The old per-field address command did not look at the provider id at all.
    #[test]
    fn nothing_is_stored_for_a_provider_that_does_not_exist() {
        let keychain = Arc::new(FakeKeychain::default());
        let state = state_over(&keychain);
        let err = run(
            &state,
            SettingsChange {
                base_url: Some("http://localhost:1/v1".into()),
                ..change("nonsense")
            },
        )
        .expect_err("refused");
        assert!(err.contains("unsupported provider id"), "{err}");
        assert!(keychain.wrote().is_empty());
    }

    #[test]
    fn the_chatgpt_plan_is_configured_only_when_signed_in_and_reads_no_key() {
        assert!(matches!(
            provider_is_configured(&AppState::default(), "chatgpt"),
            Ok(false)
        ));
        let signed_out = state_with_chatgpt(tempdir().unwrap().path(), false);
        assert!(matches!(
            provider_is_configured(&signed_out, "chatgpt"),
            Ok(false)
        ));
        let signed_in = state_with_chatgpt(tempdir().unwrap().path(), true);
        assert!(matches!(
            provider_is_configured(&signed_in, "chatgpt"),
            Ok(true)
        ));
        // Either way no key slot is read for it.
        let read = key_for_switch("chatgpt", |_| panic!("a session provider has no key slot"));
        assert!(matches!(read, Ok(None)));
        // The ones that do take a key still ask for it.
        assert!(matches!(
            key_for_switch("openai", |_| Ok(Some("k".into()))),
            Ok(Some(k)) if k == "k"
        ));
        assert!(matches!(
            provider_is_configured(&signed_in, "ollama"),
            Ok(true)
        ));
    }

    #[test]
    fn no_agent_is_built_for_the_chatgpt_plan_unless_signed_in_whatever_else_is_stored() {
        let dir = tempdir().unwrap();
        {
            let signed_in = false;
            // Active, with a key that a hand-edited keychain or an older
            // build left behind.
            let state = state_with_chatgpt(dir.path(), signed_in);
            state
                .agent_settings
                .set_active_provider("chatgpt".to_string());
            state
                .agent_settings
                .set_api_key_cache(Some("sk-left-behind".to_string()));
            let (_, built) = rebuild_and_record(&state);
            assert!(
                !built,
                "a keyed agent was built for the signed-out ChatGPT plan"
            );

            // Active with no key at all: not a keyless agent either.
            let state = state_with_chatgpt(dir.path(), signed_in);
            state
                .agent_settings
                .set_active_provider("chatgpt".to_string());
            let (_, built) = rebuild_and_record(&state);
            assert!(!built, "a keyless agent was built for the ChatGPT plan");

            // A profile that pins it.
            let agents = tempdir().unwrap();
            let state = state_with_chatgpt(dir.path(), signed_in);
            state
                .agent_settings
                .set_api_key_cache(Some("anthropic-key".to_string()));
            pin_profile(&state, agents.path(), "chatgpt");
            let (asked, built) = rebuild_and_record(&state);
            assert!(!built, "a profile built an agent for the signed-out plan");
            assert!(
                !asked.iter().any(|a| a.starts_with("key:")),
                "its key slot was read: {asked:?}"
            );
        }

        // And a profile that pins another provider still builds, so the
        // above is the provider's doing and not the profile's.
        let agents = tempdir().unwrap();
        let state = state_with_chatgpt(dir.path(), false);
        pin_profile(&state, agents.path(), "openai");
        assert!(rebuild_and_record(&state).1);
    }

    #[test]
    fn a_signed_in_chatgpt_plan_builds_an_agent_on_its_session_and_reads_no_key_or_address() {
        let dir = tempdir().unwrap();
        // Active, with a stale key left in the cache.
        let state = state_with_chatgpt(dir.path(), true);
        state
            .agent_settings
            .set_active_provider("chatgpt".to_string());
        state
            .agent_settings
            .set_api_key_cache(Some("sk-left-behind".to_string()));
        let (asked, built) = rebuild_and_record(&state);
        assert!(built, "the signed-in plan has an agent");
        assert_eq!(
            asked,
            ["effort:chatgpt"],
            "no key and no address: only the level the person chose"
        );
        assert!(state.chatgpt.has_agent());

        // A profile that pins it builds too, on the session, with no key read.
        let agents = tempdir().unwrap();
        let state = state_with_chatgpt(dir.path(), true);
        state
            .agent_settings
            .set_api_key_cache(Some("anthropic-key".to_string()));
        pin_profile(&state, agents.path(), "chatgpt");
        let (asked, built) = rebuild_and_record(&state);
        assert!(built, "a profile that pins the signed-in plan builds");
        assert!(
            !asked
                .iter()
                .any(|a| a.starts_with("key:") || a.starts_with("base_url:")),
            "{asked:?}"
        );

        // Replacing it with another provider's agent lets go of the plan's.
        let state = {
            let s = state_with_chatgpt(dir.path(), true);
            s.agent_settings.set_active_provider("chatgpt".to_string());
            s
        };
        assert!(rebuild_and_record(&state).1);
        assert!(state.chatgpt.has_agent());
        state
            .agent_settings
            .set_active_provider("ollama".to_string());
        assert!(rebuild_and_record(&state).1);
        assert!(!state.chatgpt.has_agent());
    }

    /// The rung a running agent ended on is carried across a rebuild
    /// (`with_tool_rung`), and an account change starts again. Read from the
    /// provider the agent in the slot actually holds, which is the handle's.
    #[test]
    fn the_tool_rung_is_carried_across_a_rebuild_and_reset_on_a_new_account() {
        use ai::chatgpt::provider::ToolRung;
        let dir = tempdir().unwrap();
        let state = state_with_chatgpt(dir.path(), true);
        state
            .agent_settings
            .set_active_provider("chatgpt".to_string());
        let mut slot = None;

        // The agent's provider, checked to be the slot's own.
        let provider_of = |slot: &Option<ai::Agent>| {
            let agent = slot.as_ref().expect("an agent is built");
            let held = state.chatgpt.agent_provider().expect("the handle's");
            assert_eq!(
                Arc::as_ptr(&agent.llm_config().provider).cast::<()>(),
                Arc::as_ptr(&held).cast::<()>(),
                "the agent in the slot runs on the handle's provider"
            );
            held
        };

        rebuild_agent_from(&state, &mut slot, |_| None, |_| None, |_| None);
        let first = provider_of(&slot);
        assert_eq!(first.tool_rung(), ToolRung::NamespaceFull);

        // The route refuses the tools during a turn: the agent's provider
        // steps down. The agent is rebuilt (a setting changed).
        first.adopt_rung(ToolRung::AdditionalTools);
        rebuild_agent_from(&state, &mut slot, |_| None, |_| None, |_| None);
        let second = provider_of(&slot);
        assert!(!Arc::ptr_eq(&first, &second), "a new provider was built");
        assert_eq!(
            second.tool_rung(),
            ToolRung::AdditionalTools,
            "and it starts where the last ended"
        );

        // A new account: the rebuilt agent starts at the top again.
        state.chatgpt.account_changed();
        rebuild_agent_from(&state, &mut slot, |_| None, |_| None, |_| None);
        assert_eq!(provider_of(&slot).tool_rung(), ToolRung::NamespaceFull);
    }

    /// A saved sign-in the keychain could not be asked for is not "not
    /// signed in" (#394): the answer `false` sends the person to sign in
    /// again over a session that may well be there.
    #[test]
    fn a_saved_chatgpt_sign_in_that_could_not_be_read_is_an_error_not_false() {
        let config = tempdir().unwrap();
        let session = open_session(
            config.path(),
            Arc::new(crate::chatgpt::test_support::MemoryStore::default()),
            true,
        );
        let handle = crate::chatgpt::ChatGptHandle::default();
        handle.install(
            session,
            crate::chatgpt::Hooking {
                persistence: Arc::new(|| None),
                refusal: Arc::new(|| Some("the keychain refused access at launch".into())),
                refresh_models: false,
                private_dir: None,
            },
        );
        tauri::async_runtime::block_on(handle.restore());
        assert_eq!(handle.status().state, "unread", "precondition");
        let state = AppState {
            chatgpt: handle,
            ..AppState::default()
        };

        let err = provider_is_configured(&state, "chatgpt").expect_err("not false");
        assert!(err.reason().contains("refused access"), "{}", err.reason());
        // Through the commands' own answer, as `has_api_key` gives it.
        let answer = has_key_answer(None, || provider_is_configured(&state, "chatgpt"));
        assert!(answer.is_err(), "{answer:?}");

        // A session that is simply not signed in is still `false`.
        let out = state_with_chatgpt(tempdir().unwrap().path(), false);
        assert!(matches!(provider_is_configured(&out, "chatgpt"), Ok(false)));
    }

    /// There is no key to set for the plan: the missing agent says to sign
    /// in, in words that still read as "no agent" to the chat, which then
    /// offers Settings.
    #[test]
    fn the_missing_agent_of_the_chatgpt_plan_asks_for_a_sign_in_not_a_key() {
        let out = state_with_chatgpt(tempdir().unwrap().path(), false);
        out.agent_settings
            .set_active_provider("chatgpt".to_string());
        let text = crate::commands::no_agent_error(&out).to_string();
        assert!(text.contains("Continue with ChatGPT in Settings"), "{text}");
        assert!(!text.contains("set_api_key"), "{text}");
        assert!(text.contains("no agent configured"), "{text}");

        // A profile that pins it, with another provider active.
        let agents = tempdir().unwrap();
        let pinned = state_with_chatgpt(tempdir().unwrap().path(), false);
        pin_profile(&pinned, agents.path(), "chatgpt");
        assert!(crate::commands::no_agent_error(&pinned)
            .to_string()
            .contains("Continue with ChatGPT"));

        // Everyone else is told what they always were.
        let other = state_with_chatgpt(tempdir().unwrap().path(), false);
        other
            .agent_settings
            .set_active_provider("openai".to_string());
        assert!(crate::commands::no_agent_error(&other)
            .to_string()
            .contains("set_api_key"));
    }

    // ---- Plan first across a restart (#569) -----------------------------

    /// An agent built the way startup builds one: a state with a store, a
    /// keyless provider (so no key is needed) and the settings file at
    /// `settings`, read as `run` reads it.
    fn agent_built_at_launch(settings: &std::path::Path) -> ai::Agent {
        let project = tempdir().unwrap();
        let state = state_with_a_store(project.path());
        state
            .agent_settings
            .set_active_provider("ollama".to_string());
        state.install_settings(settings.to_path_buf());
        let mut slot = None;
        rebuild_agent_from(&state, &mut slot, |_| None, |_| None, |_| None);
        slot.expect("a keyless provider builds an agent")
    }

    /// The bug: Plan first is an approval gate, and it was off again after
    /// every restart. The first turn after one ran its edits with no card,
    /// and the composer's toggle gave no sign of it. The agent startup
    /// builds must already hold edits when the person had it on, without
    /// the toggle being touched.
    #[test]
    fn the_agent_built_at_launch_holds_edits_when_plan_first_was_left_on() {
        let home = tempdir().unwrap();
        let settings = home.path().join(".edytlab").join("settings.json");

        // The last launch: the person turns it on.
        let before = AppState::new();
        before.install_settings(settings.clone());
        before.save_plan_first(true).expect("saved");
        drop(before);

        assert!(agent_built_at_launch(&settings).plan_first());
    }

    /// And it is the file that does it: with nothing saved, the same
    /// startup builds an agent that acts directly, as a fresh install does.
    #[test]
    fn the_agent_built_at_launch_acts_directly_when_nothing_was_saved() {
        let home = tempdir().unwrap();
        let settings = home.path().join(".edytlab").join("settings.json");
        assert!(!agent_built_at_launch(&settings).plan_first());
    }

    /// A setting turned off again stays off after a restart.
    #[test]
    fn the_agent_built_at_launch_acts_directly_when_plan_first_was_turned_off() {
        let home = tempdir().unwrap();
        let settings = home.path().join(".edytlab").join("settings.json");
        let before = AppState::new();
        before.install_settings(settings.clone());
        before.save_plan_first(true).expect("saved");
        before.save_plan_first(false).expect("saved");
        drop(before);

        assert!(!agent_built_at_launch(&settings).plan_first());
    }

    /// Where a chat turn finds no agent, it says why. After a refused
    /// startup that is the keychain, and the person is told what undoes it,
    /// in words that still read as "no agent" to the frontend (which offers
    /// Settings for those).
    #[test]
    fn no_agent_after_a_refused_startup_says_the_keychain_refused() {
        let state = AppState::default();
        state.agent_settings.set_keychain_refused(true);
        state
            .agent_settings
            .set_keychain_read_error(Some("locked".into()));

        let text: String = crate::commands::no_agent_error(&state).into();

        assert!(
            text.starts_with("no agent configured")
                && text.contains("refused access")
                && text.contains("locked")
                && text.contains("Settings"),
            "{text}"
        );
    }

    /// Without a refusal it is the error it always was.
    #[test]
    fn no_agent_without_a_refusal_is_the_plain_error() {
        let state = AppState::default();
        state
            .agent_settings
            .set_keychain_read_error(Some("no secret service".into()));
        let text: String = crate::commands::no_agent_error(&state).into();
        assert_eq!(text, "no agent configured; call set_api_key first");
    }

    // ---- One save, one answer (#497, #539, ADR 0005) -------------------

    use ai::keychain::Persistence;

    fn fell_back(reason: &str) -> Persistence {
        Persistence::SessionOnly {
            reason: reason.to_string(),
        }
    }

    /// An in-memory keychain: it answers each save as told, records what was
    /// written, and answers reads from what it was given. `session_only`
    /// names the saves that fall back to the kernel keyring ("api_key",
    /// "active_provider", "base_url", "effort", "model"), `failing` the ones
    /// that fail outright. The reason a fallback gives is `no secret service
    /// for <what>`, so a test can tell whose it got.
    #[derive(Default)]
    pub(crate) struct FakeKeychain {
        session_only: Vec<&'static str>,
        failing: Vec<&'static str>,
        wrote: Mutex<Vec<String>>,
        keys: Mutex<HashMap<String, String>>,
        key_read_failure: Option<&'static str>,
    }

    impl FakeKeychain {
        fn saving_session_only(what: &[&'static str]) -> Self {
            Self {
                session_only: what.to_vec(),
                ..Self::default()
            }
        }

        fn failing(what: &[&'static str]) -> Self {
            Self {
                failing: what.to_vec(),
                ..Self::default()
            }
        }

        fn holding_key(provider: &str, key: &str) -> Self {
            let fake = Self::default();
            fake.keys
                .lock()
                .unwrap()
                .insert(provider.to_string(), key.to_string());
            fake
        }

        fn save(&self, what: &'static str) -> Result<Persistence, keyring::Error> {
            self.wrote.lock().unwrap().push(format!("save {what}"));
            if self.failing.contains(&what) {
                return Err(keyring::Error::PlatformFailure("the store is gone".into()));
            }
            Ok(if self.session_only.contains(&what) {
                fell_back(&format!("no secret service for {what}"))
            } else {
                Persistence::Durable
            })
        }

        fn clear(&self, what: &'static str) -> Result<(), keyring::Error> {
            self.wrote.lock().unwrap().push(format!("clear {what}"));
            Ok(())
        }

        fn wrote(&self) -> Vec<String> {
            self.wrote.lock().unwrap().clone()
        }
    }

    impl Keychain for FakeKeychain {
        fn save_api_key(&self, p: &str, k: &str) -> Result<Persistence, keyring::Error> {
            let at = self.save("api_key")?;
            self.keys.lock().unwrap().insert(p.into(), k.into());
            Ok(at)
        }
        fn delete_api_key(&self, p: &str) -> Result<(), keyring::Error> {
            self.keys.lock().unwrap().remove(p);
            self.clear("api_key")
        }
        fn save_active_provider(&self, _: &str) -> Result<Persistence, keyring::Error> {
            self.save("active_provider")
        }
        fn save_base_url(&self, _: &str, _: &str) -> Result<Persistence, keyring::Error> {
            self.save("base_url")
        }
        fn delete_base_url(&self, _: &str) -> Result<(), keyring::Error> {
            self.clear("base_url")
        }
        fn save_effort(&self, _: &str, _: ai::Effort) -> Result<Persistence, keyring::Error> {
            self.save("effort")
        }
        fn delete_effort(&self, _: &str) -> Result<(), keyring::Error> {
            self.clear("effort")
        }
        fn save_model(&self, _: &str, _: &str) -> Result<Persistence, keyring::Error> {
            self.save("model")
        }
        fn load_api_key(&self, p: &str) -> Option<String> {
            self.keys.lock().unwrap().get(p).cloned()
        }
        fn load_base_url(&self, _: &str) -> Option<String> {
            None
        }
        fn load_effort(&self, _: &str) -> Option<ai::Effort> {
            None
        }
        fn try_load_active_provider(&self) -> Result<Option<String>, KeychainError> {
            Ok(None)
        }
        fn try_load_api_key(&self, p: &str) -> Result<Option<String>, KeychainError> {
            match self.key_read_failure {
                Some(why) => Err(KeychainError::new(why)),
                None => Ok(self.load_api_key(p)),
            }
        }
        fn try_load_model(&self, _: &str) -> Result<Option<String>, KeychainError> {
            Ok(None)
        }
        fn try_load_base_url(&self, _: &str) -> Result<Option<String>, KeychainError> {
            Ok(None)
        }
        fn try_load_effort(&self, _: &str) -> Result<Option<ai::Effort>, KeychainError> {
            Ok(None)
        }
    }

    /// A state whose settings are over `keychain`, with nothing else set up.
    fn state_over(keychain: &Arc<FakeKeychain>) -> AppState {
        AppState {
            agent_settings: AgentSettings::with_keychain(keychain.clone()),
            ..AppState::default()
        }
    }

    fn change(provider: &str) -> SettingsChange {
        SettingsChange {
            provider: provider.to_string(),
            ..SettingsChange::default()
        }
    }

    fn run(state: &AppState, change: SettingsChange) -> CmdResult<KeychainPersistenceDto> {
        tauri::async_runtime::block_on(save(state, change))
    }

    /// What a save answered, reduced to what the frontend reads.
    fn landing(answer: CmdResult<KeychainPersistenceDto>) -> (bool, Option<String>) {
        let dto = answer.expect("the save succeeded");
        (dto.persistent, dto.reason)
    }

    fn durable() -> (bool, Option<String>) {
        (true, None)
    }

    fn session_only(reason: &str) -> (bool, Option<String>) {
        (false, Some(reason.to_string()))
    }

    /// Saves taken together: durable when nothing fell back (and for a
    /// save that wrote nothing), else session-only with the reason of the
    /// first that did.
    #[test]
    fn saves_taken_together_are_durable_unless_one_fell_back() {
        let durable_dto = |saves: Vec<Persistence>| {
            let dto = KeychainPersistenceDto::landed(saves);
            (dto.persistent, dto.reason)
        };
        assert_eq!(durable_dto(vec![]), durable());
        assert_eq!(durable_dto(vec![Persistence::Durable]), durable());
        assert_eq!(
            durable_dto(vec![Persistence::Durable, fell_back("second")]),
            session_only("second")
        );
        assert_eq!(
            durable_dto(vec![fell_back("first"), fell_back("second")]),
            session_only("first"),
            "the first reason, not the last"
        );
    }

    /// What the frontend's `KeychainPersistence` reads, the same shape
    /// `get_keychain_persistence` answers with.
    #[test]
    fn a_landing_serialises_as_persistent_and_reason() {
        assert_eq!(
            serde_json::to_value(KeychainPersistenceDto::landed([fell_back("no bus")])).unwrap(),
            serde_json::json!({ "persistent": false, "reason": "no bus" })
        );
        assert_eq!(
            serde_json::to_value(KeychainPersistenceDto::landed([])).unwrap(),
            serde_json::json!({ "persistent": true, "reason": null })
        );
    }

    /// The bug (#497): a save that fell back to the kernel keyring (a Secret
    /// Service that went away after launch) looked like any other. The key
    /// and the active provider are two writes; either falling back makes the
    /// save session-only, with the key's reason first.
    #[test]
    fn saving_a_key_reports_where_it_landed() {
        let save_key = |keychain: FakeKeychain| {
            let state = state_over(&Arc::new(keychain));
            let answer = run(
                &state,
                SettingsChange {
                    key: Some("sk-test".into()),
                    ..change("openai")
                },
            );
            (landing(answer), state.agent_settings.api_key_snapshot())
        };

        let (answer, cached) = save_key(FakeKeychain::default());
        assert_eq!(answer, durable());
        assert_eq!(cached.as_deref(), Some("sk-test"));

        let (answer, cached) = save_key(FakeKeychain::saving_session_only(&["api_key"]));
        assert_eq!(answer, session_only("no secret service for api_key"));
        assert_eq!(
            cached.as_deref(),
            Some("sk-test"),
            "the key is saved and used all the same"
        );

        let (answer, _) = save_key(FakeKeychain::saving_session_only(&["active_provider"]));
        assert_eq!(
            answer,
            session_only("no secret service for active_provider")
        );

        let (answer, _) = save_key(FakeKeychain::saving_session_only(&[
            "active_provider",
            "api_key",
        ]));
        assert_eq!(
            answer,
            session_only("no secret service for api_key"),
            "the key is written first, so its reason leads"
        );
    }

    /// A save that fails is still an error, with nothing reported.
    #[test]
    fn a_failed_save_is_still_an_error() {
        let keychain = Arc::new(FakeKeychain::failing(&["api_key"]));
        let state = state_over(&keychain);
        let err = run(
            &state,
            SettingsChange {
                key: Some("sk-test".into()),
                ..change("openai")
            },
        )
        .expect_err("the save failed");
        assert!(err.contains("keychain error"), "{err}");
        assert_eq!(keychain.wrote(), ["save api_key"], "and went no further");
        assert_eq!(state.agent_settings.api_key_snapshot(), None);
        assert_eq!(state.agent_settings.rebuilds(), 0, "nothing changed");
    }

    #[test]
    fn saving_a_base_url_reports_where_it_landed() {
        let with_url = |keychain: &Arc<FakeKeychain>, url: &str| {
            run(
                &state_over(keychain),
                SettingsChange {
                    base_url: Some(url.into()),
                    ..change("ollama")
                },
            )
        };

        assert_eq!(
            landing(with_url(
                &Arc::new(FakeKeychain::default()),
                "http://localhost:1234/v1"
            )),
            durable()
        );
        let fallen = Arc::new(FakeKeychain::saving_session_only(&["base_url"]));
        assert_eq!(
            landing(with_url(&fallen, "http://localhost:1234/v1/")),
            session_only("no secret service for base_url")
        );

        // Clearing the override writes nothing, so there is nothing that
        // could be lost: durable, whatever the service is doing.
        assert_eq!(landing(with_url(&fallen, "  ")), durable());
        assert_eq!(
            fallen.wrote(),
            [
                "save base_url",
                "save active_provider",
                "clear base_url",
                "save active_provider"
            ]
        );

        // A URL that is refused is refused before anything is written.
        let untouched = Arc::new(FakeKeychain::saving_session_only(&["base_url"]));
        assert!(with_url(&untouched, "localhost:1234").is_err());
        assert!(with_url(&untouched, "http://").is_err());
        assert!(untouched.wrote().is_empty());
    }

    #[test]
    fn saving_an_effort_reports_where_it_landed() {
        let with_level = |keychain: &Arc<FakeKeychain>, provider: &str, level: &str| {
            run(
                &state_over(keychain),
                SettingsChange {
                    effort: Some(level.into()),
                    ..change(provider)
                },
            )
        };

        assert_eq!(
            landing(with_level(
                &Arc::new(FakeKeychain::default()),
                "anthropic",
                "high"
            )),
            durable()
        );
        let fallen = Arc::new(FakeKeychain::saving_session_only(&["effort"]));
        assert_eq!(
            landing(with_level(&fallen, "anthropic", "xhigh")),
            session_only("no secret service for effort")
        );
        // "Default" clears the slot: nothing written, nothing to report.
        assert_eq!(landing(with_level(&fallen, "anthropic", "")), durable());
        assert_eq!(
            fallen.wrote(),
            [
                "save effort",
                "save active_provider",
                "clear effort",
                "save active_provider"
            ]
        );

        // A level the provider cannot take is refused before any write.
        let untouched = Arc::new(FakeKeychain::saving_session_only(&["effort"]));
        assert!(with_level(&untouched, "openai", "high").is_err());
        assert!(untouched.wrote().is_empty());
    }

    #[test]
    fn switching_provider_reports_where_the_choice_landed() {
        let switch = |keychain: &Arc<FakeKeychain>, provider: &str| {
            run(&state_over(keychain), change(provider))
        };

        assert_eq!(
            landing(switch(&Arc::new(FakeKeychain::default()), "ollama")),
            durable()
        );
        assert_eq!(
            landing(switch(
                &Arc::new(FakeKeychain::saving_session_only(&["active_provider"])),
                "ollama"
            )),
            session_only("no secret service for active_provider")
        );

        let untouched = Arc::new(FakeKeychain::saving_session_only(&["active_provider"]));
        assert!(switch(&untouched, "no-such-provider").is_err());
        assert!(untouched.wrote().is_empty());
    }

    #[test]
    fn choosing_a_model_reports_where_the_choice_landed() {
        let choose = |keychain: FakeKeychain| {
            let state = state_over(&Arc::new(keychain));
            let answer = run(
                &state,
                SettingsChange {
                    model: Some(" claude-opus-5 ".into()),
                    ..change("anthropic")
                },
            );
            (answer, state)
        };

        assert_eq!(landing(choose(FakeKeychain::default()).0), durable());
        let (answer, state) = choose(FakeKeychain::saving_session_only(&["model"]));
        assert_eq!(landing(answer), session_only("no secret service for model"));
        assert_eq!(
            state.agent_settings.model_for("anthropic").as_deref(),
            Some("claude-opus-5"),
            "chosen for this session either way"
        );

        // A failed write keeps its own, older, message.
        let (answer, state) = choose(FakeKeychain::failing(&["model"]));
        let err = answer.expect_err("not saved");
        assert!(
            err.starts_with("model selected for this session, but could not be saved"),
            "{err}"
        );
        assert_eq!(
            state.agent_settings.model_for("anthropic").as_deref(),
            Some("claude-opus-5")
        );
        assert_eq!(
            state.agent_settings.rebuilds(),
            1,
            "the choice that did apply is built once"
        );

        let blank = run(
            &state_over(&Arc::new(FakeKeychain::default())),
            SettingsChange {
                model: Some("  ".into()),
                ..change("anthropic")
            },
        )
        .expect_err("a blank model is refused");
        assert_eq!(blank, "model id must not be empty");
    }

    // ---- The save is one unit of work ----------------------------------

    /// The point of ADR 0005: a Settings Save used to be up to five commands,
    /// each rebuilding the agent. It is one write of everything and one
    /// rebuild, and the answer is one.
    #[test]
    fn one_save_of_every_field_writes_in_order_and_rebuilds_once() {
        let keychain = Arc::new(FakeKeychain::default());
        let state = state_over(&keychain);
        let answer = run(
            &state,
            SettingsChange {
                provider: "anthropic".into(),
                key: Some("sk-test".into()),
                model: Some("claude-opus-5".into()),
                base_url: Some("https://gateway.example/v1/".into()),
                effort: Some("high".into()),
            },
        );
        assert_eq!(landing(answer), durable());
        assert_eq!(
            keychain.wrote(),
            [
                "save api_key",
                "save base_url",
                "save effort",
                "save active_provider",
                "save model"
            ],
            "the key and the address first: the rebuild reads them"
        );
        assert_eq!(state.agent_settings.rebuilds(), 1);
        assert_eq!(state.agent_settings.active_provider_id(), "anthropic");
        assert_eq!(
            state.agent_settings.model_for("anthropic").as_deref(),
            Some("claude-opus-5")
        );
    }

    /// Several fields fall back; the answer is the one session-only note.
    #[test]
    fn one_answer_for_a_save_that_fell_back_in_several_places() {
        let keychain = Arc::new(FakeKeychain::saving_session_only(&["effort", "model"]));
        let answer = run(
            &state_over(&keychain),
            SettingsChange {
                model: Some("claude-opus-5".into()),
                effort: Some("high".into()),
                ..change("anthropic")
            },
        );
        assert_eq!(
            landing(answer),
            session_only("no secret service for effort"),
            "the first reason"
        );
    }

    /// A save that is refused changes nothing, whichever field is wrong.
    #[test]
    fn a_save_with_a_refused_field_writes_nothing_and_builds_nothing() {
        let keychain = Arc::new(FakeKeychain::default());
        let state = state_over(&keychain);
        for (what, bad) in [
            (
                "a blank key",
                SettingsChange {
                    key: Some("  ".into()),
                    ..change("openai")
                },
            ),
            (
                "an effort the API would reject",
                SettingsChange {
                    key: Some("sk-ok".into()),
                    effort: Some("ultra".into()),
                    ..change("anthropic")
                },
            ),
            (
                "an address with no scheme",
                SettingsChange {
                    key: Some("sk-ok".into()),
                    base_url: Some("gateway.example".into()),
                    ..change("openai")
                },
            ),
            (
                "an unknown provider",
                SettingsChange {
                    key: Some("sk-ok".into()),
                    ..change("nonsense")
                },
            ),
        ] {
            assert!(run(&state, bad).is_err(), "{what}");
        }
        assert!(keychain.wrote().is_empty(), "wrote {:?}", keychain.wrote());
        assert_eq!(state.agent_settings.rebuilds(), 0);
        assert_eq!(state.agent_settings.api_key_snapshot(), None);
    }

    /// A keyless provider is chosen with no key at all, and an empty one is
    /// nothing to store (the keychain would refuse an empty secret).
    #[test]
    fn a_keyless_provider_is_selected_with_no_key_written() {
        let keychain = Arc::new(FakeKeychain::default());
        let state = state_over(&keychain);
        run(
            &state,
            SettingsChange {
                key: Some("".into()),
                ..change("ollama")
            },
        )
        .expect("selected");
        assert_eq!(keychain.wrote(), ["save active_provider"]);
        assert_eq!(state.agent_settings.active_provider_id(), "ollama");
    }

    /// Switching to a provider takes that provider's saved key, not the last
    /// one's, and says so when it cannot be read.
    #[test]
    fn switching_replaces_the_cached_key_with_the_new_providers() {
        let keychain = Arc::new(FakeKeychain::holding_key("openrouter", "sk-or-1"));
        let state = state_over(&keychain);
        state
            .agent_settings
            .set_api_key_cache(Some("anthropic-key".into()));
        run(&state, change("openrouter")).expect("switched");
        assert_eq!(
            state.agent_settings.api_key_snapshot().as_deref(),
            Some("sk-or-1")
        );

        let unreadable = Arc::new(FakeKeychain {
            key_read_failure: Some("locked"),
            ..FakeKeychain::default()
        });
        let state = state_over(&unreadable);
        state
            .agent_settings
            .set_api_key_cache(Some("anthropic-key".into()));
        let err = run(&state, change("openrouter")).expect_err("said so");
        assert_eq!(
            err,
            "switched to openrouter, but its saved key could not be read: locked"
        );
        assert_eq!(state.agent_settings.active_provider_id(), "openrouter");
        assert_eq!(
            state.agent_settings.api_key_snapshot(),
            None,
            "the previous provider's key is not sent to this one"
        );
        assert_eq!(
            state.agent_settings.rebuilds(),
            1,
            "and the agent was built"
        );
    }

    /// Saving more for the provider that is already active keeps its key in
    /// hand instead of reading it again.
    #[test]
    fn saving_for_the_active_provider_keeps_its_cached_key() {
        let unreadable = Arc::new(FakeKeychain {
            key_read_failure: Some("must not be read"),
            ..FakeKeychain::default()
        });
        let state = state_over(&unreadable);
        state.agent_settings.set_active_provider("openai".into());
        state
            .agent_settings
            .set_api_key_cache(Some("sk-held".into()));
        run(
            &state,
            SettingsChange {
                model: Some("gpt-5".into()),
                ..change("openai")
            },
        )
        .expect("saved");
        assert_eq!(
            state.agent_settings.api_key_snapshot().as_deref(),
            Some("sk-held")
        );
    }

    /// The writes before one that failed are in force, so the agent is built
    /// from them, once, and the failure is the answer.
    #[test]
    fn a_write_that_fails_part_way_still_rebuilds_once_from_what_went_through() {
        let keychain = Arc::new(FakeKeychain::failing(&["active_provider"]));
        let state = state_over(&keychain);
        let err = run(
            &state,
            SettingsChange {
                key: Some("sk-test".into()),
                effort: Some("high".into()),
                ..change("anthropic")
            },
        )
        .expect_err("the provider could not be saved");
        assert!(err.contains("keychain error"), "{err}");
        assert_eq!(
            keychain.wrote(),
            ["save api_key", "save effort", "save active_provider"]
        );
        assert_eq!(state.agent_settings.rebuilds(), 1);
    }

    /// The agent's turn holds its mutex for as long as it runs. A save made
    /// then is written at once but built after the turn ends, from what is
    /// then stored, and the turn carries on with the agent it started with.
    #[test]
    fn a_save_during_a_turn_is_applied_after_it() {
        let dir = tempdir().unwrap();
        let keychain = Arc::new(FakeKeychain::default());
        let state = AppState {
            agent_settings: AgentSettings::with_keychain(keychain.clone()),
            ..state_with_a_store(dir.path())
        };
        state.agent_settings.set_active_provider("ollama".into());

        tauri::async_runtime::block_on(async {
            let turn = state.agent.lock().await;
            let saving = {
                let state = state.clone();
                tauri::async_runtime::spawn(async move { save(&state, change("ollama")).await })
            };
            for _ in 0..50 {
                if !keychain.wrote().is_empty() {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            assert_eq!(
                keychain.wrote(),
                ["save active_provider"],
                "written while the turn runs"
            );
            assert_eq!(
                state.agent_settings.rebuilds(),
                0,
                "the rebuild waits for the turn"
            );
            assert!(turn.is_none(), "and the turn's agent is untouched");
            drop(turn);

            saving.await.unwrap().expect("saved");
            assert!(
                state.agent.lock().await.is_some(),
                "the agent is built once the turn is over"
            );
        });
        assert_eq!(state.agent_settings.rebuilds(), 1);
    }

    /// After a startup the keychain refused, no agent is built and the
    /// keychain is not asked, until a save that goes through lifts it.
    #[test]
    fn a_refused_startup_builds_no_agent_until_a_save_lifts_it() {
        let dir = tempdir().unwrap();
        let keychain = Arc::new(FakeKeychain::default());
        let state = AppState {
            agent_settings: AgentSettings::with_keychain(keychain.clone()),
            ..state_with_a_store(dir.path())
        };
        state.agent_settings.set_keychain_refused(true);
        state
            .agent_settings
            .set_keychain_read_error(Some("locked".into()));
        reapply_if_missing(&state);
        assert!(
            tauri::async_runtime::block_on(state.agent.lock()).is_none(),
            "refused: nothing built"
        );
        assert!(state.agent_settings.refusal_note().is_some());

        run(&state, change("ollama")).expect("saved");

        assert!(!state.agent_settings.keychain_refused());
        assert_eq!(state.agent_settings.keychain_read_error(), None);
        assert!(
            tauri::async_runtime::block_on(state.agent.lock()).is_some(),
            "the save that went through built the agent"
        );
    }

    /// Settings' reads wait inside the module until startup has settled, and
    /// answer from what it found.
    #[test]
    fn the_reads_wait_for_startup_inside_the_module() {
        let keychain = Arc::new(FakeKeychain::holding_key("groq", "gsk"));
        let state = state_over(&keychain);
        state.agent_settings.begin_startup_reads();
        tauri::async_runtime::block_on(async {
            let reading = {
                let state = state.clone();
                tauri::async_runtime::spawn(async move {
                    (
                        active_provider(&state).await,
                        has_api_key(&state).await,
                        has_api_key_for(&state, "groq").await,
                        chosen_model(&state, "groq").await,
                        base_url(&state, "groq").await,
                        effort(&state, "anthropic").await,
                    )
                })
            };
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            state.agent_settings.set_active_provider("groq".into());
            drop(state.agent_settings.startup_reads_guard());
            let (active, has, has_for, model, url, level) = reading.await.unwrap();
            assert_eq!(active, Ok("groq".to_string()), "read after startup set it");
            assert_eq!(has, Ok(true));
            assert_eq!(has_for, Ok(true));
            assert_eq!(model, Ok(String::new()));
            assert_eq!(url, Ok(None));
            assert_eq!(level, Ok(None));
        });
    }

    /// Clearing the active provider's key tears the agent down; another
    /// provider's key is nothing to the agent.
    #[test]
    fn clearing_the_active_providers_key_rebuilds_and_another_does_not() {
        let keychain = Arc::new(FakeKeychain::holding_key("openai", "sk-1"));
        let state = state_over(&keychain);
        state.agent_settings.set_active_provider("openai".into());
        state.agent_settings.set_api_key_cache(Some("sk-1".into()));

        tauri::async_runtime::block_on(clear_key(&state, "groq")).unwrap();
        assert_eq!(state.agent_settings.rebuilds(), 0);
        assert_eq!(
            state.agent_settings.api_key_snapshot().as_deref(),
            Some("sk-1")
        );

        tauri::async_runtime::block_on(clear_key(&state, "openai")).unwrap();
        assert_eq!(state.agent_settings.rebuilds(), 1);
        assert_eq!(state.agent_settings.api_key_snapshot(), None);
        assert_eq!(keychain.load_api_key("openai"), None);
    }

    /// The keychain this state reads at startup and for the catalogue, and
    /// what an MCP server's `<keychain:slot>` resolves to, all come from
    /// the one seam.
    #[test]
    fn startup_the_catalogue_and_mcp_read_through_the_seam() {
        let keychain = Arc::new(FakeKeychain::holding_key("anthropic", "sk-ant"));
        let state = state_over(&keychain);
        load_at_startup(&state);
        assert_eq!(
            state.agent_settings.api_key_snapshot().as_deref(),
            Some("sk-ant")
        );
        assert!(!state.agent_settings.keychain_refused());
        assert_eq!(
            catalogue_inputs(&state, "anthropic", None),
            Ok((Some("sk-ant".into()), None))
        );
        assert_eq!(
            state.agent_settings.mcp_secret("anthropic").as_deref(),
            Some("sk-ant")
        );
        keychain_readable_again(&state);
        let note = (state.agent_settings.refusal())();
        assert_eq!(note, None);
    }

    /// The ChatGPT sign-in, and its end, go through here too.
    #[test]
    fn the_chatgpt_sign_in_and_its_end_are_rebuilds_of_their_own() {
        let dir = tempdir().unwrap();
        let state = AppState {
            agent_settings: AgentSettings::with_keychain(Arc::new(FakeKeychain::default())),
            ..state_with_a_store(dir.path())
        };
        state.agent_settings.set_active_provider("ollama".into());
        state.agent_settings.set_keychain_refused(true);
        chatgpt_signed_in(&state, true);
        assert!(!state.agent_settings.keychain_refused(), "lifted");
        assert!(tauri::async_runtime::block_on(state.agent.lock()).is_some());
        // No plan agent: nothing to drop.
        chatgpt_session_ended(&state);
        tauri::async_runtime::block_on(build_after_startup(&state)).unwrap();
        tauri::async_runtime::block_on(reapply(&state)).unwrap();
        reapply_soon(&state);
        assert_eq!(state.agent_settings.rebuilds(), 4);
    }

    /// A state nothing reads into (every test, the CLI) never makes a
    /// read wait.
    #[tokio::test]
    async fn a_state_nothing_is_reading_into_is_settled() {
        let state = AppState::new();
        assert!(!state.agent_settings.startup_reads_pending());
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            state.agent_settings.startup_reads_settled(),
        )
        .await
        .expect("settled at once");
    }

    /// A read that panics must not leave `has_api_key` pending forever.
    #[tokio::test]
    async fn a_thread_that_dies_reading_still_ends_the_wait() {
        let state = AppState::new();
        state.agent_settings.begin_startup_reads();
        assert!(state.agent_settings.startup_reads_pending());

        let reader = {
            let state = state.clone();
            std::thread::spawn(move || {
                let _settled = state.agent_settings.startup_reads_guard();
                panic!("the keychain read blew up");
            })
        };
        assert!(reader.join().is_err(), "the thread did panic");

        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            state.agent_settings.startup_reads_settled(),
        )
        .await
        .expect("settled although the reader panicked");
    }

    /// The real seam, in a process that registered no settings folder (no
    /// test in this crate does, and `lib.rs`'s `setup` is the only caller):
    /// every `ai::keychain` call then fails without touching a file or the
    /// OS keychain (ADR 0009), and `OsKeychain` passes each answer on.
    #[test]
    fn the_os_keychain_passes_ai_keychain_through() {
        assert!(ai::keychain::settings_dir().is_none(), "precondition");
        let os = OsKeychain;
        assert!(os.save_api_key("openai", "k").is_err());
        assert!(os.delete_api_key("openai").is_err());
        assert!(os.save_active_provider("openai").is_err());
        assert!(os.save_base_url("openai", "http://x").is_err());
        assert!(os.delete_base_url("openai").is_err());
        assert!(os.save_effort("anthropic", ai::Effort::High).is_err());
        assert!(os.delete_effort("anthropic").is_err());
        assert!(os.save_model("openai", "m").is_err());
        assert_eq!(os.load_api_key("openai"), None);
        assert_eq!(os.load_base_url("openai"), None);
        assert_eq!(os.load_effort("anthropic"), None);
        assert!(os.try_load_active_provider().is_err());
        assert!(os.try_load_api_key("openai").is_err());
        assert!(os.try_load_model("openai").is_err());
        assert!(os.try_load_base_url("openai").is_err());
        assert!(os.try_load_effort("anthropic").is_err());
    }
}
