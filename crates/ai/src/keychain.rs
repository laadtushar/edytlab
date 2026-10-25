//! Where edytlab keeps its provider settings and its secrets (#734, ADR 0009).
//!
//! # Two homes
//!
//! - **Secrets** are in ONE item of the OS keychain: service
//!   `app.edytlab.desktop`, account `secrets`, whose value is a JSON object
//!   from slot name to value, `{"anthropic_api_key": "...",
//!   "chatgpt_refresh_token": "..."}`. The slot names are the per-slot account
//!   names earlier builds used, so a key is known by one name everywhere. The
//!   item is read **once per process** and cached for reads. Every save and
//!   delete reads the item itself again first (another process may have
//!   changed it), then updates the item and the cache together, under one
//!   lock. It is written and read as UTF-8 bytes (`set_secret`), which on
//!   Windows doubles what fits in one credential. The entry
//!   [`MIGRATED_MARKER`] is not a secret; see below.
//! - **Everything else** (the active provider and each provider's model, base
//!   URL and reasoning effort) is in a JSON file, `provider-settings.json`, in
//!   the directory the app registers with [`set_settings_dir`]. It is written
//!   by writing a temporary file beside it and renaming it over the old one.
//!   It never holds a secret.
//!
//! Why: macOS asks for the login password once for each existing keychain
//! item an app reads when the item's access list does not name that exact
//! build, and the installers are ad-hoc signed, so every update is a new build
//! (#734). With one item the cost is one prompt per launch, and a change to a
//! model, an effort, a base URL or the provider touches no keychain at all.
//! On Linux one item is also at most one Secret Service unlock (#527).
//!
//! # Migration from the per-slot items
//!
//! Earlier builds kept every value as an item of its own: `<provider>_api_key`,
//! `<provider>_model`, `<provider>_base_url`, `<provider>_effort`,
//! `active_provider` and `chatgpt_refresh_token`. The first time anything is
//! asked of this module and the settings file is not there, the migration
//! reads each of those (the last time their prompts appear), writes the
//! `secrets` item, reads it back, writes the settings file, reads that back,
//! and only then deletes the old items it found. The settings file is written
//! last and carries `version`, so a file that is there means the migration
//! finished; the item is then marked ([`MIGRATED_MARKER`]), and an item that
//! carries the mark is never overridden by a leftover per-slot item if the
//! migration runs again. Any failure before that keeps every old item and drops what the
//! migration had written, and the process then runs on the old items (the
//! `Legacy` layout below), exactly as the previous build did; the next launch
//! tries again. A migration is also put off, not run, while the Secret Service
//! could not be reached (a transient outage would otherwise read as "nothing
//! stored", and the keys it holds would be left behind), or when the new item
//! could only be saved to the kernel keyring (the old items would be deleted
//! for a copy that a reboot erases).
//!
//! # An MCP server's secret
//!
//! `<keychain:slot>` in `mcp.json` is read with [`load_api_key`]`(slot)`. A
//! slot that is not a provider id and is not in the `secrets` item is the
//! user's own item `<slot>_api_key`, read from the store on demand. It is
//! never migrated, written or deleted here.
//!
//! # A failed read
//!
//! A read of the `secrets` item that fails with `PlatformFailure` (what a
//! macOS Deny comes back as) is remembered for ten seconds and answered the
//! same way without another keychain call, so the reads of one startup do not
//! each prompt again; after that the next read asks the store, and a save
//! asks at once.
//!
//! # No folder registered
//!
//! [`set_settings_dir`] has to be called before anything is read or saved. A
//! process that has not (the CLI never reads the keychain; a test that forgot)
//! gets an error from every function here, never a file written to a guessed
//! place and never a keychain item read without knowing whether the migration
//! has run.
//!
//! # The platform stores
//!
//! On macOS this resolves to the user's login keychain via
//! Security.framework; on Windows it lands in the Credential Manager.
//!
//! On Linux, values live in the Secret Service (GNOME Keyring, KWallet
//! from Plasma 5.97 / 6, or KeePassXC), reached over D-Bus with an
//! encrypted session (the `keyring` crate's `sync-secret-service` and
//! `crypto-rust` features). So they survive a reboot (#394).
//!
//! When no Secret Service answers (no session bus, or nothing owning
//! `org.freedesktop.secrets`), the `secrets` item falls back to the kernel
//! keyring (`keyutils`) and lasts only until the next reboot. [`persistence`]
//! reports which of the two applies, and the app shows it. The settings file
//! is a file, so it is durable either way, and a save of a setting says so.
//!
//! A copy that an earlier build left in the kernel keyring is moved into the
//! Secret Service on first read, if it is still there (same boot, and within
//! the three-day expiry of the persistent keyring). A move the Secret Service
//! refuses (locked, unlock prompt dismissed) is not tried again until edytlab
//! restarts, since each try is another prompt; the value is still read from
//! the kernel keyring meanwhile.
//!
//! A durable save removes the kernel copy, so a kernel copy next to a
//! durable one is the newer: it was saved while the Secret Service could
//! not be reached. A read prefers it and moves it across the same way. For the
//! `secrets` item the two are merged key by key (the kernel copy winning per
//! key, [`merge_secrets`]), because that copy holds only what the save could
//! see; and a save or removal made while the item cannot be reached never
//! writes the durable item (a removal is an error).
//!
//! A locked or refused store is an error, never a silent "nothing
//! stored": the `try_` readers return it, and the `Option` readers log
//! it. All I/O is serialized behind one lock, because the Secret Service
//! mishandles concurrent D-Bus access, and because a save of a key and a
//! save of the ChatGPT token each rewrite the one item (read, change, write):
//! without the lock one would lose the other.
//!
//! Secrets are never written to disk by edytlab itself. Never log a value,
//! and never `{:?}` a [`keyring::Error`]: its `Debug` output can carry
//! the secret bytes (`BadEncoding`), where its `Display` does not. For the
//! same reason the text of a `serde_json` error is never kept when it was
//! parsing the secrets item: `invalid type: string "sk-..."` carries the value.
//!
//! # Slots
//!
//! A slot is one value with one name: `"<provider_id>_api_key"` (e.g.
//! `"anthropic_api_key"`, `"openrouter_api_key"`) and `"chatgpt_refresh_token"`
//! are secrets; `"<provider_id>_model"`, `"<provider_id>_base_url"`,
//! `"<provider_id>_effort"` and `"active_provider"` are settings. The names
//! are the per-slot keychain accounts of earlier builds, which is what the
//! migration reads, and the key of a secret inside the `secrets` item.
//! Anthropic's key is `"anthropic_api_key"`, the name the build before
//! multi-provider used, so it is found with no special case.
//!
//! `load_active_provider` returns the active provider's id
//! (`Some("anthropic")`, `Some("ollama")`, …); `None` means "no preference,
//! default to Anthropic" (the historical behaviour).

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use keyring::Entry;
use serde::{Deserialize, Serialize};

use crate::anthropic::Effort;

/// Service name used in the OS keychain. Must match the bundle id
/// declared in `tauri.conf.json` so the macOS keychain prompt names the
/// app correctly.
const SERVICE: &str = "app.edytlab.desktop";

/// The one keychain account that holds every secret, as a JSON object.
const SECRETS_ACCOUNT: &str = "secrets";

/// A reserved key of the `secrets` item: the migration from the per-slot items
/// finished. No slot name can equal it (a key slot ends in `_api_key`), and
/// every read looks a slot up by its own name, so nothing reads it as a secret.
const MIGRATED_MARKER: &str = "__migrated";

/// The settings file's name, inside the directory [`set_settings_dir`] was
/// given.
pub const SETTINGS_FILE: &str = "provider-settings.json";

/// What [`SETTINGS_FILE`]'s `version` says now. A file with a version of 1 or
/// more is a finished migration; one without is not a settings file.
const SETTINGS_VERSION: u32 = 1;

/// The legacy account of the "active provider" preference.
const ACTIVE_PROVIDER_ACCOUNT: &str = "active_provider";

/// The account [`persistence`] looks up. Nothing is ever written to it, so
/// the lookup can only ever answer "not found" (or fail to reach the
/// store): the Secret Service unlocks an item only when a search matches
/// one, so a lookup that matches nothing never raises an unlock prompt,
/// and reachability is all the probe needs. Probing a slot that holds
/// something, `secrets` for one, would raise a second prompt on
/// the same locked collection that startup's first read was refused on.
///
/// Its name must collide with no slot: every slot's name ends in
/// `_api_key`, `_model`, `_base_url` or `_effort`, or is
/// [`ACTIVE_PROVIDER_ACCOUNT`], [`CHATGPT_REFRESH_TOKEN_ACCOUNT`] or
/// [`SECRETS_ACCOUNT`] (a test pins this).
const PROBE_ACCOUNT: &str = "persistence_probe";

/// Where the values written now will end up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Persistence {
    /// The platform's own store: the login keychain, Credential Manager,
    /// or a Secret Service on Linux; or, for a setting, the settings file.
    /// Survives a reboot.
    Durable,
    /// Linux with no Secret Service to talk to, so a secret sits in the
    /// kernel keyring. It lasts until the machine restarts.
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
    access_denied: bool,
}

impl KeychainError {
    /// An error with the given human-readable reason.
    pub fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
            access_denied: false,
        }
    }

    /// An error for a store that is there but refused access: locked,
    /// or an unlock prompt that was dismissed.
    pub fn access_denied(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
            access_denied: true,
        }
    }

    /// The reason alone, without the "could not be read" prefix.
    pub fn reason(&self) -> &str {
        &self.reason
    }

    /// Whether the store refused access (locked, or a dismissed unlock
    /// prompt) rather than failing for one slot's sake. Every further
    /// read would be refused the same way, and on a desktop each one
    /// raises its own unlock prompt, so a caller reading several slots
    /// in a row stops at the first of these.
    pub fn is_access_denied(&self) -> bool {
        self.access_denied
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
        Self {
            reason: e.to_string(),
            access_denied: matches!(e, keyring::Error::NoStorageAccess(_)),
        }
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
    /// The value as the bytes the store holds, for the `secrets` item (see
    /// [`Bytes`]).
    fn get_secret(&self) -> keyring::Result<Vec<u8>> {
        self.get().map(String::into_bytes)
    }
    fn set_secret(&self, value: &[u8]) -> keyring::Result<()> {
        match std::str::from_utf8(value) {
            Ok(text) => self.set(text),
            Err(_) => Err(keyring::Error::PlatformFailure("not text".into())),
        }
    }
    fn delete(&self) -> keyring::Result<()>;
}

impl Slot for Entry {
    fn get(&self) -> keyring::Result<String> {
        self.get_password()
    }
    fn set(&self, value: &str) -> keyring::Result<()> {
        self.set_password(value)
    }
    fn get_secret(&self) -> keyring::Result<Vec<u8>> {
        Entry::get_secret(self)
    }
    fn set_secret(&self, value: &[u8]) -> keyring::Result<()> {
        Entry::set_secret(self, value)
    }
    fn delete(&self) -> keyring::Result<()> {
        self.delete_credential()
    }
}

/// A [`Slot`] that notes whether the store answered a read, and if not, why.
/// "Answered" is a value, "not found" or a refusal; a store that could not be
/// reached has not answered, and what a read found then is only the kernel
/// keyring's copy, not the item.
struct Answered<'a> {
    inner: &'a dyn Slot,
    answered: std::cell::Cell<bool>,
    unreachable: std::cell::RefCell<Option<String>>,
}

