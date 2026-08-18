//! Labels name moments in the recording, not offsets in a file (#203).
//!
//! Cutting thirty seconds out of the middle and leaving the chapter
//! marks where they were is a silent corruption: the file still opens,
//! every label still has a name, and every one of them after the cut
//! now points thirty seconds wrong. Nobody finds out until the episode
//! ships and chapter three lands mid-sentence.
//!
//! So the tests here are about the thing a listener would notice — does
//! the mark still land on the words it was named for — rather than
//! about arithmetic on a struct.

use std::path::{Path, PathBuf};

use hound::{SampleFormat, WavSpec, WavWriter};
use serde_json::{json, Value};
use tempfile::TempDir;
use tools::{ToolContext, ToolDispatcher, ToolResult};

const SAMPLE_RATE: u32 = 48_000;

fn write_sine(path: &Path, seconds: usize) -> PathBuf {
    let spec = WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut w = WavWriter::create(path, spec).expect("wav writer");
    for n in 0..(SAMPLE_RATE as usize * seconds) {
        let t = n as f32 / SAMPLE_RATE as f32;
        w.write_sample(((2.0 * std::f32::consts::PI * 440.0 * t).sin() * 8000.0) as i16)
            .unwrap();
    }
    w.finalize().unwrap();
    path.to_path_buf()
}

struct Session {
    _dir: TempDir,
    store: session::Store,
    engine: audio_engine::Engine,
    dispatcher: ToolDispatcher,
    clipboard: Option<Vec<f32>>,
}

impl Session {
    fn new() -> Self {
        let dir = TempDir::new().expect("tempdir");
        let wav = write_sine(&dir.path().join("episode.wav"), 30);
        let store = session::Store::open(dir.path()).expect("open store");
        let mut s = Self {
            _dir: dir,
            store,
            engine: audio_engine::Engine::new(),
            dispatcher: ToolDispatcher::default_dispatcher(),
            clipboard: None,
        };
        s.call("load", json!({ "path": wav.to_string_lossy() }));
        s
    }

    fn call(&mut self, tool: &str, args: Value) -> ToolResult {
        let mut ctx = ToolContext {
            store: &mut self.store,
            engine: &mut self.engine,
            user_message: "",
            clipboard: &mut self.clipboard,
        };
        self.dispatcher.invoke(tool, args, &mut ctx).unwrap()
    }

    fn mark(&mut self, name: &str, at: f64) {
        ok(self.call("label", json!({ "name": name, "time": at })));
    }

    /// The label lane as (name, start, end) in seconds.
    fn labels(&self) -> Vec<(String, f64, f64)> {
        let head = self.store.head().expect("head");
        let state = self.store.get(head).expect("node").state;
        state
            .annotations
            .iter()
            .map(|a| match a.kind {
                session::AnnotationKind::Marker { time_sec } => {
                    (a.name.clone(), time_sec, time_sec)
                }
                session::AnnotationKind::Region { start_sec, end_sec } => {
                    (a.name.clone(), start_sec, end_sec)
                }
            })
            .collect()
    }

    fn at(&self, name: &str) -> f64 {
        self.labels()
            .into_iter()
            .find(|(n, _, _)| n == name)
            .unwrap_or_else(|| panic!("no label named {name}: {:?}", self.labels()))
            .1
    }
}

fn ok(r: ToolResult) -> Value {
    match r {
        ToolResult::Ok(v) => v,
        ToolResult::Error(m) => panic!("expected Ok, got Error({m})"),
    }
}

const SR: u64 = SAMPLE_RATE as u64;

/// The case the ticket is about: a cut earlier in the episode moves
/// everything after it, chapter marks included.
#[test]
fn a_cut_pulls_the_later_chapters_back_with_it() {
    let mut s = Session::new();
    s.mark("intro", 2.0);
    s.mark("chapter two", 12.0);
    s.mark("outro", 25.0);

    // Remove five seconds from 4s–9s.
    let v = ok(s.call(
        "cut_range",
        json!({ "track": 0, "start_sample": 4 * SR, "end_sample": 9 * SR }),
    ));
    assert_eq!(v["dropped_labels"], json!(0), "{v}");

    assert_eq!(s.at("intro"), 2.0, "before the cut, so it has not moved");
    assert_eq!(s.at("chapter two"), 7.0, "12s minus the five that went");
    assert_eq!(s.at("outro"), 20.0);
}

