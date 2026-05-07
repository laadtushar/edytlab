//! Demucs stem separation (Phase 2, M18).
//!
//! Wraps an HT-Demucs ONNX export and exposes a simple
//! decode-buffer-in / four-stem-buffers-out API. The crate-level
//! contract is intentionally narrow: load a model once
//! ([`DemucsModel::load`]), call [`DemucsModel::separate`] N times. The
//! `separate_stems` tool in the `tools` crate keeps a single model
//! alive for the lifetime of the process.
//!
//! ## Phase-2 scope (sandbox)
//!
//! Sourcing a license-clean Demucs ONNX export plus a license-clean
//! reference clip is a manual M28 deliverable. Until those land, this
//! crate ships:
//!
//! 1. The full public API surface ([`DemucsModel`], [`StemBuffer`],
//!    [`StemPaths`], [`DemucsError`]).
//! 2. The cache-key / on-disk-layout helpers that the `separate_stems`
//!    tool wires up around the model. These are real and exhaustively
//!    unit-tested — the cache contract has to be stable so the tool can
//!    flip from "model missing" to "real inference" by dropping the
//!    `.onnx` into place.
//! 3. A clearly-documented stub for the actual ORT call. When
//!    [`DemucsModel::separate`] is invoked the crate returns
//!    [`DemucsError::NotImplemented`] so the tool layer surfaces an
//!    actionable error instead of panicking. This mirrors how
//!    `ml-whisper` left its decode loop as an empty-result stub in M09.
//!
//! ## Cache layout
//!
//! `<project>/.audiograph/stem-cache/<hex>/{vocals,drums,bass,other}.wav`
//! where `<hex>` is `ContentHash::combine(input_hash, model_id_hash)`
//! rendered as 64-char lowercase hex. The four-files-per-directory
//! shape (rather than a single archive) is deliberate so users can
//! inspect / play the stems out of band.
//!
//! ## OOM fallback
//!
//! Acceptance criterion #4 calls for falling back from `htdemucs_ft`
//! to `htdemucs` when the ORT runtime reports out-of-memory. The
//! detection helper [`is_oom_error`] lives here so both the crate's
//! unit tests and the tool can use the same matcher. The fallback
//! itself is policy that lives in the tool (it's the level that knows
//! how to re-load a different model and warn the user) — see
//! `crates/tools/src/tool/separate_stems.rs`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use audio_decoder::DecodedAudio;
use ml_pipeline::ContentHash;
use serde::{Deserialize, Serialize};

mod cache;

pub use cache::{stem_dir_for, write_stems_to_dir, StemPaths, STEM_FILENAMES, STEM_NAMES};

/// One separated stem in the same shape as [`DecodedAudio`].
///
/// Always interleaved `f32` PCM in `[-1.0, 1.0]`. Sample rate and
/// channel count match the source buffer Demucs was given, so callers
/// can serialise straight back to WAV via `hound` without any
/// resampling.
#[derive(Debug, Clone)]
pub struct StemBuffer {
    /// Stem name: one of `"vocals"`, `"drums"`, `"bass"`, `"other"`.
    pub name: &'static str,
    /// Interleaved PCM samples in `[-1.0, 1.0]`.
    pub samples: Vec<f32>,
    /// Sample rate of `samples`. Matches the source.
    pub sample_rate: u32,
    /// Channel count. Demucs is trained on stereo, so this is almost
    /// always `2`. Mono inputs come back as mono stems.
    pub channels: u16,
}

/// Errors produced by the Demucs wrapper.
///
/// Tool callers should turn [`DemucsError::ModelMissing`] into a
/// [`tools::ToolResult::Error`] carrying the install-script hint, and
/// inspect [`is_oom_error`] on [`DemucsError::Ort`] before retrying
/// with the smaller `htdemucs` model.
#[derive(Debug, thiserror::Error)]
pub enum DemucsError {
    #[error(
        "Demucs model not found at {path}. Install with `scripts/fetch-models.sh` and set DEMUCS_MODEL_PATH"
    )]
    ModelMissing { path: String },

    #[error("unsupported model id: {0:?}; expected \"htdemucs_ft\" or \"htdemucs\"")]
    UnsupportedModelId(String),

    #[error("invalid audio buffer: {0}")]
    InvalidAudio(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("ONNX runtime error: {0}")]
    Ort(String),

    #[error("WAV writer error: {0}")]
    Wav(String),

    /// Real inference is gated on M28 sourcing the ONNX export plus a
    /// license-clean reference clip. The tool layer surfaces this as
    /// an actionable `ToolResult::Error`.
    #[error(
        "Demucs inference is not yet implemented in this build; source assets/models/{model_id}.onnx and re-run (M28)"
    )]
    NotImplemented { model_id: String },
}

/// Crate-local result alias.
pub type Result<T> = std::result::Result<T, DemucsError>;

