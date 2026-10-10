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
//! `ort` is configured with `load-dynamic`, so the process needs an ONNX
//! Runtime library to load: the file `ORT_DYLIB_PATH` names, or
//! `libonnxruntime.{so,dylib}` / `onnxruntime.dll` next to the
//! executable. Nothing ships that library yet (#383).
//!
//! Left alone, `ort` **hangs** when it cannot load the library: it
//! deadlocks inside its own initialisation, and the calling thread never
//! returns. So every caller that builds an `ort` session goes through
//! [`runtime::ensure`] first, which turns a missing or unloadable
//! library into [`Error::MissingRuntime`] / [`Error::RuntimeLoad`]. See
//! the [`runtime`] module for why that matters. Tests that build real
//! sessions are `#[ignore]`d — see `tests/cache_smoke.rs`.

use std::io;
use std::path::PathBuf;

mod cache;
mod download;
mod onnx_session;
pub mod runtime;

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

    /// No ONNX Runtime library exists at any of the places
    /// [`runtime::ensure`] looks. `searched` lists them all, so the
    /// message can say where a copy would be picked up.
    ///
    /// Distinct from [`Error::Ort`] so callers can tell "this machine
    /// has no runtime" from "the runtime rejected the model".
    #[error(
        "ONNX Runtime library not found; looked in: {}. edytlab does not ship it yet (#383); \
         ORT_DYLIB_PATH can point at a local copy (ONNX Runtime 1.{} or newer)",
        list_paths(.searched),
        ort::MINOR_VERSION
    )]
    MissingRuntime { searched: Vec<PathBuf> },

    /// A library file exists where [`runtime::ensure`] looked, but it
    /// could not be loaded: not an ONNX Runtime, the wrong architecture,
    /// or older than the version `ort` needs. An explicit path that
    /// fails is reported, never skipped in favour of another one.
    #[error("ONNX Runtime at {} could not be loaded: {reason}", .path.display())]
    RuntimeLoad { path: PathBuf, reason: String },
}

/// Comma-separated paths for [`Error::MissingRuntime`]'s message.
fn list_paths(paths: &[PathBuf]) -> String {
    if paths.is_empty() {
        return "nowhere (ORT_DYLIB_PATH is unset and the executable's directory is unknown)"
            .to_string();
    }
    paths
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

impl From<ort::Error> for Error {
    fn from(value: ort::Error) -> Self {
        Error::Ort(value.to_string())
    }
}

/// Crate-local `Result` alias.
pub type Result<T> = std::result::Result<T, Error>;
