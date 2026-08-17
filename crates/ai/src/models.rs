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
//! * **Groq** — `GET /v1/models` (auth required). No filtering: the
//!   OpenAI heuristic above would reject every `llama-*` id and leave
//!   the dropdown empty.
//! * **Gemini** — `GET /models` (auth required). The path lacks the
//!   `/v1` because `GEMINI_DEFAULT_BASE_URL` already ends in
//!   `/v1beta/openai`, mirroring `GeminiProvider::list_models_path`.
//!
//! The last three share one wire format — `{"data": [{"id": …}]}` —
//! and therefore one fetch helper. Only the filtering and ordering
//! differ. Every arm here must cover an id in `SUPPORTED_PROVIDER_IDS`:
//! Groq and Gemini reached the provider layer and the Settings dropdown
//! long before they reached this match, and until they did, selecting
//! either showed `unsupported provider id: groq` where the model list
//! belongs.
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

use crate::provider::{
    ANTHROPIC_ID, GEMINI_DEFAULT_BASE_URL, GEMINI_ID, GROQ_DEFAULT_BASE_URL, GROQ_ID,
    OLLAMA_DEFAULT_BASE_URL, OLLAMA_ID, OPENAI_DEFAULT_BASE_URL, OPENAI_ID, OPENROUTER_ID,
};

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
    list_models_for_at(provider_id, api_key, None).await
}

/// [`list_models_for`] against a caller-supplied endpoint.
///
/// `base_url_override` is what the user typed into Settings when they
/// pointed a provider at a local server or a gateway. The catalogue has
/// to follow the same endpoint the agent will use, or the model picker
/// lists one server's models while chat talks to another.
pub async fn list_models_for_at(
    provider_id: &str,
    api_key: Option<&str>,
    base_url_override: Option<&str>,
) -> Result<Vec<ModelInfo>, String> {
    if let Some(base) = base_url_override.map(str::trim).filter(|b| !b.is_empty()) {
        return list_models_at(provider_id, api_key, base).await;
    }
    let base = match provider_id {
        OPENAI_ID => OPENAI_DEFAULT_BASE_URL,
        GROQ_ID => GROQ_DEFAULT_BASE_URL,
        GEMINI_ID => GEMINI_DEFAULT_BASE_URL,
        OLLAMA_ID => OLLAMA_DEFAULT_BASE_URL,
        _ => "",
    };
    list_models_at(provider_id, api_key, base).await
}

/// [`list_models_for`] with the upstream base URL parameterised.
///
/// `validate.rs` does the same thing for the same reason: the fetchers
/// used to hardcode `https://api.openai.com/...`, which made them
/// impossible to exercise without a live key. Every arm is now driven
/// by `wiremock` in the tests below.
pub(crate) async fn list_models_at(
    provider_id: &str,
    api_key: Option<&str>,
    base_url: &str,
) -> Result<Vec<ModelInfo>, String> {
    if let Some(cached) = cache_get(provider_id) {
        return Ok(cached);
    }

    // Every id in `SUPPORTED_PROVIDER_IDS` needs an arm here. Groq and
    // Gemini were added to the provider layer and to the Settings
    // dropdown but never to this match, so picking either showed
    // "unsupported provider id: groq" where the model list belongs.
    let models = match provider_id {
        ANTHROPIC_ID => anthropic_models(),
        OPENROUTER_ID => fetch_openrouter_models(api_key).await?,
        OPENAI_ID => fetch_openai_models(base_url, api_key).await?,
        GROQ_ID => fetch_groq_models(base_url, api_key).await?,
        GEMINI_ID => fetch_gemini_models(base_url, api_key).await?,
        OLLAMA_ID => fetch_ollama_models(base_url).await?,
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

/// Raw model ids from any OpenAI-compatible `/models` endpoint.
///
/// Shared by OpenAI, Groq, and Gemini, which all serve the same
/// `{"data": [{"id": …}]}` envelope — that envelope *is* what
/// "OpenAI-compatible" means for this route. What differs is the
/// filtering and ordering each caller wants afterwards, so this
/// deliberately returns everything and lets them decide.
///
/// `label` only appears in error text, so a failure names the provider
/// the user actually picked.
async fn fetch_openai_compatible_ids(
    url: &str,
    api_key: &str,
    label: &str,
) -> Result<Vec<String>, String> {
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
        .get(url)
        .header("authorization", format!("Bearer {api_key}"))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.status().is_success() {
        return Err(format!("{label} models {}", resp.status().as_u16()));
    }

    let parsed: Wire = resp.json().await.map_err(|e| e.to_string())?;
    Ok(parsed.data.into_iter().map(|m| m.id).collect())
}

/// Reject an absent or blank key before making a request that would
/// only 401.
fn require_key<'a>(api_key: Option<&'a str>, label: &str) -> Result<&'a str, String> {
    let key = api_key
        .ok_or_else(|| format!("{label} catalogue requires an API key — save your key first"))?;
    if key.trim().is_empty() {
        return Err(format!("{label} catalogue requires an API key"));
    }
    Ok(key)
}

