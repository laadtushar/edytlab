//! The `transcribe` tool on the real model, and the edits that work from its
//! transcript (#384). `#[ignore]`d: it needs what CI does not have.
//!
//! * the pinned `whisper-base` model, as the **folder** of its three files
//!   (`WHISPER_MODEL_PATH`; `crates/ml-pipeline/models.json` pins them), and
//! * the ONNX Runtime library (`ORT_DYLIB_PATH`;
//!   `scripts/fetch-ml-runtime.sh <platform> <dir>` prints it).
//!
//! ```text
//! export ORT_DYLIB_PATH=/path/to/libonnxruntime.so
//! export WHISPER_MODEL_PATH=/path/to/models/whisper-base
//! cargo test -p tools --release --test transcribe_end_to_end -- --ignored --nocapture
//! ```
//!
//! Asked for with `--ignored` it **fails** when either is missing, and says
//! which, rather than returning early: an `#[ignore]`d test that passes
//! without having run is the vacuous pass #263 is about.
//!
//! One test, in phases, because the environment and the cancel switch are
//! process-wide:
//!
//! 1. the tool, with `WHISPER_MODEL_PATH` set, on the fixture the
//!    `ml-whisper` tests use (`speech_words.wav`: twelve spoken words with
//!    known times): at most one word wrong and every start within 0.25 s,
//!    through `load`, `track` mode and the session;
//! 2. the same request again is answered from the cache;
//! 3. `cut_words` and `remove_fillers` on that real transcript, without being
//!    told the track;
//! 4. `language`;
//! 5. the model the way the app finds it: the variable unset and the models
//!    folder registered, as `ml_models_for_app` does at launch, so
//!    `ml_pipeline::models::cached` verifies the three files and the tool
//!    uses them (this needs the folder to be named `whisper-base`, which is
//!    how the app lays it out);
//! 6. Cancel, on audio long enough to be several windows.
//!
//! The numbers are printed (`--nocapture`).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use session::{Store, TranscriptSource};
use tools::{ToolContext, ToolDispatcher, ToolResult};

const MAX_WRONG_WORDS: usize = 1;
const START_TOLERANCE_S: f32 = 0.25;

#[derive(serde::Deserialize)]
struct Expected {
    text: String,
    start_s: f32,
}

/// Abort, rather than stall for ever, if the run is stuck.
fn watchdog() {
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(1_500));
        eprintln!("transcribe_end_to_end ran for 25 minutes: it is stuck, not slow");
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
        .join("../ml-whisper/tests/fixtures")
        .join(name)
}

fn normalise(word: &str) -> String {
    word.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// Edit distance between `expected` and `heard`, and for each expected word
/// the heard word it lines up with.
fn align(expected: &[String], heard: &[String]) -> (usize, Vec<Option<usize>>) {
    let (n, m) = (expected.len(), heard.len());
    let mut cost = vec![vec![0usize; m + 1]; n + 1];
    for (i, row) in cost.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, c) in cost[0].iter_mut().enumerate() {
        *c = j;
    }
    for i in 1..=n {
        for j in 1..=m {
            let same = usize::from(expected[i - 1] != heard[j - 1]);
            cost[i][j] = (cost[i - 1][j - 1] + same)
                .min(cost[i - 1][j] + 1)
                .min(cost[i][j - 1] + 1);
        }
    }
    let mut lined_up = vec![None; n];
    let (mut i, mut j) = (n, m);
    while i > 0 && j > 0 {
        let same = usize::from(expected[i - 1] != heard[j - 1]);
        if cost[i][j] == cost[i - 1][j - 1] + same {
            lined_up[i - 1] = Some(j - 1);
            i -= 1;
            j -= 1;
        } else if cost[i][j] == cost[i - 1][j] + 1 {
            i -= 1;
        } else {
            j -= 1;
        }
    }
    (cost[n][m], lined_up)
}

