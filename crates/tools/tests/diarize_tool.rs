//! The `diarize` tool, against a speaker model that is not a model (#168).
//!
//! What the tool does around the model: which audio it reads and at what rate,
//! the answer it gives (turns only, nothing written to the session), the cache,
//! the bounds, the progress strip, Cancel, and what it says when it cannot run.
//! The real model is exercised by `ml-diarize`'s `real_model.rs`.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use session::Store;
use tempfile::TempDir;
use tools::tool::diarize::{
    answer, DiarizeFailure, DiarizeTool, Heard, ModelRef, SpeakerModel, Stage, Turn,
};
use tools::{ToolContext, ToolDispatcher, ToolResult};

const RATE: u32 = 16_000;

type Bounds = (Option<usize>, Option<usize>, Option<usize>);

struct Fake {
    runs: AtomicUsize,
    heard: Heard,
    model: Mutex<Result<ModelRef, DiarizeFailure>>,
    fails_with: Mutex<Option<DiarizeFailure>>,
    bounds: Mutex<Vec<Bounds>>,
    samples: Mutex<Vec<usize>>,
    cancel_after_first_step: bool,
}

fn model(fingerprint: &str) -> ModelRef {
    ModelRef {
        folder: PathBuf::from("/models/fake"),
        fingerprint: fingerprint.to_string(),
        downloaded: true,
    }
}

fn turn(s: f64, e: f64, speaker: usize) -> Turn {
    Turn {
        start_s: s,
        end_s: e,
        speaker,
    }
}

impl Fake {
    fn new(turns: Vec<Turn>, speakers: usize) -> Self {
        Self {
            runs: AtomicUsize::new(0),
            heard: Heard {
                turns,
                speakers,
                duration_s: 2.0,
            },
            model: Mutex::new(Ok(model("one"))),
            fails_with: Mutex::new(None),
            bounds: Mutex::new(Vec::new()),
            samples: Mutex::new(Vec::new()),
            cancel_after_first_step: false,
        }
    }
    fn runs(&self) -> usize {
        self.runs.load(Ordering::SeqCst)
    }
}

impl SpeakerModel for Fake {
    fn locate(&self) -> Result<ModelRef, DiarizeFailure> {
        self.model.lock().unwrap().clone()
    }
    fn diarize(
        &self,
        _model: &ModelRef,
        audio: &[f32],
        bounds: Bounds,
        on_progress: &dyn Fn(Stage, usize, usize),
        cancel: &(dyn Fn() -> bool + Sync),
    ) -> Result<Heard, DiarizeFailure> {
        self.runs.fetch_add(1, Ordering::SeqCst);
        self.bounds.lock().unwrap().push(bounds);
        self.samples.lock().unwrap().push(audio.len());
        for i in 0..2 {
            if cancel() {
                return Err(DiarizeFailure::Cancelled);
            }
            on_progress(Stage::Segmentation, i, 2);
            if self.cancel_after_first_step && i == 0 {
                tools::progress::request_cancel();
            }
        }
        if let Some(f) = self.fails_with.lock().unwrap().clone() {
            return Err(f);
        }
        Ok(self.heard.clone())
    }
}

struct Project {
    dir: TempDir,
    store: Store,
    engine: audio_engine::Engine,
    clipboard: Option<tools::Clipboard>,
    dispatcher: ToolDispatcher,
}

impl Project {
    fn new(fake: Arc<Fake>) -> Self {
        let dir = TempDir::new().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let mut dispatcher = ToolDispatcher::default_dispatcher();
        assert!(dispatcher.unregister("diarize"));
        dispatcher.register(Box::new(DiarizeTool::with(fake)));
        let mut p = Self {
            dir,
            store,
            engine: audio_engine::Engine::new(),
            clipboard: None,
            dispatcher,
        };
        let path = p.wav("a.wav", 2.0, 220.0);
        p.ok("load", json!({ "path": path.to_string_lossy() }));
        p
    }

    fn wav(&self, name: &str, seconds: f32, hz: f32) -> PathBuf {
        let path = self.dir.path().join(name);
        write_wav(&path, seconds, hz);
        path
    }

