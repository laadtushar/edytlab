//! ML pipeline (Phase 2, M17).
//!
//! Generic ONNX Runtime hosting plus a content-hashed inference cache.
//! Downstream ML crates (`ml-whisper`, `ml-demucs`, `audio-analysis`) all
//! share this single ORT setup so the workspace doesn't pull two ONNX
//! Runtime configurations and so the inference cache can deduplicate
//! across tools.
//!
//! ## Design
//!
//! - [`ModelRegistry`] keeps one `Arc<ort::Session>` per `model_id`.
//!   Loading the same model twice returns the same `Arc`. Registry
//!   creation does not touch the runtime; sessions are built lazily by
//!   [`ModelRegistry::load`].
//! - [`InferenceCache`] is an on-disk content-addressed store rooted at
//!   `<project>/.audiograph/inference-cache/`. Keys are blake3 hashes
//!   (callers fold the model file's hash into the key so cache entries
//!   are invalidated when the model changes — see
//!   [`ContentHash::from_bytes`]). Values are JSON-serialized via
//!   `serde_json` and atomically written via `tempfile::NamedTempFile::persist`.
//! - [`ExecProvider`] picks the ORT execution provider. CoreML on Mac,
//!   CUDA where the user opts in, CPU otherwise. Non-supported EPs fall
//!   back to CPU with a `tracing::warn!` rather than panicking, which
//!   keeps the dev sandbox (Linux, no GPU) building cleanly.
//!
//! ## Runtime requirements
//!
//! `ort` is configured with `load-dynamic`, so the binary needs
//! `ORT_DYLIB_PATH` set (or `libonnxruntime.{so,dylib,dll}` next to the
//! binary). Tests that build sessions check the env var and skip with a
//! printed notice when it's absent — see `tests/cache_smoke.rs`.

use std::io;

mod cache;
mod download;
mod onnx_session;

pub use cache::{ContentHash, InferenceCache};
pub use download::fetched_model_path;
pub use onnx_session::{ExecProvider, ModelRegistry};

/// Crate-wide error type. One enum, lives at the crate root so callers
/// only ever match against a single shape.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Filesystem error from cache I/O or model loading.
    #[error("io error: {0}")]
    Io(#[from] io::Error),

    /// ONNX Runtime error wrapped as a string. `ort::Error` is not
    /// `Clone`, and we want this enum to be cheaply convertible across
    /// thread boundaries; stringifying at the boundary is fine.
    #[error("onnxruntime error: {0}")]
    Ort(String),

    /// Inference-cache I/O / serialization issue distinct from a raw
    /// `io::Error` (e.g. atomic-rename or persist failure).
    #[error("inference cache: {0}")]
    Cache(String),

    /// JSON serialization or deserialization failure for cache values.
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    /// `ORT_DYLIB_PATH` was unset or the dylib couldn't be opened. This
    /// is distinct from a generic ORT error so callers can present a
    /// structured install-script hint.
    #[error("onnxruntime dynamic library not available; set ORT_DYLIB_PATH")]
    MissingRuntime,
}

impl From<ort::Error> for Error {
    fn from(value: ort::Error) -> Self {
        Error::Ort(value.to_string())
    }
}

/// Crate-local `Result` alias.
pub type Result<T> = std::result::Result<T, Error>;
