//! `transcribe` tool: speech to text on this computer, attached to the session
//! as a transcript (#384).
//!
//! The words are made by the pinned Whisper export (`ml_whisper`), and the tool
//! is the glue around it:
//!
//! 1. **Which audio.** `track` (an index into the session: the track as it
//!    sits on the timeline, so word times are session seconds and line up with
//!    `cut_words` and `remove_fillers`) or `path` (a file that is not in the
//!    session; its times are the file's). As `analyze_track` takes them.
//! 2. **Which model, never fetched.** [`SpeechToText::locate`]: the ONNX
//!    Runtime library, then `WHISPER_MODEL_PATH` (a developer's folder), else the
//!    speech model the user downloaded in Settings → Account
//!    (`ml_pipeline::models::cached`). The tool never downloads anything: only
//!    the user's click does (`crates/ml-pipeline/tests/only_the_user_downloads.rs`),
//!    and this file names none of what downloads.
//! 3. **The cache.** The words are kept in the project's inference cache under
//!    the audio's content hash combined with the model's identity and the
//!    options, so a second request for the same audio (a retry, the same clip on
//!    another track, an off-lock re-run that lost a race) costs a hash.
//! 4. **Progress and cancel.** One `kind: "transcribe"` progress event per 30 s
//!    window, and the cancel switch the window's Cancel button sets, read
//!    before every window and every decoder step.
//! 5. **A node.** One new session node whose `state.transcript` is the words and
//!    where they came from ([`session::TranscriptSource`]), so an edit by words
//!    cuts that track's audio and no other.
//!
//! ## Off the store lock
//!
//! A minute of CPU must not freeze the window, so the tool runs off the store
//! lock (`runs_off_the_lock`, #421): it reads the head, reads one track's audio
//! or one file, and appends one node. If the user edits meanwhile the call is
//! run again on the new head. The second run finds the same audio, hits the
//! cache the first run filled, and reports nothing, so progress is not shown
//! twice. If the edit changed the clips of the transcribed track the re-run is
//! refused (the `track` argument is what the guard reads). See the `ALLOWED`
//! list in `tests/off_lock_tools.rs` for the three `progress` spellings this
//! file is allowed and why.
//!
//! ## What a failure says
//!
//! Four problems are the user's to fix, and each answer starts with a marker
//! ([`MARKER_MODEL_NOT_DOWNLOADED`] and the others), one stable code the
//! window turns into a card (`ai::ToolView::SpeechSetup`). Each says what the
//! user does and that the agent does nothing itself: an agent that is told
//! "missing" tries things. No answer carries a local path or a loader's reason
//! (the reason is logged): tool results go to the model provider.
//!
//! ## Testing without a model
//!
//! [`TranscribeTool::with`] takes any [`SpeechToText`], so CI runs the whole
//! tool (cache, progress, cancel, errors, the off-lock re-run) against a fake.
//! [`WhisperSpeech`] is the real one; `tests/transcribe_end_to_end.rs` runs it
//! on the real model when one is given.

use std::cell::Cell;
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use ml_pipeline::{ContentHash, InferenceCache};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use session::{Transcript, TranscriptSource, TranscriptWord};

use crate::tool::util::{
    append_state, check_track_index, flattened_track_wav, lane_audio_path, load_head_state,
};
use crate::{Tool, ToolContext, ToolResult};

// ---------------------------------------------------------------------
// The markers
// ---------------------------------------------------------------------

/// The speech model is not on this computer.
pub const MARKER_MODEL_NOT_DOWNLOADED: &str = "[speech_model_not_downloaded]";
/// There is no usable ONNX Runtime library.
pub const MARKER_RUNTIME_UNAVAILABLE: &str = "[onnx_runtime_unavailable]";
/// The downloaded speech model will not load.
pub const MARKER_MODEL_DAMAGED: &str = "[speech_model_damaged]";
/// `WHISPER_MODEL_PATH` names something that is not the speech model.
pub const MARKER_MODEL_PATH_INVALID: &str = "[speech_model_path_invalid]";

/// Every marker, for the code that must handle each one.
pub const MARKERS: [&str; 4] = [
    MARKER_MODEL_NOT_DOWNLOADED,
    MARKER_RUNTIME_UNAVAILABLE,
    MARKER_MODEL_DAMAGED,
    MARKER_MODEL_PATH_INVALID,
];

// ---------------------------------------------------------------------
// The seam
// ---------------------------------------------------------------------

/// Why a transcription could not be made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpeechError {
    /// No `WHISPER_MODEL_PATH`, and the user has not downloaded the model.
    ModelNotDownloaded,
    /// The downloaded model is on disk and verified, and still will not load.
    ModelDamaged,
    /// `WHISPER_MODEL_PATH` is set to something that is not the model folder.
    ModelPathInvalid,
    /// No usable ONNX Runtime. The reason is short and carries no path.
    RuntimeUnavailable(String),
    /// A language code the model has no token for.
    UnknownLanguage(String),
    /// The user pressed Cancel.
    Cancelled,
    /// Anything else the model run reported.
    Failed(String),
}

