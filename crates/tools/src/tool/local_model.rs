//! What the tools that run a model on this computer share: `diarize` and
//! `separate_stems` (`transcribe` keeps its own, older, copy of some of it).
//!
//! Each of those tools did the same six things around its model, and the
//! copies drifted apart one fix at a time. They are here once:
//!
//! 1. **Which audio** ([`resolve_audio_source`]): `track` (the track as it
//!    sits on the timeline) xor `path` (a file that is not in the session),
//!    and [`decode_input`] to read it.
//! 2. **Which model, never fetched** ([`locate`], [`resolve`]): the ONNX
//!    Runtime library first, then the developer's folder named by an
//!    environment variable, else the model the user downloaded
//!    (`ml_pipeline::models::cached`). Nothing here downloads anything
//!    (`ml-pipeline/tests/only_the_user_downloads.rs`), and the
//!    [`ModelRef::fingerprint`] that comes back is never a path.
//! 3. **The loaded model, kept for the next call** ([`LoadedSlot`]).
//! 4. **The cache key** ([`cache_key`]): the audio, the model, the options.
//! 5. **The progress strip** ([`ProgressStrip`]): events while a run goes and
//!    one closing `done`, sent by a drop guard only if a step was shown.
//! 6. **What the agent is told** when the runtime is missing
//!    ([`runtime_unavailable_answer`]).
//!
//! The words a tool says about its own model stay in the tool: they are what
//! its tests, and the model, read.

use std::cell::Cell;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use audio_decoder::DecodedAudio;
use ml_pipeline::ContentHash;
use ml_pipeline::ModelArtifact;
use serde::de::DeserializeOwned;
use serde_json::{json, Value};

use crate::tool::util::{check_track_index, flattened_track_wav, lane_audio_path, load_head_state};
use crate::{ToolContext, ToolResult};

// ---------------------------------------------------------------------
// Arguments
// ---------------------------------------------------------------------

/// The arguments of a call as the tool's own `Args`, or the message for the
/// `ToolResult::Error` when they are not that.
pub fn parse_args<T: DeserializeOwned>(args: Value) -> Result<T, String> {
    serde_json::from_value(args).map_err(|e| format!("invalid arguments: {e}"))
}

/// A tool's answer: the value, or the message of the `ToolResult::Error`.
/// These tools are written as a `run` that returns `Result<Value, String>`,
/// so `?` ends a call at the first thing it must refuse.
pub fn into_result(answer: Result<Value, String>) -> ToolResult {
    match answer {
        Ok(value) => ToolResult::Ok(value),
        Err(message) => ToolResult::Error(message),
    }
}

/// Round to two decimals, for the seconds in an answer.
pub fn round2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

// ---------------------------------------------------------------------
// Which audio
// ---------------------------------------------------------------------

/// The audio a call is about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioSource {
    /// The file to decode: the track's audio as it sits on the timeline, or
    /// the file the caller named.
    pub path: PathBuf,
    /// What the answer and the progress strip call it: `track 2`, or the
    /// file's name.
    pub label: String,
    /// The track, when the audio is one (its times are then session seconds).
    pub track: Option<usize>,
}

/// `track` xor `path` as an [`AudioSource`], or the message for the
/// `ToolResult::Error`. The head is read only for a `track`.
pub fn resolve_audio_source(
    ctx: &ToolContext,
    track: Option<usize>,
    path: Option<&str>,
) -> Result<AudioSource, String> {
    match (track, path) {
        (Some(track), None) => {
            let state = load_head_state(ctx)?;
            check_track_index(&state.tracks, track)?;
            let clips = &state.tracks[track].clips;
            let project = ctx.store.project_dir();
            let path = match lane_audio_path(project, clips) {
                Some(p) => p,
                None => flattened_track_wav(project, clips)
                    .map_err(|e| format!("track {track}: {e}"))?,
            };
            Ok(AudioSource {
                path,
                label: format!("track {track}"),
                track: Some(track),
            })
        }
        (None, Some(path)) => {
            let path = PathBuf::from(path);
            if !path.exists() {
                return Err(format!("file not found: {}", path.display()));
            }
            let label = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("audio")
                .to_string();
            Ok(AudioSource {
                path,
                label,
                track: None,
            })
        }
        (Some(_), Some(_)) => Err("give `track` or `path`, not both".to_string()),
        (None, None) => {
            Err("give `track` (a track's index in the session) or `path` (a file)".to_string())
        }
    }
}