/// A mark inside the removed span named a moment that is not in the
/// recording any more. Dropping it is the honest outcome — and saying
/// so is the point, because it is user-authored text.
#[test]
fn a_mark_inside_the_cut_is_dropped_and_reported() {
    let mut s = Session::new();
    s.mark("keep me", 2.0);
    s.mark("this bit gets cut", 6.0);

    let v = ok(s.call(
        "cut_range",
        json!({ "track": 0, "start_sample": 4 * SR, "end_sample": 9 * SR }),
    ));

    assert_eq!(v["dropped_labels"], json!(1), "the tool has to say so: {v}");
    let names: Vec<String> = s.labels().into_iter().map(|(n, _, _)| n).collect();
    assert_eq!(names, vec!["keep me"]);
}

/// A region that starts before the cut still starts where it did; only
/// the part that was inside disappears.
#[test]
fn a_region_spanning_the_cut_is_clipped_not_dropped() {
    let mut s = Session::new();
    ok(s.call(
        "import_labels",
        json!({ "labels_text": "3.0\t15.0\tthe long segment" }),
    ));

    ok(s.call(
        "cut_range",
        json!({ "track": 0, "start_sample": 5 * SR, "end_sample": 10 * SR }),
    ));

    let (name, start, end) = s.labels().into_iter().next().expect("the region survives");
    assert_eq!(name, "the long segment");
    assert_eq!(start, 3.0, "it began before the cut, so it still does");
    assert_eq!(end, 10.0, "and it is five seconds shorter");
}

/// Inserting silence pushes everything after it later.
#[test]
fn an_insert_pushes_the_later_chapters_along() {
    let mut s = Session::new();
    s.mark("intro", 1.0);
    s.mark("chapter two", 10.0);

    ok(s.call(
        "insert_silence",
        json!({ "track": 0, "at": 5.0, "duration": 2.0 }),
    ));

    assert_eq!(s.at("intro"), 1.0, "before the splice");
    assert_eq!(s.at("chapter two"), 12.0, "after it, by exactly the gap");
}

/// Cutting words is cutting audio, so the labels follow that too.
#[test]
fn cutting_words_moves_the_labels_as_well() {
    let mut s = Session::new();
    // A transcript whose second and third words span 4s–8s.
    let mut state = {
        let head = s.store.head().expect("head");
        s.store.get(head).expect("node").state
    };
    state.transcript = Some(session::Transcript {
        words: [
            ("one", 1.0, 2.0),
            ("two", 4.0, 6.0),
            ("three", 6.0, 8.0),
            ("four", 12.0, 13.0),
        ]
        .into_iter()
        .map(|(t, a, b)| session::TranscriptWord {
            text: t.into(),
            start_s: a,
            end_s: b,
            confidence: 0.9,
        })
        .collect(),
    });
    s.store
        .append(session::SessionNode {
            id: session::NodeId([0u8; 32]),
            parent: None,
            created_at: chrono::Utc::now(),
            label: Some("transcript".into()),
            reasoning: None,
            state,
            op: None,
        })
        .expect("append");

    s.mark("later", 20.0);
    ok(s.call(
        "cut_words",
        json!({ "track": 0, "from_word": 1, "to_word": 3 }),
    ));

    // Words 1..3 run 4s–8s, so four seconds came out.
    assert_eq!(s.at("later"), 16.0);
}

/// The round trip the acceptance asks for: what `export_labels` writes
/// after an edit is the moved positions, not the old ones.
#[test]
fn the_export_reflects_where_the_labels_actually_are() {
    let mut s = Session::new();
    s.mark("chapter two", 12.0);

    ok(s.call(
        "cut_range",
        json!({ "track": 0, "start_sample": 2 * SR, "end_sample": 6 * SR }),
    ));

    let v = ok(s.call("export_labels", json!({})));
    let text = v["labels"].as_str().unwrap_or_default();
    // Audacity format is `start TAB end TAB name`; the writer trims a
    // trailing `.0`, so match on the field rather than on a rendering.
    let start = text.split('\t').next().unwrap_or_default();
    assert_eq!(start, "8", "the export should say 8s, not 12s: {text:?}");
}

/// Nothing changes for a session with no labels — this must not be a
/// tax every edit pays visibly.
#[test]
fn an_edit_with_no_labels_is_unaffected() {
    let mut s = Session::new();
    let v = ok(s.call(
        "cut_range",
        json!({ "track": 0, "start_sample": 0, "end_sample": SR }),
    ));
    assert_eq!(v["dropped_labels"], json!(0));
    assert!(s.labels().is_empty());
}
