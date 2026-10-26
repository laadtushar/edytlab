//! `diarize` tool: who spoke when, on this computer (#168 section 2).
//!
//! The turns are made by `ml_diarize` (pyannote.audio 3.1's segmentation and
//! clustering pipeline on the pinned `speaker-diarization` model), and the tool
//! is the glue around it, shaped like `transcribe` (`transcribe.rs`):
//!
//! 1. **Which audio.** `track` (the track as it sits on the timeline, so turn
//!    times are session seconds and line up with `split_by_speaker`) xor `path`
//!    (a file that is not in the session; its times are the file's).
//! 2. **Which model, never fetched.** [`SpeakerModel::locate`]: the ONNX Runtime
//!    library first, then `DIARIZE_MODEL_PATH` (a developer's folder), else the
//!    speaker model the user downloaded (`ml_pipeline::models::cached`). The
//!    tool never downloads anything (`ml-pipeline/tests/only_the_user_downloads.rs`).
//! 3. **The cache.** The turns are kept in the project's inference cache under
//!    the audio's content hash combined with the model's identity, the speaker
//!    bounds and [`DIARIZE_FORMAT`].
//! 4. **Progress and cancel.** `kind: "diarize"` events (segmentation windows,
//!    then voiceprints) and the cancel switch the window's Cancel button sets.
//! 5. **No node.** The answer is turns only. The session is not written and no
//!    node is appended: the agent hands them on to `split_by_speaker`, which
//!    already takes `(start, end, speaker)` and is held for approval as an
//!    edit. That makes this tool read-only (`mutates` is `false`); the one thing
//!    it leaves behind is an inference-cache entry.
//!
//! ## Off the store lock
//!
//! A run takes about a third of the audio's length on the CPU, so the tool runs
//! off the store lock (`runs_off_the_lock`, #421) exactly as `transcribe`
//! does: it reads the head and one track or one file and appends nothing, so a
//! lost race is a re-run that hits the cache the first run filled. See the
//! `ALLOWED` list in `tests/off_lock_tools.rs` for the `progress` spellings.
//!
//! ## What a failure says
//!
//! The answers carry no local path and no loader reason (the reason is logged):
//! tool results go to the model provider. They carry no setup marker either,
//! because the setup card (`ai::ToolView::SpeechSetup`) speaks of the speech
//! model; Settings has no row for the speaker model yet (#168 part 3), and the
//! answers say so rather than send the user to a button that is not there.
//!
//! ## Testing without a model
//!
//! [`DiarizeTool::with`] takes any [`SpeakerModel`], so CI runs the whole tool
//! against a fake. [`PyannoteSpeakers`] is the real one.

use std::cell::Cell;
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use ml_pipeline::{ContentHash, InferenceCache};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::tool::util::{check_track_index, flattened_track_wav, lane_audio_path, load_head_state};
use crate::{Tool, ToolContext, ToolResult};

pub use ml_diarize::Stage;

/// Bumped when a change to the pipeline would give different turns for the same
/// audio and model, so an entry written before it is not read after it.
pub const DIARIZE_FORMAT: u32 = 1;

/// The most the turns in a result may take, in bytes of JSON.
pub const RESULT_BUDGET_BYTES: usize = 6_000;

/// The variable a developer sets to use a folder of their own.
const MODEL_PATH_VAR: &str = "DIARIZE_MODEL_PATH";

/// The most speakers a caller may ask for. A meeting has a dozen at the very
/// most; the cap keeps a typo from reaching the clustering.
const MAX_SPEAKERS_ASKED: usize = 64;

// ---------------------------------------------------------------------
// The seam
// ---------------------------------------------------------------------

