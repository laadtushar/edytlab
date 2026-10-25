//! The two homes of `keychain.rs` (#734), against a fake keychain that counts
//! every call and a settings folder of its own. No OS keychain is involved.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use super::*;

// ---------------------------------------------------------------------------
// The fake keychain.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Get,
    Set,
    Delete,
}

/// What the primary store (the login keychain, the Secret Service) holds and
/// every call made to it. With `with_kernel` there is a kernel keyring beside
/// it, as on Linux; its calls are not counted, because they raise no prompt.
#[derive(Default)]
struct Backend {
    items: Mutex<BTreeMap<String, String>>,
    kernel: Mutex<BTreeMap<String, String>>,
    with_kernel: bool,
    calls: Mutex<Vec<(Op, String)>>,
    /// Accounts whose reads are refused (a dismissed prompt).
    refuse_reads: Mutex<BTreeSet<String>>,
    /// Accounts whose writes are refused.
    refuse_writes: Mutex<BTreeSet<String>>,
    /// Accounts the store cannot reach at all (no Secret Service).
    down: Mutex<BTreeSet<String>>,
    down_all: AtomicBool,
    /// A credential size cap on the `secrets` item, as Windows has: `set` (a
    /// password, stored as UTF-16) counts two bytes a character, `set_secret`
    /// counts the bytes.
    cap: Mutex<Option<usize>>,
}

impl Backend {
    fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    fn linux() -> Arc<Self> {
        Arc::new(Self {
            with_kernel: true,
            ..Self::default()
        })
    }

    fn seed(&self, account: &str, value: &str) {
        self.items
            .lock()
            .unwrap()
            .insert(account.to_string(), value.to_string());
    }

    fn item(&self, account: &str) -> Option<String> {
        self.items.lock().unwrap().get(account).cloned()
    }

    fn count(&self, op: Op, account: &str) -> usize {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(o, a)| *o == op && a == account)
            .count()
    }

    fn total_calls(&self) -> usize {
        self.calls.lock().unwrap().len()
    }

    fn forget_calls(&self) {
        self.calls.lock().unwrap().clear();
    }

    fn secrets(&self) -> SecretMap {
        parse_secrets(&self.item(SECRETS_ACCOUNT).expect("a secrets item")).unwrap()
    }

    fn is_down(&self, account: &str) -> bool {
        self.down_all.load(Ordering::Relaxed) || self.down.lock().unwrap().contains(account)
    }
}

struct FakeSlot {
    backend: Arc<Backend>,
    account: String,
    kernel: bool,
}

impl FakeSlot {
    fn log(&self, op: Op) {
        if !self.kernel {
            self.backend
                .calls
                .lock()
                .unwrap()
                .push((op, self.account.clone()));
        }
    }
}

impl Slot for FakeSlot {
    fn get(&self) -> keyring::Result<String> {
        self.get_inner()
    }

    fn set(&self, value: &str) -> keyring::Result<()> {
        self.set_sized(value, value.encode_utf16().count() * 2)
    }

    fn get_secret(&self) -> keyring::Result<Vec<u8>> {
        self.get_inner().map(String::into_bytes)
    }

    fn set_secret(&self, value: &[u8]) -> keyring::Result<()> {
        self.set_sized(std::str::from_utf8(value).unwrap(), value.len())
    }

    fn delete(&self) -> keyring::Result<()> {
        self.log(Op::Delete);
        let map = if self.kernel {
            &self.backend.kernel
        } else {
            if self.backend.is_down(&self.account) {
                return Err(keyring::Error::PlatformFailure("no secret service".into()));
            }
            &self.backend.items
        };
        map.lock()
            .unwrap()
            .remove(&self.account)
            .map(|_| ())
            .ok_or(keyring::Error::NoEntry)
    }
}

impl FakeSlot {
    fn get_inner(&self) -> keyring::Result<String> {
        self.log(Op::Get);
        if self.kernel {
            return self
                .backend
                .kernel
                .lock()
                .unwrap()
                .get(&self.account)
                .cloned()
                .ok_or(keyring::Error::NoEntry);
        }
        if self.backend.is_down(&self.account) {
            return Err(keyring::Error::PlatformFailure("no secret service".into()));
        }
        if self
            .backend
            .refuse_reads
            .lock()
            .unwrap()
            .contains(&self.account)
        {
            return Err(keyring::Error::NoStorageAccess("denied".into()));
        }
        self.backend
            .items
            .lock()
            .unwrap()
            .get(&self.account)
            .cloned()
            .ok_or(keyring::Error::NoEntry)
    }

    fn set_sized(&self, value: &str, size: usize) -> keyring::Result<()> {
        self.log(Op::Set);
        if !self.kernel && self.account == SECRETS_ACCOUNT {
            if let Some(cap) = *self.backend.cap.lock().unwrap() {
                if size > cap {
                    return Err(keyring::Error::TooLong("secret".into(), cap as u32));
                }
            }
        }
        if self.kernel {
            self.backend
                .kernel
                .lock()
                .unwrap()
                .insert(self.account.clone(), value.to_string());
            return Ok(());
        }
        if self.backend.is_down(&self.account) {
            return Err(keyring::Error::PlatformFailure("no secret service".into()));
        }
        if self
            .backend
            .refuse_writes
            .lock()
            .unwrap()
            .contains(&self.account)
        {
            return Err(keyring::Error::NoStorageAccess("denied".into()));
        }
        self.backend
            .items
            .lock()
            .unwrap()
            .insert(self.account.clone(), value.to_string());
        Ok(())
    }
}

/// A "launch": a store with nothing cached, over the same keychain and folder.
fn launch(backend: &Arc<Backend>, dir: &Path) -> Store {
    let backend = Arc::clone(backend);
    Store::new(
        DirSource::Fixed(dir.to_path_buf()),
        Box::new(move |account| {
            let slot = |kernel| -> Box<dyn Slot> {
                Box::new(FakeSlot {
                    backend: Arc::clone(&backend),
                    account: account.to_string(),
                    kernel,
                })
            };
            Ok((slot(false), backend.with_kernel.then(|| slot(true))))
        }),
    )
}

fn settings_text(dir: &Path) -> String {
    std::fs::read_to_string(dir.join(SETTINGS_FILE)).expect("the settings file")
}

fn settings_json(dir: &Path) -> serde_json::Value {
    serde_json::from_str(&settings_text(dir)).unwrap()
}

/// The per-slot items of a build from before this change: keys for three
/// providers, models, a base URL, an effort, the active provider, the ChatGPT
/// token and its model.
const LEGACY: &[(&str, &str)] = &[
    ("anthropic_api_key", "key-anthropic"),
    ("openai_api_key", "key-openai"),
    ("groq_api_key", "key-groq"),
    ("anthropic_model", "claude-a-model"),
    ("openai_model", "gpt-a-model"),
    ("ollama_base_url", "http://localhost:11434/v1"),
    ("anthropic_effort", "high"),
    ("chatgpt_refresh_token", "refresh-token-1"),
    ("chatgpt_model", "plan-model"),
    ("active_provider", "openai"),
];

fn seed_legacy(backend: &Backend) {
    for (account, value) in LEGACY {
        backend.seed(account, value);
    }
}

/// Everything a launch reads, as the public functions would answer it.
fn everything(store: &Store) -> Vec<(&'static str, Option<String>)> {
    let get = |key| store.get(key).unwrap();
    vec![
        ("active", active_provider_from(get(Field::ActiveProvider))),
        ("anthropic key", get(Field::ApiKey("anthropic"))),
        ("openai key", get(Field::ApiKey("openai"))),
        ("groq key", get(Field::ApiKey("groq"))),
        ("gemini key", get(Field::ApiKey("gemini"))),
        ("anthropic model", get(Field::Model("anthropic"))),
        ("openai model", get(Field::Model("openai"))),
        ("chatgpt model", get(Field::Model("chatgpt"))),
        ("ollama url", base_url_from(get(Field::BaseUrl("ollama")))),
        ("anthropic url", get(Field::BaseUrl("anthropic"))),
        (
            "anthropic effort",
            effort_from(get(Field::Effort("anthropic"))).map(|e| e.as_str().to_string()),
        ),
        ("openai effort", get(Field::Effort("openai"))),
        ("chatgpt token", get(Field::ChatGptToken)),
    ]
}

fn expected_after_migration() -> Vec<(&'static str, Option<String>)> {
    let s = |v: &str| Some(v.to_string());
    vec![
        ("active", s("openai")),
        ("anthropic key", s("key-anthropic")),
        ("openai key", s("key-openai")),
        ("groq key", s("key-groq")),
        ("gemini key", None),
        ("anthropic model", s("claude-a-model")),
        ("openai model", s("gpt-a-model")),
        ("chatgpt model", s("plan-model")),
        ("ollama url", s("http://localhost:11434/v1")),
        ("anthropic url", None),
        ("anthropic effort", s("high")),
        ("openai effort", None),
        ("chatgpt token", s("refresh-token-1")),
    ]
}