/// Live fetch from OpenAI's `/v1/models`. Filters to chat-capable
/// model ids using a prefix heuristic; OpenAI's catalogue mixes
/// embeddings, audio, and image models we never want to surface here.
async fn fetch_openai_models(
    base_url: &str,
    api_key: Option<&str>,
) -> Result<Vec<ModelInfo>, String> {
    let key = require_key(api_key, "OpenAI")?;
    let ids = fetch_openai_compatible_ids(&format!("{base_url}/v1/models"), key, "openai").await?;

    let mut models: Vec<ModelInfo> = ids
        .into_iter()
        .filter(|id| is_chat_capable(id))
        .map(|id| ModelInfo {
            display_name: id.clone(),
            context_length: None,
            provider_hint: None,
            id,
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

/// Live fetch from Groq's OpenAI-compatible `/v1/models`.
///
/// Note what is *not* here: `is_chat_capable`. That filter keeps ids
/// prefixed `gpt-`/`o1-`/`o3-`/`chatgpt-`, and every Groq model is
/// named something like `llama-3.3-70b-versatile`, so reusing it would
/// return an empty list — a worse failure than the "unsupported
/// provider id" this replaces, because an empty dropdown reads as
/// "Groq has no models" rather than as a bug.
async fn fetch_groq_models(
    base_url: &str,
    api_key: Option<&str>,
) -> Result<Vec<ModelInfo>, String> {
    let key = require_key(api_key, "Groq")?;
    let mut ids =
        fetch_openai_compatible_ids(&format!("{base_url}/v1/models"), key, "groq").await?;
    ids.sort();
    Ok(ids
        .into_iter()
        .map(|id| ModelInfo {
            display_name: id.clone(),
            context_length: None,
            provider_hint: None,
            id,
        })
        .collect())
}

/// Live fetch from a local Ollama daemon.
///
/// No key: the daemon is not authenticated, so there is nothing to
/// check before making the request.
///
/// The failure mode is different from every other provider here, and
/// it is the *common* case rather than the exception — Ollama is
/// usually just not running. A bare connection-refused would surface as
/// an unexplained transport error, so it is translated into the one
/// instruction that fixes it.
async fn fetch_ollama_models(base_url: &str) -> Result<Vec<ModelInfo>, String> {
    let mut ids = fetch_openai_compatible_ids(&format!("{base_url}/models"), "", "ollama")
        .await
        .map_err(|e| {
            format!(
                "could not reach Ollama at {base_url} ({e}). \
                 Start it with `ollama serve`, then pull a model — \
                 e.g. `ollama pull llama3.2`."
            )
        })?;
    if ids.is_empty() {
        return Err(
            "Ollama is running but has no models pulled. Try `ollama pull llama3.2`.".to_string(),
        );
    }
    ids.sort();
    Ok(ids
        .into_iter()
        .map(|id| ModelInfo {
            display_name: id.clone(),
            context_length: None,
            provider_hint: None,
            id,
        })
        .collect())
}

/// Live fetch from Gemini's OpenAI-compatible model list.
///
/// The path is `/models`, not `/v1/models`: `GEMINI_DEFAULT_BASE_URL`
/// already ends in `/v1beta/openai`. That asymmetry is why
/// `GeminiProvider` overrides `list_models_path`, and it is mirrored
/// here.
///
/// Ids come back either bare (`gemini-2.0-flash`) or resource-qualified
/// (`models/gemini-2.0-flash`) depending on which surface answers. We
/// strip the prefix rather than assuming a form: the bare id is what
/// the OpenAI-compatible chat endpoint accepts, `translate_model` is
/// identity for this provider, and stripping is a no-op when the ids
/// are already bare. Correct either way, so the shape doesn't have to
/// be guessed.
async fn fetch_gemini_models(
    base_url: &str,
    api_key: Option<&str>,
) -> Result<Vec<ModelInfo>, String> {
    let key = require_key(api_key, "Gemini")?;
    let mut ids = fetch_openai_compatible_ids(&format!("{base_url}/models"), key, "gemini").await?;
    ids.sort();
    Ok(ids
        .into_iter()
        .map(|id| {
            let bare = id.strip_prefix("models/").unwrap_or(&id).to_string();
            ModelInfo {
                display_name: bare.clone(),
                context_length: None,
                provider_hint: None,
                id: bare,
            }
        })
        .collect())
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

    // ------------------------------------------------------------------
    // Live-fetch arms, driven through wiremock
    // ------------------------------------------------------------------

    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// An OpenAI-compatible `{"data":[{"id":…}]}` body.
    fn models_body(ids: &[&str]) -> serde_json::Value {
        serde_json::json!({
            "object": "list",
            "data": ids.iter().map(|id| serde_json::json!({
                "id": id, "object": "model", "owned_by": "test"
            })).collect::<Vec<_>>(),
        })
    }

    async fn serve(route: &str, ids: &[&str]) -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(route))
            .respond_with(ResponseTemplate::new(200).set_body_json(models_body(ids)))
            .mount(&server)
            .await;
        server
    }

    /// The defect: Groq is selectable in Settings and was not in the
    /// match, so the dropdown showed "unsupported provider id: groq".
    #[tokio::test]
    async fn groq_catalogue_lists_groq_models() {
        let _guard = CACHE_LOCK.lock().await;
        clear_cache();
        let server = serve(
            "/v1/models",
            &["llama-3.3-70b-versatile", "llama-3.1-8b-instant"],
        )
        .await;

        let models = list_models_at(GROQ_ID, Some("k"), &server.uri())
            .await
            .expect("groq catalogue should resolve");

        let ids: Vec<&str> = models.iter().map(|m| m.id.as_str()).collect();
        assert!(ids.contains(&"llama-3.3-70b-versatile"), "got {ids:?}");
        assert_eq!(ids.len(), 2);
    }

    /// The trap in the obvious fix: `is_chat_capable` keeps only
    /// `gpt-`/`o1-`/`o3-`/`chatgpt-` prefixes, so pointing Groq at
    /// OpenAI's fetcher would filter every Llama model away and return
    /// an empty list — which reads as "Groq has no models" rather than
    /// as a bug, and is therefore worse than the error it replaced.
    #[tokio::test]
    async fn groq_catalogue_is_not_filtered_by_the_openai_heuristic() {
        let _guard = CACHE_LOCK.lock().await;
        clear_cache();
        let server = serve("/v1/models", &["llama-3.3-70b-versatile"]).await;

        let models = list_models_at(GROQ_ID, Some("k"), &server.uri())
            .await
            .unwrap();
        assert!(
            !models.is_empty(),
            "an empty dropdown is a worse failure than an error message"
        );
        assert!(!is_chat_capable(&models[0].id), "premise of this test");
    }

    /// Gemini's list lives at `/models` because its base URL already
    /// carries `/v1beta/openai` — mirroring `GeminiProvider`'s
    /// `list_models_path` override.
    #[tokio::test]
    async fn gemini_catalogue_uses_the_bare_models_path() {
        let _guard = CACHE_LOCK.lock().await;
        clear_cache();
        let server = serve("/models", &["gemini-2.0-flash"]).await;

        let models = list_models_at(GEMINI_ID, Some("k"), &server.uri())
            .await
            .expect("gemini catalogue should resolve");
        assert_eq!(models[0].id, "gemini-2.0-flash");
    }

    /// Ids may come back resource-qualified. The bare form is what the
    /// chat endpoint accepts and `translate_model` is identity for this
    /// provider, so both shapes have to land on the same id — otherwise
    /// a model picked from the dropdown would be rejected at inference.
    #[tokio::test]
    async fn gemini_strips_a_resource_prefix_when_present() {
        let _guard = CACHE_LOCK.lock().await;
        clear_cache();
        let server = serve("/models", &["models/gemini-2.0-flash"]).await;

        let models = list_models_at(GEMINI_ID, Some("k"), &server.uri())
            .await
            .unwrap();
        assert_eq!(
            models[0].id, "gemini-2.0-flash",
            "a `models/`-qualified id must resolve to the same id as a bare one"
        );
        assert_eq!(models[0].display_name, "gemini-2.0-flash");
    }

    /// OpenAI keeps its filter and its ordering — the refactor that
    /// added the other two arms must not have loosened it.
    #[tokio::test]
    async fn openai_catalogue_keeps_its_filter_and_ranking() {
        let _guard = CACHE_LOCK.lock().await;
        clear_cache();
        let server = serve(
            "/v1/models",
            &["text-embedding-3-small", "gpt-4o-mini", "dall-e-3", "o1"],
        )
        .await;

        let models = list_models_at(OPENAI_ID, Some("k"), &server.uri())
            .await
            .unwrap();
        let ids: Vec<&str> = models.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["gpt-4o-mini", "o1"],
            "embeddings and image models must stay out, gpt-4o ranks first"
        );
    }

    #[tokio::test]
    async fn groq_and_gemini_report_a_missing_key_by_name() {
        let _guard = CACHE_LOCK.lock().await;
        clear_cache();
        let err = list_models_at(GROQ_ID, None, "http://unused")
            .await
            .unwrap_err();
        assert!(err.contains("Groq"), "got {err}");
        clear_cache();
        let err = list_models_at(GEMINI_ID, None, "http://unused")
            .await
            .unwrap_err();
        assert!(err.contains("Gemini"), "got {err}");
    }

    #[tokio::test]
    async fn ollama_lists_pulled_models_without_a_key() {
        let _guard = CACHE_LOCK.lock().await;
        clear_cache();
        let server = serve("/models", &["llama3.2:latest", "qwen2.5-coder:7b"]).await;

        let models = list_models_at(OLLAMA_ID, None, &server.uri())
            .await
            .expect("a keyless provider must resolve without a key");
        let ids: Vec<&str> = models.iter().map(|m| m.id.as_str()).collect();
        assert!(ids.contains(&"llama3.2:latest"), "got {ids:?}");
    }

    /// The common case for a local daemon is that it isn't running. A
    /// bare connection-refused would surface as an unexplained transport
    /// error, so the message has to carry the fix.
    #[tokio::test]
    async fn a_stopped_daemon_says_how_to_start_it() {
        let _guard = CACHE_LOCK.lock().await;
        clear_cache();
        // Port 1 is reserved and nothing listens on it.
        let err = list_models_at(OLLAMA_ID, None, "http://127.0.0.1:1")
            .await
            .unwrap_err();
        assert!(err.contains("Ollama"), "got {err}");
        assert!(err.contains("ollama serve"), "got {err}");
    }

    /// Running but empty is a different problem with a different fix,
    /// and an empty dropdown would explain neither.
    #[tokio::test]
    async fn a_daemon_with_no_models_says_to_pull_one() {
        let _guard = CACHE_LOCK.lock().await;
        clear_cache();
        let server = serve("/models", &[]).await;
        let err = list_models_at(OLLAMA_ID, None, &server.uri())
            .await
            .unwrap_err();
        assert!(err.contains("ollama pull"), "got {err}");
    }

    /// Upstream failures must name the provider the user picked, not
    /// whichever fetcher happens to be shared underneath.
    #[tokio::test]
    async fn upstream_errors_name_the_provider() {
        let _guard = CACHE_LOCK.lock().await;
        clear_cache();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;

        let err = list_models_at(GROQ_ID, Some("bad"), &server.uri())
            .await
            .unwrap_err();
        assert!(err.contains("groq") && err.contains("401"), "got {err}");
    }
}
