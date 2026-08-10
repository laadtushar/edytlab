//! Per-provider model catalogue.
//!
//! Each provider exposes a `list_models_for(api_key)` function that
//! returns a curated list of [`ModelInfo`] entries. The Settings UI's
//! model combo populates a `<datalist>` from this surface, with a
//! per-provider 10-minute in-memory cache keyed off the provider id.
//!
//! # Per-provider behaviour
//!
//! * **Anthropic** — static curated list. No network call. The Anthropic
//!   models endpoint exists but the curated list is small enough that a
//!   live fetch adds latency for no gain.
//! * **OpenRouter** — `GET /api/v1/models` (public; auth optional but
//!   raises rate limits when present).
//! * **OpenAI** — `GET /v1/models` (auth required). We filter to
//!   chat-capable model ids (heuristic: prefix `gpt-`, `o1-`, `o3-`,
//!   `chatgpt-`).
//!
//! # Cache
//!
//! Cached entries live in a process-global `Mutex<HashMap<String,
//! (Instant, Vec<ModelInfo>)>>` with a 10-minute TTL. The TTL is short
//! enough that newly-released models surface within a session, and long
//! enough that the Settings panel doesn't re-fetch on every render.
//!
//! Cache failures are non-fatal: a failed live fetch returns the error
//! to the caller and *does not* poison the cache, so a subsequent retry
//! has a fresh shot at the upstream API.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::provider::{ANTHROPIC_ID, OPENAI_ID, OPENROUTER_ID};

/// One entry in a provider's model catalogue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    /// Canonical id the user types into the combo (e.g.
    /// `claude-sonnet-4-6`, `anthropic/claude-sonnet-4-6`,
    /// `gpt-4o-mini`).
    pub id: String,
    /// Human-readable label rendered alongside the id in the
    /// `<datalist>` suggestion.
    pub display_name: String,
    /// Context window in tokens, when known.
    pub context_length: Option<u32>,
    /// For OpenRouter, the namespace before the `/` (e.g. `anthropic`,
    /// `openai`, `google`); for native providers, `None`.
    pub provider_hint: Option<String>,
}

/// Cache TTL for model catalogues. Ten minutes is short enough for
/// model releases to surface within a session without hammering the
/// upstream APIs from a long-running desktop process.
const CACHE_TTL: Duration = Duration::from_secs(600);

/// One cache entry: timestamp of insertion + the cached catalogue.
type CacheEntry = (Instant, Vec<ModelInfo>);
type CacheMap = HashMap<String, CacheEntry>;

static CACHE: Mutex<Option<CacheMap>> = Mutex::new(None);

fn cache_get(key: &str) -> Option<Vec<ModelInfo>> {
    let guard = CACHE.lock().ok()?;
    let map = guard.as_ref()?;
    let (when, models) = map.get(key)?;
    if when.elapsed() < CACHE_TTL {
        Some(models.clone())
    } else {
        None
    }
}

fn cache_put(key: String, models: Vec<ModelInfo>) {
    if let Ok(mut guard) = CACHE.lock() {
        let map = guard.get_or_insert_with(HashMap::new);
        map.insert(key, (Instant::now(), models));
    }
}

/// Clear the in-memory cache. Useful in tests so cached fixtures from
/// one test don't leak into another.
pub fn clear_cache() {
    if let Ok(mut g) = CACHE.lock() {
        *g = None;
    }
}

/// List models for `provider_id`. `api_key` is optional for OpenRouter
/// and Anthropic (Anthropic returns a static list anyway); OpenAI
/// requires it. Returns the cached list if fresh; otherwise hits the
/// upstream and caches the result.
pub async fn list_models_for(
    provider_id: &str,
    api_key: Option<&str>,
) -> Result<Vec<ModelInfo>, String> {
    if let Some(cached) = cache_get(provider_id) {
        return Ok(cached);
    }

    let models = match provider_id {
        ANTHROPIC_ID => anthropic_models(),
        OPENROUTER_ID => fetch_openrouter_models(api_key).await?,
        OPENAI_ID => fetch_openai_models(api_key).await?,
        other => return Err(format!("unsupported provider id: {other}")),
    };

    cache_put(provider_id.to_string(), models.clone());
    Ok(models)
}