    fn call(&mut self, tool: &str, args: Value) -> ToolResult {
        let mut ctx = ToolContext {
            store: &mut self.store,
            engine: &mut self.engine,
            user_message: "",
            clipboard: &mut self.clipboard,
            allowed_tools: None,
        };
        self.dispatcher.invoke(tool, args, &mut ctx).unwrap()
    }
    fn ok(&mut self, tool: &str, args: Value) -> Value {
        match self.call(tool, args) {
            ToolResult::Ok(v) => v,
            ToolResult::Error(m) => panic!("`{tool}` failed: {m}"),
        }
    }
    fn err(&mut self, tool: &str, args: Value) -> String {
        match self.call(tool, args) {
            ToolResult::Error(m) => m,
            ToolResult::Ok(v) => panic!("`{tool}` succeeded: {v}"),
        }
    }
}

fn write_wav(path: &Path, seconds: f32, hz: f32) {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(path, spec).unwrap();
    for n in 0..(seconds * RATE as f32) as usize {
        let s = (std::f32::consts::TAU * hz * n as f32 / RATE as f32).sin() * 0.3;
        w.write_sample((s * 32_767.0) as i16).unwrap();
    }
    w.finalize().unwrap();
}

fn two() -> Arc<Fake> {
    Arc::new(Fake::new(
        vec![turn(0.0, 1.0, 0), turn(1.0, 1.5, 1), turn(1.5, 2.0, 0)],
        2,
    ))
}

#[test]
fn turns_come_back_in_split_by_speaker_terms_and_the_session_is_untouched() {
    let fake = two();
    let mut p = Project::new(Arc::clone(&fake));
    let nodes = p.store.list_nodes().unwrap().len();
    let head = p.store.head();

    let v = p.ok("diarize", json!({ "track": 0 }));

    assert_eq!(p.store.list_nodes().unwrap().len(), nodes, "no node");
    assert_eq!(p.store.head(), head);
    assert_eq!(v["speakers"], 2);
    assert_eq!(v["turn_count"], 3);
    assert_eq!(v["cached"], false);
    assert_eq!(v["truncated"], false);
    assert_eq!(
        v["turns"],
        json!([
            [0.0, 1.0, "speaker_1"],
            [1.0, 1.5, "speaker_2"],
            [1.5, 2.0, "speaker_1"]
        ])
    );
    assert_eq!(
        v["talk_time"],
        json!([
            { "speaker": "speaker_1", "talk_sec": 1.5 },
            { "speaker": "speaker_2", "talk_sec": 0.5 }
        ])
    );
    let summary = v["summary"].as_str().unwrap();
    assert!(summary.contains("split_by_speaker"), "{summary}");
    assert!(summary.contains("session seconds"), "{summary}");
    assert!(
        !summary.contains(p.dir.path().to_str().unwrap()),
        "{summary}"
    );
    // The model is given 16 kHz mono: two seconds of it.
    assert_eq!(fake.samples.lock().unwrap().as_slice(), &[32_000]);
}

#[test]
fn a_file_is_diarized_in_its_own_seconds() {
    let fake = two();
    let mut p = Project::new(fake);
    let f = p.wav("talk.wav", 2.0, 300.0);
    let v = p.ok("diarize", json!({ "path": f.to_string_lossy() }));
    assert!(v["summary"].as_str().unwrap().contains("the file's own"));
}

#[test]
fn a_second_request_for_the_same_audio_and_options_is_a_cache_hit() {
    let fake = two();
    let mut p = Project::new(Arc::clone(&fake));
    p.ok("diarize", json!({ "track": 0 }));
    let again = p.ok("diarize", json!({ "track": 0 }));
    assert_eq!(fake.runs(), 1);
    assert_eq!(again["cached"], true);
    // Other options, another answer; another model, another answer.
    let n = p.ok("diarize", json!({ "track": 0, "num_speakers": 2 }));
    assert_eq!(n["cached"], false);
    *fake.model.lock().unwrap() = Ok(model("two"));
    let m = p.ok("diarize", json!({ "track": 0 }));
    assert_eq!(m["cached"], false);
    assert_eq!(fake.runs(), 3);
    assert_eq!(
        fake.bounds.lock().unwrap()[1],
        (Some(2), None, None),
        "the options reach the model"
    );
}

