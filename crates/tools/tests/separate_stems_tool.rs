//! The `separate_stems` tool, against a stem model that is the real pipeline
//! around a network that is not a network (#385).
//!
//! What the tool does around the model: which audio it reads, the answer it
//! gives (four stems on disk, nothing written to the session), the cache and
//! its bound, the progress strip, Cancel, what it says when it cannot run, and
//! that a stem loads as a track. The fake is `ml_demucs::separate_to_dir` with a
//! `SegmentModel` that splits its input in fractions, so the stems are real
//! files made by the real pipeline; only ONNX Runtime and the weights are
//! missing. The real model is exercised by `ml-demucs`'s `real_model.rs` and
//! `tools`' `separate_stems_end_to_end.rs`, both run by `ml.yml`.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use audio_decoder::DecodedAudio;
use ml_demucs::separate::{SegmentModel, Stereo, MODEL_SOURCES};
use ml_demucs::{Control, DemucsError, Separated};
use serde_json::{json, Value};
use session::Store;
use tempfile::TempDir;
use tools::tool::separate_stems::{answer, ModelRef, SeparateStemsTool, StemFailure, StemModel};
use tools::{ToolContext, ToolDispatcher, ToolResult};

const RATE: u32 = 44_100;
/// A tenth of a second, so two seconds of audio are several segments.
const SEG: usize = 4_410;

/// The pipeline's fake network: sources are fractions of the input.
struct Fractions {
    /// Called before each segment, with how many have run.
    on_segment: Box<dyn FnMut(usize) + Send>,
    runs: usize,
}

impl SegmentModel for Fractions {
    fn segment_samples(&self) -> usize {
        SEG
    }
    fn sources(&self) -> &[&'static str] {
        &MODEL_SOURCES
    }
    fn separate_segment(&mut self, segment: &Stereo) -> Result<Vec<Stereo>, DemucsError> {
        (self.on_segment)(self.runs);
        self.runs += 1;
        Ok([0.1f32, 0.2, 0.3, 0.4]
            .iter()
            .map(|f| {
                [
                    segment[0].iter().map(|v| v * f).collect(),
                    segment[1].iter().map(|v| v * f).collect(),
                ]
            })
            .collect())
    }
}

struct Fake {
    runs: AtomicUsize,
    model: Mutex<Result<ModelRef, StemFailure>>,
    fails_with: Mutex<Option<StemFailure>>,
    /// Press Cancel while this segment (from 0) is running.
    cancel_at_segment: Mutex<Option<usize>>,
    /// What the pipeline was given: (frames, rate, channels).
    given: Mutex<Vec<(usize, u32, u16)>>,
}

fn model(fingerprint: &str) -> ModelRef {
    ModelRef {
        folder: PathBuf::from("/models/fake"),
        fingerprint: fingerprint.to_string(),
        downloaded: true,
    }
}

impl Fake {
    fn new() -> Self {
        Self {
            runs: AtomicUsize::new(0),
            model: Mutex::new(Ok(model("one"))),
            fails_with: Mutex::new(None),
            cancel_at_segment: Mutex::new(None),
            given: Mutex::new(Vec::new()),
        }
    }
    fn runs(&self) -> usize {
        self.runs.load(Ordering::SeqCst)
    }
}

impl StemModel for Fake {
    fn locate(&self) -> Result<ModelRef, StemFailure> {
        self.model.lock().unwrap().clone()
    }

    fn separate(
        &self,
        _model: &ModelRef,
        audio: DecodedAudio,
        dest: &Path,
        control: &Control<'_>,
    ) -> Result<Separated, StemFailure> {
        self.runs.fetch_add(1, Ordering::SeqCst);
        self.given.lock().unwrap().push((
            audio.samples.len() / usize::from(audio.channels),
            audio.sample_rate,
            audio.channels,
        ));
        if let Some(f) = self.fails_with.lock().unwrap().clone() {
            return Err(f);
        }
        let cancel_at = *self.cancel_at_segment.lock().unwrap();
        let mut network = Fractions {
            on_segment: Box::new(move |k| {
                if Some(k) == cancel_at {
                    tools::progress::request_cancel();
                }
            }),
            runs: 0,
        };
        ml_demucs::separate_to_dir(&mut network, audio, dest, control).map_err(|e| match e {
            DemucsError::Cancelled => StemFailure::Cancelled,
            other => StemFailure::Failed(other.to_string()),
        })
    }
}

