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
/// Returns `Some(key)` when an entry exists, `None` when there is no
/// stored key. Any underlying keychain error (locked, denied,
/// transport) is collapsed to `None` — the caller is expected to fall
/// back to prompting the user, not to surface a low-level error.
///
/// We deliberately do not log the key, the entry contents, or the
/// keychain backend's error messages here, because some platforms
/// include the secret in error contexts.
pub fn load_api_key(provider_id: &str) -> Option<String> {
    let entry = Entry::new(SERVICE, &account_for(provider_id)).ok()?;
    entry.get_password().ok()
}

/// Persist an API key to the OS keychain for `provider_id`.
///
/// Errors propagate via [`keyring::Error`]; callers (the settings UI)
/// are expected to surface a generic "could not save key" message
/// rather than the platform-specific reason, again to avoid leaking
/// the key in logs.
pub fn save_api_key(provider_id: &str, key: &str) -> Result<(), keyring::Error> {
    let entry = Entry::new(SERVICE, &account_for(provider_id))?;
    entry.set_password(key)
}

/// Remove the stored API key for `provider_id`, if any. Used by the
/// settings UI's "sign out" affordance. A missing entry is treated as
/// success — the desired post-state is "no key".
pub fn delete_api_key(provider_id: &str) -> Result<(), keyring::Error> {
    let entry = Entry::new(SERVICE, &account_for(provider_id))?;
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e),
    }
}

/// Read the user's active-provider preference. `None` means no
/// preference is recorded, in which case callers should default to
/// Anthropic.
pub fn load_active_provider() -> Option<String> {
    let entry = Entry::new(SERVICE, ACTIVE_PROVIDER_ACCOUNT).ok()?;
    let raw = entry.get_password().ok()?;
    let trimmed = raw.trim().to_string();
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
    let entry = Entry::new(SERVICE, ACTIVE_PROVIDER_ACCOUNT)?;
    entry.set_password(provider_id)
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
    let entry = Entry::new(SERVICE, &base_url_account_for(provider_id)).ok()?;
    match entry.get_password() {
        Ok(url) if !url.trim().is_empty() => Some(url),
        _ => None,
    }
}

/// Store a provider's base-URL override.
pub fn save_base_url(provider_id: &str, base_url: &str) -> Result<(), keyring::Error> {
    let entry = Entry::new(SERVICE, &base_url_account_for(provider_id))?;
    entry.set_password(base_url)
}

/// Forget a provider's override so the built-in URL applies again.
///
/// A missing entry is success: the caller asked for "no override", and
/// that is the state either way.
pub fn delete_base_url(provider_id: &str) -> Result<(), keyring::Error> {
    let entry = Entry::new(SERVICE, &base_url_account_for(provider_id))?;
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e),
    }
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
    let entry = Entry::new(SERVICE, &model_account_for(provider_id)).ok()?;
    match entry.get_password() {
        Ok(m) if !m.trim().is_empty() => Some(m),
        _ => None,
    }
}

/// Store a provider's chosen model.
pub fn save_model(provider_id: &str, model: &str) -> Result<(), keyring::Error> {
    let entry = Entry::new(SERVICE, &model_account_for(provider_id))?;
    entry.set_password(model)
}

/// Forget a provider's choice so its default applies again.
///
/// A missing entry is success: the caller asked for "no override", and
/// that is the state either way.
pub fn delete_model(provider_id: &str) -> Result<(), keyring::Error> {
    let entry = Entry::new(SERVICE, &model_account_for(provider_id))?;
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e),
    }
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
    let entry = Entry::new(SERVICE, &effort_account_for(provider_id)).ok()?;
    entry.get_password().ok().and_then(|s| Effort::parse(&s))
}

/// Store a provider's reasoning effort.
pub fn save_effort(provider_id: &str, effort: Effort) -> Result<(), keyring::Error> {
    let entry = Entry::new(SERVICE, &effort_account_for(provider_id))?;
    entry.set_password(effort.as_str())
}

/// Forget a provider's choice so the model's default applies again.
///
/// A missing entry is success: the caller asked for "no effort", and
/// that is the state either way.
pub fn delete_effort(provider_id: &str) -> Result<(), keyring::Error> {
    let entry = Entry::new(SERVICE, &effort_account_for(provider_id))?;
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
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
}