impl<'a> Answered<'a> {
    fn new(inner: &'a dyn Slot) -> Self {
        Self {
            inner,
            answered: std::cell::Cell::new(false),
            unreachable: std::cell::RefCell::new(None),
        }
    }
}

impl Slot for Answered<'_> {
    fn get(&self) -> keyring::Result<String> {
        let read = self.inner.get();
        match &read {
            Err(e) if unreachable(e) => *self.unreachable.borrow_mut() = Some(e.to_string()),
            _ => {
                self.answered.set(true);
                *self.unreachable.borrow_mut() = None;
            }
        }
        read
    }
    fn set(&self, value: &str) -> keyring::Result<()> {
        self.inner.set(value)
    }
    fn get_secret(&self) -> keyring::Result<Vec<u8>> {
        self.inner.get_secret()
    }
    fn set_secret(&self, value: &[u8]) -> keyring::Result<()> {
        self.inner.set_secret(value)
    }
    fn delete(&self) -> keyring::Result<()> {
        self.inner.delete()
    }
}

/// A [`Slot`] read and written as UTF-8 bytes (`get_secret` / `set_secret`)
/// instead of as a password. Used for the `secrets` item only.
///
/// Why: Windows' Credential Manager holds at most 2,560 bytes per credential,
/// and `set_password` stores UTF-16, so the whole map was limited to 1,280
/// characters. The secret form stores the bytes as they are, and every
/// backend `keyring` 3.6 has in use here (macOS, Windows, Secret Service,
/// the kernel keyring) implements it. A value that is not UTF-8 is an error
/// that carries none of its bytes (`BadEncoding` would).
struct Bytes<'a>(&'a dyn Slot);

impl Slot for Bytes<'_> {
    fn get(&self) -> keyring::Result<String> {
        String::from_utf8(self.0.get_secret()?)
            .map_err(|_| keyring::Error::PlatformFailure("the stored entry is not text".into()))
    }
    fn set(&self, value: &str) -> keyring::Result<()> {
        self.0.set_secret(value.as_bytes())
    }
    fn get_secret(&self) -> keyring::Result<Vec<u8>> {
        self.0.get_secret()
    }
    fn set_secret(&self, value: &[u8]) -> keyring::Result<()> {
        self.0.set_secret(value)
    }
    fn delete(&self) -> keyring::Result<()> {
        self.0.delete()
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
/// - `primary` has it: that is the answer, unless `legacy` holds a
///   different one. [`write_to`] removes the `legacy` copy on every durable
///   save, so a copy that sits next to a durable one was written after it:
///   while `primary` could not be reached (a brief outage), or by a build
///   that only knew the kernel keyring. It is the newer value. It is
///   answered and moved across like any other (below), and a copy equal to
///   the durable one is just dropped. A `legacy` that cannot be read, or
///   holds nothing, changes nothing. (This read is a keyutils call, not a
///   Secret Service one: no prompt.)
/// - `primary` answers "not found" and `legacy` has it: a value an
///   earlier build left behind. Move it into `primary`, and drop the
///   `legacy` copy only once `primary` took it. The value is returned
///   whether or not the move worked. If `primary` refuses the move
///   (locked, prompt dismissed), `move_refused` records it and no later
///   read tries to move anything: each try is its own unlock prompt, and
///   a user who dismissed one has answered for the rest.
/// - `primary` cannot be reached: the answer is whatever `legacy` holds,
///   which is where [`write_to`] put it. If `legacy` holds nothing, ask
///   `primary` once more before calling the value missing: the Secret
///   Service client (`dbus-secret-service`) gives every D-Bus call two
///   seconds, and the first one is where a service that is not running
///   yet gets started, so a slow start reads as "unreachable" with the
///   key sitting right there. Taking that for "no key" shows the
///   first-run prompt over a stored key (#394). Once that second look
///   has also failed, `unreachable_confirmed` records it and later reads
///   skip the second look: a service that has had its two seconds twice
///   is not about to start for the next slot, and startup reads eight
///   slots, so asking twice each is about half a minute of waiting
///   (#497). Any answer from `primary` clears the record.
/// - `primary` fails any other way (locked, refused, ambiguous,
///   undecodable): that is an error. It is never read as "not found",
///   or the app would show a first-run prompt over a key that is there.
fn read_from(
    primary: &dyn Slot,
    legacy: Option<&dyn Slot>,
    move_refused: &AtomicBool,
    unreachable_confirmed: &AtomicBool,
) -> keyring::Result<Option<String>> {
    read_from_merging(primary, legacy, move_refused, unreachable_confirmed, None)
}

/// How a `legacy` copy that sits next to a durable one is combined with it:
/// the durable text and the kernel text in, the text to answer and keep out,
/// or `None` to leave the durable one standing. Without one, the `legacy` copy
/// replaces the durable one, which is right for a value that is one string.
/// The `secrets` item is a map of many, so its copies are merged key by key
/// ([`merge_secrets`]).
type Merge<'a> = Option<&'a dyn Fn(&str, &str) -> Option<String>>;

/// [`read_from`], with the way two copies of one item are combined.
fn read_from_merging(
    primary: &dyn Slot,
    legacy: Option<&dyn Slot>,
    move_refused: &AtomicBool,
    unreachable_confirmed: &AtomicBool,
    merge: Merge<'_>,
) -> keyring::Result<Option<String>> {
    let first = primary.get();
    // The service answered, with a value, "not found", or a refusal:
    // whatever the last reads found, it is not gone now.
    if !matches!(&first, Err(e) if unreachable(e)) {
        unreachable_confirmed.store(false, Ordering::Relaxed);
    }
    match first {
        Ok(v) => Ok(Some(newer_of(primary, legacy, v, move_refused, merge))),
        Err(keyring::Error::NoEntry) => {
            let Some(legacy) = legacy else {
                return Ok(None);
            };
            let Ok(v) = legacy.get() else {
                return Ok(None);
            };
            move_across(primary, legacy, &v, move_refused);
            Ok(Some(v))
        }
        Err(e) if unreachable(&e) => {
            let Some(legacy) = legacy else {
                return Err(e);
            };
            match legacy.get() {
                Ok(v) => Ok(Some(v)),
                // A second look has already failed in this process.
                Err(keyring::Error::NoEntry) if unreachable_confirmed.load(Ordering::Relaxed) => {
                    Ok(None)
                }
                Err(keyring::Error::NoEntry) => match primary.get() {
                    Ok(v) => Ok(Some(v)),
                    // Only a second failure to reach it makes this a
                    // machine with no Secret Service and nothing stored.
                    Err(keyring::Error::NoEntry) => Ok(None),
                    Err(ref again) if unreachable(again) => {
                        unreachable_confirmed.store(true, Ordering::Relaxed);
                        Ok(None)
                    }
                    Err(other) => Err(other),
                },
                Err(_) => Err(e),
            }
        }
        Err(e) => Err(e),
    }
}

/// The value a read answers when `primary` holds `durable`: a different
/// value in `legacy` is newer (see [`read_from`]), so it is that one (or, for
/// the `secrets` item, the two merged with `legacy` winning key by key), moved
/// into `primary` as [`move_across`] does it, whether or not the move works.
///
/// A copy that is blank is not a value, and one that cannot be read is no
/// copy: `durable` stands.
fn newer_of(
    primary: &dyn Slot,
    legacy: Option<&dyn Slot>,
    durable: String,
    move_refused: &AtomicBool,
    merge: Merge<'_>,
) -> String {
    let Some(legacy) = legacy else {
        return durable;
    };
    match legacy.get() {
        Ok(kernel) if kernel == durable => {
            // Two copies of one value: only the durable one is wanted.
            let _ = legacy.delete();
            durable
        }
        Ok(kernel) if !kernel.trim().is_empty() => {
            let value = match merge {
                Some(merge) => match merge(&durable, &kernel) {
                    Some(merged) => merged,
                    // Not something that can be merged: the durable copy
                    // stands, and nothing is moved or deleted.
                    None => return durable,
                },
                None => kernel,
            };
            move_across(primary, legacy, &value, move_refused);
            value
        }
        _ => durable,
    }
}

/// Move `value` from `legacy` into `primary`, unless an earlier move was
/// refused. `legacy` loses its copy only once `primary` has taken it.
///
/// A refusal (`NoStorageAccess`: a locked store, a dismissed unlock
/// prompt) sets `move_refused`. Any other failure is about this slot
/// alone and the next one is still tried.
fn move_across(primary: &dyn Slot, legacy: &dyn Slot, value: &str, move_refused: &AtomicBool) {
    if move_refused.load(Ordering::Relaxed) {
        return;
    }
    match primary.set(value) {
        Ok(()) => {
            let _ = legacy.delete();
        }
        Err(keyring::Error::NoStorageAccess(_)) => {
            move_refused.store(true, Ordering::Relaxed);
            tracing::warn!(
                "the Secret Service refused a value from the kernel keyring; \
                 not asking again until edytlab restarts"
            );
        }
        Err(_) => {}
    }
}

/// Write a value to `primary`.
///
/// A durable save leaves no `legacy` copy: [`read_from`] takes one that
/// sits next to a durable copy for the newer value.
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
                // copy would be read back if the service went away, and
                // taken for a newer one while it is there ([`read_from`]).
                match legacy.delete() {
                    Ok(()) | Err(keyring::Error::NoEntry) => {}
                    // It stays, so make it say what the durable copy says:
                    // left as it was, it would win over the value just
                    // saved. If this fails too there is nothing more to do.
                    Err(_) => {
                        let _ = legacy.set(value);
                    }
                }
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
/// `primary` should be an account nothing writes ([`PROBE_ACCOUNT`]): the
/// lookup then matches no item, so it cannot unlock anything and cannot
/// prompt, whatever state the store is in.
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
// The slots, and the two homes they live in.
// ---------------------------------------------------------------------------

/// One value this module keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field<'a> {
    ActiveProvider,
    ApiKey(&'a str),
    Model(&'a str),
    BaseUrl(&'a str),
    Effort(&'a str),
    ChatGptToken,
}

impl Field<'_> {
    /// The slot's name: the keychain account of earlier builds, and the key of
    /// a secret inside the `secrets` item.
    fn account(&self) -> String {
        match self {
            Field::ActiveProvider => ACTIVE_PROVIDER_ACCOUNT.to_string(),
            Field::ApiKey(p) => account_for(p),
            Field::Model(p) => model_account_for(p),
            Field::BaseUrl(p) => base_url_account_for(p),
            Field::Effort(p) => effort_account_for(p),
            Field::ChatGptToken => CHATGPT_REFRESH_TOKEN_ACCOUNT.to_string(),
        }
    }

    /// Whether the value belongs in the keychain. Only a key or the ChatGPT
    /// token does.
    fn is_secret(&self) -> bool {
        matches!(self, Field::ApiKey(_) | Field::ChatGptToken)
    }
}

/// The slots an earlier build could have written, which the migration reads:
/// every provider's key, model, base URL and effort, the active provider and
/// the ChatGPT token. The ChatGPT plan has no key and no base-URL slot
/// (nothing may write one), so none is read for it.
fn legacy_keys() -> Vec<Field<'static>> {
    let mut keys = vec![Field::ActiveProvider, Field::ChatGptToken];
    for id in crate::SUPPORTED_PROVIDER_IDS {
        let takes_a_key = *id != crate::CHATGPT_ID;
        if takes_a_key {
            keys.push(Field::ApiKey(id));
            keys.push(Field::BaseUrl(id));
        }
        keys.push(Field::Model(id));
        keys.push(Field::Effort(id));
    }
    keys
}

/// The settings file: everything that is not a secret.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
struct Settings {
    version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    active_provider: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    providers: BTreeMap<String, ProviderSettings>,
    /// Fields a newer build wrote, kept when this one saves.
    #[serde(flatten)]
    extra: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
struct ProviderSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    effort: Option<String>,
    #[serde(flatten)]
    extra: BTreeMap<String, serde_json::Value>,
}

impl ProviderSettings {
    fn is_empty(&self) -> bool {
        self.model.is_none()
            && self.base_url.is_none()
            && self.effort.is_none()
            && self.extra.is_empty()
    }
}

impl Settings {
    fn empty() -> Self {
        Self {
            version: SETTINGS_VERSION,
            ..Self::default()
        }
    }

    /// The field a setting lives in. Panics on a secret: those are not here.
    fn field(&mut self, key: &Field<'_>) -> &mut Option<String> {
        match key {
            Field::ActiveProvider => &mut self.active_provider,
            Field::Model(p) => &mut self.providers.entry((*p).to_string()).or_default().model,
            Field::BaseUrl(p) => &mut self.providers.entry((*p).to_string()).or_default().base_url,
            Field::Effort(p) => &mut self.providers.entry((*p).to_string()).or_default().effort,
            Field::ApiKey(_) | Field::ChatGptToken => {
                unreachable!("a secret is never kept in the settings file")
            }
        }
    }

    fn get(&self, key: &Field<'_>) -> Option<String> {
        match key {
            Field::ActiveProvider => self.active_provider.clone(),
            Field::Model(p) => self.providers.get(*p)?.model.clone(),
            Field::BaseUrl(p) => self.providers.get(*p)?.base_url.clone(),
            Field::Effort(p) => self.providers.get(*p)?.effort.clone(),
            Field::ApiKey(_) | Field::ChatGptToken => None,
        }
    }

    fn set(&mut self, key: &Field<'_>, value: &str) {
        *self.field(key) = Some(value.to_string());
    }

    fn clear(&mut self, key: &Field<'_>) {
        if self.get(key).is_none() {
            return;
        }
        *self.field(key) = None;
        self.providers.retain(|_, p| !p.is_empty());
    }
}

/// What reading the settings file found.
enum FileState {
    Present(Settings),
    Missing,
    /// There, but not a settings file this build can use.
    Corrupt,
    /// There, but it cannot be read (permissions, a folder in the way).
    Unreadable(std::io::ErrorKind),
}

fn read_settings_file(dir: &Path) -> FileState {
    let text = match std::fs::read_to_string(dir.join(SETTINGS_FILE)) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return FileState::Missing,
        Err(e) => return FileState::Unreadable(e.kind()),
    };
    match serde_json::from_str::<Settings>(&text) {
        Ok(s) if s.version >= SETTINGS_VERSION => FileState::Present(s),
        _ => FileState::Corrupt,
    }
}

/// Keep a settings file that is not valid for a look, and let a new one be
/// started. The secrets are in the keychain, so only the choices (model,
/// effort, address, provider) are lost.
fn set_aside_corrupt(dir: &Path) {
    tracing::warn!("the settings file is not valid; moved aside, starting a new one");
    let _ = std::fs::rename(
        dir.join(SETTINGS_FILE),
        dir.join(format!("{SETTINGS_FILE}.bad")),
    );
}

/// Write the file whole: a temporary file beside it, flushed, then renamed
/// over it, so a crash or a full disk leaves the old file or the new one and
/// never half of one. Readable by the owner only; it holds no secret, but it
/// names the user's providers and their servers.
fn write_settings_file(dir: &Path, settings: &Settings) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let json = serde_json::to_vec_pretty(settings).map_err(std::io::Error::other)?;
    let tmp = dir.join(format!("{SETTINGS_FILE}.tmp-{}", std::process::id()));
    let written = (|| {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        let mut file = options.open(&tmp)?;
        file.write_all(&json)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&tmp, dir.join(SETTINGS_FILE))
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
        return written;
    }
    // Make the rename itself durable. Best effort: not every platform can
    // open a directory.
    #[cfg(unix)]
    if let Ok(d) = std::fs::File::open(dir) {
        let _ = d.sync_all();
    }
    Ok(())
}

