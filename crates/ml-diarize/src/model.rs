//! The two networks on ONNX Runtime.
//!
//! [`DiarizeModel::load`] opens the folder the speaker model was downloaded
//! to (`segmentation/model.onnx` and `embedding/model.onnx`, the paths
//! `models.json` pins) and checks that each file is the network this crate
//! reads: its input and output names, element types and the dimensions that
//! are fixed. A different export is an error that says what differs, not a
//! model that runs and returns something else.
//!
//! The sessions are built with `ml_pipeline::onnx_session::build`, which calls
//! `runtime::ensure` first: this crate never lets `ort` load the library on its
//! own (it panics when it cannot), and calls `Session::builder` nowhere (a
//! source test in ml-pipeline fails if any crate does).
//!
//! ## Why there are several embedding sessions
//!
//! A recording makes thousands of voiceprints (about three per second of
//! conversation), each a separate run of a small network on a few hundred
//! frames. ONNX Runtime cannot spread one such run over several cores, so a
//! run takes about as long on four cores as on one, and a single session
//! would leave three cores idle. The model therefore holds a few embedding
//! sessions of one thread each ([`embedding_sessions`]) and hands the
//! pipeline that many at once ([`Embedder::parallelism`]). The segmentation
//! network is one session, which ONNX Runtime does spread over the cores.
//! A model is loaded once and kept by the caller (the `diarize` tool keeps
//! one), so these are built once per process, not per call.
//!
//! What the networks are:
//!
//! | | segmentation | embedding |
//! |---|---|---|
//! | file | `onnx-community/pyannote-segmentation-3.0` `onnx/model.onnx` | `Wespeaker/wespeaker-voxceleb-resnet34-LM` `voxceleb_resnet34_LM.onnx` |
//! | input | `input_values` `[batch, 1, samples]`, a 10 s window at 16 kHz | `feats` `[batch, frames, 80]`, the Kaldi filterbank |
//! | output | `logits` `[batch, frames, 7]`, 589 frames for 10 s | `embs` `[batch, 256]` |

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

use ml_pipeline::onnx_session::{self, SessionOptions};
use ort::session::Session;
use ort::value::{Outlet, Tensor, TensorElementType};

use crate::fbank::BANDS;
use crate::frames::CHUNK_SAMPLES;
use crate::pipeline::{
    Diarization, DiarizeError, Embedder, Options, Segmenter, EMBEDDING_DIM, POWERSET_CLASSES,
};

/// The segmentation network's file in the model folder.
pub const SEGMENTATION_FILE: &str = "segmentation/model.onnx";
/// The embedding network's file in the model folder.
pub const EMBEDDING_FILE: &str = "embedding/model.onnx";

/// Why the model could not be opened.
#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    /// The folder, or a file in it, is not there.
    #[error("speaker model not found at {path}")]
    ModelMissing { path: String },
    /// No usable ONNX Runtime library (see `ml_pipeline::runtime::ensure`).
    #[error("{0}")]
    RuntimeUnavailable(String),
    /// A file is not the network this build reads. The message says which
    /// input or output differs.
    #[error("not the expected speaker model: {0}")]
    UnexpectedExport(String),
    /// ONNX Runtime could not open a file.
    #[error("could not load the speaker model: {0}")]
    ModelLoad(String),
}

/// Lock a session, taking it back from a holder that panicked: nothing a run
/// leaves behind is read by the next one (see ml-whisper's `lock_session`).
fn lock(session: &Mutex<Session>) -> MutexGuard<'_, Session> {
    session.lock().unwrap_or_else(PoisonError::into_inner)
}

/// How many embedding sessions to hold: one per core but one, at most
/// [`MAX_EMBEDDING_SESSIONS`], so the window stays responsive.
pub fn embedding_sessions() -> usize {
    std::thread::available_parallelism()
        .map_or(1, |n| n.get().saturating_sub(1))
        .clamp(1, MAX_EMBEDDING_SESSIONS)
}

