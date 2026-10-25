//! The keychain against a real OS store (#394).
//!
//! The tests of the secrets item are `#[ignore]`: they write to the login
//! keychain of whoever runs them, and on Linux they need a session bus with a
//! running, unlocked Secret Service (GNOME Keyring, KWallet, KeePassXC),
//! which a CI container does not have. Run them on a desktop, or under a
//! throwaway one. `gnome-keyring-daemon --unlock` writes
//! `$XDG_DATA_HOME/keyrings/login.keyring`, so give the daemon a
//! temporary `XDG_DATA_HOME` or the run leaves a keyring in your home:
//!
//! ```text
//! dbus-run-session -- sh -c \
//!   'echo -n dev | XDG_DATA_HOME="$(mktemp -d)" \
//!      gnome-keyring-daemon --unlock --components=secrets; \
//!    cargo test -p ai --test keychain_secret_service -- --ignored'
//! ```
//!
//! Secrets are one item of the store (account `secrets`, a JSON map) and
//! settings are a file (#734), so these tests register a settings folder of
//! their own first, with a finished migration in it: they must never read
//! (and so migrate) the per-slot items of the developer's real edytlab.
//!
//! The pure store logic (fallback, moving a kernel-keyring copy across,
//! clearing, the migration, the cache) is covered by the unit tests in
//! `src/keychain.rs` and `src/keychain_layout_tests.rs`, which need none of
//! this. (A test here cannot seed a kernel-keyring copy behind the store's
//! back and expect to see it: the item is read once per process and cached.)
//! The model, effort and base-URL tests touch no keychain, so they are not
//! ignored.

use ai::anthropic::Effort;
use ai::keychain;

/// A provider id no real provider has, per process and per test, so a run
/// cannot touch a user's own setting, collide with a parallel one, or
/// collide with another test of this file: `cargo test` runs them on
/// separate threads, and two sharing a slot would race on it.
fn test_provider(tag: &str) -> String {
    register_settings_folder();
    format!("test-{}-{tag}", std::process::id())
}

/// Register a folder holding a settings file of a finished migration, once.
/// The folder is left in the temp directory.
fn register_settings_folder() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let dir =
            std::env::temp_dir().join(format!("edytlab-keychain-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp folder");
        std::fs::write(dir.join(keychain::SETTINGS_FILE), r#"{"version":1}"#).expect("seed");
        assert!(keychain::set_settings_dir(dir), "registered once");
    });
}

/// The slot itself, through the settings file: save, read, overwrite,
/// delete, and deleting again.
#[test]
fn the_effort_slot_round_trips_through_the_settings_file() {
    let id = test_provider("effort");

    keychain::save_effort(&id, Effort::High).expect("save");
    assert_eq!(keychain::load_effort(&id), Some(Effort::High));
    assert_eq!(keychain::try_load_effort(&id).unwrap(), Some(Effort::High));
    keychain::save_effort(&id, Effort::XHigh).expect("overwrite");
    assert_eq!(
        keychain::load_effort(&id),
        Some(Effort::XHigh),
        "overwrites"
    );
    keychain::delete_effort(&id).expect("clear");
    assert_eq!(
        keychain::load_effort(&id),
        None,
        "deleted slot reads as default"
    );
    assert_eq!(keychain::try_load_effort(&id).unwrap(), None);
    keychain::delete_effort(&id).expect("deleting a missing slot is success");
}

/// The `try_` reader answers what the `Option` one does while the store
/// works; the difference is only in a store that cannot be read.
#[test]
fn the_base_url_slot_round_trips_through_the_settings_file() {
    let id = test_provider("base-url");

    assert_eq!(keychain::try_load_base_url(&id).unwrap(), None);
    keychain::save_base_url(&id, "http://localhost:11434/v1").expect("save");
    assert_eq!(
        keychain::try_load_base_url(&id).unwrap().as_deref(),
        Some("http://localhost:11434/v1")
    );
    assert_eq!(
        keychain::load_base_url(&id).as_deref(),
        Some("http://localhost:11434/v1")
    );
    keychain::delete_base_url(&id).expect("clear");
    assert_eq!(keychain::try_load_base_url(&id).unwrap(), None);
    keychain::delete_base_url(&id).expect("deleting a missing slot is success");
}

#[cfg(target_os = "linux")]
mod secret_service {
    use ai::keychain::{self, Persistence};

    use super::test_provider;

    /// The kernel-keyring copy of an account, which a session-only save or an
    /// earlier build would leave.
    fn kernel_copy(account: &str) -> keyring::Result<keyring::Entry> {
        keyring::keyutils::KeyutilsCredential::new_with_target(None, "app.edytlab.desktop", account)
            .map(|c| keyring::Entry::new_with_credential(Box::new(c)))
    }

    #[test]
    #[ignore = "needs a running, unlocked Secret Service"]
    fn a_saved_key_lands_in_the_secrets_item_and_clears_again() {
        let id = test_provider("saved");

        assert_eq!(
            keychain::save_api_key(&id, "key-test-value").expect("save"),
            Persistence::Durable,
            "the save says where it went"
        );
        assert_eq!(
            keychain::persistence(),
            Persistence::Durable,
            "a Secret Service is running, so this is the durable store"
        );
        assert!(
            matches!(
                kernel_copy("secrets").and_then(|e| e.get_password()),
                Err(keyring::Error::NoEntry)
            ),
            "nothing was left in the kernel keyring"
        );
        assert!(
            matches!(
                kernel_copy(&format!("{id}_api_key")).and_then(|e| e.get_password()),
                Err(keyring::Error::NoEntry)
            ),
            "and no item of its own was made for the key"
        );
        assert_eq!(
            keychain::try_load_api_key(&id).expect("read").as_deref(),
            Some("key-test-value")
        );

        // A second key and the ChatGPT token share the item and do not
        // disturb each other.
        let other = test_provider("saved-other");
        keychain::save_api_key(&other, "key-other").expect("save");
        keychain::delete_api_key(&id).expect("clear");
        assert_eq!(keychain::try_load_api_key(&id).expect("read"), None);
        assert_eq!(
            keychain::try_load_api_key(&other).expect("read").as_deref(),
            Some("key-other")
        );
        keychain::delete_api_key(&other).expect("clear");
    }
}
