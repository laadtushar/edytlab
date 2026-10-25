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

    fn set(&self, value: &str) -> keyring::Result<()> {
        self.log(Op::Set);
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
    assert_eq!(backend.secrets().len(), 4);
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

#[test]
fn a_settings_file_that_cannot_be_read_leaves_the_old_items_in_use() {
    let (backend, dir) = (Backend::new(), tempfile::tempdir().unwrap());
    seed_legacy(&backend);
    std::fs::create_dir(dir.path().join(SETTINGS_FILE)).unwrap();
    let store = launch(&backend, dir.path());
    assert_eq!(everything(&store), expected_after_migration());
    assert_eq!(backend.item(SECRETS_ACCOUNT), None);
    assert_eq!(
        backend.item("anthropic_api_key").as_deref(),
        Some("key-anthropic")
    );
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
    assert_eq!(backend.secrets().len(), 4);
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
        1,
        "read once, however many saves"
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

    // Removing what is not there writes nothing.
    backend.forget_calls();
    store.remove(Field::ApiKey("openai")).unwrap();
    assert_eq!(backend.total_calls(), 0);

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
