//! Turning a description into a range (#168 §3).
//!
//! The leverage here is indirect: **every range-taking tool already
//! exists**, and none of them could be reached by describing where to
//! apply it. So the tests are about the two things that make a resolved
//! range usable — that it lands on the right audio, and that a
//! description which does not resolve produces a refusal rather than
//! the closest thing found. A selection that is nearly right is worse
//! than none, because the next call is a destructive edit.
//!
//! The last section is `speech_passage` with `voice_tracks`: the same
//! passages found in a voice track's audio, for a session with no
//! transcript (`transcribe` is a stub, #384).

use std::path::{Path, PathBuf};

use hound::{SampleFormat, WavSpec, WavWriter};
use serde_json::{json, Value};
use tempfile::TempDir;
use tools::{ToolContext, ToolDispatcher, ToolResult};

const SR: u32 = 48_000;

fn write_tone(path: &Path, seconds: usize) -> PathBuf {
    let spec = WavSpec {
        channels: 1,
        sample_rate: SR,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut w = WavWriter::create(path, spec).expect("wav writer");
    for n in 0..(SR as usize * seconds) {
        let t = n as f32 / SR as f32;
        w.write_sample(((2.0 * std::f32::consts::PI * 440.0 * t).sin() * 8000.0) as i16)
            .unwrap();
    }
    w.finalize().unwrap();
    path.to_path_buf()
}

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
/// The same synthetic voice `tests/ducking.rs` uses, so "speech" means the
/// same thing in both.
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
        let src = write_tone(&dir.path().join("take.wav"), 30);
        let store = session::Store::open(dir.path()).expect("open store");
        let mut s = Self {
            _dir: dir,
            store,
            engine: audio_engine::Engine::new(),
            dispatcher: ToolDispatcher::default_dispatcher(),
            clipboard: None,
        };
        s.call("load", json!({ "path": src.to_string_lossy() }));
        s
    }

    /// One mono track per `(sample_rate, samples)`, in order.
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

    fn state(&self) -> session::SessionState {
        let head = self.store.head().expect("a head");
        self.store.get(head).expect("head node").state
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

    /// Two sentences with a long gap, so passages are unambiguous.
    fn with_transcript(&mut self, words: &[(&str, f32, f32)]) {
        let head = self.store.head().expect("head");
        let mut state = self.store.get(head).expect("node").state;
        state.transcript = Some(session::Transcript {
            words: words
                .iter()
                .map(|(text, a, b)| session::TranscriptWord {
                    text: (*text).to_string(),
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

/// Two sentences: 2–4s and 12–15s.
fn transcript() -> Vec<(&'static str, f32, f32)> {
    vec![
        ("The", 2.0, 2.3),
        ("latency", 2.3, 3.0),
        ("was", 3.0, 3.4),
        ("terrible", 3.4, 4.0),
        ("We", 12.0, 12.4),
        ("fixed", 12.4, 13.0),
        ("the", 13.0, 13.3),
        ("latency", 13.3, 14.0),
        ("problem", 14.0, 15.0),
    ]
}

/// The headline case: a phrase becomes the range of the words that say
/// it, with real timings.
#[test]
fn a_phrase_resolves_to_the_words_that_say_it() {
    let mut s = Session::new();
    s.with_transcript(&transcript());

    let v = ok(s.call("select_region", json!({ "text": "the latency" })));

    // "The latency" — words 0 and 1, so 2.0s to 3.0s.
    assert!((v["start_sec"].as_f64().unwrap() - 2.0).abs() < 0.01, "{v}");
    assert!((v["end_sec"].as_f64().unwrap() - 3.0).abs() < 0.01, "{v}");
}

/// Punctuation and case are the transcriber's, not the user's — so
/// "latency" must match "latency," and "Latency".
#[test]
fn matching_ignores_case_and_the_punctuation_a_transcriber_adds() {
    let mut s = Session::new();
    s.with_transcript(&[("Well,", 1.0, 1.4), ("LATENCY.", 1.4, 2.0)]);

    let v = ok(s.call("select_region", json!({ "text": "latency" })));
    assert!((v["start_sec"].as_f64().unwrap() - 1.4).abs() < 0.01, "{v}");
}

/// A repeated phrase is addressable rather than ambiguous.
#[test]
fn a_repeated_phrase_can_be_asked_for_by_occurrence() {
    let mut s = Session::new();
    s.with_transcript(&transcript());

    let first = ok(s.call("select_region", json!({ "text": "latency" })));
    let second = ok(s.call(
        "select_region",
        json!({ "text": "latency", "occurrence": 2 }),
    ));

    assert!(
        (first["start_sec"].as_f64().unwrap() - 2.3).abs() < 0.01,
        "{first}"
    );
    assert!(
        (second["start_sec"].as_f64().unwrap() - 13.3).abs() < 0.01,
        "{second}"
    );
}

/// **The refusal that matters.** A phrase that is not there selects
/// nothing — the next tool call is a destructive edit, so the nearest
/// match would be worse than an error.
#[test]
fn a_phrase_that_is_not_there_refuses_rather_than_guessing() {
    let mut s = Session::new();
    s.with_transcript(&transcript());

    let msg = err(s.call("select_region", json!({ "text": "bandwidth" })));
    assert!(msg.contains("not in the transcript"), "{msg}");
    assert!(
        msg.contains("nearly right is worse"),
        "and says why it did not approximate: {msg}"
    );
}

/// Asking for the fifth of two says so, rather than clamping.
#[test]
fn an_occurrence_past_the_end_says_how_many_there_are() {
    let mut s = Session::new();
    s.with_transcript(&transcript());

    let msg = err(s.call(
        "select_region",
        json!({ "text": "latency", "occurrence": 5 }),
    ));
    assert!(msg.contains("appears 2 time(s)"), "{msg}");
}

/// "The last thing he said" without having to know how many there were.
#[test]
fn a_negative_passage_counts_from_the_end() {
    let mut s = Session::new();
    s.with_transcript(&transcript());

    let last = ok(s.call("select_region", json!({ "speech_passage": -1 })));
    assert!(
        (last["start_sec"].as_f64().unwrap() - 12.0).abs() < 0.01,
        "{last}"
    );
    assert!(
        (last["end_sec"].as_f64().unwrap() - 15.0).abs() < 0.01,
        "{last}"
    );

    let first = ok(s.call("select_region", json!({ "speech_passage": 1 })));
    assert!(
        (first["start_sec"].as_f64().unwrap() - 2.0).abs() < 0.01,
        "{first}"
    );
}

/// Beat-based descriptions work too, which is the other half of the
/// acceptance — music, not just speech.
#[test]
fn a_beat_range_resolves_through_the_tempo_map() {
    let mut s = Session::new();
    // Default tempo is 120 BPM, so a beat is half a second.
    let v = ok(s.call("select_region", json!({ "from_beat": 1, "to_beat": 9 })));

    assert!((v["start_sec"].as_f64().unwrap() - 0.0).abs() < 0.01, "{v}");
    assert!(
        (v["end_sec"].as_f64().unwrap() - 4.0).abs() < 0.01,
        "eight beats at 120 BPM is four seconds: {v}"
    );
}

/// Padding, for a selection that needs to breathe — and it must not go
/// negative at the start of the session.
#[test]
fn padding_widens_the_range_without_going_below_zero() {
    let mut s = Session::new();
    s.with_transcript(&[("Hello", 0.2, 0.6)]);

    let v = ok(s.call("select_region", json!({ "text": "hello", "pad_sec": 1.0 })));
    assert_eq!(
        v["start_sec"].as_f64().unwrap(),
        0.0,
        "clamped, not negative: {v}"
    );
    assert!((v["end_sec"].as_f64().unwrap() - 1.6).abs() < 0.01, "{v}");
}

/// Selecting by what was said needs to know what was said.
#[test]
fn selecting_by_text_without_a_transcript_says_what_to_run() {
    let mut s = Session::new();
    let msg = err(s.call("select_region", json!({ "text": "anything" })));
    assert!(msg.contains("transcribe"), "{msg}");
}

/// Two selectors mean two different regions; picking one silently would
/// ignore half of what was asked.
#[test]
fn giving_two_selectors_is_refused() {
    let mut s = Session::new();
    s.with_transcript(&transcript());

    let msg = err(s.call(
        "select_region",
        json!({ "text": "latency", "speech_passage": 1 }),
    ));
    assert!(msg.contains("one selector"), "{msg}");
}

/// And giving none says what the options are.
#[test]
fn giving_no_selector_says_what_to_pass() {
    let mut s = Session::new();
    let msg = err(s.call("select_region", json!({})));
    assert!(msg.contains("speech_passage"), "{msg}");
}

/// The point of the whole thing: the resolved range feeds any tool that
/// takes one, with no changes to that tool.
#[test]
fn a_resolved_range_drives_an_existing_range_taking_tool() {
    let mut s = Session::new();
    s.with_transcript(&transcript());

    let sel = ok(s.call("select_region", json!({ "text": "the latency" })));
    let (start, end) = (
        sel["start_sec"].as_f64().unwrap(),
        sel["end_sec"].as_f64().unwrap(),
    );

    // `silence_region` knew nothing about descriptions and needed no
    // changes to be driven by one.
    let v = ok(s.call(
        "silence_region",
        json!({ "track": 0, "start_sec": start, "end_sec": end }),
    ));
    assert!(v["node_id"].is_string(), "the edit landed: {v}");
}

// =============================================================================
// A passage of speech, found in a voice track's audio
// =============================================================================
//
// `transcribe` is a stub (#384), so a session in a shipped build has no
// transcript, and `speech_passage` could only fail. Which passage is "the
// last thing said" needs where the speech is, not what was said, and
// `voice_tracks` answers that from the audio with the detector
// `duck_under_speech` uses (`speech_detect.rs`): voiced (pitched) sound
// counts, a breath, a click or hiss does not. `text` still needs the words.

/// Three lines on one voice track: 2-3 s, 10-11 s and 15-16.5 s of 24.
fn three_lines() -> Vec<f32> {
    track_with(
        SR,
        24.0,
        &[
            (2.0, voice_burst(SR, 1.0)),
            (10.0, voice_burst(SR, 1.0)),
            (15.0, voice_burst(SR, 1.5)),
        ],
    )
}

/// A session whose only track is `voice`, with no transcript.
fn voice_session(voice: Vec<f32>) -> Session {
    let s = Session::with_tracks(&[(SR, voice)]);
    assert!(
        s.state().transcript.is_none(),
        "this session has no transcript"
    );
    s
}

/// `v[key]` within `tol` seconds of `want`.
fn assert_near(v: &Value, key: &str, want: f64, tol: f64) {
    let got = v[key].as_f64().unwrap_or_else(|| panic!("no {key} in {v}"));
    assert!(
        (got - want).abs() <= tol,
        "{key}: wanted {want} ± {tol}, got {got}: {v}"
    );
}

/// **The headline case.** "The last thing said", with no transcript.
#[test]
fn the_last_passage_comes_from_the_voice_audio() {
    let mut s = voice_session(three_lines());

    let v = ok(s.call(
        "select_region",
        json!({ "speech_passage": -1, "voice_tracks": [0] }),
    ));
    assert_near(&v, "start_sec", 15.0, 0.1);
    assert_near(&v, "end_sec", 16.5, 0.1);
    assert_eq!(v["keyed_on"], json!("audio"), "{v}");
    assert_eq!(v["voice_tracks"], json!([0]), "{v}");
}

#[test]
fn the_first_passage_comes_from_the_voice_audio() {
    let mut s = voice_session(three_lines());

    let v = ok(s.call(
        "select_region",
        json!({ "speech_passage": 1, "voice_tracks": [0] }),
    ));
    assert_near(&v, "start_sec", 2.0, 0.1);
    assert_near(&v, "end_sec", 3.0, 0.1);
    assert_eq!(v["keyed_on"], json!("audio"), "{v}");
}

/// 1 and -3 are the same passage, 3 and -1 are, and 0 is not one: the
/// index is 1-based, as in the transcript route.
#[test]
fn passages_are_counted_from_either_end() {
    let mut s = voice_session(three_lines());
    let lines = [(2.0, 3.0), (10.0, 11.0), (15.0, 16.5)];

    for (which, line) in [(1, 0), (2, 1), (3, 2), (-1, 2), (-2, 1), (-3, 0)] {
        let v = ok(s.call(
            "select_region",
            json!({ "speech_passage": which, "voice_tracks": [0] }),
        ));
        assert_near(&v, "start_sec", lines[line].0, 0.1);
        assert_near(&v, "end_sec", lines[line].1, 0.1);
        let matched = v["matched"].as_str().unwrap();
        assert!(
            matched.contains(&format!("passage {} of 3", line + 1)),
            "{which} should read as passage {} of 3: {matched}",
            line + 1
        );
    }
}

/// Past either end, or zero, is an error that says how many there are —
/// not a clamp to the nearest one, which would hand the next edit the
/// wrong audio.
#[test]
fn a_passage_that_does_not_exist_says_how_many_there_are() {
    let mut s = voice_session(three_lines());

    for which in [0, 4, -4, 100] {
        let msg = err(s.call(
            "select_region",
            json!({ "speech_passage": which, "voice_tracks": [0] }),
        ));
        assert!(msg.contains("3 passage"), "{which}: {msg}");
        assert!(
            msg.contains(&which.to_string()),
            "names what was asked: {msg}"
        );
    }
}

/// The transcript route says the same, with the same words.
#[test]
fn a_passage_that_does_not_exist_in_the_transcript_says_how_many_there_are() {
    let mut s = Session::new();
    s.with_transcript(&transcript());

    for which in [0, 3, -3] {
        let msg = err(s.call("select_region", json!({ "speech_passage": which })));
        assert!(msg.contains("2 passage"), "{which}: {msg}");
    }
}

/// **The same joining rule as `duck_under_speech`.** A pause shorter than a
/// second is inside one passage: "the third thing he said" must mean the
/// same to both tools, or a selection and a duck would disagree about
/// where a line ends.
#[test]
fn a_short_pause_is_inside_one_passage_as_in_ducking() {
    // 2-3 s and 3.6-4.6 s are 0.6 s apart: one passage. 10-11 s is another.
    let voice = track_with(
        SR,
        20.0,
        &[
            (2.0, voice_burst(SR, 1.0)),
            (3.6, voice_burst(SR, 1.0)),
            (10.0, voice_burst(SR, 1.0)),
        ],
    );
    let music: Vec<f32> = (0..SR as usize * 20)
        .map(|n| (2.0 * std::f32::consts::PI * 440.0 * n as f32 / SR as f32).sin() * 0.25)
        .collect();
    let mut s = Session::with_tracks(&[(SR, voice), (SR, music)]);

    let first = ok(s.call(
        "select_region",
        json!({ "speech_passage": 1, "voice_tracks": [0] }),
    ));
    assert_near(&first, "start_sec", 2.0, 0.1);
    assert_near(&first, "end_sec", 4.6, 0.1);
    let msg = err(s.call(
        "select_region",
        json!({ "speech_passage": 3, "voice_tracks": [0] }),
    ));
    assert!(msg.contains("2 passage"), "{msg}");

    let ducked = ok(s.call(
        "duck_under_speech",
        json!({ "music_track": 1, "voice_tracks": [0] }),
    ));
    assert_eq!(
        ducked["passages"],
        json!(2),
        "the duck found the same two: {ducked}"
    );
}

/// A breath as loud as the voice is not a passage, so it is not "the last
/// thing said" either.
#[test]
fn a_breath_after_the_last_line_is_not_a_passage() {
    let line = voice_burst(SR, 1.0);
    let rms = (line.iter().map(|s| s * s).sum::<f32>() / line.len() as f32).sqrt();
    let voice = track_with(
        SR,
        20.0,
        &[
            (2.0, line.clone()),
            (10.0, line),
            (15.0, noise_burst(SR, 0.5, rms)),
        ],
    );
    let mut s = voice_session(voice);

    let v = ok(s.call(
        "select_region",
        json!({ "speech_passage": -1, "voice_tracks": [0] }),
    ));
    assert_near(&v, "start_sec", 10.0, 0.1);
    assert_near(&v, "end_sec", 11.0, 0.1);
}

/// The speech is where the clip plays, not where it sits in its file.
#[test]
fn the_passage_is_read_where_the_clip_sits_on_the_timeline() {
    let mut s = voice_session(three_lines());
    ok(s.call(
        "move_clip",
        json!({ "track": 0, "clip_index": 0, "start_sec": 5.0 }),
    ));

    let v = ok(s.call(
        "select_region",
        json!({ "speech_passage": -1, "voice_tracks": [0] }),
    ));
    assert_near(&v, "start_sec", 20.0, 0.1);
    assert_near(&v, "end_sec", 21.5, 0.1);
}

/// A line on either voice track is a passage, in time order.
#[test]
fn two_voice_tracks_are_one_timeline_of_passages() {
    let a = track_with(SR, 20.0, &[(2.0, voice_burst(SR, 1.0))]);
    let b = track_with(SR, 20.0, &[(10.0, voice_burst(SR, 1.0))]);
    let mut s = Session::with_tracks(&[(SR, a), (SR, b)]);

    let last = ok(s.call(
        "select_region",
        json!({ "speech_passage": -1, "voice_tracks": [0, 1] }),
    ));
    assert_near(&last, "start_sec", 10.0, 0.1);
    assert_eq!(last["voice_tracks"], json!([0, 1]), "{last}");

    let first = ok(s.call(
        "select_region",
        json!({ "speech_passage": 1, "voice_tracks": [1, 0] }),
    ));
    assert_near(&first, "start_sec", 2.0, 0.1);

    // And either alone is only its own line.
    let only_a = ok(s.call(
        "select_region",
        json!({ "speech_passage": -1, "voice_tracks": [0] }),
    ));
    assert_near(&only_a, "start_sec", 2.0, 0.1);
}

/// Naming the same track twice is one track, not two.
#[test]
fn a_repeated_voice_track_counts_once() {
    let mut s = voice_session(three_lines());
    let v = ok(s.call(
        "select_region",
        json!({ "speech_passage": 3, "voice_tracks": [0, 0] }),
    ));
    assert_eq!(v["voice_tracks"], json!([0]), "{v}");
    assert_near(&v, "start_sec", 15.0, 0.1);
}

/// `pad_sec` widens an audio-keyed range the way it widens any other.
#[test]
fn padding_widens_a_passage_found_in_audio() {
    let mut s = voice_session(three_lines());
    let v = ok(s.call(
        "select_region",
        json!({ "speech_passage": 1, "voice_tracks": [0], "pad_sec": 0.5 }),
    ));
    assert_near(&v, "start_sec", 1.5, 0.1);
    assert_near(&v, "end_sec", 3.5, 0.1);
}

/// **A track named as the voice wins over a transcript.** The transcript
/// is session-level and cannot say which track it belongs to; a track the
/// caller named can.
#[test]
fn voice_tracks_wins_over_a_transcript() {
    let mut s = voice_session(three_lines());
    s.with_transcript(&[("elsewhere", 5.0, 6.0)]);

    let v = ok(s.call(
        "select_region",
        json!({ "speech_passage": -1, "voice_tracks": [0] }),
    ));
    assert_eq!(v["keyed_on"], json!("audio"), "{v}");
    assert_near(&v, "start_sec", 15.0, 0.1);
}

/// **The transcript route is unchanged** when `voice_tracks` is left out:
/// same ranges, now reporting that it keyed on the transcript. The audio
/// here would answer differently, so a pass means the transcript was read.
#[test]
fn without_voice_tracks_a_transcript_is_still_what_it_reads() {
    let mut s = voice_session(three_lines());
    s.with_transcript(&transcript());

    let last = ok(s.call("select_region", json!({ "speech_passage": -1 })));
    assert_near(&last, "start_sec", 12.0, 0.01);
    assert_near(&last, "end_sec", 15.0, 0.01);
    assert_eq!(last["keyed_on"], json!("transcript"), "{last}");
    assert!(
        last.get("voice_tracks").is_none(),
        "no tracks were read: {last}"
    );

    let first = ok(s.call("select_region", json!({ "speech_passage": 1 })));
    assert_near(&first, "start_sec", 2.0, 0.01);
    assert_eq!(first["keyed_on"], json!("transcript"), "{first}");
    assert_eq!(
        first["matched"],
        json!("passage 1 of 2 of speech"),
        "{first}"
    );
}

/// Every result says what it keyed on, so a caller never has to guess
/// whether a range came from words, sound or the tempo map.
#[test]
fn text_and_beat_ranges_report_what_they_keyed_on() {
    let mut s = Session::new();
    s.with_transcript(&transcript());

    let text = ok(s.call("select_region", json!({ "text": "latency" })));
    assert_eq!(text["keyed_on"], json!("transcript"), "{text}");
    let beats = ok(s.call("select_region", json!({ "from_beat": 1, "to_beat": 3 })));
    assert_eq!(beats["keyed_on"], json!("tempo_map"), "{beats}");
}

/// With neither a transcript nor `voice_tracks` there is nothing to read,
/// and the error says which argument would give it something.
#[test]
fn a_passage_with_neither_a_transcript_nor_voice_tracks_names_voice_tracks() {
    let mut s = voice_session(three_lines());

    let msg = err(s.call("select_region", json!({ "speech_passage": -1 })));
    assert!(msg.contains("voice_tracks"), "{msg}");
    assert!(msg.contains("no transcript"), "{msg}");
}

/// **`text` needs the words.** The audio can say where speech is, not what
/// was said, so a phrase is refused alongside `voice_tracks` — even when a
/// transcript exists, so it is the combination that is refused and not a
/// missing transcript — instead of ignoring one of the two.
#[test]
fn voice_tracks_is_refused_with_text() {
    let mut s = voice_session(three_lines());
    s.with_transcript(&transcript());

    let msg = err(s.call(
        "select_region",
        json!({ "text": "latency", "voice_tracks": [0] }),
    ));
    assert!(msg.contains("voice_tracks"), "{msg}");
    assert!(msg.contains("text"), "{msg}");
    assert!(
        msg.contains("what was said"),
        "says why audio cannot find a phrase: {msg}"
    );
}

/// A beat range comes from the tempo map; there is no audio to look at.
#[test]
fn voice_tracks_is_refused_with_a_beat_range() {
    let mut s = voice_session(three_lines());

    let msg = err(s.call(
        "select_region",
        json!({ "from_beat": 1, "to_beat": 9, "voice_tracks": [0] }),
    ));
    assert!(msg.contains("voice_tracks"), "{msg}");
    assert!(msg.contains("beat"), "{msg}");
}

/// A track that is not in the session is named, not read as silence.
#[test]
fn a_voice_track_that_does_not_exist_is_refused() {
    let mut s = voice_session(three_lines());

    let msg = err(s.call(
        "select_region",
        json!({ "speech_passage": -1, "voice_tracks": [7] }),
    ));
    assert!(msg.contains("out of range"), "{msg}");
    // One bad track among good ones refuses the lot.
    let msg = err(s.call(
        "select_region",
        json!({ "speech_passage": -1, "voice_tracks": [0, 7] }),
    ));
    assert!(msg.contains("out of range"), "{msg}");
}

#[test]
fn a_voice_track_with_no_clips_is_refused() {
    let mut s = voice_session(three_lines());
    ok(s.call("remove_clip", json!({ "track": 0, "clip_index": 0 })));

    let msg = err(s.call(
        "select_region",
        json!({ "speech_passage": -1, "voice_tracks": [0] }),
    ));
    assert!(msg.contains("no clips"), "{msg}");
}

/// An empty list is a mistake to report, not "no voice, use the
/// transcript". The schema refuses it before the tool sees it.
#[test]
fn an_empty_voice_tracks_list_is_refused() {
    let mut s = voice_session(three_lines());
    s.with_transcript(&transcript());
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
            "select_region",
            json!({ "speech_passage": -1, "voice_tracks": [] }),
            &mut ctx,
        )
        .expect_err("an empty voice_tracks must not reach the tool");
    let refused = refused.to_string();
    assert!(refused.contains("voice_tracks"), "{refused}");
    assert!(
        refused.contains("less than 1 item"),
        "refused for being empty, not for being unknown: {refused}"
    );
}

/// **Finding nothing is a refusal.** A track with no speech has no last
/// thing said, and a range invented for it would be the nearest thing to
/// nothing, handed to the next edit.
#[test]
fn a_silent_voice_track_has_no_passage() {
    let mut s = voice_session(vec![0.0; SR as usize * 20]);

    let msg = err(s.call(
        "select_region",
        json!({ "speech_passage": -1, "voice_tracks": [0] }),
    ));
    assert!(msg.contains("no speech"), "{msg}");
    assert!(msg.contains('0'), "names the track: {msg}");
}

/// Noise alone is not speech either, however loud.
#[test]
fn a_voice_track_of_only_hiss_has_no_passage() {
    let mut s = voice_session(noise_burst(SR, 20.0, 0.05));

    let msg = err(s.call(
        "select_region",
        json!({ "speech_passage": 1, "voice_tracks": [0] }),
    ));
    assert!(msg.contains("no speech"), "{msg}");
}

/// A track with nothing on it does not hide the one that has the voice.
#[test]
fn a_silent_track_beside_a_voice_does_not_hide_it() {
    let mut s = Session::with_tracks(&[(SR, vec![0.0; SR as usize * 24]), (SR, three_lines())]);

    let v = ok(s.call(
        "select_region",
        json!({ "speech_passage": -1, "voice_tracks": [0, 1] }),
    ));
    assert_near(&v, "start_sec", 15.0, 0.1);
}

/// Selecting is a read: no node, no new files, and the voice is untouched.
#[test]
fn selecting_from_audio_changes_nothing() {
    let mut s = voice_session(three_lines());
    let head = s.store.head();
    let nodes = s.store.list_nodes().expect("list nodes").len();
    let tracks = s.state().tracks.clone();
    let files = files_under(s._dir.path());

    ok(s.call(
        "select_region",
        json!({ "speech_passage": -1, "voice_tracks": [0] }),
    ));

    assert_eq!(s.store.head(), head, "the head did not move");
    assert_eq!(s.store.list_nodes().expect("list nodes").len(), nodes);
    assert_eq!(s.state().tracks, tracks, "the voice was not touched");
    assert_eq!(files_under(s._dir.path()), files, "nothing was written");
}

/// Every file under `dir`, sorted, so two listings can be compared.
fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(d) = pending.pop() {
        for entry in std::fs::read_dir(&d).expect("read dir").flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

/// The range from audio feeds an existing range-taking tool, as one from
/// the transcript does.
#[test]
fn a_range_found_in_audio_drives_an_existing_range_taking_tool() {
    let mut s = voice_session(three_lines());

    let sel = ok(s.call(
        "select_region",
        json!({ "speech_passage": -1, "voice_tracks": [0] }),
    ));
    let v = ok(s.call(
        "silence_region",
        json!({
            "track": 0,
            "start_sec": sel["start_sec"],
            "end_sec": sel["end_sec"],
        }),
    ));
    assert!(v["node_id"].is_string(), "the edit landed: {v}");

    // The last line is gone, so the last thing said is now the one before.
    let after = ok(s.call(
        "select_region",
        json!({ "speech_passage": -1, "voice_tracks": [0] }),
    ));
    assert_near(&after, "start_sec", 10.0, 0.1);
}

/// **The tool says what the audio mode cannot do**, where the model reads
/// it: a voice track that carries music reads as speech, and the
/// thresholds were set on synthetic recordings. The slim tool set keeps
/// only the first sentence of a parameter's description, so that sentence
/// has to say when `voice_tracks` is needed.
#[test]
fn the_schema_states_what_the_audio_mode_cannot_do() {
    use tools::Tool;
    let schema = tools::tool::SelectRegionTool.schema();
    let voice_tracks = &schema["input_schema"]["properties"]["voice_tracks"];
    assert_eq!(voice_tracks["type"], json!("array"), "{voice_tracks}");
    assert_eq!(voice_tracks["minItems"], json!(1), "{voice_tracks}");

    let text = voice_tracks["description"].as_str().expect("a description");
    let first = text.split(". ").next().unwrap();
    assert!(
        first.contains("no transcript"),
        "the first sentence must say when it is needed: {first:?}"
    );
    assert!(
        text.contains("music"),
        "a track with music reads as speech: {text}"
    );
    assert!(
        text.contains("synthetic"),
        "thresholds are untested on real takes: {text}"
    );

    let tool = schema["description"].as_str().expect("a description");
    assert!(tool.contains("voice_tracks"), "{tool}");
    assert!(
        tool.contains("`text`") && tool.contains("words"),
        "and that a phrase still needs the transcript: {tool}"
    );
}
