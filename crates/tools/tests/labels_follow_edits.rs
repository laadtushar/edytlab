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

/// tone / silence / tone / silence / tone, one second each.
///
/// `truncate_silence` needs actual silence to find; the 30s sine the
/// other tests use has none, so those tests would pass whatever the
/// remap did.
fn write_tone_islands(path: &Path) -> PathBuf {
    let spec = WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut w = WavWriter::create(path, spec).expect("wav writer");
    for band in 0..5 {
        let silent = band % 2 == 1;
        for n in 0..SAMPLE_RATE as usize {
            if silent {
                w.write_sample(0i16).unwrap();
            } else {
                let t = n as f32 / SAMPLE_RATE as f32;
                w.write_sample(((2.0 * std::f32::consts::PI * 440.0 * t).sin() * 8000.0) as i16)
                    .unwrap();
            }
        }
    }
    w.finalize().unwrap();
    path.to_path_buf()
}

struct Session {
    _dir: TempDir,
    store: session::Store,
    engine: audio_engine::Engine,
    dispatcher: ToolDispatcher,
    clipboard: Option<tools::Clipboard>,
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

    /// A session whose track has real silence in it, for
    /// `truncate_silence`.
    fn silent_with_tone_islands() -> Self {
        let dir = TempDir::new().expect("tempdir");
        let wav = write_tone_islands(&dir.path().join("islands.wav"));
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
            allowed_tools: None,
        };
        self.dispatcher.invoke(tool, args, &mut ctx).unwrap()
    }

    fn mark(&mut self, name: &str, at: f64) {
        ok(self.call("label", json!({ "name": name, "time": at })));
    }

    /// The label lane as (name, start, end) in seconds.
    /// Seed a transcript on the current head.
    ///
    /// The transcript is the second time-addressed record in the state
    /// and the one no length-changing tool used to move (#231). Tests
    /// that assert about it need one to exist first.
    fn seed_transcript(&mut self, words: &[(&str, f32, f32)]) {
        let mut state = {
            let head = self.store.head().expect("head");
            self.store.get(head).expect("node").state
        };
        state.transcript = Some(session::Transcript {
            words: words
                .iter()
                .map(|(t, a, b)| session::TranscriptWord {
                    text: (*t).into(),
                    start_s: *a,
                    end_s: *b,
                    confidence: 0.9,
                })
                .collect(),
        });
        self.store
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
    }

    /// The transcript at the current head, as `(text, start, end)`.
    fn words(&self) -> Vec<(String, f32, f32)> {
        let head = self.store.head().expect("head");
        self.store
            .get(head)
            .expect("node")
            .state
            .transcript
            .map(|t| {
                t.words
                    .into_iter()
                    .map(|w| (w.text, w.start_s, w.end_s))
                    .collect()
            })
            .unwrap_or_default()
    }

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

/// The boundaries, pinned deliberately rather than left to whatever the
/// comparison operators happened to do.
///
/// A mark placed exactly at the edge of a cut is naming the edit, and
/// the seam still exists afterwards — so it survives at the seam rather
/// than being deleted for a position that is still there.
#[test]
fn a_mark_on_the_cut_boundary_survives_at_the_seam() {
    let mut s = Session::new();
    s.mark("at the start", 4.0);
    s.mark("at the end", 9.0);

    let v = ok(s.call(
        "cut_range",
        json!({ "track": 0, "start_sample": 4 * SR, "end_sample": 9 * SR }),
    ));

    assert_eq!(v["dropped_labels"], json!(0), "neither is inside: {v}");
    assert_eq!(s.at("at the start"), 4.0);
    assert_eq!(s.at("at the end"), 4.0, "the far edge closes onto the seam");
}