type SecretMap = BTreeMap<String, String>;

/// Parse the `secrets` item. The error says nothing of the text: a type
/// error from `serde_json` quotes the value it found.
fn parse_secrets(json: &str) -> Result<SecretMap, KeychainError> {
    serde_json::from_str(json)
        .map_err(|_| KeychainError::new("the stored secrets entry is not valid JSON"))
}

/// Two copies of the `secrets` item, the durable one and the kernel keyring's,
/// as one: every key of both, the kernel copy's value where both have one.
///
/// The kernel copy was saved while the Secret Service could not be reached. It
/// holds what that save read (nothing, when nothing was cached) plus the key
/// saved, so it may be a few keys of many, and replacing the durable item with
/// it would delete the rest. `None` when either does not parse: the durable
/// copy stands and the kernel one is left alone.
fn merge_secrets(durable: &str, kernel: &str) -> Option<String> {
    let mut map = parse_secrets(durable).ok()?;
    map.extend(parse_secrets(kernel).ok()?);
    secrets_json(&map).ok()
}

fn secrets_json(map: &SecretMap) -> keyring::Result<String> {
    serde_json::to_string(map).map_err(|_| keyring::Error::PlatformFailure("serialize".into()))
}

/// A read-side failure as the error a write returns.
fn write_error(e: KeychainError) -> keyring::Error {
    if e.is_access_denied() {
        keyring::Error::NoStorageAccess(e.reason().into())
    } else {
        keyring::Error::PlatformFailure(e.reason().into())
    }
}

// ---------------------------------------------------------------------------
// The store: one lock, one cache, the two homes.
// ---------------------------------------------------------------------------

/// The settings directory registered by [`set_settings_dir`].
static SETTINGS_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Register the directory that holds the settings file ([`SETTINGS_FILE`]),
/// once, at startup: the app's config directory. Must be called before any
/// other function of this module is useful (see "No folder registered" above).
///
/// Returns whether this call set it. It does not when `dir` is not an absolute
/// path (a relative one would be wherever the process happened to start) or
/// when a directory was registered already: the first one stands.
pub fn set_settings_dir(dir: PathBuf) -> bool {
    dir.is_absolute() && SETTINGS_DIR.set(dir).is_ok()
}

/// The directory registered with [`set_settings_dir`], if any.
pub fn settings_dir() -> Option<&'static Path> {
    SETTINGS_DIR.get().map(PathBuf::as_path)
}

/// Where a [`Store`] finds the settings directory.
enum DirSource {
    /// What [`set_settings_dir`] registered, looked up at each use.
    Registered,
    /// A directory of the test's own.
    #[cfg(test)]
    Fixed(PathBuf),
}

/// An entry of the primary store and, on Linux, of the kernel keyring.
type SlotPair = (Box<dyn Slot>, Option<Box<dyn Slot>>);
type SlotFactory = Box<dyn Fn(&str) -> keyring::Result<SlotPair> + Send + Sync>;

fn os_slots(account: &str) -> keyring::Result<SlotPair> {
    let primary = open_primary(account)?;
    let legacy = open_legacy(account).map(|e| Box::new(e) as Box<dyn Slot>);
    Ok((Box::new(primary), legacy))
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

/// Which of the two layouts this process uses, decided once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Layout {
    /// Not looked at yet.
    Unknown,
    /// Settings in the file, secrets in the `secrets` item.
    Current,
    /// The migration could not finish, so every slot is still its own
    /// keychain item, as in the build before this one. Lasts until restart.
    Legacy,
}

struct State {
    layout: Layout,
    settings: Settings,
    /// The `secrets` item, once read or written. `None` until then, and while
    /// what is known of it is not the item (a Secret Service that cannot be
    /// reached).
    secrets: Option<SecretMap>,
    /// A read of the `secrets` item the platform failed (`PlatformFailure`:
    /// on macOS that is what a Deny comes back as), and when. Remembered for
    /// [`READ_FAILURE_MEMORY`], as `keychain_refused` is in the desktop (#497)
    /// but not for good: reads inside the window answer the same error with no
    /// keychain call, since each one would raise the prompt again; after it the
    /// next read asks the store. A save is a user action and asks at once.
    read_failed: Option<(KeychainError, Instant)>,
}

/// How long a failed read of the `secrets` item is remembered
/// ([`State::read_failed`]): long enough that the reads of one startup do not
/// each raise the prompt again after a Deny, short enough that a failure that
/// was only a moment (the keychain just after a sleep, a service still
/// starting) does not hide the keys for the rest of the process.
const READ_FAILURE_MEMORY: Duration = Duration::from_secs(10);

/// Why a migration did not finish.
enum NotMigrated {
    /// The keychain refused a read. Like every refusal, it is the caller's
    /// answer and the migration is asked for again at the next call.
    Refused(KeychainError),
    /// Anything else. Every old item is kept and the process runs on them.
    Kept(String),
    /// The settings file could not be written, but the `secrets` item says an
    /// earlier migration finished, so it is the truth and the per-slot items
    /// are leftovers: stay on the current layout, with these settings in
    /// memory (each save tries the file again) and the item's content, if it
    /// was read. Never the per-slot items, whose saves the next launch would
    /// delete.
    Marked(Settings, Option<SecretMap>),
}

/// Whether a read of the `secrets` item was answered by the platform's store.
enum Reach {
    Answered,
    /// Only the kernel keyring was asked (or nothing answered); the string is
    /// why the store did not answer.
    Unreachable(String),
}

struct Store {
    dir: DirSource,
    slots: SlotFactory,
    /// Held across a whole operation, keychain I/O included.
    state: Mutex<State>,
    /// See [`read_from`]; per store, for the rest of the process.
    move_refused: AtomicBool,
    unreachable_confirmed: AtomicBool,
    /// The time, for how long a failed read is remembered. A test moves it.
    now: Box<dyn Fn() -> Instant + Send + Sync>,
}

