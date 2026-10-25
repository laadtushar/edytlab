//! What the ChatGPT session keeps, and where (spec §4.3 item 4).
//!
//! | Data | Where | Why |
//! |---|---|---|
//! | host ID, issued client id, issuer, verified `sub`, email, granted scopes, `saved_at` | `<app config dir>/chatgpt/registration.json` ([`RegistrationFile`]): written atomically, `0600` on Unix | Not secret: the client is public and "a host ID is not an authentication credential". It must outlive a reboot even where the keychain does not (Linux with no Secret Service), or each sign-in would register a new client in the user's ChatGPT settings. The email is personal data: it is shown as the account's label and sent as `login_hint`, and kept out of `Debug` output and logs. |
//! | refresh token | [`RefreshTokenStore`]: the desktop app's keychain slot [`REFRESH_TOKEN_SLOT`] | Secret, and replaced on every refresh. A store that refuses it ([`SecretStoreError::TooLong`] or otherwise) leaves the session in memory, and the session says so. |
//! | access token | memory only | One hour long. Never written anywhere. |
//!
//! ID tokens are not kept at all once verified: the session keeps their
//! claims, and a returning sign-in sends `login_hint`, not `id_token_hint`.
//!
//! `registration.json` keeps one record per issued client id, each with the
//! account it was verified for, so that an account picker can be added
//! later; the session uses one active account. A record is written as soon
//! as the callback hands back an issued client id, before the code is
//! exchanged, and gains its account once the ID token verifies. Signing out
//! keeps the record and the host ID, so the next sign-in reuses the client.

use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};

use super::spec::HostId;
use crate::oauth::SecretString;

/// The session's directory under the app's config directory.
pub const DIR_NAME: &str = "chatgpt";
/// The registration, in [`DIR_NAME`].
pub const REGISTRATION_FILE: &str = "registration.json";
/// The file whose OS lock serializes refreshes across processes, in
/// [`DIR_NAME`]. It is never deleted: deleting it would let one process lock
/// the old file while another creates a new one.
pub const LOCK_FILE: &str = "refresh.lock";
/// The keychain slot (account) the desktop app keeps the refresh token in.
pub const REFRESH_TOKEN_SLOT: &str = "chatgpt_refresh_token";

/// The `version` this build writes and reads.
pub const REGISTRATION_VERSION: u32 = 1;

/// Where the refresh token is kept: the OS keychain, in the app. `crates/ai`
/// has no Tauri dependency, so the desktop app implements this over its
/// keychain slot [`REFRESH_TOKEN_SLOT`], and tests implement it in memory.
///
/// The session calls it from a blocking thread, and serializes its own
/// calls; it is shared with other processes only through the store itself,
/// under the session's refresh lock.
///
/// **Each call must return in bounded time.** The session holds its refresh
/// lock, in-process and across processes, while it waits, so a call that
/// sits on an unlock prompt holds up every other edytlab window's refresh.
/// An implementation that would have to wait for the user (a locked
/// keychain) should fail with [`SecretStoreError::Unavailable`] instead, and
/// let the app ask for the unlock outside a refresh. The session does not
/// time the calls out itself: it cannot stop a blocking keychain call, and a
/// `delete` that lands after the session gave up on it could erase a token
/// another window saved in the meantime.
pub trait RefreshTokenStore: Send + Sync {
    /// The saved token, or `None` when there is none.
    fn load(&self) -> Result<Option<SecretString>, SecretStoreError>;
    /// Saves `token` in place of any other.
    fn save(&self, token: &SecretString) -> Result<(), SecretStoreError>;
    /// Removes the token. Removing one that is not there is not an error.
    fn delete(&self) -> Result<(), SecretStoreError>;
}

/// Why the refresh-token store did not do what was asked. It never carries
/// a token: build it from a keychain error's `Display`, never its `Debug`.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SecretStoreError {
    /// The store cannot hold the value (`keyring::Error::TooLong`: Windows'
    /// Credential Manager holds 2,560 bytes in all, and every key and this
    /// token share one credential, #734). The session keeps the
    /// token in memory and says it will not survive a restart.
    #[error(
        "the sign-in is too long for this system's credential store; if other keys are saved, \
         removing one you do not use may make room"
    )]
    TooLong,
    /// The store could not be used: locked, refused, or not there.
    #[error("the system keychain could not be used: {0}")]
    Unavailable(String),
}

impl From<keyring::Error> for SecretStoreError {
    /// From `Display` only: a `keyring::Error`'s `Debug` can carry the
    /// secret's bytes (`BadEncoding`).
    fn from(e: keyring::Error) -> Self {
        match e {
            keyring::Error::TooLong(..) => Self::TooLong,
            other => Self::Unavailable(other.to_string()),
        }
    }
}

