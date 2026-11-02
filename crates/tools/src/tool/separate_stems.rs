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
//! model; the answers tell the agent to tell the user and stop instead, and
//! name the Stem model row in Settings → Account, where only the user's click
//! downloads the model (as `diarize` names the Speaker model row).
//! The paths in a *successful* answer are the point of it: the agent hands them
//! to `load`.
//!
//! ## Testing without a model
//!
//! [`SeparateStemsTool::with`] takes any [`StemModel`], so CI runs the whole
//! tool against a fake that runs the real pipeline around a network that is
//! not one. [`DemucsStems`] is the real one.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use audio_decoder::DecodedAudio;
use ml_demucs::{Control, DemucsError, DemucsModel, Separated, STEM_NAMES};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::tool::local_model::{
    self, cache_key, decode_input, into_result, parse_args, resolve_audio_source, round2,
    LoadedSlot, ModelLookup, ProgressStrip,
};
use crate::{Tool, ToolContext, ToolResult};

pub use crate::tool::local_model::ModelRef;

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

impl From<ModelLookup> for StemFailure {
    fn from(lookup: ModelLookup) -> Self {
        match lookup {
            ModelLookup::RuntimeUnavailable(reason) => Self::RuntimeUnavailable(reason),
            ModelLookup::NotDownloaded => Self::ModelNotDownloaded,
            ModelLookup::PathInvalid => Self::ModelPathInvalid,
        }
    }
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
    local_model::resolve(ml_pipeline::models::htdemucs(), env, downloaded).map_err(Into::into)
}

/// The real thing: the pinned htdemucs on ONNX Runtime.
#[derive(Debug, Default, Clone, Copy)]
pub struct DemucsStems;

/// The loaded model, kept for the next call. One slot.
static LOADED: LoadedSlot<DemucsModel> = LoadedSlot::new();

/// Return unused glibc heap pages after inference (#868). This is only a
/// request: live model allocations stay cached and the allocator may return
/// nothing. glibc inspects process-wide arenas, so this can briefly contend
/// with other threads, including another run of the model; do it once after
/// this run has released its network lock, and not on a cache hit.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
fn release_allocator_pages() {
    unsafe extern "C" {
        fn malloc_trim(pad: usize) -> std::ffi::c_int;
    }

    // SAFETY: glibc provides malloc_trim on this target; zero is a valid pad,
    // and the function is thread-safe and only releases free allocator pages.
    let _ = unsafe { malloc_trim(0) };
}

#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
fn release_allocator_pages() {}

impl DemucsStems {
    fn load(model: &ModelRef) -> Result<Arc<DemucsModel>, StemFailure> {
        LOADED
            .get_or_load(model, |folder| {
                DemucsModel::load(ml_demucs::DEFAULT_MODEL_ID, folder)
            })
            .map_err(|e| match e {
                DemucsError::RuntimeUnavailable(reason) => {
                    StemFailure::RuntimeUnavailable(local_model::runtime_load_reason(&reason))
                }
                other => load_failure(&other, model.downloaded),
            })
    }
}

/// What a model that would not load is reported as. Running out of memory
/// while loading is the computer's, not the download's: telling the user to
/// remove and fetch the model again would not help, so it is checked first.
fn load_failure(error: &DemucsError, downloaded: bool) -> StemFailure {
    if ml_demucs::is_oom_error(error) {
        tracing::warn!(%error, "the stem model could not be loaded: out of memory");
        return StemFailure::OutOfMemory;
    }
    // The reason names files and paths: logged, not handed on.
    tracing::warn!(%error, "the stem model could not be loaded");
    if downloaded {
        StemFailure::ModelDamaged
    } else {
        StemFailure::ModelPathInvalid
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
        // The runtime first: see `local_model::locate`.
        local_model::locate(
            ml_pipeline::models::htdemucs(),
            MODEL_PATH_VAR,
            ml_pipeline::models::HTDEMUCS_ID,
        )
        .map_err(Into::into)
    }

    fn separate(
        &self,
        model: &ModelRef,
        audio: DecodedAudio,
        dest: &Path,
        control: &Control<'_>,
    ) -> Result<Separated, StemFailure> {
        let loaded = Self::load(model)?;
        let result = loaded.separate_to_dir(audio, dest, control);
        // The call has returned, so its network guard and temporary buffers
        // are gone. Reclaim on success, cancellation and errors alike.
        release_allocator_pages();
        result.map_err(|e| match e {
            DemucsError::Cancelled => StemFailure::Cancelled,
            ref other if ml_demucs::is_oom_error(other) => StemFailure::OutOfMemory,
            other => StemFailure::Failed(public_reason(&other)),
        })
    }
}

