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
    clipboard: Option<tools::Clipboard>,
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
            allowed_tools: None,
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
    assert!(msg.contains("voice_tracks"), "{msg}");
}

/// The transcript route says so in its result, so a caller can tell
/// which key a curve came from.
#[test]
fn the_transcript_route_reports_what_it_keyed_on() {
    let mut s = Session::new();
    s.with_transcript(&two_passages());
    let v = ok(s.call("duck_under_speech", json!({ "music_track": 1 })));
    assert_eq!(v["keyed_on"], json!("transcript"), "{v}");
    // Two lines of half a second each, twice: 2.0-3.0 and 10.0-11.0.
    assert!(
        (v["speech_sec"].as_f64().unwrap() - 2.0).abs() < 1e-3,
        "{v}"
    );
    assert!(v.get("voice_tracks").is_none(), "{v}");
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

// =============================================================================
// Automation survives the destructive edit that follows (#240)
// =============================================================================
//
// `duck_under_speech` writing a curve to every clip is only half the
// promise. `destructive_edit_then` flattens a track into one clip, and
// it used to carry over `clips[0].volume_envelope` alone — so the very
// next effect silently discarded the ducking on everything after the
// first clip.
//
// That is the same "fine for the first half of the episode" failure the
// test above exists to prevent, one step further along the workflow.

/// Duck a split track, then run an ordinary effect over it.
#[test]
fn ducking_survives_a_later_destructive_edit_on_both_clips() {
    let mut s = Session::new();
    s.with_transcript(&two_passages());

    ok(s.call(
        "split_clip",
        json!({ "track": 1, "clip_index": 0, "at_sec": 8.0 }),
    ));
    ok(s.call("duck_under_speech", json!({ "music_track": 1 })));

    // What the second clip's curve looks like on the track timeline,
    // before anything collapses it.
    let before: Vec<u64> = {
        let st = s.state();
        st.tracks[1]
            .clips
            .iter()
            .flat_map(|c| {
                c.volume_envelope
                    .iter()
                    .map(move |p| p.time_samples + c.start_in_track)
            })
            .collect()
    };
    assert!(
        before.len() > 2,
        "the fixture should duck in both clips, got {} points",
        before.len()
    );
    let last_before = *before.iter().max().expect("points");

    // Any destructive effect. `limiter` does not change the length, so
    // the curve must come through unscaled.
    ok(s.call("limiter", json!({ "track": 1, "ceiling_db": -3.0 })));

    let st = s.state();
    assert_eq!(
        st.tracks[1].clips.len(),
        1,
        "the track collapsed to one clip"
    );
    let after: Vec<u64> = st.tracks[1].clips[0]
        .volume_envelope
        .iter()
        .map(|p| p.time_samples)
        .collect();

    assert_eq!(
        after.len(),
        before.len(),
        "automation was lost: {} points before the limiter, {} after — the \
         second clip's ducking is gone",
        before.len(),
        after.len()
    );
    assert_eq!(
        after.iter().max().copied(),
        Some(last_before),
        "the surviving curve should keep its position on the timeline"
    );
    assert!(
        after.windows(2).all(|w| w[0] <= w[1]),
        "merged points must stay ordered for the renderer to interpolate"
    );
}

/// A length-changing edit has to take the curve with it.
///
/// `time_stretch factor 0.5` doubles the duration, so a fade that ended
/// at the end of the clip must still end at the end of the clip — not
/// halfway through it.
#[test]
fn an_edit_that_changes_length_rescales_the_curve() {
    let mut s = Session::new();
    s.with_transcript(&two_passages());

    let total = s.state().tracks[1].clips[0].length;
    ok(s.call(
        "set_clip_envelope",
        json!({
            "track_index": 1,
            "clip_index": 0,
            "points": [
                { "time_sec": 0.0, "gain_db": 0.0 },
                { "time_sec": total as f64 / SAMPLE_RATE as f64, "gain_db": -24.0 },
            ],
        }),
    ));

    ok(s.call(
        "time_stretch",
        json!({ "track": 1, "factor": 0.5, "preserve_formants": false }),
    ));

    let st = s.state();
    let clip = &st.tracks[1].clips[0];
    let last = clip
        .volume_envelope
        .last()
        .expect("the fade should have survived");

    // The fade ended at the end of the clip; it still must.
    let fraction = last.time_samples as f64 / clip.length as f64;
    assert!(
        (fraction - 1.0).abs() < 0.02,
        "the fade should still end at the end of the clip; it ends at \
         {:.3} of the way through ({} of {} frames) — the curve was not \
         rescaled with the edit",
        fraction,
        last.time_samples,
        clip.length
    );
}

// =============================================================================
// Keyed on a voice track's audio (#168)
// =============================================================================
//
// `transcribe` is a stub (#384), so a transcript is not something a
// session can have in a shipped build. Ducking is mostly about where
// the speech is, not what was said, and `voice_tracks` answers that
// from the audio: voiced (pitched) sound counts, a breath, a click or
// hiss does not.

/// Mono 16-bit WAV of `samples` at `rate`.
fn write_samples(path: &Path, rate: u32, samples: &[f32]) -> PathBuf {
    let spec = WavSpec {
        channels: 1,
        sample_rate: rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut w = WavWriter::create(path, spec).expect("wav writer");
    for s in samples {
        w.write_sample((s.clamp(-1.0, 1.0) * 32_767.0) as i16)
            .unwrap();
    }
    w.finalize().unwrap();
    path.to_path_buf()
}

/// Voiced sound: six harmonics of 140 Hz falling as 1/k, about -21 dBFS.
fn voice_burst(rate: u32, seconds: f64) -> Vec<f32> {
    (0..(rate as f64 * seconds) as usize)
        .map(|n| {
            let t = n as f64 / rate as f64;
            (1..=6)
                .map(|k| 0.1 / k as f64 * (std::f64::consts::TAU * k as f64 * 140.0 * t).sin())
                .sum::<f64>() as f32
        })
        .collect()
}

/// White noise at `rms`, from a fixed xorshift so the run is repeatable.
fn noise_burst(rate: u32, seconds: f64, rms: f32) -> Vec<f32> {
    let mut state = 0x2545_f491u32;
    (0..(rate as f64 * seconds) as usize)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            (state as f32 / u32::MAX as f32 * 2.0 - 1.0) * rms * 3f32.sqrt()
        })
        .collect()
}