/// A store that has finished the migration with nothing to move.
fn installed(backend: &Arc<Backend>, dir: &Path) {
    let store = launch(backend, dir);
    store
        .put(Field::ApiKey("anthropic"), "key-anthropic")
        .unwrap();
    store.put(Field::ChatGptToken, "refresh-token-1").unwrap();
    store.put(Field::ActiveProvider, "anthropic").unwrap();
    store
        .put(Field::Model("anthropic"), "claude-a-model")
        .unwrap();
    store
        .put(Field::BaseUrl("ollama"), "http://localhost:11434/v1")
        .unwrap();
    store.put(Field::Effort("anthropic"), "high").unwrap();
}

// ---------------------------------------------------------------------------
// A fresh layout: what a launch costs.
// ---------------------------------------------------------------------------

/// The point of the change: a launch reads the one item, once, however many
/// values it asks for, and asks the keychain for nothing else.
#[test]
fn a_launch_makes_exactly_one_read_of_the_secrets_item() {
    let (backend, dir) = (Backend::new(), tempfile::tempdir().unwrap());
    installed(&backend, dir.path());
    backend.forget_calls();

    let store = launch(&backend, dir.path());
    // What startup, the agent build and the ChatGPT restore read.
    assert_eq!(
        store.get(Field::ActiveProvider).unwrap().as_deref(),
        Some("anthropic")
    );
    assert_eq!(
        store.get(Field::ApiKey("anthropic")).unwrap().as_deref(),
        Some("key-anthropic")
    );
    assert_eq!(
        store.get(Field::Model("anthropic")).unwrap().as_deref(),
        Some("claude-a-model")
    );
    assert_eq!(
        store.get(Field::ApiKey("anthropic")).unwrap().as_deref(),
        Some("key-anthropic")
    );
    assert_eq!(
        store.get(Field::BaseUrl("ollama")).unwrap().as_deref(),
        Some("http://localhost:11434/v1")
    );
    assert_eq!(
        store.get(Field::Effort("anthropic")).unwrap().as_deref(),
        Some("high")
    );
    assert_eq!(
        store.get(Field::ChatGptToken).unwrap().as_deref(),
        Some("refresh-token-1")
    );
    assert_eq!(store.get(Field::ApiKey("gemini")).unwrap(), None);

    assert_eq!(backend.count(Op::Get, SECRETS_ACCOUNT), 1);
    assert_eq!(backend.total_calls(), 1, "no other keychain call at all");
}

/// A change to the provider, a model, an effort or a base URL is a write to
/// a file: no keychain call, not even a read.
#[test]
fn saving_a_setting_makes_no_keychain_call() {
    let (backend, dir) = (Backend::new(), tempfile::tempdir().unwrap());
    installed(&backend, dir.path());
    backend.forget_calls();

    let store = launch(&backend, dir.path());
    assert_eq!(
        store.put(Field::ActiveProvider, "openrouter").unwrap(),
        Persistence::Durable
    );
    assert_eq!(
        store.put(Field::Model("openrouter"), "some/model").unwrap(),
        Persistence::Durable
    );
    assert_eq!(
        store
            .put(Field::BaseUrl("ollama"), "http://other:1/v1")
            .unwrap(),
        Persistence::Durable
    );
    assert_eq!(
        store.put(Field::Effort("openrouter"), "low").unwrap(),
        Persistence::Durable
    );
    store.remove(Field::BaseUrl("ollama")).unwrap();
    store.remove(Field::Effort("openrouter")).unwrap();
    store.remove(Field::Model("never-set")).unwrap();

    assert_eq!(backend.total_calls(), 0);
    assert_eq!(
        store.get(Field::ActiveProvider).unwrap().as_deref(),
        Some("openrouter")
    );
    assert_eq!(
        store.get(Field::Model("openrouter")).unwrap().as_deref(),
        Some("some/model")
    );
    assert_eq!(store.get(Field::BaseUrl("ollama")).unwrap(), None);
    assert_eq!(backend.total_calls(), 0, "reads of settings are free too");

    // And they are on disk for the next launch.
    let again = launch(&backend, dir.path());
    assert_eq!(
        again.get(Field::Model("openrouter")).unwrap().as_deref(),
        Some("some/model")
    );
    assert_eq!(again.get(Field::Effort("openrouter")).unwrap(), None);
    assert_eq!(backend.total_calls(), 0);
}

#[test]
fn a_first_launch_with_nothing_stored_writes_the_settings_file_and_no_secrets_item() {
    let (backend, dir) = (Backend::new(), tempfile::tempdir().unwrap());
    let store = launch(&backend, dir.path());
    assert_eq!(store.get(Field::ApiKey("anthropic")).unwrap(), None);
    assert_eq!(store.get(Field::ActiveProvider).unwrap(), None);

    assert_eq!(
        settings_json(dir.path()),
        serde_json::json!({ "version": 1 })
    );
    assert_eq!(
        backend.item(SECRETS_ACCOUNT),
        None,
        "nothing to keep, nothing created"
    );
    assert_eq!(backend.count(Op::Set, SECRETS_ACCOUNT), 0);
    assert_eq!(backend.count(Op::Delete, SECRETS_ACCOUNT), 0);

    // The old slots are not scanned again by the next launch.
    backend.forget_calls();
    let again = launch(&backend, dir.path());
    assert_eq!(again.get(Field::ActiveProvider).unwrap(), None);
    assert_eq!(backend.total_calls(), 0);
}

// ---------------------------------------------------------------------------
// The layout on disk and in the keychain.
// ---------------------------------------------------------------------------

#[test]
fn the_secrets_item_and_the_settings_file_have_the_documented_shape() {
    let (backend, dir) = (Backend::new(), tempfile::tempdir().unwrap());
    installed(&backend, dir.path());
    let store = launch(&backend, dir.path());
    store.put(Field::ApiKey("openai"), "key-openai").unwrap();
    store.put(Field::Model("openai"), "gpt-a-model").unwrap();

    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&backend.item(SECRETS_ACCOUNT).unwrap()).unwrap(),
        serde_json::json!({
            "anthropic_api_key": "key-anthropic",
            "chatgpt_refresh_token": "refresh-token-1",
            "openai_api_key": "key-openai",
        })
    );
    assert_eq!(
        settings_json(dir.path()),
        serde_json::json!({
            "version": 1,
            "active_provider": "anthropic",
            "providers": {
                "anthropic": { "model": "claude-a-model", "effort": "high" },
                "ollama": { "base_url": "http://localhost:11434/v1" },
                "openai": { "model": "gpt-a-model" },
            }
        })
    );
    // The file is never the place for a secret, and no setting is an item.
    let text = settings_text(dir.path());
    for secret in ["key-anthropic", "key-openai", "refresh-token-1"] {
        assert!(
            !text.contains(secret),
            "{secret} leaked into the settings file"
        );
    }
    let items: Vec<_> = backend.items.lock().unwrap().keys().cloned().collect();
    assert_eq!(items, [SECRETS_ACCOUNT], "one item, and nothing per slot");
}

#[test]
fn the_settings_file_is_replaced_whole_and_leaves_no_temporary_file() {
    let (backend, dir) = (Backend::new(), tempfile::tempdir().unwrap());
    let store = launch(&backend, dir.path());
    for i in 0..5 {
        store
            .put(Field::Model("anthropic"), &format!("model-{i}"))
            .unwrap();
    }
    let names: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, [SETTINGS_FILE]);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = std::fs::metadata(dir.path().join(SETTINGS_FILE))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o077, 0, "owner only");
    }
}

#[test]
fn fields_a_newer_build_wrote_survive_a_save() {
    let (backend, dir) = (Backend::new(), tempfile::tempdir().unwrap());
    std::fs::write(
        dir.path().join(SETTINGS_FILE),
        r#"{"version":2,"future":{"a":1},"providers":{"x":{"model":"m","colour":"red"}}}"#,
    )
    .unwrap();
    let store = launch(&backend, dir.path());
    store.put(Field::Model("x"), "m2").unwrap();
    store.put(Field::Effort("y"), "low").unwrap();
    store.remove(Field::Effort("y")).unwrap();
    assert_eq!(
        settings_json(dir.path()),
        serde_json::json!({
            "version": 2,
            "future": { "a": 1 },
            "providers": { "x": { "model": "m2", "colour": "red" } }
        })
    );
    assert_eq!(backend.total_calls(), 0);
}

