//! The keychain against a real OS store (#394).
//!
//! Every test here is `#[ignore]`: they write to the login keychain of
//! whoever runs them, and on Linux they need a session bus with a
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
//! The pure store logic (fallback, migration, clearing) is covered by the
//! unit tests in `src/keychain.rs`, which need none of this.

use ai::anthropic::Effort;
use ai::keychain;

/// A provider id no real provider has, per process and per test, so a run
/// cannot touch a user's own setting, collide with a parallel one, or
/// collide with another test of this file: `cargo test` runs them on
/// separate threads, and two sharing a slot would race on it.
fn test_provider(tag: &str) -> String {
    format!("test-{}-{tag}", std::process::id())
}

/// The slot itself, through the OS keychain: save, read, overwrite,
/// delete, and deleting again.
#[test]
#[ignore = "writes to the real OS keychain"]
fn the_effort_slot_round_trips_through_the_keychain() {
    let id = test_provider("effort");

    keychain::save_effort(&id, Effort::High).expect("save");
    assert_eq!(keychain::load_effort(&id), Some(Effort::High));
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
    keychain::delete_effort(&id).expect("deleting a missing slot is success");
}

#[cfg(target_os = "linux")]
mod secret_service {
    use ai::keychain::{self, Persistence};

    use super::test_provider;

    /// The slot an earlier build, or the session-only fallback, would use.
    fn kernel_copy(account: &str) -> keyring::Result<keyring::Entry> {
        keyring::keyutils::KeyutilsCredential::new_with_target(None, "app.edytlab.desktop", account)
            .map(|c| keyring::Entry::new_with_credential(Box::new(c)))
    }

    #[test]
    #[ignore = "needs a running, unlocked Secret Service"]
    fn a_saved_key_lands_in_the_secret_service_and_clears_again() {
        let id = test_provider("saved");
        let account = format!("{id}_api_key");

        keychain::save_api_key(&id, "sk-test-value").expect("save");
        assert_eq!(
            keychain::persistence(),
            Persistence::Durable,
            "a Secret Service is running, so this is the durable store"
        );
        assert!(
            matches!(
                kernel_copy(&account).and_then(|e| e.get_password()),
                Err(keyring::Error::NoEntry)
            ),
            "nothing was left in the kernel keyring"
        );
        assert_eq!(
            keychain::try_load_api_key(&id).expect("read").as_deref(),
            Some("sk-test-value")
        );

        keychain::delete_api_key(&id).expect("clear");
        assert_eq!(keychain::try_load_api_key(&id).expect("read"), None);
    }

    #[test]
    #[ignore = "needs a running, unlocked Secret Service"]
    fn a_key_an_earlier_build_left_in_the_kernel_keyring_is_moved_over() {
        let id = test_provider("migrated");
        let account = format!("{id}_api_key");

        let kernel = kernel_copy(&account).expect("kernel keyring");
        kernel.set_password("sk-from-before").expect("seed");

        assert_eq!(
            keychain::try_load_api_key(&id).expect("read").as_deref(),
            Some("sk-from-before")
        );
        assert!(
            matches!(kernel.get_password(), Err(keyring::Error::NoEntry)),
            "the kernel copy is gone once moved"
        );
        assert_eq!(
            keychain::try_load_api_key(&id).expect("read").as_deref(),
            Some("sk-from-before"),
            "and the value is now in the Secret Service"
        );

        keychain::delete_api_key(&id).expect("clear");
    }
}