// ---------------------------------------------------------------------
// What the model is told
// ---------------------------------------------------------------------

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
            local_model::model_megabytes(ml_pipeline::models::HTDEMUCS_ID)
        ),
        StemFailure::RuntimeUnavailable(reason) => {
            local_model::runtime_unavailable_answer("Stem separation", reason)
        }
        StemFailure::ModelDamaged => "The stem model is on this computer but could not be \
             loaded, so the download is damaged. Tell the user to press Remove and then Download \
             on the Stem model row in Settings → Account. Do not try to repair or replace files \
             yourself."
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
             model the user downloads once in Settings → Account, and the ONNX Runtime; if the \
             call says either is missing, tell the user and stop, and do not try to download, \
             install or configure anything. CPU, about half the audio's length; cancellable.",
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
        Ok(into_result(self.run(args, ctx)))
    }
}

impl SeparateStemsTool {
    /// The call, ending at the first thing it must refuse (`Err` is the
    /// `ToolResult::Error`'s message).
    fn run(&self, args: Value, ctx: &ToolContext) -> Result<Value, String> {
        let args: Args = parse_args(args)?;
        let model_id = args.model.as_deref().unwrap_or(ml_demucs::DEFAULT_MODEL_ID);
        if !ml_demucs::SUPPORTED_MODEL_IDS.contains(&model_id) {
            return Err(format!(
                "unsupported model {model_id:?}; expected one of {:?}",
                ml_demucs::SUPPORTED_MODEL_IDS
            ));
        }

        // 1. Which audio.
        let source = resolve_audio_source(ctx, args.track, args.path.as_deref())?;
        let what = source.label;

        // 2. Which model. Nothing is loaded and nothing fetched.
        let model = self.model.locate().map_err(|e| answer(&e))?;

        // 3. The audio.
        let (decoded, duration_sec) = decode_input(&source.path)?;
        let (rate, channels) = (
            decoded.sample_rate,
            if decoded.channels == 1 { 1 } else { 2 },
        );

        // 4. The cache: the audio, the model, the format.
        let key = cache_key(
            &decoded,
            &model.fingerprint,
            &format!("stems-format:{STEMS_FORMAT}"),
        );
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
                let strip = ProgressStrip::new("separate_stems", what.clone());
                let result = self.model.separate(
                    &model,
                    decoded,
                    &dest,
                    &Control {
                        cancel: &|| crate::progress::cancelled(),
                        progress: &|done, total| strip.step(done, total, None),
                    },
                );
                match result {
                    Ok(done) => {
                        let removed = ml_demucs::prune(&root, self.cache_cap, &dest);
                        (done.paths, false, done.clipped, removed)
                    }
                    Err(e) => {
                        strip.set_cancelled(e == StemFailure::Cancelled);
                        return Err(answer(&e));
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

        Ok(json!({
            "model": model_id,
            "cached": from_cache,
            "duration_sec": round2(duration_sec),
            "sample_rate": rate,
            "channels": channels,
            "stems": stems,
            "summary": summary,
        }))
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
    fn a_model_that_runs_out_of_memory_loading_is_not_called_damaged() {
        for downloaded in [true, false] {
            for reason in [
                "out of memory",
                "std::bad_alloc",
                "memory allocation of 4 GB failed",
            ] {
                assert_eq!(
                    load_failure(&DemucsError::Ort(reason.to_string()), downloaded),
                    StemFailure::OutOfMemory,
                    "{reason}"
                );
            }
        }
        // Any other failure stays the model's: the download's, or the path's.
        let other = DemucsError::Ort("invalid protobuf".to_string());
        assert_eq!(load_failure(&other, true), StemFailure::ModelDamaged);
        assert_eq!(load_failure(&other, false), StemFailure::ModelPathInvalid);
        let unexpected = DemucsError::UnexpectedExport("an input is missing".to_string());
        assert_eq!(load_failure(&unexpected, true), StemFailure::ModelDamaged);
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
