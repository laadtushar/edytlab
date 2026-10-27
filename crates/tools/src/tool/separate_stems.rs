//! `separate_stems` tool: split a recording into vocals, drums, bass and other,
//! on this computer (#385).
//!
//! The stems are made by `ml_demucs` (HT-Demucs v4 on the pinned `htdemucs`
//! model), and the tool is the glue around it, shaped like `diarize`
//! (`diarize.rs`) and `transcribe` (`transcribe.rs`):
//!
//! 1. **Which audio.** `track` (the track as it sits on the timeline) xor
//!    `path` (a file that is not in the session).
//! 2. **Which model, never fetched.** [`StemModel::locate`]: the ONNX Runtime
//!    library first, then `DEMUCS_MODEL_PATH` (a developer's folder), else the
//!    model the user downloaded (`ml_pipeline::models::cached`). The tool never
//!    downloads anything (`ml-pipeline/tests/only_the_user_downloads.rs`).
//! 3. **The cache.** `<project>/.audiograph/stem-cache/<hex>/{vocals,drums,
//!    bass,other}.wav`, where `<hex>` combines the audio's content hash
//!    ([`crate::provenance::audio_hash`]: samples, rate and channels), the
//!    model's identity and [`STEMS_FORMAT`]. An entry exists whole or not at all
//!    (`ml_demucs::Pending`), so a Cancel or a crash never leaves a partial
//!    hit, and the cache is kept under a cap (`ml_demucs::prune`), the stems
//!    used longest ago going first.
//! 4. **Progress and cancel.** `kind: "separate_stems"` events, one per
//!    segment of the network (7.8 s of audio), and the cancel switch the
//!    window's Cancel button sets, read before every segment.
//! 5. **The answer.** Four paths. The session is not written and no node is
//!    appended: a stem becomes a track when the agent calls `load` with its
//!    path, and that call is the node. (`load` copies a file from a cache into
//!    the project's own `derived/` store, so a node never names the cache and
//!    the cap, a sweep or the user can delete it: `tests/separate_stems_tool.rs`
//!    removes the whole cache after loading a stem and renders.)
//!
//! ## Held for approval, off the store lock
//!
//! `mutates` is the default `true`: a run writes hundreds of megabytes (about
//! 40 MB a minute of stereo, four stems), so Plan first holds it. It runs off
//! the store lock (`runs_off_the_lock`, #421) exactly as `diarize` and
//! `transcribe` do: a run takes about half the audio's length, and the tool
//! reads the head for one track or one file and appends nothing, so
//! `Store::commit` publishes its one run whatever the head did and it is never
//! run again. `tests/off_lock_tools.rs` has the `progress` spellings and why.
//!
//! ## What a failure says
//!
//! The answers carry no local path and no loader reason (the reason is logged):
//! tool results go to the model provider. They carry no setup marker either,
//! because the setup card (`ai::ToolView::SpeechSetup`) speaks of the speech
//! model; the answers name the Stem model row in Settings → Account instead.
//! The paths in a *successful* answer are the point of it: the agent hands them
//! to `load`.
//!
//! ## Testing without a model
//!
//! [`SeparateStemsTool::with`] takes any [`StemModel`], so CI runs the whole
//! tool against a fake that runs the real pipeline around a network that is
//! not one. [`DemucsStems`] is the real one.

use std::cell::Cell;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use audio_decoder::DecodedAudio;
use ml_demucs::{Control, DemucsError, DemucsModel, Separated, STEM_NAMES};
use ml_pipeline::ContentHash;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::tool::util::{check_track_index, flattened_track_wav, lane_audio_path, load_head_state};
use crate::{Tool, ToolContext, ToolResult};

/// Bumped when a change to the pipeline would give different stems for the same
/// audio and model, so an entry written before it is not read after it.
pub const STEMS_FORMAT: u32 = 1;

/// The variable a developer sets to use a folder of their own.
const MODEL_PATH_VAR: &str = "DEMUCS_MODEL_PATH";

