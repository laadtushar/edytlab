//! The keychain against a real Secret Service (#394).
//!
//! Every test here is `#[ignore]`: they need a session bus with a
//! running, unlocked Secret Service (GNOME Keyring, KWallet, KeePassXC),
//! which a CI container does not have. Run them on a desktop, or under a
//! throwaway one:
//!
//! ```text
//! dbus-run-session -- sh -c \
//!   'echo -n dev | gnome-keyring-daemon --unlock --components=secrets; \
//!    cargo test -p ai --test keychain_secret_service -- --ignored'
//! ```
//!
//! The pure store logic (fallback, migration, clearing) is covered by the
//! unit tests in `src/keychain.rs`, which need none of this.
#![cfg(target_os = "linux")]

use ai::keychain::{self, Persistence};

/// A provider id no real provider has, per process, so a run cannot touch
/// a user's own setting or collide with a parallel one.
fn test_provider() -> String {
    format!("test-{}", std::process::id())
}

/// The slot an earlier build, or the session-only fallback, would use.
fn kernel_copy(account: &str) -> keyring::Result<keyring::Entry> {
    keyring::keyutils::KeyutilsCredential::new_with_target(None, "app.edytlab.desktop", account)
        .map(|c| keyring::Entry::new_with_credential(Box::new(c)))
}

#[test]
#[ignore = "needs a running, unlocked Secret Service"]
fn a_saved_key_lands_in_the_secret_service_and_clears_again() {
    let id = test_provider();
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
    let id = test_provider();
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
