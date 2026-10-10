//! OS keychain wrapper for LLM provider API keys.
//!
//! On macOS this resolves to the user's login keychain via
//! Security.framework; on Windows it lands in the Credential Manager.
//! Both are unchanged.
//!
//! On Linux, values live in the Secret Service (GNOME Keyring, KWallet
//! from Plasma 5.97 / 6, or KeePassXC), reached over D-Bus with an
//! encrypted session (the `keyring` crate's `sync-secret-service` and
//! `crypto-rust` features). So they survive a reboot (#394).
//!
//! When no Secret Service answers (no session bus, or nothing owns
//! `org.freedesktop.secrets`), values fall back to the kernel keyring
//! (`keyutils`) and last only until the next reboot. [`persistence`]
//! reports which of the two applies, and the app shows it.
//!
//! Values that earlier builds left in the kernel keyring are moved into
//! the Secret Service on first read, if they are still there (same
//! boot, and within the three-day expiry of the persistent keyring).
//!
//! A locked or refused store is an error, never a silent "nothing
//! stored": the `try_` readers return it, and the `Option` readers log
//! it. All I/O is serialized behind one lock, because the Secret Service
//! mishandles concurrent D-Bus access.
//!
//! Keys are never written to disk by edytlab itself. Never log a value,
//! and never `{:?}` a [`keyring::Error`]: its `Debug` output can carry
//! the secret bytes (`BadEncoding`), where its `Display` does not.
//!
//! # Per-provider slots
//!
//! Each provider stores its key under a separate keychain account so a
//! user can have credentials for several providers configured at once
//! and switch between them without re-typing. The account name is
//! `"<provider_id>_api_key"` (e.g. `"anthropic_api_key"`,
//! `"openrouter_api_key"`); the chosen model and an overridden base URL
//! sit beside it as `"<provider_id>_model"` and `"<provider_id>_base_url"`,
//! and the reasoning effort as `"<provider_id>_effort"`.
//!
//! # Active provider
//!
//! Which provider is currently selected is stored as a tiny string in
//! its own slot: account `"active_provider"`. `load_active_provider`
//! returns its id (`Some("anthropic")`, `Some("ollama")`, …); `None`
//! means "no preference, default to Anthropic" (the historical
//! behaviour).
//!
//! # Back-compat
//!
//! Earlier builds stored the Anthropic key under the bare account
//! `"anthropic_api_key"`, which still matches the new naming scheme — no
//! migration is required. [`load_api_key("anthropic")`] continues to
//! return the legacy entry on first launch.

use std::sync::{Mutex, MutexGuard};

use keyring::Entry;

use crate::anthropic::Effort;

/// Service name used in the OS keychain. Must match the bundle id
/// declared in `tauri.conf.json` so the macOS keychain prompt names the
/// app correctly.
const SERVICE: &str = "app.edytlab.desktop";

/// Account slot for the "active provider" preference.
const ACTIVE_PROVIDER_ACCOUNT: &str = "active_provider";

/// Where the values written now will end up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Persistence {
    /// The platform's own store: the login keychain, Credential Manager,
    /// or a Secret Service on Linux. Survives a reboot.
    Durable,
    /// Linux with no Secret Service to talk to, so values sit in the
    /// kernel keyring. They last until the machine restarts.
    SessionOnly {
        /// Why the Secret Service could not be used, as `keyring`
        /// words it. Never contains a stored value.
        reason: String,
    },
}

/// The keychain could not be read, which is not the same as "nothing is
/// stored": a locked store, a refused prompt or a broken service all
/// land here.
///
/// Holds only the text of the underlying error, never the error itself:
/// `keyring::Error`'s `Debug` output can carry the secret bytes
/// (`BadEncoding`), and this type is `Debug` and ends up in logs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeychainError {
    reason: String,
}

impl KeychainError {
    /// An error with the given human-readable reason.
    pub fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
        }
    }

    /// The reason alone, without the "could not be read" prefix.
    pub fn reason(&self) -> &str {
        &self.reason
    }
}

impl std::fmt::Display for KeychainError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "the system keychain could not be read: {}", self.reason)
    }
}

impl std::error::Error for KeychainError {}

impl From<keyring::Error> for KeychainError {
    /// Built from `Display` only. `Debug` would include the bytes of an
    /// undecodable value.
    fn from(e: keyring::Error) -> Self {
        Self::new(e.to_string())
    }
}

// ---------------------------------------------------------------------------
// The store logic, kept apart from the OS so it can be tested without one.
//
// Everything in this section compiles on every platform. On macOS and
// Windows there is no second store, `legacy` is `None` throughout, and
// the code reduces to "ask the one store".
// ---------------------------------------------------------------------------

/// One keychain entry, as the logic below needs to see it.
trait Slot {
    fn get(&self) -> keyring::Result<String>;
    fn set(&self, value: &str) -> keyring::Result<()>;
    fn delete(&self) -> keyring::Result<()>;
}