/// Compare the words the tool returned (`[text, start, end]`) with the labels.
/// Returns (words wrong, worst start error).
fn check(label: &str, words: &Value, expected: &[Expected]) -> (usize, f32) {
    let heard_words: Vec<(String, f32)> = words
        .as_array()
        .expect("the tool listed its words")
        .iter()
        .map(|w| {
            (
                w[0].as_str().unwrap().to_string(),
                w[1].as_f64().unwrap() as f32,
            )
        })
        .collect();
    let want: Vec<String> = expected.iter().map(|e| normalise(&e.text)).collect();
    let heard: Vec<String> = heard_words.iter().map(|w| normalise(&w.0)).collect();
    let (distance, lined_up) = align(&want, &heard);
    let mut worst = 0.0f32;
    for (e, at) in expected.iter().zip(&lined_up) {
        if let Some(j) = at {
            let ds = (heard_words[*j].1 - e.start_s).abs();
            worst = worst.max(ds);
            assert!(
                ds <= START_TOLERANCE_S,
                "{label}: {:?} starts at {:.3} s, expected {:.3} s (tolerance {START_TOLERANCE_S} s)",
                heard_words[*j].0,
                heard_words[*j].1,
                e.start_s
            );
        }
    }
    println!(
        "  {label}: {distance} of {} words wrong; worst start error {worst:.3} s; heard {heard:?}",
        expected.len()
    );
    assert!(
        distance <= MAX_WRONG_WORDS,
        "{label}: {distance} words wrong (at most {MAX_WRONG_WORDS}). Expected {want:?}, heard {heard:?}"
    );
    (distance, worst)
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
            // The real dispatcher: the real `transcribe`.
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
            .unwrap_or_else(|e| panic!("{tool}: dispatcher error {e}"))
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

    fn state(&self) -> session::SessionState {
        self.store
            .get(self.store.head().expect("a head"))
            .expect("head")
            .state
    }

    fn length(&self, track: usize) -> u64 {
        self.state().tracks[track]
            .clips
            .iter()
            .map(|c| c.length)
            .sum()
    }
}

fn write_tone(path: &Path, seconds: f32) {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 16_000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(path, spec).unwrap();
    for n in 0..(seconds * 16_000.0) as usize {
        let t = n as f32 / 16_000.0;
        w.write_sample(((std::f32::consts::TAU * 220.0 * t).sin() * 0.2 * 32_767.0) as i16)
            .unwrap();
    }
    w.finalize().unwrap();
}

/// The fixture repeated `times` over, with a second of silence between.
fn write_tiled(path: &Path, times: usize) {
    let mut reader = hound::WavReader::open(fixture("speech_words.wav")).unwrap();
    let samples: Vec<i16> = reader.samples::<i16>().map(Result::unwrap).collect();
    let spec = reader.spec();
    let mut w = hound::WavWriter::create(path, spec).unwrap();
    for _ in 0..times {
        for s in &samples {
            w.write_sample(*s).unwrap();
        }
        for _ in 0..spec.sample_rate {
            w.write_sample(0i16).unwrap();
        }
    }
    w.finalize().unwrap();
}

