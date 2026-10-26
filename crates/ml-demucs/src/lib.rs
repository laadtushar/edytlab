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
//!    unit-tested — the cache contract has to be stable so that the
//!    decode loop, when it lands, drops into a shape that already
//!    works.
//! 3. A clearly-documented stub for the actual ORT call. When
//!    [`DemucsModel::separate`] is invoked the crate returns
//!    [`DemucsError::NotImplemented`] instead of panicking.
//!
//! One piece of the decoder exists ahead of the rest: [`spec`], the STFT
//! and iSTFT that htdemucs runs outside its ONNX graph, checked against
//! PyTorch. Nothing calls it yet, so [`DemucsModel::separate`] is as
//! unimplemented as before (#385).
//!
//! **Supplying a model does not enable separation.** What is missing is
//! the decoder, not the file: a valid `.onnx` loads and then `separate`
//! still returns `NotImplemented`. The crate says so rather than naming
//! a path to drop it into, because the path was a dead end (#233).
//!
//! `ml-whisper` is in the same state, and now reports it the same way.
//! It used to return `Ok(Vec::new())` — an empty transcript reported as
//! success — which was worse, and is gone.
//!
//! One piece of the decoder exists ahead of the rest: [`separate`], the
//! segment and overlap-add pipeline around the network, behind a
//! [`separate::SegmentModel`] trait and tested with a fake model. Nothing
//! implements that trait with ONNX Runtime yet, so
//! [`DemucsModel::separate`] is as unimplemented as before (#385).
//!
//! ## Cache layout
//!
//! `<project>/.audiograph/stem-cache/<hex>/{vocals,drums,bass,other}.wav`
//! where `<hex>` is `ContentHash::combine(input_hash, model_id_hash)`
//! rendered as 64-char lowercase hex. The four-files-per-directory
//! shape (rather than a single archive) is deliberate so users can
//! inspect / play the stems out of band.
//!
//! ## Models
//!
//! Only `htdemucs` is recognised ([`SUPPORTED_MODEL_IDS`]). `htdemucs_ft`
//! used to be the default, with `htdemucs` as the out-of-memory
//! fallback, but it is a bag of four fine-tuned models, one per stem:
//! four times the download and four times the inference, and there is
//! no authoritative ONNX export of it to pin (#385). With no second
//! model there is nothing smaller to fall back to, so there is no
//! fallback either.
//!
//! ## Out of memory
//!
//! [`is_oom_error`] recognises an out-of-memory failure from the ORT
//! runtime so the tool can report it as such. It does not retry:
//! `htdemucs` is the smallest model there is.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use audio_decoder::DecodedAudio;
use ml_pipeline::{onnx_session, ContentHash, SessionOptions};
use serde::{Deserialize, Serialize};

mod cache;
pub mod spec;

pub use cache::{stem_dir_for, write_stems_to_dir, StemPaths, STEM_FILENAMES, STEM_NAMES};

pub mod separate;

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
/// inspect [`is_oom_error`] on [`DemucsError::Ort`] to report an
/// out-of-memory failure as one.
#[derive(Debug, thiserror::Error)]
pub enum DemucsError {
    #[error(
        "Demucs model not found at {path}. Set DEMUCS_MODEL_PATH to an ONNX Demucs export. \
         Note that stem separation is not implemented in this build, so supplying a model will \
         not yet produce stems (#233)."
    )]
    ModelMissing { path: String },

    #[error("unsupported model id: {0:?}; expected \"htdemucs\"")]
    UnsupportedModelId(String),

    #[error("invalid audio buffer: {0}")]
    InvalidAudio(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("ONNX runtime error: {0}")]
    Ort(String),

    /// No usable ONNX Runtime library (see
    /// [`ml_pipeline::runtime::ensure`]). Reported instead of building a
    /// session, because `ort` panics when it cannot load the library.
    ///
    /// Deliberately not [`DemucsError::Ort`]: [`is_oom_error`] reads
    /// that variant's text, and a missing library is not an
    /// out-of-memory condition to retry with a smaller model.
    ///
    /// The message says what the feature does today too: a user who
    /// goes and installs the runtime on the strength of this error would
    /// otherwise reach the next one believing they were nearly done.
    #[error(
        "{0}. Stem separation is not implemented in this build either, so installing ONNX \
         Runtime will not yet produce stems (#385)"
    )]
    RuntimeUnavailable(String),

    #[error("WAV writer error: {0}")]
    Wav(String),

    /// The segment pipeline ([`separate`]) was given a [`separate::SegmentModel`]
    /// it cannot use: no segment size or sources, or a result of the wrong
    /// shape. A bug in the model wrapper, not in the audio or the runtime.
    #[error("stem separation pipeline: {0}")]
    Pipeline(String),

    /// Real inference is gated on M28 wiring the ORT decode loop. The
    /// tool layer surfaces this as a `ToolResult::Error`.
    ///
    /// The message deliberately offers no setup step. It used to say
    /// "source assets/models/{model_id}.onnx and re-run", which reads
    /// as something the user can act on — and there is nothing they can
    /// do, because no model makes this build separate stems (#233).
    #[error(
        "Demucs stem separation is not implemented in this build (model `{model_id}` loaded \
         successfully). This is a missing decoder, not a missing file: no model, environment \
         variable or setup step enables it."
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
    // #[allow(dead_code)]: intentionally unused until Phase-2 stem-separation inference is wired up
    #[allow(dead_code)]
    session: Arc<ort::session::Session>,
    /// Canonical model identifier: `"htdemucs"`, the only one in
    /// [`SUPPORTED_MODEL_IDS`]. Kept as a string, not implied, so a
    /// second model needs no change to the cache key. Folded into the
    /// cache key via [`Self::cache_key`].
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
///
/// `htdemucs_ft` is deliberately absent: it is four models, not one, and
/// has no pinned ONNX export (#385). Adding it back needs a pinned,
/// reproducible export of all four.
pub const SUPPORTED_MODEL_IDS: &[&str] = &["htdemucs"];