impl Slot for Entry {
    fn get(&self) -> keyring::Result<String> {
        self.get_password()
    }
    fn set(&self, value: &str) -> keyring::Result<()> {
        self.set_password(value)
    }
    fn delete(&self) -> keyring::Result<()> {
        self.delete_credential()
    }
}

/// Whether the store itself could not be reached, as opposed to
/// answering "no" (not found, locked, refused).
///
/// `keyring` reports a Secret Service that is not there (no session bus,
/// nothing owning `org.freedesktop.secrets`) as `PlatformFailure`, and a
/// locked or dismissed one as `NoStorageAccess`.
fn unreachable(e: &keyring::Error) -> bool {
    matches!(e, keyring::Error::PlatformFailure(_))
}

/// Read a value from `primary`, falling back to `legacy`.
///
/// - `primary` has it: that is the answer.
/// - `primary` answers "not found" and `legacy` has it: a value an
///   earlier build left behind. Move it into `primary`, and drop the
///   `legacy` copy only once `primary` took it.
/// - `primary` cannot be reached: the answer is whatever `legacy` holds,
///   which is where [`write_to`] put it.
/// - `primary` fails any other way (locked, refused, ambiguous,
///   undecodable): that is an error. It is never read as "not found",
///   or the app would show a first-run prompt over a key that is there.
fn read_from(primary: &dyn Slot, legacy: Option<&dyn Slot>) -> keyring::Result<Option<String>> {
    match primary.get() {
        Ok(v) => Ok(Some(v)),
        Err(keyring::Error::NoEntry) => {
            let Some(legacy) = legacy else {
                return Ok(None);
            };
            let Ok(v) = legacy.get() else {
                return Ok(None);
            };
            if primary.set(&v).is_ok() {
                let _ = legacy.delete();
            }
            Ok(Some(v))
        }
        Err(e) if unreachable(&e) => match legacy {
            Some(legacy) => match legacy.get() {
                Ok(v) => Ok(Some(v)),
                Err(keyring::Error::NoEntry) => Ok(None),
                Err(_) => Err(e),
            },
            None => Err(e),
        },
        Err(e) => Err(e),
    }
}

/// Write a value to `primary`.
///
/// Only a `primary` that cannot be reached at all degrades to `legacy`,
/// and then only after a second look confirms it: `keyring` reports a
/// failed `create_item` as `PlatformFailure` even when the service is
/// up, and that must fail the save rather than park the key in a store
/// that disappears at reboot. A locked store fails the save too.
fn write_to(
    primary: &dyn Slot,
    legacy: Option<&dyn Slot>,
    value: &str,
) -> keyring::Result<Persistence> {
    match primary.set(value) {
        Ok(()) => {
            if let Some(legacy) = legacy {
                // The durable copy is the only one now. A stale kernel
                // copy would be read back if the service went away.
                let _ = legacy.delete();
            }
            Ok(Persistence::Durable)
        }
        Err(e) if unreachable(&e) => {
            if let Some(legacy) = legacy {
                if matches!(primary.get(), Err(ref again) if unreachable(again)) {
                    return match legacy.set(value) {
                        Ok(()) => Ok(Persistence::SessionOnly {
                            reason: e.to_string(),
                        }),
                        Err(_) => Err(e),
                    };
                }
            }
            Err(e)
        }
        Err(e) => Err(e),
    }
}

/// Remove a value from both stores. Missing is success: the caller
/// asked for "no value", and that is the state either way.
///
/// The `legacy` copy goes first and a failure to remove it fails the
/// clear, even when `primary` was cleared: [`read_from`] would move a
/// surviving kernel copy straight back and resurrect the value.
fn delete_from(primary: &dyn Slot, legacy: Option<&dyn Slot>) -> keyring::Result<()> {
    let mut legacy_error = None;
    if let Some(legacy) = legacy {
        match legacy.delete() {
            Ok(()) | Err(keyring::Error::NoEntry) => {}
            Err(e) => legacy_error = Some(e),
        }
    }
    match primary.delete() {
        Ok(()) | Err(keyring::Error::NoEntry) => {}
        // Session-only mode: nothing was ever stored in `primary`. (A
        // Secret Service that is only briefly down keeps its copy; the
        // next save or clear while it is up replaces or removes it.)
        Err(e) if unreachable(&e) && legacy.is_some() => {}
        Err(e) => return Err(e),
    }
    legacy_error.map_or(Ok(()), Err)
}

/// Where a write would land right now.
///
/// With no second store (`fallback_available` false: macOS, Windows)
/// the answer is always durable and `primary` is not touched, so those
/// platforms never see an extra keychain prompt. Otherwise the answer
/// is session-only exactly when `primary` cannot be reached. A locked
/// store is still the durable one.
fn probe(primary: &dyn Slot, fallback_available: bool) -> Persistence {
    if !fallback_available {
        return Persistence::Durable;
    }
    match primary.get() {
        Err(e) if unreachable(&e) => Persistence::SessionOnly {
            reason: e.to_string(),
        },
        _ => Persistence::Durable,
    }
}