fn rms_of(v: &[f32]) -> f32 {
    (v.iter().map(|s| s * s).sum::<f32>() / v.len() as f32).sqrt()
}

/// `seconds` of silence at `rate` with each `(at_sec, signal)` mixed in.
fn track_with(rate: u32, seconds: f64, parts: &[(f64, Vec<f32>)]) -> Vec<f32> {
    let mut buf = vec![0.0f32; (rate as f64 * seconds) as usize];
    for (at, signal) in parts {
        let start = (at * rate as f64).round() as usize;
        for (d, s) in buf[start..].iter_mut().zip(signal) {
            *d += *s;
        }
    }
    buf
}

/// The default voice: lines at 2.0-3.0 s and 10.0-11.0 s of twenty.
fn two_voiced_lines(rate: u32) -> Vec<f32> {
    track_with(
        rate,
        20.0,
        &[
            (2.0, voice_burst(rate, 1.0)),
            (10.0, voice_burst(rate, 1.0)),
        ],
    )
}

/// Twenty seconds of the usual music sine at `rate`.
fn music_at(rate: u32) -> Vec<f32> {
    (0..rate as usize * 20)
        .map(|n| (2.0 * std::f32::consts::PI * 440.0 * n as f32 / rate as f32).sin() * 0.25)
        .collect()
}

impl Session {
    /// One track per `(sample_rate, samples)`, in order.
    fn with_tracks(tracks: &[(u32, Vec<f32>)]) -> Self {
        let dir = TempDir::new().expect("tempdir");
        let paths: Vec<PathBuf> = tracks
            .iter()
            .enumerate()
            .map(|(i, (rate, samples))| {
                write_samples(&dir.path().join(format!("track{i}.wav")), *rate, samples)
            })
            .collect();
        let store = session::Store::open(dir.path()).expect("open store");
        let mut s = Self {
            _dir: dir,
            store,
            engine: audio_engine::Engine::new(),
            dispatcher: ToolDispatcher::default_dispatcher(),
            clipboard: None,
        };
        for p in &paths {
            ok(s.call("load", json!({ "path": p.to_string_lossy() })));
        }
        assert_eq!(s.state().tracks.len(), tracks.len(), "one track each");
        s
    }