// ---------------------------------------------------------------------
// The seam
// ---------------------------------------------------------------------

/// Why stems could not be made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StemFailure {
    /// No `DEMUCS_MODEL_PATH`, and the user has not downloaded the model.
    ModelNotDownloaded,
    /// The downloaded model is on disk and verified, and still will not load.
    ModelDamaged,
    /// `DEMUCS_MODEL_PATH` is set to something that is not the model folder.
    ModelPathInvalid,
    /// No usable ONNX Runtime. The reason is short and carries no path.
    RuntimeUnavailable(String),
    /// The runtime ran out of memory.
    OutOfMemory,
    /// The user pressed Cancel.
    Cancelled,
    /// Anything else the run reported, in words with no path in them.
    Failed(String),
}

/// A model found and identified, not yet loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelRef {
    /// The folder holding the graph and its weights.
    pub folder: PathBuf,
    /// What the cache key folds in: changes when the model does. Never a path.
    pub fingerprint: String,
    pub downloaded: bool,
}

/// Stem separation, as the tool needs it. [`DemucsStems`] is the real one;
/// tests supply their own.
pub trait StemModel: Send + Sync {
    /// Find the model and say what it is, or why it cannot be used. Loads
    /// nothing.
    fn locate(&self) -> Result<ModelRef, StemFailure>;

    /// Separate `audio` into the entry `dest` (a directory that does not exist
    /// yet), calling `control.progress` as segments are done and reading
    /// `control.cancel` before each. On any error or Cancel, nothing is left at
    /// `dest` or beside it.
    fn separate(
        &self,
        model: &ModelRef,
        audio: DecodedAudio,
        dest: &Path,
        control: &Control<'_>,
    ) -> Result<Separated, StemFailure>;
}

// ---------------------------------------------------------------------
// The real model
// ---------------------------------------------------------------------