/// The non-secret registration: this host's ID and the clients it has
/// registered. Saved as `registration.json`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Registration {
    pub version: u32,
    pub host_id: HostId,
    /// The account in use: one of [`Self::clients`], verified.
    #[serde(default)]
    pub active_client_id: Option<String>,
    #[serde(default)]
    pub clients: Vec<ClientRecord>,
}

impl Registration {
    pub fn new(host_id: HostId) -> Self {
        Self {
            version: REGISTRATION_VERSION,
            host_id,
            active_client_id: None,
            clients: Vec::new(),
        }
    }

    pub fn client(&self, client_id: &str) -> Option<&ClientRecord> {
        self.clients.iter().find(|c| c.client_id == client_id)
    }

    pub fn client_mut(&mut self, client_id: &str) -> Option<&mut ClientRecord> {
        self.clients.iter_mut().find(|c| c.client_id == client_id)
    }

    /// The active account, if it was registered with `issuer`.
    pub fn active(&self, issuer: &str) -> Option<&ClientRecord> {
        let id = self.active_client_id.as_deref()?;
        self.client(id).filter(|c| c.issuer == issuer)
    }

    /// The client the next sign-in with `issuer` should use: the active
    /// account's, else the newest one registered but never verified (a
    /// sign-in whose code exchange failed: "start a fresh authorization with
    /// the issued client ID retained for this attempt"), else none, which
    /// means a new registration. A record from another issuer (a test
    /// issuer, say) is never used.
    pub fn client_for_sign_in(&self, issuer: &str) -> Option<&ClientRecord> {
        self.active(issuer).or_else(|| {
            self.clients
                .iter()
                .filter(|c| c.issuer == issuer && c.subject.is_none())
                .max_by_key(|c| c.saved_at)
        })
    }
}

/// One registered client and the account it was verified for.
#[derive(Clone, Serialize, Deserialize)]
pub struct ClientRecord {
    /// The issued `client_id` (`oaiapp_...`). Never `dynamic_agent_client`.
    pub client_id: String,
    pub issuer: String,
    /// The verified ID token's `sub`; `None` until a code exchange with this
    /// client verifies one.
    #[serde(default)]
    pub subject: Option<String>,
    /// Personal data: a label for the account, and the `login_hint` of a
    /// returning sign-in. Kept out of `Debug` output and logs.
    #[serde(default)]
    pub email: Option<String>,
    /// The scopes the last token response granted.
    #[serde(default)]
    pub scopes: Vec<String>,
    /// The claims of the sign-in's ID token that a refreshed ID token is
    /// compared with (OpenID Connect Core §12.2), so that a session resumed
    /// after a restart checks refreshed ID tokens the way a running one does.
    #[serde(default)]
    pub id_token_reference: IdTokenReference,
    /// Unix seconds: when this record was last written.
    pub saved_at: u64,
    /// The user signed out of this account. The record stays, so the next
    /// sign-in reuses its client; the session reads "signed out" rather than
    /// "session ended" for it.
    #[serde(default)]
    pub signed_out: bool,
}

impl fmt::Debug for ClientRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClientRecord")
            .field("client_id", &self.client_id)
            .field("issuer", &self.issuer)
            .field("subject", &self.subject)
            .field("email", &self.email.as_ref().map(|_| "<redacted>"))
            .field("scopes", &self.scopes)
            .field("saved_at", &self.saved_at)
            .field("signed_out", &self.signed_out)
            .finish_non_exhaustive()
    }
}

/// The parts of the original ID token, beyond `iss`, `sub` and `aud`, that
/// OpenID Connect Core §12.2 compares a refreshed one against: `azp` must be
/// the same (present or absent alike), a `nonce` the original's, and an
/// `auth_time` the original's. The nonce is the spent one of a finished
/// sign-in: it authorizes nothing.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct IdTokenReference {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub azp: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_time: Option<serde_json::Number>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nonce: Option<String>,
}

/// Why `registration.json` could not be read or written.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("could not {action} {path}: {source}")]
    Io {
        action: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    /// Not JSON, or not a registration. The message says no more: the file
    /// holds an email address.
    #[error("{0} is not a valid ChatGPT registration")]
    Corrupt(PathBuf),
    /// Written by a newer build, whose format this one does not know.
    #[error("{path} was written by a newer edytlab (format {version})")]
    UnsupportedVersion { path: PathBuf, version: u32 },
}

