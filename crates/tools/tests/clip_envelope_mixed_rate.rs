//! A clip's volume envelope on a clip at another rate than the session's
//! (#677, after #673).
//!
//! The renderer, `set_clip_envelope`, `duck_under_speech` and the slicing
//! helpers behind `cut_range`, `trim` and `split_clip` disagreed about the
//! unit of an envelope point on a 16 kHz voice in a 48 kHz (or 44.1 kHz)
//! session: the renderer and `set_clip_envelope` meant project frames,
//! `duck_under_speech` wrote the clip's own frames, and the slicers cut by
//! the clip's own frames. Each test here writes an envelope, edits the clip
//! or not, renders, and reads the gain at a known time off the audio.

use std::path::{Path, PathBuf};

use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
use serde_json::{json, Value};
use tempfile::TempDir;
use tools::{ToolContext, ToolDispatcher, ToolResult};

const VOICE_RATE: u32 = 16_000;
const VOICE_SECONDS: usize = 4;
const BED_SECONDS: usize = 6;

/// Both are common and neither is a multiple of 16 kHz, which is the case a
/// ratio hides.
const SESSION_RATES: [u32; 2] = [48_000, 44_100];

fn write_tone(path: &Path, rate: u32, seconds: usize) -> PathBuf {
    let spec = WavSpec {
        channels: 1,
        sample_rate: rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut w = WavWriter::create(path, spec).expect("wav writer");
    for n in 0..(rate as usize * seconds) {
        let t = n as f32 / rate as f32;
        let s = (2.0 * std::f32::consts::PI * 440.0 * t).sin() * 0.5;
        w.write_sample((s * 32_767.0) as i16).unwrap();
    }
    w.finalize().unwrap();
    path.to_path_buf()
}

struct Session {
    dir: TempDir,
    store: session::Store,
    engine: audio_engine::Engine,
    dispatcher: ToolDispatcher,
    clipboard: Option<tools::Clipboard>,
    rate: u32,
}

impl Session {
    /// Track 0 is a six-second bed at `rate` (so the session is at that
    /// rate); track 1 is a four-second 16 kHz voice.
    fn new(rate: u32) -> Self {
        let dir = TempDir::new().expect("tempdir");
        let bed = write_tone(&dir.path().join("bed.wav"), rate, BED_SECONDS);
        let voice = write_tone(&dir.path().join("voice.wav"), VOICE_RATE, VOICE_SECONDS);
        let store = session::Store::open(dir.path()).expect("open store");
        let mut s = Self {
            dir,
            store,
            engine: audio_engine::Engine::new(),
            dispatcher: ToolDispatcher::default_dispatcher(),
            clipboard: None,
            rate,
        };
        s.ok("load", json!({ "path": bed.to_string_lossy() }));
        s.ok("load", json!({ "path": voice.to_string_lossy() }));
        assert_eq!(s.state().sample_rate, rate, "the bed sets the rate");
        s
    }

    fn ok(&mut self, tool: &str, args: Value) -> Value {
        let mut ctx = ToolContext {
            store: &mut self.store,
            engine: &mut self.engine,
            user_message: "",
            clipboard: &mut self.clipboard,
            allowed_tools: None,
        };
        match self.dispatcher.invoke(tool, args, &mut ctx).unwrap() {
            ToolResult::Ok(v) => v,
            ToolResult::Error(m) => panic!("{tool}: expected Ok, got Error({m})"),
        }
    }

    fn state(&self) -> session::SessionState {
        let head = self.store.head().expect("a head");
        self.store.get(head).expect("head node").state
    }

    fn append(&mut self, state: session::SessionState) {
        self.store
            .append(session::SessionNode {
                id: session::NodeId([0u8; 32]),
                parent: None,
                created_at: chrono::Utc::now(),
                label: Some("test edit".into()),
                reasoning: None,
                state,
                op: None,
            })
            .expect("append");
    }

    /// Set the voice's envelope, `(seconds from the clip start, dB)`.
    fn envelope(&mut self, points: &[(f64, f64)]) {
        let points: Vec<Value> = points
            .iter()
            .map(|(t, g)| json!({ "time_sec": t, "gain_db": g }))
            .collect();
        self.ok(
            "set_clip_envelope",
            json!({ "track_index": 1, "clip_index": 0, "points": points }),
        );
    }

    /// Render the voice track alone, with its envelope and without it.
    fn render_voice(&self) -> (Vec<f32>, Vec<f32>) {
        let mut with = self.state();
        with.tracks.remove(0);
        let mut without = with.clone();
        for c in &mut without.tracks[0].clips {
            c.volume_envelope.clear();
        }
        (self.render(&with, "with"), self.render(&without, "without"))
    }

    /// Render the bed alone, with its envelope and without it.
    fn render_bed(&self) -> (Vec<f32>, Vec<f32>) {
        let mut with = self.state();
        with.tracks.remove(1);
        let mut without = with.clone();
        for c in &mut without.tracks[0].clips {
            c.volume_envelope.clear();
        }
        (
            self.render(&with, "bwith"),
            self.render(&without, "bwithout"),
        )
    }

    fn render(&self, state: &session::SessionState, name: &str) -> Vec<f32> {
        let out = self.dir.path().join(format!("{name}.wav"));
        audio_engine::render_state_to_wav(state, &out, None).expect("render");
        let mut r = WavReader::open(&out).expect("rendered wav");
        assert_eq!(r.spec().sample_rate, self.rate);
        let ch = r.spec().channels as usize;
        let all: Vec<f32> = r
            .samples::<i16>()
            .map(|s| f32::from(s.unwrap()) / 32_768.0)
            .collect();
        all.chunks(ch).map(|f| f[0]).collect()
    }
}

/// The gain the envelope applied at `t` seconds, read off the audio: the
/// loudness of a 50 ms window with the envelope over the same window
/// without it.
fn gain_db_at(rate: u32, with: &[f32], without: &[f32], t: f64) -> f64 {
    let half = (rate as f64 * 0.025) as usize;
    let mid = (t * rate as f64) as usize;
    let rms = |x: &[f32]| -> f64 {
        let w = &x[mid - half..mid + half];
        (w.iter().map(|s| f64::from(*s).powi(2)).sum::<f64>() / w.len() as f64).sqrt()
    };
    20.0 * (rms(with) / rms(without)).log10()
}

fn assert_gain(rate: u32, (with, without): &(Vec<f32>, Vec<f32>), t: f64, want: f64, what: &str) {
    let got = gain_db_at(rate, with, without, t);
    assert!(
        (got - want).abs() < 2.0,
        "{what}: at {t} s the gain should be {want} dB, got {got:.2} dB (session at {rate})"
    );
}

/// 0 dB until 2 s, down to -20 dB by 2.1 s: a drop whose position can be
/// read to a tenth of a second.
const DROP_AT_TWO: [(f64, f64); 4] = [(0.0, 0.0), (2.0, 0.0), (2.1, -20.0), (4.0, -20.0)];

/// As above but at 3 s, for tests that cut a second off the front.
const DROP_AT_THREE: [(f64, f64); 4] = [(0.0, 0.0), (3.0, 0.0), (3.1, -20.0), (4.0, -20.0)];

fn check_drop(rate: u32, r: &(Vec<f32>, Vec<f32>), at: f64, what: &str) {
    assert_gain(rate, r, at - 0.5, 0.0, what);
    assert_gain(rate, r, at + 0.05, -10.0, what);
    assert_gain(rate, r, at + 0.5, -20.0, what);
}

// ---------------------------------------------------------------------------
// set_clip_envelope
// ---------------------------------------------------------------------------

#[test]
fn set_clip_envelope_lands_at_its_seconds_on_an_off_rate_clip() {
    for rate in SESSION_RATES {
        let mut s = Session::new(rate);
        s.envelope(&DROP_AT_TWO);
        check_drop(rate, &s.render_voice(), 2.0, "set_clip_envelope");
    }
}

#[test]
fn set_clip_envelope_is_unchanged_on_a_clip_at_the_session_rate() {
    for rate in SESSION_RATES {
        let mut s = Session::new(rate);
        s.ok(
            "set_clip_envelope",
            json!({ "track_index": 0, "clip_index": 0, "points": [
                { "time_sec": 0.0, "gain_db": 0.0 }, { "time_sec": 2.0, "gain_db": 0.0 },
                { "time_sec": 2.1, "gain_db": -20.0 }, { "time_sec": 5.0, "gain_db": -20.0 }] }),
        );
        check_drop(rate, &s.render_bed(), 2.0, "same-rate set_clip_envelope");
    }
}

// ---------------------------------------------------------------------------
// duck_under_speech
// ---------------------------------------------------------------------------

#[test]
fn duck_under_speech_ducks_at_the_speech_on_an_off_rate_clip() {
    for rate in SESSION_RATES {
        let mut s = Session::new(rate);
        // One line, 2.0 to 3.0 s, over the 16 kHz track.
        let mut state = s.state();
        state.transcript = Some(session::Transcript {
            source: None,
            words: vec![session::TranscriptWord {
                text: "line".into(),
                start_s: 2.0,
                end_s: 3.0,
                confidence: 0.9,
            }],
        });
        s.append(state);
        s.ok(
            "duck_under_speech",
            json!({ "music_track": 1, "duck_db": -12.0 }),
        );

        let r = s.render_voice();
        assert_gain(rate, &r, 1.0, 0.0, "before the line");
        assert_gain(rate, &r, 2.5, -12.0, "under the line");
        assert_gain(rate, &r, 3.8, 0.0, "after the line");
    }
}

// ---------------------------------------------------------------------------
// cut_range, trim, split_clip on an enveloped clip
// ---------------------------------------------------------------------------

#[test]
fn cut_range_moves_an_envelope_by_the_clips_own_frames() {
    for rate in SESSION_RATES {
        let mut s = Session::new(rate);
        s.envelope(&DROP_AT_THREE);
        let r = u64::from(VOICE_RATE);
        s.ok(
            "cut_range",
            json!({ "track": 1, "start_sample": 0, "end_sample": r }),
        );
        // The first second is gone, so the drop is a second earlier.
        check_drop(rate, &s.render_voice(), 2.0, "cut_range");
    }
}

#[test]
fn trim_moves_an_envelope_by_the_clips_own_frames() {
    for rate in SESSION_RATES {
        let mut s = Session::new(rate);
        s.envelope(&DROP_AT_THREE);
        let r = u64::from(VOICE_RATE);
        s.ok(
            "trim",
            json!({ "track": 1, "start_sample": r, "end_sample": 4 * r }),
        );
        check_drop(rate, &s.render_voice(), 2.0, "trim");
    }
}

#[test]
fn split_clip_keeps_an_envelope_on_the_audio_it_was_written_for() {
    for rate in SESSION_RATES {
        let mut s = Session::new(rate);
        s.envelope(&DROP_AT_THREE);
        s.ok(
            "split_clip",
            json!({ "track": 1, "clip_index": 0, "at_sec": 1.0 }),
        );
        assert_eq!(s.state().tracks[1].clips.len(), 2);
        // Splitting changes no sample of the mix.
        check_drop(rate, &s.render_voice(), 3.0, "split_clip");
    }
}

#[test]
fn cut_range_on_a_clip_at_the_session_rate_is_unchanged() {
    for rate in SESSION_RATES {
        let mut s = Session::new(rate);
        s.ok(
            "set_clip_envelope",
            json!({ "track_index": 0, "clip_index": 0, "points": [
                { "time_sec": 0.0, "gain_db": 0.0 }, { "time_sec": 3.0, "gain_db": 0.0 },
                { "time_sec": 3.1, "gain_db": -20.0 }, { "time_sec": 6.0, "gain_db": -20.0 }] }),
        );
        let b = u64::from(rate);
        s.ok(
            "cut_range",
            json!({ "track": 0, "start_sample": 0, "end_sample": b }),
        );
        check_drop(rate, &s.render_bed(), 2.0, "same-rate cut_range");
    }
}

// ---------------------------------------------------------------------------
// an envelope stored before #677
// ---------------------------------------------------------------------------

/// A project saved before the unit was recorded: the voice clip carries
/// `volume_envelope` in project frames and no `volume_envelope_frames`.
fn with_stored_envelope(rate: u32) -> Session {
    let mut s = Session::new(rate);
    let state = s.state();
    let mut v = serde_json::to_value(&state).unwrap();
    let r = f64::from(rate);
    let clip = &mut v["tracks"][1]["clips"][0];
    clip["volume_envelope"] = json!([
        { "time_samples": 0, "gain_db": 0.0 },
        { "time_samples": (2.0 * r) as u64, "gain_db": 0.0 },
        { "time_samples": (2.1 * r) as u64, "gain_db": -20.0 },
        { "time_samples": (4.0 * r) as u64, "gain_db": -20.0 },
    ]);
    clip.as_object_mut()
        .unwrap()
        .remove("volume_envelope_frames");
    s.append(serde_json::from_value(v).expect("a pre-#677 state"));
    s
}

#[test]
fn a_stored_envelope_plays_as_it_did() {
    for rate in SESSION_RATES {
        let s = with_stored_envelope(rate);
        check_drop(rate, &s.render_voice(), 2.0, "stored envelope");
    }
}

#[test]
fn a_stored_envelope_survives_an_edit_that_slices_it() {
    for rate in SESSION_RATES {
        let mut s = with_stored_envelope(rate);
        let r = u64::from(VOICE_RATE);
        s.ok(
            "cut_range",
            json!({ "track": 1, "start_sample": 0, "end_sample": r }),
        );
        check_drop(rate, &s.render_voice(), 1.0, "cut stored envelope");
    }
}
