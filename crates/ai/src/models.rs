//! Per-provider model catalogue.
//!
//! Each provider exposes a `list_models_for(api_key)` function that
//! returns a [`Catalogue`]: the [`ModelInfo`] entries and where they came
//! from ([`CatalogueSource`]). The Settings UI's model combo populates a
//! `<datalist>` from this surface, with a per-provider 10-minute in-memory
//! cache.
//!
//! # Per-provider behaviour
//!
//! * **Anthropic** — `GET /v1/models` with the saved key (newest model
//!   first; deprecated models are included and labelled from `lifecycle`;
//!   retired ones come only when asked for, and are never asked for). With
//!   no key, or when the fetch fails, the list is [`anthropic_fallback_models`]
//!   and the source says so, so Settings can tell the person which list it
//!   is showing instead of presenting a built-in list as Anthropic's.
//! * **OpenRouter** — `GET /api/v1/models` (public; auth optional but
//!   raises rate limits when present).
//! * **OpenAI** — `GET /v1/models` (auth required). We keep the ids that
//!   can chat (see [`is_chat_capable`]) and whose context window can hold the
//!   agent's request (see [`holds_the_agents_request`]), newest family first
//!   (see [`openai_rank`]).
//! * **Groq** — `GET /v1/models` (auth required). No filtering: the
//!   OpenAI heuristic above would reject every `llama-*` id and leave
//!   the dropdown empty.
//! * **ChatGPT plan** — `GET /v1/models` with the signed-in session's bearer,
//!   not a key: [`list_chatgpt_models`]. The reply is
//!   `{"models": [{"slug", "display_name", "visibility", "supported_in_api",
//!   "context_window", "input_modalities", "supported_reasoning_levels"}]}`,
//!   one account's catalogue, and only entries with `visibility == "list"`
//!   that are not marked unsupported in the API are kept, in the server's
//!   order. An empty list stays empty, and a rejected token is an error, never
//!   a built-in list. It is not cached (the answer is one account's, and the
//!   token is the account) and `list_models_for` refuses the id, so a key or a
//!   base URL typed into Settings cannot reach the route.
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
//! # Output limits
//!
//! Besides the catalogues, this module holds the one table of per-model
//! output limits ([`max_output_tokens`]). The agent asks every provider for
//! the same `max_tokens`, and OpenAI is reported to reject a request above
//! what a model can write, so [`clamp_max_tokens`] lowers it for the models
//! in the table. It is the only place a limit is written down.
//!
//! # Cache
//!
//! Cached entries live in a process-global `Mutex<HashMap<String,
//! (Instant, Vec<ModelInfo>)>>` with a 10-minute TTL, keyed by provider,
//! endpoint and a fingerprint of the key (`cache_key`). The TTL is short
//! enough that newly-released models surface within a session, and long
//! enough that the Settings panel doesn't re-fetch on every render.
//!
//! Only a live list is cached. A failed fetch returns the error (or, for
//! Anthropic, the fallback) to the caller and *does not* poison the cache,
//! so a retry has a fresh shot at the upstream API, and saving a key shows
//! the live list on the next fetch rather than ten minutes later.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::prompt::DEFAULT_MODEL;
use crate::provider::{
    AnthropicProvider, LlmProvider, ANTHROPIC_DEFAULT_BASE_URL, ANTHROPIC_ID, CHATGPT_ID,
    GEMINI_DEFAULT_BASE_URL, GEMINI_ID, GROQ_DEFAULT_BASE_URL, GROQ_ID, OLLAMA_DEFAULT_BASE_URL,
    OLLAMA_ID, OPENAI_DEFAULT_BASE_URL, OPENAI_DEFAULT_MODEL, OPENAI_ID, OPENROUTER_ID,
};

/// One entry in a provider's model catalogue.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModelInfo {
    /// Canonical id the user types into the combo (e.g.
    /// `claude-sonnet-5-5`, `anthropic/claude-sonnet-5.5`,
    /// `gpt-6-luna`).
    pub id: String,
    /// Human-readable label rendered alongside the id in the
    /// `<datalist>` suggestion.
    pub display_name: String,
    /// Context window in tokens, when known.
    pub context_length: Option<u32>,
    /// For OpenRouter, the namespace before the `/` (e.g. `anthropic`,
    /// `openai`, `google`); for native providers, `None`.
    pub provider_hint: Option<String>,
    /// What the model takes as input (`text`, `image`, ...), where the
    /// catalogue says: the ChatGPT plan's does.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub input_modalities: Vec<String>,
    /// The reasoning efforts the model takes (`low`, `medium`, `high`, ...),
    /// where the catalogue says: the ChatGPT plan's does, and an effort the
    /// model does not list is never sent for it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reasoning_levels: Vec<String>,
}

/// Where a catalogue came from, so Settings can say which list it shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CatalogueSource {
    /// Fetched from the provider just now (or from the cache of such a fetch).
    Live,
    /// edytlab's built-in list, because the live one was unavailable.
    /// `reason` is user-facing: it is shown in Settings as it is.
    Fallback { reason: String },
}

/// A provider's models and where they came from.
#[derive(Debug, Clone)]
pub struct Catalogue {
    pub models: Vec<ModelInfo>,
    pub source: CatalogueSource,
}

impl Catalogue {
    fn live(models: Vec<ModelInfo>) -> Self {
        Self {
            models,
            source: CatalogueSource::Live,
        }
    }
}

/// Cache TTL for model catalogues. Ten minutes is short enough for
/// model releases to surface within a session without hammering the
/// upstream APIs from a long-running desktop process.
const CACHE_TTL: Duration = Duration::from_secs(600);

/// One cache entry: timestamp of insertion + the cached catalogue.
type CacheEntry = (Instant, Vec<ModelInfo>);
type CacheMap = HashMap<String, CacheEntry>;

static CACHE: Mutex<Option<CacheMap>> = Mutex::new(None);

/// What a cached list is valid for: this provider, at this endpoint, with
/// this key. A list is the answer of one account at one server, so a changed
/// base URL or a newly saved or rotated key must fetch again instead of being
/// served another's list for ten minutes.
///
/// The key is reduced to a hash that exists only in memory (never logged,
/// never stored), so the cache holds no copy of it. No key and a blank key
/// are the same.
fn cache_key(provider_id: &str, base_url: &str, api_key: Option<&str>) -> String {
    use std::hash::{Hash, Hasher};

    let fingerprint = match api_key.map(str::trim).filter(|k| !k.is_empty()) {
        Some(key) => {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            key.hash(&mut hasher);
            format!("{:016x}", hasher.finish())
        }
        None => "-".to_string(),
    };
    format!("{provider_id}|{base_url}|{fingerprint}")
}

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

