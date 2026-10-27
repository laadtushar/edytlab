//! Demucs stem separation (#385).
//!
//! Splits a recording into four stems (`vocals`, `drums`, `bass`, `other`)
//! with HT-Demucs v4 (`htdemucs`) on ONNX Runtime, on this computer. The
//! `separate_stems` tool in the `tools` crate keeps one [`DemucsModel`] alive
//! for the process and calls [`DemucsModel::separate_to_dir`].
//!
//! ## Layers
//!
//! | module | what it is | tested against |
//! |---|---|---|
//! | [`htdemucs`] | the network: STFT, the per-segment normalisation, the pinned ONNX graph, the iSTFT ([`htdemucs::Network`] over a [`htdemucs::Backend`]) | a Rust function standing in for the graph; the committed `htdemucs_split.onnx` on the real runtime; the real model |
//! | [`separate`] | the segment pipeline around a [`separate::SegmentModel`]: cut, weight, add, normalise, stream out | a fake model |
//! | [`resample`] | rate conversion in pieces, keeping time and length | itself |
//! | [`pipeline`] | a decoded file in, four 16-bit WAVs out, in the file's own rate and channels | a fake model |
//! | [`cache`] | the stem directory: whole or not at all, bounded | itself |
//! | [`spec`] | the STFT and iSTFT, as PyTorch has them | PyTorch fixtures |
//!
//! Each layer is used through the one above it, and the seam between the
//! network and the rest is [`separate::SegmentModel`]: CI runs everything but
//! ONNX Runtime and the model. `tests/real_model.rs` (`#[ignore]`, run by
//! `ml.yml`) runs the real thing on a fixture mixed from known stems and
//! checks the separation against them.
//!
//! ## What does not match upstream, on purpose
//!
//! `shifts=0` (the output is a pure function of the input, so the cache
//! cannot lie; see [`separate`]), and 16-bit output (the format of everything
//! else the project writes; clipping is counted, see [`pipeline`]).
//!
//! ## Cache layout
//!
//! `<project>/.audiograph/stem-cache/<hex>/{vocals,drums,bass,other}.wav`
//! where `<hex>` is the tool's key (the audio, the model and a format
//! version). The four-files-per-directory shape (rather than a single
//! archive) is deliberate so users can inspect / play the stems out of band.
//! See [`cache`].
//!
//! ## Models
//!
//! Only `htdemucs` is recognised ([`SUPPORTED_MODEL_IDS`]). `htdemucs_ft`
//! is a bag of four fine-tuned models, one per stem: four times the download
//! and four times the inference, and there is no authoritative ONNX export of
//! it to pin (#385). With no second model there is nothing smaller to fall
//! back to on out-of-memory, so there is no fallback ([`is_oom_error`] lets
//! the tool report it as one).

use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use audio_decoder::DecodedAudio;
use ml_pipeline::onnx_session::SessionOptions;
use serde::{Deserialize, Serialize};

pub mod cache;
pub mod htdemucs;
pub mod pipeline;
pub mod resample;
pub mod separate;
pub mod spec;

pub use cache::{
    prune, stem_dir_for, touch, Pending, Pruned, StemPaths, STEM_CACHE_CAP_BYTES, STEM_FILENAMES,
    STEM_NAMES,
};
pub use pipeline::{separate_to_dir, Separated};
pub use separate::Control;

/// Errors produced by the Demucs wrapper.
///
/// Tool callers should turn [`DemucsError::ModelMissing`] into a
/// [`tools::ToolResult::Error`] that tells the user to download the model, and
/// inspect [`is_oom_error`] on [`DemucsError::Ort`] to report an
/// out-of-memory failure as one.
#[derive(Debug, thiserror::Error)]
pub enum DemucsError {
    /// The folder, or a file in it, is not there.
    #[error("Demucs model not found at {path}")]
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
    /// out-of-memory condition.
    #[error("{0}")]
    RuntimeUnavailable(String),

    /// The sample-rate converter refused the rates or failed.
    #[error("sample-rate conversion: {0}")]
    Resample(String),

    /// A model folder whose graph is not the one this crate reads. The
    /// message says which input or output differs.
    #[error("not the expected htdemucs model: {0}")]
    UnexpectedExport(String),

    #[error("WAV writer error: {0}")]
    Wav(String),

    /// The segment pipeline ([`separate`]) was given a [`separate::SegmentModel`]
    /// it cannot use: no segment size or sources, or a result of the wrong
    /// shape. A bug in the model wrapper, not in the audio or the runtime.
    #[error("stem separation pipeline: {0}")]
    Pipeline(String),

    /// The user stopped the run.
    #[error("stem separation was cancelled")]
    Cancelled,
}

/// Crate-local result alias.
pub type Result<T> = std::result::Result<T, DemucsError>;

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

/// The htdemucs model, open and checked: one ONNX Runtime session, reused for
/// every segment of every file, behind a lock (a run needs `&mut`, so two
/// separations take turns).
pub struct DemucsModel {
    model_id: String,
    network: Mutex<htdemucs::Htdemucs>,
}