/// The most embedding sessions held (26 MB each).
pub const MAX_EMBEDDING_SESSIONS: usize = 4;

fn open(path: &Path, threads: Option<usize>) -> Result<Session, LoadError> {
    use ml_pipeline::Error as E;
    onnx_session::build(
        path,
        SessionOptions {
            intra_op_threads: threads,
            ..SessionOptions::default()
        },
    )
    .map_err(|e| match e {
        E::MissingRuntime { .. } | E::RuntimeLoad { .. } | E::RuntimeUnsupported { .. } => {
            LoadError::RuntimeUnavailable(e.to_string())
        }
        E::Io(io) if io.kind() == std::io::ErrorKind::NotFound => LoadError::ModelMissing {
            path: path.display().to_string(),
        },
        other => LoadError::ModelLoad(format!("{}: {other}", path.display())),
    })
}

/// One input or output of a session: name, element type (`None` if not a
/// tensor of a type this crate reads) and dimensions (`-1`: not fixed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Port {
    pub name: String,
    pub float32: bool,
    pub dims: Vec<i64>,
}

fn ports(outlets: &[Outlet]) -> Vec<Port> {
    outlets
        .iter()
        .map(|o| Port {
            name: o.name().to_string(),
            float32: matches!(o.dtype().tensor_type(), Some(TensorElementType::Float32)),
            dims: o
                .dtype()
                .tensor_shape()
                .map(|s| s.to_vec())
                .unwrap_or_default(),
        })
        .collect()
}

/// Check one port against what is expected: a float32 tensor of this name and
/// rank whose fixed dimensions are the ones given (`None`: any).
fn check_port(
    file: &str,
    ports: &[Port],
    name: &str,
    want: &[Option<i64>],
) -> Result<(), LoadError> {
    let bad = |what: String| LoadError::UnexpectedExport(format!("{file}: {what}"));
    let got = ports.iter().find(|p| p.name == name).ok_or_else(|| {
        bad(format!(
            "no `{name}`; it has {:?}",
            ports.iter().map(|p| p.name.as_str()).collect::<Vec<_>>()
        ))
    })?;
    if !got.float32 {
        return Err(bad(format!("`{name}` is not a float32 tensor")));
    }
    if got.dims.len() != want.len() {
        return Err(bad(format!(
            "`{name}` has {} dimensions, expected {}",
            got.dims.len(),
            want.len()
        )));
    }
    for (i, (g, w)) in got.dims.iter().zip(want).enumerate() {
        if let Some(w) = w {
            if g != w {
                return Err(bad(format!("`{name}` dimension {i} is {g}, expected {w}")));
            }
        }
    }
    Ok(())
}

/// Check a segmentation network's ports: `input_values [batch, 1 or any, samples]`
/// in, `logits [batch, frames, 7]` out.
pub fn check_segmentation(inputs: &[Port], outputs: &[Port]) -> Result<(), LoadError> {
    check_port(
        SEGMENTATION_FILE,
        inputs,
        "input_values",
        &[None, None, None],
    )?;
    check_port(
        SEGMENTATION_FILE,
        outputs,
        "logits",
        &[None, None, Some(POWERSET_CLASSES as i64)],
    )
}

/// Check an embedding network's ports: `feats [batch, frames, 80]` in,
/// `embs [batch, 256]` out.
pub fn check_embedding(inputs: &[Port], outputs: &[Port]) -> Result<(), LoadError> {
    check_port(
        EMBEDDING_FILE,
        inputs,
        "feats",
        &[None, None, Some(BANDS as i64)],
    )?;
    check_port(
        EMBEDDING_FILE,
        outputs,
        "embs",
        &[None, Some(EMBEDDING_DIM as i64)],
    )
}

/// The speaker model, open and checked: both networks, ready to run.
pub struct DiarizeModel {
    segmentation: Mutex<Session>,
    /// One thread each; a run takes whichever is free.
    embedding: Vec<Mutex<Session>>,
    /// Where the next run looks first, so runs spread over the sessions.
    next: AtomicUsize,
}