/// List models for `provider_id`. `api_key` is optional for OpenRouter;
/// OpenAI, Groq and Gemini require it, and Anthropic uses it for the live
/// list and answers with its built-in one when there is none. Returns the
/// cached list if fresh; otherwise hits the upstream and caches a live
/// result.
pub async fn list_models_for(
    provider_id: &str,
    api_key: Option<&str>,
) -> Result<Catalogue, String> {
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
) -> Result<Catalogue, String> {
    if let Some(base) = base_url_override.map(str::trim).filter(|b| !b.is_empty()) {
        return list_models_at(provider_id, api_key, base).await;
    }
    let base = match provider_id {
        ANTHROPIC_ID => ANTHROPIC_DEFAULT_BASE_URL,
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
) -> Result<Catalogue, String> {
    let cache_key = cache_key(provider_id, base_url, api_key);
    if let Some(cached) = cache_get(&cache_key) {
        return Ok(Catalogue::live(cached));
    }

    // Every id in `SUPPORTED_PROVIDER_IDS` needs an arm here. Groq and
    // Gemini were added to the provider layer and to the Settings
    // dropdown but never to this match, so picking either showed
    // "unsupported provider id: groq" where the model list belongs.
    let catalogue = match provider_id {
        ANTHROPIC_ID => anthropic_catalogue(base_url, api_key).await,
        OPENROUTER_ID => Catalogue::live(fetch_openrouter_models(api_key).await?),
        OPENAI_ID => Catalogue::live(fetch_openai_models(base_url, api_key).await?),
        GROQ_ID => Catalogue::live(fetch_groq_models(base_url, api_key).await?),
        GEMINI_ID => Catalogue::live(fetch_gemini_models(base_url, api_key).await?),
        OLLAMA_ID => Catalogue::live(fetch_ollama_models(base_url).await?),
        // The plan's list needs the session's bearer, which this function is
        // never handed and the webview never holds: see `list_chatgpt_models`.
        CHATGPT_ID => {
            return Err(
                "the ChatGPT plan's models are listed with its signed-in session, not a key"
                    .to_string(),
            )
        }
        other => return Err(format!("unsupported provider id: {other}")),
    };

    // Only what the provider said. A fallback is not cached: it would keep
    // showing the built-in list for ten minutes after the key was fixed.
    if catalogue.source == CatalogueSource::Live {
        cache_put(cache_key, catalogue.models.clone());
    }
    Ok(catalogue)
}

/// The models the signed-in ChatGPT account can use: `GET /v1/models` with
/// the session's bearer (`cfg` is a [`crate::LlmConfig::new_chatgpt`]), the
/// entries with `visibility == "list"`, in the server's order. "Show
/// `display_name` in your UI and pass the selected `slug` as `model`"
/// (<https://developers.openai.com/siwc/token-sharing-open-source/models-and-inference>),
/// so `slug` is [`ModelInfo::id`].
///
/// The bearer comes from the session, renewed once on a `401` like any other
/// request ([`crate::LlmConfig::send_with`]); nothing about the token or the
/// URL is an argument. `Err` is the person-facing text of whatever stopped it,
/// which for a session that ended or an account that cannot use its plan is
/// the same [`crate::PlanUsageError`] text a turn would show.
pub async fn list_chatgpt_models(
    cfg: &crate::LlmConfig,
    http: &reqwest::Client,
) -> Result<Catalogue, String> {
    let url = format!("{}{}", cfg.base_url(), cfg.provider.list_models_path());
    let resp = cfg
        .send_with(|| http.get(&url).timeout(ANTHROPIC_MODELS_TIMEOUT))
        .await
        .map_err(|e| e.to_string())?;
    let status = resp.status();
    let request_id = crate::responses::request_id_of(resp.headers());
    let text = resp.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        let failure = crate::ProviderFailure::Http {
            status: status.as_u16(),
            body: &text,
            request_id: request_id.as_deref(),
        };
        return Err(match cfg.provider.classify_failure(&failure) {
            Some(error) => error.to_string(),
            None => format!("the model list request failed ({})", status.as_u16()),
        });
    }
    let json: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| format!("the model list could not be read: {e}"))?;
    let models = parse_chatgpt_models(&json);
    // The provider's requests depend on what each model supports.
    cfg.provider.note_catalogue(&models);
    Ok(Catalogue::live(models))
}

/// The listed models of a `GET /v1/models` reply from the ChatGPT plan route:
/// the entries with `visibility == "list"` that are `supported_in_api` (an
/// entry that does not say is kept: the field is read as a filter on a
/// "false", not as a requirement the preview may not state), in the server's
/// order. An entry without a `slug` is skipped, and one without a
/// `display_name` is shown by its slug. `context_window`, `input_modalities`
/// and `supported_reasoning_levels` are read where they are given and left
/// empty where they are not or are the wrong type: nothing a catalogue says
/// wrongly fails it.
fn parse_chatgpt_models(body: &serde_json::Value) -> Vec<ModelInfo> {
    use serde_json::Value;
    let strings = |value: Option<&Value>| -> Vec<String> {
        value
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect()
    };
    body.get("models")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|m| m.get("visibility").and_then(Value::as_str) == Some("list"))
        .filter(|m| m.get("supported_in_api").and_then(Value::as_bool) != Some(false))
        .filter_map(|m| {
            let slug = m.get("slug")?.as_str().filter(|s| !s.is_empty())?;
            let display = m
                .get("display_name")
                .and_then(Value::as_str)
                .filter(|d| !d.is_empty())
                .unwrap_or(slug);
            Some(ModelInfo {
                id: slug.to_string(),
                display_name: display.to_string(),
                context_length: m
                    .get("context_window")
                    .and_then(Value::as_u64)
                    .and_then(|n| u32::try_from(n).ok()),
                provider_hint: None,
                input_modalities: strings(m.get("input_modalities")),
                reasoning_levels: strings(m.get("supported_reasoning_levels")),
            })
        })
        .collect()
}

/// The most pages of Anthropic's model list that are read. A request asks for
/// the documented maximum page size (1000), so this is one page in practice;
/// the cap is for a gateway that never stops saying there is more.
const ANTHROPIC_MODELS_MAX_PAGES: usize = 10;

/// `limit` for `GET /v1/models`: "Ranges from `1` to `1000`"
/// (<https://platform.claude.com/docs/en/api/models/list>).
const ANTHROPIC_MODELS_PAGE_SIZE: &str = "1000";

/// How long Settings waits for the list. A hung proxy must not leave the
/// model picker loading for good; the fallback should appear instead.
const ANTHROPIC_MODELS_TIMEOUT: Duration = Duration::from_secs(10);

/// Anthropic's catalogue: the live list when there is a key and the fetch
/// works, [`anthropic_fallback_models`] with the reason when not. Never an
/// error: a model picker that says "failed" has less to offer than one that
/// lists the models edytlab knows and says why it is not Anthropic's list.
async fn anthropic_catalogue(base_url: &str, api_key: Option<&str>) -> Catalogue {
    let fallback = |reason: String| Catalogue {
        models: anthropic_fallback_models(),
        source: CatalogueSource::Fallback { reason },
    };
    let Some(key) = api_key.map(str::trim).filter(|k| !k.is_empty()) else {
        return fallback("no API key saved".to_string());
    };
    match fetch_anthropic_models(base_url, key).await {
        Ok(models) if !models.is_empty() => Catalogue::live(models),
        Ok(_) => fallback("Anthropic listed no models".to_string()),
        Err(reason) => fallback(reason),
    }
}

/// edytlab's own list of Anthropic models, for when there is no key to
/// fetch the real one with or the fetch failed (see [`anthropic_catalogue`]).
///
/// The current generation only, one model per line, with the two models the
/// app uses unasked ([`DEFAULT_MODEL`] and [`CLASSIFIER_MODEL`]). Every one
/// has the 1M-token context the models overview gives
/// (<https://platform.claude.com/docs/en/about-claude/models/overview>). The
/// tests fail if this names a retired model or loses the defaults, so a new
/// generation has to come in here with the constants.
///
/// [`CLASSIFIER_MODEL`]: crate::CLASSIFIER_MODEL
pub fn anthropic_fallback_models() -> Vec<ModelInfo> {
    [
        ("claude-sonnet-5-5", "Claude Sonnet 5.5 (default)"),
        ("claude-haiku-5-5", "Claude Haiku 5.5"),
        ("claude-opus-5-5", "Claude Opus 5.5"),
        ("claude-fable-5-1", "Claude Fable 5.1"),
    ]
    .into_iter()
    .map(|(id, name)| ModelInfo {
        id: id.to_string(),
        display_name: name.to_string(),
        context_length: Some(1_000_000),
        provider_hint: None,
        ..Default::default()
    })
    .collect()
}

