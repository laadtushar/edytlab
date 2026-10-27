//! The `diarize` tool on the real speaker model (#168). `#[ignore]`d: it needs
//! what CI does not have.
//!
//! * the pinned `speaker-diarization` model, as the **folder** of its two
//!   networks (`DIARIZE_MODEL_PATH`; `crates/ml-pipeline/models.json` pins them;
//!   `scripts/fetch-ml-model.sh speaker-diarization <dir>` makes the folder), and
//! * the ONNX Runtime library (`ORT_DYLIB_PATH`;
//!   `scripts/fetch-ml-runtime.sh <platform> <dir>` prints it).
//!
//! ```text
//! export ORT_DYLIB_PATH=/path/to/libonnxruntime.so
//! export DIARIZE_MODEL_PATH=/path/to/models/speaker-diarization
//! cargo test -p tools --release --test diarize_end_to_end -- --ignored --nocapture
//! ```
//!
//! Asked for with `--ignored` it **fails** when either is missing, and says
//! which, rather than returning early: an `#[ignore]`d test that passes without
//! having run is the vacuous pass #263 is about.
//!
//! `ml-diarize`'s `real_model.rs` holds the networks and the algorithm to
//! pyannote.audio's; this holds the tool around them: the audio read through
//! `load` and a track, the answer, that nothing is written to the session, and
//! the cache.
//!
//! The recording is the one that test uses, not a copy of it:
//! `crates/ml-diarize/tests/fixtures/ami_es2004a_640_680s.wav`, 40 s of the AMI
//! Meeting Corpus (CC BY 4.0, `AMI-LICENSE.md` beside it) in which two people
//! talk and interrupt each other, with the corpus' hand annotation (`.rttm`).
//!
//! One test, in phases, because the environment and the cancel switch are
//! process-wide:
//!
//! 1. `num_speakers = 2` through `track`: turns come back, two speakers, in
//!    order and inside the recording, and the talk time is near what the
//!    annotation says (the diarization error rate is `real_model.rs`'s to
//!    bound; this only catches a tool that returns the wrong thing);
//! 2. the tool is read-only: no node, the same head;
//! 3. the same request again is answered from the cache, fast, with the same
//!    turns, and the cache holds one entry.
//!
//! The numbers are printed (`--nocapture`).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use session::Store;
use tools::{ToolContext, ToolDispatcher, ToolResult};

/// Talk time found, against the annotation's speech (the union of its turns,
/// 33.1 s). Overlap goes to one speaker, and the pinned model is 13 to 15 %
/// off by `real_model.rs`'s measure, so this is wide on purpose.
const TALK_RATIO: std::ops::RangeInclusive<f64> = 0.75..=1.25;
/// Each of the two people speaks for at least this long in the annotation
/// (14 s and 23 s).
const MIN_PER_SPEAKER_S: f64 = 5.0;
const RECORDING_S: f64 = 40.0;

/// Abort, rather than stall for ever, if the run is stuck.
fn watchdog() {
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(900));
        eprintln!("diarize_end_to_end ran for 15 minutes: it is stuck, not slow");
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
        .join("../ml-diarize/tests/fixtures")
        .join(name)
}

/// The seconds in which anybody speaks in the human annotation (the union of
/// its turns), and each speaker's own seconds, from the `.rttm`.
fn annotated_speech() -> (f64, Vec<f64>) {
    let text = std::fs::read_to_string(fixture("ami_es2004a_640_680s.rttm")).expect("the rttm");
    let mut names: Vec<String> = Vec::new();
    let mut spans: Vec<(f64, f64)> = Vec::new();
    let mut per_speaker: Vec<f64> = Vec::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let f: Vec<&str> = line.split_whitespace().collect();
        let (start, len): (f64, f64) = (f[3].parse().unwrap(), f[4].parse().unwrap());
        let k = names.iter().position(|n| n == f[7]).unwrap_or_else(|| {
            names.push(f[7].to_string());
            per_speaker.push(0.0);
            names.len() - 1
        });
        per_speaker[k] += len;
        spans.push((start, start + len));
    }
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut union = 0.0;
    let mut reach = f64::MIN;
    for (s, e) in spans {
        let s = s.max(reach);
        if e > s {
            union += e - s;
        }
        reach = reach.max(e);
    }
    (union, per_speaker)
}

struct Session {
    _dir: tempfile::TempDir,
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
            _dir: dir,
            store,
            engine: audio_engine::Engine::new(),
            clipboard: None,
            // The real dispatcher: the real `diarize`, on the real model.
            dispatcher: ToolDispatcher::default_dispatcher(),
        }
    }

    fn ok(&mut self, tool: &str, args: Value) -> Value {
        let mut ctx = ToolContext {
            store: &mut self.store,
            engine: &mut self.engine,
            user_message: "",
            clipboard: &mut self.clipboard,
            allowed_tools: None,
        };
        match self
            .dispatcher
            .invoke(tool, args, &mut ctx)
            .unwrap_or_else(|e| panic!("{tool}: dispatcher error {e}"))
        {
            ToolResult::Ok(v) => v,
            ToolResult::Error(m) => panic!("`{tool}` failed: {m}"),
        }
    }

    fn cache_entries(&self) -> usize {
        std::fs::read_dir(self.store.project_dir().join(".audiograph/inference-cache"))
            .map(|d| d.count())
            .unwrap_or(0)
    }
}