    /// A voice track and a music track, the voice being `voice`.
    fn with_voice(voice: Vec<f32>) -> Self {
        Self::with_tracks(&[(SAMPLE_RATE, voice), (SAMPLE_RATE, music_at(SAMPLE_RATE))])
    }

    /// Track `track`'s first clip's automation, in (seconds, dB), read
    /// at that clip's own `rate`.
    fn envelope_of(&self, track: usize, rate: u32) -> Vec<(f64, f32)> {
        self.state().tracks[track].clips[0]
            .volume_envelope
            .iter()
            .map(|p| (p.time_samples as f64 / rate as f64, p.gain_db))
            .collect()
    }
}

/// With no transcript at all, naming the voice track is enough.
#[test]
fn with_no_transcript_it_keys_on_the_voice_track() {
    let mut s = Session::with_voice(two_voiced_lines(SAMPLE_RATE));
    assert!(
        s.state().transcript.is_none(),
        "this session has no transcript"
    );

    let v = ok(s.call(
        "duck_under_speech",
        json!({ "music_track": 1, "voice_tracks": [0] }),
    ));
    assert_eq!(v["passages"], json!(2), "{v}");
    assert_eq!(v["keyed_on"], json!("audio"), "{v}");
    assert_eq!(v["voice_tracks"], json!([0]), "{v}");
    let speech = v["speech_sec"].as_f64().unwrap();
    assert!((2.0..2.4).contains(&speech), "two 1 s lines: {speech}");

    let env = s.envelope();
    assert!(level_at(&env, 2.5) < -6.0, "down under the first line");
    assert!(level_at(&env, 6.0) > -0.5, "up in the gap");
    assert!(level_at(&env, 10.5) < -6.0, "down under the second");
}

/// **The thing a level trigger gets wrong.** A breath between two
/// lines is as loud as the voice, and it must not duck the music.
#[test]
fn a_breath_between_lines_does_not_duck() {
    let line = voice_burst(SAMPLE_RATE, 1.0);
    let breath = noise_burst(SAMPLE_RATE, 0.5, rms_of(&line));
    let voice = track_with(
        SAMPLE_RATE,
        20.0,
        &[(2.0, line.clone()), (6.0, breath), (10.0, line)],
    );
    let mut s = Session::with_voice(voice);

    let v = ok(s.call(
        "duck_under_speech",
        json!({ "music_track": 1, "voice_tracks": [0] }),
    ));
    assert_eq!(v["passages"], json!(2), "the breath is not a passage: {v}");
    assert!(
        level_at(&s.envelope(), 6.25) > -0.5,
        "the music stays up under a breath"
    );
}

/// The pre-roll is why this is better than a sidechain, and it holds
/// when the key is audio too.
#[test]
fn audio_keyed_ducking_still_starts_before_the_line() {
    let mut s = Session::with_voice(two_voiced_lines(SAMPLE_RATE));
    ok(s.call(
        "duck_under_speech",
        json!({ "music_track": 1, "voice_tracks": [0], "pre_roll_ms": 500, "attack_ms": 0 }),
    ));
    assert!(
        level_at(&s.envelope(), 1.6) < -6.0,
        "ducking should start half a second early: {:?}",
        s.envelope()
    );
}

/// A line on either voice track ducks the music.
#[test]
fn two_voice_tracks_duck_under_either() {
    let a = track_with(SAMPLE_RATE, 20.0, &[(2.0, voice_burst(SAMPLE_RATE, 1.0))]);
    let b = track_with(SAMPLE_RATE, 20.0, &[(10.0, voice_burst(SAMPLE_RATE, 1.0))]);
    let mut s = Session::with_tracks(&[
        (SAMPLE_RATE, a),
        (SAMPLE_RATE, b),
        (SAMPLE_RATE, music_at(SAMPLE_RATE)),
    ]);

    let v = ok(s.call(
        "duck_under_speech",
        json!({ "music_track": 2, "voice_tracks": [0, 1] }),
    ));
    assert_eq!(v["passages"], json!(2), "{v}");
    assert_eq!(v["voice_tracks"], json!([0, 1]), "{v}");

    let env = s.envelope_of(2, SAMPLE_RATE);
    assert!(level_at(&env, 2.5) < -6.0, "ducked under voice A");
    assert!(level_at(&env, 6.0) > -0.5, "up between them");
    assert!(level_at(&env, 10.5) < -6.0, "ducked under voice B");
}

