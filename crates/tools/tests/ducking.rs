//! Ducking music under speech, keyed on words (#168).
//!
//! A sidechain compressor keys on level: it mistakes a breath for
//! speech, misses a quiet line, and cannot start before a line because
//! it only knows the line began after it has. Keying on the transcript
//! fixes all three, and the tests are about exactly those properties —
//! plus the one that makes it usable, that the output is an ordinary
//! automation curve rather than a black box.

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
    clipboard: Option<Vec<f32>>,
}

impl Session {
    /// A voice track and a music track, both twenty seconds.
    fn new() -> Self {
        let dir = TempDir::new().expect("tempdir");
        let voice = write_sine(&dir.path().join("voice.wav"), 20);
        let music = write_sine(&dir.path().join("music.wav"), 20);
        let store = session::Store::open(dir.path()).expect("open store");
        let mut s = Self {
            _dir: dir,
            store,
            engine: audio_engine::Engine::new(),
            dispatcher: ToolDispatcher::default_dispatcher(),
            clipboard: None,
        };
        s.call("load", json!({ "path": voice.to_string_lossy() }));
        s.call("load", json!({ "path": music.to_string_lossy() }));
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

    fn state(&self) -> session::SessionState {
        let head = self.store.head().expect("a head");
        self.store.get(head).expect("head node").state
    }

    fn with_transcript(&mut self, words: &[(&str, f32, f32)]) {
        let mut state = self.state();
        state.transcript = Some(session::Transcript {
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

    /// The music track's automation, in (seconds, dB).
    fn envelope(&self) -> Vec<(f64, f32)> {
        let state = self.state();
        state.tracks[1].clips[0]
            .volume_envelope
            .iter()
            .map(|p| (p.time_samples as f64 / SAMPLE_RATE as f64, p.gain_db))
            .collect()
    }
}

fn ok(r: ToolResult) -> Value {
    match r {
        ToolResult::Ok(v) => v,
        ToolResult::Error(m) => panic!("expected Ok, got Error({m})"),
    }
}

fn err(r: ToolResult) -> String {
    match r {
        ToolResult::Error(m) => m,
        ToolResult::Ok(v) => panic!("expected Error, got Ok({v})"),
    }
}

/// Two sentences with a five-second gap between them.
fn two_passages() -> Vec<(&'static str, f32, f32)> {
    vec![
        ("Hello", 2.0, 2.5),
        ("there", 2.5, 3.0),
        // Long gap — the music should come back up in it.
        ("Second", 10.0, 10.5),
        ("line", 10.5, 11.0),
    ]
}

/// Music drops under speech and recovers in the gaps.
#[test]
fn the_music_drops_under_speech_and_recovers() {
    let mut s = Session::new();
    s.with_transcript(&two_passages());

    let v = ok(s.call("duck_under_speech", json!({ "music_track": 1 })));
    assert_eq!(v["passages"], json!(2), "{v}");

    let env = s.envelope();
    assert!(!env.is_empty(), "an automation curve should exist");

    // Full level before the first line, ducked during it, back up in
    // the gap between the two.
    assert!(level_at(&env, 1.0) > -0.5, "music is up before the line");
    assert!(level_at(&env, 2.7) < -6.0, "music is down during the line");
    assert!(level_at(&env, 6.0) > -0.5, "music is back up in the gap");
    assert!(level_at(&env, 10.7) < -6.0, "and down again for the second");
}

/// **The thing a sidechain cannot do.** The duck starts before the
/// line, not when the level crosses a threshold.
#[test]
fn the_duck_starts_before_the_line() {
    let mut s = Session::new();
    s.with_transcript(&two_passages());

    ok(s.call(
        "duck_under_speech",
        json!({ "music_track": 1, "pre_roll_ms": 500, "attack_ms": 0 }),
    ));

    let env = s.envelope();
    // The line starts at 2.0s; with 500ms of pre-roll the music is
    // already down by 1.6s.
    assert!(
        level_at(&env, 1.6) < -6.0,
        "should already be ducking half a second early: {env:?}"
    );
}

/// Depth and recovery are the caller's.
#[test]
fn depth_and_recovery_are_adjustable() {
    let mut s = Session::new();
    s.with_transcript(&two_passages());

    ok(s.call(
        "duck_under_speech",
        json!({ "music_track": 1, "duck_db": -24.0, "attack_ms": 0 }),
    ));
    let env = s.envelope();
    assert!(
        (level_at(&env, 2.7) + 24.0).abs() < 0.5,
        "should duck by the requested 24 dB: got {}",
        level_at(&env, 2.7)
    );
}

/// A short gap inside a sentence must not un-duck: ducking back up for
/// a comma is a pump, not an edit.
#[test]
fn a_pause_inside_a_sentence_does_not_un_duck() {
    let mut s = Session::new();
    s.with_transcript(&[
        ("Hello", 2.0, 2.5),
        // A third of a second — a breath, not a gap.
        ("there", 2.8, 3.3),
    ]);

    let v = ok(s.call("duck_under_speech", json!({ "music_track": 1 })));
    assert_eq!(v["passages"], json!(1), "one passage, not two: {v}");

    let env = s.envelope();
    assert!(
        level_at(&env, 2.65) < -6.0,
        "the music must stay down across the breath"
    );
}

/// The output is the same automation curve the lane already draws and
/// the user can already drag — not a hidden processor.
#[test]
fn the_result_is_an_editable_automation_curve() {
    let mut s = Session::new();
    s.with_transcript(&two_passages());
    ok(s.call("duck_under_speech", json!({ "music_track": 1 })));

    let state = s.state();
    let env = &state.tracks[1].clips[0].volume_envelope;
    assert!(env.len() >= 4, "a duck is at least four points");

    // Ascending and unique, or the renderer's interpolation has nothing
    // to interpolate between.
    for pair in env.windows(2) {
        assert!(
            pair[1].time_samples > pair[0].time_samples,
            "envelope points must ascend: {env:?}"
        );
    }

    // And it is editable by the ordinary tool, which is the point.
    ok(s.call(
        "set_clip_envelope",
        json!({
            "track_index": 1,
            "clip_index": 0,
            "points": [ { "time_sec": 0.0, "gain_db": 0.0 } ],
        }),
    ));
}

/// A track cut into two clips must duck across both.
///
/// Automating only `clips[0]` is the kind of failure that sounds fine
/// for the first half of the episode and wrong for the second — the
/// worst sort, because it passes a spot check.
#[test]
fn every_clip_on_the_track_gets_ducked() {
    let mut s = Session::new();
    s.with_transcript(&two_passages());

    // Split the music at 8s so the second line falls on the second clip.
    ok(s.call(
        "split_clip",
        json!({ "track": 1, "clip_index": 0, "at_sec": 8.0 }),
    ));
    assert_eq!(s.state().tracks[1].clips.len(), 2, "two clips to cover");

    let v = ok(s.call("duck_under_speech", json!({ "music_track": 1 })));
    assert_eq!(v["clips"], json!(2), "both clips carry a curve: {v}");

    for (i, clip) in s.state().tracks[1].clips.iter().enumerate() {
        assert!(
            !clip.volume_envelope.is_empty(),
            "clip {i} has no automation, so the music never ducks under it"
        );
    }
}

/// A release that lands exactly on the clip boundary must still put the
/// recovery point down. Dropping it leaves the last value ducked, so the
/// music never comes back up before the clip ends.
#[test]
fn a_duck_recovering_at_the_very_end_still_comes_back_up() {
    let mut s = Session::new();
    // The track is 20s; put a line so its release lands past the end.
    s.with_transcript(&[("last", 19.0, 19.6)]);

    ok(s.call(
        "duck_under_speech",
        json!({ "music_track": 1, "release_ms": 2000 }),
    ));

    let env = s.envelope();
    let (_, last_db) = *env.last().expect("an envelope");
    assert!(
        last_db > -0.5,
        "the curve must end back at unity, not stuck down: {env:?}"
    );
}

/// The voice track is untouched: this is an edit to the music.
#[test]
fn the_speech_track_is_not_modified() {
    let mut s = Session::new();
    s.with_transcript(&two_passages());
    let before = s.state().tracks[0].clone();

    ok(s.call("duck_under_speech", json!({ "music_track": 1 })));

    assert_eq!(s.state().tracks[0], before, "the voice must not be touched");
}

#[test]
fn no_transcript_says_what_to_do() {
    let mut s = Session::new();
    let msg = err(s.call("duck_under_speech", json!({ "music_track": 1 })));
    assert!(msg.contains("transcribe"), "{msg}");
}

#[test]
fn a_positive_duck_is_refused() {
    let mut s = Session::new();
    s.with_transcript(&two_passages());
    let msg = err(s.call(
        "duck_under_speech",
        json!({ "music_track": 1, "duck_db": 6.0 }),
    ));
    assert!(msg.contains("drop"), "{msg}");
}

/// Linear interpolation between the surrounding points, which is what
/// the renderer does.
fn level_at(env: &[(f64, f32)], t: f64) -> f32 {
    if env.is_empty() {
        return 0.0;
    }
    if t <= env[0].0 {
        return env[0].1;
    }
    for pair in env.windows(2) {
        let (t0, g0) = pair[0];
        let (t1, g1) = pair[1];
        if t >= t0 && t <= t1 {
            let span = (t1 - t0).max(1e-9);
            let k = ((t - t0) / span) as f32;
            return g0 + (g1 - g0) * k;
        }
    }
    env[env.len() - 1].1
}