/// Decode `path`, and say how long it is in seconds.
pub fn decode_input(path: &Path) -> Result<(DecodedAudio, f64), String> {
    let decoded = audio_decoder::decode_file(path).map_err(|e| format!("decode failed: {e}"))?;
    let frames = decoded.samples.len() / usize::from(decoded.channels.max(1));
    let duration_sec = frames as f64 / f64::from(decoded.sample_rate.max(1));
    Ok((decoded, duration_sec))
}

/// The cache key of a result: the audio (samples, rate and channels), the
/// model's fingerprint and `options`, which the tool writes out whole.
pub fn cache_key(decoded: &DecodedAudio, fingerprint: &str, options: &str) -> ContentHash {
    ContentHash::combine(&[
        ContentHash::from_bytes(
            crate::provenance::audio_hash(&decoded.samples, decoded.sample_rate, decoded.channels)
                .as_bytes(),
        ),
        ContentHash::from_bytes(fingerprint.as_bytes()),
        ContentHash::from_bytes(options.as_bytes()),
    ])
}

// ---------------------------------------------------------------------
// Which model
// ---------------------------------------------------------------------

/// A model found and identified, not yet loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelRef {
    /// The folder holding the model's files.
    pub folder: PathBuf,
    /// What the cache key folds in: changes when the model does. Never a path.
    pub fingerprint: String,
    pub downloaded: bool,
}

/// Why no model could be found. Each tool turns it into its own failure,
/// with its own words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelLookup {
    /// No usable ONNX Runtime. The reason is short and carries no path.
    RuntimeUnavailable(String),
    /// No environment variable, and the user has not downloaded the model.
    NotDownloaded,
    /// The environment variable names something that is not the model folder.
    PathInvalid,
}

/// Find the model without fetching anything: `env` (the value of the
/// developer's variable) first, else `downloaded()`. A variable that is set
/// wins even when it is wrong: the developer asked for that folder.
pub fn resolve(
    pinned: &ModelArtifact,
    env: Option<OsString>,
    downloaded: impl FnOnce() -> Option<PathBuf>,
) -> Result<ModelRef, ModelLookup> {
    if let Some(value) = env.filter(|v| !v.is_empty()) {
        let folder = PathBuf::from(value);
        if !folder.is_dir() {
            return Err(ModelLookup::PathInvalid);
        }
        let mut identity = String::from("override");
        for file in &pinned.files {
            let stat = std::fs::metadata(folder.join(&file.path)).ok();
            let nanos = stat
                .as_ref()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_nanos());
            identity.push_str(&format!(
                "|{}:{}:{}",
                file.path,
                stat.as_ref().map_or(0, |m| m.len()),
                nanos
            ));
        }
        return Ok(ModelRef {
            folder,
            fingerprint: ContentHash::from_bytes(identity.as_bytes()).to_hex(),
            downloaded: false,
        });
    }
    let folder = downloaded().ok_or(ModelLookup::NotDownloaded)?;
    // The pinned hashes are what `cached` just verified the files against.
    let mut identity = String::from("pinned");
    for file in &pinned.files {
        identity.push_str(&format!("|{}:{}", file.path, file.sha256));
    }
    Ok(ModelRef {
        folder,
        fingerprint: ContentHash::from_bytes(identity.as_bytes()).to_hex(),
        downloaded: true,
    })
}

/// [`resolve`], after the runtime check: the ONNX Runtime first, because a
/// build that has none cannot run any model, and sending its user to download
/// one first is the worse trap. `var` is the developer's variable and `id`
/// the model's id in the catalogue.
pub fn locate(pinned: &ModelArtifact, var: &str, id: &str) -> Result<ModelRef, ModelLookup> {
    ml_pipeline::runtime::ensure()
        .map_err(|e| ModelLookup::RuntimeUnavailable(runtime_reason(&e)))?;
    resolve(pinned, std::env::var_os(var), || {
        ml_pipeline::models::cached(id)
    })
}

/// A short reason for a runtime that cannot be used, with no path in it.
fn runtime_reason(error: &ml_pipeline::Error) -> String {
    use ml_pipeline::Error as E;
    match error {
        E::MissingRuntime { .. } => "no ONNX Runtime library was found".to_string(),
        E::RuntimeUnsupported { needs, running } => format!(
            "ONNX Runtime needs macOS {needs} or newer, and this Mac is running macOS {running}"
        ),
        other => {
            tracing::warn!(error = %other, "the ONNX Runtime library could not be loaded");
            "the ONNX Runtime library could not be loaded".to_string()
        }
    }
}