impl DiarizeModel {
    /// Open the model in `dir`, the folder `models.json`'s `speaker-diarization`
    /// was downloaded to (or a developer's copy of it).
    ///
    /// Nothing is downloaded: the folder is the user's, from Settings, or a
    /// developer's (`DIARIZE_MODEL_PATH` in the tools crate).
    pub fn load(dir: &Path) -> Result<Self, LoadError> {
        // First, before the folder is looked at: a build with no runtime is
        // never sent to download a model, so the answer on a machine without
        // the runtime is the same whatever the path holds.
        ml_pipeline::runtime::ensure().map_err(|e| LoadError::RuntimeUnavailable(e.to_string()))?;
        if !dir.exists() {
            return Err(LoadError::ModelMissing {
                path: dir.display().to_string(),
            });
        }
        let seg_path: PathBuf = dir.join(SEGMENTATION_FILE);
        let emb_path: PathBuf = dir.join(EMBEDDING_FILE);
        for p in [&seg_path, &emb_path] {
            if !p.is_file() {
                return Err(LoadError::ModelMissing {
                    path: p.display().to_string(),
                });
            }
        }
        let segmentation = open(&seg_path, None)?;
        check_segmentation(
            &ports(segmentation.inputs()),
            &ports(segmentation.outputs()),
        )?;
        let mut embedding = Vec::new();
        for _ in 0..embedding_sessions() {
            let session = open(&emb_path, Some(1))?;
            check_embedding(&ports(session.inputs()), &ports(session.outputs()))?;
            embedding.push(Mutex::new(session));
        }
        Ok(Self {
            segmentation: Mutex::new(segmentation),
            embedding,
            next: AtomicUsize::new(0),
        })
    }

    /// Who spoke when in `audio` (16 kHz mono, -1 to 1).
    pub fn diarize(
        &self,
        audio: &[f32],
        options: &Options<'_>,
    ) -> Result<Diarization, DiarizeError> {
        crate::pipeline::diarize(self, self, audio, options)
    }
}

fn inference(what: &str, e: impl std::fmt::Display) -> DiarizeError {
    DiarizeError::Inference(format!("{what}: {e}"))
}

impl Segmenter for DiarizeModel {
    fn segment(&self, window: &[f32]) -> Result<Vec<f32>, DiarizeError> {
        debug_assert_eq!(window.len(), CHUNK_SAMPLES);
        let input = Tensor::from_array(([1usize, 1, window.len()], window.to_vec()))
            .map_err(|e| inference("segmentation input", e))?;
        let mut session = lock(&self.segmentation);
        let outputs = session
            .run(ort::inputs!["input_values" => input])
            .map_err(|e| inference("segmentation run", e))?;
        let (shape, data) = outputs["logits"]
            .try_extract_tensor::<f32>()
            .map_err(|e| inference("segmentation output", e))?;
        if shape.len() != 3 || shape[0] != 1 || shape[2] != POWERSET_CLASSES as i64 {
            return Err(DiarizeError::Inference(format!(
                "the segmentation network returned {:?}, expected [1, frames, {POWERSET_CLASSES}]",
                &shape[..]
            )));
        }
        Ok(data.to_vec())
    }
}

impl Embedder for DiarizeModel {
    fn parallelism(&self) -> usize {
        self.embedding.len()
    }

