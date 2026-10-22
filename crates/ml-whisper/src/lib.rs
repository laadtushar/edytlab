//! Whisper transcription (Phase 1, M09; the decoder is #384).
//!
//! A small, sync wrapper around the ONNX Whisper-base export that
//! `ml_pipeline::models::whisper_base()` pins
//! (`onnx-community/whisper-base_timestamped`). Load the model once with
//! [`WhisperModel::load`], then call [`transcribe`] as often as needed.
//!
//! ## What is here
//!
//! * [`mel`], [`tokenizer`], [`decode`], [`timing`]: the log-mel spectrogram, the
//!   token decoder, the greedy decoding loop behind a trait, and word timing
//!   from cross-attention. Each is ported from OpenAI's `whisper` and checked
//!   against it without a model.
//! * [`export`]: the names, types and shapes the ONNX files must have, checked
//!   by [`WhisperModel::load`] so a different export is an error that says what
//!   differs.
//! * `cached`: the merged decoder's key-value cache as a [`decode::DecoderStep`],
//!   and the attention it collects; the cache hand-over is tested with a fake
//!   runner.
//! * `chunk`: 30 s windows and where the next one starts.
//! * `refine`: word edges pulled in to where there is sound.
//! * [`resample_to_16khz_mono`]: any rate and channel count to 16 kHz mono,
//!   without the delay an FFT resampler adds.
//! * [`transcribe`]: all of it together. `tests/transcribe_known_words.rs` runs
//!   it on a real model; CI, which has none, runs the pieces.
//!
//! ## What the app does with it: nothing yet
//!
//! The `transcribe` tool does not call [`transcribe`]. It answers
//! [`WhisperError::NotImplemented`] whatever model is configured, so the app
//! still reports that speech-to-text is not implemented in this build. Wiring
//! the tool to this crate (the model folder from Settings, progress, the cache,
//! the download prompt) and changing every line of copy that says so is the next
//! change on #384. Until it lands, do not describe transcription as working.
//!
//! The stub used to return an empty `Ok`, a success carrying an empty
//! transcript, which a caller cannot tell from "this recording contains no
//! speech" (#233). This crate must not go back to an empty success for
//! something it did not do.

mod cached;
mod chunk;
pub mod decode;
pub mod export;
pub mod mel;
mod model;
mod ort_io;
mod refine;
mod resample;
pub mod timing;
pub mod tokenizer;

pub use model::{transcribe, Transcript, WhisperModel, DECODER_FILE, ENCODER_FILE, TOKENIZER_FILE};
pub use resample::resample_to_16khz_mono;

/// One word of the transcript with timing and confidence.
///
/// `start_s`/`end_s` are seconds from the start of the audio buffer
/// passed to [`transcribe`]. `confidence` is in `[0.0, 1.0]`.
///
/// Mirrors `session::TranscriptWord` field-for-field; the `transcribe`
/// tool maps between the two when writing the session-state side effect.
#[derive(Debug, Clone, PartialEq)]
pub struct Word {
    pub text: String,
    pub start_s: f32,
    pub end_s: f32,
    pub confidence: f32,
}