/// Loaded Demucs ONNX session plus the metadata needed to fold the
/// model identity into the inference cache key.
///
/// Cheap to clone-by-`Arc` so the `separate_stems` tool can share one
/// model between concurrent invocations without rebuilding the
/// session.
pub struct DemucsModel {
    /// Underlying ORT session. Held but not driven yet — see the
    /// module doc-comment for the Phase-2 scope.
    #[allow(dead_code)]
    session: Arc<ort::session::Session>,
    /// Canonical model identifier: `"htdemucs"` or `"htdemucs_ft"`.
    /// Folded into the cache key via [`Self::cache_key`].
    model_id: String,
    /// Content hash of the ONNX file on disk. Fold this (rather than
    /// the model_id string) into the cache key so swapping the file
    /// for a different export of the same nominal model invalidates
    /// prior cache entries.
    model_path_hash: ContentHash,
}

// `ort::session::Session` doesn't implement `Debug`; provide a manual
// stub so callers can put `DemucsModel` behind `OnceLock` etc.
impl std::fmt::Debug for DemucsModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DemucsModel")
            .field("model_id", &self.model_id)
            .field("model_path_hash", &self.model_path_hash.to_hex())
            .finish_non_exhaustive()
    }
}

/// Recognised Demucs model identifiers.
///
/// The tool layer accepts these as strings (Anthropic schema enum).
/// Keeping the list here means the tool layer's schema and the
/// crate's validation can never drift apart.
pub const SUPPORTED_MODEL_IDS: &[&str] = &["htdemucs_ft", "htdemucs"];

/// Default model when the caller doesn't specify one. `htdemucs_ft` is
/// best-quality but ~5x slower than `htdemucs`; the plan picks the
/// quality side for the default and lets users opt down for speed.
pub const DEFAULT_MODEL_ID: &str = "htdemucs_ft";

/// The smaller / faster model we fall back to on OOM.
pub const OOM_FALLBACK_MODEL_ID: &str = "htdemucs";

impl DemucsModel {
    /// Open the ONNX model at `path`.
    ///
    /// Returns [`DemucsError::ModelMissing`] if the path does not
    /// exist (so callers can surface the install-script hint instead
    /// of an opaque ORT error), [`DemucsError::UnsupportedModelId`]
    /// for unknown ids, and propagates any other ORT failure as
    /// [`DemucsError::Ort`].
    pub fn load(model_id: &str, path: &Path) -> Result<Self> {
        if !SUPPORTED_MODEL_IDS.contains(&model_id) {
            return Err(DemucsError::UnsupportedModelId(model_id.to_string()));
        }
        if !path.exists() {
            return Err(DemucsError::ModelMissing {
                path: path.display().to_string(),
            });
        }

        // Hash the model bytes so the cache key invalidates when the
        // file changes (swapped community export, retrain, …). We
        // memory-map-read here for the side effect; the bytes are
        // dropped immediately after hashing.
        let bytes = std::fs::read(path)?;
        let model_path_hash = ContentHash::from_bytes(&bytes);

        let session = ort::session::Session::builder()
            .map_err(|e| DemucsError::Ort(e.to_string()))?
            .commit_from_file(path)
            .map_err(|e| DemucsError::Ort(e.to_string()))?;

        Ok(Self {
            session: Arc::new(session),
            model_id: model_id.to_string(),
            model_path_hash,
        })
    }

    /// The canonical model id the session was loaded with.
    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    /// Hash of the ONNX file on disk at load time. Folded into the
    /// inference-cache key.
    pub fn model_path_hash(&self) -> ContentHash {
        self.model_path_hash
    }

    /// Compute the cache key for `audio` under this model. Combines
    /// the audio bytes hash with the model file hash so that swapping
    /// either invalidates the cache.
    ///
    /// Hashes the raw `f32` PCM little-endian view of `audio.samples`
    /// plus the sample-rate / channel count — two buffers with the
    /// same samples but different rate/channel headers are different
    /// inputs.
    pub fn cache_key(&self, audio: &DecodedAudio) -> ContentHash {
        let input_hash = hash_decoded_audio(audio);
        ContentHash::combine(&[input_hash, self.model_path_hash])
    }

    /// Run stem separation on `audio`.
    ///
    /// Phase-2 stub: returns [`DemucsError::NotImplemented`] until
    /// M28 wires up the real ORT decode loop. The signature is the
    /// final one; the tool layer's caching, error mapping, and OOM
    /// retry path are all wired around this so M28 only has to fill
    /// in the body.
    pub fn separate(&self, audio: &DecodedAudio) -> Result<Vec<StemBuffer>> {
        if !audio.samples.iter().all(|s| s.is_finite()) {
            return Err(DemucsError::InvalidAudio(
                "buffer contains non-finite samples".into(),
            ));
        }
        if audio.sample_rate == 0 || audio.channels == 0 {
            return Err(DemucsError::InvalidAudio(format!(
                "invalid header: sample_rate={} channels={}",
                audio.sample_rate, audio.channels
            )));
        }
        // See module doc for why this is a stub. The real decoder
        // will replace this body without changing the signature.
        Err(DemucsError::NotImplemented {
            model_id: self.model_id.clone(),
        })
    }
}

