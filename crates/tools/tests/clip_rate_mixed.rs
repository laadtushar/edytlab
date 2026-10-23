//! Tools that turn a time into clip frames, on a track whose rate is not the
//! session's (#673, after #672).
//!
//! A clip's `start_in_track`, `source_offset` and `length` are counted in the
//! frames of its source file, not the session's (#234), and the session takes
//! the rate of the first load. So with a 48 kHz (or 44.1 kHz) bed loaded first
//! and a 16 kHz voice second, the same second is 48 000 frames on track 0 and
//! 16 000 on track 1. `cut_words` and `remove_fillers` were fixed in
//! `cut_words_mixed_rate.rs`; these are the other tools that make the same
//! conversion, or hand a time on one track to another.

use std::path::{Path, PathBuf};

use hound::{SampleFormat, WavSpec, WavWriter};
use serde_json::{json, Value};
use tempfile::TempDir;
use tools::{ToolContext, ToolDispatcher, ToolResult};

/// The voice track's own rate.
const VOICE_RATE: u32 = 16_000;
const VOICE_SECONDS: usize = 4;
const BED_SECONDS: usize = 6;

/// A mono 16-bit WAV of a 440 Hz sine whose loudness is shaped by `amp(t)`.
fn write_wav(path: &Path, rate: u32, seconds: usize, amp: impl Fn(f32) -> f32) -> PathBuf {
    write_signal(path, rate, seconds, |t| {
        (2.0 * std::f32::consts::PI * 440.0 * t).sin() * 0.25 * amp(t)
    })
}