/// Why a diarization could not be made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiarizeFailure {
    /// No `DIARIZE_MODEL_PATH`, and the user has not downloaded the model.
    ModelNotDownloaded,
    /// The downloaded model is on disk and verified, and still will not load.
    ModelDamaged,
    /// `DIARIZE_MODEL_PATH` is set to something that is not the model folder.
    ModelPathInvalid,
    /// No usable ONNX Runtime. The reason is short and carries no path.
    RuntimeUnavailable(String),
    /// The speaker bounds make no sense.
    BadSpeakerCount(String),
    /// The user pressed Cancel.
    Cancelled,
    /// Anything else the run reported.
    Failed(String),
}

/// A model found and identified, not yet loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelRef {
    /// The folder holding both networks.
    pub folder: PathBuf,
    /// What the cache key folds in: changes when the model does. Never a path.
    pub fingerprint: String,
    pub downloaded: bool,
}

/// One stretch of one speaker talking, in seconds from the start of the audio.
/// `speaker` counts from 0 in the order the speakers first speak.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Turn {
    pub start_s: f64,
    pub end_s: f64,
    pub speaker: usize,
}

/// What a diarization made. This is also the cache's value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Heard {
    pub turns: Vec<Turn>,
    pub speakers: usize,
    pub duration_s: f64,
}

/// Speaker diarization, as the tool needs it. [`PyannoteSpeakers`] is the real
/// one; tests supply their own.
pub trait SpeakerModel: Send + Sync {
    /// Find the model and say what it is, or why it cannot be used. Loads
    /// nothing.
    fn locate(&self) -> Result<ModelRef, DiarizeFailure>;

    /// Diarize `audio` (16 kHz mono, -1 to 1). `bounds` are
    /// `(num_speakers, min_speakers, max_speakers)`. `on_progress(stage, done,
    /// total)` is called as work is done; `cancel` answers `true` once the
    /// user has asked to stop, and the run then ends with
    /// [`DiarizeFailure::Cancelled`].
    fn diarize(
        &self,
        model: &ModelRef,
        audio: &[f32],
        bounds: (Option<usize>, Option<usize>, Option<usize>),
        on_progress: &dyn Fn(Stage, usize, usize),
        cancel: &(dyn Fn() -> bool + Sync),
    ) -> Result<Heard, DiarizeFailure>;
}

// ---------------------------------------------------------------------
// The real model
// ---------------------------------------------------------------------

