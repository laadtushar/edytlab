//! The `separate_stems` tool on the real model (#385). `#[ignore]`d: it needs
//! what CI does not have.
//!
//! * the pinned `htdemucs` model, as the **folder** of its two files
//!   (`DEMUCS_MODEL_PATH`; `crates/ml-pipeline/models.json` pins them;
//!   `scripts/fetch-ml-model.sh htdemucs <dir>` makes the folder), and
//! * the ONNX Runtime library (`ORT_DYLIB_PATH`;
//!   `scripts/fetch-ml-runtime.sh <platform> <dir>` prints it).
//!
//! ```text
//! export ORT_DYLIB_PATH=/path/to/libonnxruntime.so
//! export DEMUCS_MODEL_PATH=/path/to/models/htdemucs
//! cargo test -p tools --release --test separate_stems_end_to_end -- --ignored --nocapture
//! ```
//!
//! Asked for with `--ignored` it **fails** when either is missing, and says
//! which, rather than returning early: an `#[ignore]`d test that passes without
//! having run is the vacuous pass #263 is about.
//!
//! `ml-demucs`'s `real_model.rs` holds the separator and its SDR; this holds
//! the tool around it: the audio read through `load` and a track, the answer,
//! that nothing is written to the session, the progress strip, the cache,
//! Cancel, and that a stem loads as a track (the acceptance box of #385).
//!
//! The recording is the one that test uses, not a copy of it:
//! `crates/ml-demucs/tests/fixtures/stems/`, four stems of 8.5 s (credit in the
//! README beside them), added up.
//!
//! One test, in phases, because the environment, the progress sink and the
//! cancel switch are process-wide:
//!
//! 1. through `track`: four stems come back, each far closer to the stem that
//!    went in than the mix is, the session has no new node, and the strip was
//!    told about every segment and closed;
//! 2. the same request again is answered from the cache, with the same files;
//! 3. a stem loads as a track, as long as the recording;
//! 4. a Cancel pressed in the first segment of another recording stops the
//!    run, says so, and leaves the cache as it was.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use session::Store;
use tools::{ToolContext, ToolDispatcher, ToolResult};

/// Abort, rather than stall for ever, if the run is stuck.
fn watchdog() {
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(1200));
        eprintln!("separate_stems_end_to_end ran for 20 minutes: it is stuck, not slow");
        std::process::abort();
    });
}

fn required(var: &str, how: &str, folder: bool) -> PathBuf {
    let value = std::env::var_os(var).unwrap_or_else(|| {
        panic!(
            "{var} is not set, so this test cannot run. It was asked for with --ignored, and a \
             skip is not an answer (#263). {how}"
        )
    });
    let path = PathBuf::from(value);
    assert!(
        if folder {
            path.is_dir()
        } else {
            path.is_file()
        },
        "{var} is {}, which is not an existing {}. {how}",
        path.display(),
        if folder { "folder" } else { "file" }
    );
    path
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../ml-demucs/tests/fixtures/stems")
        .join(name)
}

const STEMS: [&str; 4] = ["vocals", "drums", "bass", "other"];

/// The four fixture stems as stereo `f32`, and their sum.
fn references() -> (Vec<Vec<f32>>, Vec<f32>) {
    let stems: Vec<Vec<f32>> = STEMS
        .iter()
        .map(|n| {
            audio_decoder::decode_file(&fixture(&format!("{n}.flac")))
                .unwrap_or_else(|e| panic!("{n}: {e}"))
                .samples
        })
        .collect();
    let mix = (0..stems[0].len())
        .map(|i| stems.iter().map(|s| s[i]).sum())
        .collect();
    (stems, mix)
}

fn write_wav(path: &Path, samples: &[f32]) {
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: 44_100,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(path, spec).unwrap();
    for &s in samples {
        w.write_sample((s * 32_768.0).round().clamp(-32_768.0, 32_767.0) as i16)
            .unwrap();
    }
    w.finalize().unwrap();
}

fn read_wav(path: &Path) -> Vec<f32> {
    hound::WavReader::open(path)
        .unwrap()
        .samples::<i16>()
        .map(|s| f32::from(s.unwrap()) / 32_768.0)
        .collect()
}

fn sdr(reference: &[f32], estimate: &[f32]) -> f64 {
    let (mut signal, mut noise) = (0.0f64, 0.0f64);
    for (&s, &e) in reference.iter().zip(estimate) {
        signal += f64::from(s).powi(2);
        noise += (f64::from(s) - f64::from(e)).powi(2);
    }
    10.0 * (signal / noise.max(1e-30)).log10()
}

struct Session {
    dir: tempfile::TempDir,
    store: Store,
    engine: audio_engine::Engine,
    clipboard: Option<tools::Clipboard>,
    dispatcher: ToolDispatcher,
}

impl Session {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = Store::open(dir.path()).expect("store");
        Self {
            dir,
            store,
            engine: audio_engine::Engine::new(),
            clipboard: None,
            // The real dispatcher: the real `separate_stems`, on the real model.
            dispatcher: ToolDispatcher::default_dispatcher(),
        }
    }

    fn call(&mut self, tool: &str, args: Value) -> ToolResult {
        let mut ctx = ToolContext {
            store: &mut self.store,
            engine: &mut self.engine,
            user_message: "",
            clipboard: &mut self.clipboard,
            allowed_tools: None,
        };
        self.dispatcher
            .invoke(tool, args, &mut ctx)
            .unwrap_or_else(|e| panic!("`{tool}` did not dispatch: {e}"))
    }

    fn ok(&mut self, tool: &str, args: Value) -> Value {
        match self.call(tool, args) {
            ToolResult::Ok(v) => v,
            ToolResult::Error(m) => panic!("`{tool}` failed: {m}"),
        }
    }

    fn stem_cache(&self) -> PathBuf {
        self.dir.path().join(".audiograph").join("stem-cache")
    }
}