/// Default model when the caller doesn't specify one: the only one
/// there is. It must be a member of [`SUPPORTED_MODEL_IDS`], which a
/// test holds it to.
pub const DEFAULT_MODEL_ID: &str = "htdemucs";

impl DemucsModel {
    /// Open the ONNX model at `path`.
    ///
    /// Returns [`DemucsError::ModelMissing`] if the path does not
    /// exist (so callers can surface the install-script hint instead
    /// of an opaque ORT error), [`DemucsError::UnsupportedModelId`]
    /// for unknown ids, [`DemucsError::RuntimeUnavailable`] if there is
    /// no ONNX Runtime library to load, and propagates any other ORT
    /// failure as [`DemucsError::Ort`].
    pub fn load(model_id: &str, path: &Path) -> Result<Self> {
        if !SUPPORTED_MODEL_IDS.contains(&model_id) {
            return Err(DemucsError::UnsupportedModelId(model_id.to_string()));
        }
        if !path.exists() {
            return Err(DemucsError::ModelMissing {
                path: path.display().to_string(),
            });
        }
        // Before any `ort` call, and before reading the model: a missing
        // library is known without hashing a file that cannot be used.
        // `Session::builder()` panics, rather than erroring, without the
        // library.
        ml_pipeline::runtime::ensure()
            .map_err(|e| DemucsError::RuntimeUnavailable(e.to_string()))?;

        // Hash the model bytes so the cache key invalidates when the
        // file changes (swapped community export, retrain, …). We
        // memory-map-read here for the side effect; the bytes are
        // dropped immediately after hashing.
        let bytes = std::fs::read(path)?;
        let model_path_hash = ContentHash::from_bytes(&bytes);

        let session = onnx_session::build(path, SessionOptions::default())
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

    /// The default is the only model there is a plan to ship (#385):
    /// `htdemucs_ft` is a bag of four models, four times the download and
    /// four times the inference, and there is no pinned ONNX export of it.
    #[test]
    fn default_model_is_htdemucs_and_is_supported() {
        assert_eq!(DEFAULT_MODEL_ID, "htdemucs");
        assert!(SUPPORTED_MODEL_IDS.contains(&DEFAULT_MODEL_ID));
    }

    #[test]
    fn htdemucs_is_the_only_supported_model() {
        assert_eq!(SUPPORTED_MODEL_IDS, &["htdemucs"]);
    }

    #[test]
    fn load_rejects_htdemucs_ft_and_names_only_what_is_supported() {
        let err = DemucsModel::load("htdemucs_ft", Path::new("/nope.onnx")).unwrap_err();
        match &err {
            DemucsError::UnsupportedModelId(id) => assert_eq!(id, "htdemucs_ft"),
            other => panic!("expected UnsupportedModelId, got {other:?}"),
        }
        let text = err.to_string();
        // The id is quoted back once, as the one that was refused; the
        // expectation that follows must not offer it again.
        assert_eq!(text.matches("htdemucs_ft").count(), 1, "{text}");
        assert!(text.contains("expected \"htdemucs\""), "{text}");
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