#[test]
fn no_speech_is_an_answer_not_an_error() {
    let mut p = Project::new(Arc::new(Fake::new(vec![], 0)));
    let v = p.ok("diarize", json!({ "track": 0 }));
    assert_eq!(v["speakers"], 0);
    assert!(v["summary"].as_str().unwrap().contains("No speech"));
}

#[test]
fn a_long_conversation_is_cut_to_the_budget_and_says_so() {
    let turns: Vec<Turn> = (0..600)
        .map(|i| turn(i as f64, i as f64 + 0.9, i % 2))
        .collect();
    let mut p = Project::new(Arc::new(Fake::new(turns, 2)));
    let v = p.ok("diarize", json!({ "track": 0 }));
    assert_eq!(v["truncated"], true);
    assert_eq!(v["turn_count"], 600);
    let listed = v["turns"].as_array().unwrap();
    assert!(listed.len() < 600);
    assert!(
        serde_json::to_string(listed).unwrap().len() <= tools::tool::diarize::RESULT_BUDGET_BYTES
    );
    assert!(v["summary"]
        .as_str()
        .unwrap()
        .contains("do not split from this list"));
}

#[test]
fn bad_arguments_are_refused_before_the_model_is_asked() {
    let fake = two();
    let mut p = Project::new(Arc::clone(&fake));
    assert!(p.err("diarize", json!({})).contains("`track`"));
    let f = p.wav("x.wav", 1.0, 200.0);
    assert!(p
        .err(
            "diarize",
            json!({ "track": 0, "path": f.to_string_lossy() })
        )
        .contains("not both"));
    assert!(p
        .err("diarize", json!({ "track": 7 }))
        .to_lowercase()
        .contains("track"));
    assert!(p
        .err(
            "diarize",
            json!({ "track": 0, "min_speakers": 3, "max_speakers": 2 })
        )
        .contains("invalid speaker count"));
    assert!(p
        .err("diarize", json!({ "track": 0, "num_speakers": 1000 }))
        .contains("num_speakers"));
    assert_eq!(fake.runs(), 0);
}

#[test]
fn every_setup_problem_tells_the_agent_to_stop_and_names_no_path() {
    for f in [
        DiarizeFailure::ModelNotDownloaded,
        DiarizeFailure::RuntimeUnavailable("no ONNX Runtime library was found".into()),
        DiarizeFailure::ModelDamaged,
        DiarizeFailure::ModelPathInvalid,
    ] {
        let fake = two();
        *fake.model.lock().unwrap() = Err(f.clone());
        let mut p = Project::new(Arc::clone(&fake));
        let text = p.err("diarize", json!({ "track": 0 }));
        assert_eq!(text, answer(&f));
        assert!(
            text.contains("Do not try") || text.contains("Tell the user"),
            "{text}"
        );
        assert!(!text.contains("/models"), "{text}");
        assert_eq!(fake.runs(), 0, "no run without a model");
    }
}

#[test]
fn a_cancel_stops_the_run_and_leaves_nothing_in_the_cache() {
    let mut f = Fake::new(vec![turn(0.0, 1.0, 0)], 1);
    f.cancel_after_first_step = true;
    let fake = Arc::new(f);
    let mut p = Project::new(Arc::clone(&fake));
    let text = p.err("diarize", json!({ "track": 0 }));
    assert!(text.contains("cancelled by the user"), "{text}");
    let cache = p.dir.path().join(".audiograph").join("inference-cache");
    let entries = std::fs::read_dir(cache).map(|d| d.count()).unwrap_or(0);
    assert_eq!(entries, 0);
}

#[test]
fn the_tool_is_read_only_and_runs_off_the_lock() {
    use tools::Tool;
    let t = DiarizeTool::with(two());
    assert!(!t.mutates());
    assert!(t.runs_off_the_lock());
    assert_eq!(t.name(), "diarize");
}