#[test]
fn a_corrupt_settings_file_is_set_aside_and_the_keys_stay() {
    let (backend, dir) = (Backend::new(), tempfile::tempdir().unwrap());
    backend.seed(SECRETS_ACCOUNT, r#"{"openai_api_key":"key-openai"}"#);
    std::fs::write(dir.path().join(SETTINGS_FILE), "{not json").unwrap();

    let store = launch(&backend, dir.path());
    assert_eq!(
        store.get(Field::ApiKey("openai")).unwrap().as_deref(),
        Some("key-openai")
    );
    assert_eq!(store.get(Field::Model("openai")).unwrap(), None);
    assert_eq!(
        std::fs::read_to_string(dir.path().join(format!("{SETTINGS_FILE}.bad"))).unwrap(),
        "{not json"
    );
    assert_eq!(
        settings_json(dir.path()),
        serde_json::json!({ "version": 1 })
    );
    assert_eq!(
        backend.item(SECRETS_ACCOUNT).as_deref(),
        Some(r#"{"openai_api_key":"key-openai"}"#)
    );
}

// ---------------------------------------------------------------------------
// Migration.
// ---------------------------------------------------------------------------

#[test]
fn migrating_a_legacy_layout_keeps_every_value_and_removes_the_old_items() {
    let (backend, dir) = (Backend::new(), tempfile::tempdir().unwrap());
    seed_legacy(&backend);
    // Not slots the migration may carry over: a key for the ChatGPT plan
    // (nothing may write one), a blank model and an effort that is no level.
    backend.seed("chatgpt_api_key", "stray");
    backend.seed("gemini_model", "  ");
    backend.seed("openai_effort", "ultra");

    let store = launch(&backend, dir.path());
    assert_eq!(everything(&store), expected_after_migration());

    // Where it all went.
    assert_eq!(
        backend.secrets(),
        SecretMap::from([
            ("anthropic_api_key".to_string(), "key-anthropic".to_string()),
            ("openai_api_key".to_string(), "key-openai".to_string()),
            ("groq_api_key".to_string(), "key-groq".to_string()),
            (
                "chatgpt_refresh_token".to_string(),
                "refresh-token-1".to_string()
            ),
            (MIGRATED_MARKER.to_string(), "1".to_string()),
        ])
    );
    let text = settings_text(dir.path());
    for secret in [
        "key-anthropic",
        "key-openai",
        "key-groq",
        "refresh-token-1",
        "stray",
    ] {
        assert!(
            !text.contains(secret),
            "{secret} leaked into the settings file"
        );
    }
    // The old items are gone, the ones it does not own are left alone.
    let left: Vec<_> = backend.items.lock().unwrap().keys().cloned().collect();
    assert_eq!(
        left,
        ["chatgpt_api_key", SECRETS_ACCOUNT],
        "the new item, and the one slot the migration does not read"
    );

    // The next launch reads one item and sees the same.
    backend.forget_calls();
    let next = launch(&backend, dir.path());
    assert_eq!(everything(&next), expected_after_migration());
    assert_eq!(backend.count(Op::Get, SECRETS_ACCOUNT), 1);
    assert_eq!(backend.total_calls(), 1);
}

/// The old items are deleted only after the new ones are written and read back.
#[test]
fn the_old_items_are_deleted_after_the_new_ones_are_written() {
    let (backend, dir) = (Backend::new(), tempfile::tempdir().unwrap());
    seed_legacy(&backend);
    let store = launch(&backend, dir.path());
    store.get(Field::ActiveProvider).unwrap();

    let calls = backend.calls.lock().unwrap().clone();
    let set = calls
        .iter()
        .position(|c| *c == (Op::Set, SECRETS_ACCOUNT.to_string()))
        .unwrap();
    let read_back = calls
        .iter()
        .rposition(|c| *c == (Op::Get, SECRETS_ACCOUNT.to_string()))
        .unwrap();
    let first_delete = calls.iter().position(|(op, _)| *op == Op::Delete).unwrap();
    assert!(set < read_back && read_back < first_delete, "{calls:?}");
    assert!(dir.path().join(SETTINGS_FILE).exists());
}

#[test]
fn a_migration_that_cannot_write_the_item_keeps_the_old_items_and_serves_their_values() {
    let (backend, dir) = (Backend::new(), tempfile::tempdir().unwrap());
    seed_legacy(&backend);
    backend
        .refuse_writes
        .lock()
        .unwrap()
        .insert(SECRETS_ACCOUNT.to_string());

    let store = launch(&backend, dir.path());
    assert_eq!(
        everything(&store),
        expected_after_migration(),
        "the values read are served"
    );
    for (account, value) in LEGACY {
        assert_eq!(
            backend.item(account).as_deref(),
            Some(*value),
            "{account} kept"
        );
    }
    assert_eq!(backend.item(SECRETS_ACCOUNT), None);
    assert!(
        !dir.path().join(SETTINGS_FILE).exists(),
        "no marker, so it is tried again"
    );

    // The process goes on as the old build did: a save lands in its own item.
    store
        .put(Field::Model("anthropic"), "claude-b-model")
        .unwrap();
    store.put(Field::ApiKey("groq"), "key-groq-2").unwrap();
    store.remove(Field::Effort("anthropic")).unwrap();
    assert_eq!(
        backend.item("anthropic_model").as_deref(),
        Some("claude-b-model")
    );
    assert_eq!(backend.item("groq_api_key").as_deref(), Some("key-groq-2"));
    assert_eq!(backend.item("anthropic_effort"), None);
    assert!(!dir.path().join(SETTINGS_FILE).exists());

    // The next launch, with the keychain working, finishes it with the newer values.
    backend.refuse_writes.lock().unwrap().clear();
    let next = launch(&backend, dir.path());
    let all = everything(&next);
    let find = |name: &str| all.iter().find(|(n, _)| *n == name).unwrap().1.clone();
    assert_eq!(find("anthropic model").as_deref(), Some("claude-b-model"));
    assert_eq!(find("groq key").as_deref(), Some("key-groq-2"));
    assert_eq!(find("anthropic effort"), None);
    assert_eq!(find("openai key").as_deref(), Some("key-openai"));
    assert_eq!(find("chatgpt token").as_deref(), Some("refresh-token-1"));
    assert_eq!(
        backend.secrets().len(),
        5,
        "four secrets and the migrated marker"
    );
    assert!(!backend
        .items
        .lock()
        .unwrap()
        .contains_key("anthropic_model"));
    assert!(dir.path().join(SETTINGS_FILE).exists());
}

#[test]
fn a_migration_that_cannot_write_the_settings_file_keeps_the_old_items_and_drops_the_new_one() {
    let (backend, dir) = (Backend::new(), tempfile::tempdir().unwrap());
    seed_legacy(&backend);
    // A folder where the temporary file goes: the file cannot be written, but
    // a missing settings file can be told from an unreadable one.
    std::fs::create_dir(
        dir.path()
            .join(format!("{SETTINGS_FILE}.tmp-{}", std::process::id())),
    )
    .unwrap();

    let store = launch(&backend, dir.path());
    assert_eq!(everything(&store), expected_after_migration());
    for (account, value) in LEGACY {
        assert_eq!(
            backend.item(account).as_deref(),
            Some(*value),
            "{account} kept"
        );
    }
    assert_eq!(
        backend.item(SECRETS_ACCOUNT),
        None,
        "the new item was taken back"
    );
    assert!(!dir.path().join(SETTINGS_FILE).exists());
    // A save lands in the old item, not in a file that would hide the rest.
    store.put(Field::ActiveProvider, "groq").unwrap();
    assert_eq!(backend.item("active_provider").as_deref(), Some("groq"));
}

/// Before the migration has run, too: the old items are left alone and nothing
/// is read from them as if the migration had not happened (finding 3).
#[test]
fn a_settings_file_that_cannot_be_read_leaves_the_old_items_alone() {
    let (backend, dir) = (Backend::new(), tempfile::tempdir().unwrap());
    seed_legacy(&backend);
    std::fs::create_dir(dir.path().join(SETTINGS_FILE)).unwrap();
    let store = launch(&backend, dir.path());
    assert!(store.get(Field::ApiKey("anthropic")).is_err());
    assert!(store.put(Field::ApiKey("anthropic"), "key-new").is_err());
    assert_eq!(backend.item(SECRETS_ACCOUNT), None);
    assert_eq!(
        backend.item("anthropic_api_key").as_deref(),
        Some("key-anthropic")
    );
    assert_eq!(backend.items.lock().unwrap().len(), LEGACY.len());
}

/// A keychain that refuses the first read is the caller's answer, as before,
/// and stops the migration: no read after the refusal, no prompt after it.
#[test]
fn a_refused_read_is_the_answer_and_the_migration_is_asked_for_again_later() {
    let (backend, dir) = (Backend::new(), tempfile::tempdir().unwrap());
    seed_legacy(&backend);
    backend
        .refuse_reads
        .lock()
        .unwrap()
        .insert("active_provider".to_string());

    let store = launch(&backend, dir.path());
    let err = store.get(Field::ActiveProvider).expect_err("refused");
    assert!(err.is_access_denied());
    assert_eq!(
        backend.total_calls(),
        1,
        "nothing was read after the refusal"
    );
    let err = store
        .put(Field::Model("anthropic"), "m")
        .expect_err("refused");
    assert!(matches!(err, keyring::Error::NoStorageAccess(_)));
    assert!(!dir.path().join(SETTINGS_FILE).exists());
    for (account, value) in LEGACY {
        assert_eq!(backend.item(account).as_deref(), Some(*value));
    }

    // Unlocked later in the same process: it goes through.
    backend.refuse_reads.lock().unwrap().clear();
    assert_eq!(everything(&store), expected_after_migration());
}

/// A Secret Service that cannot be asked answers "nothing stored" for every
/// slot, which must not be taken for an empty keychain.
#[test]
fn a_migration_waits_for_a_secret_service_that_does_not_answer() {
    let (backend, dir) = (Backend::linux(), tempfile::tempdir().unwrap());
    seed_legacy(&backend);
    backend.down_all.store(true, Ordering::Relaxed);

    let store = launch(&backend, dir.path());
    store.get(Field::Model("anthropic")).unwrap();
    assert!(
        !dir.path().join(SETTINGS_FILE).exists(),
        "nothing was concluded"
    );
    for (account, value) in LEGACY {
        assert_eq!(
            backend.item(account).as_deref(),
            Some(*value),
            "{account} kept"
        );
    }

    // It comes back: the next launch moves everything.
    backend.down_all.store(false, Ordering::Relaxed);
    let next = launch(&backend, dir.path());
    assert_eq!(everything(&next), expected_after_migration());
    assert!(dir.path().join(SETTINGS_FILE).exists());
    assert_eq!(
        backend.secrets().len(),
        5,
        "four secrets and the migrated marker"
    );
    assert!(backend.item("anthropic_api_key").is_none());
}

/// If the new item could only be saved to the kernel keyring, the old items
/// are not deleted for a copy that a reboot erases.
#[test]
fn a_migration_that_could_only_save_to_the_kernel_keyring_changes_nothing() {
    let (backend, dir) = (Backend::linux(), tempfile::tempdir().unwrap());
    seed_legacy(&backend);
    backend
        .down
        .lock()
        .unwrap()
        .insert(SECRETS_ACCOUNT.to_string());

    let store = launch(&backend, dir.path());
    assert_eq!(everything(&store), expected_after_migration());
    assert!(
        backend
            .kernel
            .lock()
            .unwrap()
            .get(SECRETS_ACCOUNT)
            .is_none(),
        "taken back"
    );
    assert!(!dir.path().join(SETTINGS_FILE).exists());
    for (account, value) in LEGACY {
        assert_eq!(
            backend.item(account).as_deref(),
            Some(*value),
            "{account} kept"
        );
    }
}

/// A leftover kernel-keyring copy of a slot (a session-only save of an earlier
/// build) is a value too, and goes where the others go.
#[test]
fn a_kernel_keyring_copy_of_an_old_slot_is_migrated_and_removed() {
    let (backend, dir) = (Backend::linux(), tempfile::tempdir().unwrap());
    seed_legacy(&backend);
    backend
        .kernel
        .lock()
        .unwrap()
        .insert("openai_api_key".to_string(), "key-openai-newer".to_string());

    let store = launch(&backend, dir.path());
    assert_eq!(
        store.get(Field::ApiKey("openai")).unwrap().as_deref(),
        Some("key-openai-newer")
    );
    assert!(backend.kernel.lock().unwrap().is_empty());
    assert!(backend.item("openai_api_key").is_none());
}

// ---------------------------------------------------------------------------
// Writes, deletes, the cache and the lock.
// ---------------------------------------------------------------------------

/// A key save and a ChatGPT token refresh at the same moment each rewrite the
/// one item. Both must be in it afterwards.
#[test]
fn a_key_and_the_chatgpt_token_saved_at_once_both_persist() {
    for round in 0..40 {
        let (backend, dir) = (Backend::new(), tempfile::tempdir().unwrap());
        let store = launch(&backend, dir.path());
        let barrier = std::sync::Barrier::new(3);
        std::thread::scope(|scope| {
            scope.spawn(|| {
                barrier.wait();
                store
                    .put(Field::ApiKey("openai"), &format!("key-{round}"))
                    .unwrap();
            });
            scope.spawn(|| {
                barrier.wait();
                store
                    .put(Field::ChatGptToken, &format!("token-{round}"))
                    .unwrap();
            });
            barrier.wait();
        });
        for reader in [&store, &launch(&backend, dir.path())] {
            assert_eq!(
                reader.get(Field::ApiKey("openai")).unwrap(),
                Some(format!("key-{round}"))
            );
            assert_eq!(
                reader.get(Field::ChatGptToken).unwrap(),
                Some(format!("token-{round}"))
            );
        }
        assert_eq!(backend.secrets().len(), 2);
    }
}

#[test]
fn a_save_is_what_the_next_read_returns_in_the_same_process() {
    let (backend, dir) = (Backend::new(), tempfile::tempdir().unwrap());
    let store = launch(&backend, dir.path());
    assert_eq!(store.get(Field::ApiKey("openai")).unwrap(), None);
    store.put(Field::ApiKey("openai"), "key-1").unwrap();
    assert_eq!(
        store.get(Field::ApiKey("openai")).unwrap().as_deref(),
        Some("key-1")
    );
    store.put(Field::ApiKey("openai"), "key-2").unwrap();
    assert_eq!(
        store.get(Field::ApiKey("openai")).unwrap().as_deref(),
        Some("key-2")
    );
    assert_eq!(backend.secrets()["openai_api_key"], "key-2");
    assert_eq!(
        backend.count(Op::Get, SECRETS_ACCOUNT),
        3,
        "one read for the first get, then one for each save (it reads the item itself)"
    );
}

#[test]
fn deleting_a_key_removes_it_from_the_item_and_the_cache_alike() {
    let (backend, dir) = (Backend::new(), tempfile::tempdir().unwrap());
    let store = launch(&backend, dir.path());
    store.put(Field::ApiKey("openai"), "key-o").unwrap();
    store.put(Field::ApiKey("groq"), "key-g").unwrap();

    store.remove(Field::ApiKey("openai")).unwrap();
    assert_eq!(
        store.get(Field::ApiKey("openai")).unwrap(),
        None,
        "the cache agrees"
    );
    assert_eq!(
        store.get(Field::ApiKey("groq")).unwrap().as_deref(),
        Some("key-g")
    );
    assert_eq!(
        backend.secrets(),
        SecretMap::from([("groq_api_key".to_string(), "key-g".to_string())]),
        "and so does the item"
    );
    assert_eq!(
        launch(&backend, dir.path())
            .get(Field::ApiKey("openai"))
            .unwrap(),
        None
    );

    // Removing what is not there reads the item (to know) and writes nothing.
    backend.forget_calls();
    store.remove(Field::ApiKey("openai")).unwrap();
    assert_eq!(backend.count(Op::Set, SECRETS_ACCOUNT), 0);
    assert_eq!(backend.count(Op::Delete, SECRETS_ACCOUNT), 0);

    // The last one takes the item with it.
    store.remove(Field::ApiKey("groq")).unwrap();
    assert_eq!(backend.item(SECRETS_ACCOUNT), None);
    assert_eq!(store.get(Field::ApiKey("groq")).unwrap(), None);
    let fresh = launch(&backend, dir.path());
    assert_eq!(fresh.get(Field::ApiKey("groq")).unwrap(), None);
    fresh.put(Field::ChatGptToken, "t").unwrap();
    assert_eq!(
        fresh.get(Field::ChatGptToken).unwrap().as_deref(),
        Some("t")
    );
}

/// A save that failed changes neither the item nor what the next read says.
#[test]
fn a_refused_save_leaves_the_cache_as_it_was() {
    let (backend, dir) = (Backend::new(), tempfile::tempdir().unwrap());
    let store = launch(&backend, dir.path());
    store.put(Field::ApiKey("openai"), "key-1").unwrap();
    store.put(Field::ApiKey("groq"), "key-g1").unwrap();
    backend
        .refuse_writes
        .lock()
        .unwrap()
        .insert(SECRETS_ACCOUNT.to_string());
    assert!(store.put(Field::ApiKey("openai"), "key-2").is_err());
    assert!(store.put(Field::ApiKey("gemini"), "key-m").is_err());
    assert!(store.remove(Field::ApiKey("openai")).is_err());
    assert_eq!(
        store.get(Field::ApiKey("openai")).unwrap().as_deref(),
        Some("key-1")
    );
    assert_eq!(
        store.get(Field::ApiKey("groq")).unwrap().as_deref(),
        Some("key-g1")
    );
    assert_eq!(store.get(Field::ApiKey("gemini")).unwrap(), None);
    assert_eq!(backend.secrets()["openai_api_key"], "key-1");
}

#[test]
fn a_session_only_save_is_reported_and_read_back() {
    let (backend, dir) = (Backend::linux(), tempfile::tempdir().unwrap());
    std::fs::write(dir.path().join(SETTINGS_FILE), r#"{"version":1}"#).unwrap();
    backend.down_all.store(true, Ordering::Relaxed);

    let store = launch(&backend, dir.path());
    match store.put(Field::ApiKey("openai"), "key-o").unwrap() {
        Persistence::SessionOnly { reason } => {
            assert!(reason.contains("no secret service"), "{reason}")
        }
        other => panic!("expected session-only, got {other:?}"),
    }
    assert_eq!(
        store.get(Field::ApiKey("openai")).unwrap().as_deref(),
        Some("key-o")
    );
    assert!(backend.kernel.lock().unwrap()[SECRETS_ACCOUNT].contains("key-o"));
    // A setting is a file, and says so.
    assert_eq!(
        store.put(Field::Model("openai"), "m").unwrap(),
        Persistence::Durable
    );
}

/// No Secret Service yet, and nothing stored: not an answer to remember, or a
/// service that starts a second later would be ignored for the whole process.
#[test]
fn an_unreachable_secret_service_is_not_remembered_as_an_empty_item() {
    let (backend, dir) = (Backend::linux(), tempfile::tempdir().unwrap());
    std::fs::write(dir.path().join(SETTINGS_FILE), r#"{"version":1}"#).unwrap();
    backend.seed(SECRETS_ACCOUNT, r#"{"openai_api_key":"key-o"}"#);
    backend.down_all.store(true, Ordering::Relaxed);

    let store = launch(&backend, dir.path());
    assert_eq!(store.get(Field::ApiKey("openai")).unwrap(), None);
    backend.down_all.store(false, Ordering::Relaxed);
    assert_eq!(
        store.get(Field::ApiKey("openai")).unwrap().as_deref(),
        Some("key-o")
    );
}

#[test]
fn a_locked_keychain_is_an_error_not_an_empty_item() {
    let (backend, dir) = (Backend::new(), tempfile::tempdir().unwrap());
    std::fs::write(dir.path().join(SETTINGS_FILE), r#"{"version":1}"#).unwrap();
    backend.seed(SECRETS_ACCOUNT, r#"{"openai_api_key":"key-o"}"#);
    backend
        .refuse_reads
        .lock()
        .unwrap()
        .insert(SECRETS_ACCOUNT.to_string());

    let store = launch(&backend, dir.path());
    assert!(store
        .get(Field::ApiKey("openai"))
        .unwrap_err()
        .is_access_denied());
    assert!(matches!(
        store.put(Field::ApiKey("groq"), "key-g").unwrap_err(),
        keyring::Error::NoStorageAccess(_)
    ));
    assert!(store.remove(Field::ChatGptToken).is_err());
    // Settings do not need the keychain.
    assert_eq!(store.get(Field::Model("openai")).unwrap(), None);
    backend.refuse_reads.lock().unwrap().clear();
    assert_eq!(
        store.get(Field::ApiKey("openai")).unwrap().as_deref(),
        Some("key-o")
    );
    assert_eq!(backend.secrets().len(), 1, "the refused save wrote nothing");
}

/// A secrets item that is not what it should be is an error that does not
/// quote it: a parse error from `serde_json` would carry the value.
#[test]
fn an_unreadable_secrets_item_is_an_error_that_does_not_quote_it() {
    let (backend, dir) = (Backend::new(), tempfile::tempdir().unwrap());
    std::fs::write(dir.path().join(SETTINGS_FILE), r#"{"version":1}"#).unwrap();
    backend.seed(SECRETS_ACCOUNT, r#""key-leaky""#);
    let store = launch(&backend, dir.path());
    let err = store.get(Field::ApiKey("openai")).unwrap_err();
    assert!(!format!("{err} {err:?}").contains("key-leaky"));
    let err = store.put(Field::ApiKey("openai"), "key-new").unwrap_err();
    assert!(!format!("{err}").contains("key-leaky"));
    assert_eq!(
        backend.item(SECRETS_ACCOUNT).as_deref(),
        Some(r#""key-leaky""#),
        "not overwritten"
    );
}

// ---------------------------------------------------------------------------
// No folder, and the probe.
// ---------------------------------------------------------------------------

/// A process that registered no settings folder is told so, by every function,
/// and touches neither a file nor the keychain.
#[test]
fn with_no_settings_folder_every_function_fails_and_touches_nothing() {
    assert!(
        !set_settings_dir(PathBuf::from("relative/dir")),
        "a relative folder is refused"
    );
    assert!(settings_dir().is_none());

    let backend = Backend::new();
    let b = Arc::clone(&backend);
    let store = Store::new(
        DirSource::Registered,
        Box::new(move |account| {
            Ok((
                Box::new(FakeSlot {
                    backend: Arc::clone(&b),
                    account: account.to_string(),
                    kernel: false,
                }) as Box<dyn Slot>,
                None,
            ))
        }),
    );
    assert!(store.get(Field::ApiKey("openai")).is_err());
    assert!(store.get(Field::Model("openai")).is_err());
    assert!(store.put(Field::Model("openai"), "m").is_err());
    assert!(store.put(Field::ApiKey("openai"), "k").is_err());
    assert!(store.remove(Field::ActiveProvider).is_err());
    assert_eq!(backend.total_calls(), 0);
    assert!(backend.items.lock().unwrap().is_empty());
}

#[test]
fn the_persistence_probe_reads_an_account_nothing_writes_and_only_where_there_is_a_fallback() {
    let (backend, dir) = (Backend::new(), tempfile::tempdir().unwrap());
    let store = launch(&backend, dir.path());
    assert_eq!(store.persistence(), Persistence::Durable);
    assert_eq!(
        backend.total_calls(),
        0,
        "macOS and Windows: no extra keychain call"
    );

    let (linux, dir) = (Backend::linux(), tempfile::tempdir().unwrap());
    let store = launch(&linux, dir.path());
    assert_eq!(store.persistence(), Persistence::Durable);
    assert_eq!(linux.count(Op::Get, PROBE_ACCOUNT), 1);
    linux.down_all.store(true, Ordering::Relaxed);
    assert!(matches!(
        store.persistence(),
        Persistence::SessionOnly { .. }
    ));
    assert!(linux.items.lock().unwrap().is_empty());
}

#[test]
fn the_new_accounts_collide_with_no_slot() {
    assert_ne!(SECRETS_ACCOUNT, PROBE_ACCOUNT);
    assert_ne!(SECRETS_ACCOUNT, ACTIVE_PROVIDER_ACCOUNT);
    assert_ne!(SECRETS_ACCOUNT, CHATGPT_REFRESH_TOKEN_ACCOUNT);
    for p in crate::SUPPORTED_PROVIDER_IDS {
        for key in [
            Field::ApiKey(p),
            Field::Model(p),
            Field::BaseUrl(p),
            Field::Effort(p),
        ] {
            assert_ne!(key.account(), SECRETS_ACCOUNT);
        }
    }
    // Which slots are secrets is the whole of the split.
    let secrets: Vec<_> = legacy_keys()
        .into_iter()
        .filter(Field::is_secret)
        .map(|k| k.account())
        .collect();
    assert!(
        secrets
            .iter()
            .all(|a| a.ends_with("_api_key") || a == CHATGPT_REFRESH_TOKEN_ACCOUNT),
        "{secrets:?}"
    );
    assert!(
        !secrets.contains(&"chatgpt_api_key".to_string()),
        "the plan takes no key"
    );
    assert!(
        !legacy_keys().contains(&Field::BaseUrl(crate::CHATGPT_ID)),
        "and no address"
    );
}

// ---------------------------------------------------------------------------
// Review fixes on #741.
// ---------------------------------------------------------------------------

/// A store that has finished the migration, over a Linux keychain, with the
/// durable `secrets` item holding the given keys.
fn linux_with_keys(keys: &[(&str, &str)]) -> (Arc<Backend>, tempfile::TempDir) {
    let (backend, dir) = (Backend::linux(), tempfile::tempdir().unwrap());
    std::fs::write(dir.path().join(SETTINGS_FILE), r#"{"version":1}"#).unwrap();
    let map: SecretMap = keys
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect();
    backend.seed(SECRETS_ACCOUNT, &secrets_json(&map).unwrap());
    (backend, dir)
}

/// Finding 1. The Secret Service is down (and nothing is cached) when a key is
/// saved, so the save lands in the kernel keyring. When the service is back,
/// that copy has to join the durable item, not replace it.
#[test]
fn a_save_during_a_secret_service_outage_does_not_wipe_the_other_keys() {
    let (backend, dir) =
        linux_with_keys(&[("anthropic_api_key", "key-a"), ("openai_api_key", "key-o")]);
    backend.down_all.store(true, Ordering::Relaxed);
    let store = launch(&backend, dir.path());
    assert!(matches!(
        store.put(Field::ApiKey("groq"), "key-g").unwrap(),
        Persistence::SessionOnly { .. }
    ));
    backend.down_all.store(false, Ordering::Relaxed);

    for reader in [&store, &launch(&backend, dir.path())] {
        assert_eq!(
            reader.get(Field::ApiKey("anthropic")).unwrap().as_deref(),
            Some("key-a")
        );
        assert_eq!(
            reader.get(Field::ApiKey("openai")).unwrap().as_deref(),
            Some("key-o")
        );
        assert_eq!(
            reader.get(Field::ApiKey("groq")).unwrap().as_deref(),
            Some("key-g")
        );
    }
    assert_eq!(
        backend.secrets().len(),
        3,
        "the durable item holds all three"
    );
    assert!(
        backend.kernel.lock().unwrap().is_empty(),
        "and the kernel copy is gone"
    );
}

/// The same, with the item already read when the outage starts.
#[test]
fn a_save_during_an_outage_after_a_read_keeps_what_was_read() {
    let (backend, dir) =
        linux_with_keys(&[("anthropic_api_key", "key-a"), ("openai_api_key", "key-o")]);
    let store = launch(&backend, dir.path());
    assert!(store.get(Field::ApiKey("anthropic")).unwrap().is_some());
    backend.down_all.store(true, Ordering::Relaxed);
    assert!(store.put(Field::ApiKey("groq"), "key-g").is_ok());
    backend.down_all.store(false, Ordering::Relaxed);
    let fresh = launch(&backend, dir.path());
    for (key, want) in [
        ("anthropic", "key-a"),
        ("openai", "key-o"),
        ("groq", "key-g"),
    ] {
        assert_eq!(
            fresh.get(Field::ApiKey(key)).unwrap().as_deref(),
            Some(want),
            "{key}"
        );
    }
}

/// A kernel copy wins per key, and only for the keys it has.
#[test]
fn a_kernel_copy_of_the_secrets_item_is_merged_key_by_key() {
    let (backend, dir) = linux_with_keys(&[
        ("anthropic_api_key", "key-a"),
        ("openai_api_key", "key-o-old"),
    ]);
    backend.kernel.lock().unwrap().insert(
        SECRETS_ACCOUNT.to_string(),
        r#"{"openai_api_key":"key-o-new","groq_api_key":"key-g"}"#.to_string(),
    );
    let store = launch(&backend, dir.path());
    assert_eq!(
        store.get(Field::ApiKey("anthropic")).unwrap().as_deref(),
        Some("key-a")
    );
    assert_eq!(
        store.get(Field::ApiKey("openai")).unwrap().as_deref(),
        Some("key-o-new")
    );
    assert_eq!(
        store.get(Field::ApiKey("groq")).unwrap().as_deref(),
        Some("key-g")
    );
    assert_eq!(backend.secrets().len(), 3);
    assert!(backend.kernel.lock().unwrap().is_empty());
}

/// A removal cannot be recorded while the durable item cannot be reached (the
/// kernel copy would be merged over it and bring the key back), so it says so.
#[test]
fn a_remove_during_an_outage_is_an_error_and_removes_nothing() {
    let (backend, dir) =
        linux_with_keys(&[("anthropic_api_key", "key-a"), ("openai_api_key", "key-o")]);
    backend.down_all.store(true, Ordering::Relaxed);
    let store = launch(&backend, dir.path());
    assert!(store.remove(Field::ApiKey("openai")).is_err());
    assert!(store.remove(Field::ChatGptToken).is_err());

    // With the item read before the outage, too.
    backend.down_all.store(false, Ordering::Relaxed);
    let store = launch(&backend, dir.path());
    assert!(store.get(Field::ApiKey("openai")).unwrap().is_some());
    backend.down_all.store(true, Ordering::Relaxed);
    assert!(store.remove(Field::ApiKey("openai")).is_err());

    backend.down_all.store(false, Ordering::Relaxed);
    assert_eq!(
        launch(&backend, dir.path())
            .get(Field::ApiKey("openai"))
            .unwrap()
            .as_deref(),
        Some("key-o")
    );
}

/// Finding 2. Two processes share the keychain and the folder. A rotated
/// ChatGPT token saved by one is not undone by a key saved by the other.
#[test]
fn two_processes_do_not_overwrite_each_others_secrets() {
    let (backend, dir) = (Backend::new(), tempfile::tempdir().unwrap());
    installed(&backend, dir.path());
    let (a, b) = (launch(&backend, dir.path()), launch(&backend, dir.path()));
    // B has read the item, so it holds the old token in its cache.
    assert_eq!(
        b.get(Field::ChatGptToken).unwrap().as_deref(),
        Some("refresh-token-1")
    );
    a.put(Field::ChatGptToken, "refresh-token-2").unwrap();
    b.put(Field::ApiKey("groq"), "key-g").unwrap();

    let item = backend.secrets();
    assert_eq!(item["chatgpt_refresh_token"], "refresh-token-2");
    assert_eq!(item["groq_api_key"], "key-g");
    assert_eq!(item["anthropic_api_key"], "key-anthropic");
    for reader in [&a, &b, &launch(&backend, dir.path())] {
        assert_eq!(
            reader.get(Field::ChatGptToken).unwrap().as_deref(),
            Some("refresh-token-2")
        );
    }

    // And a removal in one is not undone by a save in the other.
    a.remove(Field::ApiKey("groq")).unwrap();
    b.put(Field::ApiKey("openai"), "key-o").unwrap();
    let item = backend.secrets();
    assert!(!item.contains_key("groq_api_key"));
    assert_eq!(item["openai_api_key"], "key-o");
}

/// Finding 2, for the settings file.
#[test]
fn two_processes_do_not_overwrite_each_others_settings() {
    let (backend, dir) = (Backend::new(), tempfile::tempdir().unwrap());
    installed(&backend, dir.path());
    let (a, b) = (launch(&backend, dir.path()), launch(&backend, dir.path()));
    assert!(b.get(Field::ActiveProvider).unwrap().is_some());
    a.put(Field::Model("openai"), "gpt-from-a").unwrap();
    b.put(Field::Effort("openai"), "low").unwrap();
    b.remove(Field::Model("anthropic")).unwrap();

    let json = settings_json(dir.path());
    assert_eq!(json["providers"]["openai"]["model"], "gpt-from-a");
    assert_eq!(json["providers"]["openai"]["effort"], "low");
    assert!(json["providers"]
        .get("anthropic")
        .is_none_or(|p| p.get("model").is_none()));
    assert_eq!(json["providers"]["anthropic"]["effort"], "high");
}

/// Finding 3. A settings file that is there and cannot be read is not a
/// reason to read per-slot items that the migration deleted.
#[test]
fn an_unreadable_settings_file_is_an_error_not_the_legacy_layout() {
    let (backend, dir) = (Backend::new(), tempfile::tempdir().unwrap());
    installed(&backend, dir.path());
    std::fs::remove_file(dir.path().join(SETTINGS_FILE)).unwrap();
    std::fs::create_dir(dir.path().join(SETTINGS_FILE)).unwrap();
    let before = backend.secrets();

    let store = launch(&backend, dir.path());
    assert!(store.get(Field::ApiKey("anthropic")).is_err());
    assert!(store.get(Field::ChatGptToken).is_err());
    assert!(store.get(Field::Model("anthropic")).is_err());
    assert!(store.put(Field::ApiKey("openai"), "key-o").is_err());
    assert!(store.put(Field::Model("openai"), "m").is_err());
    assert!(store.remove(Field::ApiKey("anthropic")).is_err());
    let text = store
        .get(Field::ApiKey("anthropic"))
        .unwrap_err()
        .to_string();
    assert!(!text.contains(dir.path().to_str().unwrap()), "{text}");

    assert_eq!(backend.secrets(), before, "no data lost");
    assert!(
        backend.item("openai_api_key").is_none() && backend.item("active_provider").is_none(),
        "nothing was saved to a per-slot item that is never read"
    );

    // The folder in the way is cleared: the same store answers again.
    std::fs::remove_dir(dir.path().join(SETTINGS_FILE)).unwrap();
    std::fs::write(dir.path().join(SETTINGS_FILE), r#"{"version":1}"#).unwrap();
    assert_eq!(
        store.get(Field::ApiKey("anthropic")).unwrap().as_deref(),
        Some("key-anthropic")
    );
}

/// Finding 4. The migration finished once; a per-slot item whose delete failed
/// is still there when the settings file goes missing. The newer key in the
/// item stands.
#[test]
fn a_leftover_per_slot_item_does_not_overwrite_a_newer_key_after_a_finished_migration() {
    let (backend, dir) = (Backend::new(), tempfile::tempdir().unwrap());
    backend.seed(
        SECRETS_ACCOUNT,
        &format!(r#"{{"anthropic_api_key":"key-new","{MIGRATED_MARKER}":"1"}}"#),
    );
    backend.seed("anthropic_api_key", "key-old");
    backend.seed("openai_api_key", "key-openai-leftover");
    backend.seed("anthropic_model", "claude-left-over");
    assert!(!dir.path().join(SETTINGS_FILE).exists());

    let store = launch(&backend, dir.path());
    assert_eq!(
        store.get(Field::ApiKey("anthropic")).unwrap().as_deref(),
        Some("key-new")
    );
    assert_eq!(
        store.get(Field::ApiKey("openai")).unwrap(),
        None,
        "a leftover is deleted, not adopted"
    );
    assert!(backend.item("anthropic_api_key").is_none());
    assert!(backend.item("openai_api_key").is_none());
    assert_eq!(backend.secrets()["anthropic_api_key"], "key-new");
    assert!(backend.secrets().contains_key(MIGRATED_MARKER));
}

/// A migration writes the marker only once the settings file is there, and the
/// marker is no key anyone can read or save.
#[test]
fn a_finished_migration_marks_the_item_and_an_unfinished_one_does_not() {
    let (backend, dir) = (Backend::new(), tempfile::tempdir().unwrap());
    seed_legacy(&backend);
    let store = launch(&backend, dir.path());
    assert_eq!(everything(&store), expected_after_migration());
    assert_eq!(backend.secrets()[MIGRATED_MARKER], "1");
    assert_eq!(
        store.get(Field::ApiKey(MIGRATED_MARKER)).ok().flatten(),
        None
    );
    // Removing the last key leaves the marker, not an item that vanishes.
    for k in ["anthropic", "openai", "groq"] {
        store.remove(Field::ApiKey(k)).unwrap();
    }
    store.remove(Field::ChatGptToken).unwrap();
    assert_eq!(
        backend.secrets(),
        SecretMap::from([(MIGRATED_MARKER.to_string(), "1".to_string())])
    );

    // An attempt that stopped before the file: no marker, so the per-slot
    // items (what the app was using) still win over the half-written item.
    let (backend, dir) = (Backend::new(), tempfile::tempdir().unwrap());
    backend.seed(SECRETS_ACCOUNT, r#"{"anthropic_api_key":"key-half"}"#);
    backend.seed("anthropic_api_key", "key-in-use");
    let store = launch(&backend, dir.path());
    assert_eq!(
        store.get(Field::ApiKey("anthropic")).unwrap().as_deref(),
        Some("key-in-use")
    );
}

/// Finding 5. An MCP server's `<keychain:slot>` is the user's own per-slot item
/// (`<slot>_api_key`). It is read on demand, never migrated, and it does not
/// cost a provider's slot an extra read.
#[test]
fn an_mcp_slot_resolves_after_migration() {
    let (backend, dir) = (Backend::new(), tempfile::tempdir().unwrap());
    seed_legacy(&backend);
    backend.seed("github_token_api_key", "ghp-from-the-os-store");
    let store = launch(&backend, dir.path());
    assert_eq!(everything(&store), expected_after_migration());

    assert_eq!(
        store.get(Field::ApiKey("github_token")).unwrap().as_deref(),
        Some("ghp-from-the-os-store")
    );
    assert_eq!(
        backend.item("github_token_api_key").as_deref(),
        Some("ghp-from-the-os-store"),
        "not migrated, not deleted"
    );
    assert!(!backend.secrets().contains_key("github_token_api_key"));
    assert_eq!(store.get(Field::ApiKey("no_such_slot")).unwrap(), None);

    // A provider's slot that is not set never reads its per-slot item.
    backend.forget_calls();
    assert_eq!(store.get(Field::ApiKey("gemini")).unwrap(), None);
    assert_eq!(backend.total_calls(), 0);
    assert_eq!(backend.count(Op::Get, "gemini_api_key"), 0);
}

/// Finding 6. The item is written as bytes: a map that would not fit as
/// UTF-16 (the cap here is Windows' 2,560 bytes) is saved, one that is over the
/// cap in bytes fails with `TooLong` and changes nothing.
#[test]
fn the_secrets_item_is_written_as_bytes_so_twice_as_much_fits() {
    let (backend, dir) = (Backend::new(), tempfile::tempdir().unwrap());
    *backend.cap.lock().unwrap() = Some(2560);
    let store = launch(&backend, dir.path());
    let long = "k".repeat(300);
    for p in ["anthropic", "openai", "groq", "gemini", "openrouter"] {
        store.put(Field::ApiKey(p), &long).unwrap();
    }
    store.put(Field::ChatGptToken, &long).unwrap();
    let size = backend.item(SECRETS_ACCOUNT).unwrap().len();
    assert!(size > 1280 && size < 2560, "{size}");

    let err = store
        .put(Field::ApiKey("ollama"), &"k".repeat(900))
        .unwrap_err();
    assert!(matches!(err, keyring::Error::TooLong(..)), "{err}");
    assert_eq!(backend.item(SECRETS_ACCOUNT).unwrap().len(), size);
    assert_eq!(store.get(Field::ApiKey("ollama")).unwrap(), None);
    assert_eq!(
        launch(&backend, dir.path())
            .get(Field::ApiKey("openrouter"))
            .unwrap()
            .as_deref(),
        Some(long.as_str())
    );
}

/// Finding 7. A read the platform failed (a macOS Deny arrives as
/// `PlatformFailure`) is not asked again in the same launch; a save is a user
/// action, so it asks again.
#[test]
fn a_failed_secrets_read_is_remembered_until_a_save_tries_again() {
    let (backend, dir) = (Backend::new(), tempfile::tempdir().unwrap());
    std::fs::write(dir.path().join(SETTINGS_FILE), r#"{"version":1}"#).unwrap();
    backend.seed(SECRETS_ACCOUNT, r#"{"openai_api_key":"key-o"}"#);
    backend
        .down
        .lock()
        .unwrap()
        .insert(SECRETS_ACCOUNT.to_string());

    let store = launch(&backend, dir.path());
    let first = store.get(Field::ApiKey("openai")).unwrap_err();
    assert_eq!(backend.count(Op::Get, SECRETS_ACCOUNT), 1);
    assert_eq!(store.get(Field::ApiKey("openai")).unwrap_err(), first);
    assert_eq!(store.get(Field::ChatGptToken).unwrap_err(), first);
    assert_eq!(
        backend.count(Op::Get, SECRETS_ACCOUNT),
        1,
        "no second prompt"
    );
    // Settings never needed it.
    assert_eq!(store.get(Field::Model("openai")).unwrap(), None);

    // A save asks again (and, failing again, is remembered again).
    assert!(store.put(Field::ApiKey("groq"), "key-g").is_err());
    assert_eq!(backend.count(Op::Get, SECRETS_ACCOUNT), 2);
    assert!(store.get(Field::ApiKey("groq")).is_err());
    assert_eq!(backend.count(Op::Get, SECRETS_ACCOUNT), 2);

    // Allowed this time: the save goes through and the reads work again.
    backend.down.lock().unwrap().clear();
    store.put(Field::ApiKey("groq"), "key-g").unwrap();
    assert_eq!(
        store.get(Field::ApiKey("openai")).unwrap().as_deref(),
        Some("key-o")
    );
    assert_eq!(
        store.get(Field::ApiKey("groq")).unwrap().as_deref(),
        Some("key-g")
    );
}

// ---------------------------------------------------------------------------
// Second-round review fixes on #741.
// ---------------------------------------------------------------------------

/// The name `write_settings_file` gives its temporary file: a folder of this
/// name in the way makes every write of the settings file fail, while a read of
/// it still finds the file missing (the tests run as a user a read-only folder
/// would not stop).
fn block_settings_writes(dir: &Path) {
    std::fs::create_dir(dir.join(format!("{SETTINGS_FILE}.tmp-{}", std::process::id()))).unwrap();
}

/// A marked item and no settings file that can be written: the item is still
/// the truth. The launch must not fall back to the per-slot items (the next
/// launch would delete what was saved there).
#[test]
fn a_marked_item_and_a_settings_file_that_cannot_be_written_stay_on_the_item() {
    let (backend, dir) = (Backend::new(), tempfile::tempdir().unwrap());
    backend.seed(
        SECRETS_ACCOUNT,
        &format!(r#"{{"{MIGRATED_MARKER}":"1","anthropic_api_key":"key-a"}}"#),
    );
    block_settings_writes(dir.path());

    let store = launch(&backend, dir.path());
    assert_eq!(
        store.get(Field::ApiKey("anthropic")).unwrap().as_deref(),
        Some("key-a"),
        "no Welcome over a stored key"
    );
    // A re-entered key lands in the item, not in a per-slot one.
    store.put(Field::ApiKey("anthropic"), "key-new").unwrap();
    assert_eq!(backend.secrets()["anthropic_api_key"], "key-new");
    assert!(backend.item("anthropic_api_key").is_none());
    // A setting cannot be saved while the file cannot be written, and says so.
    assert!(store.put(Field::Model("anthropic"), "m").is_err());
    assert_eq!(store.get(Field::Model("anthropic")).unwrap(), None);

    // The next launch, with the folder unblocked, keeps the new key.
    std::fs::remove_dir(
        dir.path()
            .join(format!("{SETTINGS_FILE}.tmp-{}", std::process::id())),
    )
    .unwrap();
    let next = launch(&backend, dir.path());
    assert_eq!(
        next.get(Field::ApiKey("anthropic")).unwrap().as_deref(),
        Some("key-new")
    );
    // And the save that failed works now.
    next.put(Field::Model("anthropic"), "m").unwrap();
    assert_eq!(
        settings_json(dir.path())["providers"]["anthropic"]["model"],
        "m"
    );
}

/// The same, with per-slot settings left over: they seed the launch's settings.
#[test]
fn a_marked_item_with_leftover_settings_serves_them_while_the_file_cannot_be_written() {
    let (backend, dir) = (Backend::new(), tempfile::tempdir().unwrap());
    backend.seed(
        SECRETS_ACCOUNT,
        &format!(r#"{{"{MIGRATED_MARKER}":"1","anthropic_api_key":"key-a"}}"#),
    );
    backend.seed("anthropic_model", "claude-left-over");
    block_settings_writes(dir.path());
    let store = launch(&backend, dir.path());
    assert_eq!(
        store.get(Field::Model("anthropic")).unwrap().as_deref(),
        Some("claude-left-over")
    );
    assert_eq!(
        backend.item("anthropic_model").as_deref(),
        Some("claude-left-over"),
        "kept until the file can be written"
    );
}

/// An item with no marker is not known to be finished: that case keeps the
/// per-slot items, as before.
#[test]
fn an_unmarked_item_and_a_settings_file_that_cannot_be_written_stay_on_the_old_items() {
    let (backend, dir) = (Backend::new(), tempfile::tempdir().unwrap());
    seed_legacy(&backend);
    block_settings_writes(dir.path());
    let store = launch(&backend, dir.path());
    assert_eq!(everything(&store), expected_after_migration());
    store.put(Field::ApiKey("groq"), "key-groq-2").unwrap();
    assert_eq!(backend.item("groq_api_key").as_deref(), Some("key-groq-2"));
}

/// A launch clock the test moves.
fn clocked(store: &mut Store) -> Arc<Mutex<std::time::Duration>> {
    let start = std::time::Instant::now();
    let moved = Arc::new(Mutex::new(std::time::Duration::ZERO));
    let seen = Arc::clone(&moved);
    store.now = Box::new(move || start + *seen.lock().unwrap());
    moved
}

/// Finding 2 of round two. A failed read is remembered for a short while (the
/// startup sequence's reads must not each raise the prompt again after a
/// Deny), not for the whole process: a transient failure heals.
#[test]
fn a_failed_secrets_read_is_remembered_for_a_short_while_only() {
    let (backend, dir) = (Backend::new(), tempfile::tempdir().unwrap());
    std::fs::write(dir.path().join(SETTINGS_FILE), r#"{"version":1}"#).unwrap();
    backend.seed(SECRETS_ACCOUNT, r#"{"openai_api_key":"key-o"}"#);
    backend
        .down
        .lock()
        .unwrap()
        .insert(SECRETS_ACCOUNT.to_string());
    let mut store = launch(&backend, dir.path());
    let clock = clocked(&mut store);

    assert!(store.get(Field::ApiKey("openai")).is_err());
    *clock.lock().unwrap() = std::time::Duration::from_secs(5);
    assert!(store.get(Field::ChatGptToken).is_err());
    assert_eq!(
        backend.count(Op::Get, SECRETS_ACCOUNT),
        1,
        "inside the window: no second backend call"
    );

    // The store recovers; after the window the next read asks again.
    backend.down.lock().unwrap().clear();
    *clock.lock().unwrap() = std::time::Duration::from_secs(11);
    assert_eq!(
        store.get(Field::ApiKey("openai")).unwrap().as_deref(),
        Some("key-o")
    );
    assert_eq!(backend.count(Op::Get, SECRETS_ACCOUNT), 2);

    // A failure again starts a new window.
    let (backend, dir) = (Backend::new(), tempfile::tempdir().unwrap());
    std::fs::write(dir.path().join(SETTINGS_FILE), r#"{"version":1}"#).unwrap();
    backend.seed(SECRETS_ACCOUNT, r#"{"openai_api_key":"key-o"}"#);
    backend
        .down
        .lock()
        .unwrap()
        .insert(SECRETS_ACCOUNT.to_string());
    let mut store = launch(&backend, dir.path());
    let clock = clocked(&mut store);
    assert!(store.get(Field::ApiKey("openai")).is_err());
    *clock.lock().unwrap() = std::time::Duration::from_secs(11);
    assert!(store.get(Field::ApiKey("openai")).is_err());
    assert_eq!(backend.count(Op::Get, SECRETS_ACCOUNT), 2);
    *clock.lock().unwrap() = std::time::Duration::from_secs(15);
    assert!(store.get(Field::ApiKey("openai")).is_err());
    assert_eq!(
        backend.count(Op::Get, SECRETS_ACCOUNT),
        2,
        "inside the new window"
    );
}

/// A settings file found damaged when a save re-reads it is set aside as
/// `.bad`, as at launch, before a good one is written over it.
#[test]
fn a_settings_file_damaged_under_a_running_store_is_kept_as_bad_before_it_is_replaced() {
    let (backend, dir) = (Backend::new(), tempfile::tempdir().unwrap());
    installed(&backend, dir.path());
    let store = launch(&backend, dir.path());
    assert!(store.get(Field::ActiveProvider).unwrap().is_some());
    std::fs::write(dir.path().join(SETTINGS_FILE), "{ not json").unwrap();

    store.put(Field::Model("openai"), "gpt-x").unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.path().join(format!("{SETTINGS_FILE}.bad"))).unwrap(),
        "{ not json"
    );
    let json = settings_json(dir.path());
    assert_eq!(json["providers"]["openai"]["model"], "gpt-x");
    assert_eq!(json["providers"]["anthropic"]["model"], "claude-a-model");
}

/// Finding 4 of round two. The text of the sign-in's `TooLong` holds whether
/// the keys fill the credential or the sign-in alone is too long for it.
#[test]
fn the_sign_in_too_long_text_holds_when_no_key_is_saved_too() {
    let text = crate::chatgpt::store::SecretStoreError::TooLong.to_string();
    assert!(text.contains("if other keys are saved"), "{text}");
    assert!(text.contains("may make room"), "{text}");
    assert!(!text.contains("no longer fit"), "{text}");
}

/// A migration that cannot read the `secrets` item cannot tell whether it is
/// marked, so it must not fall back to the per-slot items: if it is marked, a
/// leftover would be served over the newer key, and a key saved per slot would
/// be deleted by the next launch. Reads reject instead, and are asked again.
#[test]
fn a_migration_that_cannot_read_the_item_rejects_rather_than_serving_leftovers() {
    let (backend, dir) = (Backend::new(), tempfile::tempdir().unwrap());
    backend.seed(
        SECRETS_ACCOUNT,
        &format!(r#"{{"{MIGRATED_MARKER}":"1","anthropic_api_key":"key-new"}}"#),
    );
    backend.seed("anthropic_api_key", "key-old");
    backend
        .down
        .lock()
        .unwrap()
        .insert(SECRETS_ACCOUNT.to_string());

    let store = launch(&backend, dir.path());
    assert!(
        store.get(Field::ApiKey("anthropic")).is_err(),
        "a leftover is never served over an item that could not be read"
    );
    assert!(store.put(Field::ApiKey("anthropic"), "key-typed").is_err());
    assert_eq!(
        backend.item("anthropic_api_key").as_deref(),
        Some("key-old")
    );

    // Once the item can be read, the marked item wins and nothing was lost.
    backend.down.lock().unwrap().clear();
    let next = launch(&backend, dir.path());
    assert_eq!(
        next.get(Field::ApiKey("anthropic")).unwrap().as_deref(),
        Some("key-new")
    );
}

/// The same when there is no leftover and the settings file cannot be
/// written: the check for the marker cannot read the item either.
#[test]
fn an_unreadable_item_and_an_unwritable_settings_file_reject_rather_than_fall_back() {
    let (backend, dir) = (Backend::new(), tempfile::tempdir().unwrap());
    backend.seed(
        SECRETS_ACCOUNT,
        &format!(r#"{{"{MIGRATED_MARKER}":"1","anthropic_api_key":"key-a"}}"#),
    );
    block_settings_writes(dir.path());
    backend
        .down
        .lock()
        .unwrap()
        .insert(SECRETS_ACCOUNT.to_string());

    let store = launch(&backend, dir.path());
    assert!(store.get(Field::ApiKey("anthropic")).is_err());
    assert!(store.put(Field::ApiKey("anthropic"), "key-typed").is_err());
    assert!(
        backend.item("anthropic_api_key").is_none(),
        "nothing saved per slot"
    );
}