fn stem_paths(v: &Value) -> Vec<PathBuf> {
    STEMS
        .iter()
        .map(|n| {
            PathBuf::from(
                v["stems"][n]
                    .as_str()
                    .unwrap_or_else(|| panic!("no {n}: {v}")),
            )
        })
        .collect()
}

#[test]
#[ignore = "needs ORT_DYLIB_PATH (an ONNX Runtime library) and DEMUCS_MODEL_PATH (the model folder); run by ml.yml"]
fn the_tool_separates_a_track_on_the_real_model() {
    watchdog();
    required(
        "ORT_DYLIB_PATH",
        "Point it at libonnxruntime (scripts/fetch-ml-runtime.sh <platform> <dir> prints it).",
        false,
    );
    required(
        "DEMUCS_MODEL_PATH",
        "Point it at the folder scripts/fetch-ml-model.sh htdemucs <dir> makes.",
        true,
    );

    // What the strip is told. Registered once: this is the only test here.
    let events: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
    let cancel_in_first_segment = Arc::new(Mutex::new(false));
    {
        let (events, cancel) = (Arc::clone(&events), Arc::clone(&cancel_in_first_segment));
        assert!(tools::progress::set_sink(move |event| {
            if event["kind"] == "separate_stems" {
                if *cancel.lock().unwrap() && event["done"].is_null() {
                    tools::progress::request_cancel();
                }
                events.lock().unwrap().push(event);
            }
        }));
    }

    let (stems, mix) = references();
    let mut s = Session::new();
    let recording = s.dir.path().join("mix.wav");
    write_wav(&recording, &mix);
    s.ok("load", json!({ "path": recording.to_string_lossy() }));
    let nodes = s.store.list_nodes().unwrap().len();
    let head = s.store.head();

    // 1. Through `track`.
    let started = Instant::now();
    let first = s.ok("separate_stems", json!({ "track": 0 }));
    let seconds = started.elapsed().as_secs_f64();
    eprintln!(
        "8.5 s of audio through the tool in {seconds:.1} s (model load included): {}",
        first["summary"]
    );
    assert_eq!(first["cached"], false);
    assert_eq!(first["model"], "htdemucs");
    assert_eq!(
        (first["sample_rate"].as_u64(), first["channels"].as_u64()),
        (Some(44_100), Some(2))
    );
    let paths = stem_paths(&first);
    for (i, path) in paths.iter().enumerate() {
        let estimate = read_wav(path);
        assert_eq!(
            estimate.len(),
            mix.len(),
            "{}: the recording's own length",
            STEMS[i]
        );
        let (got, baseline) = (sdr(&stems[i], &estimate), sdr(&stems[i], &mix));
        eprintln!(
            "{:<7} SDR {got:6.2} dB (the mix as the stem: {baseline:6.2} dB)",
            STEMS[i]
        );
        assert!(
            got > baseline + 3.0,
            "{}: {got:.2} dB is not 3 dB better than the mix itself ({baseline:.2} dB)",
            STEMS[i]
        );
    }
    assert_eq!(s.store.list_nodes().unwrap().len(), nodes, "no node");
    assert_eq!(s.store.head(), head);
    {
        let events = events.lock().unwrap();
        let shown: Vec<&Value> = events.iter().filter(|e| e["done"].is_null()).collect();
        assert_eq!(shown.len(), 3, "0 of 2, 1 of 2, 2 of 2: {events:?}");
        assert!(shown.iter().all(|e| e["total"] == 2), "{events:?}");
        let closing = events.last().unwrap();
        assert_eq!(closing["done"], true, "the strip is closed: {events:?}");
        assert_eq!(closing["cancelled"], false);
    }

    // 2. The same request again is a hit, on the same files.
    let again = s.ok("separate_stems", json!({ "track": 0 }));
    assert_eq!(again["cached"], true);
    assert_eq!(stem_paths(&again), paths);
    assert_eq!(std::fs::read_dir(s.stem_cache()).unwrap().count(), 1);

    // 3. A stem is a track.
    let loaded = s.ok("load", json!({ "path": paths[0].to_string_lossy() }));
    assert!(
        loaded["summary"].as_str().unwrap().contains("as track 1"),
        "{loaded}"
    );
    let state = s.store.get(s.store.head().unwrap()).unwrap().state;
    assert_eq!(state.tracks.len(), 2);
    assert_eq!(
        state.tracks[1].clips[0].length as usize,
        mix.len() / 2,
        "the stem is as long as the recording"
    );

    // 4. Cancel in the first segment of other audio: said, and nothing kept.
    let quieter: Vec<f32> = mix.iter().map(|v| v * 0.5).collect();
    let other = s.dir.path().join("quieter.wav");
    write_wav(&other, &quieter);
    events.lock().unwrap().clear();
    *cancel_in_first_segment.lock().unwrap() = true;
    let result = s.call("separate_stems", json!({ "path": other.to_string_lossy() }));
    *cancel_in_first_segment.lock().unwrap() = false;
    let ToolResult::Error(text) = result else {
        panic!("a cancelled run reported success");
    };
    assert!(text.contains("cancelled by the user"), "{text}");
    assert_eq!(
        std::fs::read_dir(s.stem_cache()).unwrap().count(),
        1,
        "the first entry only: no entry, no .tmp- leftover"
    );
    let events = events.lock().unwrap();
    assert_eq!(events.last().unwrap()["cancelled"], true, "{events:?}");
}