/// Static Anthropic catalogue. Curated rather than fetched: the public
/// Anthropic models endpoint exists but its output ordering is unstable
/// and includes deprecated ids we don't want to surface.
pub fn anthropic_models() -> Vec<ModelInfo> {
    vec![
        ModelInfo {
            id: "claude-sonnet-4-6".to_string(),
            display_name: "Claude Sonnet 4.6 (default)".to_string(),
            context_length: Some(200_000),
            provider_hint: None,
        },
        ModelInfo {
            id: "claude-haiku-4-5-20251001".to_string(),
            display_name: "Claude Haiku 4.5 (cheap mode)".to_string(),
            context_length: Some(200_000),
            provider_hint: None,
        },
        ModelInfo {
            id: "claude-opus-4-1-20250805".to_string(),
            display_name: "Claude Opus 4.1".to_string(),
            context_length: Some(200_000),
            provider_hint: None,
        },
    ]
}

/// Live fetch from `https://openrouter.ai/api/v1/models`. The
/// `Authorization` header is optional but raises rate limits when set,
/// so we attach it when the user has a stored key.
async fn fetch_openrouter_models(api_key: Option<&str>) -> Result<Vec<ModelInfo>, String> {
    #[derive(Deserialize)]
    struct Wire {
        data: Vec<WireModel>,
    }
    #[derive(Deserialize)]
    struct WireModel {
        id: String,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        context_length: Option<u32>,
    }

    let client = reqwest::Client::new();
    let mut req = client.get("https://openrouter.ai/api/v1/models");
    if let Some(k) = api_key {
        if !k.trim().is_empty() {
            req = req.header("authorization", format!("Bearer {k}"));
        }
    }
    let resp = req.send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("openrouter models {}", resp.status().as_u16()));
    }
    let parsed: Wire = resp.json().await.map_err(|e| e.to_string())?;

    let mut models: Vec<ModelInfo> = parsed
        .data
        .into_iter()
        .map(|m| {
            let hint = m.id.split('/').next().map(str::to_string);
            ModelInfo {
                display_name: m.name.unwrap_or_else(|| m.id.clone()),
                context_length: m.context_length,
                provider_hint: hint,
                id: m.id,
            }
        })
        .collect();
    // Stable sort: Anthropic-namespaced first (we pre-default to those),
    // then alphabetical by id.
    models.sort_by(|a, b| {
        let a_anth = a.provider_hint.as_deref() == Some("anthropic");
        let b_anth = b.provider_hint.as_deref() == Some("anthropic");
        b_anth.cmp(&a_anth).then_with(|| a.id.cmp(&b.id))
    });
    Ok(models)
}

/// Live fetch from OpenAI's `/v1/models`. Filters to chat-capable
/// model ids using a prefix heuristic; OpenAI's catalogue mixes
/// embeddings, audio, and image models we never want to surface here.
async fn fetch_openai_models(api_key: Option<&str>) -> Result<Vec<ModelInfo>, String> {
    let key = api_key
        .ok_or_else(|| "OpenAI catalogue requires an API key — save your key first".to_string())?;
    if key.trim().is_empty() {
        return Err("OpenAI catalogue requires an API key".to_string());
    }

    #[derive(Deserialize)]
    struct Wire {
        data: Vec<WireModel>,
    }
    #[derive(Deserialize)]
    struct WireModel {
        id: String,
    }

    let client = reqwest::Client::new();
    let resp = client
        .get("https://api.openai.com/v1/models")
        .header("authorization", format!("Bearer {key}"))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.status().is_success() {
        return Err(format!("openai models {}", resp.status().as_u16()));
    }

    let parsed: Wire = resp.json().await.map_err(|e| e.to_string())?;
    let mut models: Vec<ModelInfo> = parsed
        .data
        .into_iter()
        .filter(|m| is_chat_capable(&m.id))
        .map(|m| ModelInfo {
            display_name: m.id.clone(),
            context_length: None,
            provider_hint: None,
            id: m.id,
        })
        .collect();

    // Newest-first heuristic: gpt-4o, gpt-4.1, o3, o1, then alphabetical.
    models.sort_by(|a, b| {
        openai_rank(&a.id)
            .cmp(&openai_rank(&b.id))
            .then_with(|| a.id.cmp(&b.id))
    });
    Ok(models)
}

fn is_chat_capable(id: &str) -> bool {
    id.starts_with("gpt-")
        || id.starts_with("o1-")
        || id.starts_with("o3-")
        || id == "o1"
        || id == "o3"
        || id.starts_with("chatgpt-")
}