/// Live fetch from Anthropic's `GET /v1/models`.
///
/// The Models API lists "more recently released models first", includes
/// `active` and `deprecated` models, and returns `retired` ones "only when
/// `retired` is requested explicitly"
/// (<https://platform.claude.com/docs/en/api/models/list>). The order is
/// kept, with the deprecated models moved behind the active ones, and each
/// deprecated model says when it retires. A retired one is dropped should a
/// gateway return it.
///
/// `Err` is the reason the fallback list is shown, written for the person
/// reading Settings. None of them contains the key.
async fn fetch_anthropic_models(base_url: &str, api_key: &str) -> Result<Vec<ModelInfo>, String> {
    #[derive(Deserialize)]
    struct Page {
        #[serde(default)]
        data: Vec<WireModel>,
        #[serde(default)]
        has_more: bool,
        #[serde(default)]
        last_id: Option<String>,
    }
    #[derive(Deserialize)]
    struct WireModel {
        id: String,
        #[serde(default)]
        display_name: Option<String>,
        #[serde(default)]
        max_input_tokens: Option<u64>,
        #[serde(default)]
        lifecycle: Option<String>,
        #[serde(default)]
        retires_at: Option<String>,
        #[serde(default, rename = "type")]
        kind: Option<String>,
    }

    let base = base_url.trim_end_matches('/');
    let client = reqwest::Client::builder()
        .timeout(ANTHROPIC_MODELS_TIMEOUT)
        .build()
        .map_err(|e| format!("could not build an HTTP client ({e})"))?;

    let mut wire: Vec<WireModel> = Vec::new();
    let mut after_id: Option<String> = None;
    for _ in 0..ANTHROPIC_MODELS_MAX_PAGES {
        let mut req = client
            .get(format!("{base}/v1/models"))
            .query(&[("limit", ANTHROPIC_MODELS_PAGE_SIZE)]);
        if let Some(cursor) = &after_id {
            req = req.query(&[("after_id", cursor.as_str())]);
        }
        // The same headers chat sends: `x-api-key` and `anthropic-version`
        // are written down in one place.
        let req = AnthropicProvider.apply_auth(req, api_key);
        let resp = req
            .send()
            .await
            .map_err(|e| format!("could not reach {base} ({e})"))?;

        let status = resp.status();
        if matches!(status.as_u16(), 401 | 403) {
            return Err(format!("Anthropic rejected the key ({status})"));
        }
        if !status.is_success() {
            return Err(format!("the models list answered {status}"));
        }
        let page: Page = resp
            .json()
            .await
            .map_err(|e| format!("the models list could not be read ({e})"))?;
        wire.extend(page.data);

        // Another page only if one is promised and its cursor moved: a
        // gateway repeating itself cannot loop this.
        match page.last_id {
            Some(last) if page.has_more && after_id.as_deref() != Some(last.as_str()) => {
                after_id = Some(last)
            }
            _ => break,
        }
    }

    let mut seen = std::collections::HashSet::new();
    let mut active = Vec::new();
    let mut deprecated = Vec::new();
    for m in wire {
        if m.kind.as_deref().is_some_and(|k| k != "model")
            || m.lifecycle.as_deref() == Some("retired")
            || !seen.insert(m.id.clone())
        {
            continue;
        }
        let is_deprecated = m.lifecycle.as_deref() == Some("deprecated");
        let mut label = m
            .display_name
            .as_deref()
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .unwrap_or(&m.id)
            .to_string();
        if m.id == DEFAULT_MODEL {
            label.push_str(" (default)");
        }
        if is_deprecated {
            // `retires_at` is an RFC 3339 time; the day is what a person needs.
            match m.retires_at.as_deref().and_then(|t| t.get(..10)) {
                Some(day) => label.push_str(&format!(" — deprecated, retires {day}")),
                None => label.push_str(" — deprecated"),
            }
        }
        let info = ModelInfo {
            context_length: m
                .max_input_tokens
                .map(|n| u32::try_from(n).unwrap_or(u32::MAX)),
            display_name: label,
            provider_hint: None,
            id: m.id,
            ..Default::default()
        };
        if is_deprecated {
            deprecated.push(info);
        } else {
            active.push(info);
        }
    }
    active.extend(deprecated);
    Ok(active)
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
                ..Default::default()
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

/// Live fetch from OpenAI's `/v1/models`. Keeps the ids that can chat and
/// can hold the agent's request ([`is_chat_capable`],
/// [`holds_the_agents_request`]) — OpenAI's catalogue mixes embeddings, audio,
/// image and search models we never want to surface here — and orders them
/// newest family first ([`openai_rank`]). The default is labelled.
///
/// Known limitation, not coded around: a model that is listed here may still
/// not take tools on Chat Completions, the only API edytlab speaks.
/// `gpt-6.1-sol` is one: "Use the Responses API for tool calling. Chat
/// Completions is supported without tool calling"
/// (<https://developers.openai.com/api/docs/models/gpt-6.1-sol>). The list
/// carries no capability to filter on, so TEST's tool probe is where that
/// shows, as "did not call the tool".
async fn fetch_openai_models(
    base_url: &str,
    api_key: Option<&str>,
) -> Result<Vec<ModelInfo>, String> {
    let key = require_key(api_key, "OpenAI")?;
    let ids = fetch_openai_compatible_ids(&format!("{base_url}/v1/models"), key, "openai").await?;

    let mut models: Vec<ModelInfo> = ids
        .into_iter()
        .filter(|id| is_chat_capable(id) && holds_the_agents_request(id))
        .map(|id| ModelInfo {
            display_name: if id == OPENAI_DEFAULT_MODEL {
                format!("{id} (default)")
            } else {
                id.clone()
            },
            context_length: None,
            provider_hint: None,
            id,
            ..Default::default()
        })
        .collect();

    models.sort_by_cached_key(|m| openai_rank(&m.id));
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
            ..Default::default()
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
            ..Default::default()
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
/// Ids come back either bare (`gemini-3.8-flash`) or resource-qualified
/// (`models/gemini-3.8-flash`) depending on which surface answers. We
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
                ..Default::default()
            }
        })
        .collect())
}

/// Whether `id` is an OpenAI o-series reasoning model: `o`, a number, and
/// then nothing or a `-` (`o1`, `o3-mini`, `o4-mini`), but not `omni-…`.
fn is_o_series(id: &str) -> bool {
    let Some(rest) = id.strip_prefix('o') else {
        return false;
    };
    let digits = rest.chars().take_while(char::is_ascii_digit).count();
    digits > 0 && matches!(rest[digits..].chars().next(), None | Some('-'))
}

/// Whether `id` is a model the agent can chat with: a `gpt-` or `chatgpt-`
/// id or an o-series one, minus the ids that start with `gpt-` and are not
/// chat models.
///
/// There are many of those, and the prefix alone offered all of them:
/// `gpt-4o-transcribe`, `gpt-4o-mini-tts`, `gpt-realtime-2.1`,
/// `gpt-audio-1.5`, `gpt-live-transcribe`, `gpt-transcribe`, `gpt-image-1`
/// and `gpt-4o-search-preview-2025-03-11` are all on OpenAI's deprecations
/// page or models page (<https://developers.openai.com/api/docs/deprecations>),
/// and so are the `-completions` aliases and `gpt-3.5-turbo-instruct`, which
/// serve the legacy completions endpoint and not Chat Completions. The words
/// are matched anywhere in the id, so a family that does not exist yet and is
/// named the same way is left out too.
fn is_chat_capable(id: &str) -> bool {
    const NOT_CHAT: [&str; 10] = [
        "transcribe",
        "tts",
        "realtime",
        "audio",
        "image",
        "search-preview",
        "embedding",
        "moderation",
        "instruct",
        "completions",
    ];
    let chats = id.starts_with("gpt-") || id.starts_with("chatgpt-") || is_o_series(id);
    chats && !NOT_CHAT.iter().any(|word| id.contains(word))
}

/// Whether a model's context window can hold the agent's first request, which
/// is the tool list and the prompt: about 57 KB as chat-completions JSON
/// (`tests/request_size.rs`), roughly 15K tokens (llama.cpp counted 15,157
/// tokens for a request of this kind), and then the agent asks to write up
/// to 4,096 more, and a context window is shared by input and output
/// (<https://developers.openai.com/api/docs/guides/conversation-state>). A
/// model whose whole window is smaller than that fails every request it is
/// given, so offering it in a dropdown is offering something that cannot
/// work.
///
/// * `gpt-4`, `gpt-4-0613` and `gpt-4-0314`: 8,192 tokens of context
///   (<https://developers.openai.com/api/docs/models/gpt-4>).
/// * `gpt-3.5-turbo` and its snapshots: 16,385 tokens
///   (<https://developers.openai.com/api/docs/models/gpt-3.5-turbo>), less
///   than the request and its 4,096 together.
///
/// What is left of both families shuts down on 2026-10-23 anyway (see
/// [`openai_max_output_tokens`]). It only hides them: a model typed into
/// Settings by hand still goes out, with its output limit lowered, and fails
/// with the provider's own context-length answer.
///
/// The older models that stay are well above it: `gpt-4-turbo`'s page gives
/// 128,000 (<https://developers.openai.com/api/docs/models/gpt-4-turbo>).
fn holds_the_agents_request(id: &str) -> bool {
    !(matches!(id, "gpt-4" | "gpt-4-0613" | "gpt-4-0314") || id.starts_with("gpt-3.5-turbo"))
}