// ---------------------------------------------------------------------------
// The real stores.
// ---------------------------------------------------------------------------

/// The Secret Service mishandles concurrent D-Bus access from several
/// threads (the `keyring` docs say so), and startup, the agent rebuilds
/// and the Settings commands all read from different threads. So every
/// keychain operation takes this lock. It is not reentrant: only the
/// four `*_account` helpers below take it, and none calls another.
static KEYCHAIN_LOCK: Mutex<()> = Mutex::new(());

fn lock() -> MutexGuard<'static, ()> {
    // The guarded data is `()`; a panic elsewhere cannot have left it
    // in a bad state.
    KEYCHAIN_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// The platform's own store: login keychain, Credential Manager, or on
/// Linux the Secret Service.
fn open_primary(account: &str) -> keyring::Result<Entry> {
    Entry::new(SERVICE, account)
}

/// The kernel keyring on Linux: where earlier builds kept everything,
/// and where values go while no Secret Service answers. The description
/// is `keyring-rs:{account}@app.edytlab.desktop`, which is exactly what
/// the previous default store used, so old entries are found as they
/// were. `None` where there is no such store, or no session keyring.
#[cfg(target_os = "linux")]
fn open_legacy(account: &str) -> Option<Entry> {
    keyring::keyutils::KeyutilsCredential::new_with_target(None, SERVICE, account)
        .ok()
        .map(|c| Entry::new_with_credential(Box::new(c)))
}

#[cfg(not(target_os = "linux"))]
fn open_legacy(_account: &str) -> Option<Entry> {
    None
}

fn read_account(account: &str) -> Result<Option<String>, KeychainError> {
    let _guard = lock();
    let primary = open_primary(account)?;
    let legacy = open_legacy(account);
    read_from(&primary, legacy.as_ref().map(|e| e as &dyn Slot)).map_err(KeychainError::from)
}

fn write_account(account: &str, value: &str) -> keyring::Result<Persistence> {
    let _guard = lock();
    let primary = open_primary(account)?;
    let legacy = open_legacy(account);
    let persistence = write_to(&primary, legacy.as_ref().map(|e| e as &dyn Slot), value)?;
    if let Persistence::SessionOnly { reason } = &persistence {
        tracing::warn!(
            account = %account,
            reason = %reason,
            "no Secret Service; stored in the kernel keyring until reboot"
        );
    }
    Ok(persistence)
}

fn delete_account(account: &str) -> keyring::Result<()> {
    let _guard = lock();
    let primary = open_primary(account)?;
    let legacy = open_legacy(account);
    delete_from(&primary, legacy.as_ref().map(|e| e as &dyn Slot))
}

/// Read for the loaders that answer with an `Option`: an unreadable
/// keychain is logged and reads as nothing stored. Callers that must
/// tell the two apart use the `try_` variants.
fn read_or_warn(account: &str) -> Option<String> {
    match read_account(account) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(account = %account, error = %e, "keychain read failed");
            None
        }
    }
}

/// Whether values saved now survive a reboot.
///
/// Always [`Persistence::Durable`] on macOS and Windows. On Linux it is
/// [`Persistence::SessionOnly`] while no Secret Service answers, which
/// the app surfaces so the user is not surprised by a Welcome screen
/// after the next restart (#394).
pub fn persistence() -> Persistence {
    let _guard = lock();
    let Ok(primary) = open_primary(ACTIVE_PROVIDER_ACCOUNT) else {
        tracing::warn!("could not build the keychain entry to probe persistence");
        return Persistence::Durable;
    };
    probe(&primary, open_legacy(ACTIVE_PROVIDER_ACCOUNT).is_some())
}

/// Build the per-provider keychain account name.
///
/// Anthropic intentionally lands on `"anthropic_api_key"` — the same
/// name the pre-multi-provider build used — so existing keychain
/// entries are still found on first launch after upgrade.
fn account_for(provider_id: &str) -> String {
    format!("{provider_id}_api_key")
}

/// Read an API key from the OS keychain for `provider_id`.
///
/// Returns `Some(key)` when an entry exists and `None` when there is no
/// stored key. A keychain that cannot be read (locked, access denied,
/// no service) also reads as `None`, but is logged with the reason and
/// never the value. Use [`try_load_api_key`] where the difference
/// matters, which is anywhere that would otherwise tell the user to
/// enter a key that is already stored.
///
/// We deliberately do not log the key, the entry contents, or a
/// `{:?}` of the backend's error, because `Debug` can include the
/// secret.
pub fn load_api_key(provider_id: &str) -> Option<String> {
    read_or_warn(&account_for(provider_id))
}

