//! `SessionState::length_samples` on a session whose tracks are at different
//! rates (#678, after #234 and #676).
//!
//! A clip is counted in the frames of its own source file, so a 16 kHz voice
//! and a 48 kHz bed count the same second in different numbers. The session's
//! length is one number, and it is in session frames: the frames of the rate
//! the first load set. Most tools used to write it as the largest clip end in
//! *clip* frames, which compares 16 000-a-second with 48 000-a-second (a
//! voice that runs past the bed read as the shorter of the two), while
//! `cut_words` subtracted session frames from whatever was there. These tests
//! build the mixed session, run each tool that writes the length, and check
//! the number against the seconds the clips really reach, and against the
//! length of a render.

use std::path::{Path, PathBuf};

use hound::{SampleFormat, WavSpec, WavWriter};
use serde_json::{json, Value};
use tempfile::TempDir;
use tools::{ToolContext, ToolDispatcher, ToolResult};

const VOICE_RATE: u32 = 16_000;
/// Common session rates; neither is a multiple of 16 kHz.
const SESSION_RATES: [u32; 2] = [48_000, 44_100];

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
        let v = (2.0 * std::f32::consts::PI * 440.0 * t).sin() * 0.25;
        w.write_sample((v * 32_767.0) as i16).unwrap();
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
    /// Track 0 is a `first_rate` file of `first_secs` seconds (so the session
    /// is at that rate); track 1 is a `second_rate` file of `second_secs`.
    fn new(first: (u32, usize), second: (u32, usize)) -> Self {
        let dir = TempDir::new().expect("tempdir");
        let a = write_sine(&dir.path().join("a.wav"), first.0, first.1);
        let b = write_sine(&dir.path().join("b.wav"), second.0, second.1);
        let store = session::Store::open(dir.path()).expect("open store");
        let mut s = Self {
            _dir: dir,
            store,
            engine: audio_engine::Engine::new(),
            dispatcher: ToolDispatcher::default_dispatcher(),
            clipboard: None,
        };
        s.ok("load", json!({ "path": a.to_string_lossy() }));
        s.ok("load", json!({ "path": b.to_string_lossy() }));
        assert_eq!(
            s.state().sample_rate,
            first.0,
            "the first load sets the rate"
        );
        s
    }

    /// A bed at `rate` and a 16 kHz voice.
    fn bed_and_voice(rate: u32, bed_secs: usize, voice_secs: usize) -> Self {
        Self::new((rate, bed_secs), (VOICE_RATE, voice_secs))
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
            ToolResult::Error(m) => panic!("{tool}: expected Ok, got Error({m})"),
        }
    }

    fn state(&self) -> session::SessionState {
        let head = self.store.head().expect("a head");
        self.store.get(head).expect("head node").state
    }

    fn length(&self) -> u64 {
        self.state().length_samples
    }

    /// Frames a full render of the head writes.
    fn rendered_frames(&self) -> u64 {
        let out = self._dir.path().join("length-check.wav");
        audio_engine::render_state_to_wav(&self.state(), &out, None)
            .expect("render")
            .frames_written
    }

    /// Words of track 1, one second each, as `transcribe` leaves them.
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
}

fn rate_secs(rate: u32, secs: u64) -> u64 {
    u64::from(rate) * secs
}

// ---------------------------------------------------------------------------
// load
// ---------------------------------------------------------------------------

/// A voice that runs past the bed is the longer track, though its frame count
/// is a third of the bed's. The session's length is in session frames.
#[test]
fn load_counts_a_longer_voice_in_session_frames() {
    for rate in SESSION_RATES {
        let s = Session::bed_and_voice(rate, 2, 4);
        assert_eq!(s.length(), rate_secs(rate, 4), "session at {rate}");
        assert_eq!(s.rendered_frames(), s.length(), "session at {rate}");
    }
}

/// And a bed that runs past the voice keeps the bed's length.
#[test]
fn load_keeps_a_longer_bed() {
    for rate in SESSION_RATES {
        let s = Session::bed_and_voice(rate, 6, 4);
        assert_eq!(s.length(), rate_secs(rate, 6), "session at {rate}");
        assert_eq!(s.rendered_frames(), s.length(), "session at {rate}");
    }
}