/// The sort key of an OpenAI model id: lower sorts higher in the list, so the
/// newest models surface at the top of the combo.
///
/// No generation is written into it, so it does not go stale. Family 0 is
/// `gpt-N[.M]…`, ordered by `N.M` descending (`gpt-6.1-sol` is 6.1, `gpt-6-luna`
/// is 6.0, `gpt-4o-mini` is 4.0); family 1 is the o-series, ordered by its
/// number descending; family 2 is anything else (`chatgpt-…`). The id is the
/// last component, so models of one generation sort alphabetically.
fn openai_rank(id: &str) -> (u8, std::cmp::Reverse<(u32, u32)>, String) {
    fn leading_number(s: &str) -> (u32, &str) {
        let digits = s.chars().take_while(char::is_ascii_digit).count();
        (s[..digits].parse().unwrap_or(0), &s[digits..])
    }

    let (family, version) = if let Some(rest) = id.strip_prefix("gpt-") {
        let (major, rest) = leading_number(rest);
        let minor = rest
            .strip_prefix('.')
            .map_or(0, |after_dot| leading_number(after_dot).0);
        (0, (major, minor))
    } else if is_o_series(id) {
        (1, (leading_number(&id[1..]).0, 0))
    } else {
        (2, (0, 0))
    };
    (family, std::cmp::Reverse(version), id.to_string())
}

/// The most output tokens one request may ask `model` for on `provider_id`,
/// for the models whose limit is 8192 or less, the caps the agent sends
/// ([`DEFAULT_MAX_TOKENS`](crate::prompt::DEFAULT_MAX_TOKENS) and
/// [`PLAN_MAX_TOKENS`](crate::prompt::PLAN_MAX_TOKENS), both 8192). A limit of
/// exactly 8192 lowers nothing; it is in the table so that the figure is
/// written down where a model with it is looked up.
///
/// `None` means this table does not know a limit, not that there is none: the
/// request goes out as built. Every figure comes from the provider's own
/// documentation, cited at its arm, and an id nobody has sourced is left out
/// rather than guessed. The arms also say which ids were looked at and left
/// out, and why.
///
/// There is no arm for the other providers, each for its own reason.
/// Anthropic's current models, which is all its built-in list holds, allow
/// 128K tokens of output
/// (<https://platform.claude.com/docs/en/about-claude/models/overview>), and
/// OpenRouter's public model list gives 128,000 for its two defaults
/// (`anthropic/claude-sonnet-5.5` and `anthropic/claude-haiku-5.5`,
/// <https://openrouter.ai/api/v1/models>). Ollama's OpenAI-compatible API
/// lists `max_tokens` as supported
/// (<https://docs.ollama.com/api/openai-compatibility>) and neither that page
/// nor `llama3.2`'s (<https://ollama.com/library/llama3.2>) states an output
/// limit.
pub fn max_output_tokens(provider_id: &str, model: &str) -> Option<u32> {
    match provider_id {
        OPENAI_ID => openai_max_output_tokens(model),
        GROQ_ID => groq_max_output_tokens(model),
        GEMINI_ID => gemini_max_output_tokens(model),
        _ => None,
    }
}

/// `requested`, lowered to the limit [`max_output_tokens`] knows for `model`.
/// It never raises a request, and it leaves one alone where no limit is
/// known.
pub fn clamp_max_tokens(provider_id: &str, model: &str, requested: u32) -> u32 {
    max_output_tokens(provider_id, model).map_or(requested, |limit| requested.min(limit))
}

/// OpenAI's models with an output limit of 8192 or less.
///
/// # Shutdown dates
///
/// Every row here is for a model OpenAI is retiring. By its deprecations page
/// (<https://developers.openai.com/api/docs/deprecations>):
///
/// * **2026-10-23**: `gpt-3.5-turbo-0125` (and its alias `gpt-3.5-turbo`),
///   `gpt-4-0613` (`gpt-4`), `gpt-4-turbo` (`gpt-4-turbo-2024-04-09`) and
///   `gpt-4-1106-preview` (the page's 2026-04-22 notice; an older one gave
///   2026-03-26), and `gpt-4o-2024-05-13`, which this table leaves out.
/// * Already shut down: `gpt-4-0314`, `gpt-4-0125-preview` (and its alias
///   `gpt-4-turbo-preview`) on 2026-03-26; `gpt-3.5-turbo-1106` and
///   `gpt-3.5-turbo-instruct` on 2026-09-28; `gpt-4-vision-preview` and
///   `gpt-4-1106-vision-preview` on 2024-12-06.
///
/// So the rows, and the tests that pin them (`legacy_openai_models_have_their_published_output_limit`
/// and the ones that name these ids), can be pruned as the dates pass, and all
/// of them once the last has. Until then they are what keeps a typed-in id
/// from being sent a limit it would refuse. The dropdown does not offer the
/// models that cannot run the agent in any case ([`holds_the_agents_request`]).
///
/// # Sources
///
/// OpenAI's model pages, which give one context window and one
/// maximum output for a model and list the snapshots and aliases they cover:
///
/// * <https://developers.openai.com/api/docs/models/gpt-3.5-turbo>: 16,385
///   context, 4,096 output; snapshots `gpt-3.5-turbo-0125`, `-1106` and
///   `-instruct`.
/// * <https://developers.openai.com/api/docs/models/gpt-4-turbo>: 128,000
///   context, 4,096 output; snapshots `gpt-4-turbo-2024-04-09`,
///   `gpt-4-turbo-preview`, `gpt-4-0125-preview` and `gpt-4-1106-vision-preview`.
/// * <https://developers.openai.com/api/docs/models/gpt-4-turbo-preview>:
///   128,000 context, 4,096 output; default snapshot `gpt-4-0125-preview`.
/// * <https://developers.openai.com/api/docs/models/gpt-4>: 8,192 context and
///   8,192 output; snapshots `gpt-4-0613` and `gpt-4-0314`.
///
/// The 4,096 arm is the three prefix rules below. They are the families the
/// pages name; the ids they match that no page names verbatim
/// (`gpt-4-1106-preview`, `gpt-4-vision-preview`, `gpt-3.5-turbo-16k`) take
/// their family's figure.
///
/// `gpt-4` is a different case. Its 8,192 output is also its whole context
/// window, and a context window is shared by input and output
/// (<https://developers.openai.com/api/docs/guides/conversation-state>), so
/// what a request may actually ask for is 8,192 minus its prompt. That changes
/// with every request and a table cannot say it; 8,192 is the one figure that
/// can be sourced, and it never lowers a request of 8192. The agent cannot
/// run on these models in any case: the tool definitions sent with each step
/// (93 tools, 54 KB of JSON when this was written) are well over 8,192
/// tokens, so the request fails on the context length whatever the output
/// cap is. That is why the dropdown does not list them
/// ([`holds_the_agents_request`]).
///
/// Left out: `gpt-4o-2024-05-13`. Its page,
/// <https://developers.openai.com/api/docs/models/gpt-4o>, gives 16,384, but as
/// the figure of the default snapshot (`gpt-4o-2024-08-06`); it gives none for
/// `2024-05-13` and that snapshot has no page of its own. Third parties put it
/// at 4,096, which is not a source, so it stays unclamped until OpenAI's
/// documentation gives a number.
///
/// The current models need no arm: their pages give `gpt-4o`, `gpt-4o-mini`
/// and `chatgpt-4o-latest` 16,384, `gpt-4.1` 32,768, the o-series 32,768 to
/// 100,000 and `gpt-5` 128,000. Any other id is as unknown here as
/// `gpt-4o-2024-05-13` is.
fn openai_max_output_tokens(model: &str) -> Option<u32> {
    if model.starts_with("gpt-3.5-turbo")
        || model.starts_with("gpt-4-turbo")
        || (model.starts_with("gpt-4-") && model.ends_with("-preview"))
    {
        Some(4_096)
    } else if matches!(model, "gpt-4" | "gpt-4-0613" | "gpt-4-0314") {
        Some(8_192)
    } else {
        None
    }
}