/// Naming the same track twice is one track, not two.
#[test]
fn a_repeated_voice_track_counts_once() {
    let mut s = Session::with_voice(two_voiced_lines(SAMPLE_RATE));
    let v = ok(s.call(
        "duck_under_speech",
        json!({ "music_track": 1, "voice_tracks": [0, 0] }),
    ));
    assert_eq!(v["voice_tracks"], json!([0]), "{v}");
    assert_eq!(v["passages"], json!(2), "{v}");
}

/// The speech is read where the clip sits on the timeline, not where it
/// sits in its source file.
#[test]
fn the_voice_is_read_where_it_sits_on_the_timeline() {
    let mut s = Session::with_voice(two_voiced_lines(SAMPLE_RATE));
    ok(s.call(
        "move_clip",
        json!({ "track": 0, "clip_index": 0, "start_sec": 5.0 }),
    ));

    ok(s.call(
        "duck_under_speech",
        json!({ "music_track": 1, "voice_tracks": [0] }),
    ));
    let env = s.envelope();
    // The lines moved from 2-3 and 10-11 to 7-8 and 15-16.
    assert!(level_at(&env, 7.5) < -6.0, "ducked under the moved line");
    assert!(level_at(&env, 15.5) < -6.0, "and the second");
    assert!(
        level_at(&env, 2.5) > -0.5,
        "nothing is said at 2.5 s any more"
    );
}

/// **Background under a voice clip that does not start at 0.** The
/// detector reads the track as a window from frame 0, with zeros before
/// the clip. Counted in its noise floor, five seconds of them drop the
/// floor to digital silence, so the room tone under the voice cleared
/// the threshold for the whole clip and the two lines became one passage
/// that ducked the gap between them.
#[test]
fn room_tone_under_a_moved_voice_is_not_speech() {
    let room_tone = noise_burst(SAMPLE_RATE, 20.0, 0.01); // -40 dBFS
    let voice = track_with(
        SAMPLE_RATE,
        20.0,
        &[
            (0.0, room_tone),
            (2.0, voice_burst(SAMPLE_RATE, 1.0)),
            (10.0, voice_burst(SAMPLE_RATE, 1.0)),
        ],
    );

    let mut in_place = Session::with_voice(voice.clone());
    let v = ok(in_place.call(
        "duck_under_speech",
        json!({ "music_track": 1, "voice_tracks": [0] }),
    ));
    assert_eq!(v["passages"], json!(2), "in place: {v}");

    let mut s = Session::with_voice(voice);
    ok(s.call(
        "move_clip",
        json!({ "track": 0, "clip_index": 0, "start_sec": 5.0 }),
    ));
    let v = ok(s.call(
        "duck_under_speech",
        json!({ "music_track": 1, "voice_tracks": [0] }),
    ));
    assert_eq!(v["passages"], json!(2), "moved to 5 s: {v}");
    let speech = v["speech_sec"].as_f64().unwrap();
    assert!((2.0..2.4).contains(&speech), "two 1 s lines: {speech}");

    let env = s.envelope();
    // The lines moved from 2-3 and 10-11 to 7-8 and 15-16.
    assert!(level_at(&env, 7.5) < -6.0, "down under the first line");
    assert!(level_at(&env, 15.5) < -6.0, "and the second");
    assert!(
        level_at(&env, 11.5) > -0.5,
        "up in the seven seconds between them: {env:?}"
    );
}

