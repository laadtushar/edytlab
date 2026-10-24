//! The model Settings shows for a provider must be the one the backend uses
//! (#468).
//!
//! `Settings.tsx` keeps `DEFAULT_MODEL_BY_PROVIDER`: the placeholder of the
//! model field, and what the field holds for a provider the user has not
//! chosen a model for. The backend has its own answer, `default_model()`,
//! which is what the agent really sends. Two copies of one fact drift, and
//! when they do the screen names a model the app is not using, or the other
//! way round. The same trade as `settings_provider_list.rs`: pin the two
//! here, so neither can move alone.
//!
//! It passes on a tree where both name the same model, whichever that is,
//! and fails when one side is changed without the other.

use std::path::PathBuf;

use ai::provider::provider_from_id;
use std::sync::LazyLock;

/// Providers in the registry that Settings does not offer yet. None now: the
/// ChatGPT plan's card landed in #474 PR 6, so this test checks its row too.
/// The site copy has its own list (`website_provider_claims.rs`, PR 7).
const NOT_OFFERED_YET: &[&str] = &[];

/// The registry's ids, less those not offered yet.
static SUPPORTED_PROVIDER_IDS: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    ai::SUPPORTED_PROVIDER_IDS
        .iter()
        .copied()
        .filter(|id| !NOT_OFFERED_YET.contains(id))
        .collect()
});

fn settings_source() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../apps/desktop/src/components/Settings.tsx");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

/// The `id: "model"` pairs of the `DEFAULT_MODEL_BY_PROVIDER` object literal.
///
/// Scoped to that literal, which is one entry per line, so a parse that
/// matched nothing is caught by the length check in the test instead of
/// making every comparison vacuous.
fn defaults_in_settings(src: &str) -> Vec<(String, String)> {
    let start = src
        .find("const DEFAULT_MODEL_BY_PROVIDER")
        .unwrap_or_else(|| {
            panic!("no `const DEFAULT_MODEL_BY_PROVIDER` in Settings.tsx; renamed?")
        });
    let rest = &src[start..];
    let open = rest
        .find('{')
        .expect("no `{` after DEFAULT_MODEL_BY_PROVIDER");
    let close = rest[open..]
        .find("\n};")
        .unwrap_or_else(|| panic!("DEFAULT_MODEL_BY_PROVIDER is not terminated by `\\n}};`"));
    let body = &rest[open + 1..open + close];

    body.lines()
        .filter_map(|line| {
            let (key, value) = line.trim().split_once(':')?;
            let value = value.trim().trim_end_matches(',');
            let value = value.strip_prefix('"')?.strip_suffix('"')?;
            Some((key.trim().to_string(), value.to_string()))
        })
        .collect()
}

#[test]
fn settings_names_the_model_the_backend_uses_for_every_provider() {
    let found = defaults_in_settings(&settings_source());

    assert_eq!(
        found.len(),
        SUPPORTED_PROVIDER_IDS.len(),
        "parsed {} defaults out of Settings.tsx for {} providers; the literal must have changed \
         shape, and every comparison below is meaningless until this is fixed: {found:?}",
        found.len(),
        SUPPORTED_PROVIDER_IDS.len()
    );

    for id in SUPPORTED_PROVIDER_IDS.iter() {
        let provider = provider_from_id(id);
        let sent = provider.translate_model(provider.default_model());
        let shown = found
            .iter()
            .find(|(k, _)| k == id)
            .map(|(_, v)| v.as_str())
            .unwrap_or_else(|| panic!("Settings.tsx has no default model for `{id}`"));
        assert_eq!(
            shown, sent,
            "{id}: Settings shows `{shown}` as the default model and the backend sends `{sent}`"
        );
    }
}