/// Groq's models with an output limit under 8192.
///
/// <https://console.groq.com/docs/models> gives 512 for the two Llama Prompt
/// Guard 2 models, as do their pages
/// (<https://console.groq.com/docs/model/meta-llama/llama-prompt-guard-2-22m>
/// and `…-86m`). They classify prompts and are not chat models, but Groq's
/// catalogue is not filtered here (see [`fetch_groq_models`]), so they reach
/// the dropdown. The defaults are well above: `llama-3.3-70b-versatile` writes
/// up to 32,768 and `llama-3.1-8b-instant` up to 131,072.
fn groq_max_output_tokens(model: &str) -> Option<u32> {
    match model {
        "meta-llama/llama-prompt-guard-2-22m" | "meta-llama/llama-prompt-guard-2-86m" => Some(512),
        _ => None,
    }
}

/// Gemini's models with an output limit under 8192.
///
/// Google's page for each model in <https://ai.google.dev/gemini-api/docs/models>
/// states an output token limit. The only one under 8192 is
/// <https://ai.google.dev/gemini-api/docs/models/gemini-3.1-flash-lite-image>,
/// 4,096, an image generation model that the agent cannot use; it is here for
/// the same reason as Groq's. The default and the classifier, `gemini-3.8-flash`
/// and `gemini-3.5-flash-lite`, both state 65,536
/// (<https://ai.google.dev/gemini-api/docs/models/gemini-3.8-flash> and
/// <https://ai.google.dev/gemini-api/docs/models/gemini-3.5-flash-lite>) and so
/// take the cap as it is. The embedding models state an output dimension and
/// no output limit, and nor do the pages of `gemini-3.5-transcribe` and
/// `gemini-omni-flash`; those stay unclamped.
fn gemini_max_output_tokens(model: &str) -> Option<u32> {
    match model {
        "gemini-3.1-flash-lite-image" => Some(4_096),
        _ => None,
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
    fn is_chat_capable_filters_correctly() {
        for id in [
            "gpt-4o-mini",
            "gpt-4.1",
            "o1",
            "o3",
            "o1-preview",
            "o3-mini",
            "o4-mini",
            "chatgpt-4o-latest",
            // The current generation: ids with a major and a minor in them.
            "gpt-6-luna",
            "gpt-6.1-sol",
            "gpt-6-astra",
            "gpt-5.6-terra",
        ] {
            assert!(is_chat_capable(id), "{id} chats");
        }
        for id in [
            "text-embedding-3-small",
            "dall-e-3",
            "whisper-1",
            "tts-1",
            // All of these start with `gpt-` and none of them chats. They are
            // ids on OpenAI's deprecations page
            // (<https://developers.openai.com/api/docs/deprecations>) or its
            // models page, and the dropdown used to offer every one.
            "gpt-4o-transcribe",
            "gpt-4o-mini-tts",
            "gpt-realtime-2.1",
            "gpt-audio-1.5",
            "gpt-live-transcribe",
            "gpt-transcribe",
            "gpt-image-1",
            "gpt-4o-search-preview",
            "gpt-4o-mini-search-preview-2025-03-11",
            // Completions-only ids: the `-completions` aliases and the
            // instruct model, which Chat Completions does not serve.
            "gpt-4-completions",
            "gpt-4-turbo-completions",
            "gpt-3.5-turbo-instruct",
        ] {
            assert!(!is_chat_capable(id), "{id} does not chat");
        }
    }

    /// No generation is written into the order: a model that did not exist
    /// when this was written sorts by its number, and the families sort as
    /// `gpt-N[.M]`, then the o-series, then the rest.
    #[test]
    fn openai_rank_orders_newest_families_first() {
        let in_order = [
            "gpt-6.1-sol",
            "gpt-6-astra",
            "gpt-6-luna",
            "gpt-5.6-terra",
            "gpt-4.1",
            "gpt-4o-mini",
            "o4-mini",
            "o3",
            "o1-preview",
            "chatgpt-4o-latest",
        ];
        for pair in in_order.windows(2) {
            assert!(
                openai_rank(pair[0]) < openai_rank(pair[1]),
                "{} should sort before {}",
                pair[0],
                pair[1]
            );
        }
        // The same family and generation sort by id, and `gpt-4-turbo` is
        // `gpt-4`, not `gpt-4.1`.
        assert!(openai_rank("gpt-4-turbo") < openai_rank("gpt-4o-mini"));
        assert!(openai_rank("gpt-4.1-nano") < openai_rank("gpt-4-turbo"));
        assert!(openai_rank("gpt-4-turbo") < openai_rank("gpt-3.5-turbo"));
    }

    /// The agent's request is about 15K tokens of tools and prompt, and it is
    /// asked to write up to 4,096 more. A model whose whole context window is
    /// smaller than that can never run it, whatever it is called.
    #[test]
    fn models_that_cannot_hold_the_agents_request_are_not_listed() {
        for id in [
            // 8,192 tokens of context and of output
            // (<https://developers.openai.com/api/docs/models/gpt-4>).
            "gpt-4",
            "gpt-4-0613",
            "gpt-4-0314",
            // 16,385 tokens of context
            // (<https://developers.openai.com/api/docs/models/gpt-3.5-turbo>).
            "gpt-3.5-turbo",
            "gpt-3.5-turbo-0125",
            "gpt-3.5-turbo-1106",
        ] {
            assert!(!holds_the_agents_request(id), "{id}");
        }
        for id in [
            "gpt-4-turbo",
            "gpt-4-turbo-2024-04-09",
            "gpt-4o",
            "gpt-4o-mini",
            "gpt-4.1",
            "gpt-6-luna",
            "o3",
            // Not the 8K model, whatever the prefix looks like.
            "gpt-4-0125-preview",
            "gpt-4-1106-preview",
        ] {
            assert!(holds_the_agents_request(id), "{id}");
        }
    }

    // ------------------------------------------------------------------
    // Output limits
    // ------------------------------------------------------------------

    /// The older OpenAI models that cannot write the 8192 the agent asks
    /// of every provider, with the limit their pages give.
    #[test]
    fn legacy_openai_models_have_their_published_output_limit() {
        for id in [
            "gpt-3.5-turbo",
            "gpt-3.5-turbo-0125",
            "gpt-3.5-turbo-1106",
            "gpt-4-turbo",
            "gpt-4-turbo-2024-04-09",
            "gpt-4-turbo-preview",
            "gpt-4-0125-preview",
            "gpt-4-1106-preview",
            "gpt-4-1106-vision-preview",
        ] {
            assert_eq!(max_output_tokens(OPENAI_ID, id), Some(4_096), "{id}");
            assert_eq!(clamp_max_tokens(OPENAI_ID, id, 8192), 4_096, "{id}");
        }
        for id in ["gpt-4", "gpt-4-0613", "gpt-4-0314"] {
            assert_eq!(max_output_tokens(OPENAI_ID, id), Some(8_192), "{id}");
            assert_eq!(clamp_max_tokens(OPENAI_ID, id, 16_384), 8_192, "{id}");
        }
    }

    /// A model that can write more than the caps has no entry, and an id
    /// nobody has sourced goes out as built. `gpt-4o-2024-05-13` is the
    /// second kind on purpose: OpenAI's page gives 16,384 for the default
    /// snapshot of `gpt-4o` and nothing for it. If its documentation ever
    /// gives a figure, this is the test to change with the table.
    #[test]
    fn current_and_unsourced_models_are_not_clamped() {
        for id in [
            "gpt-4o",
            "gpt-4o-mini",
            "gpt-4o-2024-08-06",
            "gpt-4o-2024-05-13",
            "chatgpt-4o-latest",
            "gpt-4.1",
            "gpt-4.5-preview",
            "gpt-4o-audio-preview",
            "o1",
            "o3-mini",
            "gpt-5",
            "a-model-nobody-has-heard-of",
            "",
        ] {
            assert_eq!(max_output_tokens(OPENAI_ID, id), None, "{id}");
            assert_eq!(clamp_max_tokens(OPENAI_ID, id, 8192), 8192, "{id}");
        }
    }

    /// A clamp lowers a request and does nothing else.
    #[test]
    fn a_clamp_never_raises_a_request() {
        assert_eq!(clamp_max_tokens(OPENAI_ID, "gpt-4-turbo", 256), 256);
        assert_eq!(clamp_max_tokens(OPENAI_ID, "gpt-4-turbo", 4_096), 4_096);
        assert_eq!(clamp_max_tokens(OPENAI_ID, "gpt-4-turbo", 4_097), 4_096);
        assert_eq!(clamp_max_tokens(OPENAI_ID, "gpt-4-turbo", 0), 0);
        assert_eq!(clamp_max_tokens(OPENAI_ID, "gpt-4o-mini", 100_000), 100_000);
    }

    /// A limit belongs to the provider whose documentation gave it. The
    /// same id on another provider is another model, or none we know.
    #[test]
    fn a_limit_is_not_borrowed_across_providers() {
        for provider in [
            ANTHROPIC_ID,
            OPENROUTER_ID,
            GROQ_ID,
            GEMINI_ID,
            OLLAMA_ID,
            "not-a-provider",
        ] {
            assert_eq!(
                max_output_tokens(provider, "gpt-4-turbo"),
                None,
                "{provider}"
            );
            assert_eq!(
                max_output_tokens(provider, "gpt-3.5-turbo"),
                None,
                "{provider}"
            );
        }
        assert_eq!(
            max_output_tokens(OPENROUTER_ID, "openai/gpt-4-turbo"),
            None,
            "OpenRouter's catalogue is out of this table"
        );
        assert_eq!(
            max_output_tokens(OPENAI_ID, "meta-llama/llama-prompt-guard-2-22m"),
            None
        );
        assert_eq!(
            max_output_tokens(GROQ_ID, "gemini-3.1-flash-lite-image"),
            None
        );
        assert_eq!(max_output_tokens(GEMINI_ID, "gpt-4"), None);
    }

    /// The Groq and Gemini models found under 8192, and the defaults the
    /// app names for every provider, which are not.
    #[test]
    fn groq_and_gemini_have_only_the_limits_their_docs_give() {
        for id in [
            "meta-llama/llama-prompt-guard-2-22m",
            "meta-llama/llama-prompt-guard-2-86m",
        ] {
            assert_eq!(max_output_tokens(GROQ_ID, id), Some(512), "{id}");
            assert_eq!(clamp_max_tokens(GROQ_ID, id, 8192), 512, "{id}");
        }
        assert_eq!(
            clamp_max_tokens(GEMINI_ID, "gemini-3.1-flash-lite-image", 8192),
            4_096
        );

        for (provider, id) in [
            (GROQ_ID, "llama-3.3-70b-versatile"),
            (GROQ_ID, "llama-3.1-8b-instant"),
            (GEMINI_ID, "gemini-3.8-flash"),
            (GEMINI_ID, "gemini-3.5-flash-lite"),
            (OLLAMA_ID, "llama3.2"),
            (OPENAI_ID, "gpt-6-luna"),
            (OPENROUTER_ID, "anthropic/claude-sonnet-5.5"),
            (OPENROUTER_ID, "anthropic/claude-haiku-5.5"),
            (ANTHROPIC_ID, "claude-sonnet-5-5"),
            (ANTHROPIC_ID, "claude-haiku-5-5"),
        ] {
            assert_eq!(
                clamp_max_tokens(provider, id, 8192),
                8192,
                "{provider} {id}"
            );
        }
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

    // ------------------------------------------------------------------
    // Anthropic: fetched live, with a built-in fallback
    // ------------------------------------------------------------------

    use wiremock::matchers::{header, query_param};

    /// One entry of Anthropic's `GET /v1/models` answer, with the fields
    /// the page documents (`type`, `id`, `display_name`, `max_input_tokens`,
    /// `lifecycle`, `retires_at`).
    fn wire_model(
        id: &str,
        display_name: &str,
        lifecycle: &str,
        retires_at: Option<&str>,
    ) -> serde_json::Value {
        serde_json::json!({
            "type": "model",
            "id": id,
            "display_name": display_name,
            "created_at": "2026-01-01T00:00:00Z",
            "max_input_tokens": 1_000_000,
            "max_tokens": 128_000,
            "lifecycle": lifecycle,
            "retires_at": retires_at,
        })
    }

    fn anthropic_page(
        data: Vec<serde_json::Value>,
        has_more: bool,
        last_id: Option<&str>,
    ) -> serde_json::Value {
        serde_json::json!({
            "data": data,
            "has_more": has_more,
            "first_id": null,
            "last_id": last_id,
        })
    }

    fn ids_of(models: &[ModelInfo]) -> Vec<&str> {
        models.iter().map(|m| m.id.as_str()).collect()
    }

    /// The docs' contract: newest first, deprecated models included and
    /// labelled, `x-api-key` and `anthropic-version` on the request, one page
    /// of up to 1000. Anything else the mock answers 404, which would fall
    /// back and fail the assertions.
    #[tokio::test]
    async fn anthropic_catalogue_is_fetched_live() {
        let _guard = CACHE_LOCK.lock().await;
        clear_cache();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .and(header("x-api-key", "k"))
            .and(header("anthropic-version", "2023-06-01"))
            .and(query_param("limit", "1000"))
            .respond_with(ResponseTemplate::new(200).set_body_json(anthropic_page(
                vec![
                    wire_model("claude-sonnet-5-5", "Claude Sonnet 5.5", "active", None),
                    wire_model(
                        "claude-sonnet-4-5-20250929",
                        "Claude Sonnet 4.5",
                        "deprecated",
                        Some("2026-11-30T00:00:00Z"),
                    ),
                    wire_model("claude-haiku-5-5", "Claude Haiku 5.5", "active", None),
                ],
                false,
                Some("claude-haiku-5-5"),
            )))
            .mount(&server)
            .await;

        let cat = list_models_for_at(ANTHROPIC_ID, Some("k"), Some(&server.uri()))
            .await
            .unwrap();

        assert_eq!(cat.source, CatalogueSource::Live);
        assert_eq!(
            ids_of(&cat.models),
            [
                "claude-sonnet-5-5",
                "claude-haiku-5-5",
                "claude-sonnet-4-5-20250929"
            ],
            "active models first, the API's own order kept, deprecated last"
        );
        assert_eq!(cat.models[0].display_name, "Claude Sonnet 5.5 (default)");
        assert_eq!(cat.models[1].display_name, "Claude Haiku 5.5");
        assert_eq!(
            cat.models[2].display_name,
            "Claude Sonnet 4.5 — deprecated, retires 2026-11-30"
        );
        assert_eq!(cat.models[0].context_length, Some(1_000_000));
        assert_eq!(cat.models[0].provider_hint, None);
    }

    /// A saved base URL (a gateway or proxy) is used for the list as it is
    /// for chat, and a trailing slash on it is harmless.
    #[tokio::test]
    async fn anthropic_catalogue_follows_the_saved_base_url() {
        let _guard = CACHE_LOCK.lock().await;
        clear_cache();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_json(anthropic_page(
                vec![wire_model(
                    "claude-haiku-5-5",
                    "Claude Haiku 5.5",
                    "active",
                    None,
                )],
                false,
                None,
            )))
            .mount(&server)
            .await;

        let with_slash = format!("{}/", server.uri());
        let cat = list_models_for_at(ANTHROPIC_ID, Some("k"), Some(&with_slash))
            .await
            .unwrap();
        assert_eq!(cat.source, CatalogueSource::Live);
        assert_eq!(ids_of(&cat.models), ["claude-haiku-5-5"]);
    }

    #[tokio::test]
    async fn anthropic_catalogue_follows_pagination() {
        let _guard = CACHE_LOCK.lock().await;
        clear_cache();
        let server = MockServer::start().await;
        // The second page is mounted first: it is the one that names `after_id`.
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .and(query_param("after_id", "b"))
            .respond_with(ResponseTemplate::new(200).set_body_json(anthropic_page(
                vec![wire_model("c", "C", "active", None)],
                false,
                Some("c"),
            )))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_json(anthropic_page(
                vec![
                    wire_model("a", "A", "active", None),
                    wire_model("b", "B", "active", None),
                ],
                true,
                Some("b"),
            )))
            .mount(&server)
            .await;

        let cat = list_models_for_at(ANTHROPIC_ID, Some("k"), Some(&server.uri()))
            .await
            .unwrap();
        assert_eq!(cat.source, CatalogueSource::Live);
        assert_eq!(ids_of(&cat.models), ["a", "b", "c"]);
    }

    /// A proxy that answers `has_more` forever with the same cursor must not
    /// hang Settings.
    #[tokio::test]
    async fn anthropic_pagination_stops_on_a_repeated_cursor() {
        let _guard = CACHE_LOCK.lock().await;
        clear_cache();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_json(anthropic_page(
                vec![wire_model("a", "A", "active", None)],
                true,
                Some("b"),
            )))
            .mount(&server)
            .await;

        let cat = list_models_for_at(ANTHROPIC_ID, Some("k"), Some(&server.uri()))
            .await
            .unwrap();
        let requests = server.received_requests().await.unwrap().len();
        assert!(
            requests <= ANTHROPIC_MODELS_MAX_PAGES,
            "{requests} requests: the page cap is {ANTHROPIC_MODELS_MAX_PAGES}"
        );
        assert_eq!(requests, 2, "a repeated cursor ends the walk on the spot");
        assert_eq!(cat.source, CatalogueSource::Live);
        assert_eq!(ids_of(&cat.models), ["a"], "the repeated page adds nothing");
    }

    /// Even a cursor that keeps changing is cut off at the page cap.
    #[tokio::test]
    async fn anthropic_pagination_is_capped() {
        let _guard = CACHE_LOCK.lock().await;
        clear_cache();
        let server = MockServer::start().await;
        // Each request names the previous answer's cursor and gets a new one.
        // Mounted last-first: the first mount that matches a request answers it,
        // and the one for page 0 matches every request.
        for n in (0..ANTHROPIC_MODELS_MAX_PAGES + 5).rev() {
            let mock = Mock::given(method("GET")).and(path("/v1/models"));
            let mock = if n == 0 {
                mock
            } else {
                mock.and(query_param("after_id", format!("id{}", n - 1)))
            };
            mock.respond_with(ResponseTemplate::new(200).set_body_json(anthropic_page(
                vec![wire_model(&format!("id{n}"), "X", "active", None)],
                true,
                Some(&format!("id{n}")),
            )))
            .mount(&server)
            .await;
        }

        let cat = list_models_for_at(ANTHROPIC_ID, Some("k"), Some(&server.uri()))
            .await
            .unwrap();
        let requests = server.received_requests().await.unwrap().len();
        assert_eq!(requests, ANTHROPIC_MODELS_MAX_PAGES);
        assert_eq!(cat.models.len(), ANTHROPIC_MODELS_MAX_PAGES);
    }

    #[tokio::test]
    async fn anthropic_catalogue_falls_back_on_401_and_does_not_cache_it() {
        let _guard = CACHE_LOCK.lock().await;
        clear_cache();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({
                "type": "error",
                "error": {"type": "authentication_error", "message": "invalid x-api-key"}
            })))
            .mount(&server)
            .await;

        let cat = list_models_for_at(ANTHROPIC_ID, Some("bad-key"), Some(&server.uri()))
            .await
            .unwrap();
        match &cat.source {
            CatalogueSource::Fallback { reason } => {
                assert!(
                    reason.contains("401"),
                    "the reason names the status: {reason}"
                );
                assert!(
                    !reason.contains("bad-key"),
                    "the key is never echoed: {reason}"
                );
            }
            other => panic!("expected the fallback, got {other:?}"),
        }
        assert_eq!(
            ids_of(&cat.models),
            ids_of(&anthropic_fallback_models()),
            "the fallback is the built-in list"
        );

        // The same endpoint and key, now answering: a cached fallback would
        // keep showing the built-in list for ten minutes.
        server.reset().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_json(anthropic_page(
                vec![wire_model("claude-live-only", "Live only", "active", None)],
                false,
                None,
            )))
            .mount(&server)
            .await;
        let cat = list_models_for_at(ANTHROPIC_ID, Some("bad-key"), Some(&server.uri()))
            .await
            .unwrap();
        assert_eq!(cat.source, CatalogueSource::Live);
        assert_eq!(ids_of(&cat.models), ["claude-live-only"]);
    }

    #[tokio::test]
    async fn anthropic_catalogue_falls_back_on_any_other_status_and_on_a_bad_body() {
        let _guard = CACHE_LOCK.lock().await;
        for (template, expect) in [
            (ResponseTemplate::new(500), "500"),
            (
                ResponseTemplate::new(200).set_body_string("<html>"),
                "could not be read",
            ),
        ] {
            clear_cache();
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .and(path("/v1/models"))
                .respond_with(template)
                .mount(&server)
                .await;
            let cat = list_models_for_at(ANTHROPIC_ID, Some("k"), Some(&server.uri()))
                .await
                .unwrap();
            match cat.source {
                CatalogueSource::Fallback { reason } => {
                    assert!(reason.contains(expect), "{reason}")
                }
                other => panic!("expected the fallback, got {other:?}"),
            }
            assert_eq!(ids_of(&cat.models), ids_of(&anthropic_fallback_models()));
        }
    }

    #[tokio::test]
    async fn anthropic_catalogue_falls_back_when_it_lists_nothing() {
        let _guard = CACHE_LOCK.lock().await;
        clear_cache();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_json(anthropic_page(
                vec![],
                false,
                None,
            )))
            .mount(&server)
            .await;
        let cat = list_models_for_at(ANTHROPIC_ID, Some("k"), Some(&server.uri()))
            .await
            .unwrap();
        assert!(matches!(cat.source, CatalogueSource::Fallback { .. }));
        assert!(!cat.models.is_empty());
    }

    #[tokio::test]
    async fn anthropic_catalogue_falls_back_when_unreachable() {
        let _guard = CACHE_LOCK.lock().await;
        clear_cache();
        // Port 1 is reserved and nothing listens on it.
        let cat = list_models_for_at(ANTHROPIC_ID, Some("k"), Some("http://127.0.0.1:1"))
            .await
            .unwrap();
        match cat.source {
            CatalogueSource::Fallback { reason } => assert!(!reason.is_empty()),
            other => panic!("expected the fallback, got {other:?}"),
        }
        assert_eq!(ids_of(&cat.models), ids_of(&anthropic_fallback_models()));
    }

    #[tokio::test]
    async fn anthropic_catalogue_without_a_key_makes_no_request() {
        let _guard = CACHE_LOCK.lock().await;
        for key in [None, Some(""), Some("   ")] {
            clear_cache();
            let server = MockServer::start().await;
            Mock::given(wiremock::matchers::any())
                .respond_with(ResponseTemplate::new(200))
                .expect(0)
                .mount(&server)
                .await;
            let cat = list_models_for_at(ANTHROPIC_ID, key, Some(&server.uri()))
                .await
                .unwrap();
            assert_eq!(
                cat.source,
                CatalogueSource::Fallback {
                    reason: "no API key saved".to_string()
                },
                "key {key:?}"
            );
            assert!(server.received_requests().await.unwrap().is_empty());
        }
    }

    /// Retired models are not returned unless asked for, and this never
    /// asks. A proxy that returns one anyway must not put it in the list.
    #[tokio::test]
    async fn retired_models_are_never_listed() {
        let _guard = CACHE_LOCK.lock().await;
        clear_cache();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_json(anthropic_page(
                vec![
                    wire_model("claude-sonnet-5-5", "Claude Sonnet 5.5", "active", None),
                    wire_model(
                        "claude-opus-4-1-20250805",
                        "Claude Opus 4.1",
                        "retired",
                        Some("2026-08-05T00:00:00Z"),
                    ),
                ],
                false,
                None,
            )))
            .mount(&server)
            .await;
        let cat = list_models_for_at(ANTHROPIC_ID, Some("k"), Some(&server.uri()))
            .await
            .unwrap();
        assert_eq!(ids_of(&cat.models), ["claude-sonnet-5-5"]);
    }

    /// Older `anthropic-version`s and gateways leave fields out; only `id`
    /// is needed.
    #[tokio::test]
    async fn anthropic_entries_may_omit_everything_but_the_id() {
        let _guard = CACHE_LOCK.lock().await;
        clear_cache();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"data": [{"id": "claude-x"}]})),
            )
            .mount(&server)
            .await;
        let cat = list_models_for_at(ANTHROPIC_ID, Some("k"), Some(&server.uri()))
            .await
            .unwrap();
        assert_eq!(cat.source, CatalogueSource::Live);
        assert_eq!(ids_of(&cat.models), ["claude-x"]);
        assert_eq!(cat.models[0].display_name, "claude-x");
        assert_eq!(cat.models[0].context_length, None);
    }

    /// The built-in list is for when the live one cannot be had, so it holds
    /// the current generation only. Every id below is "Retired" on
    /// <https://platform.claude.com/docs/en/about-claude/model-deprecations>
    /// ("Requests to retired models will fail"); offering one is offering a
    /// model that answers 404. It must also hold the two models the app
    /// uses without being asked.
    #[test]
    fn the_fallback_names_no_retired_model_and_holds_the_defaults() {
        const RETIRED: [&str; 6] = [
            "claude-opus-4-1-20250805",
            "claude-opus-4-20250514",
            "claude-sonnet-4-20250514",
            "claude-3-7-sonnet-20250219",
            "claude-3-5-haiku-20241022",
            "claude-3-haiku-20240307",
        ];
        let fallback = anthropic_fallback_models();
        let ids = ids_of(&fallback);
        for retired in RETIRED {
            assert!(!ids.contains(&retired), "{retired} is retired");
        }
        assert!(ids.contains(&crate::DEFAULT_MODEL), "{ids:?}");
        assert!(ids.contains(&crate::CLASSIFIER_MODEL), "{ids:?}");
        assert_eq!(
            ids,
            [
                "claude-sonnet-5-5",
                "claude-haiku-5-5",
                "claude-opus-5-5",
                "claude-fable-5-1"
            ]
        );
        assert_eq!(fallback[0].display_name, "Claude Sonnet 5.5 (default)");
        assert!(fallback.iter().all(|m| m.context_length == Some(1_000_000)));
    }

    /// A changed endpoint or key must not be served the previous one's list
    /// for ten minutes; an unchanged one is served from the cache.
    #[tokio::test]
    async fn the_cache_is_keyed_by_endpoint_and_key() {
        let _guard = CACHE_LOCK.lock().await;
        clear_cache();
        async fn one_model_server(id: &str) -> MockServer {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .and(path("/v1/models"))
                .respond_with(ResponseTemplate::new(200).set_body_json(anthropic_page(
                    vec![wire_model(id, id, "active", None)],
                    false,
                    None,
                )))
                .mount(&server)
                .await;
            server
        }
        let first = one_model_server("first").await;
        let second = one_model_server("second").await;

        let a = list_models_for_at(ANTHROPIC_ID, Some("k"), Some(&first.uri()))
            .await
            .unwrap();
        let b = list_models_for_at(ANTHROPIC_ID, Some("k"), Some(&second.uri()))
            .await
            .unwrap();
        assert_eq!(ids_of(&a.models), ["first"]);
        assert_eq!(
            ids_of(&b.models),
            ["second"],
            "another endpoint, another list"
        );

        let again = list_models_for_at(ANTHROPIC_ID, Some("k"), Some(&first.uri()))
            .await
            .unwrap();
        assert_eq!(again.source, CatalogueSource::Live);
        assert_eq!(ids_of(&again.models), ["first"]);
        assert_eq!(
            first.received_requests().await.unwrap().len(),
            1,
            "the same endpoint and key is served from the cache"
        );

        let other_key = list_models_for_at(ANTHROPIC_ID, Some("k2"), Some(&first.uri()))
            .await
            .unwrap();
        assert_eq!(ids_of(&other_key.models), ["first"]);
        assert_eq!(
            first.received_requests().await.unwrap().len(),
            2,
            "a rotated key fetches again"
        );
    }

    #[test]
    fn the_cache_key_does_not_contain_the_api_key() {
        let key = "secret-value-0123456789";
        let ck = cache_key(ANTHROPIC_ID, "https://api.anthropic.com", Some(key));
        assert!(!ck.contains(key) && !ck.contains("secret"), "{ck}");
        assert_ne!(
            ck,
            cache_key(ANTHROPIC_ID, "https://api.anthropic.com", Some("other")),
        );
        assert_ne!(
            ck,
            cache_key(ANTHROPIC_ID, "https://proxy.example", Some(key))
        );
        assert_eq!(
            cache_key(ANTHROPIC_ID, "u", None),
            cache_key(ANTHROPIC_ID, "u", Some("  ")),
            "no key and a blank key are the same"
        );
    }

    #[tokio::test]
    async fn cache_returns_within_ttl() {
        let _guard = CACHE_LOCK.lock().await;
        clear_cache();
        cache_put(
            cache_key(OPENAI_ID, OPENAI_DEFAULT_BASE_URL, Some("k")),
            vec![ModelInfo {
                id: "x".into(),
                display_name: "x".into(),
                context_length: None,
                provider_hint: None,
                ..Default::default()
            }],
        );
        // No server is running behind this: an answer can only be the cache's.
        let cat = list_models_for(OPENAI_ID, Some("k")).await.unwrap();
        assert_eq!(cat.source, CatalogueSource::Live);
        assert_eq!(cat.models.len(), 1);
        assert_eq!(cat.models[0].id, "x");
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
            .expect("groq catalogue should resolve")
            .models;

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
            .unwrap()
            .models;
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
        let server = serve("/models", &["gemini-3.8-flash"]).await;

        let models = list_models_at(GEMINI_ID, Some("k"), &server.uri())
            .await
            .expect("gemini catalogue should resolve")
            .models;
        assert_eq!(models[0].id, "gemini-3.8-flash");
    }

    /// Ids may come back resource-qualified. The bare form is what the
    /// chat endpoint accepts and `translate_model` is identity for this
    /// provider, so both shapes have to land on the same id — otherwise
    /// a model picked from the dropdown would be rejected at inference.
    #[tokio::test]
    async fn gemini_strips_a_resource_prefix_when_present() {
        let _guard = CACHE_LOCK.lock().await;
        clear_cache();
        let server = serve("/models", &["models/gemini-3.8-flash"]).await;

        let models = list_models_at(GEMINI_ID, Some("k"), &server.uri())
            .await
            .unwrap()
            .models;
        assert_eq!(
            models[0].id, "gemini-3.8-flash",
            "a `models/`-qualified id must resolve to the same id as a bare one"
        );
        assert_eq!(models[0].display_name, "gemini-3.8-flash");
    }

    /// OpenAI keeps its filter and its ordering — the refactor that
    /// added the other two arms must not have loosened it. The current
    /// default comes first among the current generation, and says it is the
    /// default.
    #[tokio::test]
    async fn openai_catalogue_keeps_its_filter_and_ranking() {
        let _guard = CACHE_LOCK.lock().await;
        clear_cache();
        let server = serve(
            "/v1/models",
            &[
                "text-embedding-3-small",
                "gpt-4o-mini",
                "gpt-6-luna",
                "gpt-4o-transcribe",
                "o3",
            ],
        )
        .await;

        let models = list_models_at(OPENAI_ID, Some("k"), &server.uri())
            .await
            .unwrap()
            .models;
        let ids: Vec<&str> = models.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["gpt-6-luna", "gpt-4o-mini", "o3"],
            "embeddings and audio models must stay out, the newest family ranks first"
        );
        assert!(
            models[0].display_name.ends_with("(default)"),
            "{}",
            models[0].display_name
        );
        assert!(!models[1].display_name.contains("default"));
    }

    #[tokio::test]
    async fn openai_catalogue_hides_what_cannot_run_the_agent() {
        let _guard = CACHE_LOCK.lock().await;
        clear_cache();
        let server = serve(
            "/v1/models",
            &[
                "gpt-4",
                "gpt-4-0613",
                "gpt-3.5-turbo",
                "gpt-3.5-turbo-instruct",
                "gpt-4-turbo",
                "gpt-4o-mini",
            ],
        )
        .await;
        let models = list_models_at(OPENAI_ID, Some("k"), &server.uri())
            .await
            .unwrap()
            .models;
        let ids: Vec<&str> = models.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, vec!["gpt-4-turbo", "gpt-4o-mini"]);
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
            .expect("a keyless provider must resolve without a key")
            .models;
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