/// Like [`load_api_key`], but a keychain that cannot be read is an
/// error rather than `None`.
pub fn try_load_api_key(provider_id: &str) -> Result<Option<String>, KeychainError> {
    read_account(&account_for(provider_id))
}

/// Persist an API key to the OS keychain for `provider_id`.
///
/// Errors propagate via [`keyring::Error`]; callers (the settings UI)
/// are expected to surface a generic "could not save key" message
/// rather than the platform-specific reason, again to avoid leaking
/// the key in logs.
pub fn save_api_key(provider_id: &str, key: &str) -> Result<(), keyring::Error> {
    write_account(&account_for(provider_id), key).map(|_| ())
}

/// Remove the stored API key for `provider_id`, if any. Used by the
/// settings UI's "sign out" affordance. A missing entry is treated as
/// success — the desired post-state is "no key".
pub fn delete_api_key(provider_id: &str) -> Result<(), keyring::Error> {
    delete_account(&account_for(provider_id))
}

/// Read the user's active-provider preference. `None` means no
/// preference is recorded, in which case callers should default to
/// Anthropic. An unreadable keychain is logged and reads as `None`;
/// see [`try_load_active_provider`].
pub fn load_active_provider() -> Option<String> {
    active_provider_from(read_or_warn(ACTIVE_PROVIDER_ACCOUNT))
}

/// Like [`load_active_provider`], but a keychain that cannot be read is
/// an error rather than `None`.
pub fn try_load_active_provider() -> Result<Option<String>, KeychainError> {
    read_account(ACTIVE_PROVIDER_ACCOUNT).map(active_provider_from)
}

