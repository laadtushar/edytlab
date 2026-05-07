//! Model-fetch stub.
//!
//! Phase-2 scope: real download/verify logic is post-v1. For now models
//! are pre-fetched by `scripts/fetch-models.sh` into `assets/models/`
//! and resolved by callers via env vars (`WHISPER_MODEL_PATH`,
//! `DEMUCS_MODEL_PATH`, …). [`fetched_model_path`] exists so downstream
//! crates can call a stable API today and have it route to a real
//! downloader once it lands.
//!
//! Currently always returns [`Error::MissingRuntime`] — callers should
//! treat this as "model not pre-fetched yet" and fall back to their
//! own env-var lookup with an install-script hint.

use std::path::PathBuf;

use crate::{Error, Result};

/// Resolve `model_id` to a local on-disk path, fetching from a registry
/// if necessary. Stub: always errors today.
pub fn fetched_model_path(_model_id: &str) -> Result<PathBuf> {
    Err(Error::MissingRuntime)
}
