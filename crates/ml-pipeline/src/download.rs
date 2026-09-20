//! Model-fetch stub.
//!
//! Phase-2 scope: real download/verify logic is post-v1. Models are
//! resolved by callers via env vars (`WHISPER_MODEL_PATH`,
//! `DEMUCS_MODEL_PATH`, …). [`fetched_model_path`] exists so downstream
//! crates can call a stable API today and have it route to a real
//! downloader once it lands.
//!
//! This used to say models were pre-fetched by a fetch-models shell
//! script. No such script has ever been in the repository (#233). Nor
//! would it have helped: the Whisper decoder and Demucs inference are
//! both stubs, so there is no model file that produces output. The env
//! vars above are the *intended* mechanism, not a working one.
//!
//! Currently always returns [`Error::MissingRuntime`], and has no
//! callers. Callers should read that as "this feature is unavailable",
//! not as "the model has not been fetched yet" — there is nothing to
//! fetch it with, and a fetched model would change nothing.
//!
//! In particular, do not fall back to an install hint. That is the
//! advice this module used to give, and it is what sent people looking
//! for a script that does not exist.

use std::path::PathBuf;

use crate::{Error, Result};

/// Resolve `model_id` to a local on-disk path, fetching from a registry
/// if necessary. Stub: always errors today.
pub fn fetched_model_path(_model_id: &str) -> Result<PathBuf> {
    Err(Error::MissingRuntime)
}