/// A model found and identified, not yet loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelRef {
    /// The folder holding the export.
    pub folder: PathBuf,
    /// What the cache key folds in: changes when the model does. Never a path.
    pub fingerprint: String,
    pub source: ModelSource,
    /// Recorded in the node's provenance (`provenance::model_inputs`).
    pub model_id: String,
    pub model_version: String,
}

/// Where a [`ModelRef`] came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelSource {
    /// The speech model the user downloaded in Settings.
    Downloaded,
    /// A developer's folder, `WHISPER_MODEL_PATH`.
    Override,
}

/// Audio as decoded: interleaved samples at their own rate.
#[derive(Debug, Clone, Copy)]
pub struct Audio<'a> {
    pub samples: &'a [f32],
    pub sample_rate: u32,
    pub channels: u16,
}

/// What a transcription made. This is also the cache's value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Spoken {
    /// In order, with times in seconds from the start of the audio.
    pub words: Vec<TranscriptWord>,
    /// The language code the model used or detected, if any window had speech.
    pub language: Option<String>,
    /// How many 30 s windows were decoded.
    pub windows: usize,
}

/// Speech to text, as the tool needs it. [`WhisperSpeech`] is the real one;
/// tests supply their own.
pub trait SpeechToText: Send + Sync {
    /// Find the model and say what it is, or why it cannot be used. Cheap
    /// enough to call on every request, and it loads nothing.
    fn locate(&self) -> Result<ModelRef, SpeechError>;

    /// Transcribe `audio`. `language` is a normalised ISO 639-1 code or `None`
    /// to detect. `on_window(index, total)` is called before each 30 s window;
    /// `cancel` answers `true` once the user has asked to stop, and the run
    /// then ends with [`SpeechError::Cancelled`].
    fn transcribe(
        &self,
        model: &ModelRef,
        audio: Audio<'_>,
        language: Option<&str>,
        on_window: &dyn Fn(usize, usize),
        cancel: &dyn Fn() -> bool,
    ) -> Result<Spoken, SpeechError>;
}

// ---------------------------------------------------------------------
// Finding the model
// ---------------------------------------------------------------------

/// The variable a developer sets to use a folder of their own.
const MODEL_PATH_VAR: &str = "WHISPER_MODEL_PATH";