impl Store {
    fn new(dir: DirSource, slots: SlotFactory) -> Self {
        Self {
            dir,
            slots,
            state: Mutex::new(State {
                layout: Layout::Unknown,
                settings: Settings::empty(),
                secrets: None,
                read_failed: None,
            }),
            move_refused: AtomicBool::new(false),
            unreachable_confirmed: AtomicBool::new(false),
            now: Box::new(Instant::now),
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        // Every change to the state is made after the I/O it depends on
        // worked, so a panic elsewhere cannot have left it half done.
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn dir(&self) -> Option<PathBuf> {
        match &self.dir {
            DirSource::Registered => SETTINGS_DIR.get().cloned(),
            #[cfg(test)]
            DirSource::Fixed(dir) => Some(dir.clone()),
        }
    }

    // --- one keychain item ---

    fn read_item(&self, account: &str) -> keyring::Result<Option<String>> {
        let (primary, legacy) = (self.slots)(account)?;
        if account == SECRETS_ACCOUNT {
            return self.read_secrets_item(&*primary, legacy.as_deref()).0;
        }
        read_from(
            &*primary,
            legacy.as_deref(),
            &self.move_refused,
            &self.unreachable_confirmed,
        )
    }

    /// Read the `secrets` item as bytes, merging a kernel copy into it. Also
    /// answers whether the store itself answered, or only the kernel
    /// keyring did (and why it did not).
    fn read_secrets_item(
        &self,
        primary: &dyn Slot,
        legacy: Option<&dyn Slot>,
    ) -> (keyring::Result<Option<String>>, Reach) {
        let bytes = Bytes(primary);
        let primary = Answered::new(&bytes);
        let legacy = legacy.map(Bytes);
        let read = read_from_merging(
            &primary,
            legacy.as_ref().map(|l| l as &dyn Slot),
            &self.move_refused,
            &self.unreachable_confirmed,
            Some(&merge_secrets),
        );
        let reach = if primary.answered.get() {
            Reach::Answered
        } else {
            Reach::Unreachable(
                primary
                    .unreachable
                    .borrow()
                    .clone()
                    .unwrap_or_else(|| "the Secret Service could not be reached".to_string()),
            )
        };
        (read, reach)
    }

    fn write_item(&self, account: &str, value: &str) -> keyring::Result<Persistence> {
        let (primary, legacy) = (self.slots)(account)?;
        let persistence = if account == SECRETS_ACCOUNT {
            write_to(
                &Bytes(&*primary),
                legacy
                    .as_deref()
                    .map(Bytes)
                    .as_ref()
                    .map(|l| l as &dyn Slot),
                value,
            )?
        } else {
            write_to(&*primary, legacy.as_deref(), value)?
        };
        if let Persistence::SessionOnly { reason } = &persistence {
            tracing::warn!(
                account = %account,
                reason = %reason,
                "no Secret Service; stored in the kernel keyring until reboot"
            );
        }
        Ok(persistence)
    }

    fn delete_item(&self, account: &str) -> keyring::Result<()> {
        let (primary, legacy) = (self.slots)(account)?;
        delete_from(&*primary, legacy.as_deref())
    }

    // --- which layout ---

    /// Decide the layout, once. A settings file that is there means the
    /// migration ran; one that is not there starts it. A call that finds the
    /// keychain refusing a read in the migration fails with that refusal and
    /// asks again next time.
    fn ensure(&self, st: &mut State) -> Result<(), KeychainError> {
        if st.layout != Layout::Unknown {
            return Ok(());
        }
        let Some(dir) = self.dir() else {
            return Err(KeychainError::new(
                "the folder for edytlab's settings was not set up",
            ));
        };
        match read_settings_file(&dir) {
            FileState::Present(settings) => {
                st.settings = settings;
                st.layout = Layout::Current;
                return Ok(());
            }
            FileState::Missing => {}
            FileState::Corrupt => set_aside_corrupt(&dir),
            FileState::Unreadable(kind) => {
                // Not the per-slot layout: if the migration ran, those items
                // are gone, and a read of them would find nothing and a save
                // would write an item nothing reads. Asked again next time.
                tracing::warn!(error_kind = ?kind, "the settings file cannot be read");
                return Err(KeychainError::new(
                    "edytlab's settings file cannot be read (is it a folder, or not readable?)",
                ));
            }
        }
        match self.migrate(&dir) {
            Ok((settings, secrets)) => {
                st.settings = settings;
                st.secrets = secrets;
                st.layout = Layout::Current;
                Ok(())
            }
            Err(NotMigrated::Refused(e)) => Err(e),
            Err(NotMigrated::Marked(settings, secrets)) => {
                tracing::warn!(
                    "the settings file cannot be written; settings are kept in memory \
                     and each save tries the file again"
                );
                st.settings = settings;
                st.secrets = secrets;
                st.layout = Layout::Current;
                Ok(())
            }
            Err(NotMigrated::Kept(reason)) => {
                tracing::warn!(
                    reason = %reason,
                    "settings and keys stay in their per-slot keychain items; \
                     the move to one item is tried again at the next launch"
                );
                st.layout = Layout::Legacy;
                Ok(())
            }
        }
    }

    /// Move the per-slot items into the two new homes (see the module docs).
    /// Returns the settings, and the secrets item's content if it was written.
    fn migrate(&self, dir: &Path) -> Result<(Settings, Option<SecretMap>), NotMigrated> {
        fn kept(e: impl std::fmt::Display) -> NotMigrated {
            NotMigrated::Kept(e.to_string())
        }
        /// The `secrets` item could not be read. Whether it carries the
        /// marker is unknown, so the per-slot items cannot be trusted: a
        /// leftover would be served over a newer key, and a key saved per slot
        /// deleted by the next launch. Reads reject and the migration is asked
        /// for again at the next call.
        fn unreadable(e: keyring::Error) -> NotMigrated {
            NotMigrated::Refused(KeychainError::from(e))
        }
        fn classify(e: keyring::Error) -> NotMigrated {
            let e = KeychainError::from(e);
            if e.is_access_denied() {
                NotMigrated::Refused(e)
            } else {
                NotMigrated::Kept(e.reason().to_string())
            }
        }

        // 1. Read every per-slot item. Nothing is moved between the Secret
        //    Service and the kernel keyring here (`no_moves`): what is found
        //    goes to its new home, and the old items are deleted at the end.
        let no_moves = AtomicBool::new(true);
        let mut settings = Settings::empty();
        let mut secrets = SecretMap::new();
        let mut found = Vec::new();
        for key in legacy_keys() {
            let account = key.account();
            let (primary, legacy) = (self.slots)(&account).map_err(classify)?;
            let value = read_from(
                &*primary,
                legacy.as_deref(),
                &no_moves,
                &self.unreachable_confirmed,
            )
            .map_err(classify)?;
            // A Secret Service that could not be asked answers "nothing
            // stored" for every slot, and the keys it holds would be left
            // behind and then forgotten. Wait until it answers.
            if self.unreachable_confirmed.load(Ordering::Relaxed) {
                return Err(kept("the Secret Service did not answer"));
            }
            let Some(value) = value else { continue };
            found.push(account);
            match key {
                Field::ActiveProvider => {
                    if let Some(p) = active_provider_from(Some(value)) {
                        settings.active_provider = Some(p);
                    }
                }
                Field::Model(_) | Field::BaseUrl(_) => {
                    if !value.trim().is_empty() {
                        settings.set(&key, &value);
                    }
                }
                Field::Effort(_) => {
                    if let Some(effort) = Effort::parse(&value) {
                        settings.set(&key, effort.as_str());
                    }
                }
                Field::ApiKey(_) | Field::ChatGptToken => {
                    if !value.trim().is_empty() {
                        secrets.insert(key.account(), value);
                    }
                }
            }
        }

        // 2. The secrets item. An item that is already there is merged with,
        //    the per-slot items winning: they are what the app was using. Not
        //    when it carries the marker, though: that says an earlier
        //    migration finished, so the item is the newer and what is found
        //    per slot is a leftover whose delete failed, to be deleted again.
        let mut written = None;
        let mut created_item = false;
        let mut wrote_item = false;
        if !secrets.is_empty() {
            // Any failure to read it is the caller's answer, never the old
            // items: an item that could not be read may be marked, and then
            // the old items are leftovers (see `unreadable`).
            let existing = match self.read_item(SECRETS_ACCOUNT).map_err(unreadable)? {
                Some(json) => Some(parse_secrets(&json).map_err(|e| kept(e.reason()))?),
                None => None,
            };
            if existing
                .as_ref()
                .is_some_and(|m| m.contains_key(MIGRATED_MARKER))
            {
                written = existing;
            } else {
                created_item = existing.is_none();
                let mut merged = existing.unwrap_or_default();
                merged.extend(secrets);
                let json =
                    secrets_json(&merged).map_err(|e| kept(KeychainError::from(e).reason()))?;
                let rollback = |this: &Self| {
                    if created_item {
                        let _ = this.delete_item(SECRETS_ACCOUNT);
                    }
                };
                match self.write_item(SECRETS_ACCOUNT, &json) {
                    Ok(Persistence::Durable) => {}
                    Ok(Persistence::SessionOnly { .. }) => {
                        // The old items would go for a copy a reboot erases.
                        rollback(self);
                        return Err(kept("the Secret Service could not be reached to save"));
                    }
                    // A refused write (the prompt for creating the item was
                    // dismissed) is no reason to ask again: stay on the old
                    // items.
                    Err(e) => {
                        rollback(self);
                        return Err(kept(KeychainError::from(e).reason()));
                    }
                }
                let back = self
                    .read_item(SECRETS_ACCOUNT)
                    .ok()
                    .flatten()
                    .and_then(|j| parse_secrets(&j).ok());
                if back.as_ref() != Some(&merged) {
                    rollback(self);
                    return Err(kept("the new secrets entry did not read back"));
                }
                wrote_item = true;
                written = Some(merged);
            }
        }

        // 3. The settings file, last: its presence says the migration is done.
        let undo = |this: &Self| {
            if created_item {
                let _ = this.delete_item(SECRETS_ACCOUNT);
            }
        };
        // A failure here keeps the old items, unless the item is marked: then
        // it is the truth, and the old items are not used (see `Marked`).
        let stay = |this: &Self, why: String| {
            undo(this);
            let marked = !created_item
                && (written
                    .as_ref()
                    .is_some_and(|m| m.contains_key(MIGRATED_MARKER))
                    || match this.read_item(SECRETS_ACCOUNT) {
                        Ok(json) => json
                            .and_then(|j| parse_secrets(&j).ok())
                            .is_some_and(|m| m.contains_key(MIGRATED_MARKER)),
                        Err(e) => return unreadable(e),
                    });
            if marked {
                NotMigrated::Marked(settings.clone(), written.clone())
            } else {
                NotMigrated::Kept(why)
            }
        };
        if let Err(e) = write_settings_file(dir, &settings) {
            return Err(stay(
                self,
                format!("the settings file could not be written: {e}"),
            ));
        }
        match read_settings_file(dir) {
            FileState::Present(back) if back == settings => {}
            _ => {
                let _ = std::fs::remove_file(dir.join(SETTINGS_FILE));
                return Err(stay(
                    self,
                    "the settings file did not read back".to_string(),
                ));
            }
        }

        // 3b. Now that the file is there, say so in the item (see 2). If this
        //     fails nothing is lost: it is what the migration did before the
        //     marker, and the per-slot items are deleted just the same.
        if wrote_item {
            if let Some(map) = written.as_mut() {
                let mut marked = map.clone();
                marked.insert(MIGRATED_MARKER.to_string(), "1".to_string());
                let done = secrets_json(&marked)
                    .map_err(KeychainError::from)
                    .and_then(|json| self.write_item(SECRETS_ACCOUNT, &json).map_err(Into::into));
                match done {
                    Ok(Persistence::Durable) => *map = marked,
                    _ => tracing::warn!("the secrets entry could not be marked as migrated"),
                }
            }
        }

        // 4. The per-slot items are not read again. Deleting one that stays
        //    is a leftover, not a loss.
        for account in &found {
            if let Err(e) = self.delete_item(account) {
                tracing::warn!(account = %account, error = %e, "an old keychain item could not be deleted");
            }
        }
        tracing::info!(
            moved = found.len(),
            "provider settings and keys moved to one keychain item and a settings file"
        );
        Ok((settings, written))
    }

    // --- the secrets item ---

    /// The `secrets` item's content: the cache, or one read of the item.
    ///
    /// An item that is not there is an empty map, remembered like any other
    /// answer, except when the reason is a Secret Service that could not be
    /// asked: that is no answer, and the next call asks again. A read the
    /// platform failed is remembered as the answer (see
    /// [`State::read_failed`]).
    fn load_secrets(&self, st: &mut State) -> Result<SecretMap, KeychainError> {
        if let Some(map) = &st.secrets {
            return Ok(map.clone());
        }
        Ok(self.read_secrets(st)?.0)
    }

    /// Read the item itself, whatever the cache holds: what a save or a
    /// removal changes. A second process may have changed it since this one
    /// read it (a ChatGPT token it refreshed), and a change made to a copy
    /// that is out of date puts the old value back. One read of an item this
    /// build wrote, which raises no prompt.
    ///
    /// Answers the map to change and whether the store answered. When it did
    /// not (a Secret Service that cannot be reached), the map is what is
    /// known: the cache and the kernel keyring's copy, which together are
    /// what this process saw, and it is not cached.
    fn read_secrets(&self, st: &mut State) -> Result<(SecretMap, Reach), KeychainError> {
        if let Some((failure, at)) = &st.read_failed {
            if (self.now)().saturating_duration_since(*at) < READ_FAILURE_MEMORY {
                return Err(failure.clone());
            }
            st.read_failed = None;
        }
        let (primary, legacy) = (self.slots)(SECRETS_ACCOUNT)?;
        let (read, reach) = self.read_secrets_item(&*primary, legacy.as_deref());
        let found = match read {
            Ok(found) => found,
            Err(e) => {
                let platform_failure = matches!(e, keyring::Error::PlatformFailure(_));
                let e = KeychainError::from(e);
                if platform_failure {
                    st.read_failed = Some((e.clone(), (self.now)()));
                }
                return Err(e);
            }
        };
        let item = match found {
            Some(json) => Some(parse_secrets(&json)?),
            None => None,
        };
        match reach {
            Reach::Answered => {
                let map = item.unwrap_or_default();
                st.secrets = Some(map.clone());
                Ok((map, Reach::Answered))
            }
            Reach::Unreachable(why) => {
                let mut map = st.secrets.clone().unwrap_or_default();
                map.extend(item.unwrap_or_default());
                Ok((map, Reach::Unreachable(why)))
            }
        }
    }

    /// Replace the item with `map` (or remove it when `map` is empty), and the
    /// cache with it, or neither when the item cannot be written.
    ///
    /// With `reach` unreachable the item is not touched: the map goes to the
    /// kernel keyring alone, so a store that answers again a moment later is
    /// not written from a map that did not include what it holds. The next
    /// read merges the two.
    fn store_secrets(
        &self,
        st: &mut State,
        map: SecretMap,
        reach: Reach,
    ) -> keyring::Result<Persistence> {
        let persistence = match reach {
            Reach::Unreachable(why) => {
                let (_, legacy) = (self.slots)(SECRETS_ACCOUNT)?;
                let Some(legacy) = legacy else {
                    return Err(keyring::Error::PlatformFailure(why.into()));
                };
                Bytes(&*legacy).set(&secrets_json(&map)?)?;
                tracing::warn!(
                    reason = %why,
                    "no Secret Service; stored in the kernel keyring until reboot"
                );
                Persistence::SessionOnly { reason: why }
            }
            Reach::Answered if map.is_empty() => {
                self.delete_item(SECRETS_ACCOUNT)?;
                Persistence::Durable
            }
            Reach::Answered => self.write_item(SECRETS_ACCOUNT, &secrets_json(&map)?)?,
        };
        match persistence {
            Persistence::Durable => st.secrets = Some(map),
            // The item does not hold this yet: a copy that said it did would
            // be read for the item after the service is back.
            Persistence::SessionOnly { .. } => {
                if st.secrets.is_some() {
                    st.secrets = Some(map);
                }
            }
        }
        Ok(persistence)
    }

    // --- the operations the public functions are made of ---

    fn get(&self, key: Field<'_>) -> Result<Option<String>, KeychainError> {
        let mut st = self.lock();
        self.ensure(&mut st)?;
        if st.layout == Layout::Legacy {
            return Ok(self.read_item(&key.account())?);
        }
        if !key.is_secret() {
            return Ok(st.settings.get(&key));
        }
        let account = key.account();
        if let Some(value) = self.load_secrets(&mut st)?.remove(&account) {
            return Ok(Some(value));
        }
        // An MCP server's `<keychain:slot>` names a key the user made in the
        // OS store by hand: `<slot>_api_key`, not a provider's, so never in
        // the item and never migrated. Read on demand. A provider's own slot
        // that is not set does not come here: it costs no second read.
        if let Field::ApiKey(slot) = key {
            if !crate::SUPPORTED_PROVIDER_IDS.contains(&slot) {
                return Ok(self.read_item(&account)?);
            }
        }
        Ok(None)
    }

    fn put(&self, key: Field<'_>, value: &str) -> keyring::Result<Persistence> {
        let mut st = self.lock();
        self.ensure(&mut st).map_err(write_error)?;
        if st.layout == Layout::Legacy {
            return self.write_item(&key.account(), value);
        }
        if key.is_secret() {
            // A save is the user's act: it asks again after a failed read.
            st.read_failed = None;
            let (mut map, reach) = self.read_secrets(&mut st).map_err(write_error)?;
            map.insert(key.account(), value.to_string());
            return self.store_secrets(&mut st, map, reach);
        }
        let account = key.account();
        self.change_settings(&mut st, |settings| settings.set(&key, value))
            .inspect_err(
                |_| tracing::warn!(account = %account, "the settings file could not be saved"),
            )
    }

    fn remove(&self, key: Field<'_>) -> keyring::Result<()> {
        let mut st = self.lock();
        self.ensure(&mut st).map_err(write_error)?;
        if st.layout == Layout::Legacy {
            return self.delete_item(&key.account());
        }
        if key.is_secret() {
            st.read_failed = None;
            let (mut map, reach) = self.read_secrets(&mut st).map_err(write_error)?;
            // A removal cannot be kept while the item cannot be reached: the
            // kernel copy that would carry it is merged over the item, which
            // still holds the key, and brings it back. Say so.
            if let Reach::Unreachable(why) = reach {
                return Err(keyring::Error::PlatformFailure(why.into()));
            }
            if map.remove(&key.account()).is_none() {
                return Ok(());
            }
            return self
                .store_secrets(&mut st, map, Reach::Answered)
                .map(|_| ());
        }
        self.change_settings(&mut st, |settings| settings.clear(&key))
            .map(|_| ())
    }

    /// Change the settings file. The file is read again first: another process
    /// may have changed it (a model picked in a second window), and writing
    /// this process's copy would put its old values back.
    fn change_settings(
        &self,
        st: &mut State,
        change: impl FnOnce(&mut Settings),
    ) -> keyring::Result<Persistence> {
        let Some(dir) = self.dir() else {
            return Err(keyring::Error::PlatformFailure(
                "the folder for edytlab's settings was not set up".into(),
            ));
        };
        let current = match read_settings_file(&dir) {
            FileState::Present(on_disk) => on_disk,
            // Gone or damaged since it was read: what this process holds is
            // the best there is, and the write puts a good file in its place.
            FileState::Missing => st.settings.clone(),
            FileState::Corrupt => {
                set_aside_corrupt(&dir);
                st.settings.clone()
            }
            FileState::Unreadable(_) => {
                return Err(keyring::Error::PlatformFailure(
                    "the settings file cannot be read".into(),
                ))
            }
        };
        let mut next = current.clone();
        change(&mut next);
        if next != current {
            write_settings_file(&dir, &next).map_err(|e| {
                keyring::Error::PlatformFailure(
                    format!("the settings file could not be written: {e}").into(),
                )
            })?;
        }
        st.settings = next;
        Ok(Persistence::Durable)
    }

    fn persistence(&self) -> Persistence {
        let _guard = self.lock();
        let Ok((primary, legacy)) = (self.slots)(PROBE_ACCOUNT) else {
            tracing::warn!("could not build the keychain entry to probe persistence");
            return Persistence::Durable;
        };
        probe(&*primary, legacy.is_some())
    }
}

/// The store of this process.
fn store() -> &'static Store {
    static STORE: OnceLock<Store> = OnceLock::new();
    STORE.get_or_init(|| Store::new(DirSource::Registered, Box::new(os_slots)))
}

/// Read for the loaders that answer with an `Option`: an unreadable
/// keychain is logged and reads as nothing stored. Callers that must
/// tell the two apart use the `try_` variants.
fn or_warn(key: Field<'_>, read: Result<Option<String>, KeychainError>) -> Option<String> {
    match read {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(account = %key.account(), error = %e, "keychain read failed");
            None
        }
    }
}

