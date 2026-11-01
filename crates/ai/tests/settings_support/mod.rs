//! What the two tests that read `Settings.tsx` share
//! (`settings_provider_list.rs`, `settings_default_models.rs`).

// Each test binary uses a subset.
#![allow(dead_code)]

use std::path::PathBuf;
use std::sync::LazyLock;

/// Providers in the registry that Settings does not offer yet. None now: the
/// ChatGPT plan's card landed in #474 PR 6, so this test checks its row too.
/// The site copy has its own list (`website_provider_claims.rs`, PR 7).
pub const NOT_OFFERED_YET: &[&str] = &[];

/// The registry's ids, less those not offered yet.
pub static SUPPORTED_PROVIDER_IDS: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    ai::SUPPORTED_PROVIDER_IDS
        .iter()
        .copied()
        .filter(|id| !NOT_OFFERED_YET.contains(id))
        .collect()
});

pub fn settings_source() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../apps/desktop/src/components/Settings.tsx");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}
