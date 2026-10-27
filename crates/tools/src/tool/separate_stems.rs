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
//! **Not available in this build.** Every invocation ends in a
//! `ToolResult::Error`, by one of three routes:
//!
//! - no model configured — [`DemucsModel::load`] returns
//!   `ModelMissing`;
//! - a model configured but no ONNX Runtime library to load it with —
//!   `RuntimeUnavailable` (this used to hang the agent, #383);
//! - a model configured and loaded — [`DemucsModel::separate`] returns
//!   `NotImplemented`.
//!
//! The last is the one that matters: **dropping an `.onnx` into place
//! does not enable separation.** What is missing is the ORT decode
//! loop, an M28 deliverable, not the file. This module used to say the
//! opposite, and the error called itself actionable (#233).
//!
//! The caching path and tool dispatch are fully wired, so the decoder
//! lands into a shape that already works.
//!
//! ## Out of memory
//!
//! On `out of memory` from ORT the tool says so. It does not retry with
//! another model: `htdemucs` is the only one (`htdemucs_ft`, four
//! models with no pinned ONNX export, is not offered — #385), so there
//! is nothing smaller to fall back to. Detection lives in
//! [`ml_demucs::is_oom_error`].

use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};

use ml_demucs::{DemucsError, DemucsModel, DEFAULT_MODEL_ID, SUPPORTED_MODEL_IDS};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::schema::anthropic_tool;
use crate::{Tool, ToolContext, ToolResult};

/// What to tell a user who cannot separate stems.
///
/// Named a fetch-models shell script until #233; that script has never
/// been in the repository, and following it would not have helped
/// anyway — `DemucsModel::separate` is a stub that always returns
/// `NotImplemented`, so no model file produces stems.
const INSTALL_HINT: &str = "stem separation is not implemented in this build: Demucs inference is \
     a stub, so no DEMUCS_MODEL_PATH setting will produce stems. There is no setup that makes \
     this work today; it is tracked post-v1 (#233)";

/// Process-wide cache of `(model_path, &'static DemucsModel)`. One
/// entry per loaded model path.
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
            "NOT IMPLEMENTED IN THIS BUILD. Would run Demucs stem separation on an audio file \
             and return paths to the four output stems (vocals/drums/bass/other) as WAVs, cached \
             by content hash. Inference is a stub, so this always returns an error and there is \
             no setup that changes that — do not suggest installing a model or setting \
             DEMUCS_MODEL_PATH. Tell the user stem separation is unavailable in this build \
             instead.",
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

    fn invoke(&self, args: Value, _ctx: &mut ToolContext) -> crate::Result<ToolResult> {
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

        // The inference exists in `ml-demucs` (#385) and this tool is wired to
        // it in the next change: until then it answers as it always has.
        Ok(ToolResult::Error(run_separation(&model_id)))
    }
}

/// Find and load the model, then say what this build does: the text of the
/// `ToolResult::Error`. Until the tool is wired to the inference this is the
/// whole of it.
fn run_separation(model_id: &str) -> String {
    let model_path = match resolve_model_path(model_id) {
        Ok(p) => p,
        Err(msg) => return msg,
    };

    match get_or_load_model(model_id, &model_path) {
        Ok(_) => INSTALL_HINT.to_string(),
        Err(DemucsError::ModelMissing { path }) => {
            format!("Demucs model not found at {path}; {INSTALL_HINT}")
        }
        // No ONNX Runtime library. Only the reason goes in: the hint is the
        // sentence the agent is told never to contradict, so it is appended
        // as for a missing model.
        Err(DemucsError::RuntimeUnavailable(reason)) => format!("{reason}; {INSTALL_HINT}"),
        Err(e) => format!("model load failed: {e}"),
    }
}