/// Where a `registration.json` that does not parse is moved, in the same
/// directory, so it can still be looked at. A later one replaces it.
pub const CORRUPT_REGISTRATION_FILE: &str = "registration.json.corrupt";

/// `registration.json` in the session's directory.
#[derive(Clone, Debug)]
pub struct RegistrationFile {
    dir: PathBuf,
    /// Tests only: every save fails while this is set.
    #[cfg(feature = "test-endpoints")]
    fail_saves: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
}

impl RegistrationFile {
    /// The file in `dir` (`<app config dir>/chatgpt`).
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            #[cfg(feature = "test-endpoints")]
            fail_saves: None,
        }
    }

    /// Tests only (the `test-endpoints` feature): every save fails while
    /// `switch` is set, so a test can see what the session does when
    /// `registration.json` cannot be written.
    #[cfg(feature = "test-endpoints")]
    pub fn with_failing_saves(
        mut self,
        switch: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> Self {
        self.fail_saves = Some(switch);
        self
    }

    pub fn path(&self) -> PathBuf {
        self.dir.join(REGISTRATION_FILE)
    }

    /// Moves a `registration.json` that does not parse to
    /// [`CORRUPT_REGISTRATION_FILE`], replacing an older one there.
    pub fn set_aside(&self) -> Result<PathBuf, StoreError> {
        let to = self.dir.join(CORRUPT_REGISTRATION_FILE);
        fs::rename(self.path(), &to).map_err(|source| StoreError::Io {
            action: "set aside",
            path: self.path(),
            source,
        })?;
        Ok(to)
    }

    /// The saved registration, or `None` when there is no file.
    pub fn load(&self) -> Result<Option<Registration>, StoreError> {
        let path = self.path();
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(source) => {
                return Err(StoreError::Io {
                    action: "read",
                    path,
                    source,
                })
            }
        };
        #[derive(Deserialize)]
        struct Versioned {
            version: u32,
        }
        let version = serde_json::from_slice::<Versioned>(&bytes)
            .map_err(|_| StoreError::Corrupt(path.clone()))?
            .version;
        if version != REGISTRATION_VERSION {
            return Err(StoreError::UnsupportedVersion { path, version });
        }
        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|_| StoreError::Corrupt(path))
    }

    /// Saves `registration`, replacing the file in one step: a new file is
    /// written beside it, flushed to disk and renamed over it, so a reader
    /// (or a crash) never sees half a file. On Unix the directory is created
    /// `0700` and the file is `0600`. On Windows both inherit the ACL of the
    /// per-user config directory.
    pub fn save(&self, registration: &Registration) -> Result<(), StoreError> {
        #[cfg(feature = "test-endpoints")]
        if self
            .fail_saves
            .as_ref()
            .is_some_and(|s| s.load(std::sync::atomic::Ordering::SeqCst))
        {
            return Err(StoreError::Io {
                action: "write",
                path: self.path(),
                source: io::Error::other("a test made this save fail"),
            });
        }
        let bytes =
            serde_json::to_vec_pretty(registration).expect("a registration always serializes");
        write_private_atomically(&self.dir, REGISTRATION_FILE, &bytes)
    }
}

/// Creates `dir` (and its parents) if needed, the last component owner-only
/// on Unix. An existing `dir` must be a real directory, not a symbolic link
/// (which could point the registration and the lock file anywhere), and on
/// Unix one that others can reach into is narrowed to `0700`.
pub(crate) fn ensure_private_dir(dir: &Path) -> io::Result<()> {
    if let Some(parent) = dir.parent() {
        fs::create_dir_all(parent)?;
    }
    #[cfg_attr(not(unix), allow(unused_mut))]
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    match builder.create(dir) {
        Ok(()) => return Ok(()),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e),
    }
    let meta = fs::symlink_metadata(dir)?;
    if meta.file_type().is_symlink() {
        return Err(io::Error::other(format!(
            "{} is a symbolic link; the ChatGPT session keeps its files only in a real directory",
            dir.display()
        )));
    }
    if !meta.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} exists and is not a directory", dir.display()),
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o077 != 0 {
            fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
        }
    }
    Ok(())
}

