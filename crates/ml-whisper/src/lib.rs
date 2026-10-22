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
//! ## What the app does with it
//!
//! The `transcribe` tool (`crates/tools/src/tool/transcribe.rs`) is the one
//! caller. It finds the model folder (`WHISPER_MODEL_PATH`, else the speech
//! model the user downloaded in Settings), loads it with
//! [`WhisperModel::load`], and runs [`transcribe_observed`] off the store lock,
//! with a progress hook and a cancel check. This crate never downloads
//! anything and never reads a setting: it is given a folder.
//!
//! An empty transcript from [`transcribe`] means the recording had no speech
//! the model would write down, and nothing else. It used to be what a stub
//! returned for a model that was never run (#233), which a caller cannot tell
//! from silence, so this crate must not return an empty success for something
//! it did not do: every path that cannot transcribe is an error.

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

pub use model::{
    transcribe, transcribe_observed, Transcript, WhisperModel, DECODER_FILE, ENCODER_FILE,
    TOKENIZER_FILE,
};
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
        "Whisper model not found at {path}. WHISPER_MODEL_PATH names the folder of the ONNX \
         Whisper export (onnx/encoder_model.onnx, onnx/decoder_model_merged_int8.onnx and \
         tokenizer.json)."
    )]
    ModelMissing { path: String },

    /// No usable ONNX Runtime library (see
    /// [`ml_pipeline::runtime::ensure`]). Reported instead of building a
    /// session, because `ort` panics when it cannot load the library.
    ///
    /// The message is the runtime's own reason; what to do about it is the
    /// caller's to say (the `transcribe` tool names `ORT_DYLIB_PATH`).
    #[error("{0}")]
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
    fn no_error_of_this_crate_says_the_feature_is_missing() {
        // The model runs. A message that still said "not implemented in this
        // build" would send a user who has done everything right away from
        // the cause (#384).
        let errors = [
            WhisperError::ModelMissing { path: "/m".into() },
            WhisperError::RuntimeUnavailable("no ONNX Runtime library was found".into()),
            WhisperError::UnexpectedExport("x".into()),
            WhisperError::ModelLoad("x".into()),
            WhisperError::Inference("x".into()),
            WhisperError::Cancelled,
            WhisperError::UnknownLanguage("xx".into()),
            WhisperError::InvalidAudio("x".into()),
        ];
        for e in errors {
            let msg = e.to_string().to_lowercase();
            assert!(
                !msg.contains("not implemented") && !msg.contains("stub"),
                "{e:?}: {msg}"
            );
        }
        // The missing-model message still names the variable a developer sets.
        assert!(WhisperError::ModelMissing { path: "/m".into() }
            .to_string()
            .contains("WHISPER_MODEL_PATH"));
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