/// The same through a gap between two clips of one voice track, which
/// reaches the detector as zeros in the middle of the window.
#[test]
fn room_tone_around_a_gap_between_voice_clips_is_not_speech() {
    let room_tone = noise_burst(SAMPLE_RATE, 20.0, 0.01); // -40 dBFS
    let voice = track_with(
        SAMPLE_RATE,
        20.0,
        &[
            (0.0, room_tone),
            (2.0, voice_burst(SAMPLE_RATE, 1.0)),
            (12.0, voice_burst(SAMPLE_RATE, 1.0)),
        ],
    );
    let mut s = Session::with_voice(voice);
    // Cut at 8 s, then slide the second half out to 14 s: clips at
    // 0-8 s and 14-26 s with six seconds of nothing between them.
    ok(s.call(
        "split_clip",
        json!({ "track": 0, "clip_index": 0, "at_sec": 8.0 }),
    ));
    ok(s.call(
        "move_clip",
        json!({ "track": 0, "clip_index": 1, "start_sec": 14.0 }),
    ));

    let v = ok(s.call(
        "duck_under_speech",
        json!({ "music_track": 1, "voice_tracks": [0] }),
    ));
    // The lines are at 2-3 s and, 4 s into the second clip, 18-19 s.
    assert_eq!(v["passages"], json!(2), "{v}");
    let env = s.envelope();
    assert!(level_at(&env, 2.5) < -6.0, "down under the first line");
    assert!(level_at(&env, 6.0) > -0.5, "up after it, in the first clip");
    assert!(level_at(&env, 11.0) > -0.5, "up in the gap");
    assert!(
        level_at(&env, 15.0) > -0.5,
        "up in the second clip before its line: {env:?}"
    );
    assert!(level_at(&env, 18.5) < -6.0, "down under the second line");
}

/// The voice and the music need not share a sample rate: the passages
/// are seconds, and each music clip maps them with its own rate (#234).
#[test]
fn a_voice_at_another_rate_ducks_at_the_right_time() {
    let voice_rate = 44_100;
    let mut s = Session::with_tracks(&[
        (voice_rate, two_voiced_lines(voice_rate)),
        (SAMPLE_RATE, music_at(SAMPLE_RATE)),
    ]);

    ok(s.call(
        "duck_under_speech",
        json!({ "music_track": 1, "voice_tracks": [0], "attack_ms": 0, "pre_roll_ms": 0 }),
    ));
    let env = s.envelope();
    let first_drop = env
        .iter()
        .find(|(_, db)| *db < -6.0)
        .expect("a ducked point")
        .0;
    assert!(
        (first_drop - 2.0).abs() < 0.03,
        "the duck should land at 2.0 s, not {first_drop}: {env:?}"
    );
    assert!(level_at(&env, 6.0) > -0.5, "up in the gap");
}

/// The same, the other way round: the music is the odd rate out, so the
/// envelope is counted in 44.1 kHz frames.
#[test]
fn a_music_bed_at_another_rate_ducks_at_the_right_time() {
    let music_rate = 44_100;
    let mut s = Session::with_tracks(&[
        (SAMPLE_RATE, two_voiced_lines(SAMPLE_RATE)),
        (music_rate, music_at(music_rate)),
    ]);

    ok(s.call(
        "duck_under_speech",
        json!({ "music_track": 1, "voice_tracks": [0], "attack_ms": 0, "pre_roll_ms": 0 }),
    ));
    let env = s.envelope_of(1, music_rate);
    let first_drop = env
        .iter()
        .find(|(_, db)| *db < -6.0)
        .expect("a ducked point")
        .0;
    assert!(
        (first_drop - 2.0).abs() < 0.03,
        "the duck should land at 2.0 s, not {first_drop}: {env:?}"
    );
}

/// A track named as the voice wins over a transcript: the transcript is
/// session-level and cannot say which track it belongs to.
#[test]
fn voice_tracks_wins_over_a_transcript() {
    let mut s = Session::with_voice(two_voiced_lines(SAMPLE_RATE));
    s.with_transcript(&[("elsewhere", 14.0, 15.0)]);

    let v = ok(s.call(
        "duck_under_speech",
        json!({ "music_track": 1, "voice_tracks": [0] }),
    ));
    assert_eq!(v["keyed_on"], json!("audio"), "{v}");
    let env = s.envelope();
    assert!(level_at(&env, 2.5) < -6.0, "ducked where the audio speaks");
    assert!(
        level_at(&env, 14.5) > -0.5,
        "not where the transcript says: {env:?}"
    );
}