/// Errors produced by the Whisper wrapper.
///
/// Tool callers should turn [`WhisperError::ModelMissing`] into a
/// `ToolResult::Error` carrying the install-script hint, so the model
/// can recover without the host process panicking.
#[derive(Debug, thiserror::Error)]
pub enum WhisperError {
    #[error(
        "Whisper model not found at {path}. Set WHISPER_MODEL_PATH to the folder of an ONNX \
         Whisper export. Note that speech-to-text is not implemented in this build, so supplying \
         a model will not yet produce a transcript (#233)."
    )]
    ModelMissing { path: String },

    /// No usable ONNX Runtime library (see
    /// [`ml_pipeline::runtime::ensure`]). Reported instead of building a
    /// session, because `ort` panics when it cannot load the library.
    ///
    /// The message says what the feature does today too: a user who
    /// goes and installs the runtime on the strength of this error would
    /// otherwise reach the next one believing they were nearly done.
    #[error(
        "{0}. Speech-to-text is not implemented in this build either, so installing ONNX \
         Runtime will not yet produce a transcript (#384)"
    )]
    RuntimeUnavailable(String),

    /// The folder or the ONNX files in it are not the export this build reads:
    /// a file is missing, or a session's inputs and outputs are not the
    /// ones `export` lists. The message says which.
    #[error("not the expected Whisper export: {0}")]
    UnexpectedExport(String),

    /// ONNX Runtime could not open a model file (corrupt, truncated, or an
    /// opset it does not support).
    #[error("could not load the model: {0}")]
    ModelLoad(String),

    /// A session run failed or returned something other than its shapes.
    #[error("inference failed: {0}")]
    Inference(String),

    /// `tokenizer.json` could not be read or is not the byte-level BPE
    /// tokenizer of the export.
    #[error("tokenizer: {0}")]
    Tokenizer(#[from] tokenizer::TokenizerError),

    /// The decode loop failed (a decoder step, logits of the wrong shape).
    #[error("decoding failed: {0}")]
    Decode(decode::DecodeError),

    /// Word timing failed.
    #[error("word timing failed: {0}")]
    Timing(#[from] timing::TimingError),

    /// The caller's `cancel` answered `true`.
    #[error("transcription cancelled")]
    Cancelled,

    /// A language code the model has no token for.
    #[error("unknown language {0:?}: the model has no <|{0}|> token")]
    UnknownLanguage(String),

    #[error("rubato resampler construction failed: {0}")]
    ResamplerInit(#[from] rubato::ResamplerConstructionError),

    #[error("rubato resampler process failed: {0}")]
    ResamplerProcess(#[from] rubato::ResampleError),

    #[error("invalid audio: {0}")]
    InvalidAudio(String),

    /// Speech-to-text is not available to the app in this build (#233, #384).
    ///
    /// Not returned by this crate: [`transcribe`] runs the model. It is what a
    /// caller that has not been wired to [`transcribe`] answers, in place of an
    /// empty transcript. An empty success is a success carrying zero words, and
    /// every caller reads it as "this audio contains no speech" — so a
    /// correctly configured model produced a silently wrong answer rather than
    /// a failure anyone could act on. The `transcribe` tool is that caller
    /// today. Mirrors `ml_demucs::DemucsError::NotImplemented`.
    #[error(
        "speech-to-text is not implemented in this build — the app does not run the speech model \
         yet, so no model file will produce a transcript. Tracked post-v1; there is no \
         configuration that makes this work today."
    )]
    NotImplemented,
}

impl From<export::ExportError> for WhisperError {
    fn from(e: export::ExportError) -> Self {
        WhisperError::UnexpectedExport(e.to_string())
    }
}

impl From<decode::DecodeError> for WhisperError {
    fn from(e: decode::DecodeError) -> Self {
        match e {
            decode::DecodeError::Cancelled => WhisperError::Cancelled,
            other => WhisperError::Decode(other),
        }
    }
}

pub type Result<T> = std::result::Result<T, WhisperError>;

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn load_returns_model_missing_for_nonexistent_path() {
        let path = Path::new("/definitely/does/not/exist/whisper");
        let err = WhisperModel::load(path).unwrap_err();
        match err {
            WhisperError::ModelMissing { path: p } => {
                assert!(p.contains("whisper"));
            }
            other => panic!("expected ModelMissing, got {other:?}"),
        }
    }

    #[test]
    fn what_the_unwired_tool_answers_says_so_without_calling_the_model_a_stub() {
        // The tool answers this until it is rewired. It is checked on the
        // error's text, which is what the tool shows.
        let msg = WhisperError::NotImplemented.to_string();
        assert!(msg.contains("not implemented in this build"), "{msg}");
        assert!(!msg.contains("stub"), "{msg}");
    }

    #[test]
    fn a_cancelled_decode_is_a_cancelled_transcription() {
        assert!(matches!(
            WhisperError::from(decode::DecodeError::Cancelled),
            WhisperError::Cancelled
        ));
        assert!(matches!(
            WhisperError::from(decode::DecodeError::NoViableToken),
            WhisperError::Decode(_)
        ));
    }
}