    fn embed(&self, features: &[f32], frames: usize) -> Result<Vec<f32>, DiarizeError> {
        debug_assert_eq!(features.len(), frames * BANDS);
        let input = Tensor::from_array(([1usize, frames, BANDS], features.to_vec()))
            .map_err(|e| inference("embedding input", e))?;
        // A free session if there is one, else wait for the one this run is
        // assigned.
        let start = self.next.fetch_add(1, Ordering::Relaxed) % self.embedding.len();
        let mut session = (0..self.embedding.len())
            .find_map(|i| {
                self.embedding[(start + i) % self.embedding.len()]
                    .try_lock()
                    .ok()
            })
            .unwrap_or_else(|| lock(&self.embedding[start]));
        let outputs = session
            .run(ort::inputs!["feats" => input])
            .map_err(|e| inference("embedding run", e))?;
        let (shape, data) = outputs["embs"]
            .try_extract_tensor::<f32>()
            .map_err(|e| inference("embedding output", e))?;
        if shape.len() != 2 || shape[0] != 1 || shape[1] != EMBEDDING_DIM as i64 {
            return Err(DiarizeError::Inference(format!(
                "the embedding network returned {:?}, expected [1, {EMBEDDING_DIM}]",
                &shape[..]
            )));
        }
        Ok(data.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn port(name: &str, dims: &[i64]) -> Port {
        Port {
            name: name.to_string(),
            float32: true,
            dims: dims.to_vec(),
        }
    }

    #[test]
    fn the_pinned_networks_ports_pass() {
        check_segmentation(
            &[port("input_values", &[-1, -1, -1])],
            &[port("logits", &[-1, -1, 7])],
        )
        .unwrap();
        check_embedding(&[port("feats", &[-1, -1, 80])], &[port("embs", &[-1, 256])]).unwrap();
    }

    #[test]
    fn a_different_export_is_named_not_guessed() {
        // sherpa-onnx's export of the same segmentation network.
        let e =
            check_segmentation(&[port("x", &[-1, 1, -1])], &[port("y", &[-1, -1, 7])]).unwrap_err();
        assert!(
            e.to_string().contains("no `input_values`") && e.to_string().contains("[\"x\"]"),
            "{e}"
        );
        // The community ONNX export of the same embedding network names its ports otherwise.
        let e = check_embedding(
            &[port("input_features", &[-1, -1, 80])],
            &[port("last_hidden_state", &[-1, 256])],
        )
        .unwrap_err();
        assert!(e.to_string().contains("no `feats`"), "{e}");
        let e = check_embedding(&[port("feats", &[-1, -1, 40])], &[port("embs", &[-1, 256])])
            .unwrap_err();
        assert!(
            e.to_string().contains("dimension 2 is 40, expected 80"),
            "{e}"
        );
        let e = check_embedding(&[port("feats", &[-1, -1, 80])], &[port("embs", &[-1, 192])])
            .unwrap_err();
        assert!(
            e.to_string().contains("dimension 1 is 192, expected 256"),
            "{e}"
        );
        let mut not_float = port("feats", &[-1, -1, 80]);
        not_float.float32 = false;
        assert!(check_embedding(&[not_float], &[port("embs", &[-1, 256])])
            .unwrap_err()
            .to_string()
            .contains("float32"));
        let e = check_segmentation(
            &[port("input_values", &[-1, -1, -1])],
            &[port("logits", &[-1, 7])],
        )
        .unwrap_err();
        assert!(e.to_string().contains("2 dimensions, expected 3"), "{e}");
    }

    #[test]
    fn a_folder_that_is_not_there_is_model_missing_and_the_runtime_is_asked_first() {
        let err = DiarizeModel::load(Path::new("/definitely/not/here"))
            .err()
            .unwrap();
        if ml_pipeline::runtime::ensure().is_err() {
            // No runtime (CI): that is the answer, before the folder is looked at.
            assert!(matches!(err, LoadError::RuntimeUnavailable(_)), "{err:?}");
        } else {
            assert!(matches!(err, LoadError::ModelMissing { .. }), "{err:?}");
        }
    }

    #[test]
    fn a_folder_without_the_files_is_model_missing_when_there_is_a_runtime_and_runtime_unavailable_when_not(
    ) {
        let dir = tempfile::tempdir().unwrap();
        match DiarizeModel::load(dir.path()).err().unwrap() {
            // CI has no runtime: the answer is the same whatever the folder holds.
            LoadError::RuntimeUnavailable(_) => {}
            LoadError::ModelMissing { path } => assert!(path.ends_with("model.onnx"), "{path}"),
            other => panic!("{other:?}"),
        }
    }
}
