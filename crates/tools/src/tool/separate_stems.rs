//! `separate_stems` tool — decode an audio file, hand it to a Demucs
//! ONNX model, and surface the four-stem set as on-disk WAVs.
//!
//! The crate-level model lives behind a process-wide cache so 10
//! invocations against the same model load it once. The on-disk stem
//! cache lives at
//! `<project>/.audiograph/stem-cache/<hex>/{vocals,drums,bass,other}.wav`
//! where `<hex>` is `ContentHash::combine(input_hash, model_hash)`. A
//! second invocation with the same input + model returns the same
//! four paths without re-running inference (acceptance criterion #2).
//!
//! ## Phase-2 sandbox behaviour
//!
//! Sourcing a license-clean Demucs ONNX export is an M28 deliverable.
//! Until it lands, [`DemucsModel::load`] returns `ModelMissing` and
//! the tool surfaces an actionable
//! `ToolResult::Error("…model not yet sourced…")`. The caching path
//! and tool dispatch are fully wired so M28 only has to drop the
//! `.onnx` into place.
//!
//! ## OOM fallback (acceptance criterion #4)
//!
//! On `out of memory` from ORT we retry once with the smaller
//! `htdemucs` model and log a `tracing::warn!`. Detection lives in
//! [`ml_demucs::is_oom_error`] so the policy and the matcher stay in
//! one crate.

use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};

use ml_demucs::{
    is_oom_error, read_cached_stems, stem_dir_for, write_stems_to_dir, DemucsError, DemucsModel,
    StemPaths, StemSeparationResult, DEFAULT_MODEL_ID, OOM_FALLBACK_MODEL_ID, SUPPORTED_MODEL_IDS,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::schema::anthropic_tool;
use crate::{Tool, ToolContext, ToolResult};

const INSTALL_HINT: &str = "install Demucs model: run `scripts/fetch-models.sh` and set DEMUCS_MODEL_PATH (or DEMUCS_FT_MODEL_PATH for htdemucs_ft) to the resulting .onnx path";

/// Process-wide cache of `(model_path, &'static DemucsModel)`. One
/// entry per loaded model id; we hold up to two at a time
/// (`htdemucs_ft` + `htdemucs` for the OOM fallback path).
type ModelCache = RwLock<Vec<(PathBuf, &'static DemucsModel)>>;

fn model_cache() -> &'static ModelCache {
    static CACHE: OnceLock<ModelCache> = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(Vec::new()))
}

/// Fetch the cached [`DemucsModel`] for `path`, loading it on first
/// use. The returned `&'static DemucsModel` is a reference into a
/// `Box::leak`-ed singleton — fine for a process-wide cache.
fn get_or_load_model(model_id: &str, path: &Path) -> Result<&'static DemucsModel, DemucsError> {
    {
        let guard = model_cache()
            .read()
            .expect("demucs model cache lock poisoned (read)");
        for (cached_path, model) in guard.iter() {
            if cached_path == path {
                return Ok(*model);
            }
        }
    }
    let mut guard = model_cache()
        .write()
        .expect("demucs model cache lock poisoned (write)");
    for (cached_path, model) in guard.iter() {
        if cached_path == path {
            return Ok(*model);
        }
    }
    let model = DemucsModel::load(model_id, path)?;
    let leaked: &'static DemucsModel = Box::leak(Box::new(model));
    guard.push((path.to_path_buf(), leaked));
    Ok(leaked)
}

/// Resolve the on-disk model path for a given `model_id` from env.
/// Returns `Err(message)` shaped for `ToolResult::Error` on failure.
fn resolve_model_path(model_id: &str) -> std::result::Result<PathBuf, String> {
    let env_var = match model_id {
        "htdemucs_ft" => "DEMUCS_FT_MODEL_PATH",
        "htdemucs" => "DEMUCS_MODEL_PATH",
        other => return Err(format!("unsupported model: {other:?}")),
    };
    match std::env::var(env_var) {
        Ok(p) => Ok(PathBuf::from(p)),
        Err(_) => Err(format!("{env_var} env var not set; {INSTALL_HINT}")),
    }
}

#[derive(Debug, Deserialize)]
struct Args {
    path: String,
    #[serde(default)]
    model: Option<String>,
}

pub struct SeparateStemsTool;

