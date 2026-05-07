//! OS keychain wrapper for the Anthropic API key.
//!
//! On macOS this resolves to the user's login keychain via Security.framework;
//! on Windows it lands in the Credential Manager; on Linux it uses
//! libsecret/Secret Service. The key is never written to disk by edytlab
//! itself and never logged.
//!
//! The [`Entry`](keyring::Entry) name is namespaced under the bundle
//! identifier so it doesn't collide with other apps' Anthropic keys the
//! user may have stored.

use keyring::Entry;

/// Service name used in the OS keychain. Must match the bundle id
/// declared in `tauri.conf.json` so the macOS keychain prompt names the
/// app correctly.
const SERVICE: &str = "app.edytlab.desktop";
const ACCOUNT: &str = "anthropic_api_key";

/// Read the Anthropic API key from the OS keychain.
///
/// Returns `Some(key)` when an entry exists, `None` when there is no
/// stored key. Any underlying keychain error (locked, denied, transport)
/// is collapsed to `None` — the caller is expected to fall back to
/// prompting the user, not to surface a low-level error.
///
/// We deliberately do not log the key, the entry contents, or the
/// keychain backend's error messages here, because some platforms
/// include the secret in error contexts.
pub fn load_api_key() -> Option<String> {
    let entry = Entry::new(SERVICE, ACCOUNT).ok()?;
    entry.get_password().ok()
}

/// Persist the Anthropic API key to the OS keychain.
///
/// Errors propagate via [`keyring::Error`]; callers (the settings UI) are
/// expected to surface a generic "could not save key" message rather than
/// the platform-specific reason, again to avoid leaking the key in logs.
pub fn save_api_key(key: &str) -> Result<(), keyring::Error> {
    let entry = Entry::new(SERVICE, ACCOUNT)?;
    entry.set_password(key)
}

/// Remove the stored API key, if any. Used by the settings UI's "sign
/// out" affordance.
pub fn delete_api_key() -> Result<(), keyring::Error> {
    let entry = Entry::new(SERVICE, ACCOUNT)?;
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        // `NoEntry` is fine — the desired post-state is "no key".
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e),
    }
}