/// Whether secrets saved now survive a reboot.
///
/// Always [`Persistence::Durable`] on macOS and Windows. On Linux it is
/// [`Persistence::SessionOnly`] while no Secret Service answers, which
/// the app surfaces so the user is not surprised by a Welcome screen
/// after the next restart (#394).
///
/// Looks up [`PROBE_ACCOUNT`], which nothing writes, so it never raises
/// an unlock prompt.
pub fn persistence() -> Persistence {
    store().persistence()
}

/// Build the per-provider key slot's name.
///
/// Anthropic intentionally lands on `"anthropic_api_key"` — the same
/// name the pre-multi-provider build used — so existing keychain
/// entries are still found on first launch after upgrade.
fn account_for(provider_id: &str) -> String {
    format!("{provider_id}_api_key")
}

/// Read an API key from the keychain for `provider_id`.
///
/// Returns `Some(key)` when one is stored and `None` when there is no
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
    let key = Field::ApiKey(provider_id);
    or_warn(key, store().get(key))
}

/// Like [`load_api_key`], but a keychain that cannot be read is an
/// error rather than `None`.
pub fn try_load_api_key(provider_id: &str) -> Result<Option<String>, KeychainError> {
    store().get(Field::ApiKey(provider_id))
}

/// Persist an API key to the keychain for `provider_id`.
///
/// Errors propagate via [`keyring::Error`]; callers (the settings UI)
/// are expected to surface a generic "could not save key" message
/// rather than the platform-specific reason, again to avoid leaking
/// the key in logs.
///
/// Returns where the key ended up: [`Persistence::SessionOnly`] is a save
/// that fell back to the kernel keyring because no Secret Service could be
/// reached, and will not survive a reboot. (The same goes for every
/// `save_*` below; a setting's save is [`Persistence::Durable`], because the
/// settings file is a file.)
pub fn save_api_key(provider_id: &str, key: &str) -> Result<Persistence, keyring::Error> {
    store().put(Field::ApiKey(provider_id), key)
}

/// Remove the stored API key for `provider_id`, if any. Used by the
/// settings UI's "sign out" affordance. A missing entry is treated as
/// success — the desired post-state is "no key".
pub fn delete_api_key(provider_id: &str) -> Result<(), keyring::Error> {
    store().remove(Field::ApiKey(provider_id))
}

/// Read the user's active-provider preference. `None` means no
/// preference is recorded, in which case callers should default to
/// Anthropic. An unreadable store is logged and reads as `None`;
/// see [`try_load_active_provider`].
pub fn load_active_provider() -> Option<String> {
    active_provider_from(or_warn(
        Field::ActiveProvider,
        store().get(Field::ActiveProvider),
    ))
}