/// Finding nothing is an error, not an empty curve and a new node that
/// looks like it did something.
#[test]
fn a_silent_voice_track_is_an_error_not_an_empty_curve() {
    let mut s = Session::with_voice(vec![0.0; SAMPLE_RATE as usize * 20]);
    let head = s.store.head();

    let msg = err(s.call(
        "duck_under_speech",
        json!({ "music_track": 1, "voice_tracks": [0] }),
    ));
    assert!(msg.contains("no speech"), "{msg}");
    assert!(msg.contains('0'), "names the track: {msg}");
    assert_eq!(s.store.head(), head, "no node was appended");
    assert!(
        s.state().tracks[1].clips[0].volume_envelope.is_empty(),
        "the music is untouched"
    );
}

/// Noise alone is not speech either, however loud.
#[test]
fn a_voice_track_of_only_hiss_is_an_error() {
    let mut s = Session::with_voice(noise_burst(SAMPLE_RATE, 20.0, 0.05));
    let msg = err(s.call(
        "duck_under_speech",
        json!({ "music_track": 1, "voice_tracks": [0] }),
    ));
    assert!(msg.contains("no speech"), "{msg}");
}

#[test]
fn the_music_track_cannot_be_its_own_voice() {
    let mut s = Session::with_voice(two_voiced_lines(SAMPLE_RATE));
    let msg = err(s.call(
        "duck_under_speech",
        json!({ "music_track": 1, "voice_tracks": [1] }),
    ));
    assert!(msg.contains("music track"), "{msg}");

    let msg = err(s.call(
        "duck_under_speech",
        json!({ "music_track": 1, "voice_tracks": [7] }),
    ));
    assert!(msg.contains("out of range"), "{msg}");
}

/// An empty list is a mistake to report, not "no voice, use the
/// transcript". The schema refuses it before the tool sees it.
#[test]
fn an_empty_voice_tracks_list_is_refused() {
    let mut s = Session::with_voice(two_voiced_lines(SAMPLE_RATE));
    s.with_transcript(&two_passages());
    let mut ctx = ToolContext {
        store: &mut s.store,
        engine: &mut s.engine,
        user_message: "",
        clipboard: &mut s.clipboard,
        allowed_tools: None,
    };
    let refused = s
        .dispatcher
        .invoke(
            "duck_under_speech",
            json!({ "music_track": 1, "voice_tracks": [] }),
            &mut ctx,
        )
        .expect_err("an empty voice_tracks must not reach the tool");
    assert!(refused.to_string().contains("voice_tracks"), "{refused}");
}

#[test]
fn a_voice_track_with_no_clips_is_refused() {
    let mut s = Session::with_voice(two_voiced_lines(SAMPLE_RATE));
    ok(s.call("remove_clip", json!({ "track": 0, "clip_index": 0 })));
    let msg = err(s.call(
        "duck_under_speech",
        json!({ "music_track": 1, "voice_tracks": [0] }),
    ));
    assert!(msg.contains("no clips"), "{msg}");
}

/// A music track with nothing to automate is known before any audio is
/// read, so the refusal must not wait for the voice to be decoded and
/// analysed first. The voice file is deleted: if the tool read it first
/// the error would be about that file, not about the music track.
#[test]
fn an_empty_music_track_is_refused_before_the_voice_is_read() {
    let mut s = Session::with_voice(two_voiced_lines(SAMPLE_RATE));
    std::fs::remove_file(s._dir.path().join("track0.wav")).expect("delete the voice file");

    // The control: with a music clip to automate, the missing voice
    // file is what the tool trips over.
    let msg = err(s.call(
        "duck_under_speech",
        json!({ "music_track": 1, "voice_tracks": [0] }),
    ));
    assert!(msg.contains("failed to decode"), "{msg}");

    ok(s.call("remove_clip", json!({ "track": 1, "clip_index": 0 })));
    let msg = err(s.call(
        "duck_under_speech",
        json!({ "music_track": 1, "voice_tracks": [0] }),
    ));
    assert!(msg.contains("no clips to automate"), "{msg}");
}

/// Reading the voice is a read: the track keeps its audio.
#[test]
fn the_voice_track_is_not_modified_in_audio_mode() {
    let mut s = Session::with_voice(two_voiced_lines(SAMPLE_RATE));
    let before = s.state().tracks[0].clone();

    ok(s.call(
        "duck_under_speech",
        json!({ "music_track": 1, "voice_tracks": [0] }),
    ));

    assert_eq!(s.state().tracks[0], before, "the voice must not be touched");
}