/// The stored id, trimmed, with a blank one counting as no preference.
fn active_provider_from(raw: Option<String>) -> Option<String> {
    let trimmed = raw?.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

/// Persist the active-provider preference. The provider id is stored
/// alongside API keys so it survives app restarts without leaking into
/// any plaintext settings file.
pub fn save_active_provider(provider_id: &str) -> Result<(), keyring::Error> {
    write_account(ACTIVE_PROVIDER_ACCOUNT, provider_id).map(|_| ())
}

/// Account slot for a provider's base-URL override.
fn base_url_account_for(provider_id: &str) -> String {
    format!("{provider_id}_base_url")
}

/// Read a provider's base-URL override.
///
/// `None` means "use the provider's built-in URL", which is the case for
/// everyone who has never opened the field.
///
/// Stored per provider rather than globally on purpose: a URL that makes
/// sense for a local server is nonsense for Anthropic, and a single slot
/// would follow you across a provider switch and fail confusingly.
///
/// This is configuration rather than a secret, and it shares the
/// keychain with the API keys because `active_provider` already
/// established that this module is where per-provider settings live.
/// Standing up a second store for one string would be worse.
pub fn load_base_url(provider_id: &str) -> Option<String> {
    read_or_warn(&base_url_account_for(provider_id)).filter(|url| !url.trim().is_empty())
}

/// Store a provider's base-URL override.
pub fn save_base_url(provider_id: &str, base_url: &str) -> Result<(), keyring::Error> {
    write_account(&base_url_account_for(provider_id), base_url).map(|_| ())
}

/// Forget a provider's override so the built-in URL applies again.
///
/// A missing entry is success: the caller asked for "no override", and
/// that is the state either way.
pub fn delete_base_url(provider_id: &str) -> Result<(), keyring::Error> {
    delete_account(&base_url_account_for(provider_id))
}

/// Account slot for a provider's chosen model.
fn model_account_for(provider_id: &str) -> String {
    format!("{provider_id}_model")
}

/// Read a provider's chosen model.
///
/// `None` means "use the provider's default", which is what every user
/// gets until they pick something in Settings.
///
/// This existed only in memory before (#249): `active_model_by_provider`
/// was written by Settings and nothing persisted it, so after a restart
/// the agent built its config with no `with_model` call and silently ran
/// `provider.default_model()` — while Settings went on displaying the
/// user's pick, which it read from localStorage rather than from the
/// backend. Different capabilities and a different price per token, with
/// nothing on screen to say so.
///
/// Per provider for the same reason as the base URL: a model name that
/// means something to Anthropic is meaningless to Ollama, and a single
/// slot would follow you across a provider switch and fail confusingly.
pub fn load_model(provider_id: &str) -> Option<String> {
    read_or_warn(&model_account_for(provider_id)).filter(|m| !m.trim().is_empty())
}

/// Store a provider's chosen model.
pub fn save_model(provider_id: &str, model: &str) -> Result<(), keyring::Error> {
    write_account(&model_account_for(provider_id), model).map(|_| ())
}

/// Forget a provider's choice so its default applies again.
///
/// A missing entry is success: the caller asked for "no override", and
/// that is the state either way.
pub fn delete_model(provider_id: &str) -> Result<(), keyring::Error> {
    delete_account(&model_account_for(provider_id))
}

/// Account slot for a provider's reasoning effort.
fn effort_account_for(provider_id: &str) -> String {
    format!("{provider_id}_effort")
}

/// Read a provider's reasoning effort.
///
/// `None` means "the model's own default", which is what everyone gets
/// until they pick a level in Settings. So does a stored string that is
/// not one of the five levels (a hand edit, a build from the future):
/// reading that as a level would put a value on the wire that the API
/// answers with a 400 on every request.
///
/// Per provider like the model and the base URL. Only Anthropic's slot
/// is ever used today, but a level means nothing to a server that has
/// no such setting, and a shared slot would follow you across a switch.
pub fn load_effort(provider_id: &str) -> Option<Effort> {
    read_or_warn(&effort_account_for(provider_id)).and_then(|s| Effort::parse(&s))
}

/// Store a provider's reasoning effort.
pub fn save_effort(provider_id: &str, effort: Effort) -> Result<(), keyring::Error> {
    write_account(&effort_account_for(provider_id), effort.as_str()).map(|_| ())
}

/// Forget a provider's choice so the model's default applies again.
///
/// A missing entry is success: the caller asked for "no effort", and
/// that is the state either way.
pub fn delete_effort(provider_id: &str) -> Result<(), keyring::Error> {
    delete_account(&effort_account_for(provider_id))
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};

    use super::*;

    #[test]
    fn account_uses_provider_id_suffix() {
        assert_eq!(account_for("anthropic"), "anthropic_api_key");
        assert_eq!(account_for("openrouter"), "openrouter_api_key");
    }

    /// The two slots must not collide, or saving a base URL would
    /// overwrite the API key for the same provider.
    #[test]
    fn base_url_and_key_use_separate_slots() {
        assert_eq!(base_url_account_for("ollama"), "ollama_base_url");
        assert_ne!(base_url_account_for("ollama"), account_for("ollama"));
        // The model slot joins them and must collide with neither, or
        // saving a model would overwrite a key or a base URL.
        assert_eq!(model_account_for("ollama"), "ollama_model");
        assert_ne!(model_account_for("ollama"), account_for("ollama"));
        assert_ne!(model_account_for("ollama"), base_url_account_for("ollama"));
        // And the effort slot joins all three, for the same reason.
        assert_eq!(effort_account_for("anthropic"), "anthropic_effort");
        for other in [
            account_for("anthropic"),
            base_url_account_for("anthropic"),
            model_account_for("anthropic"),
        ] {
            assert_ne!(effort_account_for("anthropic"), other);
        }
        for p in [
            "anthropic",
            "openai",
            "openrouter",
            "groq",
            "gemini",
            "ollama",
        ] {
            assert_ne!(base_url_account_for(p), account_for(p), "collision for {p}");
        }
    }

    /// What goes into the slot and what comes out of it are the same
    /// five strings the API takes, and nothing else is a level.
    #[test]
    fn a_stored_effort_reads_back_as_the_same_level_and_junk_as_none() {
        for e in Effort::ALL {
            assert_eq!(Effort::parse(e.as_str()), Some(e));
        }
        for junk in ["", " ", "HIGH", "extra high", "ultra", "xhigh!", "0"] {
            assert_eq!(Effort::parse(junk), None, "{junk:?} must read as unset");
        }
        // Whitespace around a real level is tolerated: it is the level.
        assert_eq!(Effort::parse(" xhigh\n"), Some(Effort::XHigh));
    }

    /// The slot itself, through the OS keychain: save, read, overwrite,
    /// delete, and deleting again.
    ///
    /// Uses a provider id no real provider has, so it cannot touch a
    /// user's own setting. A machine with no usable keychain (a CI
    /// container with no session keyring) cannot run this; it says so
    /// and returns rather than failing a build for something the code
    /// under test does not control.
    #[test]
    fn the_effort_slot_round_trips_through_the_keychain() {
        let id = "effort-slot-roundtrip-test";
        if save_effort(id, Effort::High).is_err() || load_effort(id).is_none() {
            eprintln!("skipping: no usable OS keychain on this machine");
            let _ = delete_effort(id);
            return;
        }
        assert_eq!(load_effort(id), Some(Effort::High));
        save_effort(id, Effort::XHigh).unwrap();
        assert_eq!(load_effort(id), Some(Effort::XHigh), "overwrites");
        delete_effort(id).unwrap();
        assert_eq!(load_effort(id), None, "deleted slot reads as default");
        delete_effort(id).expect("deleting a missing slot is success");
    }

    // -----------------------------------------------------------------
    // The store logic, against fake slots. No OS keychain involved.
    // -----------------------------------------------------------------

    /// How a fake slot behaves. `keyring::Error` is not `Clone`, so the
    /// errors are built on demand from this.
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Mode {
        /// A store that works.
        Works,
        /// No Secret Service on the bus: every operation is a
        /// `PlatformFailure`.
        Unreachable,
        /// A locked or dismissed store: every operation is
        /// `NoStorageAccess`.
        Locked,
        /// Reads and deletes work, but creating an item fails with a
        /// `PlatformFailure`, which is what `keyring` reports for a
        /// failed `create_item` on a service that is up.
        WriteFails,
        /// Holds bytes that are not UTF-8.
        Garbled,
    }

    struct Fake {
        mode: Mode,
        value: RefCell<Option<String>>,
        gets: Cell<u32>,
    }

    impl Fake {
        fn empty(mode: Mode) -> Self {
            Self {
                mode,
                value: RefCell::new(None),
                gets: Cell::new(0),
            }
        }
        fn holding(mode: Mode, v: &str) -> Self {
            let f = Self::empty(mode);
            *f.value.borrow_mut() = Some(v.to_string());
            f
        }
        fn value(&self) -> Option<String> {
            self.value.borrow().clone()
        }
    }

    fn no_service() -> keyring::Error {
        keyring::Error::PlatformFailure("no secret service".into())
    }
    fn locked() -> keyring::Error {
        keyring::Error::NoStorageAccess("locked".into())
    }

    impl Slot for Fake {
        fn get(&self) -> keyring::Result<String> {
            self.gets.set(self.gets.get() + 1);
            match self.mode {
                Mode::Unreachable => Err(no_service()),
                Mode::Locked => Err(locked()),
                Mode::Garbled => Err(keyring::Error::BadEncoding(b"sk-secret".to_vec())),
                Mode::Works | Mode::WriteFails => {
                    self.value.borrow().clone().ok_or(keyring::Error::NoEntry)
                }
            }
        }
        fn set(&self, v: &str) -> keyring::Result<()> {
            match self.mode {
                Mode::Unreachable => Err(no_service()),
                Mode::Locked => Err(locked()),
                Mode::WriteFails => {
                    Err(keyring::Error::PlatformFailure("create_item failed".into()))
                }
                Mode::Garbled => Err(keyring::Error::BadEncoding(b"sk-secret".to_vec())),
                Mode::Works => {
                    *self.value.borrow_mut() = Some(v.to_string());
                    Ok(())
                }
            }
        }
        fn delete(&self) -> keyring::Result<()> {
            match self.mode {
                Mode::Unreachable => Err(no_service()),
                Mode::Locked => Err(locked()),
                Mode::Garbled => Err(keyring::Error::BadEncoding(b"sk-secret".to_vec())),
                Mode::Works | Mode::WriteFails => self
                    .value
                    .borrow_mut()
                    .take()
                    .map(|_| ())
                    .ok_or(keyring::Error::NoEntry),
            }
        }
    }

    fn some(f: &Fake) -> Option<&dyn Slot> {
        Some(f)
    }

    // --- read ---

    #[test]
    fn a_missing_value_is_none_not_an_error() {
        let primary = Fake::empty(Mode::Works);
        let legacy = Fake::empty(Mode::Works);
        assert_eq!(read_from(&primary, some(&legacy)).unwrap(), None);
        assert_eq!(read_from(&primary, None).unwrap(), None);
    }

    #[test]
    fn a_stored_value_is_read_back() {
        let primary = Fake::holding(Mode::Works, "sk-a");
        assert_eq!(read_from(&primary, None).unwrap().as_deref(), Some("sk-a"));
    }

    /// The pin for the bug in #394's second half: a store that is there
    /// but will not open must not read as "no key", or the app shows
    /// the first-run Welcome over a key that is stored.
    #[test]
    fn a_locked_store_is_an_error_not_a_missing_value() {
        let primary = Fake::empty(Mode::Locked);
        let legacy = Fake::holding(Mode::Works, "sk-old");
        let err = read_from(&primary, some(&legacy)).expect_err("locked must not read as None");
        assert!(matches!(err, keyring::Error::NoStorageAccess(_)));
        // And it did not wander into the other store to find an answer.
        assert_eq!(legacy.value().as_deref(), Some("sk-old"));
        let err = read_from(&primary, None).expect_err("with no fallback store either");
        assert!(matches!(err, keyring::Error::NoStorageAccess(_)));
    }

    #[test]
    fn an_ambiguous_or_undecodable_value_is_an_error() {
        let primary = Fake::empty(Mode::Garbled);
        let legacy = Fake::holding(Mode::Works, "sk-old");
        let err = read_from(&primary, some(&legacy)).expect_err("garbled must not read as None");
        assert!(matches!(err, keyring::Error::BadEncoding(_)));
        assert_eq!(
            legacy.gets.get(),
            0,
            "no fallback for a store that answered"
        );
    }

    #[test]
    fn an_unreachable_secret_service_reads_the_session_copy() {
        let primary = Fake::empty(Mode::Unreachable);
        let legacy = Fake::holding(Mode::Works, "sk-session");
        assert_eq!(
            read_from(&primary, some(&legacy)).unwrap().as_deref(),
            Some("sk-session")
        );
        assert_eq!(legacy.value().as_deref(), Some("sk-session"), "not moved");
    }

    #[test]
    fn an_unreachable_secret_service_with_no_session_copy_reads_as_missing() {
        let primary = Fake::empty(Mode::Unreachable);
        let legacy = Fake::empty(Mode::Works);
        assert_eq!(read_from(&primary, some(&legacy)).unwrap(), None);
    }

    #[test]
    fn neither_store_answering_is_an_error() {
        let primary = Fake::empty(Mode::Unreachable);
        let legacy = Fake::empty(Mode::Unreachable);
        let err = read_from(&primary, some(&legacy)).expect_err("nothing could be asked");
        assert!(matches!(err, keyring::Error::PlatformFailure(_)));
        // With no second store at all (macOS, Windows) the same.
        let err = read_from(&primary, None).expect_err("nothing could be asked");
        assert!(matches!(err, keyring::Error::PlatformFailure(_)));
    }

    #[test]
    fn a_kernel_keyring_copy_is_moved_into_the_secret_service() {
        let primary = Fake::empty(Mode::Works);
        let legacy = Fake::holding(Mode::Works, "sk-from-last-version");
        assert_eq!(
            read_from(&primary, some(&legacy)).unwrap().as_deref(),
            Some("sk-from-last-version")
        );
        assert_eq!(primary.value().as_deref(), Some("sk-from-last-version"));
        assert_eq!(legacy.value(), None, "the old copy is gone once moved");
        // And the next read is just a read.
        assert_eq!(
            read_from(&primary, some(&legacy)).unwrap().as_deref(),
            Some("sk-from-last-version")
        );
    }

    #[test]
    fn a_failed_move_keeps_the_kernel_keyring_copy() {
        let primary = Fake::empty(Mode::WriteFails);
        let legacy = Fake::holding(Mode::Works, "sk-keep-me");
        assert_eq!(
            read_from(&primary, some(&legacy)).unwrap().as_deref(),
            Some("sk-keep-me"),
            "the value is still returned"
        );
        assert_eq!(legacy.value().as_deref(), Some("sk-keep-me"), "and kept");
        assert_eq!(primary.value(), None);
    }

    // --- write ---

    #[test]
    fn saving_to_a_reachable_store_is_durable_and_drops_the_old_copy() {
        let primary = Fake::empty(Mode::Works);
        let legacy = Fake::holding(Mode::Works, "sk-stale");
        assert_eq!(
            write_to(&primary, some(&legacy), "sk-new").unwrap(),
            Persistence::Durable
        );
        assert_eq!(primary.value().as_deref(), Some("sk-new"));
        assert_eq!(legacy.value(), None, "a stale copy would outlive a clear");
    }

    #[test]
    fn saving_without_a_secret_service_is_session_only_and_says_why() {
        let primary = Fake::empty(Mode::Unreachable);
        let legacy = Fake::empty(Mode::Works);
        match write_to(&primary, some(&legacy), "sk-session").unwrap() {
            Persistence::SessionOnly { reason } => {
                assert!(reason.contains("no secret service"), "{reason}");
            }
            other => panic!("expected session-only, got {other:?}"),
        }
        assert_eq!(legacy.value().as_deref(), Some("sk-session"));
    }

    /// `keyring` words a failed `create_item` on a live service the same
    /// way as a missing service. Telling them apart takes a second look,
    /// and getting it wrong parks the key where a reboot erases it.
    #[test]
    fn a_write_failure_from_a_reachable_secret_service_is_an_error_not_a_fallback() {
        let primary = Fake::empty(Mode::WriteFails);
        let legacy = Fake::empty(Mode::Works);
        let err = write_to(&primary, some(&legacy), "sk-x").expect_err("must fail the save");
        assert!(matches!(err, keyring::Error::PlatformFailure(_)));
        assert_eq!(legacy.value(), None, "nothing was parked in the kernel");
    }

    #[test]
    fn saving_to_a_locked_store_fails_instead_of_degrading() {
        let primary = Fake::empty(Mode::Locked);
        let legacy = Fake::empty(Mode::Works);
        let err = write_to(&primary, some(&legacy), "sk-x").expect_err("must fail the save");
        assert!(matches!(err, keyring::Error::NoStorageAccess(_)));
        assert_eq!(legacy.value(), None);
    }

    #[test]
    fn saving_without_a_fallback_store_propagates_the_error() {
        // The macOS and Windows shape: one store, no second.
        let primary = Fake::empty(Mode::Unreachable);
        let err = write_to(&primary, None, "sk-x").expect_err("must fail the save");
        assert!(matches!(err, keyring::Error::PlatformFailure(_)));
        let primary = Fake::empty(Mode::Works);
        assert_eq!(
            write_to(&primary, None, "sk-x").unwrap(),
            Persistence::Durable
        );
        assert_eq!(primary.value().as_deref(), Some("sk-x"));
    }

    #[test]
    fn a_session_only_save_fails_if_the_kernel_keyring_refuses_it_too() {
        let primary = Fake::empty(Mode::Unreachable);
        let legacy = Fake::empty(Mode::Locked);
        let err = write_to(&primary, some(&legacy), "sk-x").expect_err("nowhere to put it");
        // The error is the Secret Service's: that is the store the user
        // is meant to fix.
        assert!(matches!(err, keyring::Error::PlatformFailure(_)));
    }

    // --- delete ---

    #[test]
    fn clearing_removes_both_copies_so_the_key_does_not_come_back() {
        let primary = Fake::holding(Mode::Works, "sk-a");
        let legacy = Fake::holding(Mode::Works, "sk-b");
        delete_from(&primary, some(&legacy)).unwrap();
        assert_eq!(primary.value(), None);
        assert_eq!(legacy.value(), None);
        assert_eq!(read_from(&primary, some(&legacy)).unwrap(), None);
        // Clearing what is not there is success.
        delete_from(&primary, some(&legacy)).unwrap();
        delete_from(&primary, None).unwrap();
    }

    #[test]
    fn clearing_without_a_secret_service_clears_the_session_copy() {
        let primary = Fake::empty(Mode::Unreachable);
        let legacy = Fake::holding(Mode::Works, "sk-session");
        delete_from(&primary, some(&legacy)).unwrap();
        assert_eq!(legacy.value(), None);
    }

    #[test]
    fn clearing_a_store_that_cannot_be_reached_and_has_no_fallback_fails() {
        let primary = Fake::empty(Mode::Unreachable);
        let err = delete_from(&primary, None).expect_err("could not clear it");
        assert!(matches!(err, keyring::Error::PlatformFailure(_)));
    }

    #[test]
    fn clearing_a_locked_store_fails() {
        let primary = Fake::holding(Mode::Locked, "sk-a");
        let legacy = Fake::empty(Mode::Works);
        let err = delete_from(&primary, some(&legacy)).expect_err("could not clear it");
        assert!(matches!(err, keyring::Error::NoStorageAccess(_)));
    }

    /// If the kernel copy stayed, the next read would move it back into
    /// the Secret Service and the cleared key would be back.
    #[test]
    fn a_kernel_keyring_copy_that_cannot_be_deleted_fails_the_clear() {
        let primary = Fake::holding(Mode::Works, "sk-a");
        let legacy = Fake::holding(Mode::Locked, "sk-b");
        let err = delete_from(&primary, some(&legacy)).expect_err("a copy survives");
        assert!(matches!(err, keyring::Error::NoStorageAccess(_)));
        assert_eq!(primary.value(), None, "the rest was still cleared");
    }

    // --- probe ---

    #[test]
    fn probe_is_session_only_only_when_the_secret_service_is_unreachable() {
        assert_eq!(
            probe(&Fake::holding(Mode::Works, "x"), true),
            Persistence::Durable
        );
        assert_eq!(probe(&Fake::empty(Mode::Works), true), Persistence::Durable);
        assert_eq!(
            probe(&Fake::empty(Mode::Locked), true),
            Persistence::Durable,
            "a locked store is still the durable one"
        );
        match probe(&Fake::empty(Mode::Unreachable), true) {
            Persistence::SessionOnly { reason } => {
                assert!(reason.contains("no secret service"), "{reason}");
            }
            other => panic!("expected session-only, got {other:?}"),
        }
    }

    #[test]
    fn probe_without_a_fallback_store_is_durable_and_reads_nothing() {
        let primary = Fake::empty(Mode::Unreachable);
        assert_eq!(probe(&primary, false), Persistence::Durable);
        assert_eq!(primary.gets.get(), 0, "no extra keychain prompt");
    }

    // --- the error type ---

    #[test]
    fn keychain_error_never_carries_the_secret() {
        let e = KeychainError::from(keyring::Error::BadEncoding(b"sk-secret".to_vec()));
        assert!(!e.to_string().contains("sk-secret"), "{e}");
        assert!(!format!("{e:?}").contains("sk-secret"), "{e:?}");
        assert!(e
            .to_string()
            .starts_with("the system keychain could not be read: "));
    }

    #[test]
    fn active_provider_is_trimmed_and_blank_means_no_preference() {
        assert_eq!(active_provider_from(None), None);
        assert_eq!(active_provider_from(Some("  \n".into())), None);
        assert_eq!(
            active_provider_from(Some(" openrouter\n".into())).as_deref(),
            Some("openrouter")
        );
    }
}
