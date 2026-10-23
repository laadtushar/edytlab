//! Cutting by words on a track whose rate is not the session's.
//!
//! A clip's `start_in_track`, `source_offset` and `length` are counted in the
//! frames of its own source file, not the session's (#234). `cut_words` and
//! `remove_fillers` turn a word's seconds into frames, so they have to use the
//! clip's rate: with the session's, a 16 kHz voice track under a 44.1 kHz or
//! 48 kHz first track had its audio cut at the wrong place, and past the end
//! of the clip entirely for anything later than a third of the way in, so the
//! rest of the take was thrown away instead of the words. Found by driving the
//! v0.8.0 build: transcribe a 16 kHz track that is the second one loaded.

use std::path::{Path, PathBuf};

use hound::{SampleFormat, WavSpec, WavWriter};
use serde_json::{json, Value};
use tempfile::TempDir;
use tools::{ToolContext, ToolDispatcher, ToolResult};

/// The first track's rate, which is the session's.
const SESSION_RATE: u32 = 48_000;
/// The voice track's own rate.
const VOICE_RATE: u32 = 16_000;
const VOICE_SECONDS: usize = 4;

fn write_sine(path: &Path, rate: u32, seconds: usize) -> PathBuf {
    let spec = WavSpec {
        channels: 1,
        sample_rate: rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut w = WavWriter::create(path, spec).expect("wav writer");
    for n in 0..(rate as usize * seconds) {
        let t = n as f32 / rate as f32;
        let s = (2.0 * std::f32::consts::PI * 440.0 * t).sin() * 0.25;
        w.write_sample((s * 32_767.0) as i16).unwrap();
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
    /// Track 0 is a 48 kHz bed (so the session is 48 kHz); track 1 is the
    /// 16 kHz voice, four seconds long.
    fn new() -> Self {
        let dir = TempDir::new().expect("tempdir");
        let bed = write_sine(&dir.path().join("bed.wav"), SESSION_RATE, 6);
        let voice = write_sine(&dir.path().join("voice.wav"), VOICE_RATE, VOICE_SECONDS);
        let store = session::Store::open(dir.path()).expect("open store");
        let mut s = Self {
            _dir: dir,
            store,
            engine: audio_engine::Engine::new(),
            dispatcher: ToolDispatcher::default_dispatcher(),
            clipboard: None,
        };
        s.call("load", json!({ "path": bed.to_string_lossy() }));
        s.call("load", json!({ "path": voice.to_string_lossy() }));
        assert_eq!(s.state().sample_rate, SESSION_RATE, "the bed sets the rate");
        assert_eq!(s.state().tracks.len(), 2);
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

    fn state(&self) -> session::SessionState {
        let head = self.store.head().expect("a head");
        self.store.get(head).expect("head node").state
    }

    /// What `transcribe` leaves: words in session seconds, of track 1.
    fn with_transcript(&mut self, words: &[(&str, f32, f32)]) {
        let mut state = self.state();
        let track_id = state.tracks[1].id.clone();
        state.transcript = Some(session::Transcript {
            source: Some(session::TranscriptSource::Track { track: track_id }),
            words: words
                .iter()
                .map(|(text, start, end)| session::TranscriptWord {
                    text: (*text).to_string(),
                    start_s: *start,
                    end_s: *end,
                    confidence: 0.9,
                })
                .collect(),
        });
        let node = session::SessionNode {
            id: session::NodeId([0u8; 32]),
            parent: None,
            created_at: chrono::Utc::now(),
            label: Some("transcript".into()),
            reasoning: None,
            state,
            op: None,
        };
        self.store.append(node).expect("append transcript");
    }

    /// The voice track's clips as (start_in_track, source_offset, length), in
    /// the voice file's own frames.
    fn voice_clips(&self) -> Vec<(u64, u64, u64)> {
        self.state().tracks[1]
            .clips
            .iter()
            .map(|c| (c.start_in_track, c.source_offset, c.length))
            .collect()
    }
}

fn ok(r: ToolResult) -> Value {
    match r {
        ToolResult::Ok(v) => v,
        ToolResult::Error(m) => panic!("expected Ok, got Error({m})"),
    }
}

/// "one two um three": one second each, except the filler, which is half a
/// second, so a cut at the wrong rate lands somewhere else entirely.
fn words() -> Vec<(&'static str, f32, f32)> {
    vec![
        ("one", 0.0, 1.0),
        ("two", 1.0, 2.0),
        ("um", 2.0, 2.5),
        ("three", 2.5, 4.0),
    ]
}

/// Deleting the second word of a 16 kHz track removes seconds 1 to 2 of that
/// file: the head keeps the first second, the tail starts at the file's second
/// 2 and lands at second 1 of the track, both counted in 16 kHz frames.
#[test]
fn cut_words_cuts_a_voice_track_at_its_own_rate() {
    let mut s = Session::new();
    s.with_transcript(&words());
    let bed_before = s.state().tracks[0].clips.clone();

    ok(s.call(
        "cut_words",
        json!({ "from_word": 1, "to_word": 2, "track": 1 }),
    ));

    let r = VOICE_RATE as u64;
    assert_eq!(
        s.voice_clips(),
        vec![(0, 0, r), (r, 2 * r, 2 * r)],
        "the first second stays, then seconds 2 to 4 of the file, closed up"
    );
    assert_eq!(
        s.state().tracks[0].clips,
        bed_before,
        "the bed is untouched"
    );
}

/// A cut late in the take, past the point where the session's frame count
/// would run off the end of a 16 kHz clip, still cuts only the words.
#[test]
fn cut_words_late_in_the_take_keeps_what_follows() {
    let mut s = Session::new();
    s.with_transcript(&words());

    ok(s.call(
        "cut_words",
        json!({ "from_word": 2, "to_word": 3, "track": 1 }),
    ));

    let r = VOICE_RATE as u64;
    // "um" is 2.0 to 2.5 s: the file's first two seconds, then 2.5 s onwards.
    assert_eq!(
        s.voice_clips(),
        vec![(0, 0, 2 * r), (2 * r, 2 * r + r / 2, 3 * r / 2)]
    );
}

/// The session's length shrinks by the seconds removed, in session frames.
#[test]
fn cut_words_shortens_the_session_by_the_seconds_removed() {
    let mut s = Session::new();
    s.with_transcript(&words());
    let before = s.state().length_samples;

    ok(s.call(
        "cut_words",
        json!({ "from_word": 1, "to_word": 2, "track": 1 }),
    ));

    assert_eq!(
        s.state().length_samples,
        before - SESSION_RATE as u64,
        "one second, counted at the session's rate"
    );
}

/// `remove_fillers` makes the same conversion.
#[test]
fn remove_fillers_cuts_a_voice_track_at_its_own_rate() {
    let mut s = Session::new();
    s.with_transcript(&words());
    let bed_before = s.state().tracks[0].clips.clone();

    let v = ok(s.call(
        "remove_fillers",
        json!({ "apply": true, "words": ["um"], "keep_gap_ms": 100, "track": 1 }),
    ));
    assert_eq!(v["found"], json!(1));

    // "um" runs 2.0 to 2.5 s; with 0.1 s left, the cut is 2.0 to 2.4 s.
    let r = VOICE_RATE as u64;
    assert_eq!(
        s.voice_clips(),
        vec![(0, 0, 2 * r), (2 * r, 2 * r + 2 * r / 5, 8 * r / 5)]
    );
    assert_eq!(
        s.state().tracks[0].clips,
        bed_before,
        "the bed is untouched"
    );
}

/// With no `track`, they act on the transcript's track, at that track's rate.
#[test]
fn the_transcripts_track_is_the_default_and_keeps_its_rate() {
    let mut s = Session::new();
    s.with_transcript(&words());

    ok(s.call("cut_words", json!({ "from_word": 1, "to_word": 2 })));

    let r = VOICE_RATE as u64;
    assert_eq!(s.voice_clips(), vec![(0, 0, r), (r, 2 * r, 2 * r)]);
}