/// Find the model without fetching anything: `env` (the value of
/// `DIARIZE_MODEL_PATH`) first, else `downloaded()`. A variable that is set wins
/// even when it is wrong: the developer asked for that folder.
pub fn resolve_model(
    env: Option<OsString>,
    downloaded: impl FnOnce() -> Option<PathBuf>,
) -> Result<ModelRef, DiarizeFailure> {
    let pinned = ml_pipeline::models::speaker_diarization();
    if let Some(value) = env.filter(|v| !v.is_empty()) {
        let folder = PathBuf::from(value);
        if !folder.is_dir() {
            return Err(DiarizeFailure::ModelPathInvalid);
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
    let folder = downloaded().ok_or(DiarizeFailure::ModelNotDownloaded)?;
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

/// The real thing: the pinned pyannote 3.1 pipeline on ONNX Runtime.
#[derive(Debug, Default, Clone, Copy)]
pub struct PyannoteSpeakers;

/// The loaded model, kept for the next call. One slot.
static LOADED: Mutex<Option<(String, Arc<ml_diarize::DiarizeModel>)>> = Mutex::new(None);

impl PyannoteSpeakers {
    fn load(model: &ModelRef) -> Result<Arc<ml_diarize::DiarizeModel>, DiarizeFailure> {
        use ml_diarize::LoadError as L;
        let key = format!("{}|{}", model.folder.display(), model.fingerprint);
        let mut slot = LOADED.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((held, loaded)) = slot.as_ref() {
            if *held == key {
                return Ok(Arc::clone(loaded));
            }
        }
        match ml_diarize::DiarizeModel::load(&model.folder) {
            Ok(loaded) => {
                let loaded = Arc::new(loaded);
                *slot = Some((key, Arc::clone(&loaded)));
                Ok(loaded)
            }
            Err(L::RuntimeUnavailable(reason)) => {
                tracing::warn!(%reason, "the ONNX Runtime library could not be used");
                Err(DiarizeFailure::RuntimeUnavailable(
                    "the ONNX Runtime library could not be loaded".to_string(),
                ))
            }
            Err(other) => {
                // The reason names files and paths: logged, not handed on.
                tracing::warn!(error = %other, "the speaker model could not be loaded");
                Err(if model.downloaded {
                    DiarizeFailure::ModelDamaged
                } else {
                    DiarizeFailure::ModelPathInvalid
                })
            }
        }
    }
}

impl SpeakerModel for PyannoteSpeakers {
    fn locate(&self) -> Result<ModelRef, DiarizeFailure> {
        // The runtime first: a build that has none cannot run any model, and
        // sending its user to download one first is the worse trap.
        ml_pipeline::runtime::ensure()
            .map_err(|e| DiarizeFailure::RuntimeUnavailable(runtime_reason(&e)))?;
        resolve_model(std::env::var_os(MODEL_PATH_VAR), || {
            ml_pipeline::models::cached(ml_pipeline::models::SPEAKER_DIARIZATION_ID)
        })
    }

    fn diarize(
        &self,
        model: &ModelRef,
        audio: &[f32],
        bounds: (Option<usize>, Option<usize>, Option<usize>),
        on_progress: &dyn Fn(Stage, usize, usize),
        cancel: &(dyn Fn() -> bool + Sync),
    ) -> Result<Heard, DiarizeFailure> {
        use ml_diarize::DiarizeError as D;
        let bounds = ml_diarize::SpeakerBounds::new(bounds.0, bounds.1, bounds.2)
            .map_err(|e| DiarizeFailure::BadSpeakerCount(e.to_string()))?;
        let loaded = Self::load(model)?;
        let options = ml_diarize::Options {
            bounds,
            on_progress,
            cancel,
        };
        match loaded.diarize(audio, &options) {
            Ok(d) => Ok(Heard {
                turns: d
                    .turns
                    .iter()
                    .map(|t| Turn {
                        start_s: t.start_s,
                        end_s: t.end_s,
                        speaker: t.speaker,
                    })
                    .collect(),
                speakers: d.speakers,
                duration_s: d.duration_s,
            }),
            Err(D::Cancelled) => Err(DiarizeFailure::Cancelled),
            Err(D::Bounds(e)) => Err(DiarizeFailure::BadSpeakerCount(e.to_string())),
            Err(other) => {
                tracing::warn!(error = %other, "diarization failed");
                Err(DiarizeFailure::Failed(other.to_string()))
            }
        }
    }
}

// ---------------------------------------------------------------------
// What the model is told
// ---------------------------------------------------------------------

fn model_megabytes() -> u64 {
    ml_pipeline::Catalog::built_in()
        .get(ml_pipeline::models::SPEAKER_DIARIZATION_ID)
        .map_or(0, |m| (m.size() + 500_000) / 1_000_000)
}

/// The answer for a diarization that could not be made: the text of the
/// `ToolResult::Error`. None starts with a setup marker (see the module docs).
pub fn answer(failure: &DiarizeFailure) -> String {
    match failure {
        DiarizeFailure::ModelNotDownloaded => format!(
            "The speaker model (about {} MB) is not on this computer, so speakers cannot be told \
             apart yet. Tell the user; only they can download it, and this build's Settings does \
             not have a button for it yet. Do not try to download, install or configure anything \
             yourself, and do not call diarize again until the user says the model is there.",
            model_megabytes()
        ),
        DiarizeFailure::RuntimeUnavailable(reason) => format!(
            "Speaker diarization needs the ONNX Runtime library, and {reason}. The release \
             installers include it; a build from source does not. Tell the user: for a build \
             from source, set ORT_DYLIB_PATH to a copy of ONNX Runtime 1.23.2 \
             (docs/development-guide.md, \"ML Model Files\"). Do not try to install anything \
             yourself."
        ),
        DiarizeFailure::ModelDamaged => "The speaker model is on this computer but could not be \
             loaded, so the download is damaged. Tell the user to remove it and download it \
             again when Settings offers that. Do not try to repair or replace files yourself."
            .to_string(),
        DiarizeFailure::ModelPathInvalid => format!(
            "{MODEL_PATH_VAR} is set, and what it names is not the speaker model: the folder has \
             to hold segmentation/model.onnx and embedding/model.onnx. Tell the user to correct \
             it or to unset it."
        ),
        DiarizeFailure::BadSpeakerCount(why) => format!(
            "invalid speaker count: {why}. Give `num_speakers` alone when the number of people \
             is known, or `min_speakers` and `max_speakers` as a range."
        ),
        DiarizeFailure::Cancelled => "Diarization was cancelled by the user. Nothing was \
             changed. Do not run it again unless they ask."
            .to_string(),
        DiarizeFailure::Failed(why) => format!("diarization failed: {why}"),
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
    num_speakers: Option<usize>,
    #[serde(default)]
    min_speakers: Option<usize>,
    #[serde(default)]
    max_speakers: Option<usize>,
}

pub struct DiarizeTool {
    model: Arc<dyn SpeakerModel>,
}

impl DiarizeTool {
    /// The tool with the real model.
    pub fn new() -> Self {
        Self::with(Arc::new(PyannoteSpeakers))
    }

    /// The tool with any model: what the tests use.
    pub fn with(model: Arc<dyn SpeakerModel>) -> Self {
        Self { model }
    }
}

impl Default for DiarizeTool {
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

    fn step(&self, stage: Stage, done: usize, total: usize) {
        self.started.set(true);
        self.total.set(total);
        let doing = match stage {
            Stage::Segmentation => "finding speech",
            Stage::Embedding => "comparing voices",
        };
        crate::progress::report(json!({
            "kind": "diarize",
            "index": done.min(total.saturating_sub(1)),
            "total": total,
            "file": format!("{} ({doing})", self.file),
        }));
    }
}

impl Drop for Strip {
    /// However the run ends, a strip that was shown is cleared.
    fn drop(&mut self) {
        if self.started.get() {
            crate::progress::report(json!({
                "kind": "diarize",
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

/// The name a speaker is given in the result, and so in `split_by_speaker`.
fn label(speaker: usize) -> String {
    format!("speaker_{}", speaker + 1)
}

impl Tool for DiarizeTool {
    fn name(&self) -> &'static str {
        "diarize"
    }

    fn schema(&self) -> Value {
        crate::schema::anthropic_tool(
            "diarize",
            "Find who spoke when, on this computer. Returns speaker turns [start_sec, end_sec, \"speaker_N\"] \
             (numbered by who speaks first) and changes nothing; pass them to split_by_speaker \
             as segments to give each voice a track. Give `track` (session seconds) or `path` \
             (a file's own). If the user said how many people speak, pass `num_speakers`: it is \
             the best way to improve the result; else the count is estimated, bounded by \
             `min_speakers` and `max_speakers`. Sound-alike voices can merge, and overlapping \
             speech goes to one speaker. Needs the speaker model the user downloads once; if the \
             call says the model or runtime is missing, tell the user and stop, and do not try \
             to install anything. CPU, about a third of the audio's length; cancellable.",
            json!({
                "type": "object",
                "properties": {
                    "track": {
                        "type": "integer",
                        "description": "Index of a track in the session."
                    },
                    "path": {
                        "type": "string",
                        "description": "An audio file that is not in the session."
                    },
                    "num_speakers": {
                        "type": "integer",
                        "minimum": 1,
                        "description": "Exactly this many speakers, when the user said."
                    },
                    "min_speakers": {
                        "type": "integer",
                        "minimum": 1,
                        "description": "At least this many speakers."
                    },
                    "max_speakers": {
                        "type": "integer",
                        "minimum": 1,
                        "description": "At most this many speakers."
                    }
                },
                "additionalProperties": false
            }),
        )
    }

    // Nothing is written to the session: the one thing a run leaves is an
    // entry in the inference cache, which `Tool::mutates` does not count.
    fn mutates(&self) -> bool {
        false
    }

    // A run takes a fraction of the audio's length and holds the store's lock
    // for none of it (#421). The module docs say why that is safe.
    fn runs_off_the_lock(&self) -> bool {
        true
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };
        for (name, v) in [
            ("num_speakers", args.num_speakers),
            ("min_speakers", args.min_speakers),
            ("max_speakers", args.max_speakers),
        ] {
            if v.is_some_and(|v| v > MAX_SPEAKERS_ASKED) {
                return Ok(ToolResult::Error(format!(
                    "{name} is more than {MAX_SPEAKERS_ASKED}"
                )));
            }
        }
        let bounds = (args.num_speakers, args.min_speakers, args.max_speakers);
        if let Err(e) = ml_diarize::SpeakerBounds::new(bounds.0, bounds.1, bounds.2) {
            return Ok(ToolResult::Error(answer(&DiarizeFailure::BadSpeakerCount(
                e.to_string(),
            ))));
        }

        // 1. Which audio.
        let state = match load_head_state(ctx) {
            Ok(s) => s,
            Err(msg) => return Ok(ToolResult::Error(msg)),
        };
        let (path, label_of_source, track) = match (args.track, &args.path) {
            (Some(track), None) => {
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
                (path, format!("track {track}"), Some(track))
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
                (path, name, None)
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

        // 3. The audio, at the 16 kHz mono the networks are defined for.
        let decoded = match audio_decoder::decode_file(&path) {
            Ok(d) => d,
            Err(e) => return Ok(ToolResult::Error(format!("decode failed: {e}"))),
        };
        let frames = decoded.samples.len() / usize::from(decoded.channels.max(1));
        let duration_sec = frames as f64 / f64::from(decoded.sample_rate.max(1));
        let mono = match ml_whisper::resample_to_16khz_mono(
            &decoded.samples,
            decoded.sample_rate,
            decoded.channels,
        ) {
            Ok(m) => m,
            Err(e) => return Ok(ToolResult::Error(format!("resampling failed: {e}"))),
        };

        // 4. The cache: the audio, the model, the options.
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
            ContentHash::from_bytes(
                format!(
                    "diarize-format:{DIARIZE_FORMAT};num:{};min:{};max:{}",
                    args.num_speakers.unwrap_or(0),
                    args.min_speakers.unwrap_or(0),
                    args.max_speakers.unwrap_or(0)
                )
                .as_bytes(),
            ),
        ]);
        let mut ran = false;
        let mut failure: Option<DiarizeFailure> = None;
        let mut compute = || -> ml_pipeline::Result<Heard> {
            ran = true;
            // Clears a Cancel pressed before this run began.
            crate::progress::begin();
            let strip = Strip::new(label_of_source.clone());
            let result = self.model.diarize(
                &model,
                &mono,
                bounds,
                &|stage, done, total| strip.step(stage, done, total),
                &|| crate::progress::cancelled(),
            );
            match result {
                Ok(heard) => Ok(heard),
                Err(e) => {
                    strip.cancelled.set(e == DiarizeFailure::Cancelled);
                    failure = Some(e);
                    Err(ml_pipeline::Error::Cache("no diarization".to_string()))
                }
            }
        };
        let outcome = match InferenceCache::open(ctx.store.project_dir()) {
            Ok(cache) => cache.get_or_compute(key, compute),
            Err(e) => {
                tracing::warn!(error = %e, "the inference cache could not be opened; not caching");
                compute()
            }
        };
        let heard = match outcome {
            Ok(h) => h,
            Err(e) => {
                return Ok(ToolResult::Error(match failure {
                    Some(f) => answer(&f),
                    None => format!("diarization failed: {e}"),
                }))
            }
        };
        let from_cache = !ran;

        // 5. The answer.
        let what = match track {
            Some(t) => format!("track {t}"),
            None => label_of_source,
        };
        if heard.turns.is_empty() {
            return Ok(ToolResult::Ok(json!({
                "speakers": 0,
                "turn_count": 0,
                "duration_sec": round2(duration_sec),
                "cached": from_cache,
                "summary": format!(
                    "No speech found in {what}; nothing to split. There is nothing to retry: the \
                     audio has no voices the model could tell apart."
                ),
            })));
        }

        let mut talk = vec![0.0_f64; heard.speakers];
        for t in &heard.turns {
            if let Some(slot) = talk.get_mut(t.speaker) {
                *slot += (t.end_s - t.start_s).max(0.0);
            }
        }
        let talk_json: Vec<Value> = talk
            .iter()
            .enumerate()
            .map(|(i, s)| json!({ "speaker": label(i), "talk_sec": round2(*s) }))
            .collect();

        // As many turns as fit the budget, in time order.
        let mut shown: Vec<Value> = Vec::new();
        let mut used = 2;
        for t in &heard.turns {
            let item = json!([round2(t.start_s), round2(t.end_s), label(t.speaker)]);
            let size = item.to_string().len() + 1;
            if !shown.is_empty() && used + size > RESULT_BUDGET_BYTES {
                break;
            }
            used += size;
            shown.push(item);
        }
        let truncated = shown.len() < heard.turns.len();

        let mut summary = format!(
            "Found {} speaker{} in {what} ({} turn{}{}). Turn times are {}.",
            heard.speakers,
            if heard.speakers == 1 { "" } else { "s" },
            heard.turns.len(),
            if heard.turns.len() == 1 { "" } else { "s" },
            if from_cache {
                ", from the cache: this audio was diarized before"
            } else {
                ""
            },
            if track.is_some() {
                "session seconds"
            } else {
                "the file's own seconds"
            },
        );
        summary.push_str(
            " Nothing in the session was changed. To give each voice its own track, call \
             split_by_speaker with these turns as segments {start_sec, end_sec, speaker}; the \
             names can be changed there. Overlapping speech goes to one speaker, and the user \
             can drag the boundaries afterwards.",
        );
        if truncated {
            summary.push_str(&format!(
                " Only the first {} of {} turns are listed, to keep this reply short, so do not \
                 split from this list: diarize a shorter stretch (a clip, or a file cut to the \
                 part wanted).",
                shown.len(),
                heard.turns.len()
            ));
        }

        Ok(ToolResult::Ok(json!({
            "speakers": heard.speakers,
            "turn_count": heard.turns.len(),
            "duration_sec": round2(duration_sec),
            "cached": from_cache,
            "talk_time": talk_json,
            "turns": shown,
            "truncated": truncated,
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
            DiarizeFailure::ModelNotDownloaded,
            DiarizeFailure::ModelDamaged,
            DiarizeFailure::ModelPathInvalid,
            DiarizeFailure::RuntimeUnavailable("x".into()),
            DiarizeFailure::BadSpeakerCount("x".into()),
            DiarizeFailure::Cancelled,
            DiarizeFailure::Failed("x".into()),
        ] {
            let text = answer(&f);
            for m in crate::tool::transcribe::MARKERS {
                assert!(!text.contains(m), "{f:?}: {text}");
            }
        }
    }

    #[test]
    fn labels_count_from_one() {
        assert_eq!(label(0), "speaker_1");
        assert_eq!(label(2), "speaker_3");
    }
}