/// The cancel switch and the progress sink are the process's, as the window's
/// one Cancel button is, so two runs at once in this binary would press and
/// clear each other's. A `Project` holds this for as long as it lives.
static ONE_RUN_AT_A_TIME: Mutex<()> = Mutex::new(());

struct Project {
    // Dropped last (fields drop in order): the project is gone before the next
    // test's starts.
    dir: TempDir,
    store: Store,
    engine: audio_engine::Engine,
    clipboard: Option<tools::Clipboard>,
    dispatcher: ToolDispatcher,
    _serial: std::sync::MutexGuard<'static, ()>,
}

impl Project {
    fn new(fake: Arc<Fake>) -> Self {
        Self::with_tool(SeparateStemsTool::with(fake))
    }

    fn with_tool(tool: SeparateStemsTool) -> Self {
        let serial = ONE_RUN_AT_A_TIME
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let dir = TempDir::new().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let mut dispatcher = ToolDispatcher::default_dispatcher();
        assert!(dispatcher.unregister("separate_stems"));
        dispatcher.register(Box::new(tool));
        let mut p = Self {
            dir,
            store,
            engine: audio_engine::Engine::new(),
            clipboard: None,
            dispatcher,
            _serial: serial,
        };
        let path = p.wav("a.wav", 2.0, 220.0, RATE, 2);
        p.ok("load", json!({ "path": path.to_string_lossy() }));
        p
    }