/// `[start, end, "speaker_N"]` triples out of the tool's `turns`.
fn turns_of(answer: &Value) -> Vec<(f64, f64, String)> {
    answer["turns"]
        .as_array()
        .expect("the tool listed its turns")
        .iter()
        .map(|t| {
            (
                t[0].as_f64().unwrap(),
                t[1].as_f64().unwrap(),
                t[2].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

#[test]
#[ignore = "needs ORT_DYLIB_PATH and DIARIZE_MODEL_PATH (the model folder); run with --ignored"]
fn the_tool_finds_two_speakers_in_the_ami_excerpt_changes_nothing_and_caches() {
    watchdog();
    required(
        "ORT_DYLIB_PATH",
        "scripts/fetch-ml-runtime.sh <platform> <dir> prints the library to point it at.",
        false,
    );
    required(
        "DIARIZE_MODEL_PATH",
        "It names the folder with segmentation/model.onnx and embedding/model.onnx, the files \
         crates/ml-pipeline/models.json pins (scripts/fetch-ml-model.sh speaker-diarization <dir>).",
        true,
    );
    let (speech_s, per_speaker_s) = annotated_speech();
    println!("diarize_end_to_end:");
    println!("  annotation: {speech_s:.2} s of speech, per speaker {per_speaker_s:?}");

    // The recording goes in through `load`, as a user's would.
    let mut s = Session::new();
    let wav = fixture("ami_es2004a_640_680s.wav");
    s.ok("load", json!({ "path": wav.to_string_lossy() }));
    let nodes = s.store.list_nodes().unwrap().len();
    let head = s.store.head();
    assert_eq!(s.cache_entries(), 0, "`load` leaves nothing in the cache");

    // 1. The turns.
    let began = Instant::now();
    let first = s.ok("diarize", json!({ "track": 0, "num_speakers": 2 }));
    let took = began.elapsed();
    println!(
        "  first request: {:.2} s for {RECORDING_S:.0} s of audio (realtime factor {:.3}, includes \
         loading the model)",
        took.as_secs_f32(),
        took.as_secs_f64() / RECORDING_S
    );
    assert_eq!(first["cached"], false, "{first}");
    assert_eq!(first["truncated"], false, "{first}");
    assert_eq!(first["speakers"], 2, "{first}");
    let turns = turns_of(&first);
    assert!(!turns.is_empty(), "no turns: {first}");
    assert_eq!(
        turns.len() as u64,
        first["turn_count"].as_u64().unwrap(),
        "all the turns are listed"
    );
    let duration = first["duration_sec"].as_f64().unwrap();
    assert!((duration - RECORDING_S).abs() < 0.05, "duration {duration}");
    let mut labels: Vec<&str> = turns.iter().map(|t| t.2.as_str()).collect();
    labels.sort_unstable();
    labels.dedup();
    assert_eq!(labels, ["speaker_1", "speaker_2"], "{first}");
    for (start, end, label) in &turns {
        assert!(
            0.0 <= *start && start < end && *end <= duration + 0.05,
            "turn [{start}, {end}] {label} is not inside the {duration} s recording"
        );
    }
    assert!(
        turns.windows(2).all(|p| p[0].0 <= p[1].0),
        "turns are in time order"
    );

    // Talk time: the tool's own sums agree with its turns, and are near the
    // annotation's.
    let talk = first["talk_time"].as_array().expect("talk_time");
    assert_eq!(talk.len(), 2);
    let mut total = 0.0;
    for entry in talk {
        let label = entry["speaker"].as_str().unwrap();
        let said = entry["talk_sec"].as_f64().unwrap();
        let from_turns: f64 = turns
            .iter()
            .filter(|t| t.2 == label)
            .map(|t| t.1 - t.0)
            .sum();
        assert!(
            (said - from_turns).abs() < 0.1,
            "{label}: talk_time says {said:.2} s, its turns add up to {from_turns:.2} s"
        );
        assert!(
            said >= MIN_PER_SPEAKER_S,
            "{label} talks for only {said:.2} s; the annotation has both people talking for \
             {MIN_PER_SPEAKER_S} s or more"
        );
        total += said;
    }
    println!(
        "  found {} turns, {total:.2} s of talk ({:.0} % of the annotation's speech)",
        turns.len(),
        total / speech_s * 100.0
    );
    assert!(
        TALK_RATIO.contains(&(total / speech_s)),
        "{total:.2} s of talk against {speech_s:.2} s annotated; allowed {:.0} % to {:.0} %",
        TALK_RATIO.start() * 100.0,
        TALK_RATIO.end() * 100.0
    );

    // 2. Read-only: no node, the same head.
    assert_eq!(s.store.list_nodes().unwrap().len(), nodes, "no node");
    assert_eq!(s.store.head(), head, "the head did not move");

    // 3. Asked again, the cache answers.
    assert_eq!(s.cache_entries(), 1);
    let began = Instant::now();
    let second = s.ok("diarize", json!({ "track": 0, "num_speakers": 2 }));
    let hit = began.elapsed();
    println!(
        "  second request (cache): {:.3} s; first took {:.2} s",
        hit.as_secs_f32(),
        took.as_secs_f32()
    );
    assert_eq!(second["cached"], true, "{second}");
    assert_eq!(second["turns"], first["turns"]);
    assert_eq!(second["talk_time"], first["talk_time"]);
    assert!(
        hit < took / 2,
        "the cache saved nothing: {hit:?} against {took:?}"
    );
    assert_eq!(s.cache_entries(), 1, "the repeat added no entry");
    assert_eq!(s.store.list_nodes().unwrap().len(), nodes, "still no node");
    assert_eq!(s.store.head(), head);
}