/// The session can take a *lower* rate than a later track: a 16 kHz voice
/// loaded first and a 48 kHz bed second. The bed's frames are converted down,
/// not compared as they stand (which read a two-second bed as 96 000 frames
/// in a 16 kHz session whose voice is 64 000).
#[test]
fn load_converts_a_higher_rate_track_down_to_the_session_rate() {
    let s = Session::new((VOICE_RATE, 4), (48_000, 2));
    assert_eq!(s.state().sample_rate, VOICE_RATE);
    assert_eq!(s.length(), rate_secs(VOICE_RATE, 4));
    assert_eq!(s.rendered_frames(), s.length());
}

/// Two tracks at one rate: the number is the plain maximum, as it always was.
#[test]
fn load_on_a_session_at_one_rate_is_the_plain_maximum() {
    let s = Session::new((48_000, 6), (48_000, 4));
    assert_eq!(s.length(), 6 * 48_000);
}

// ---------------------------------------------------------------------------
// cut_range, trim, time_shift, insert_silence, remove_track
// ---------------------------------------------------------------------------

#[test]
fn cut_range_on_the_voice_keeps_the_length_in_session_frames() {
    for rate in SESSION_RATES {
        let mut s = Session::bed_and_voice(rate, 2, 4);
        let r = u64::from(VOICE_RATE);
        s.ok(
            "cut_range",
            json!({ "track": 1, "start_sample": r, "end_sample": 2 * r }),
        );
        // The voice is three seconds now and still the longer track.
        assert_eq!(s.length(), rate_secs(rate, 3), "session at {rate}");
        assert_eq!(s.rendered_frames(), s.length(), "session at {rate}");
    }
}

#[test]
fn trim_on_the_voice_keeps_the_length_in_session_frames() {
    for rate in SESSION_RATES {
        let mut s = Session::bed_and_voice(rate, 2, 4);
        let r = u64::from(VOICE_RATE);
        s.ok(
            "trim",
            json!({ "track": 1, "start_sample": 0, "end_sample": 3 * r }),
        );
        assert_eq!(s.length(), rate_secs(rate, 3), "session at {rate}");
        assert_eq!(s.rendered_frames(), s.length(), "session at {rate}");
    }
}

#[test]
fn time_shift_on_the_voice_moves_the_end_in_session_frames() {
    for rate in SESSION_RATES {
        let mut s = Session::bed_and_voice(rate, 6, 4);
        s.ok("time_shift", json!({ "track": 1, "offset_sec": 4.0 }));
        // The voice now ends at eight seconds, past the six-second bed.
        assert_eq!(s.length(), rate_secs(rate, 8), "session at {rate}");
        assert_eq!(s.rendered_frames(), s.length(), "session at {rate}");

        s.ok("time_shift", json!({ "track": 1, "offset_sec": -4.0 }));
        assert_eq!(s.length(), rate_secs(rate, 6), "back under the bed, {rate}");
    }
}

#[test]
fn insert_silence_in_the_voice_lengthens_by_seconds() {
    for rate in SESSION_RATES {
        let mut s = Session::bed_and_voice(rate, 2, 4);
        s.ok(
            "insert_silence",
            json!({ "track": 1, "at": 1.0, "duration": 1.0 }),
        );
        assert_eq!(s.length(), rate_secs(rate, 5), "session at {rate}");
        assert_eq!(s.rendered_frames(), s.length(), "session at {rate}");
    }
}

#[test]
fn remove_track_leaves_the_length_of_what_is_left() {
    for rate in SESSION_RATES {
        let mut s = Session::bed_and_voice(rate, 6, 4);
        s.ok("remove_track", json!({ "track": 0 }));
        assert_eq!(s.length(), rate_secs(rate, 4), "session at {rate}");
        assert_eq!(s.rendered_frames(), s.length(), "session at {rate}");
    }
}

#[test]
fn move_clip_on_the_voice_moves_the_end_in_session_frames() {
    for rate in SESSION_RATES {
        let mut s = Session::bed_and_voice(rate, 6, 4);
        s.ok(
            "move_clip",
            json!({ "track": 1, "clip_index": 0, "start_sec": 5.0 }),
        );
        assert_eq!(s.length(), rate_secs(rate, 9), "session at {rate}");
        assert_eq!(s.rendered_frames(), s.length(), "session at {rate}");
    }
}