/// Like [`load_active_provider`], but a store that cannot be read is
/// an error rather than `None`.
pub fn try_load_active_provider() -> Result<Option<String>, KeychainError> {
    store().get(Field::ActiveProvider).map(active_provider_from)
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

/// Persist the active-provider preference, in the settings file.
pub fn save_active_provider(provider_id: &str) -> Result<Persistence, keyring::Error> {
    store().put(Field::ActiveProvider, provider_id)
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
/// This is configuration rather than a secret, so it is in the settings
/// file and reading it touches no keychain (#734).
pub fn load_base_url(provider_id: &str) -> Option<String> {
    let key = Field::BaseUrl(provider_id);
    base_url_from(or_warn(key, store().get(key)))
}

/// Like [`load_base_url`], but a store that cannot be read is an error
/// rather than `None`.
///
/// Settings needs the difference: it saves what it showed, and a blank
/// that stood for "could not be read" would be saved as "no override",
/// deleting the one that is stored (#497).
pub fn try_load_base_url(provider_id: &str) -> Result<Option<String>, KeychainError> {
    store().get(Field::BaseUrl(provider_id)).map(base_url_from)
}

/// The stored URL, with a blank one counting as no override.
fn base_url_from(raw: Option<String>) -> Option<String> {
    raw.filter(|url| !url.trim().is_empty())
}

/// Store a provider's base-URL override.
pub fn save_base_url(provider_id: &str, base_url: &str) -> Result<Persistence, keyring::Error> {
    store().put(Field::BaseUrl(provider_id), base_url)
}

/// Forget a provider's override so the built-in URL applies again.
///
/// A missing entry is success: the caller asked for "no override", and
/// that is the state either way.
pub fn delete_base_url(provider_id: &str) -> Result<(), keyring::Error> {
    store().remove(Field::BaseUrl(provider_id))
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
    let key = Field::Model(provider_id);
    or_warn(key, store().get(key)).filter(|m| !m.trim().is_empty())
}

/// Like [`load_model`], but a store that cannot be read is an error
/// rather than `None`. Startup reads every provider's model in a row and
/// needs to tell a locked keychain, where it stops, from a slot that is
/// empty.
pub fn try_load_model(provider_id: &str) -> Result<Option<String>, KeychainError> {
    store()
        .get(Field::Model(provider_id))
        .map(|model| model.filter(|m| !m.trim().is_empty()))
}

/// Store a provider's chosen model.
pub fn save_model(provider_id: &str, model: &str) -> Result<Persistence, keyring::Error> {
    store().put(Field::Model(provider_id), model)
}

/// Forget a provider's choice so its default applies again.
///
/// A missing entry is success: the caller asked for "no override", and
/// that is the state either way.
pub fn delete_model(provider_id: &str) -> Result<(), keyring::Error> {
    store().remove(Field::Model(provider_id))
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
    let key = Field::Effort(provider_id);
    effort_from(or_warn(key, store().get(key)))
}

/// Like [`load_effort`], but a store that cannot be read is an error
/// rather than `None`. A stored string that is not a level is still
/// `Ok(None)`: the store answered, and the answer is "the default".
pub fn try_load_effort(provider_id: &str) -> Result<Option<Effort>, KeychainError> {
    store().get(Field::Effort(provider_id)).map(effort_from)
}

/// The stored level, with anything that is not one of the five counting
/// as unset.
fn effort_from(raw: Option<String>) -> Option<Effort> {
    raw.and_then(|s| Effort::parse(&s))
}

/// Store a provider's reasoning effort.
pub fn save_effort(provider_id: &str, effort: Effort) -> Result<Persistence, keyring::Error> {
    store().put(Field::Effort(provider_id), effort.as_str())
}

/// Forget a provider's choice so the model's default applies again.
///
/// A missing entry is success: the caller asked for "no effort", and
/// that is the state either way.
pub fn delete_effort(provider_id: &str) -> Result<(), keyring::Error> {
    store().remove(Field::Effort(provider_id))
}

// ---------------------------------------------------------------------------
// The ChatGPT plan's refresh token (#474)
// ---------------------------------------------------------------------------

/// The refresh token of the signed-in ChatGPT account. The only slot the
/// ChatGPT plan has: no `chatgpt_api_key`, no `chatgpt_base_url` (a key or an
/// address for a provider that takes neither), and the access token is never
/// stored. It is the slot the session's
/// [`RefreshTokenStore`](crate::chatgpt::store::RefreshTokenStore) names
/// ([`crate::chatgpt::store::REFRESH_TOKEN_SLOT`]; a test holds the two
/// together), and it is a secret like an API key: a member of the `secrets`
/// item, so the Secret Service, the Linux kernel-keyring fallback (#394) and
/// the access-refused handling behave exactly as they do for an API key.
const CHATGPT_REFRESH_TOKEN_ACCOUNT: &str = "chatgpt_refresh_token";

/// Read the ChatGPT refresh token. A keychain that cannot be read is an
/// error and not "no token": the session ends differently for the two.
pub fn try_load_chatgpt_refresh_token() -> Result<Option<String>, KeychainError> {
    store()
        .get(Field::ChatGptToken)
        .map(|t| t.filter(|t| !t.trim().is_empty()))
}

/// Save the ChatGPT refresh token, replacing any other. Says where it ended
/// up, like every `save_*`: [`Persistence::SessionOnly`] is the kernel
/// keyring, gone at the next reboot.
pub fn save_chatgpt_refresh_token(token: &str) -> Result<Persistence, keyring::Error> {
    store().put(Field::ChatGptToken, token)
}

/// Remove the ChatGPT refresh token, from both stores. Missing is success.
pub fn delete_chatgpt_refresh_token() -> Result<(), keyring::Error> {
    store().remove(Field::ChatGptToken)
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};

    use super::*;

    /// The refresh token has one slot, the one the session's store names,
    /// and it collides with no other: not a provider's key, address, model or
    /// level, not the active provider, not the persistence probe.
    #[test]
    fn the_chatgpt_refresh_token_has_its_own_slot() {
        assert_eq!(CHATGPT_REFRESH_TOKEN_ACCOUNT, "chatgpt_refresh_token");
        assert_eq!(
            CHATGPT_REFRESH_TOKEN_ACCOUNT,
            crate::chatgpt::store::REFRESH_TOKEN_SLOT
        );
        for p in crate::SUPPORTED_PROVIDER_IDS {
            for other in [
                account_for(p),
                base_url_account_for(p),
                model_account_for(p),
                effort_account_for(p),
            ] {
                assert_ne!(CHATGPT_REFRESH_TOKEN_ACCOUNT, other, "collides for {p}");
            }
        }
        assert_ne!(CHATGPT_REFRESH_TOKEN_ACCOUNT, ACTIVE_PROVIDER_ACCOUNT);
        assert_ne!(CHATGPT_REFRESH_TOKEN_ACCOUNT, PROBE_ACCOUNT);
        // And no slot is made for the plan's key or address.
        assert_ne!(CHATGPT_REFRESH_TOKEN_ACCOUNT, account_for("chatgpt"));
    }

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

    /// The pin for the persistence fix in #394, which no other test in
    /// CI can see: the unit tests below run against fake slots, and CI has
    /// no Secret Service to run the real one against.
    ///
    /// With `keyring`'s Secret Service feature off, the default store on
    /// Linux is the kernel keyring, whose entries last until reboot and
    /// whose builder says so. This is what `Entry::new` builds from.
    #[cfg(target_os = "linux")]
    #[test]
    fn the_linux_default_store_is_the_secret_service_not_the_kernel_keyring() {
        use keyring::credential::CredentialPersistence;
        assert!(matches!(
            keyring::default::default_credential_builder().persistence(),
            CredentialPersistence::UntilDelete
        ));
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
        /// Reads work (a locked collection holds no match for a value
        /// that was never moved in), but creating an item is refused:
        /// the unlock prompt for it was dismissed.
        WriteRefused,
        /// Reads and writes work, but an item cannot be removed: a kernel
        /// keyring copy that stays after a durable save.
        DeleteFails,
    }

    struct Fake {
        mode: Mode,
        value: RefCell<Option<String>>,
        gets: Cell<u32>,
        sets: Cell<u32>,
        /// How many of the first `get`s fail as unreachable whatever the
        /// mode is: a Secret Service whose first D-Bus call timed out
        /// while the daemon was still starting.
        slow_start_gets: Cell<u32>,
    }

    impl Fake {
        fn empty(mode: Mode) -> Self {
            Self {
                mode,
                value: RefCell::new(None),
                gets: Cell::new(0),
                sets: Cell::new(0),
                slow_start_gets: Cell::new(0),
            }
        }
        fn holding(mode: Mode, v: &str) -> Self {
            let f = Self::empty(mode);
            *f.value.borrow_mut() = Some(v.to_string());
            f
        }
        /// Unreachable for the first `n` reads, then `mode`.
        fn slow_to_start(mut self, n: u32) -> Self {
            self.slow_start_gets = Cell::new(n);
            self
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
            if self.slow_start_gets.get() > 0 {
                self.slow_start_gets.set(self.slow_start_gets.get() - 1);
                return Err(no_service());
            }
            match self.mode {
                Mode::Unreachable => Err(no_service()),
                Mode::Locked => Err(locked()),
                Mode::Garbled => Err(keyring::Error::BadEncoding(b"sk-secret".to_vec())),
                Mode::Works | Mode::WriteFails | Mode::WriteRefused | Mode::DeleteFails => {
                    self.value.borrow().clone().ok_or(keyring::Error::NoEntry)
                }
            }
        }
        fn set(&self, v: &str) -> keyring::Result<()> {
            self.sets.set(self.sets.get() + 1);
            match self.mode {
                Mode::Unreachable => Err(no_service()),
                Mode::Locked | Mode::WriteRefused => Err(locked()),
                Mode::WriteFails => {
                    Err(keyring::Error::PlatformFailure("create_item failed".into()))
                }
                Mode::Garbled => Err(keyring::Error::BadEncoding(b"sk-secret".to_vec())),
                Mode::Works | Mode::DeleteFails => {
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
                Mode::DeleteFails => Err(keyring::Error::PlatformFailure("cannot unlink".into())),
                Mode::Works | Mode::WriteFails | Mode::WriteRefused => self
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

    /// A read in a process where no move has been refused yet.
    fn read(primary: &dyn Slot, legacy: Option<&dyn Slot>) -> keyring::Result<Option<String>> {
        read_from(
            primary,
            legacy,
            &AtomicBool::new(false),
            &AtomicBool::new(false),
        )
    }

    // --- read ---

    #[test]
    fn a_missing_value_is_none_not_an_error() {
        let primary = Fake::empty(Mode::Works);
        let legacy = Fake::empty(Mode::Works);
        assert_eq!(read(&primary, some(&legacy)).unwrap(), None);
        assert_eq!(read(&primary, None).unwrap(), None);
    }

    #[test]
    fn a_stored_value_is_read_back() {
        let primary = Fake::holding(Mode::Works, "sk-a");
        assert_eq!(read(&primary, None).unwrap().as_deref(), Some("sk-a"));
    }

    /// The pin for the bug in #394's second half: a store that is there
    /// but will not open must not read as "no key", or the app shows
    /// the first-run Welcome over a key that is stored.
    #[test]
    fn a_locked_store_is_an_error_not_a_missing_value() {
        let primary = Fake::empty(Mode::Locked);
        let legacy = Fake::holding(Mode::Works, "sk-old");
        let err = read(&primary, some(&legacy)).expect_err("locked must not read as None");
        assert!(matches!(err, keyring::Error::NoStorageAccess(_)));
        // And it did not wander into the other store to find an answer.
        assert_eq!(legacy.value().as_deref(), Some("sk-old"));
        let err = read(&primary, None).expect_err("with no fallback store either");
        assert!(matches!(err, keyring::Error::NoStorageAccess(_)));
    }

    #[test]
    fn an_ambiguous_or_undecodable_value_is_an_error() {
        let primary = Fake::empty(Mode::Garbled);
        let legacy = Fake::holding(Mode::Works, "sk-old");
        let err = read(&primary, some(&legacy)).expect_err("garbled must not read as None");
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
            read(&primary, some(&legacy)).unwrap().as_deref(),
            Some("sk-session")
        );
        assert_eq!(legacy.value().as_deref(), Some("sk-session"), "not moved");
    }

    #[test]
    fn an_unreachable_secret_service_with_no_session_copy_reads_as_missing() {
        let primary = Fake::empty(Mode::Unreachable);
        let legacy = Fake::empty(Mode::Works);
        assert_eq!(read(&primary, some(&legacy)).unwrap(), None);
        assert_eq!(
            primary.gets.get(),
            2,
            "asked twice before calling it missing"
        );
    }

    /// The first D-Bus call is where the Secret Service gets started, and
    /// it has two seconds. A slow start answers "unreachable" with the key
    /// stored right there; reading that as "no key" put the first-run
    /// Welcome over it (#394), the symptom this issue is about.
    #[test]
    fn a_secret_service_that_is_slow_to_start_is_asked_again_before_the_key_is_called_missing() {
        let primary = Fake::holding(Mode::Works, "sk-durable").slow_to_start(1);
        let legacy = Fake::empty(Mode::Works);
        assert_eq!(
            read(&primary, some(&legacy)).unwrap().as_deref(),
            Some("sk-durable"),
            "the second look found it"
        );
        assert_eq!(primary.gets.get(), 2);
        assert_eq!(legacy.value(), None);
    }

    /// Nothing stored and a service that was merely slow: the second look
    /// answers "no entry", which is a fresh install, not an error.
    #[test]
    fn a_slow_secret_service_with_nothing_stored_reads_as_missing() {
        let primary = Fake::empty(Mode::Works).slow_to_start(1);
        let legacy = Fake::empty(Mode::Works);
        assert_eq!(read(&primary, some(&legacy)).unwrap(), None);
    }

    /// A service that comes up locked on the second look is a locked
    /// store, which is an error and never "no key".
    #[test]
    fn a_secret_service_that_comes_up_locked_on_the_second_look_is_an_error() {
        let primary = Fake::empty(Mode::Locked).slow_to_start(1);
        let legacy = Fake::empty(Mode::Works);
        let err = read(&primary, some(&legacy)).expect_err("locked must not read as None");
        assert!(matches!(err, keyring::Error::NoStorageAccess(_)));
    }

    /// A session copy is an answer already, so there is no second look
    /// at a service that cannot be reached.
    #[test]
    fn a_session_copy_is_returned_without_a_second_look() {
        let primary = Fake::holding(Mode::Works, "sk-durable").slow_to_start(1);
        let legacy = Fake::holding(Mode::Works, "sk-session");
        assert_eq!(
            read(&primary, some(&legacy)).unwrap().as_deref(),
            Some("sk-session")
        );
        assert_eq!(primary.gets.get(), 1);
    }

    // --- the second look is not repeated for every slot (#497) ---

    /// The bug: startup reads eight slots, and with a Secret Service that
    /// hangs each read took its two seconds twice, so the window waited
    /// about half a minute. Once a second look has failed, the slots after
    /// it get one look each.
    #[test]
    fn one_confirmed_unreachable_read_ends_the_second_look_for_later_slots() {
        let (refused, gone) = (AtomicBool::new(false), AtomicBool::new(false));

        let first = Fake::empty(Mode::Unreachable);
        assert_eq!(
            read_from(&first, some(&Fake::empty(Mode::Works)), &refused, &gone).unwrap(),
            None
        );
        assert_eq!(first.gets.get(), 2, "the first slot gets both looks");
        assert!(gone.load(Ordering::Relaxed), "and it is remembered");

        for slot in 0..7 {
            let later = Fake::empty(Mode::Unreachable);
            assert_eq!(
                read_from(&later, some(&Fake::empty(Mode::Works)), &refused, &gone).unwrap(),
                None,
                "slot {slot}: still nothing stored"
            );
            assert_eq!(later.gets.get(), 1, "slot {slot} was asked once");
        }
    }

    /// A kernel keyring copy is an answer whatever the flag says, so the
    /// shortcut never hides a session-only value.
    #[test]
    fn a_session_copy_is_still_found_once_the_second_look_has_been_dropped() {
        let (refused, gone) = (AtomicBool::new(false), AtomicBool::new(true));
        let primary = Fake::empty(Mode::Unreachable);
        let legacy = Fake::holding(Mode::Works, "sk-session");
        assert_eq!(
            read_from(&primary, some(&legacy), &refused, &gone)
                .unwrap()
                .as_deref(),
            Some("sk-session")
        );
    }

    /// One look that fails is a service that is slow, not one that is gone.
    /// Only a second failure may drop the second look for the slots after.
    #[test]
    fn a_session_copy_or_a_late_answer_does_not_confirm_anything() {
        let (refused, gone) = (AtomicBool::new(false), AtomicBool::new(false));
        read_from(
            &Fake::empty(Mode::Unreachable),
            some(&Fake::holding(Mode::Works, "sk-session")),
            &refused,
            &gone,
        )
        .unwrap();
        assert!(
            !gone.load(Ordering::Relaxed),
            "a session copy settles nothing"
        );

        let slow = Fake::holding(Mode::Works, "sk-durable").slow_to_start(1);
        assert_eq!(
            read_from(&slow, some(&Fake::empty(Mode::Works)), &refused, &gone)
                .unwrap()
                .as_deref(),
            Some("sk-durable")
        );
        assert!(!gone.load(Ordering::Relaxed), "the second look found it");
    }

    /// If the service comes up after startup, reading it again says so, and
    /// the next time it goes quiet it gets both looks again.
    #[test]
    fn a_service_that_answers_again_gets_its_second_look_back() {
        let (refused, gone) = (AtomicBool::new(false), AtomicBool::new(true));
        for answering in [
            Fake::holding(Mode::Works, "sk-a"),
            Fake::empty(Mode::Works),
            Fake::empty(Mode::Locked),
        ] {
            gone.store(true, Ordering::Relaxed);
            let _ = read_from(&answering, some(&Fake::empty(Mode::Works)), &refused, &gone);
            assert!(
                !gone.load(Ordering::Relaxed),
                "an answer (even a refusal) means it is there"
            );
        }

        let quiet_again = Fake::empty(Mode::Unreachable);
        read_from(
            &quiet_again,
            some(&Fake::empty(Mode::Works)),
            &refused,
            &gone,
        )
        .unwrap();
        assert_eq!(quiet_again.gets.get(), 2);
    }

    #[test]
    fn neither_store_answering_is_an_error() {
        let primary = Fake::empty(Mode::Unreachable);
        let legacy = Fake::empty(Mode::Unreachable);
        let err = read(&primary, some(&legacy)).expect_err("nothing could be asked");
        assert!(matches!(err, keyring::Error::PlatformFailure(_)));
        // With no second store at all (macOS, Windows) the same.
        let err = read(&primary, None).expect_err("nothing could be asked");
        assert!(matches!(err, keyring::Error::PlatformFailure(_)));
    }

    #[test]
    fn a_kernel_keyring_copy_is_moved_into_the_secret_service() {
        let primary = Fake::empty(Mode::Works);
        let legacy = Fake::holding(Mode::Works, "sk-from-last-version");
        assert_eq!(
            read(&primary, some(&legacy)).unwrap().as_deref(),
            Some("sk-from-last-version")
        );
        assert_eq!(primary.value().as_deref(), Some("sk-from-last-version"));
        assert_eq!(legacy.value(), None, "the old copy is gone once moved");
        // And the next read is just a read.
        assert_eq!(
            read(&primary, some(&legacy)).unwrap().as_deref(),
            Some("sk-from-last-version")
        );
    }

    #[test]
    fn a_failed_move_keeps_the_kernel_keyring_copy() {
        let primary = Fake::empty(Mode::WriteFails);
        let legacy = Fake::holding(Mode::Works, "sk-keep-me");
        assert_eq!(
            read(&primary, some(&legacy)).unwrap().as_deref(),
            Some("sk-keep-me"),
            "the value is still returned"
        );
        assert_eq!(legacy.value().as_deref(), Some("sk-keep-me"), "and kept");
        assert_eq!(primary.value(), None);
    }

    /// The bug: with a locked keyring and a dismissed prompt, every slot
    /// that still had a kernel copy tried its own move and raised its own
    /// prompt (the active provider, the key, each model), and startup never
    /// stopped asking. One refusal is the answer for the rest of the
    /// process, and every value is still returned from the kernel copy.
    #[test]
    fn a_refused_move_is_not_tried_again_for_the_rest_of_the_process() {
        let refused = AtomicBool::new(false);
        let gone = AtomicBool::new(false);
        let (key_primary, key_kernel) = (
            Fake::empty(Mode::WriteRefused),
            Fake::holding(Mode::Works, "sk-key"),
        );
        let (model_primary, model_kernel) = (
            Fake::empty(Mode::WriteRefused),
            Fake::holding(Mode::Works, "a-model"),
        );

        assert_eq!(
            read_from(&key_primary, some(&key_kernel), &refused, &gone)
                .unwrap()
                .as_deref(),
            Some("sk-key"),
            "the value is still returned"
        );
        assert_eq!(key_primary.sets.get(), 1, "the first slot asked once");

        assert_eq!(
            read_from(&model_primary, some(&model_kernel), &refused, &gone)
                .unwrap()
                .as_deref(),
            Some("a-model")
        );
        assert_eq!(
            model_primary.sets.get(),
            0,
            "the second slot did not ask again"
        );
        assert!(refused.load(Ordering::Relaxed), "and it is remembered");
        assert_eq!(key_kernel.value().as_deref(), Some("sk-key"), "kept");
        assert_eq!(model_kernel.value().as_deref(), Some("a-model"), "kept");
    }

    /// Only a refusal ends the moves. A move that failed for a reason of
    /// its own raises no prompt, and the next slot may well work.
    #[test]
    fn a_move_that_failed_for_another_reason_does_not_stop_the_next_one() {
        let refused = AtomicBool::new(false);
        let gone = AtomicBool::new(false);
        let broken = Fake::empty(Mode::WriteFails);
        let broken_kernel = Fake::holding(Mode::Works, "sk-a");
        read_from(&broken, some(&broken_kernel), &refused, &gone).unwrap();
        assert!(!refused.load(Ordering::Relaxed));

        let primary = Fake::empty(Mode::Works);
        let kernel = Fake::holding(Mode::Works, "sk-b");
        assert_eq!(
            read_from(&primary, some(&kernel), &refused, &gone)
                .unwrap()
                .as_deref(),
            Some("sk-b")
        );
        assert_eq!(primary.value().as_deref(), Some("sk-b"), "moved");
        assert_eq!(kernel.value(), None);
    }

    /// Reading a value the Secret Service already holds is not a move and
    /// is not affected by an earlier refusal.
    #[test]
    fn a_refused_move_does_not_stop_reads_the_secret_service_answers() {
        let refused = AtomicBool::new(true);
        let gone = AtomicBool::new(false);
        let primary = Fake::holding(Mode::Works, "sk-durable");
        let kernel = Fake::empty(Mode::Works);
        assert_eq!(
            read_from(&primary, some(&kernel), &refused, &gone)
                .unwrap()
                .as_deref(),
            Some("sk-durable")
        );
    }

    // --- a kernel copy next to a durable one is the newer (#497) ---
    //
    // `write_to` drops the kernel copy on every durable save, so one that
    // sits next to a durable copy was written after it: while the Secret
    // Service could not be reached, or by a build that only knew the
    // kernel keyring.

    /// The bug: during a brief outage a new key goes to the kernel keyring.
    /// Once the service is back the read returned the older durable copy
    /// and never looked at the kernel one, so the key the user saved last
    /// was ignored and the one they had replaced was used.
    #[test]
    fn a_key_saved_during_an_outage_wins_over_the_older_durable_one_when_the_service_is_back() {
        let durable = Fake::holding(Mode::Works, "sk-old");
        let kernel = Fake::empty(Mode::Works);

        // The service is down for the save, so it lands in the kernel keyring.
        let down = Fake::empty(Mode::Unreachable);
        assert!(matches!(
            write_to(&down, some(&kernel), "sk-new").unwrap(),
            Persistence::SessionOnly { .. }
        ));

        // Back again, and still holding what it held before.
        assert_eq!(
            read(&durable, some(&kernel)).unwrap().as_deref(),
            Some("sk-new")
        );
        assert_eq!(durable.value().as_deref(), Some("sk-new"), "moved across");
        assert_eq!(kernel.value(), None, "and the kernel copy is gone");
        assert_eq!(
            read(&durable, some(&kernel)).unwrap().as_deref(),
            Some("sk-new"),
            "the next read is just a read"
        );
    }

    /// The same copy twice (a move whose last step, dropping the kernel
    /// copy, did not happen) is not news: nothing is written to the
    /// Secret Service, which on a locked one is an unlock prompt, and the
    /// duplicate goes.
    #[test]
    fn a_kernel_copy_equal_to_the_durable_one_is_dropped_without_a_write() {
        let primary = Fake::holding(Mode::Works, "sk-a");
        let kernel = Fake::holding(Mode::Works, "sk-a");
        assert_eq!(
            read(&primary, some(&kernel)).unwrap().as_deref(),
            Some("sk-a")
        );
        assert_eq!(primary.sets.get(), 0, "nothing was rewritten");
        assert_eq!(kernel.value(), None);
    }

    /// With nothing in the kernel keyring a read is a read, and writes
    /// nothing, as before.
    #[test]
    fn a_durable_value_with_no_kernel_copy_is_read_and_nothing_is_written() {
        let primary = Fake::holding(Mode::Works, "sk-a");
        let kernel = Fake::empty(Mode::Works);
        assert_eq!(
            read(&primary, some(&kernel)).unwrap().as_deref(),
            Some("sk-a")
        );
        assert_eq!(primary.sets.get(), 0);
        assert_eq!(kernel.value(), None);
    }

    /// The newer copy is the answer even when the Secret Service will not
    /// take it: a dismissed unlock prompt. It stays where it is, and the
    /// refusal is remembered so no later slot asks again.
    #[test]
    fn a_newer_kernel_copy_is_returned_and_kept_when_the_move_is_refused() {
        let (refused, gone) = (AtomicBool::new(false), AtomicBool::new(false));
        let primary = Fake::holding(Mode::WriteRefused, "sk-old");
        let kernel = Fake::holding(Mode::Works, "sk-new");
        assert_eq!(
            read_from(&primary, some(&kernel), &refused, &gone)
                .unwrap()
                .as_deref(),
            Some("sk-new")
        );
        assert_eq!(kernel.value().as_deref(), Some("sk-new"), "kept");
        assert_eq!(primary.value().as_deref(), Some("sk-old"), "untouched");
        assert!(refused.load(Ordering::Relaxed), "and remembered");

        let (later, later_kernel) = (
            Fake::holding(Mode::WriteRefused, "a-model"),
            Fake::holding(Mode::Works, "a-newer-model"),
        );
        assert_eq!(
            read_from(&later, some(&later_kernel), &refused, &gone)
                .unwrap()
                .as_deref(),
            Some("a-newer-model"),
            "still the newer one"
        );
        assert_eq!(later.sets.get(), 0, "without asking again");
    }

    /// A move that failed for a reason of its own keeps the kernel copy
    /// too, and stops nothing.
    #[test]
    fn a_newer_kernel_copy_is_returned_and_kept_when_the_move_fails() {
        let (refused, gone) = (AtomicBool::new(false), AtomicBool::new(false));
        let primary = Fake::holding(Mode::WriteFails, "sk-old");
        let kernel = Fake::holding(Mode::Works, "sk-new");
        assert_eq!(
            read_from(&primary, some(&kernel), &refused, &gone)
                .unwrap()
                .as_deref(),
            Some("sk-new")
        );
        assert_eq!(kernel.value().as_deref(), Some("sk-new"));
        assert!(!refused.load(Ordering::Relaxed));
    }

    /// A kernel keyring that cannot be read says nothing about a value the
    /// Secret Service holds: the read does not fail for it.
    #[test]
    fn a_kernel_keyring_that_cannot_be_read_hides_nothing() {
        for mode in [Mode::Unreachable, Mode::Locked, Mode::Garbled] {
            let primary = Fake::holding(Mode::Works, "sk-durable");
            let kernel = Fake::holding(mode, "sk-kernel");
            assert_eq!(
                read(&primary, some(&kernel)).unwrap().as_deref(),
                Some("sk-durable")
            );
            assert_eq!(primary.sets.get(), 0);
        }
    }

    /// A blank kernel copy is not a newer value: it would put an empty key
    /// over a real one.
    #[test]
    fn a_blank_kernel_copy_never_overrules_a_durable_value() {
        let primary = Fake::holding(Mode::Works, "sk-durable");
        let kernel = Fake::holding(Mode::Works, "  ");
        assert_eq!(
            read(&primary, some(&kernel)).unwrap().as_deref(),
            Some("sk-durable")
        );
        assert_eq!(primary.sets.get(), 0);
    }

    /// A store that is locked or garbled answers with an error, and the
    /// kernel copy is not consulted to get round it, as before.
    #[test]
    fn a_secret_service_that_errors_is_not_overruled_by_the_kernel_copy() {
        for mode in [Mode::Locked, Mode::Garbled] {
            let primary = Fake::empty(mode);
            let kernel = Fake::holding(Mode::Works, "sk-kernel");
            assert!(read(&primary, some(&kernel)).is_err());
            assert_eq!(kernel.gets.get(), 0);
            assert_eq!(kernel.value().as_deref(), Some("sk-kernel"));
        }
    }

    // --- write ---

    /// The other half of the rule above: a durable save leaves no kernel
    /// copy behind, so the one that is read back is the one just saved.
    #[test]
    fn a_durable_save_is_what_is_read_back_whatever_the_kernel_held() {
        let primary = Fake::holding(Mode::Works, "sk-older");
        let kernel = Fake::holding(Mode::Works, "sk-stale");
        write_to(&primary, some(&kernel), "sk-new").unwrap();
        assert_eq!(
            read(&primary, some(&kernel)).unwrap().as_deref(),
            Some("sk-new")
        );
    }

    /// If the kernel copy cannot be removed after a durable save, it is
    /// overwritten with the new value instead. Left as it was, the read
    /// rule above would take the stale copy for a newer one and put it
    /// over the key that was just saved.
    #[test]
    fn a_kernel_copy_that_cannot_be_dropped_after_a_save_is_overwritten_not_left_stale() {
        let primary = Fake::holding(Mode::Works, "sk-older");
        let kernel = Fake::holding(Mode::DeleteFails, "sk-stale");
        assert_eq!(
            write_to(&primary, some(&kernel), "sk-new").unwrap(),
            Persistence::Durable
        );
        assert_eq!(kernel.value().as_deref(), Some("sk-new"));
        assert_eq!(
            read(&primary, some(&kernel)).unwrap().as_deref(),
            Some("sk-new")
        );
        assert_eq!(primary.value().as_deref(), Some("sk-new"));
    }

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
        assert_eq!(read(&primary, some(&legacy)).unwrap(), None);
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

    /// The probe looks up an account nothing writes (reading a stored,
    /// locked one raises an unlock prompt), so it must be no slot's name:
    /// a collision would put the prompt back, and a save to that slot
    /// would put a value where the probe finds it.
    #[test]
    fn the_probe_account_collides_with_no_slot() {
        assert_ne!(PROBE_ACCOUNT, ACTIVE_PROVIDER_ACCOUNT);
        // Every per-provider slot is `<provider_id><suffix>`. Whatever the
        // provider id, the probe account is not one of them.
        for suffix in [
            account_for(""),
            base_url_account_for(""),
            model_account_for(""),
            effort_account_for(""),
        ] {
            assert!(suffix.starts_with('_'), "{suffix}");
            assert!(
                !PROBE_ACCOUNT.ends_with(&suffix),
                "{PROBE_ACCOUNT} would be the {suffix} slot of some provider"
            );
        }
        for p in [
            "anthropic",
            "openai",
            "openrouter",
            "groq",
            "gemini",
            "ollama",
        ] {
            for slot in [
                account_for(p),
                base_url_account_for(p),
                model_account_for(p),
                effort_account_for(p),
            ] {
                assert_ne!(PROBE_ACCOUNT, slot);
            }
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

    /// A locked or dismissed store is the one failure that says every
    /// read after it will fail too, which is what startup stops on.
    #[test]
    fn only_a_refused_store_counts_as_access_denied() {
        assert!(
            KeychainError::from(keyring::Error::NoStorageAccess("locked".into()))
                .is_access_denied()
        );
        assert!(KeychainError::access_denied("locked").is_access_denied());
        for other in [
            keyring::Error::PlatformFailure("no secret service".into()),
            keyring::Error::BadEncoding(b"sk-secret".to_vec()),
            keyring::Error::Ambiguous(Vec::new()),
        ] {
            assert!(
                !KeychainError::from(other).is_access_denied(),
                "that is about one slot, not the store"
            );
        }
        assert!(!KeychainError::new("anything").is_access_denied());
    }

    /// A blank stored URL is what `set_base_url_for` would never write, but a
    /// hand edit could; it must read as no override rather than a URL
    /// that fails every request.
    #[test]
    fn a_blank_base_url_means_no_override() {
        assert_eq!(base_url_from(None), None);
        assert_eq!(base_url_from(Some(String::new())), None);
        assert_eq!(base_url_from(Some(" \n".into())), None);
        assert_eq!(
            base_url_from(Some("http://localhost:11434/v1".into())).as_deref(),
            Some("http://localhost:11434/v1")
        );
    }

    #[test]
    fn a_stored_value_that_is_not_a_level_reads_as_unset() {
        assert_eq!(effort_from(None), None);
        assert_eq!(effort_from(Some("ultra".into())), None);
        assert_eq!(effort_from(Some("high".into())), Some(Effort::High));
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

#[cfg(test)]
#[path = "keychain_layout_tests.rs"]
mod layout_tests;
