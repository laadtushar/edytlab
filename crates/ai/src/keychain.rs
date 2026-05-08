//! OS keychain wrapper for LLM provider API keys.
//!
//! On macOS this resolves to the user's login keychain via Security.framework;
//! on Windows it lands in the Credential Manager; on Linux it uses
//! libsecret/Secret Service. Keys are never written to disk by edytlab
//! itself and never logged.
//!
//! # Per-provider slots
//!
//! Each provider stores its key under a separate keychain account so a
//! user can have credentials for both Anthropic and OpenRouter
//! configured at once and switch between them without re-typing. The
//! account name is `"<provider_id>_api_key"` (e.g.
//! `"anthropic_api_key"`, `"openrouter_api_key"`).
//!
//! # Active provider
//!
//! Which provider is currently selected is stored as a tiny string in
//! its own slot: account `"active_provider"`. `load_active_provider`
//! returns `Some("anthropic")` or `Some("openrouter")`; `None` means
//! "no preference, default to Anthropic" (the historical behaviour).
//!
//! # Back-compat
//!
//! Earlier builds stored the Anthropic key under the bare account
//! `"anthropic_api_key"`, which still matches the new naming scheme — no
//! migration is required. [`load_api_key("anthropic")`] continues to
//! return the legacy entry on first launch.

use keyring::Entry;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_uses_provider_id_suffix() {
        assert_eq!(account_for("anthropic"), "anthropic_api_key");
        assert_eq!(account_for("openrouter"), "openrouter_api_key");
    }
}