/// Find the model without fetching anything: `env` (the value of
/// `DEMUCS_MODEL_PATH`) first, else `downloaded()`. A variable that is set wins
/// even when it is wrong: the developer asked for that folder.
pub fn resolve_model(
    env: Option<OsString>,
    downloaded: impl FnOnce() -> Option<PathBuf>,
) -> Result<ModelRef, StemFailure> {
    let pinned = ml_pipeline::models::htdemucs();
    if let Some(value) = env.filter(|v| !v.is_empty()) {
        let folder = PathBuf::from(value);
        if !folder.is_dir() {
            return Err(StemFailure::ModelPathInvalid);
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
    let folder = downloaded().ok_or(StemFailure::ModelNotDownloaded)?;
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

/// The real thing: the pinned htdemucs on ONNX Runtime.
#[derive(Debug, Default, Clone, Copy)]
pub struct DemucsStems;

/// The loaded model, kept for the next call. One slot.
static LOADED: Mutex<Option<(String, Arc<DemucsModel>)>> = Mutex::new(None);

impl DemucsStems {
    fn load(model: &ModelRef) -> Result<Arc<DemucsModel>, StemFailure> {
        let key = format!("{}|{}", model.folder.display(), model.fingerprint);
        let mut slot = LOADED.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((held, loaded)) = slot.as_ref() {
            if *held == key {
                return Ok(Arc::clone(loaded));
            }
        }
        match DemucsModel::load(ml_demucs::DEFAULT_MODEL_ID, &model.folder) {
            Ok(loaded) => {
                let loaded = Arc::new(loaded);
                *slot = Some((key, Arc::clone(&loaded)));
                Ok(loaded)
            }
            Err(DemucsError::RuntimeUnavailable(reason)) => {
                tracing::warn!(%reason, "the ONNX Runtime library could not be used");
                Err(StemFailure::RuntimeUnavailable(
                    "the ONNX Runtime library could not be loaded".to_string(),
                ))
            }
            Err(other) => {
                // The reason names files and paths: logged, not handed on.
                tracing::warn!(error = %other, "the stem model could not be loaded");
                Err(if model.downloaded {
                    StemFailure::ModelDamaged
                } else {
                    StemFailure::ModelPathInvalid
                })
            }
        }
    }
}

/// What the agent is told of a failure of the run itself: fixed sentences for
/// the errors that name files, the error's own words for the rest.
fn public_reason(error: &DemucsError) -> String {
    match error {
        DemucsError::Io(_) | DemucsError::Wav(_) => {
            tracing::warn!(%error, "the stems could not be written");
            "the stems could not be written (is the disk full?)".to_string()
        }
        other => other.to_string(),
    }
}

impl StemModel for DemucsStems {
    fn locate(&self) -> Result<ModelRef, StemFailure> {
        // The runtime first: a build that has none cannot run any model, and
        // sending its user to download one first is the worse trap.
        ml_pipeline::runtime::ensure()
            .map_err(|e| StemFailure::RuntimeUnavailable(runtime_reason(&e)))?;
        resolve_model(std::env::var_os(MODEL_PATH_VAR), || {
            ml_pipeline::models::cached(ml_pipeline::models::HTDEMUCS_ID)
        })
    }

    fn separate(
        &self,
        model: &ModelRef,
        audio: DecodedAudio,
        dest: &Path,
        control: &Control<'_>,
    ) -> Result<Separated, StemFailure> {
        let loaded = Self::load(model)?;
        loaded
            .separate_to_dir(audio, dest, control)
            .map_err(|e| match e {
                DemucsError::Cancelled => StemFailure::Cancelled,
                ref other if ml_demucs::is_oom_error(other) => StemFailure::OutOfMemory,
                other => StemFailure::Failed(public_reason(&other)),
            })
    }
}

// ---------------------------------------------------------------------
// What the model is told
// ---------------------------------------------------------------------

fn model_megabytes() -> u64 {
    ml_pipeline::Catalog::built_in()
        .get(ml_pipeline::models::HTDEMUCS_ID)
        .map_or(0, |m| (m.size() + 500_000) / 1_000_000)
}

/// The answer for stems that could not be made: the text of the
/// `ToolResult::Error`. None starts with a setup marker (see the module docs).
pub fn answer(failure: &StemFailure) -> String {
    match failure {
        StemFailure::ModelNotDownloaded => format!(
            "The stem model (about {} MB) is not on this computer, so audio cannot be split into \
             stems yet. Tell the user; only they can download it, with Download on the Stem \
             model row in Settings → Account. Do not try to download, install or configure \
             anything yourself, and do not call separate_stems again until the user says the \
             model is there.",
            model_megabytes()
        ),
        StemFailure::RuntimeUnavailable(reason) => format!(
            "Stem separation needs the ONNX Runtime library, and {reason}. The release \
             installers include it; a build from source does not. Tell the user: for a build \
             from source, set ORT_DYLIB_PATH to a copy of ONNX Runtime 1.23.2 \
             (docs/development-guide.md, \"ML Model Files\"). Do not try to install anything \
             yourself."
        ),
        StemFailure::ModelDamaged => "The stem model is on this computer but could not be \
             loaded, so the download is damaged. Tell the user to press Remove and then \
             Download on the Stem model row in Settings → Account. Do not try to repair or \
             replace files yourself."
            .to_string(),
        StemFailure::ModelPathInvalid => format!(
            "{MODEL_PATH_VAR} is set, and what it names is not the stem model: the folder has \
             to hold htdemucs_fwd.onnx and htdemucs_fwd.onnx.data. Tell the user to correct it \
             or to unset it."
        ),
        StemFailure::OutOfMemory => "Stem separation ran out of memory. A computer with less \
             free memory than the model needs cannot run it, and there is no smaller model to \
             fall back to. Tell the user, and do not call separate_stems again unless they \
             free memory and ask."
            .to_string(),
        StemFailure::Cancelled => "Stem separation was cancelled by the user. Nothing was \
             changed and nothing was kept. Do not run it again unless they ask."
            .to_string(),
        StemFailure::Failed(why) => format!("stem separation failed: {why}"),
    }
}

// ---------------------------------------------------------------------
// The tool
// ---------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    #[serde(default)]
    track: Option<usize>,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    model: Option<String>,
}

pub struct SeparateStemsTool {
    model: Arc<dyn StemModel>,
    cache_cap: u64,
}

impl SeparateStemsTool {
    /// The tool with the real model.
    pub fn new() -> Self {
        Self::with(Arc::new(DemucsStems))
    }

    /// The tool with any model: what the tests use.
    pub fn with(model: Arc<dyn StemModel>) -> Self {
        Self {
            model,
            cache_cap: ml_demucs::STEM_CACHE_CAP_BYTES,
        }
    }

    /// Keep `stem-cache/` under this many bytes instead of the default.
    #[must_use]
    pub fn with_cache_cap(mut self, bytes: u64) -> Self {
        self.cache_cap = bytes;
        self
    }
}

impl Default for SeparateStemsTool {
    fn default() -> Self {
        Self::new()
    }
}

/// What the progress strip is told while a run goes, and the closing `done`
/// that clears it. Nothing is said, and nothing cleared, for a cache hit.
struct Strip {
    file: String,
    started: Cell<bool>,
    total: Cell<usize>,
    cancelled: Cell<bool>,
}

impl Strip {
    fn new(file: String) -> Self {
        Self {
            file,
            started: Cell::new(false),
            total: Cell::new(0),
            cancelled: Cell::new(false),
        }
    }

    fn step(&self, done: usize, total: usize) {
        self.started.set(true);
        self.total.set(total);
        crate::progress::report(json!({
            "kind": "separate_stems",
            "index": done.min(total.saturating_sub(1)),
            "total": total,
            "file": self.file,
        }));
    }
}

impl Drop for Strip {
    /// However the run ends, a strip that was shown is cleared.
    fn drop(&mut self) {
        if self.started.get() {
            crate::progress::report(json!({
                "kind": "separate_stems",
                "done": true,
                "cancelled": self.cancelled.get(),
                "total": self.total.get(),
                "file": self.file,
            }));
        }
    }
}

fn round2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

impl Tool for SeparateStemsTool {
    fn name(&self) -> &'static str {
        "separate_stems"
    }

    fn schema(&self) -> Value {
        crate::schema::anthropic_tool(
            "separate_stems",
            "Split `track` or a file (`path`) into vocals, drums, bass and other stems with Demucs, \
             on this computer. Writes four WAVs and returns their paths; the session is not \
             changed, so `load` a stem to put it on a track. Stems are estimates. Needs the stem \
             model, which only the user can download (once, in Settings → Account); if the call \
             says it or the runtime is missing, tell the user and stop, and do not try to \
             download, install or configure anything. CPU, about half the audio's length; \
             cancellable.",
            json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
                    "path": { "type": "string" },
                    "model": {
                        "type": "string",
                        "enum": ml_demucs::SUPPORTED_MODEL_IDS,
                    },
                },
                "additionalProperties": false,
            }),
        )
    }

    // The default `mutates` (true) stands: a run writes hundreds of megabytes
    // and takes minutes, so Plan first holds it. See the module docs.

    // A run takes about half the audio's length and holds the store's lock for
    // none of it (#421). The module docs say why that is safe.
    fn runs_off_the_lock(&self) -> bool {
        true
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };
        let model_id = args.model.as_deref().unwrap_or(ml_demucs::DEFAULT_MODEL_ID);
        if !ml_demucs::SUPPORTED_MODEL_IDS.contains(&model_id) {
            return Ok(ToolResult::Error(format!(
                "unsupported model {model_id:?}; expected one of {:?}",
                ml_demucs::SUPPORTED_MODEL_IDS
            )));
        }

        // 1. Which audio.
        let (path, what) = match (args.track, &args.path) {
            (Some(track), None) => {
                let state = match load_head_state(ctx) {
                    Ok(s) => s,
                    Err(msg) => return Ok(ToolResult::Error(msg)),
                };
                if let Err(e) = check_track_index(&state.tracks, track) {
                    return Ok(ToolResult::Error(e));
                }
                let clips = &state.tracks[track].clips;
                let path = match lane_audio_path(ctx.store.project_dir(), clips) {
                    Some(p) => p,
                    None => match flattened_track_wav(ctx.store.project_dir(), clips) {
                        Ok(p) => p,
                        Err(e) => return Ok(ToolResult::Error(format!("track {track}: {e}"))),
                    },
                };
                (path, format!("track {track}"))
            }
            (None, Some(path)) => {
                let path = PathBuf::from(path);
                if !path.exists() {
                    return Ok(ToolResult::Error(format!(
                        "file not found: {}",
                        path.display()
                    )));
                }
                let name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("audio")
                    .to_string();
                (path, name)
            }
            (Some(_), Some(_)) => {
                return Ok(ToolResult::Error(
                    "give `track` or `path`, not both".to_string(),
                ))
            }
            (None, None) => {
                return Ok(ToolResult::Error(
                    "give `track` (a track's index in the session) or `path` (a file)".to_string(),
                ))
            }
        };

        // 2. Which model. Nothing is loaded and nothing fetched.
        let model = match self.model.locate() {
            Ok(m) => m,
            Err(e) => return Ok(ToolResult::Error(answer(&e))),
        };

        // 3. The audio.
        let decoded = match audio_decoder::decode_file(&path) {
            Ok(d) => d,
            Err(e) => return Ok(ToolResult::Error(format!("decode failed: {e}"))),
        };
        let frames = decoded.samples.len() / usize::from(decoded.channels.max(1));
        let (rate, channels) = (
            decoded.sample_rate,
            if decoded.channels == 1 { 1 } else { 2 },
        );
        let duration_sec = frames as f64 / f64::from(rate.max(1));

        // 4. The cache: the audio, the model, the format.
        let key = ContentHash::combine(&[
            ContentHash::from_bytes(
                crate::provenance::audio_hash(
                    &decoded.samples,
                    decoded.sample_rate,
                    decoded.channels,
                )
                .as_bytes(),
            ),
            ContentHash::from_bytes(model.fingerprint.as_bytes()),
            ContentHash::from_bytes(format!("stems-format:{STEMS_FORMAT}").as_bytes()),
        ]);
        let root = ctx
            .store
            .project_dir()
            .join(".audiograph")
            .join("stem-cache");
        let dest = ml_demucs::stem_dir_for(&root, &key.to_hex());

        let (paths, from_cache, clipped, removed) =
            if let Some(paths) = ml_demucs::read_cached_stems(&dest) {
                ml_demucs::touch(&dest);
                (paths, true, [0u64; 4], ml_demucs::Pruned::default())
            } else {
                // Clears a Cancel pressed before this run began.
                crate::progress::begin();
                let strip = Strip::new(what.clone());
                let result = self.model.separate(
                    &model,
                    decoded,
                    &dest,
                    &Control {
                        cancel: &|| crate::progress::cancelled(),
                        progress: &|done, total| strip.step(done, total),
                    },
                );
                match result {
                    Ok(done) => {
                        let removed = ml_demucs::prune(&root, self.cache_cap, &dest);
                        (done.paths, false, done.clipped, removed)
                    }
                    Err(e) => {
                        strip.cancelled.set(e == StemFailure::Cancelled);
                        return Ok(ToolResult::Error(answer(&e)));
                    }
                }
            };

        // 5. The answer.
        let stems: serde_json::Map<String, Value> = STEM_NAMES
            .iter()
            .zip(paths.iter())
            .map(|(name, p)| ((*name).to_string(), json!(p.to_string_lossy())))
            .collect();
        let mut summary = format!(
            "Separated {what} into vocals, drums, bass and other with {model_id}{}. The four \
             WAVs are 16-bit {}, {rate} Hz, as long as the source. Nothing in the session was \
             changed: call `load` with a stem's path to put it on a new track (the project keeps \
             its own copy, so the files here can go). The stems are estimates, so each carries a \
             little of the others, and they add up to the source only approximately.",
            if from_cache {
                ", from the cache: this audio was separated before"
            } else {
                ""
            },
            if channels == 1 { "mono" } else { "stereo" },
        );
        let clipped_names: Vec<&str> = STEM_NAMES
            .iter()
            .zip(clipped)
            .filter(|(_, n)| *n > 0)
            .map(|(name, _)| *name)
            .collect();
        if !clipped_names.is_empty() {
            summary.push_str(&format!(
                " The {} went past full scale in places and was clipped there.",
                clipped_names.join(" and ")
            ));
        }
        if removed.entries > 0 {
            summary.push_str(&format!(
                " To keep the project's stem cache under its limit, {} older separation{} \
                 ({} MB) {} removed.",
                removed.entries,
                if removed.entries == 1 { "" } else { "s" },
                removed.bytes / 1_000_000,
                if removed.entries == 1 { "was" } else { "were" },
            ));
        }

        Ok(ToolResult::Ok(json!({
            "model": model_id,
            "cached": from_cache,
            "duration_sec": round2(duration_sec),
            "sample_rate": rate,
            "channels": channels,
            "stems": stems,
            "summary": summary,
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_answer_starts_with_a_setup_marker() {
        // The setup card speaks of the speech model; see the module docs.
        for f in [
            StemFailure::ModelNotDownloaded,
            StemFailure::ModelDamaged,
            StemFailure::ModelPathInvalid,
            StemFailure::RuntimeUnavailable("x".into()),
            StemFailure::OutOfMemory,
            StemFailure::Cancelled,
            StemFailure::Failed("x".into()),
        ] {
            let text = answer(&f);
            for m in crate::tool::transcribe::MARKERS {
                assert!(!text.contains(m), "{f:?}: {text}");
            }
        }
    }

    #[test]
    fn a_developers_folder_wins_over_the_download_and_is_identified_by_what_is_in_it() {
        let dir = tempfile::tempdir().unwrap();
        for file in &ml_pipeline::models::htdemucs().files {
            std::fs::write(dir.path().join(&file.path), b"x").unwrap();
        }
        let a = resolve_model(Some(dir.path().into()), || panic!("not asked")).unwrap();
        assert!(!a.downloaded);
        std::fs::write(dir.path().join("htdemucs_fwd.onnx"), b"changed").unwrap();
        let b = resolve_model(Some(dir.path().into()), || None).unwrap();
        assert_ne!(a.fingerprint, b.fingerprint, "another file, another cache");
        assert_eq!(
            resolve_model(Some("/definitely/not/a/folder".into()), || None),
            Err(StemFailure::ModelPathInvalid)
        );
    }

    #[test]
    fn the_download_is_found_by_the_pins_and_a_missing_one_is_not_downloaded() {
        let m = resolve_model(None, || Some(PathBuf::from("/models/htdemucs"))).unwrap();
        assert!(m.downloaded);
        assert_eq!(
            resolve_model(None, || None),
            Err(StemFailure::ModelNotDownloaded)
        );
        // The empty variable is no variable.
        assert_eq!(
            resolve_model(Some(OsString::new()), || None),
            Err(StemFailure::ModelNotDownloaded)
        );
    }

    #[test]
    fn what_the_agent_is_told_of_a_write_failure_names_no_path() {
        let io = DemucsError::Io(std::io::Error::other(
            "/home/someone/project/.audiograph full",
        ));
        let text = public_reason(&io);
        assert!(!text.contains("/home"), "{text}");
    }
}