    fn wav(&self, name: &str, seconds: f32, hz: f32, rate: u32, channels: u16) -> PathBuf {
        let path = self.dir.path().join(name);
        write_wav(&path, seconds, hz, rate, channels);
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
    fn stem_cache(&self) -> PathBuf {
        self.dir.path().join(".audiograph").join("stem-cache")
    }
}

fn write_wav(path: &Path, seconds: f32, hz: f32, rate: u32, channels: u16) {
    let spec = hound::WavSpec {
        channels,
        sample_rate: rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(path, spec).unwrap();
    for n in 0..(seconds * rate as f32) as usize {
        let s = (std::f32::consts::TAU * hz * n as f32 / rate as f32).sin() * 0.3;
        for _ in 0..channels {
            w.write_sample((s * 32_767.0) as i16).unwrap();
        }
    }
    w.finalize().unwrap();
}

fn stem_paths(v: &Value) -> Vec<PathBuf> {
    ["vocals", "drums", "bass", "other"]
        .iter()
        .map(|n| {
            PathBuf::from(
                v["stems"][n]
                    .as_str()
                    .unwrap_or_else(|| panic!("no {n} in {v}")),
            )
        })
        .collect()
}

fn frames_of(path: &Path) -> (u32, u32) {
    let r = hound::WavReader::open(path).unwrap();
    (r.spec().sample_rate, r.duration())
}

// -- the answer -------------------------------------------------------------

#[test]
fn a_track_is_separated_into_four_files_and_the_session_is_untouched() {
    let fake = Arc::new(Fake::new());
    let mut p = Project::new(Arc::clone(&fake));
    let nodes = p.store.list_nodes().unwrap().len();
    let head = p.store.head();

    let v = p.ok("separate_stems", json!({ "track": 0 }));

    assert_eq!(p.store.list_nodes().unwrap().len(), nodes, "no node");
    assert_eq!(p.store.head(), head);
    assert_eq!(v["model"], "htdemucs");
    assert_eq!(v["cached"], false);
    assert_eq!(v["duration_sec"], 2.0);
    assert_eq!(
        (v["sample_rate"].as_u64(), v["channels"].as_u64()),
        (Some(44_100), Some(2))
    );
    let paths = stem_paths(&v);
    for (path, name) in paths.iter().zip(["vocals", "drums", "bass", "other"]) {
        assert!(path.is_file(), "{}", path.display());
        assert!(path.ends_with(format!("{name}.wav")));
        assert!(path.starts_with(p.stem_cache()), "{}", path.display());
        assert_eq!(
            frames_of(path),
            (RATE, 2 * RATE),
            "{name}: the track's own length"
        );
    }
    // One entry, whole, and nothing beside it.
    let entries: Vec<_> = std::fs::read_dir(p.stem_cache()).unwrap().collect();
    assert_eq!(entries.len(), 1);
    let summary = v["summary"].as_str().unwrap();
    assert!(
        summary.contains("load"),
        "tells the agent how to use them: {summary}"
    );
    assert!(
        summary.contains("Nothing in the session was changed"),
        "{summary}"
    );
    assert_eq!(fake.runs(), 1);
}

#[test]
fn a_file_is_separated_at_its_own_rate_and_channels() {
    let fake = Arc::new(Fake::new());
    let mut p = Project::new(Arc::clone(&fake));
    let f = p.wav("mono48.wav", 1.5, 330.0, 48_000, 1);
    let v = p.ok("separate_stems", json!({ "path": f.to_string_lossy() }));
    assert_eq!(fake.given.lock().unwrap()[0], (72_000, 48_000, 1));
    assert_eq!(
        (v["sample_rate"].as_u64(), v["channels"].as_u64()),
        (Some(48_000), Some(1))
    );
    for path in stem_paths(&v) {
        assert_eq!(frames_of(&path), (48_000, 72_000));
    }
}

#[test]
fn a_second_request_for_the_same_audio_is_a_cache_hit_and_other_audio_or_model_is_not() {
    let fake = Arc::new(Fake::new());
    let mut p = Project::new(Arc::clone(&fake));
    let first = p.ok("separate_stems", json!({ "track": 0 }));
    let again = p.ok("separate_stems", json!({ "track": 0 }));
    assert_eq!(fake.runs(), 1);
    assert_eq!(again["cached"], true);
    assert_eq!(stem_paths(&first), stem_paths(&again), "the same files");
    assert!(again["summary"]
        .as_str()
        .unwrap()
        .contains("from the cache"));

    // The same samples from another file are the same audio.
    let copy = p.wav("copy.wav", 2.0, 220.0, RATE, 2);
    let same = p.ok("separate_stems", json!({ "path": copy.to_string_lossy() }));
    assert_eq!(same["cached"], true);
    assert_eq!(fake.runs(), 1);

    // Another sound, another entry; another model, another entry.
    let other = p.wav("other.wav", 2.0, 440.0, RATE, 2);
    let o = p.ok("separate_stems", json!({ "path": other.to_string_lossy() }));
    assert_eq!(o["cached"], false);
    *fake.model.lock().unwrap() = Ok(model("two"));
    let m = p.ok("separate_stems", json!({ "track": 0 }));
    assert_eq!(m["cached"], false);
    assert_eq!(fake.runs(), 3);
    assert_ne!(stem_paths(&first), stem_paths(&m));
}

/// The same samples at another rate or in another layout are not the same
/// audio: the cache is keyed with `provenance::audio_hash`, which holds both.
#[test]
fn the_layout_is_part_of_the_audio() {
    let fake = Arc::new(Fake::new());
    let mut p = Project::new(Arc::clone(&fake));
    let a = p.wav("m.wav", 1.0, 220.0, 44_100, 1);
    let b = p.wav("s.wav", 1.0, 220.0, 44_100, 2);
    let x = p.ok("separate_stems", json!({ "path": a.to_string_lossy() }));
    let y = p.ok("separate_stems", json!({ "path": b.to_string_lossy() }));
    assert_eq!(
        (x["cached"].as_bool(), y["cached"].as_bool()),
        (Some(false), Some(false))
    );
    assert_eq!(fake.runs(), 2);
}

#[test]
fn bad_arguments_are_refused_before_the_model_is_asked() {
    let fake = Arc::new(Fake::new());
    let mut p = Project::new(Arc::clone(&fake));
    assert!(p.err("separate_stems", json!({})).contains("`track`"));
    let f = p.wav("x.wav", 1.0, 200.0, RATE, 2);
    assert!(p
        .err(
            "separate_stems",
            json!({ "track": 0, "path": f.to_string_lossy() })
        )
        .contains("not both"));
    assert!(p
        .err("separate_stems", json!({ "track": 7 }))
        .to_lowercase()
        .contains("track"));
    assert!(p
        .err(
            "separate_stems",
            json!({ "path": "/definitely/not/here.wav" })
        )
        .contains("file not found"));
    assert_eq!(fake.runs(), 0);
}

/// The schema enforces the model enum, so a model that is not offered fails
/// dispatch-time validation (a `DispatchError`, not a `ToolResult::Error`).
#[test]
fn a_model_that_is_not_offered_is_refused_by_the_schema() {
    let fake = Arc::new(Fake::new());
    let mut p = Project::new(Arc::clone(&fake));
    let mut ctx = ToolContext {
        store: &mut p.store,
        engine: &mut p.engine,
        user_message: "",
        clipboard: &mut p.clipboard,
        allowed_tools: None,
    };
    let err = p
        .dispatcher
        .invoke(
            "separate_stems",
            json!({ "track": 0, "model": "htdemucs_ft" }),
            &mut ctx,
        )
        .unwrap_err();
    assert!(err.to_string().contains("htdemucs_ft"), "{err}");
    assert_eq!(fake.runs(), 0);
}

#[test]
fn audio_that_cannot_be_decoded_is_an_error_not_a_stem() {
    let fake = Arc::new(Fake::new());
    let mut p = Project::new(Arc::clone(&fake));
    let junk = p.dir.path().join("junk.wav");
    std::fs::write(&junk, b"this is not audio").unwrap();
    let text = p.err("separate_stems", json!({ "path": junk.to_string_lossy() }));
    assert!(text.contains("decode failed"), "{text}");
    assert_eq!(fake.runs(), 0);
}

// -- what it says when it cannot run -----------------------------------------

#[test]
fn every_setup_problem_tells_the_agent_to_stop_and_names_no_path() {
    for f in [
        StemFailure::ModelNotDownloaded,
        StemFailure::RuntimeUnavailable("no ONNX Runtime library was found".into()),
        StemFailure::ModelDamaged,
        StemFailure::ModelPathInvalid,
    ] {
        let fake = Arc::new(Fake::new());
        *fake.model.lock().unwrap() = Err(f.clone());
        let mut p = Project::new(Arc::clone(&fake));
        let text = p.err("separate_stems", json!({ "track": 0 }));
        assert_eq!(text, answer(&f));
        assert!(text.contains("Tell the user"), "{text}");
        assert!(
            text.contains("Do not try") || text.contains("Tell the user to correct"),
            "{text}"
        );
        assert!(!text.contains("/models"), "{text}");
        assert_eq!(fake.runs(), 0, "no run without a model");
        assert!(!p.stem_cache().exists(), "nothing was made");
        // No marker: the setup card speaks of the speech model.
        for m in tools::tool::transcribe::MARKERS {
            assert!(!text.contains(m), "{f:?}: {text}");
        }
    }
    // The download is the user's: the answer names the row, not a command.
    let text = answer(&StemFailure::ModelNotDownloaded);
    assert!(text.contains("Stem model"), "{text}");
    assert!(text.contains("Settings"), "{text}");
}

#[test]
fn a_failed_run_says_why_and_leaves_no_entry() {
    for (failure, wanted) in [
        (StemFailure::OutOfMemory, "memory"),
        (
            StemFailure::Failed("the network returned the wrong shape".into()),
            "wrong shape",
        ),
    ] {
        let fake = Arc::new(Fake::new());
        *fake.fails_with.lock().unwrap() = Some(failure.clone());
        let mut p = Project::new(Arc::clone(&fake));
        let text = p.err("separate_stems", json!({ "track": 0 }));
        assert_eq!(text, answer(&failure));
        assert!(text.contains(wanted), "{text}");
        assert!(!text.contains(p.dir.path().to_str().unwrap()), "{text}");
    }
}

// -- cancel and the progress strip ---------------------------------------------

#[test]
fn a_cancel_stops_the_run_and_leaves_no_entry_and_no_partial_hit() {
    let fake = Arc::new(Fake::new());
    *fake.cancel_at_segment.lock().unwrap() = Some(2);
    let mut p = Project::new(Arc::clone(&fake));
    let text = p.err("separate_stems", json!({ "track": 0 }));
    assert!(text.contains("cancelled by the user"), "{text}");
    assert!(text.contains("Do not run it again"), "{text}");
    let left: Vec<_> = std::fs::read_dir(p.stem_cache())
        .map(|d| d.flatten().map(|e| e.file_name()).collect())
        .unwrap_or_default();
    assert!(left.is_empty(), "nothing left in the cache: {left:?}");

    // The next call, on the same audio, is a run: not a cache hit, and not the
    // stale cancel (`begin` clears it).
    *fake.cancel_at_segment.lock().unwrap() = None;
    let again = p.ok("separate_stems", json!({ "track": 0 }));
    assert_eq!(again["cached"], false);
    assert_eq!(fake.runs(), 2);
}

// -- the bound on the cache ----------------------------------------------------

#[test]
fn the_cache_is_kept_under_its_cap_and_the_new_entry_is_kept() {
    let fake = Arc::new(Fake::new());
    // One entry is 4 x 2 s x 44.1 kHz x 2 channels x 2 bytes = 705,600 bytes
    // and a bit; a cap of one and a half entries holds one.
    let mut p = Project::with_tool(
        SeparateStemsTool::with(Arc::clone(&fake) as Arc<dyn StemModel>).with_cache_cap(1_000_000),
    );
    let a = p.wav("a2.wav", 2.0, 300.0, RATE, 2);
    let b = p.wav("b2.wav", 2.0, 500.0, RATE, 2);
    let first = p.ok("separate_stems", json!({ "path": a.to_string_lossy() }));
    // Make the first the older.
    for path in stem_paths(&first) {
        let file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        file.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(3600))
            .unwrap();
    }
    let second = p.ok("separate_stems", json!({ "path": b.to_string_lossy() }));
    assert!(
        stem_paths(&second).iter().all(|f| f.is_file()),
        "the new entry stays"
    );
    assert!(
        stem_paths(&first).iter().all(|f| !f.exists()),
        "the older one is gone"
    );
    assert!(
        second["summary"].as_str().unwrap().contains("removed"),
        "{}",
        second["summary"]
    );
}

// -- a stem is a track --------------------------------------------------------

/// The acceptance box of #385: loading a stem as a track works end to end, and
/// the track does not depend on the cache entry it came from.
#[test]
fn a_stem_loads_as_a_track_that_survives_its_cache_entry() {
    let fake = Arc::new(Fake::new());
    let mut p = Project::new(Arc::clone(&fake));
    let v = p.ok("separate_stems", json!({ "track": 0 }));
    let vocals = stem_paths(&v).remove(0);

    let loaded = p.ok("load", json!({ "path": vocals.to_string_lossy() }));
    assert!(
        loaded["summary"].as_str().unwrap().contains("as track 1"),
        "{loaded}"
    );

    let state = p.store.get(p.store.head().unwrap()).unwrap().state;
    assert_eq!(state.tracks.len(), 2);
    let clip = &state.tracks[1].clips[0];
    assert_eq!(clip.length, 2 * u64::from(RATE), "the stem's full length");
    // The store's own copy, not the cache's file: removing the entry does not
    // leave a clip pointing at nothing, and a sweep may delete the cache.
    assert!(
        clip.source_path
            .starts_with(p.dir.path().join(".audiograph").join("derived")),
        "{}",
        clip.source_path.display()
    );
    std::fs::remove_dir_all(p.stem_cache()).unwrap();
    assert!(clip.source_path.is_file());
    let head = p.store.head().unwrap().to_hex();
    let rendered = p.ok("render_preview", json!({ "node_id": head }));
    assert!(
        rendered.to_string().len() > 2,
        "the session still renders: {rendered}"
    );
}

// -- the tool's place ------------------------------------------------------------

#[test]
fn the_schema_no_longer_says_it_is_not_implemented() {
    use tools::Tool;
    let t = SeparateStemsTool::with(Arc::new(Fake::new()));
    let schema = t.schema();
    let text = schema.to_string();
    assert!(!text.to_uppercase().contains("NOT IMPLEMENTED"), "{text}");
    assert!(!text.contains("DEMUCS_MODEL_PATH"), "{text}");
    assert_eq!(
        schema["input_schema"]["properties"]["model"]["enum"],
        json!(["htdemucs"])
    );
    assert_eq!(schema["input_schema"]["additionalProperties"], false);
    assert_eq!(t.name(), "separate_stems");
}

/// A run takes about half the audio's length and writes hundreds of
/// megabytes, so the tool is held for approval (it writes files) and runs off
/// the store lock (it appends no node, so a lost race re-runs nothing).
#[test]
fn the_tool_is_held_for_approval_and_runs_off_the_lock() {
    use tools::Tool;
    let t = SeparateStemsTool::with(Arc::new(Fake::new()));
    assert!(t.mutates());
    assert!(t.runs_off_the_lock());
}
