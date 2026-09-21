//! Settings' provider radio group must match the provider registry
//! (#225 §3).
//!
//! `Settings.tsx` hardcodes a `PROVIDERS` array, because the UI needs
//! more per provider than the registry carries — a display label, a key
//! placeholder, the URL where you get a key, and whether it
//! authenticates at all. That array is the right place for those.
//!
//! What it must not do is disagree about *which providers exist*. Two
//! ways to be wrong, and both are silent:
//!
//! * an id in the registry and not in the array — the provider ships,
//!   and there is no way to select it. That is #225's whole subject:
//!   a capability fully plumbed with nothing to reach it by.
//! * an id in the array and not in the registry — the radio is offered,
//!   `set_active_provider` takes it, and the first message fails.
//!
//! `list_providers` exists on the bridge for this and returns nothing
//! but `SUPPORTED_PROVIDER_IDS`, so calling it at runtime would buy
//! async complexity and a fail-open branch for information the build
//! already has. Pinning the two lists here is the cheaper, stricter
//! answer — the same trade as `website_provider_claims.rs`, which
//! checks the marketing copy against this same constant.

use std::path::PathBuf;

use ai::SUPPORTED_PROVIDER_IDS;

fn settings_source() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../apps/desktop/src/components/Settings.tsx");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

/// The ids in the `PROVIDERS` array literal.
///
/// Scoped to that array rather than scanning the file: every id also
/// appears in `DEFAULT_MODEL_BY_PROVIDER` just above it, and a check
/// that counted those would pass on a provider the radio group never
/// renders — which is the bug, not the fix.
fn provider_ids_in_settings(src: &str) -> Vec<String> {
    let start = src
        .find("const PROVIDERS")
        .unwrap_or_else(|| panic!("no `const PROVIDERS` in Settings.tsx — has it been renamed?"));
    let rest = &src[start..];
    let end = rest
        .find("\n];")
        .unwrap_or_else(|| panic!("`PROVIDERS` array is not terminated by `\n];`"));
    let array = &rest[..end];

    let mut ids = Vec::new();
    for (idx, _) in array.match_indices("id: \"") {
        let after = &array[idx + 5..];
        if let Some(close) = after.find('"') {
            ids.push(after[..close].to_string());
        }
    }
    ids
}

#[test]
fn settings_offers_exactly_the_registry_providers() {
    let src = settings_source();
    let found = provider_ids_in_settings(&src);

    // A parse that silently matched nothing would make every
    // comparison below vacuous — the failure mode this repo keeps
    // running into.
    assert!(
        found.len() >= 2,
        "parsed {} provider ids out of Settings.tsx; the array shape must have changed, and \
         everything below is meaningless until this is fixed",
        found.len()
    );

    // Inclusion in both directions is not an exact list. A second
    // `id: "anthropic"` leaves `missing` and `extra` both empty, so the
    // checks below would pass while the radio group renders the
    // provider twice — two rows and two React keys for one thing.
    // Raised in review on #324.
    let mut seen = std::collections::BTreeSet::new();
    let dupes: Vec<_> = found.iter().filter(|id| !seen.insert(*id)).collect();
    assert!(
        dupes.is_empty(),
        "Settings lists these provider ids more than once, so the radio group renders \
         duplicate rows and duplicate React keys for them: {dupes:?}"
    );
    assert_eq!(
        found.len(),
        SUPPORTED_PROVIDER_IDS.len(),
        "Settings lists {} provider ids and the registry has {}; the two lists have the same \
         members but not the same length, which only a repeat can do",
        found.len(),
        SUPPORTED_PROVIDER_IDS.len()
    );

    let missing: Vec<_> = SUPPORTED_PROVIDER_IDS
        .iter()
        .filter(|id| !found.iter().any(|f| f == *id))
        .collect();
    assert!(
        missing.is_empty(),
        "these providers are in the registry but Settings offers no radio for them, so they \
         ship with no way to select them: {missing:?}"
    );

    let extra: Vec<_> = found
        .iter()
        .filter(|f| !SUPPORTED_PROVIDER_IDS.contains(&f.as_str()))
        .collect();
    assert!(
        extra.is_empty(),
        "Settings offers these providers and the registry does not have them, so selecting one \
         is accepted and then fails on the first message: {extra:?}"
    );
}

/// The default has to be one of them.
///
/// It is what a profile with no stored preference lands on, and what
/// the initial-state reader falls back to when `localStorage` holds
/// something unrecognised.
#[test]
fn the_default_provider_is_a_real_one() {
    let src = settings_source();
    let start = src
        .find("const DEFAULT_PROVIDER: ProviderId = \"")
        .expect("no DEFAULT_PROVIDER in Settings.tsx");
    let after = &src[start + "const DEFAULT_PROVIDER: ProviderId = \"".len()..];
    let id = &after[..after.find('"').expect("unterminated DEFAULT_PROVIDER")];

    assert!(
        SUPPORTED_PROVIDER_IDS.contains(&id),
        "DEFAULT_PROVIDER is {id:?}, which the registry does not have: {SUPPORTED_PROVIDER_IDS:?}"
    );
}
