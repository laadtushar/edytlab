//! `transcribe` tool — decode an audio file, resample to 16 kHz mono,
//! hand it to the Whisper wrapper, and append a session node carrying
//! the resulting transcript.
//!
//! **Not available in this build.** The tool is not wired to
//! `ml_whisper::transcribe`, which runs the pinned export end to end and which
//! nothing in the app calls yet, so every invocation ends in a
//! `ToolResult::Error` and no session node is ever appended. The steps above
//! describe the shape the tool will have, not what it does today (#233, #384).
//! Wiring it (the model folder from Settings, progress, the cache, the
//! download prompt) and every line of copy that says it is unavailable is the
//! next change on #384.
//!
//! The failure paths differ only in wording, and every one of them says the
//! same thing, [`INSTALL_HINT`]: transcription is not implemented in this
//! build and no setup changes that.
//!
//! - no model configured: `WHISPER_MODEL_PATH` unset, or naming a folder that
//!   is not there;
//! - a model configured but no ONNX Runtime library to load it with:
//!   [`WhisperError::RuntimeUnavailable`] (this used to hang the agent, #383);
//! - a model configured that cannot be loaded: not the pinned export
//!   ([`WhisperError::UnexpectedExport`]), a tokenizer that does not parse
//!   ([`WhisperError::Tokenizer`]), a file the runtime will not open
//!   ([`WhisperError::ModelLoad`]), or anything else. The reason is logged,
//!   not handed to the agent: it reads whatever it is given as something to
//!   act on, and there is nothing to act on;
//! - a model configured and loaded: [`WhisperError::NotImplemented`].
//!
//! None is a setup problem: every message ends by saying no setup makes
//! transcription work. The first used to be called an "install hint" and
//! to name a fetch-models script that has never existed in this
//! repository.
//!
//! Side effect, once the tool is wired: the new session head's
//! `state.transcript` is set to the produced words, and the result JSON
//! also returns the word list for the model's convenience.
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

/// What to tell a user who cannot transcribe.
///
/// It used to name a fetch-models shell script, which has never existed
/// in this repository — so the error, the tool schema the model reads,
/// and the in-app card all sent people to run a file that is not there
/// (#233). Worse, following it perfectly would not have helped: nothing in
/// the app runs the speech model yet (`ml_whisper::transcribe` exists, and
/// this tool does not call it), so no model file produces a transcript.
///
/// The hint now says that, because "you are missing a step" and "this
/// does not work yet" call for completely different responses from
/// both the user and the agent.
const INSTALL_HINT: &str = "speech-to-text is not implemented in this build: the app does not \
     run the speech model yet, so no WHISPER_MODEL_PATH setting will produce a transcript. There \
     is no setup that makes this work today; it is tracked post-v1 (#233)";

/// The answer for a model that could not be loaded.
///
/// Every load error ends in the same place, [`INSTALL_HINT`], because none of
/// them is a step the user or the agent can take: the model would not be run
/// if it loaded. A missing runtime and a missing model keep their own first
/// words (the runtime's reason; where the model was looked for), which is all
/// the agent is told. Any other reason (not the pinned export, a tokenizer
/// that does not parse, a file the runtime will not open) is logged for
/// whoever set `WHISPER_MODEL_PATH`, and not repeated to the agent, which
/// would try to fix it.
fn load_failure(error: &WhisperError) -> String {
    match error {
        WhisperError::ModelMissing { path } => {
            format!("Whisper model not found at {path}; {INSTALL_HINT}")
        }
        // Only the reason goes in: the error's own text repeats "not
        // implemented in this build" (the hint already says it) and the agent
        // reads whatever it is given as something to act on. The hint is the
        // sentence it is told never to contradict, so it is appended as for a
        // missing model.
        WhisperError::RuntimeUnavailable(reason) => format!("{reason}; {INSTALL_HINT}"),
        other => {
            tracing::warn!(error = %other, "the transcribe tool could not load WHISPER_MODEL_PATH");
            format!("the speech model could not be loaded; {INSTALL_HINT}")
        }
    }
}