/// A region that merely *ends* where silence is inserted does not
/// stretch: regions are half-open, so the silence lands after it and
/// the passage the user marked is no longer than it was.
#[test]
fn a_region_ending_at_the_insert_point_does_not_stretch() {
    let mut s = Session::new();
    ok(s.call(
        "import_labels",
        json!({ "labels_text": "1.0\t5.0\tcold open" }),
    ));

    ok(s.call(
        "insert_silence",
        json!({ "track": 0, "at": 5.0, "duration": 2.0 }),
    ));

    let (_, start, end) = s.labels().into_iter().next().expect("the region");
    assert_eq!(start, 1.0);
    assert_eq!(end, 5.0, "it ends where it did — the silence is after it");
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

// =============================================================================
// The transcript follows the audio too (#231)
// =============================================================================
//
// Labels got this treatment in #203. The transcript — the other
// time-addressed record in the state — did not, and it is the more
// dangerous of the two: `cut_words`, `select_region` and
// `duck_under_speech` all turn a word's `start_s` back into a sample
// offset and edit there. A transcript describing the timeline as it was
// before an edit therefore makes the *next* text edit destroy audio
// somewhere else, while reporting the word it meant to remove.

/// The headline regression: a cut, then a word edit.
///
/// Before the fix, `cut_range` remapped the labels and left the words
/// alone. `omega` still read 3.0–3.5s after two seconds had come out of
/// the middle, so `cut_words` removed [3.0, 3.5) — audio a full second
/// away from the word it named — and reported success with the summary
/// "The remaining word timings were shifted to match."
#[test]
fn a_word_edit_after_a_cut_removes_the_audio_the_word_actually_occupies() {
    let mut s = Session::new();
    s.seed_transcript(&[("alpha", 0.0, 0.5), ("omega", 3.0, 3.5)]);

    // Two seconds out of the middle: [1.0, 3.0).
    ok(s.call(
        "cut_range",
        json!({
            "track": 0,
            "start_sample": SAMPLE_RATE as u64,
            "end_sample": 3 * SAMPLE_RATE as u64,
        }),
    ));

    // omega's audio is now at 1.0–1.5s, and the transcript must say so.
    let words = s.words();
    assert_eq!(words.len(), 2, "no word was inside the cut");
    assert_eq!(words[1].0, "omega");
    assert!(
        (words[1].1 - 1.0).abs() < 1e-4,
        "omega should have moved to 1.0s, reads {}",
        words[1].1
    );

    // And the word edit that follows must cut where omega now lives.
    let out = ok(s.call(
        "cut_words",
        json!({ "track": 0, "from_word": 1, "to_word": 2 }),
    ));
    assert_eq!(out["removed_text"], "omega");
    assert!(
        (out["start_sec"].as_f64().unwrap() - 1.0).abs() < 1e-4,
        "cut_words removed [{}, {}) — the pre-cut position, not omega's",
        out["start_sec"],
        out["end_sec"],
    );
}

#[test]
fn a_cut_pulls_the_later_words_back_with_it() {
    let mut s = Session::new();
    s.seed_transcript(&[("before", 0.2, 0.8), ("after", 10.0, 10.5)]);
    ok(s.call(
        "cut_range",
        json!({
            "track": 0,
            "start_sample": 2 * SAMPLE_RATE as u64,
            "end_sample": 5 * SAMPLE_RATE as u64,
        }),
    ));
    let words = s.words();
    assert!(
        (words[0].1 - 0.2).abs() < 1e-4,
        "a word before the cut moved"
    );
    assert!(
        (words[1].1 - 7.0).abs() < 1e-4,
        "a word after a 3s cut should be 3s earlier, reads {}",
        words[1].1
    );
}

#[test]
fn a_word_inside_the_cut_is_dropped() {
    let mut s = Session::new();
    s.seed_transcript(&[("keep", 0.0, 0.5), ("gone", 3.0, 3.5), ("keep2", 8.0, 8.5)]);
    ok(s.call(
        "cut_range",
        json!({
            "track": 0,
            "start_sample": 2 * SAMPLE_RATE as u64,
            "end_sample": 5 * SAMPLE_RATE as u64,
        }),
    ));
    let texts: Vec<String> = s.words().into_iter().map(|w| w.0).collect();
    assert_eq!(texts, vec!["keep", "keep2"]);
}

#[test]
fn an_insert_pushes_the_later_words_along() {
    let mut s = Session::new();
    s.seed_transcript(&[("early", 0.5, 1.0), ("late", 6.0, 6.5)]);
    ok(s.call(
        "insert_silence",
        json!({ "track": 0, "at": 3.0, "duration": 2.0 }),
    ));
    let words = s.words();
    assert!(
        (words[0].1 - 0.5).abs() < 1e-4,
        "a word before the insert moved"
    );
    assert!(
        (words[1].1 - 8.0).abs() < 1e-4,
        "a word after a 2s insert should be 2s later, reads {}",
        words[1].1
    );
}

// -----------------------------------------------------------------------------
// trim moved neither record
// -----------------------------------------------------------------------------

#[test]
fn trim_rebases_labels_onto_the_kept_window() {
    let mut s = Session::new();
    s.mark("chapter two", 12.0);
    ok(s.call(
        "trim",
        json!({
            "track": 0,
            "start_sample": 10 * SAMPLE_RATE as u64,
            "end_sample": 20 * SAMPLE_RATE as u64,
        }),
    ));
    // The window starts at 10s, so a mark at 12s is 2s into the result.
    assert!(
        (s.at("chapter two") - 2.0).abs() < 1e-4,
        "expected 2.0 after trimming to [10, 20), got {}",
        s.at("chapter two")
    );
}

#[test]
fn trim_drops_labels_outside_the_window_and_reports_them() {
    let mut s = Session::new();
    s.mark("kept", 12.0);
    s.mark("before the window", 2.0);
    s.mark("after the window", 25.0);
    let out = ok(s.call(
        "trim",
        json!({
            "track": 0,
            "start_sample": 10 * SAMPLE_RATE as u64,
            "end_sample": 20 * SAMPLE_RATE as u64,
        }),
    ));
    let names: Vec<String> = s.labels().into_iter().map(|l| l.0).collect();
    assert_eq!(names, vec!["kept".to_string()]);
    assert_eq!(
        out["dropped_labels"], 2,
        "trim discarded two labels without saying so"
    );
}

#[test]
fn trim_rebases_the_transcript_onto_the_kept_window() {
    let mut s = Session::new();
    s.seed_transcript(&[("outside", 2.0, 2.5), ("inside", 12.0, 12.5)]);
    ok(s.call(
        "trim",
        json!({
            "track": 0,
            "start_sample": 10 * SAMPLE_RATE as u64,
            "end_sample": 20 * SAMPLE_RATE as u64,
        }),
    ));
    let words = s.words();
    assert_eq!(words.len(), 1, "the word outside the window survived");
    assert_eq!(words[0].0, "inside");
    assert!(
        (words[0].1 - 2.0).abs() < 1e-4,
        "expected 2.0 after trimming to [10, 20), got {}",
        words[0].1
    );
}

// =============================================================================
// The tools that route through destructive_edit_then (#276)
// =============================================================================
//
// These four could not be fixed with #275's helpers: the edit closure is
// an opaque buffer mutation, so it cannot say what it removed, and the
// result JSON was fixed at `node_id` + `summary`, so a dropped count had
// nowhere to go. Both are now plumbed.

/// Speeding up re-times the whole recording — output duration is input
/// divided by the factor — so every mark moves by the same ratio.
#[test]
fn change_speed_rescales_labels_and_words() {
    let mut s = Session::new();
    s.mark("halfway", 10.0);
    s.seed_transcript(&[("word", 10.0, 11.0)]);
    ok(s.call("change_speed", json!({ "track": 0, "factor": 2.0 })));

    assert!(
        (s.at("halfway") - 5.0).abs() < 1e-3,
        "at double speed a 10s mark belongs at 5s, got {}",
        s.at("halfway")
    );
    let words = s.words();
    assert!(
        (words[0].1 - 5.0).abs() < 1e-3,
        "at double speed a 10s word belongs at 5s, got {}",
        words[0].1
    );
}

#[test]
fn slowing_down_pushes_labels_later() {
    let mut s = Session::new();
    s.mark("halfway", 5.0);
    ok(s.call("change_speed", json!({ "track": 0, "factor": 0.5 })));
    assert!(
        (s.at("halfway") - 10.0).abs() < 1e-3,
        "at half speed a 5s mark belongs at 10s, got {}",
        s.at("halfway")
    );
}

#[test]
fn time_stretch_rescales_labels_and_words() {
    let mut s = Session::new();
    s.mark("halfway", 10.0);
    s.seed_transcript(&[("word", 10.0, 11.0)]);
    ok(s.call(
        "time_stretch",
        json!({ "track": 0, "factor": 2.0, "preserve_formants": false }),
    ));
    assert!(
        (s.at("halfway") - 5.0).abs() < 1e-3,
        "factor 2.0 halves the duration, so a 10s mark belongs at 5s, got {}",
        s.at("halfway")
    );
    let words = s.words();
    assert!((words[0].1 - 5.0).abs() < 1e-3, "word not rescaled");
}

/// `truncate_silence` removes many spans at once, so the remap has to
/// run back to front — each cut renumbers everything after it.
#[test]
fn truncate_silence_pulls_labels_back_past_every_removed_gap() {
    let mut s = Session::silent_with_tone_islands();
    s.mark("after the gaps", 9.0);
    let out = ok(s.call(
        "truncate_silence",
        json!({ "track": 0, "threshold_db": -40.0, "min_silence_ms": 100.0 }),
    ));

    // The fixture is tone/silence/tone/silence/tone, one second each, so
    // two seconds of silence come out and a mark at 9s lands at 7s.
    let moved = s.at("after the gaps");
    assert!(
        moved < 9.0,
        "the mark did not move at all — {moved} is where it started"
    );
    assert!(
        (moved - 7.0).abs() < 0.2,
        "expected ~7.0 after two 1s gaps came out, got {moved}"
    );
    // Nothing was marked inside a gap here, so no count is reported.
    assert!(out.get("dropped_labels").is_none());
}

#[test]
fn truncate_silence_reports_a_label_it_removed() {
    let mut s = Session::silent_with_tone_islands();
    s.mark("inside the first gap", 1.5);
    let out = ok(s.call(
        "truncate_silence",
        json!({ "track": 0, "threshold_db": -40.0, "min_silence_ms": 100.0 }),
    ));
    assert_eq!(
        out["dropped_labels"], 1,
        "a mark inside removed silence was discarded without saying so"
    );
}

/// The test that actually pins the ordering.
///
/// A mark *past* both gaps lands at the same place whichever direction
/// the spans are applied in, so it cannot tell a correct implementation
/// from a broken one — the first version of this file made exactly that
/// mistake and a forwards-iterating mutation passed it.
///
/// A mark inside the **second** gap does distinguish them. The spans are
/// recorded in pre-edit coordinates: applied back to front, `[3s, 4s)`
/// still names the second gap and the mark inside it is dropped. Applied
/// front to back, the first cut has already pulled that mark to 2.5s, so
/// `[3s, 4s)` no longer contains it and it survives at a position where
/// there is no longer any audio.
#[test]
fn truncate_silence_applies_removed_spans_back_to_front() {
    let mut s = Session::silent_with_tone_islands();
    s.mark("inside the second gap", 3.5);
    let out = ok(s.call(
        "truncate_silence",
        json!({ "track": 0, "threshold_db": -40.0, "min_silence_ms": 100.0 }),
    ));
    assert_eq!(
        out["dropped_labels"], 1,
        "the mark inside the second gap survived, so the spans were \
         applied in the wrong order: the earlier cut shifted it out of \
         the later span before that span was used"
    );
    assert!(
        s.labels().is_empty(),
        "the dropped mark is still present: {:?}",
        s.labels()
    );
}

/// Not every length-changing tool needs a remap, and asserting that is
/// as much a part of the contract as the shifts above.
///
/// The issue listed `repeat_selection` alongside the others. It is not
/// affected: `apply_repeat` **appends** the copies to the end of the
/// buffer rather than splicing them at the selection, so nothing that
/// already exists moves. Remapping here would corrupt correct data.
#[test]
fn repeat_selection_appends_and_so_moves_nothing() {
    let mut s = Session::new();
    s.mark("chapter two", 12.0);
    s.seed_transcript(&[("word", 12.0, 12.5)]);
    ok(s.call(
        "repeat_selection",
        json!({ "track": 0, "start_sec": 1.0, "end_sec": 3.0, "times": 2 }),
    ));
    assert!(
        (s.at("chapter two") - 12.0).abs() < 1e-4,
        "repeat_selection appends at the end; nothing before it may move"
    );
    assert!((s.words()[0].1 - 12.0).abs() < 1e-4);
}
