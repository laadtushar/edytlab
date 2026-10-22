//! A recogniser that is not a model, and a project to point `transcribe` at.
//!
//! CI has no speech model and no ONNX Runtime, so the tool's own behaviour
//! (the cache, the progress, the cancel, the error answers, the re-run after
//! a lost race) is tested against [`Fake`] through `TranscribeTool::with`.
//! Each test binary that uses this includes it with `#[path]` and uses a
//! subset.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use session::{Store, TranscriptWord};
use tempfile::TempDir;
use tools::tool::transcribe::{
    Audio, ModelRef, ModelSource, SpeechError, SpeechToText, Spoken, TranscribeTool,
};
use tools::{ToolContext, ToolDispatcher, ToolResult};

pub const RATE: u32 = 16_000;

pub fn word(text: &str, start: f32, end: f32) -> TranscriptWord {
    TranscriptWord {
        text: text.to_string(),
        start_s: start,
        end_s: end,
        confidence: 0.9,
    }
}

/// What a test does at the start of a run, given the run's number.
pub type OnRun = Box<dyn FnMut(usize) + Send>;

/// A recogniser that says the same words every time and counts the runs.
pub struct Fake {
    pub runs: Arc<AtomicUsize>,
    pub words: Vec<TranscriptWord>,
    pub language: Option<String>,
    /// What `locate` answers, for the setup problems.
    pub model: Mutex<Result<ModelRef, SpeechError>>,
    /// What a run ends with instead of words.
    pub fails_with: Mutex<Option<SpeechError>>,
    /// The `language` each run was given.
    pub languages: Mutex<Vec<Option<String>>>,
    /// How many windows a run reports, asking `cancel` before each.
    pub windows: usize,
    /// Called at the start of each run, with the number of the run (1-based).
    /// A test plays the user here.
    pub on_run: Mutex<Option<OnRun>>,
    /// Press Cancel (the window's button) once this window has been reported.
    pub cancel_after: Mutex<Option<usize>>,
}

pub fn model_ref(fingerprint: &str) -> ModelRef {
    ModelRef {
        folder: PathBuf::from("/models/fake"),
        fingerprint: fingerprint.to_string(),
        source: ModelSource::Downloaded,
        model_id: "whisper-base".to_string(),
        model_version: "abc123".to_string(),
    }
}

impl Fake {
    pub fn new(words: Vec<TranscriptWord>) -> Self {
        Self {
            runs: Arc::new(AtomicUsize::new(0)),
            words,
            language: Some("en".to_string()),
            model: Mutex::new(Ok(model_ref("model-one"))),
            fails_with: Mutex::new(None),
            languages: Mutex::new(Vec::new()),
            windows: 1,
            on_run: Mutex::new(None),
            cancel_after: Mutex::new(None),
        }
    }

    pub fn runs(&self) -> usize {
        self.runs.load(Ordering::SeqCst)
    }

    pub fn use_model(&self, fingerprint: &str) {
        *self.model.lock().unwrap() = Ok(model_ref(fingerprint));
    }
}

impl SpeechToText for Fake {
    fn locate(&self) -> Result<ModelRef, SpeechError> {
        self.model.lock().unwrap().clone()
    }

    fn transcribe(
        &self,
        _model: &ModelRef,
        _audio: Audio<'_>,
        language: Option<&str>,
        on_window: &dyn Fn(usize, usize),
        cancel: &dyn Fn() -> bool,
    ) -> Result<Spoken, SpeechError> {
        let run = self.runs.fetch_add(1, Ordering::SeqCst) + 1;
        self.languages
            .lock()
            .unwrap()
            .push(language.map(str::to_string));
        if let Some(hook) = self.on_run.lock().unwrap().as_mut() {
            hook(run);
        }
        for index in 0..self.windows {
            if cancel() {
                return Err(SpeechError::Cancelled);
            }
            on_window(index, self.windows);
            if *self.cancel_after.lock().unwrap() == Some(index) {
                tools::progress::request_cancel();
            }
        }
        if let Some(e) = self.fails_with.lock().unwrap().clone() {
            return Err(e);
        }
        Ok(Spoken {
            words: self.words.clone(),
            language: self.language.clone(),
            windows: self.windows,
        })
    }
}