/// The reason given when a model's own loader reports that the runtime could
/// not be used: the loader's words go to the log, not to the agent.
pub fn runtime_load_reason(loader_reason: &dyn std::fmt::Display) -> String {
    tracing::warn!(reason = %loader_reason, "the ONNX Runtime library could not be used");
    "the ONNX Runtime library could not be loaded".to_string()
}

/// What the agent is told when the runtime is missing. `what` starts the
/// sentence ("Stem separation", "Speaker diarization").
pub fn runtime_unavailable_answer(what: &str, reason: &str) -> String {
    format!(
        "{what} needs the ONNX Runtime library, and {reason}. The release \
         installers include it; a build from source does not. Tell the user: for a build \
         from source, set ORT_DYLIB_PATH to a copy of ONNX Runtime 1.23.2 \
         (docs/development-guide.md, \"ML Model Files\"). Do not try to install anything \
         yourself."
    )
}

/// The model's download size in megabytes, rounded, for what the agent is
/// told when it is missing.
pub fn model_megabytes(id: &str) -> u64 {
    ml_pipeline::Catalog::built_in()
        .get(id)
        .map_or(0, |m| (m.size() + 500_000) / 1_000_000)
}

/// The loaded model, kept for the next call. One slot per tool, and a
/// different folder or fingerprint replaces what is held.
pub struct LoadedSlot<T> {
    held: Mutex<Option<(String, Arc<T>)>>,
}

impl<T> LoadedSlot<T> {
    pub const fn new() -> Self {
        Self {
            held: Mutex::new(None),
        }
    }

    /// The model held for `model`, else the one `load` makes from its folder,
    /// which is then held. A failed load holds nothing.
    pub fn get_or_load<E>(
        &self,
        model: &ModelRef,
        load: impl FnOnce(&Path) -> Result<T, E>,
    ) -> Result<Arc<T>, E> {
        let key = format!("{}|{}", model.folder.display(), model.fingerprint);
        let mut slot = self.held.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((held, loaded)) = slot.as_ref() {
            if *held == key {
                return Ok(Arc::clone(loaded));
            }
        }
        let loaded = Arc::new(load(&model.folder)?);
        *slot = Some((key, Arc::clone(&loaded)));
        Ok(loaded)
    }
}

impl<T> Default for LoadedSlot<T> {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------
// Progress
// ---------------------------------------------------------------------

/// What the progress strip is told while a run goes, and the closing `done`
/// that clears it. Nothing is said, and nothing cleared, for a cache hit.
///
/// The tool calls `crate::progress::begin()` (it clears a Cancel pressed
/// before the run) and reads `crate::progress::cancelled()` itself.
pub struct ProgressStrip {
    kind: &'static str,
    file: String,
    started: Cell<bool>,
    total: Cell<usize>,
    cancelled: Cell<bool>,
}

impl ProgressStrip {
    pub fn new(kind: &'static str, file: String) -> Self {
        Self {
            kind,
            file,
            started: Cell::new(false),
            total: Cell::new(0),
            cancelled: Cell::new(false),
        }
    }

    /// `done` of `total` steps are done. `detail`, when given, follows the
    /// file's name in brackets.
    pub fn step(&self, done: usize, total: usize, detail: Option<&str>) {
        self.started.set(true);
        self.total.set(total);
        let file = match detail {
            Some(doing) => format!("{} ({doing})", self.file),
            None => self.file.clone(),
        };
        crate::progress::report(json!({
            "kind": self.kind,
            "index": done.min(total.saturating_sub(1)),
            "total": total,
            "file": file,
        }));
    }

    /// Say whether the run ended because the user pressed Cancel.
    pub fn set_cancelled(&self, cancelled: bool) {
        self.cancelled.set(cancelled);
    }
}

impl Drop for ProgressStrip {
    /// However the run ends, a strip that was shown is cleared.
    fn drop(&mut self) {
        if self.started.get() {
            crate::progress::report(json!({
                "kind": self.kind,
                "done": true,
                "cancelled": self.cancelled.get(),
                "total": self.total.get(),
                "file": self.file,
            }));
        }
    }
}