/// A mono 16-bit WAV of `signal(t)`, `t` in seconds.
fn write_signal(path: &Path, rate: u32, seconds: usize, signal: impl Fn(f32) -> f32) -> PathBuf {
    let spec = WavSpec {
        channels: 1,
        sample_rate: rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut w = WavWriter::create(path, spec).expect("wav writer");
    for n in 0..(rate as usize * seconds) {
        let t = n as f32 / rate as f32;
        w.write_sample((signal(t) * 32_767.0) as i16).unwrap();
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
    session_rate: u32,
}

impl Session {
    /// Track 0 is a bed at `session_rate` (so the session is at that rate);
    /// track 1 is a 16 kHz voice, four seconds long.
    fn new(session_rate: u32) -> Self {
        Self::with_voice(session_rate, |_| 1.0)
    }

    /// As `new`, with the voice's loudness shaped by `amp(t)`.
    fn with_voice(session_rate: u32, amp: impl Fn(f32) -> f32) -> Self {
        Self::with_signal(session_rate, move |t| {
            (2.0 * std::f32::consts::PI * 440.0 * t).sin() * 0.25 * amp(t)
        })
    }

    /// As `new`, with the voice track holding `signal(t)`.
    fn with_signal(session_rate: u32, signal: impl Fn(f32) -> f32) -> Self {
        let dir = TempDir::new().expect("tempdir");
        let bed = write_wav(
            &dir.path().join("bed.wav"),
            session_rate,
            BED_SECONDS,
            |_| 1.0,
        );
        let voice = write_signal(
            &dir.path().join("voice.wav"),
            VOICE_RATE,
            VOICE_SECONDS,
            signal,
        );
        let store = session::Store::open(dir.path()).expect("open store");
        let mut s = Self {
            dir,
            store,
            engine: audio_engine::Engine::new(),
            dispatcher: ToolDispatcher::default_dispatcher(),
            clipboard: None,
            session_rate,
        };
        s.call("load", json!({ "path": bed.to_string_lossy() }));
        s.call("load", json!({ "path": voice.to_string_lossy() }));
        assert_eq!(s.state().sample_rate, session_rate, "the bed sets the rate");
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

    fn ok(&mut self, tool: &str, args: Value) -> Value {
        match self.call(tool, args) {
            ToolResult::Ok(v) => v,
            ToolResult::Error(m) => panic!("{tool}: expected Ok, got Error({m})"),
        }
    }

    fn err(&mut self, tool: &str, args: Value) -> String {
        match self.call(tool, args) {
            ToolResult::Error(m) => m,
            ToolResult::Ok(v) => panic!("{tool}: expected Error, got Ok({v})"),
        }
    }

    fn state(&self) -> session::SessionState {
        let head = self.store.head().expect("a head");
        self.store.get(head).expect("head node").state
    }

    /// A track's clips as (start_in_track, source_offset, length), in the
    /// frames of each clip's own file.
    fn clips(&self, track: usize) -> Vec<(u64, u64, u64)> {
        self.state().tracks[track]
            .clips
            .iter()
            .map(|c| (c.start_in_track, c.source_offset, c.length))
            .collect()
    }

    /// The point markers' times in seconds, in order.
    fn markers(&self) -> Vec<f64> {
        self.state()
            .annotations
            .iter()
            .filter_map(|a| match a.kind {
                session::AnnotationKind::Marker { time_sec } => Some(time_sec),
                _ => None,
            })
            .collect()
    }

    fn marker_at(&mut self, name: &str, time: f64) {
        self.ok("label", json!({ "name": name, "time": time }));
    }

    /// Words in seconds, of track 1, as `transcribe` leaves them.
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

    fn words(&self) -> Vec<(String, f32, f32)> {
        self.state()
            .transcript
            .expect("a transcript")
            .words
            .into_iter()
            .map(|w| (w.text, w.start_s, w.end_s))
            .collect()
    }

    fn bed_rate(&self) -> u64 {
        u64::from(self.session_rate)
    }
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-3
}

/// "one two um three": one second each, except the filler, which is half a
/// second.
fn words() -> Vec<(&'static str, f32, f32)> {
    vec![
        ("one", 0.0, 1.0),
        ("two", 1.0, 2.0),
        ("um", 2.0, 2.5),
        ("three", 2.5, 4.0),
    ]
}

/// The rates the session can take from the first load: both are common and
/// neither is a multiple of 16 kHz, which is the case a ratio hides.
const SESSION_RATES: [u32; 2] = [48_000, 44_100];

// ---------------------------------------------------------------------------
// cut_range
// ---------------------------------------------------------------------------

/// `cut_range` takes frames of the track it names. The words and labels that
/// follow it are in seconds, so the span it removed is those frames over the
/// track's own rate: the second second of a 16 kHz voice is 16 000 to 32 000.
#[test]
fn cut_range_moves_labels_and_words_by_the_tracks_own_rate() {
    for rate in SESSION_RATES {
        let mut s = Session::new(rate);
        s.with_transcript(&words());
        s.marker_at("inside", 1.5);
        s.marker_at("after", 3.0);

        let r = u64::from(VOICE_RATE);
        let v = s.ok(
            "cut_range",
            json!({ "track": 1, "start_sample": r, "end_sample": 2 * r }),
        );

        assert_eq!(v["dropped_labels"], json!(1), "session at {rate}");
        let markers = s.markers();
        assert_eq!(markers.len(), 1, "session at {rate}");
        assert!(
            near(markers[0], 2.0),
            "the marker at 3.0 s is a second earlier, got {} (session at {rate})",
            markers[0]
        );
        let words = s.words();
        let names: Vec<&str> = words.iter().map(|w| w.0.as_str()).collect();
        assert_eq!(names, ["one", "um", "three"], "session at {rate}");
        assert!(near(f64::from(words[1].1), 1.0), "{words:?} at {rate}");
        assert!(near(f64::from(words[2].2), 3.0), "{words:?} at {rate}");
    }
}

/// With sync-lock on, a cut on one track removes the same *time* from the
/// others, each counted in its own frames.
#[test]
fn cut_range_sync_lock_cuts_the_other_track_at_the_same_seconds() {
    for rate in SESSION_RATES {
        // Cut the bed; the voice follows.
        let mut s = Session::new(rate);
        s.ok("set_sync_lock", json!({ "enabled": true }));
        let b = s.bed_rate();
        let r = u64::from(VOICE_RATE);
        s.ok(
            "cut_range",
            json!({ "track": 0, "start_sample": b, "end_sample": 2 * b }),
        );
        assert_eq!(
            s.clips(1),
            vec![(0, 0, r), (r, 2 * r, 2 * r)],
            "the voice loses its second second (session at {rate})"
        );
        assert_eq!(s.clips(0).len(), 2, "session at {rate}");

        // Cut the voice; the bed follows.
        let mut s = Session::new(rate);
        s.ok("set_sync_lock", json!({ "enabled": true }));
        s.ok(
            "cut_range",
            json!({ "track": 1, "start_sample": r, "end_sample": 2 * r }),
        );
        assert_eq!(
            s.clips(0),
            vec![(0, 0, b), (b, 2 * b, (BED_SECONDS as u64 - 2) * b)],
            "the bed loses its second second (session at {rate})"
        );
    }
}

// ---------------------------------------------------------------------------
// trim
// ---------------------------------------------------------------------------

/// `trim` keeps frames of the track it names and discards the rest, so the
/// labels and words outside the window are the seconds those frames name.
#[test]
fn trim_moves_labels_and_words_by_the_tracks_own_rate() {
    for rate in SESSION_RATES {
        let mut s = Session::new(rate);
        s.with_transcript(&words());
        s.marker_at("before", 0.5);
        s.marker_at("inside", 2.0);
        s.marker_at("after", 3.5);

        // Keep seconds 1 to 3 of the voice.
        let r = u64::from(VOICE_RATE);
        s.ok(
            "trim",
            json!({ "track": 1, "start_sample": r, "end_sample": 3 * r }),
        );

        assert_eq!(s.clips(1), vec![(0, r, 2 * r)], "session at {rate}");
        let markers = s.markers();
        assert_eq!(markers.len(), 1, "only the marker inside stays ({rate})");
        assert!(near(markers[0], 1.0), "{markers:?} (session at {rate})");
        let words = s.words();
        let names: Vec<&str> = words.iter().map(|w| w.0.as_str()).collect();
        // "three" runs 2.5 to 4.0 s and is cut by the window's end at 3.0 s.
        assert_eq!(
            names,
            ["two", "um", "three"],
            "session at {rate}: {words:?}"
        );
        assert!(near(f64::from(words[0].1), 0.0), "{words:?} at {rate}");
        assert!(near(f64::from(words[1].2), 1.5), "{words:?} at {rate}");
        assert!(near(f64::from(words[2].1), 1.5), "{words:?} at {rate}");
        assert!(near(f64::from(words[2].2), 2.0), "{words:?} at {rate}");
    }
}

// ---------------------------------------------------------------------------
// split_clip
// ---------------------------------------------------------------------------

/// `at_sec` is a position on the track, so a clip of a 16 kHz file is split
/// at 16 000 frames a second.
#[test]
fn split_clip_splits_at_seconds_in_the_clips_own_frames() {
    for rate in SESSION_RATES {
        let mut s = Session::new(rate);
        let r = u64::from(VOICE_RATE);
        s.ok(
            "split_clip",
            json!({ "track": 1, "clip_index": 0, "at_sec": 2.0 }),
        );
        assert_eq!(
            s.clips(1),
            vec![(0, 0, 2 * r), (2 * r, 2 * r, 2 * r)],
            "session at {rate}"
        );
    }
}

/// A clip that is not the first on its track is split by where it sits.
#[test]
fn split_clip_on_a_later_clip_uses_the_same_rate() {
    let mut s = Session::new(48_000);
    let r = u64::from(VOICE_RATE);
    s.ok(
        "split_clip",
        json!({ "track": 1, "clip_index": 0, "at_sec": 1.0 }),
    );
    s.ok(
        "split_clip",
        json!({ "track": 1, "clip_index": 1, "at_sec": 3.0 }),
    );
    assert_eq!(
        s.clips(1),
        vec![(0, 0, r), (r, r, 2 * r), (3 * r, 3 * r, r)]
    );
}

// ---------------------------------------------------------------------------
// time_shift
// ---------------------------------------------------------------------------

/// A shift of one second moves the voice 16 000 frames, and says one second.
#[test]
fn time_shift_moves_a_track_by_seconds_in_its_own_frames() {
    for rate in SESSION_RATES {
        let mut s = Session::new(rate);
        let r = u64::from(VOICE_RATE);
        let bed_before = s.state().tracks[0].clips.clone();
        let v = s.ok("time_shift", json!({ "track": 1, "offset_sec": 1.0 }));
        assert_eq!(s.clips(1), vec![(r, 0, 4 * r)], "session at {rate}");
        assert!(near(v["applied_sec"].as_f64().unwrap(), 1.0));
        assert_eq!(s.state().tracks[0].clips, bed_before, "the bed stays");
    }
}

/// Moving it back past the start clamps at the start, in seconds.
#[test]
fn time_shift_clamps_at_the_start_and_reports_the_seconds_applied() {
    let mut s = Session::new(48_000);
    let r = u64::from(VOICE_RATE);
    s.ok("time_shift", json!({ "track": 1, "offset_sec": 1.0 }));
    let v = s.ok("time_shift", json!({ "track": 1, "offset_sec": -3.0 }));
    assert_eq!(s.clips(1), vec![(0, 0, 4 * r)]);
    assert_eq!(v["clamped"], json!(true));
    assert!(
        near(v["applied_sec"].as_f64().unwrap(), -1.0),
        "{}",
        v["applied_sec"]
    );
}

// ---------------------------------------------------------------------------
// split_by_speaker
// ---------------------------------------------------------------------------

/// Segments are in seconds; the slices are in the voice file's frames.
#[test]
fn split_by_speaker_cuts_a_voice_track_at_its_own_rate() {
    for rate in SESSION_RATES {
        let mut s = Session::new(rate);
        let r = u64::from(VOICE_RATE);
        let v = s.ok(
            "split_by_speaker",
            json!({
                "track": 1,
                "segments": [
                    { "start_sec": 0.0, "end_sec": 1.0, "speaker": "A" },
                    { "start_sec": 2.0, "end_sec": 3.0, "speaker": "B" },
                ],
            }),
        );
        let state = s.state();
        let names: Vec<&str> = state.tracks.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names.len(), 4, "bed, A, B, unassigned: {names:?} ({rate})");
        let clips = |i: usize| -> Vec<(u64, u64, u64)> {
            state.tracks[i]
                .clips
                .iter()
                .map(|c| (c.start_in_track, c.source_offset, c.length))
                .collect()
        };
        assert_eq!(clips(1), vec![(0, 0, r)], "A, session at {rate}");
        assert_eq!(clips(2), vec![(2 * r, 2 * r, r)], "B, session at {rate}");
        assert_eq!(
            clips(3),
            vec![(r, r, r), (3 * r, 3 * r, r)],
            "what no segment covered, session at {rate}"
        );
        // The report counts the frames of the track it split, and speaks in
        // that track's seconds.
        assert_eq!(v["unassigned_samples"], json!(2 * r), "session at {rate}");
        let summary = v["summary"].as_str().unwrap();
        assert!(summary.contains("2.00s"), "{summary}");
    }
}

// ---------------------------------------------------------------------------
// punch_in
// ---------------------------------------------------------------------------

/// The take has to be at the *track's* rate, not the session's: a 16 kHz take
/// belongs in a 16 kHz track whatever the first load was.
#[test]
fn punch_in_accepts_a_take_at_the_tracks_rate() {
    let mut s = Session::new(48_000);
    let take = write_wav(&s.dir.path().join("take16.wav"), VOICE_RATE, 1, |_| 0.5);
    let r = u64::from(VOICE_RATE);

    let v = s.ok(
        "punch_in",
        json!({
            "track": 1,
            "start_sec": 1.0,
            "end_sec": 2.0,
            "take_path": take.to_string_lossy(),
        }),
    );

    assert!(near(v["region_sec"].as_f64().unwrap(), 1.0), "{v}");
    assert!(near(v["padded_sec"].as_f64().unwrap(), 0.0), "{v}");
    // The edit collapses the track to one clip of the same length.
    let clips = s.clips(1);
    assert_eq!(clips.len(), 1);
    assert_eq!(clips[0].2, 4 * r, "still four seconds of 16 kHz");
}

/// A take at the session's rate is the wrong one for a 16 kHz track: spliced
/// in as it is, it would play three times too slow.
#[test]
fn punch_in_refuses_a_take_at_another_rate_than_the_tracks() {
    let mut s = Session::new(48_000);
    let take = write_wav(&s.dir.path().join("take48.wav"), 48_000, 1, |_| 0.5);
    let before = s.state().tracks[1].clips.clone();

    let msg = s.err(
        "punch_in",
        json!({
            "track": 1,
            "start_sec": 1.0,
            "end_sec": 2.0,
            "take_path": take.to_string_lossy(),
        }),
    );

    assert!(
        msg.contains("48000 Hz") && msg.contains("16000 Hz"),
        "{msg}"
    );
    assert_eq!(s.state().tracks[1].clips, before, "nothing changed");
}

/// The bed is the other way round: its rate is the session's, so a 48 kHz
/// take goes in and a 16 kHz one does not.
#[test]
fn punch_in_on_the_bed_still_takes_the_session_rate() {
    let mut s = Session::new(48_000);
    let good = write_wav(&s.dir.path().join("take48.wav"), 48_000, 1, |_| 0.5);
    let bad = write_wav(&s.dir.path().join("take16.wav"), VOICE_RATE, 1, |_| 0.5);

    s.ok(
        "punch_in",
        json!({ "track": 0, "start_sec": 1.0, "end_sec": 2.0, "take_path": good.to_string_lossy() }),
    );
    let msg = s.err(
        "punch_in",
        json!({ "track": 0, "start_sec": 1.0, "end_sec": 2.0, "take_path": bad.to_string_lossy() }),
    );
    assert!(
        msg.contains("16000 Hz") && msg.contains("48000 Hz"),
        "{msg}"
    );
}

// ---------------------------------------------------------------------------
// truncate_silence
// ---------------------------------------------------------------------------

/// A second of voice, a second of silence, a second of voice. The silence is
/// found in the track's frames, so the labels after it move by its seconds.
#[test]
fn truncate_silence_moves_labels_by_the_tracks_own_rate() {
    for rate in SESSION_RATES {
        let mut s = Session::with_voice(rate, |t| if (1.0..2.0).contains(&t) { 0.0 } else { 1.0 });
        s.marker_at("after", 2.5);
        let r = u64::from(VOICE_RATE);

        s.ok(
            "truncate_silence",
            json!({ "track": 1, "threshold_db": -60.0, "min_silence_ms": 500.0 }),
        );

        let clips = s.clips(1);
        assert_eq!(clips.len(), 1);
        let length = clips[0].2;
        assert!(
            length.abs_diff(3 * r) < r / 50,
            "about a second came out of the voice, got {length} frames"
        );
        let markers = s.markers();
        assert_eq!(markers.len(), 1);
        assert!(
            near(markers[0], 1.5),
            "the label at 2.5 s moves a second earlier, got {} (session at {rate})",
            markers[0]
        );
    }
}

// ---------------------------------------------------------------------------
// insert_silence and paste_region, with sync-lock
// ---------------------------------------------------------------------------

/// A second of silence opened in the bed opens a second in the voice, which
/// is 16 000 of its frames and not 48 000.
#[test]
fn insert_silence_sync_lock_opens_the_gap_in_the_other_tracks_frames() {
    let mut s = Session::new(48_000);
    s.ok("set_sync_lock", json!({ "enabled": true }));
    let r = u64::from(VOICE_RATE);

    s.ok(
        "insert_silence",
        json!({ "track": 0, "at": 1.0, "duration": 1.0 }),
    );

    assert_eq!(
        s.clips(1),
        vec![(0, 0, r), (2 * r, r, 3 * r)],
        "the voice opens a second at its second 1"
    );
}

/// And the other way round: a gap opened in the voice is a gap of seconds in
/// the bed.
#[test]
fn insert_silence_sync_lock_from_the_voice_opens_the_bed_by_seconds() {
    for rate in SESSION_RATES {
        let mut s = Session::new(rate);
        s.ok("set_sync_lock", json!({ "enabled": true }));
        let b = s.bed_rate();

        s.ok(
            "insert_silence",
            json!({ "track": 1, "at": 1.0, "duration": 1.0 }),
        );

        assert_eq!(
            s.clips(0),
            vec![(0, 0, b), (2 * b, b, (BED_SECONDS as u64 - 1) * b)],
            "session at {rate}"
        );
    }
}

/// A pasted second opens a second in the other tracks, each in its own frames.
#[test]
fn paste_region_sync_lock_opens_the_gap_in_the_other_tracks_frames() {
    let mut s = Session::new(48_000);
    s.ok(
        "copy_region",
        json!({ "track": 0, "range": { "start_sec": 1.0, "end_sec": 2.0 } }),
    );
    s.ok("set_sync_lock", json!({ "enabled": true }));
    let r = u64::from(VOICE_RATE);

    s.ok("paste_region", json!({ "track": 0, "at": 1.0 }));

    assert_eq!(
        s.clips(1),
        vec![(0, 0, r), (2 * r, r, 3 * r)],
        "the voice opens a second at its second 1"
    );
}

/// Pasting into the voice (a 16 kHz clipboard) opens the same seconds in the
/// bed.
#[test]
fn paste_region_from_the_voice_opens_the_bed_by_seconds() {
    let mut s = Session::new(44_100);
    s.ok(
        "copy_region",
        json!({ "track": 1, "range": { "start_sec": 0.0, "end_sec": 1.0 } }),
    );
    s.ok("set_sync_lock", json!({ "enabled": true }));
    let b = s.bed_rate();

    s.ok("paste_region", json!({ "track": 1, "at": 2.0 }));

    assert_eq!(
        s.clips(0),
        vec![(0, 0, 2 * b), (3 * b, 2 * b, (BED_SECONDS as u64 - 2) * b)]
    );
}

// ---------------------------------------------------------------------------
// select_region: audited, already right, pinned here
// ---------------------------------------------------------------------------

/// Six harmonics of 140 Hz, falling as 1/k: pitched, and what the detector
/// reads as a voice. Silent outside `[from, to)`.
fn voiced_between(from: f32, to: f32) -> impl Fn(f32) -> f32 {
    move |t| {
        if t < from || t >= to {
            return 0.0;
        }
        (1..=6)
            .map(|k| 0.1 / k as f32 * (2.0 * std::f32::consts::PI * k as f32 * 140.0 * t).sin())
            .sum()
    }
}

/// `select_region` answers in seconds, and finds speech in a track's audio at
/// that track's rate, so a 16 kHz voice under a 48 kHz or 44.1 kHz bed is
/// found where it sounds.
#[test]
fn select_region_finds_speech_on_a_voice_track_at_its_own_rate() {
    for rate in SESSION_RATES {
        let mut s = Session::with_signal(rate, voiced_between(1.0, 2.0));
        let v = s.ok(
            "select_region",
            json!({ "speech_passage": -1, "voice_tracks": [1] }),
        );
        let (start, end) = (
            v["start_sec"].as_f64().unwrap(),
            v["end_sec"].as_f64().unwrap(),
        );
        assert!(
            (start - 1.0).abs() < 0.15 && (end - 2.0).abs() < 0.15,
            "speech sits at 1 to 2 s, got {start} to {end} (session at {rate})"
        );
    }
}

// ---------------------------------------------------------------------------
// A track that has no single rate
// ---------------------------------------------------------------------------

/// A cut on a track with clips at different rates has no single frame for a
/// second, and is refused rather than made at a guess.
#[test]
fn a_track_of_clips_at_two_rates_is_refused_by_the_tools_that_need_one() {
    let mut s = Session::new(48_000);
    // Put the 48 kHz bed's clip on the voice track as well.
    let mut state = s.state();
    let extra = state.tracks[0].clips[0].clone();
    state.tracks[1].clips.push(session::Clip {
        start_in_track: 4 * u64::from(VOICE_RATE),
        ..extra
    });
    let node = session::SessionNode {
        id: session::NodeId([0u8; 32]),
        parent: None,
        created_at: chrono::Utc::now(),
        label: Some("mixed".into()),
        reasoning: None,
        state,
        op: None,
    };
    s.store.append(node).expect("append");

    for (tool, args) in [
        ("time_shift", json!({ "track": 1, "offset_sec": 1.0 })),
        (
            "split_by_speaker",
            json!({ "track": 1, "segments": [{ "start_sec": 0.0, "end_sec": 1.0, "speaker": "A" }] }),
        ),
        (
            "cut_range",
            json!({ "track": 1, "start_sample": 0, "end_sample": 100 }),
        ),
        (
            "trim",
            json!({ "track": 1, "start_sample": 0, "end_sample": 100 }),
        ),
    ] {
        let before = s.state();
        let msg = s.err(tool, args);
        assert!(msg.contains("different sample rates"), "{tool}: {msg}");
        assert_eq!(s.state(), before, "{tool} changed nothing");
    }
}