/// Find the model without fetching anything: `env` (the value of
/// `WHISPER_MODEL_PATH`) first, else `downloaded()` (the folder of the model
/// the user downloaded, if it is all there and verified).
///
/// A variable that is set wins even when it is wrong: the developer asked for
/// that folder, and quietly using another would hide the mistake.
pub fn resolve_model(
    env: Option<OsString>,
    downloaded: impl FnOnce() -> Option<PathBuf>,
) -> Result<ModelRef, SpeechError> {
    let pinned = ml_pipeline::models::whisper_base();
    if let Some(value) = env.filter(|v| !v.is_empty()) {
        let folder = PathBuf::from(value);
        if !folder.is_dir() {
            return Err(SpeechError::ModelPathInvalid);
        }
        // A developer's folder has no manifest to vouch for it, so it is told
        // apart by what the three files look like: name, size, modified time.
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
        let name = folder
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();
        return Ok(ModelRef {
            folder,
            fingerprint: ContentHash::from_bytes(identity.as_bytes()).to_hex(),
            source: ModelSource::Override,
            model_id: "whisper".to_string(),
            // The folder's name, not its path: provenance is shareable (#162).
            model_version: name,
        });
    }

    let folder = downloaded().ok_or(SpeechError::ModelNotDownloaded)?;
    // The pinned hashes are what `cached` just verified the files against.
    let mut identity = String::from("pinned");
    for file in &pinned.files {
        identity.push_str(&format!("|{}:{}", file.path, file.sha256));
    }
    let revision = ml_pipeline::Catalog::built_in()
        .get(ml_pipeline::models::WHISPER_BASE_ID)
        .map(|m| m.revision.clone())
        .unwrap_or_default();
    Ok(ModelRef {
        folder,
        fingerprint: ContentHash::from_bytes(identity.as_bytes()).to_hex(),
        source: ModelSource::Downloaded,
        model_id: ml_pipeline::models::WHISPER_BASE_ID.to_string(),
        model_version: revision,
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

/// The real thing: the pinned Whisper export on ONNX Runtime, on the CPU.
#[derive(Debug, Default, Clone, Copy)]
pub struct WhisperSpeech;

/// The loaded model, kept for the next call: `(key, model)`. One slot: a
/// folder or a fingerprint that changes replaces it. (The sessions behind an
/// old model stay in `ml_pipeline`'s registry; #658.)
static LOADED: Mutex<Option<(String, Arc<ml_whisper::WhisperModel>)>> = Mutex::new(None);

impl WhisperSpeech {
    fn load(model: &ModelRef) -> Result<Arc<ml_whisper::WhisperModel>, SpeechError> {
        use ml_whisper::WhisperError as W;
        let key = format!("{}|{}", model.folder.display(), model.fingerprint);
        let mut slot = LOADED.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((held, loaded)) = slot.as_ref() {
            if *held == key {
                return Ok(Arc::clone(loaded));
            }
        }
        match ml_whisper::WhisperModel::load(&model.folder) {
            Ok(loaded) => {
                let loaded = Arc::new(loaded);
                *slot = Some((key, Arc::clone(&loaded)));
                Ok(loaded)
            }
            Err(W::RuntimeUnavailable(reason)) => {
                tracing::warn!(%reason, "the ONNX Runtime library could not be used");
                Err(SpeechError::RuntimeUnavailable(
                    "the ONNX Runtime library could not be loaded".to_string(),
                ))
            }
            Err(other) => {
                // The reason names files and paths, and the agent acts on
                // whatever it is given: it is logged, not handed on.
                tracing::warn!(error = %other, "the speech model could not be loaded");
                Err(match model.source {
                    ModelSource::Downloaded => SpeechError::ModelDamaged,
                    ModelSource::Override => SpeechError::ModelPathInvalid,
                })
            }
        }
    }
}

impl SpeechToText for WhisperSpeech {
    fn locate(&self) -> Result<ModelRef, SpeechError> {
        // The runtime first: a build that has none cannot run any model, and
        // sending its user to download 139 MB first is the worse trap.
        ml_pipeline::runtime::ensure()
            .map_err(|e| SpeechError::RuntimeUnavailable(runtime_reason(&e)))?;
        resolve_model(std::env::var_os(MODEL_PATH_VAR), || {
            ml_pipeline::models::cached(ml_pipeline::models::WHISPER_BASE_ID)
        })
    }

    fn transcribe(
        &self,
        model: &ModelRef,
        audio: Audio<'_>,
        language: Option<&str>,
        on_window: &dyn Fn(usize, usize),
        cancel: &dyn Fn() -> bool,
    ) -> Result<Spoken, SpeechError> {
        use ml_whisper::WhisperError as W;
        let loaded = Self::load(model)?;
        let done = ml_whisper::transcribe_observed(
            &loaded,
            audio.samples,
            audio.sample_rate,
            audio.channels,
            language,
            cancel,
            on_window,
        );
        match done {
            Ok(t) => Ok(Spoken {
                words: t
                    .words
                    .iter()
                    .map(|w| TranscriptWord {
                        text: w.text.clone(),
                        start_s: w.start_s,
                        end_s: w.end_s,
                        confidence: w.confidence,
                    })
                    .collect(),
                language: t.language,
                windows: t.windows,
            }),
            Err(W::Cancelled) => Err(SpeechError::Cancelled),
            Err(W::UnknownLanguage(code)) => Err(SpeechError::UnknownLanguage(code)),
            Err(other) => {
                tracing::warn!(error = %other, "transcription failed");
                Err(SpeechError::Failed(other.to_string()))
            }
        }
    }
}

// ---------------------------------------------------------------------
// What the model is told
// ---------------------------------------------------------------------

/// The size of the speech model, for the sentence that sends the user to
/// download it. From the catalog, so it cannot drift from the pin.
fn model_megabytes() -> u64 {
    ml_pipeline::Catalog::built_in()
        .get(ml_pipeline::models::WHISPER_BASE_ID)
        .map_or(0, |m| (m.size() + 500_000) / 1_000_000)
}

/// The answer for a transcription that could not be made: the text of the
/// `ToolResult::Error`. A setup problem starts with its marker.
pub fn answer(error: &SpeechError) -> String {
    match error {
        SpeechError::ModelNotDownloaded => format!(
            "{MARKER_MODEL_NOT_DOWNLOADED} The speech model is not on this computer. Tell the \
             user to open Settings → Account and press Download beside the speech model (about \
             {} MB); only they can start it. Then ask them to say when it has finished, and call \
             transcribe again. Do not try to download, install or configure anything yourself.",
            model_megabytes()
        ),
        SpeechError::RuntimeUnavailable(reason) => format!(
            "{MARKER_RUNTIME_UNAVAILABLE} Transcription needs the ONNX Runtime library, and \
             {reason}. The release installers include it; a build from source does not. Tell \
             the user: for a build from source, set ORT_DYLIB_PATH to a copy of ONNX Runtime \
             1.23.2 (docs/development-guide.md, \"ML Model Files\"). Do not try to install \
             anything yourself."
        ),
        SpeechError::ModelDamaged => format!(
            "{MARKER_MODEL_DAMAGED} The speech model is on this computer but could not be \
             loaded, so the download is damaged. Tell the user to open Settings → Account, press \
             Remove beside the speech model, and then Download it again. Do not try to repair or \
             replace files yourself."
        ),
        SpeechError::ModelPathInvalid => format!(
            "{MARKER_MODEL_PATH_INVALID} {MODEL_PATH_VAR} is set, and what it names is not the \
             speech model: the folder has to hold onnx/encoder_model.onnx, \
             onnx/decoder_model_merged_int8.onnx and tokenizer.json. Tell the user to correct it \
             or to unset it, and the model they downloaded in Settings is used instead."
        ),
        SpeechError::UnknownLanguage(code) => format!(
            "unknown language {code:?}: the model has no such language. Use a two-letter ISO \
             639-1 code (en, es, fr, de, it, pt, ja, zh), or leave `language` out and it is \
             detected."
        ),
        SpeechError::Cancelled => "Transcription was cancelled by the user. Nothing was changed. \
             Do not run it again unless they ask."
            .to_string(),
        SpeechError::Failed(why) => format!("transcription failed: {why}"),
    }
}

// ---------------------------------------------------------------------
// The tool
// ---------------------------------------------------------------------

/// Bumped when a change to the decoder, the aligner or the windowing would
/// give different words for the same audio and model, so that an entry written
/// before it is not read as an answer to the question asked after it.
const TRANSCRIPT_FORMAT: u32 = 2;

/// The most the words in a result may take, in bytes of JSON. The result goes
/// to the model provider (and into the model's context for the rest of the
/// conversation), and a ten-minute talk is about 1,500 words.
pub const RESULT_BUDGET_BYTES: usize = 6_000;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    #[serde(default)]
    track: Option<usize>,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    language: Option<String>,
}

pub struct TranscribeTool {
    stt: Arc<dyn SpeechToText>,
}

impl TranscribeTool {
    /// The tool with the real model.
    pub fn new() -> Self {
        Self::with(Arc::new(WhisperSpeech))
    }

    /// The tool with any recogniser: what the tests use.
    pub fn with(stt: Arc<dyn SpeechToText>) -> Self {
        Self { stt }
    }
}

impl Default for TranscribeTool {
    fn default() -> Self {
        Self::new()
    }
}

/// What the progress strip is told while a transcription runs, and the
/// closing `done` that clears it. Nothing is said, and nothing cleared, for a
/// call that answered from the cache.
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

    fn window(&self, index: usize, total: usize) {
        self.started.set(true);
        self.total.set(total);
        crate::progress::report(json!({
            "kind": "transcribe",
            "index": index,
            "total": total,
            "file": self.file,
        }));
    }
}

impl Drop for Strip {
    /// However the run ends (words, an error, a cancel, a panic), a strip that
    /// was shown is cleared.
    fn drop(&mut self) {
        if self.started.get() {
            crate::progress::report(json!({
                "kind": "transcribe",
                "done": true,
                "cancelled": self.cancelled.get(),
                "total": self.total.get(),
                "file": self.file,
            }));
        }
    }
}

/// The audio to transcribe and what to call it.
struct Source {
    path: PathBuf,
    /// "track 1", or the file's name: the progress strip's label.
    label: String,
    /// Where the words come from, for the transcript.
    origin: TranscriptSource,
    /// The track index, in track mode.
    track: Option<usize>,
}

impl Tool for TranscribeTool {
    fn name(&self) -> &'static str {
        "transcribe"
    }

    fn schema(&self) -> Value {
        crate::schema::anthropic_tool(
            "transcribe",
            "Transcribe speech to text on this computer and attach the words, with times, to the \
             session as a new version, replacing the earlier transcript that cut_words, \
             remove_fillers and select_region's `text` work from. Give `track` (word times are \
             then session seconds) or `path` for a file that is not in the session (the file's \
             times). The audio never leaves the computer. It needs the speech model the user \
             downloads once in Settings → Account; if the call returns a message starting with a \
             bracketed code, tell the user what it says and stop, and do not try to download, \
             install or configure anything. Runs on the CPU, usually in under half the audio's \
             length; the user can cancel. Returns [text, start_sec, end_sec] per word (the \
             position is the word number cut_words takes), or for long audio sentence segments \
             with word numbers.",
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
                    "language": {
                        "type": "string",
                        "description": "ISO 639-1 code of the language spoken (en, de, fr, es, ...). Leave out to detect it."
                    }
                },
                "additionalProperties": false
            }),
        )
    }

    // The model run takes a minute for a few minutes of speech, and holds the
    // store's lock for none of it (#421). The module docs say why that is safe,
    // and `tests/off_lock_tools.rs` why `progress` is allowed here.
    fn runs_off_the_lock(&self) -> bool {
        true
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };
        let language = args
            .language
            .as_deref()
            .map(|l| l.trim().to_ascii_lowercase())
            .filter(|l| !l.is_empty());

        // 1. Which audio. Cheap, and its mistakes are the caller's to fix.
        let state = match load_head_state(ctx) {
            Ok(s) => s,
            Err(msg) => return Ok(ToolResult::Error(msg)),
        };
        let source = match (args.track, &args.path) {
            (Some(track), None) => {
                if let Err(e) = check_track_index(&state.tracks, track) {
                    return Ok(ToolResult::Error(e));
                }
                let clips = &state.tracks[track].clips;
                let path = match lane_audio_path(ctx.store.project_dir(), clips) {
                    Some(p) => p,
                    // Say why, rather than that there is "no audio".
                    None => match flattened_track_wav(ctx.store.project_dir(), clips) {
                        Ok(p) => p,
                        Err(e) => return Ok(ToolResult::Error(format!("track {track}: {e}"))),
                    },
                };
                Source {
                    path,
                    label: format!("track {track}"),
                    origin: TranscriptSource::Track {
                        track: state.tracks[track].id.clone(),
                    },
                    track: Some(track),
                }
            }
            (None, Some(path)) => {
                let path = PathBuf::from(path);
                if !path.exists() {
                    return Ok(ToolResult::Error(format!(
                        "file not found: {}",
                        path.display()
                    )));
                }
                let label = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("audio")
                    .to_string();
                Source {
                    path,
                    label,
                    origin: TranscriptSource::File,
                    track: None,
                }
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
        let model = match self.stt.locate() {
            Ok(m) => m,
            Err(e) => return Ok(ToolResult::Error(answer(&e))),
        };

        // 3. The audio.
        let decoded = match audio_decoder::decode_file(&source.path) {
            Ok(d) => d,
            Err(e) => return Ok(ToolResult::Error(format!("decode failed: {e}"))),
        };
        let audio = Audio {
            samples: &decoded.samples,
            sample_rate: decoded.sample_rate,
            channels: decoded.channels,
        };
        let frames = decoded.samples.len() / usize::from(decoded.channels.max(1));
        let duration_sec = frames as f64 / f64::from(decoded.sample_rate.max(1));

        // 4. The cache: the audio, the model, the options.
        let key = ContentHash::combine(&[
            ContentHash::from_bytes(
                crate::provenance::audio_hash(audio.samples, audio.sample_rate, audio.channels)
                    .as_bytes(),
            ),
            ContentHash::from_bytes(model.fingerprint.as_bytes()),
            ContentHash::from_bytes(
                format!(
                    "transcript-format:{TRANSCRIPT_FORMAT};language:{}",
                    language.as_deref().unwrap_or("auto")
                )
                .as_bytes(),
            ),
        ]);
        let mut ran = false;
        let mut failure: Option<SpeechError> = None;
        let mut compute = || -> ml_pipeline::Result<Spoken> {
            ran = true;
            // Clears a Cancel pressed before this run began, so it cannot stop
            // this one.
            crate::progress::begin();
            let strip = Strip::new(source.label.clone());
            let result = self.stt.transcribe(
                &model,
                audio,
                language.as_deref(),
                &|index, total| strip.window(index, total),
                &|| crate::progress::cancelled(),
            );
            match result {
                Ok(spoken) => Ok(spoken),
                Err(e) => {
                    strip.cancelled.set(e == SpeechError::Cancelled);
                    failure = Some(e);
                    Err(ml_pipeline::Error::Cache("no transcript".to_string()))
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
        let spoken = match outcome {
            Ok(spoken) => spoken,
            Err(e) => {
                return Ok(ToolResult::Error(match failure {
                    Some(f) => answer(&f),
                    None => format!("transcription failed: {e}"),
                }))
            }
        };
        let from_cache = !ran;

        // 5. Nothing heard is an answer, not a node: the session is unchanged.
        let what = match source.track {
            Some(t) => format!("track {t}"),
            None => source.label.clone(),
        };
        if spoken.words.is_empty() {
            return Ok(ToolResult::Ok(json!({
                "word_count": 0,
                "duration_sec": round2(duration_sec),
                "cached": from_cache,
                "summary": format!(
                    "No speech found in {what}; the session is unchanged. There is nothing to \
                     retry: the audio has no speech the model could write down."
                ),
            })));
        }

        // 6. The node.
        let replaced = state
            .transcript
            .as_ref()
            .is_some_and(|t| !t.words.is_empty());
        let mut next = state;
        next.transcript = Some(Transcript {
            words: spoken.words.clone(),
            source: Some(source.origin.clone()),
        });
        let new_id = match append_state(
            ctx,
            next,
            format!("transcribe {what} ({} words)", spoken.words.len()),
        ) {
            Ok(id) => id,
            Err(msg) => return Ok(ToolResult::Error(msg)),
        };

        // Which model made it (#163). Model weights are not part of the
        // session, so this stays non-replayable; what the record buys is
        // diagnosis: a transcript that looks wrong can be traced to its model.
        let mut params = json!({});
        match (source.track, &args.path) {
            (Some(t), _) => params["track"] = json!(t),
            (None, Some(p)) => params["path"] = json!(p),
            _ => {}
        }
        if let Some(l) = &language {
            params["language"] = json!(l);
        }
        let op = session::NodeOp::new(
            "transcribe".to_string(),
            params,
            env!("CARGO_PKG_VERSION").to_string(),
        )
        .not_reproducible()
        .with_inputs(crate::provenance::model_inputs(
            &model.model_id,
            &model.model_version,
        ));
        if let Err(e) = ctx.store.set_op(new_id, op) {
            tracing::warn!(error = %e, "failed to record transcribe provenance");
        }

        let mut out = json!({
            "node_id": new_id.to_hex(),
            "word_count": spoken.words.len(),
            "language": spoken.language,
            "duration_sec": round2(duration_sec),
            "cached": from_cache,
            "replaced_earlier_transcript": replaced,
        });
        let truncated = listing(&spoken.words, &mut out);
        out["summary"] = json!(summary(
            &what,
            source.track,
            &spoken,
            from_cache,
            replaced,
            truncated,
            &new_id.to_hex()
        ));
        Ok(ToolResult::Ok(out))
    }
}

// ---------------------------------------------------------------------
// The result
// ---------------------------------------------------------------------

fn round2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

/// A gap between two words at least this long ends a segment.
const SEGMENT_GAP_S: f32 = 0.8;
/// A segment is cut here at the latest, so one run-on sentence is not one entry.
const SEGMENT_MAX_WORDS: usize = 40;

/// Put the words in `out`, within [`RESULT_BUDGET_BYTES`]: as `words`
/// (`[text, start_sec, end_sec]`, the position being the word number) if they
/// fit, else as `segments`, sentence-sized runs with their word numbers, for as
/// many as fit. Returns whether segments were cut off.
fn listing(words: &[TranscriptWord], out: &mut Value) -> bool {
    let triples: Vec<Value> = words
        .iter()
        .map(|w| {
            json!([
                w.text,
                round2(f64::from(w.start_s)),
                round2(f64::from(w.end_s))
            ])
        })
        .collect();
    if serde_json::to_string(&triples).map_or(usize::MAX, |s| s.len()) <= RESULT_BUDGET_BYTES {
        out["words"] = Value::Array(triples);
        return false;
    }

    let segments = segments_of(words);
    let total = segments.len();
    let mut shown: Vec<Value> = Vec::new();
    let mut used = 2;
    for segment in segments {
        let size = serde_json::to_string(&segment).map_or(usize::MAX, |s| s.len()) + 1;
        if !shown.is_empty() && used + size > RESULT_BUDGET_BYTES {
            break;
        }
        used += size;
        shown.push(segment);
    }
    let truncated = shown.len() < total;
    out["segments_shown"] = json!(shown.len());
    out["segments"] = Value::Array(shown);
    out["segments_total"] = json!(total);
    out["truncated"] = json!(truncated);
    truncated
}

/// Sentence-sized runs of `words`: `from_word` inclusive, `to_word` exclusive.
fn segments_of(words: &[TranscriptWord]) -> Vec<Value> {
    let mut out = Vec::new();
    let mut from = 0;
    for (i, w) in words.iter().enumerate() {
        let last = i + 1 == words.len();
        let ends_sentence = w
            .text
            .trim_end_matches(['"', '\'', ')', '”', '’'])
            .ends_with(['.', '?', '!', '…']);
        let pause = words
            .get(i + 1)
            .is_some_and(|next| next.start_s - w.end_s >= SEGMENT_GAP_S);
        let long = i + 1 - from >= SEGMENT_MAX_WORDS;
        if last || ends_sentence || pause || long {
            let run = &words[from..=i];
            out.push(json!({
                "from_word": from,
                "to_word": i + 1,
                "start_sec": round2(f64::from(run[0].start_s)),
                "end_sec": round2(f64::from(w.end_s)),
                "text": run.iter().map(|r| r.text.as_str()).collect::<Vec<_>>().join(" "),
            }));
            from = i + 1;
        }
    }
    out
}

fn summary(
    what: &str,
    track: Option<usize>,
    spoken: &Spoken,
    from_cache: bool,
    replaced: bool,
    truncated: bool,
    node: &str,
) -> String {
    let language = spoken
        .language
        .as_deref()
        .map(|l| format!(", {l}"))
        .unwrap_or_default();
    let mut s = format!(
        "Transcribed {what} ({} words{language}{}); new head {node}.",
        spoken.words.len(),
        if from_cache {
            ", from the cache: this audio was transcribed before"
        } else {
            ""
        }
    );
    if replaced {
        s.push_str(" It replaced the session's earlier transcript.");
    }
    match track {
        Some(t) => s.push_str(&format!(
            " Word times are session seconds. Pass track {t} to cut_words and remove_fillers: \
             from_word is inclusive and to_word exclusive, and select_region finds a phrase."
        )),
        None => s.push_str(
            " Word times are the file's own, not the session timeline's: pass `track` to \
             cut_words and remove_fillers to say which track they line up with.",
        ),
    }
    if truncated {
        s.push_str(
            " The listing is cut off to keep this reply short; the whole transcript is in the \
             session (select_region with `text` finds any phrase).",
        );
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(text: &str, start: f32, end: f32) -> TranscriptWord {
        TranscriptWord {
            text: text.to_string(),
            start_s: start,
            end_s: end,
            confidence: 0.9,
        }
    }

    // --- the marker for each setup problem -----------------------------

    fn every_error() -> Vec<SpeechError> {
        vec![
            SpeechError::ModelNotDownloaded,
            SpeechError::ModelDamaged,
            SpeechError::ModelPathInvalid,
            SpeechError::RuntimeUnavailable("no ONNX Runtime library was found".into()),
            SpeechError::UnknownLanguage("xx".into()),
            SpeechError::Cancelled,
            SpeechError::Failed("the decoder gave up".into()),
        ]
    }

    #[test]
    fn each_setup_problem_starts_with_its_own_marker_and_nothing_else_does() {
        let marked = [
            (SpeechError::ModelNotDownloaded, MARKER_MODEL_NOT_DOWNLOADED),
            (SpeechError::ModelDamaged, MARKER_MODEL_DAMAGED),
            (SpeechError::ModelPathInvalid, MARKER_MODEL_PATH_INVALID),
            (
                SpeechError::RuntimeUnavailable("x".into()),
                MARKER_RUNTIME_UNAVAILABLE,
            ),
        ];
        for (error, marker) in &marked {
            let text = answer(error);
            assert!(text.starts_with(marker), "{error:?}: {text}");
            for other in MARKERS.iter().filter(|m| *m != marker) {
                assert!(!text.contains(other), "{error:?} also carries {other}");
            }
        }
        for error in every_error() {
            if marked
                .iter()
                .any(|(e, _)| std::mem::discriminant(e) == std::mem::discriminant(&error))
            {
                continue;
            }
            let text = answer(&error);
            assert!(
                MARKERS.iter().all(|m| !text.contains(m)),
                "{error:?} is the agent's or the user's to handle, not a setup card: {text}"
            );
        }
        let mut sorted = MARKERS.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), MARKERS.len(), "one marker per problem");
    }

    #[test]
    fn the_answers_tell_the_user_what_to_do_and_the_agent_to_do_nothing() {
        let text = answer(&SpeechError::ModelNotDownloaded);
        assert!(text.contains("Settings → Account"), "{text}");
        assert!(text.contains("press Download"), "{text}");
        assert!(text.contains("Do not try to download"), "{text}");
        // The variable is a developer's, not a step for the agent to suggest.
        assert!(!text.contains(MODEL_PATH_VAR), "{text}");
        assert!(text.contains("139 MB"), "the size is the pin's: {text}");

        let text = answer(&SpeechError::ModelDamaged);
        assert!(
            text.contains("Remove") && text.contains("Download it again"),
            "{text}"
        );

        let text = answer(&SpeechError::RuntimeUnavailable(
            "no ONNX Runtime library was found".into(),
        ));
        assert!(text.contains("ORT_DYLIB_PATH"), "{text}");
        assert!(text.contains("development-guide"), "{text}");

        let text = answer(&SpeechError::ModelPathInvalid);
        assert!(text.contains(MODEL_PATH_VAR), "{text}");
    }

    #[test]
    fn no_answer_carries_a_path_or_says_the_feature_is_missing() {
        for error in every_error() {
            let text = answer(&error);
            assert!(!text.contains("not implemented"), "{error:?}: {text}");
            assert!(
                !text.contains("/home/") && !text.contains("C:\\"),
                "{error:?}: {text}"
            );
        }
    }

    #[test]
    fn a_cancel_says_nothing_was_changed_and_not_to_retry() {
        let text = answer(&SpeechError::Cancelled);
        assert!(text.contains("Nothing was changed"), "{text}");
        assert!(text.contains("Do not run it again unless"), "{text}");
    }

    // --- finding the model ----------------------------------------------

    #[test]
    fn a_variable_that_is_set_wins_even_over_a_downloaded_model() {
        let dir = tempfile::tempdir().unwrap();
        let found = resolve_model(Some(dir.path().into()), || {
            panic!("the variable is set, so the downloaded model is not even looked for")
        })
        .unwrap();
        assert_eq!(found.source, ModelSource::Override);
        assert_eq!(found.folder, dir.path());
        assert_eq!(found.model_id, "whisper");
    }

    #[test]
    fn a_variable_that_is_not_a_folder_is_an_error_and_does_not_fall_through() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("model.onnx");
        std::fs::write(&file, b"x").unwrap();
        for value in [
            file.into_os_string(),
            dir.path().join("nope").into_os_string(),
        ] {
            let err =
                resolve_model(Some(value), || Some(PathBuf::from("/downloaded"))).unwrap_err();
            assert_eq!(err, SpeechError::ModelPathInvalid);
        }
    }

    #[test]
    fn an_empty_variable_is_unset() {
        let found = resolve_model(Some(OsString::new()), || Some(PathBuf::from("/d"))).unwrap();
        assert_eq!(found.source, ModelSource::Downloaded);
    }

    #[test]
    fn with_no_variable_the_downloaded_model_is_used_and_if_there_is_none_it_is_not_downloaded() {
        let found = resolve_model(None, || Some(PathBuf::from("/d"))).unwrap();
        assert_eq!(found.source, ModelSource::Downloaded);
        assert_eq!(found.folder, PathBuf::from("/d"));
        assert_eq!(found.model_id, ml_pipeline::models::WHISPER_BASE_ID);
        assert_eq!(found.model_version.len(), 40, "the pinned commit");
        assert_eq!(
            resolve_model(None, || None).unwrap_err(),
            SpeechError::ModelNotDownloaded
        );
    }

    #[test]
    fn a_downloaded_models_identity_is_the_pin_and_a_developers_folder_is_told_by_its_files() {
        let a = resolve_model(None, || Some(PathBuf::from("/d"))).unwrap();
        let b = resolve_model(None, || Some(PathBuf::from("/elsewhere"))).unwrap();
        assert_eq!(
            a.fingerprint, b.fingerprint,
            "the same pinned files are one model"
        );
        assert!(!a.fingerprint.contains('/'));

        let dir = tempfile::tempdir().unwrap();
        let first = resolve_model(Some(dir.path().into()), || None).unwrap();
        assert_ne!(first.fingerprint, a.fingerprint);
        // A file appearing in the folder, or changing size, is a new identity.
        let pinned = ml_pipeline::models::whisper_base();
        let encoder = dir.path().join(&pinned.files[0].path);
        std::fs::create_dir_all(encoder.parent().unwrap()).unwrap();
        std::fs::write(&encoder, b"one").unwrap();
        let second = resolve_model(Some(dir.path().into()), || None).unwrap();
        assert_ne!(first.fingerprint, second.fingerprint);
        std::fs::write(&encoder, b"three").unwrap();
        let third = resolve_model(Some(dir.path().into()), || None).unwrap();
        assert_ne!(second.fingerprint, third.fingerprint);
        // The version recorded in provenance is the folder's name, not its path.
        assert!(!third.model_version.contains('/'));
    }

    // --- the result -------------------------------------------------------

    #[test]
    fn a_short_transcript_is_listed_word_by_word() {
        let words = vec![word("Hello,", 0.3, 0.84), word("world.", 0.9, 1.41)];
        let mut out = json!({});
        let truncated = listing(&words, &mut out);
        assert!(!truncated);
        assert_eq!(
            out["words"],
            json!([["Hello,", 0.3, 0.84], ["world.", 0.9, 1.41]])
        );
        assert!(out.get("segments").is_none());
    }

    fn long_talk(n: usize) -> Vec<TranscriptWord> {
        (0..n)
            .map(|i| {
                let start = i as f32 * 0.5;
                let text = if i % 9 == 8 {
                    format!("word{i}.")
                } else {
                    format!("word{i}")
                };
                word(&text, start, start + 0.4)
            })
            .collect()
    }

    #[test]
    fn a_long_transcript_is_listed_as_segments_within_the_budget() {
        let words = long_talk(1_500);
        let mut out = json!({});
        let truncated = listing(&words, &mut out);
        assert!(truncated, "1,500 words do not fit");
        assert!(out.get("words").is_none());
        let bytes = serde_json::to_string(&out["segments"]).unwrap().len();
        assert!(bytes <= RESULT_BUDGET_BYTES, "{bytes} B");
        let shown = out["segments_shown"].as_u64().unwrap();
        assert!(shown > 10 && shown < out["segments_total"].as_u64().unwrap());
        assert_eq!(out["truncated"], json!(true));
        // A segment names the words it covers: from inclusive, to exclusive.
        let first = &out["segments"][0];
        assert_eq!(first["from_word"], json!(0));
        assert_eq!(
            first["to_word"],
            json!(9),
            "a sentence ends at the full stop"
        );
        assert_eq!(
            first["text"],
            json!("word0 word1 word2 word3 word4 word5 word6 word7 word8.")
        );
        let second = &out["segments"][1];
        assert_eq!(
            second["from_word"], first["to_word"],
            "no word is in two segments or none"
        );
    }

    #[test]
    fn a_transcript_that_fits_only_as_segments_is_not_cut_off() {
        // Short words with no full stops: forty to a segment, which names the
        // run once and so takes far less than a list of every word's times.
        let words: Vec<TranscriptWord> = (0..1_000)
            .map(|i| word("w", i as f32 * 0.3, i as f32 * 0.3 + 0.2))
            .collect();
        let mut out = json!({});
        let truncated = listing(&words, &mut out);
        assert!(
            out.get("words").is_none(),
            "1,000 words do not fit as a list"
        );
        assert!(!truncated, "25 segments are well inside the budget");
        assert_eq!(out["segments_shown"], out["segments_total"]);
        assert_eq!(out["truncated"], json!(false));
        let last = out["segments"].as_array().unwrap().last().unwrap().clone();
        assert_eq!(last["to_word"], json!(1_000));
    }

    #[test]
    fn a_pause_ends_a_segment_and_a_run_on_sentence_is_cut_at_forty_words() {
        let words = vec![
            word("one", 0.0, 0.3),
            word("two", 0.4, 0.7),
            word("three", 2.0, 2.3),
        ];
        let segments = segments_of(&words);
        assert_eq!(segments.len(), 2, "{segments:?}");
        assert_eq!(segments[0]["to_word"], json!(2));
        let words: Vec<TranscriptWord> = (0..95)
            .map(|i| word("w", i as f32 * 0.3, i as f32 * 0.3 + 0.2))
            .collect();
        let segments = segments_of(&words);
        let sizes: Vec<u64> = segments
            .iter()
            .map(|s| s["to_word"].as_u64().unwrap() - s["from_word"].as_u64().unwrap())
            .collect();
        assert_eq!(sizes, vec![40, 40, 15]);
    }

    #[test]
    fn the_summary_says_which_track_to_cut_and_what_a_file_transcript_is() {
        let spoken = Spoken {
            words: vec![word("hi", 0.0, 0.2)],
            language: Some("en".into()),
            windows: 1,
        };
        let by_track = summary("track 1", Some(1), &spoken, false, false, false, "abcd");
        assert!(
            by_track.contains("Pass track 1 to cut_words and remove_fillers"),
            "{by_track}"
        );
        assert!(by_track.contains("session seconds"), "{by_track}");
        assert!(!by_track.contains("replaced"), "{by_track}");
        let by_file = summary("a.wav", None, &spoken, true, true, true, "abcd");
        assert!(by_file.contains("the file's own"), "{by_file}");
        assert!(by_file.contains("from the cache"), "{by_file}");
        assert!(
            by_file.contains("replaced the session's earlier transcript"),
            "{by_file}"
        );
        assert!(by_file.contains("cut off"), "{by_file}");
    }
}