/// Hash a [`DecodedAudio`] buffer for cache-key purposes.
///
/// Public because `separate_stems` needs to compute the cache key
/// before instantiating the (possibly missing) model — the model file
/// hash is the *other* input to `ContentHash::combine`, but the audio
/// hash on its own is useful for cache lookups against the model id
/// alone, and exposing it lets tests assert key stability without
/// touching ORT.
pub fn hash_decoded_audio(audio: &DecodedAudio) -> ContentHash {
    // blake3 wants `&[u8]`, so fold the f32 samples through their
    // `to_le_bytes` view. Per-sample loop avoids any unsafe
    // transmute. For 30 sec stereo 44.1 kHz that's ~10 MB; still
    // ~10 ms on a modern CPU, dwarfed by inference cost.
    let mut hasher = blake3::Hasher::new();
    hasher.update(&audio.sample_rate.to_le_bytes());
    hasher.update(&audio.channels.to_le_bytes());
    for s in &audio.samples {
        hasher.update(&s.to_le_bytes());
    }
    ContentHash::from_bytes(hasher.finalize().as_bytes())
}

/// Returns `true` if `err` is the kind of ORT failure that the OOM
/// fallback policy should react to.
///
/// The check is deliberately string-based: `ort::Error` is opaque,
/// but every backend (CPU, CoreML, CUDA) tunnels OOM through the
/// same "out of memory" / "OOM" / "memory allocation" substrings in
/// its `Display`. We're loose on purpose — false positives cost a
/// model reload, false negatives cost a hard error to the user.
pub fn is_oom_error(err: &DemucsError) -> bool {
    let DemucsError::Ort(msg) = err else {
        return false;
    };
    let lower = msg.to_ascii_lowercase();
    lower.contains("out of memory")
        || lower.contains("oom")
        || lower.contains("memory allocation")
        || lower.contains("bad_alloc")
}

/// Convenience for the tool layer: walk a stem cache directory and
/// return the [`StemPaths`] if all four files exist, or `None` if
/// anything is missing. Lets the cache hit path stay one
/// `if let Some(paths) = read_cached_stems(&dir) { ... }` block.
pub fn read_cached_stems(dir: &Path) -> Option<StemPaths> {
    let paths = StemPaths::for_dir(dir);
    paths.all_exist().then_some(paths)
}

/// Re-export so callers don't need to depend on `serde` directly to
/// inspect the JSON shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StemSeparationResult {
    pub vocals: PathBuf,
    pub drums: PathBuf,
    pub bass: PathBuf,
    pub other: PathBuf,
}

impl From<StemPaths> for StemSeparationResult {
    fn from(p: StemPaths) -> Self {
        Self {
            vocals: p.vocals,
            drums: p.drums,
            bass: p.bass,
            other: p.other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn audio(samples: Vec<f32>, sr: u32, ch: u16) -> DecodedAudio {
        DecodedAudio {
            samples,
            sample_rate: sr,
            channels: ch,
        }
    }

    #[test]
    fn hash_decoded_audio_is_deterministic() {
        let a = audio(vec![0.1, 0.2, -0.3, 0.4], 44_100, 2);
        let b = audio(vec![0.1, 0.2, -0.3, 0.4], 44_100, 2);
        assert_eq!(hash_decoded_audio(&a), hash_decoded_audio(&b));
    }

    #[test]
    fn hash_changes_with_sample_rate() {
        let a = audio(vec![0.1, 0.2], 44_100, 1);
        let b = audio(vec![0.1, 0.2], 48_000, 1);
        assert_ne!(hash_decoded_audio(&a), hash_decoded_audio(&b));
    }

    #[test]
    fn hash_changes_with_channels() {
        let a = audio(vec![0.1, 0.2], 44_100, 1);
        let b = audio(vec![0.1, 0.2], 44_100, 2);
        assert_ne!(hash_decoded_audio(&a), hash_decoded_audio(&b));
    }

    #[test]
    fn load_rejects_unsupported_model_id() {
        let err = DemucsModel::load("htdemucs_xl", Path::new("/nope.onnx")).unwrap_err();
        match err {
            DemucsError::UnsupportedModelId(id) => assert_eq!(id, "htdemucs_xl"),
            other => panic!("expected UnsupportedModelId, got {other:?}"),
        }
    }

    #[test]
    fn load_reports_model_missing_for_nonexistent_path() {
        let err = DemucsModel::load("htdemucs", Path::new("/no/such/demucs.onnx")).unwrap_err();
        match err {
            DemucsError::ModelMissing { path } => assert!(path.contains("demucs.onnx")),
            other => panic!("expected ModelMissing, got {other:?}"),
        }
    }

    #[test]
    fn is_oom_error_matches_common_phrasings() {
        assert!(is_oom_error(&DemucsError::Ort("out of memory".into())));
        assert!(is_oom_error(&DemucsError::Ort(
            "ORT runtime: OOM during allocation".into()
        )));
        assert!(is_oom_error(&DemucsError::Ort(
            "memory allocation failed for tensor of size N".into()
        )));
        assert!(is_oom_error(&DemucsError::Ort("std::bad_alloc".into())));
        assert!(!is_oom_error(&DemucsError::Ort("shape mismatch".into())));
        assert!(!is_oom_error(&DemucsError::InvalidAudio(
            "out of memory".into()
        )));
    }
}