/// Run the model on `mono16k`. The one place the tool would call
/// `ml_whisper::transcribe`; it does not yet (#384), so it says what every
/// path of this tool says.
fn run_model(_model: &WhisperModel, _mono16k: &[f32]) -> Result<Vec<Word>, WhisperError> {
    Err(WhisperError::NotImplemented)
}

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
            "NOT IMPLEMENTED IN THIS BUILD. Speech-to-text would transcribe an audio file with \
             a local Whisper ONNX model, resampling to 16 kHz mono and appending a session node \
             carrying the transcript. The app does not run the speech model yet, so this always \
             returns an error and there is no setup that changes that — do not suggest installing a model or setting \
             WHISPER_MODEL_PATH. Tell the user transcription is unavailable in this build \
             instead. Tools that consume a transcript (cut_words, and the text-based editing \
             flow) are unavailable for the same reason.",
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
            Err(e) => return Ok(ToolResult::Error(load_failure(&e))),
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
        let words = match run_model(model, &mono16k) {
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

        // Record which model produced this (#163). Model weights are not
        // part of the session and should not be, so this stays
        // permanently non-replayable — what the record buys is
        // diagnosis: a transcript that looks wrong can be traced to the
        // model that made it.
        let op = session::NodeOp::new(
            "transcribe".to_string(),
            json!({ "path": args.path }),
            env!("CARGO_PKG_VERSION").to_string(),
        )
        .not_reproducible()
        .with_inputs(crate::provenance::model_inputs(
            "whisper",
            // The file name, not the path: provenance is meant to be
            // shareable (#162 exports it as a recipe), and an absolute
            // path names the machine it ran on.
            model_path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown"),
        ));
        if let Err(e) = ctx.store.set_op(new_id, op) {
            tracing::warn!(error = %e, "failed to record transcribe provenance");
        }

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

#[cfg(test)]
mod tests {
    use super::*;
    use ml_whisper::tokenizer::TokenizerError;

    /// Every way a model can fail to load. A new `WhisperError` variant that
    /// can come out of `WhisperModel::load` belongs here too.
    fn load_errors() -> Vec<WhisperError> {
        vec![
            WhisperError::ModelMissing {
                path: "/models/whisper".into(),
            },
            WhisperError::RuntimeUnavailable("no ONNX Runtime library was found".into()),
            WhisperError::UnexpectedExport("/models/whisper has no onnx/encoder_model.onnx".into()),
            WhisperError::ModelLoad("/models/whisper/onnx/encoder_model.onnx: bad protobuf".into()),
            WhisperError::Tokenizer(TokenizerError::Unsupported("not byte-level".into())),
            WhisperError::Inference("the encoder session's lock is poisoned".into()),
            WhisperError::UnknownLanguage("xx".into()),
            WhisperError::NotImplemented,
        ]
    }

    #[test]
    fn every_load_error_is_answered_as_not_implemented_with_the_hint() {
        for error in load_errors() {
            let answer = load_failure(&error);
            assert!(
                answer.ends_with(INSTALL_HINT),
                "{error:?} is answered {answer:?}, which does not end in the hint"
            );
            assert_eq!(
                answer.matches("not implemented in this build").count(),
                1,
                "{error:?}: it should say so once: {answer}"
            );
        }
    }

    #[test]
    fn a_load_error_the_agent_cannot_act_on_is_not_repeated_to_it() {
        // "the folder has to hold ..." reads as a setup step, and the hint says
        // there is none.
        for error in [
            WhisperError::UnexpectedExport("/m has no onnx/encoder_model.onnx".into()),
            WhisperError::ModelLoad("/m/onnx/decoder.onnx: bad protobuf".into()),
            WhisperError::Tokenizer(TokenizerError::Unsupported("not byte-level".into())),
        ] {
            let answer = load_failure(&error);
            assert!(
                !answer.contains("/m") && !answer.contains("protobuf"),
                "{error:?} leaks its reason: {answer}"
            );
        }
        // The two the agent has always been told, in their own words.
        assert!(load_failure(&WhisperError::ModelMissing { path: "/x".into() }).contains("/x"));
        assert!(
            load_failure(&WhisperError::RuntimeUnavailable("no ONNX Runtime".into()))
                .starts_with("no ONNX Runtime;")
        );
    }

    #[test]
    fn the_hint_and_the_schema_say_what_the_frontend_and_the_guards_look_for() {
        // `isWhisperError` in Chat.tsx matches on these, and ml_setup_is_real.rs
        // on the schema's.
        assert!(INSTALL_HINT.starts_with("speech-to-text is not implemented"));
        assert!(INSTALL_HINT.contains("WHISPER_MODEL_PATH"));
        assert!(
            !INSTALL_HINT.contains("stub"),
            "it does not call anything a stub"
        );
    }
}
