//! `transcribe` tool — decode an audio file, resample to 16 kHz mono,
//! run the Whisper-base ONNX model, and append a session node carrying
//! the resulting transcript.
//!
//! Side effect: on success the new session head's `state.transcript`
//! field is set to the produced words. The result JSON also returns the
//! word list for the model's convenience.
//!
//! Missing-model path: `WHISPER_MODEL_PATH` env var is consulted at
//! invocation time. If unset, or the file does not exist, the tool
//! returns a structured `ToolResult::Error` with the install hint
//! instead of panicking. This satisfies M09 acceptance criterion #4.
//!
//! Model-reuse: the [`WhisperModel`] is cached process-wide behind a
//! [`OnceLock`] keyed by the model path, so 10 invocations against the
//! same model load it once. Switching `WHISPER_MODEL_PATH` mid-process
//! is unsupported (the cached entry wins) — Phase 2 may add an explicit
//! reload tool if that becomes a real workflow.

use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};

use ml_whisper::{resample_to_16khz_mono, WhisperError, WhisperModel, Word};
use serde::Deserialize;
use serde_json::{json, Value};
use session::{Transcript, TranscriptWord};

use crate::schema::{anthropic_tool, object_schema};
use crate::tool::util::{append_state, load_head_state};
use crate::{Tool, ToolContext, ToolResult};

const INSTALL_HINT: &str = "install Whisper model: run `scripts/fetch-models.sh` and set WHISPER_MODEL_PATH to the resulting .onnx path";

/// Process-wide cache of `(model_path, WhisperModel)`. Phase 1 only ever
/// holds a single entry; the `RwLock` is a forward-compat hedge against
/// a Phase-2 multi-model dispatcher.
fn model_cache() -> &'static RwLock<Option<(PathBuf, &'static WhisperModel)>> {
    static CACHE: OnceLock<RwLock<Option<(PathBuf, &'static WhisperModel)>>> = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(None))
}

/// Fetch the cached [`WhisperModel`] for `path`, loading it on first
/// use. The returned `&'static WhisperModel` is a reference into a
/// `Box::leak`-ed singleton — fine for a process-wide cache.
fn get_or_load_model(path: &Path) -> Result<&'static WhisperModel, WhisperError> {
    {
        let guard = model_cache()
            .read()
            .expect("model cache lock poisoned (read)");
        if let Some((cached_path, model)) = guard.as_ref() {
            if cached_path == path {
                return Ok(*model);
            }
        }
    }
    let mut guard = model_cache()
        .write()
        .expect("model cache lock poisoned (write)");
    if let Some((cached_path, model)) = guard.as_ref() {
        if cached_path == path {
            return Ok(*model);
        }
    }
    let model = WhisperModel::load(path)?;
    let leaked: &'static WhisperModel = Box::leak(Box::new(model));
    *guard = Some((path.to_path_buf(), leaked));
    Ok(leaked)
}

#[derive(Debug, Deserialize)]
struct Args {
    path: String,
}

pub struct TranscribeTool;

impl Tool for TranscribeTool {
    fn name(&self) -> &'static str {
        "transcribe"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "transcribe",
            "Transcribe an audio file using the local Whisper-base ONNX model. Resamples internally to 16 kHz mono. Appends a new session node whose state carries the transcript; returns the produced words. Requires WHISPER_MODEL_PATH to point at the .onnx file (use scripts/fetch-models.sh).",
            object_schema(&[("path", "string", true)]),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };

        let model_path = match std::env::var("WHISPER_MODEL_PATH") {
            Ok(p) => PathBuf::from(p),
            Err(_) => {
                return Ok(ToolResult::Error(format!(
                    "WHISPER_MODEL_PATH env var not set; {INSTALL_HINT}"
                )))
            }
        };

        let model = match get_or_load_model(&model_path) {
            Ok(m) => m,
            Err(WhisperError::ModelMissing { path }) => {
                return Ok(ToolResult::Error(format!(
                    "Whisper model not found at {path}; {INSTALL_HINT}"
                )))
            }
            Err(e) => return Ok(ToolResult::Error(format!("model load failed: {e}"))),
        };

        let audio_path = PathBuf::from(&args.path);
        if !audio_path.exists() {
            return Ok(ToolResult::Error(format!(
                "file not found: {}",
                audio_path.display()
            )));
        }

        // Decode -> resample -> transcribe. Each step folds its own
        // error into a `ToolResult::Error` so the model can recover.
        let decoded = match audio_decoder::decode_file(&audio_path) {
            Ok(d) => d,
            Err(e) => return Ok(ToolResult::Error(format!("decode failed: {e}"))),
        };
        let mono16k =
            match resample_to_16khz_mono(&decoded.samples, decoded.sample_rate, decoded.channels) {
                Ok(s) => s,
                Err(e) => return Ok(ToolResult::Error(format!("resample failed: {e}"))),
            };
        let words = match model.transcribe(&mono16k) {
            Ok(w) => w,
            Err(e) => return Ok(ToolResult::Error(format!("transcribe failed: {e}"))),
        };

        // Mutate the current session head: clone, attach transcript,
        // append. If no session is loaded the user gets a clear hint.
        let mut state = match load_head_state(ctx) {
            Ok(s) => s,
            Err(msg) => return Ok(ToolResult::Error(msg)),
        };
        state.transcript = Some(Transcript {
            words: words.iter().map(word_to_session_word).collect(),
        });

        let new_id = match append_state(
            ctx,
            state,
            format!(
                "transcribe {} ({} words)",
                audio_path.display(),
                words.len()
            ),
        ) {
            Ok(id) => id,
            Err(msg) => return Ok(ToolResult::Error(msg)),
        };

        Ok(ToolResult::Ok(json!({
            "node_id": new_id.to_hex(),
            "words": words.iter().map(word_to_json).collect::<Vec<_>>(),
            "summary": format!(
                "Transcribed {} ({} words); new head {}",
                audio_path.display(),
                words.len(),
                new_id.to_hex(),
            ),
        })))
    }
}

fn word_to_session_word(w: &Word) -> TranscriptWord {
    TranscriptWord {
        text: w.text.clone(),
        start_s: w.start_s,
        end_s: w.end_s,
        confidence: w.confidence,
    }
}

fn word_to_json(w: &Word) -> Value {
    json!({
        "text": w.text,
        "start_s": w.start_s,
        "end_s": w.end_s,
        "confidence": w.confidence,
    })
}