/// Writes `bytes` to `dir/name` through a temporary file in `dir` that is
/// created new (never an existing file or a symlink planted at its name),
/// owner-only, flushed, then renamed over `name`.
fn write_private_atomically(dir: &Path, name: &str, bytes: &[u8]) -> Result<(), StoreError> {
    let target = dir.join(name);
    let io_error = |action: &'static str, path: &Path| {
        let path = path.to_path_buf();
        move |source: io::Error| StoreError::Io {
            action,
            path,
            source,
        }
    };
    ensure_private_dir(dir).map_err(io_error("create", dir))?;

    let mut suffix = [0u8; 12];
    SystemRandom::new()
        .fill(&mut suffix)
        .map_err(|_| StoreError::Io {
            action: "name a temporary file for",
            path: target.clone(),
            source: io::Error::other("the OS random generator failed"),
        })?;
    let temp = dir.join(format!(".{name}.{}.tmp", URL_SAFE_NO_PAD.encode(suffix)));

    let written = (|| -> io::Result<()> {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temp)?;
        // The umask cannot widen 0600, but it can narrow it; set it exactly.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(fs::Permissions::from_mode(0o600))?;
        }
        file.write_all(bytes)?;
        file.sync_all()
    })();
    if let Err(source) = written {
        let _ = fs::remove_file(&temp);
        return Err(StoreError::Io {
            action: "write",
            path: temp,
            source,
        });
    }
    if let Err(source) = fs::rename(&temp, &target) {
        let _ = fs::remove_file(&temp);
        return Err(StoreError::Io {
            action: "replace",
            path: target,
            source,
        });
    }
    // The rename itself reaches the disk with the directory.
    #[cfg(unix)]
    if let Ok(d) = fs::File::open(dir) {
        let _ = d.sync_all();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(client_id: &str, subject: Option<&str>, saved_at: u64) -> ClientRecord {
        ClientRecord {
            client_id: client_id.into(),
            issuer: "https://auth.openai.com".into(),
            subject: subject.map(str::to_string),
            email: Some("person@example.com".into()),
            scopes: vec!["openid".into()],
            id_token_reference: IdTokenReference::default(),
            saved_at,
            signed_out: false,
        }
    }

    #[test]
    fn the_sign_in_client_is_the_active_one_else_the_newest_unverified() {
        let issuer = "https://auth.openai.com";
        let mut r = Registration::new(HostId::generate().unwrap());
        assert!(r.client_for_sign_in(issuer).is_none(), "nothing: register");

        r.clients.push(record("oaiapp_old", None, 1));
        r.clients.push(record("oaiapp_new", None, 2));
        r.clients.push(record("oaiapp_verified", Some("sub"), 3));
        assert_eq!(
            r.client_for_sign_in(issuer).unwrap().client_id,
            "oaiapp_new"
        );

        r.active_client_id = Some("oaiapp_verified".into());
        assert_eq!(
            r.client_for_sign_in(issuer).unwrap().client_id,
            "oaiapp_verified"
        );
        // Another issuer's records are never used.
        assert!(r.client_for_sign_in("http://127.0.0.1:9").is_none());
        assert!(r.active("http://127.0.0.1:9").is_none());
    }

    #[test]
    fn debug_output_leaves_the_email_out() {
        let mut r = Registration::new(HostId::generate().unwrap());
        r.clients.push(record("oaiapp_1", Some("sub"), 1));
        let debug = format!("{r:?} {r:#?}");
        assert!(!debug.contains("person@example.com"), "{debug}");
        assert!(debug.contains("oaiapp_1"), "{debug}");
    }

    #[test]
    fn registration_json_round_trips_and_holds_no_secret_fields() {
        let dir = tempfile::tempdir().unwrap();
        let file = RegistrationFile::new(dir.path().join(DIR_NAME));
        assert!(file.load().unwrap().is_none(), "no file is no registration");
        let mut r = Registration::new(HostId::generate().unwrap());
        r.clients.push(record("oaiapp_1", Some("sub-1"), 7));
        r.active_client_id = Some("oaiapp_1".into());
        file.save(&r).unwrap();

        let loaded = file.load().unwrap().unwrap();
        assert_eq!(loaded.host_id, r.host_id);
        assert_eq!(loaded.active_client_id.as_deref(), Some("oaiapp_1"));
        assert_eq!(loaded.clients[0].subject.as_deref(), Some("sub-1"));
        assert_eq!(
            loaded.clients[0].email.as_deref(),
            Some("person@example.com")
        );

        let text = fs::read_to_string(file.path()).unwrap();
        let json: serde_json::Value = serde_json::from_str(&text).unwrap();
        let mut keys = Vec::new();
        fn walk(v: &serde_json::Value, keys: &mut Vec<String>) {
            match v {
                serde_json::Value::Object(m) => {
                    for (k, v) in m {
                        keys.push(k.clone());
                        walk(v, keys);
                    }
                }
                serde_json::Value::Array(a) => a.iter().for_each(|v| walk(v, keys)),
                _ => {}
            }
        }
        walk(&json, &mut keys);
        for key in &keys {
            assert!(
                !key.contains("token") || key == "id_token_reference",
                "a token field in registration.json: {key}"
            );
        }
    }

    #[test]
    #[cfg(unix)]
    fn registration_json_is_owner_only_and_replaced_in_one_step() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let parent = tempfile::tempdir().unwrap();
        let dir = parent.path().join("config").join(DIR_NAME);
        let file = RegistrationFile::new(&dir);
        let r = Registration::new(HostId::generate().unwrap());
        file.save(&r).unwrap();

        let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&dir), 0o700, "the directory");
        assert_eq!(mode(&file.path()), 0o600, "the file");

        // A file someone loosened, or left by an older build, is replaced
        // by a new 0600 file, not rewritten in place: a reader (or a crash)
        // never sees half of one.
        fs::set_permissions(file.path(), fs::Permissions::from_mode(0o644)).unwrap();
        let before = fs::metadata(file.path()).unwrap().ino();
        file.save(&r).unwrap();
        let after = fs::metadata(file.path()).unwrap().ino();
        assert_ne!(before, after, "renamed over, not truncated and rewritten");
        assert_eq!(mode(&file.path()), 0o600);

        // No temporary file is left behind.
        let names: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(names, [REGISTRATION_FILE], "{names:?}");
    }

    #[test]
    #[cfg(unix)]
    fn an_existing_directory_is_narrowed_and_a_symlinked_one_refused() {
        use std::os::unix::fs::PermissionsExt;
        let parent = tempfile::tempdir().unwrap();
        let dir = parent.path().join(DIR_NAME);
        fs::create_dir(&dir).unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
        RegistrationFile::new(&dir)
            .save(&Registration::new(HostId::generate().unwrap()))
            .unwrap();
        let mode = fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700, "an existing 0755 directory is narrowed");

        // A symlink in its place could send the files anywhere: refused.
        let elsewhere = parent.path().join("elsewhere");
        fs::create_dir(&elsewhere).unwrap();
        let linked = parent.path().join("linked");
        std::os::unix::fs::symlink(&elsewhere, &linked).unwrap();
        let err = RegistrationFile::new(&linked)
            .save(&Registration::new(HostId::generate().unwrap()))
            .unwrap_err();
        assert!(err.to_string().contains("symbolic link"), "{err}");
        assert_eq!(
            fs::read_dir(&elsewhere).unwrap().count(),
            0,
            "nothing written there"
        );
    }

    #[test]
    fn a_corrupt_registration_is_set_aside() {
        let dir = tempfile::tempdir().unwrap();
        let file = RegistrationFile::new(dir.path());
        fs::write(file.path(), b"{ broken").unwrap();
        let to = file.set_aside().unwrap();
        assert_eq!(to, dir.path().join(CORRUPT_REGISTRATION_FILE));
        assert_eq!(fs::read(&to).unwrap(), b"{ broken");
        assert!(file.load().unwrap().is_none());
        // A second one replaces the first.
        fs::write(file.path(), b"{ broken again").unwrap();
        file.set_aside().unwrap();
        assert_eq!(fs::read(&to).unwrap(), b"{ broken again");
    }

    #[test]
    fn a_newer_or_broken_registration_is_reported_not_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let file = RegistrationFile::new(dir.path());
        fs::write(file.path(), br#"{"version": 2}"#).unwrap();
        assert!(matches!(
            file.load(),
            Err(StoreError::UnsupportedVersion { version: 2, .. })
        ));
        for broken in [
            &b"{"[..],
            br#"{"version": 1}"#,
            br#"{"version": 1, "host_id": "me@example.com"}"#,
        ] {
            fs::write(file.path(), broken).unwrap();
            let err = file.load().unwrap_err();
            assert!(matches!(err, StoreError::Corrupt(_)), "{err:?}");
            assert!(!err.to_string().contains("example.com"), "{err}");
        }
    }

    #[test]
    fn a_keychain_too_long_error_is_its_own_kind() {
        assert_eq!(
            SecretStoreError::from(keyring::Error::TooLong("password".into(), 2560)),
            SecretStoreError::TooLong
        );
        assert!(matches!(
            SecretStoreError::from(keyring::Error::NoStorageAccess(Box::new(io::Error::other(
                "locked"
            )))),
            SecretStoreError::Unavailable(_)
        ));
        // `BadEncoding` holds the stored bytes; only `Display` is kept.
        let e = SecretStoreError::from(keyring::Error::BadEncoding(b"RT-secret".to_vec()));
        assert!(!format!("{e:?} {e}").contains("RT-secret"), "{e:?}");
    }
}