/// A mono 16-bit WAV at [`RATE`].
pub fn write_wav(path: &Path, samples: &[f32]) -> PathBuf {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(path, spec).expect("wav writer");
    for s in samples {
        w.write_sample((s.clamp(-1.0, 1.0) * 32_767.0) as i16)
            .unwrap();
    }
    w.finalize().unwrap();
    path.to_path_buf()
}

/// `seconds` of a sine of `hz`: audio with a content of its own.
pub fn tone(seconds: f32, hz: f32) -> Vec<f32> {
    (0..(seconds * RATE as f32) as usize)
        .map(|n| (std::f32::consts::TAU * hz * n as f32 / RATE as f32).sin() * 0.3)
        .collect()
}

/// A project folder, its store, and a dispatcher with `transcribe` wired to
/// a recogniser.
pub struct Project {
    pub dir: TempDir,
    pub store: Store,
    pub engine: audio_engine::Engine,
    pub clipboard: Option<tools::Clipboard>,
    pub dispatcher: ToolDispatcher,
}

impl Project {
    pub fn new(stt: Arc<dyn SpeechToText>) -> Self {
        let dir = TempDir::new().expect("tempdir");
        let store = Store::open(dir.path()).expect("store");
        let mut dispatcher = ToolDispatcher::default_dispatcher();
        assert!(dispatcher.unregister("transcribe"));
        dispatcher.register(Box::new(TranscribeTool::with(stt)));
        Self {
            dir,
            store,
            engine: audio_engine::Engine::new(),
            clipboard: None,
            dispatcher,
        }
    }

    /// Write `samples` to a WAV in the project folder and `load` it as the
    /// next track. Returns the file.
    pub fn load(&mut self, name: &str, samples: &[f32]) -> PathBuf {
        let path = write_wav(&self.dir.path().join(name), samples);
        match self.call("load", json!({ "path": path.to_string_lossy() })) {
            ToolResult::Ok(_) => path,
            ToolResult::Error(m) => panic!("load failed: {m}"),
        }
    }

    pub fn call(&mut self, tool: &str, args: Value) -> ToolResult {
        let mut ctx = ToolContext {
            store: &mut self.store,
            engine: &mut self.engine,
            user_message: "",
            clipboard: &mut self.clipboard,
            allowed_tools: None,
        };
        self.dispatcher
            .invoke(tool, args, &mut ctx)
            .unwrap_or_else(|e| panic!("{tool}: dispatcher error {e}"))
    }

    pub fn ok(&mut self, tool: &str, args: Value) -> Value {
        match self.call(tool, args) {
            ToolResult::Ok(v) => v,
            ToolResult::Error(m) => panic!("`{tool}` failed: {m}"),
        }
    }

    pub fn err(&mut self, tool: &str, args: Value) -> String {
        match self.call(tool, args) {
            ToolResult::Error(m) => m,
            ToolResult::Ok(v) => panic!("`{tool}` succeeded: {v}"),
        }
    }

    pub fn state(&self) -> session::SessionState {
        let head = self.store.head().expect("a head");
        self.store.get(head).expect("head node").state
    }

    pub fn node_count(&self) -> usize {
        self.store.list_nodes().expect("nodes").len()
    }

    /// The files in the project's inference cache.
    pub fn cache_files(&self) -> Vec<PathBuf> {
        let dir = self.dir.path().join(".audiograph").join("inference-cache");
        std::fs::read_dir(dir)
            .map(|d| d.flatten().map(|e| e.path()).collect())
            .unwrap_or_default()
    }
}