impl Tool for SeparateStemsTool {
    fn name(&self) -> &'static str {
        "separate_stems"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "separate_stems",
            "Run Demucs stem separation on an audio file and return paths to the four output stems (vocals/drums/bass/other) as WAVs. Cached by content hash of the input plus the model file, so a second call with the same input returns the same paths without re-running inference. Default model is htdemucs_ft (best quality, slowest); pass model=\"htdemucs\" for the faster, slightly lower-quality variant. Requires DEMUCS_MODEL_PATH / DEMUCS_FT_MODEL_PATH env vars.",
            json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "model": { "type": "string", "enum": SUPPORTED_MODEL_IDS },
                },
                "required": ["path"],
                "additionalProperties": false,
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };
        let model_id = args
            .model
            .as_deref()
            .unwrap_or(DEFAULT_MODEL_ID)
            .to_string();
        if !SUPPORTED_MODEL_IDS.contains(&model_id.as_str()) {
            return Ok(ToolResult::Error(format!(
                "unsupported model {model_id:?}; expected one of {SUPPORTED_MODEL_IDS:?}"
            )));
        }

        let audio_path = PathBuf::from(&args.path);
        if !audio_path.exists() {
            return Ok(ToolResult::Error(format!(
                "file not found: {}",
                audio_path.display()
            )));
        }

        let decoded = match audio_decoder::decode_file(&audio_path) {
            Ok(d) => d,
            Err(e) => return Ok(ToolResult::Error(format!("decode failed: {e}"))),
        };

        let stem_cache_root = ctx
            .store
            .project_dir()
            .join(".audiograph")
            .join("stem-cache");

        // Try the requested model. On OOM, fall back to htdemucs.
        match run_separation(&model_id, &audio_path, &decoded, &stem_cache_root) {
            Ok(result) => Ok(ToolResult::Ok(stem_result_json(&result, &model_id))),
            Err(SeparateError::Tool(msg)) => Ok(ToolResult::Error(msg)),
            Err(SeparateError::Oom) if model_id != OOM_FALLBACK_MODEL_ID => {
                tracing::warn!(
                    requested_model = %model_id,
                    fallback_model = OOM_FALLBACK_MODEL_ID,
                    "Demucs OOM on {model_id}; retrying with {OOM_FALLBACK_MODEL_ID}",
                );
                match run_separation(
                    OOM_FALLBACK_MODEL_ID,
                    &audio_path,
                    &decoded,
                    &stem_cache_root,
                ) {
                    Ok(result) => Ok(ToolResult::Ok(stem_result_json(
                        &result,
                        OOM_FALLBACK_MODEL_ID,
                    ))),
                    Err(SeparateError::Tool(msg)) => Ok(ToolResult::Error(format!(
                        "OOM fallback to {OOM_FALLBACK_MODEL_ID} also failed: {msg}"
                    ))),
                    Err(SeparateError::Oom) => Ok(ToolResult::Error(format!(
                        "OOM fallback to {OOM_FALLBACK_MODEL_ID} also reported out-of-memory"
                    ))),
                }
            }
            Err(SeparateError::Oom) => Ok(ToolResult::Error(format!(
                "Demucs reported out-of-memory on {OOM_FALLBACK_MODEL_ID}; no smaller fallback available"
            ))),
        }
    }
}

/// Internal error vocabulary for the tool's two-tier OOM fallback.
enum SeparateError {
    /// Plain `ToolResult::Error` text — pass through to the caller.
    Tool(String),
    /// ORT reported an out-of-memory condition; the dispatcher should
    /// retry with a smaller model.
    Oom,
}

/// Run the cache-or-compute path for a single (model_id, audio) pair.
fn run_separation(
    model_id: &str,
    audio_path: &Path,
    decoded: &audio_decoder::DecodedAudio,
    stem_cache_root: &Path,
) -> std::result::Result<StemPaths, SeparateError> {
    let model_path = match resolve_model_path(model_id) {
        Ok(p) => p,
        Err(msg) => return Err(SeparateError::Tool(msg)),
    };

    let model = match get_or_load_model(model_id, &model_path) {
        Ok(m) => m,
        Err(DemucsError::ModelMissing { path }) => {
            return Err(SeparateError::Tool(format!(
                "Demucs model not found at {path}; {INSTALL_HINT}"
            )));
        }
        Err(e) => return Err(SeparateError::Tool(format!("model load failed: {e}"))),
    };

    let key = model.cache_key(decoded);
    let dir = stem_dir_for(stem_cache_root, &key.to_hex());
    if let Some(paths) = read_cached_stems(&dir) {
        tracing::debug!(
            cache_dir = %dir.display(),
            model_id,
            audio = %audio_path.display(),
            "stem-cache hit"
        );
        return Ok(paths);
    }

    let stems = match model.separate(decoded) {
        Ok(s) => s,
        Err(e) => {
            if is_oom_error(&e) {
                return Err(SeparateError::Oom);
            }
            return Err(SeparateError::Tool(format!("stem separation failed: {e}")));
        }
    };

    if stems.len() != 4 {
        return Err(SeparateError::Tool(format!(
            "Demucs returned {} stems, expected 4",
            stems.len()
        )));
    }
    let stems: [ml_demucs::StemBuffer; 4] = match <[_; 4]>::try_from(stems) {
        Ok(arr) => arr,
        Err(_) => {
            return Err(SeparateError::Tool(
                "internal error: stem count check passed but conversion failed".into(),
            ))
        }
    };

    let paths = match write_stems_to_dir(&dir, &stems) {
        Ok(p) => p,
        Err(e) => {
            return Err(SeparateError::Tool(format!(
                "writing stems to {} failed: {e}",
                dir.display()
            )))
        }
    };
    // Sanity: the writer must produce a directory the reader can
    // immediately re-hit. Catches a refactor where the hex layout
    // drifts between writer and reader.
    debug_assert!(read_cached_stems(&dir).is_some());
    Ok(paths)
}

fn stem_result_json(paths: &StemPaths, model_id: &str) -> Value {
    let result: StemSeparationResult = paths.clone().into();
    json!({
        "model": model_id,
        "stems": {
            "vocals": result.vocals.to_string_lossy(),
            "drums": result.drums.to_string_lossy(),
            "bass": result.bass.to_string_lossy(),
            "other": result.other.to_string_lossy(),
        },
        "summary": format!(
            "Separated stems with {model_id}: 4 stems written to {}",
            result.vocals.parent().map(|p| p.display().to_string()).unwrap_or_default(),
        ),
    })
}