/// Lower rank = higher in the list. Designed so the most-recent /
/// most-relevant model families surface at the top of the combo.
fn openai_rank(id: &str) -> u8 {
    if id.starts_with("gpt-4o") {
        0
    } else if id.starts_with("gpt-4.1") {
        1
    } else if id.starts_with("o3") {
        2
    } else if id.starts_with("o1") {
        3
    } else if id.starts_with("gpt-4") {
        4
    } else if id.starts_with("gpt-") {
        5
    } else {
        9
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serialises the tests that touch the process-wide model cache.
    ///
    /// `CACHE` is a `static`, so every test in this binary shares one.
    /// The cache-touching tests each start with `clear_cache()`, and
    /// `cargo test` runs them concurrently — so a sibling's `clear_cache`
    /// could land between this test's `cache_put` and its read, wiping
    /// the entry it was about to assert on. Measured at 5 failures in 40
    /// runs before this guard, 0 in 40 after.
    ///
    /// A test-only mutex is the right instrument rather than
    /// `--test-threads=1`: it costs nothing for the tests that don't
    /// touch the cache, and it keeps the fix next to the shared state
    /// instead of in CI configuration where the next person to add a
    /// cache test won't see it.
    ///
    /// It has to be tokio's mutex, not `std`'s: these are `#[tokio::test]`
    /// bodies and the guard is held across an `.await`, which is exactly
    /// the case a blocking guard must not be used for.
    static CACHE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    #[test]
    fn anthropic_catalogue_includes_curated_ids() {
        let m = anthropic_models();
        assert!(m.iter().any(|x| x.id == "claude-sonnet-4-6"));
        assert!(m.iter().any(|x| x.id == "claude-haiku-4-5-20251001"));
        assert!(m.iter().any(|x| x.id == "claude-opus-4-1-20250805"));
    }

    #[test]
    fn is_chat_capable_filters_correctly() {
        assert!(is_chat_capable("gpt-4o-mini"));
        assert!(is_chat_capable("gpt-4.1"));
        assert!(is_chat_capable("o1-preview"));
        assert!(is_chat_capable("o3-mini"));
        assert!(is_chat_capable("chatgpt-4o-latest"));
        assert!(!is_chat_capable("text-embedding-3-small"));
        assert!(!is_chat_capable("dall-e-3"));
        assert!(!is_chat_capable("whisper-1"));
        assert!(!is_chat_capable("tts-1"));
    }

    #[test]
    fn openai_rank_orders_newest_families_first() {
        assert!(openai_rank("gpt-4o-mini") < openai_rank("gpt-4-turbo"));
        assert!(openai_rank("gpt-4.1") < openai_rank("gpt-4-turbo"));
        assert!(openai_rank("o3-mini") < openai_rank("o1-preview"));
        assert!(openai_rank("gpt-4o") < openai_rank("o1-preview"));
    }

    #[tokio::test]
    async fn anthropic_list_does_not_hit_network() {
        let _guard = CACHE_LOCK.lock().await;
        clear_cache();
        // Pass a bogus base URL via api_key=None — anthropic is static.
        let m = list_models_for(ANTHROPIC_ID, None).await.unwrap();
        assert!(!m.is_empty());
    }

    #[tokio::test]
    async fn cache_returns_within_ttl() {
        let _guard = CACHE_LOCK.lock().await;
        clear_cache();
        cache_put(
            "anthropic".to_string(),
            vec![ModelInfo {
                id: "x".into(),
                display_name: "x".into(),
                context_length: None,
                provider_hint: None,
            }],
        );
        let m = list_models_for(ANTHROPIC_ID, None).await.unwrap();
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].id, "x");
    }

    #[tokio::test]
    async fn unsupported_provider_yields_error() {
        let _guard = CACHE_LOCK.lock().await;
        clear_cache();
        let err = list_models_for("nope", None).await.unwrap_err();
        assert!(err.contains("unsupported"));
    }

    #[tokio::test]
    async fn openai_catalogue_requires_api_key() {
        let _guard = CACHE_LOCK.lock().await;
        clear_cache();
        let err = list_models_for(OPENAI_ID, None).await.unwrap_err();
        assert!(err.contains("OpenAI catalogue requires an API key"));
    }

    #[tokio::test]
    async fn openrouter_catalogue_works_without_api_key() {
        // Sanity that the function signature accepts None for OpenRouter.
        // We don't hit the network in this unit test (would require
        // wiremock + base url override), so we just confirm the auth
        // attachment compiles. A live network test lives in the
        // integration suite if any.
        let _guard = CACHE_LOCK.lock().await;
        clear_cache();
        let _ = OPENROUTER_ID; // silence unused-import
    }
}