// ---------------------------------------------------------------------------
// cut_words, remove_fillers
// ---------------------------------------------------------------------------

fn words() -> Vec<(&'static str, f32, f32)> {
    vec![
        ("one", 0.0, 1.0),
        ("two", 1.0, 2.0),
        ("um", 2.0, 2.5),
        ("three", 2.5, 4.0),
    ]
}

/// `cut_words` on the voice, which is shorter than the bed: the session is as
/// long as it was. It used to take the cut's seconds off the stored number.
#[test]
fn cut_words_on_a_shorter_voice_leaves_the_bed_length() {
    for rate in SESSION_RATES {
        let mut s = Session::bed_and_voice(rate, 6, 4);
        s.with_transcript(&words());
        s.ok("cut_words", json!({ "from_word": 1, "to_word": 2 }));
        assert_eq!(s.length(), rate_secs(rate, 6), "session at {rate}");
        assert_eq!(s.rendered_frames(), s.length(), "session at {rate}");
    }
}

/// `cut_words` on a voice that is the longest track: it is a second shorter,
/// in session frames.
#[test]
fn cut_words_on_the_longest_voice_shortens_by_seconds() {
    for rate in SESSION_RATES {
        let mut s = Session::bed_and_voice(rate, 2, 4);
        s.with_transcript(&words());
        s.ok("cut_words", json!({ "from_word": 1, "to_word": 2 }));
        assert_eq!(s.length(), rate_secs(rate, 3), "session at {rate}");
        assert_eq!(s.rendered_frames(), s.length(), "session at {rate}");
    }
}

#[test]
fn remove_fillers_on_the_longest_voice_shortens_by_seconds() {
    for rate in SESSION_RATES {
        let mut s = Session::bed_and_voice(rate, 2, 4);
        s.with_transcript(&words());
        s.ok("remove_fillers", json!({ "apply": true }));
        // "um" is half a second, and the cut leaves a little around it for a
        // fade, so the voice is a bit under 3.5 s and not one frame exact. In
        // session frames that is near 3.5 s; counted in the voice's frames it
        // would be a third of that, and beside the bed's two seconds.
        let near = rate_secs(rate, 7) / 2;
        let tolerance = u64::from(rate) / 5;
        assert!(
            s.length().abs_diff(near) < tolerance,
            "length {} should be near {near} (session at {rate})",
            s.length()
        );
        assert_eq!(s.rendered_frames(), s.length(), "session at {rate}");
    }
}

// ---------------------------------------------------------------------------
// generated and mixed tracks, same-rate sessions
// ---------------------------------------------------------------------------

/// A generated tone is at the session's rate, so it is just as long in session
/// frames as it is in its own. The voice that runs longer still decides.
#[test]
fn generate_tone_does_not_shorten_a_longer_voice() {
    for rate in SESSION_RATES {
        let mut s = Session::bed_and_voice(rate, 2, 4);
        s.ok(
            "generate_tone",
            json!({ "frequency_hz": 440.0, "duration_sec": 1.0 }),
        );
        assert_eq!(s.length(), rate_secs(rate, 4), "session at {rate}");
        s.ok(
            "generate_tone",
            json!({ "frequency_hz": 440.0, "duration_sec": 6.0 }),
        );
        assert_eq!(s.length(), rate_secs(rate, 6), "session at {rate}");
    }
}

/// A same-rate session gives the same numbers as before: the plain maximum
/// of the clip ends, and each tool's result as it was.
#[test]
fn a_session_at_one_rate_is_unchanged() {
    let mut s = Session::new((48_000, 6), (48_000, 4));
    assert_eq!(s.length(), 6 * 48_000);

    s.ok(
        "cut_range",
        json!({ "track": 0, "start_sample": 48_000, "end_sample": 96_000 }),
    );
    assert_eq!(s.length(), 5 * 48_000);

    s.ok("time_shift", json!({ "track": 1, "offset_sec": 3.0 }));
    assert_eq!(s.length(), 7 * 48_000);

    s.ok(
        "insert_silence",
        json!({ "track": 1, "at": 1.0, "duration": 1.0 }),
    );
    assert_eq!(s.length(), 8 * 48_000);

    s.ok("remove_track", json!({ "track": 1 }));
    assert_eq!(s.length(), 5 * 48_000);
    assert_eq!(s.rendered_frames(), s.length());
}