impl std::fmt::Debug for DemucsModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DemucsModel")
            .field("model_id", &self.model_id)
            .finish_non_exhaustive()
    }
}

impl DemucsModel {
    /// Open the model in `dir`, the folder `models.json`'s `htdemucs` was
    /// downloaded to (or a developer's copy of it): `htdemucs_fwd.onnx` and
    /// `htdemucs_fwd.onnx.data` side by side. ONNX Runtime picks the number of
    /// threads and runs on the CPU, with the memory options of
    /// [`DemucsModel::session_options`]; use [`DemucsModel::load_with`] to
    /// choose.
    ///
    /// Returns [`DemucsError::UnsupportedModelId`] for an id that is not in
    /// [`SUPPORTED_MODEL_IDS`], [`DemucsError::RuntimeUnavailable`] if there
    /// is no ONNX Runtime library to load (before the folder is looked at),
    /// [`DemucsError::ModelMissing`] if the folder or a file is not there,
    /// [`DemucsError::UnexpectedExport`] if the graph is another export, and
    /// any other ORT failure as [`DemucsError::Ort`]. Nothing is downloaded.
    pub fn load(model_id: &str, dir: &Path) -> Result<Self> {
        Self::load_with(model_id, dir, Self::session_options())
    }

    /// The options [`DemucsModel::load`] builds the session with: ONNX
    /// Runtime's defaults except for its memory. Measured on the pinned
    /// model on the 4 vCPU runner (`ml.yml`, 102 s of audio, release build):
    ///
    /// | arena | memory pattern | peak | after the run | realtime |
    /// |---|---|---|---|---|
    /// | on | on (ORT's default) | 4,968 MB | 4,926 MB | 2.26x |
    /// | on | off | 3,266 MB | 3,225 MB | 2.30x |
    /// | off | on | 3,711 MB | 444 MB | 1.89x |
    /// | **off** | **off** (this) | 3,282 MB | 913 MB | 1.96x |
    ///
    /// The memory pattern (a plan for reusing one run's layout in the next)
    /// costs 1.7 GB and buys nothing here. The arena keeps the peak of a run
    /// for the life of the session, which would leave a desktop app holding
    /// 3 to 5 GB after a separation; turning it off returns it, for about 15 %
    /// of the speed.
    pub fn session_options() -> SessionOptions {
        SessionOptions {
            cpu_memory_arena: Some(false),
            memory_pattern: Some(false),
            ..SessionOptions::default()
        }
    }

    /// [`DemucsModel::load`] with the session's threads, optimisation level
    /// and execution provider chosen.
    pub fn load_with(model_id: &str, dir: &Path, options: SessionOptions) -> Result<Self> {
        if !SUPPORTED_MODEL_IDS.contains(&model_id) {
            return Err(DemucsError::UnsupportedModelId(model_id.to_string()));
        }
        Ok(Self {
            model_id: model_id.to_string(),
            network: Mutex::new(htdemucs::Htdemucs::load(dir, options)?),
        })
    }

    /// The canonical model id the session was loaded with.
    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    /// Separate `audio` and write the four stems as the entry `dest`; see
    /// [`separate_to_dir`], which this runs on the loaded network.
    pub fn separate_to_dir(
        &self,
        audio: DecodedAudio,
        dest: &Path,
        control: &Control<'_>,
    ) -> Result<Separated> {
        // Nothing a run leaves behind is read by the next one: the lock is
        // taken back from a holder that panicked.
        let mut network = self.network.lock().unwrap_or_else(PoisonError::into_inner);
        separate_to_dir(&mut *network, audio, dest, control)
    }
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

    #[test]
    fn load_rejects_unsupported_model_id() {
        let err = DemucsModel::load("htdemucs_xl", Path::new("/nope")).unwrap_err();
        match err {
            DemucsError::UnsupportedModelId(id) => assert_eq!(id, "htdemucs_xl"),
            other => panic!("expected UnsupportedModelId, got {other:?}"),
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
        let err = DemucsModel::load("htdemucs_ft", Path::new("/nope")).unwrap_err();
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
    fn no_error_of_this_crate_says_the_feature_is_missing() {
        // The stub's errors said "not implemented in this build" (#233); a
        // message that does is a lie now, and the tool passes some on.
        for e in [
            DemucsError::ModelMissing { path: "x".into() },
            DemucsError::RuntimeUnavailable("no library".into()),
            DemucsError::Cancelled,
            DemucsError::Pipeline("x".into()),
        ] {
            let text = e.to_string().to_lowercase();
            assert!(!text.contains("not implemented"), "{text}");
        }
    }

    /// The model's gigabytes are not kept for the life of the process
    /// (measured in `session_options`' table), and the provider and threads
    /// are left to the caller.
    #[test]
    fn the_default_session_hands_its_memory_back_and_leaves_the_rest_alone() {
        let options = DemucsModel::session_options();
        assert_eq!(options.cpu_memory_arena, Some(false));
        assert_eq!(options.memory_pattern, Some(false));
        let plain = SessionOptions::default();
        assert_eq!(options.intra_op_threads, plain.intra_op_threads);
        assert_eq!(options.execution_provider, plain.execution_provider);
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