#[test]
#[ignore = "needs ORT_DYLIB_PATH and WHISPER_MODEL_PATH (the model folder); run with --ignored"]
fn the_tool_transcribes_the_known_words_and_the_edits_work_on_the_real_transcript() {
    watchdog();
    required(
        "ORT_DYLIB_PATH",
        "scripts/fetch-ml-runtime.sh <platform> <dir> prints the library to point it at.",
        false,
    );
    let model_dir = required(
        "WHISPER_MODEL_PATH",
        "It names the folder with onnx/encoder_model.onnx, onnx/decoder_model_merged_int8.onnx \
         and tokenizer.json, the files crates/ml-pipeline/models.json pins.",
        true,
    );
    let expected: Vec<Expected> = serde_json::from_str(
        &std::fs::read_to_string(fixture("speech_words.json")).expect("the labels"),
    )
    .expect("the labels parse");
    let speech_s = 7.0f64;

    // The session: track 0 is a tone, track 1 the speech. The words are of
    // track 1, which is what has to be cut.
    let mut s = Session::new();
    let tone = s.dir.path().join("tone.wav");
    write_tone(&tone, 4.0);
    s.ok("load", json!({ "path": tone.to_string_lossy() }));
    let speech = fixture("speech_words.wav");
    s.ok("load", json!({ "path": speech.to_string_lossy() }));
    assert_eq!(s.state().tracks.len(), 2);

    // 1. The known words, through the tool.
    println!("transcribe_end_to_end:");
    let began = Instant::now();
    let first = s.ok("transcribe", json!({ "track": 1 }));
    let took = began.elapsed();
    println!(
        "  first request: {:.2} s for {speech_s:.1} s of audio (realtime factor {:.1}x, includes \
         loading the model)",
        took.as_secs_f32(),
        speech_s / f64::from(took.as_secs_f32())
    );
    assert_eq!(first["cached"], false);
    assert_eq!(first["language"], "en");
    let (wrong, worst) = check("known words", &first["words"], &expected);
    let transcript = s
        .state()
        .transcript
        .expect("the node carries the transcript");
    assert_eq!(
        transcript.words.len(),
        first["word_count"].as_u64().unwrap() as usize
    );
    assert!(matches!(
        transcript.source,
        Some(TranscriptSource::Track { .. })
    ));
    println!("  numbers: wrong={wrong} worst_start_error={worst:.3}");

    // 2. Asked again, the cache answers.
    let began = Instant::now();
    let second = s.ok("transcribe", json!({ "track": 1 }));
    let hit = began.elapsed();
    println!(
        "  second request (cache): {:.3} s; first took {:.2} s",
        hit.as_secs_f32(),
        took.as_secs_f32()
    );
    assert_eq!(second["cached"], true);
    assert_eq!(second["words"], first["words"]);
    assert!(
        hit < took / 2,
        "the cache saved nothing: {hit:?} against {took:?}"
    );
    assert_eq!(
        std::fs::read_dir(s.dir.path().join(".audiograph/inference-cache"))
            .unwrap()
            .count(),
        1
    );

    // 3. The edits by words, on the real transcript. No `track`: the
    //    transcript's.
    let (tone_len, speech_len) = (s.length(0), s.length(1));
    let words: Vec<String> = transcript
        .words
        .iter()
        .map(|w| normalise(&w.text))
        .collect();
    let cut = s.ok("cut_words", json!({ "from_word": 4, "to_word": 6 }));
    let removed = cut["removed_sec"].as_f64().unwrap();
    println!(
        "  cut_words 4..6 ({:?}): removed {removed:.2} s",
        cut["removed_text"]
    );
    assert_eq!(s.length(0), tone_len, "the other track is untouched");
    let shrank = (speech_len - s.length(1)) as f64 / 16_000.0;
    assert!(
        (shrank - removed).abs() < 0.01,
        "the track shrank by {shrank:.3} s, the tool says {removed:.3} s"
    );
    let after = s.state().transcript.unwrap();
    assert_eq!(after.words.len(), words.len() - 2);
    assert!(
        after.words.windows(2).all(|p| p[0].start_s <= p[1].start_s),
        "the shifted words are still in order"
    );

    // A word the real transcript contains, named as a filler.
    let victim = normalise(&after.words[2].text);
    let report = s.ok("remove_fillers", json!({ "words": [victim] }));
    assert_eq!(report["applied"], false);
    assert_eq!(report["found"], 1, "{report}");
    let before = s.length(1);
    let done = s.ok(
        "remove_fillers",
        json!({ "words": [victim], "apply": true }),
    );
    println!(
        "  remove_fillers ({victim:?}): {} found, {:.2} s removed",
        done["found"], done["removed_sec"]
    );
    assert!(s.length(1) < before);
    assert_eq!(s.length(0), tone_len);
    assert_eq!(s.state().transcript.unwrap().words.len(), words.len() - 3);

    // Cutting the other track by these words is refused, whatever the words.
    let refused = s.err(
        "cut_words",
        json!({ "from_word": 0, "to_word": 1, "track": 0 }),
    );
    assert!(refused.contains("is of track 1"), "{refused}");

    // 4. The language.
    let en = s.ok(
        "transcribe",
        json!({ "path": speech.to_string_lossy(), "language": "en" }),
    );
    check("language given", &en["words"], &expected);
    let msg = s.err("transcribe", json!({ "track": 1, "language": "xx" }));
    assert!(msg.contains("unknown language"), "{msg}");

    // 5. The model the way the app finds it: variable unset, models folder
    //    registered, `cached` verifying the three files.
    assert_eq!(
        model_dir.file_name().and_then(|n| n.to_str()),
        Some("whisper-base"),
        "the app keeps the model in <models>/whisper-base; name the folder that"
    );
    let models = model_dir.parent().unwrap().to_path_buf();
    assert!(ml_pipeline::models::set_models_dir(models));
    let began = Instant::now();
    let found = ml_pipeline::models::cached(ml_pipeline::models::WHISPER_BASE_ID);
    let cached_took = began.elapsed();
    let began = Instant::now();
    let found_again = ml_pipeline::models::cached(ml_pipeline::models::WHISPER_BASE_ID);
    let cached_again = began.elapsed();
    println!(
        "  models::cached: {:.3} s, and again {:.3} s",
        cached_took.as_secs_f32(),
        cached_again.as_secs_f32()
    );
    assert_eq!(found.as_deref(), Some(model_dir.as_path()));
    assert_eq!(found_again, found);
    // SAFETY: the one test in this binary, and nothing else reads the
    // environment while it runs.
    unsafe { std::env::remove_var("WHISPER_MODEL_PATH") };
    let mut fresh = Session::new();
    fresh.ok("load", json!({ "path": speech.to_string_lossy() }));
    let began = Instant::now();
    let downloaded = fresh.ok("transcribe", json!({ "track": 0 }));
    println!(
        "  as the app finds the model: {:.2} s (a new project, so no cache entry)",
        began.elapsed().as_secs_f32()
    );
    check("downloaded model", &downloaded["words"], &expected);
    let op = fresh
        .store
        .get(fresh.store.head().unwrap())
        .unwrap()
        .op
        .unwrap();
    assert_eq!(op.inputs["model"]["id"], "whisper-base", "{:?}", op.inputs);

    // 6. Cancel: audio of several windows, stopped after the first.
    let long = fresh.dir.path().join("long.wav");
    write_tiled(&long, 10);
    fresh.ok("load", json!({ "path": long.to_string_lossy() }));
    static WINDOWS: AtomicUsize = AtomicUsize::new(0);
    assert!(tools::progress::set_sink(|event| {
        if event["kind"] == "transcribe" && event.get("index").is_some() {
            WINDOWS.fetch_add(1, Ordering::SeqCst);
            if event["index"] == 0 {
                tools::progress::request_cancel();
            }
        }
    }));
    let nodes = fresh.store.list_nodes().unwrap().len();
    let began = Instant::now();
    let msg = fresh.err("transcribe", json!({ "track": 1 }));
    println!(
        "  cancel: stopped after {} window(s) in {:.2} s",
        WINDOWS.load(Ordering::SeqCst),
        began.elapsed().as_secs_f32()
    );
    assert!(msg.contains("cancelled by the user"), "{msg}");
    assert_eq!(
        WINDOWS.load(Ordering::SeqCst),
        1,
        "the second window never started"
    );
    assert_eq!(fresh.store.list_nodes().unwrap().len(), nodes, "no node");
}
