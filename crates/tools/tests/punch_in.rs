//! Punch-in: fixing one line without redoing the take (#203 §2).
//!
//! The property that makes it punch-in rather than an edit is that
//! *nothing else moves*. The region's length is unchanged, so the music
//! underneath still lines up, the labels still point at the right words,
//! and the other tracks in a multitrack session stay in sync.
//!
//! So these tests mostly check what did **not** happen.

use std::path::{Path, PathBuf};

use hound::{SampleFormat, WavSpec, WavWriter};
use serde_json::json;

#[path = "support/harness.rs"]
mod harness;
use harness::{err, ok, write_frames, Session};

const SR: u32 = 48_000;

/// A constant-amplitude tone, so "which take is this?" is answerable by
/// reading one sample.
fn write_tone(path: &Path, seconds: f64, amp: f32) -> PathBuf {
    let frames = (SR as f64 * seconds) as usize;
    write_frames(path, SR, 1, frames, |_| amp * 32_000.0 / 32_767.0)
}

impl Session {
    fn new() -> Self {
        let mut s = Session::empty();
        let original = write_tone(&s.dir.path().join("take.wav"), 10.0, 0.5);
        s.load(&original);
        s
    }

    /// The rendered track, so assertions are about audio rather than
    /// about clip bookkeeping.
    fn samples(&self) -> Vec<f32> {
        let head = self.store.head().expect("head");
        let state = self.store.get(head).expect("node").state;
        let clip = &state.tracks[0].clips[0];
        audio_decoder::decode_file(&clip.source_path)
            .expect("decode")
            .samples
    }

    fn amp_at(&self, sec: f64) -> f32 {
        let s = self.samples();
        s.get((sec * SR as f64) as usize)
            .copied()
            .unwrap_or(0.0)
            .abs()
    }

    fn len_sec(&self) -> f64 {
        self.samples().len() as f64 / SR as f64
    }
}

/// The whole feature in one test: the region is the retake, and every
/// other part of the track is untouched.
#[test]
fn the_region_becomes_the_retake_and_nothing_else_changes() {
    let mut s = Session::new();
    let retake = write_tone(&s.dir.path().join("retake.wav"), 2.0, 0.2);

    let before_len = s.len_sec();
    ok(s.call(
        "punch_in",
        json!({
            "track": 0,
            "start_sec": 4.0,
            "end_sec": 6.0,
            "take_path": retake.to_string_lossy(),
        }),
    ));

    assert!(
        (s.amp_at(5.0) - 0.2).abs() < 0.02,
        "the region is the retake"
    );
    assert!(
        (s.amp_at(1.0) - 0.5).abs() < 0.02,
        "before it, the original"
    );
    assert!((s.amp_at(8.0) - 0.5).abs() < 0.02, "after it, the original");
    assert!(
        (s.len_sec() - before_len).abs() < 0.001,
        "and the track is the same length — that is what makes it a punch"
    );
}

/// A take longer than the region is trimmed rather than allowed to
/// ripple, and the tool says so.
#[test]
fn a_long_take_is_trimmed_not_rippled() {
    let mut s = Session::new();
    let retake = write_tone(&s.dir.path().join("long.wav"), 5.0, 0.2);

    let before_len = s.len_sec();
    let v = ok(s.call(
        "punch_in",
        json!({
            "track": 0,
            "start_sec": 4.0,
            "end_sec": 6.0,
            "take_path": retake.to_string_lossy(),
        }),
    ));

    assert!(
        v["trimmed_sec"].as_f64().unwrap_or(0.0) > 2.9,
        "three seconds too long: {v}"
    );
    assert!(
        (s.len_sec() - before_len).abs() < 0.001,
        "still the same length"
    );
    assert!(
        (s.amp_at(7.0) - 0.5).abs() < 0.02,
        "past the region, untouched"
    );
}

/// A short take leaves silence rather than the old performance. Half a
/// retake over half the original is the one outcome nobody wants.
#[test]
fn a_short_take_leaves_silence_not_the_old_line() {
    let mut s = Session::new();
    let retake = write_tone(&s.dir.path().join("short.wav"), 1.0, 0.2);

    let v = ok(s.call(
        "punch_in",
        json!({
            "track": 0,
            "start_sec": 4.0,
            "end_sec": 6.0,
            "take_path": retake.to_string_lossy(),
        }),
    ));

    assert!(v["padded_sec"].as_f64().unwrap_or(0.0) > 0.9, "{v}");
    assert!((s.amp_at(4.5) - 0.2).abs() < 0.02, "the retake");
    assert!(
        s.amp_at(5.5) < 0.01,
        "then silence, not the line it replaced"
    );
    assert!(
        (s.amp_at(7.0) - 0.5).abs() < 0.02,
        "and the original resumes after"
    );
}

/// A take at the wrong rate would play at the wrong speed and pitch.
/// Refusing is better than a result the user has to notice by ear.
#[test]
fn a_take_at_the_wrong_sample_rate_is_refused() {
    let mut s = Session::new();
    let path = s.dir.path().join("wrong-rate.wav");
    let spec = WavSpec {
        channels: 1,
        sample_rate: 22_050,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut w = WavWriter::create(&path, spec).unwrap();
    for _ in 0..22_050 {
        w.write_sample(1000i16).unwrap();
    }
    w.finalize().unwrap();

    let msg = err(s.call(
        "punch_in",
        json!({
            "track": 0,
            "start_sec": 1.0,
            "end_sec": 2.0,
            "take_path": path.to_string_lossy(),
        }),
    ));
    assert!(msg.contains("22050") && msg.contains("48000"), "{msg}");
    assert!(msg.contains("resample"), "and says what to do: {msg}");
}

/// The bug a channel mismatch causes is not subtle: the stride belongs
/// to the buffer being written into, so a mono take spliced into a
/// stereo track at the wrong stride lands at half the intended position
/// and swaps the channels for the rest of the region. Refusing names
/// something the user can fix.
#[test]
fn a_take_with_the_wrong_channel_count_is_refused() {
    let mut s = Session::new();
    let path = s.dir.path().join("stereo.wav");
    let spec = WavSpec {
        channels: 2,
        sample_rate: SR,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut w = WavWriter::create(&path, spec).unwrap();
    for _ in 0..(SR as usize) {
        w.write_sample(1000i16).unwrap();
        w.write_sample(1000i16).unwrap();
    }
    w.finalize().unwrap();

    let msg = err(s.call(
        "punch_in",
        json!({
            "track": 0,
            "start_sec": 1.0,
            "end_sec": 2.0,
            "take_path": path.to_string_lossy(),
        }),
    ));
    assert!(
        msg.contains("2-channel") && msg.contains("1-channel"),
        "it should name both layouts: {msg}"
    );
}

/// A missing file is a typo, not a crash.
#[test]
fn a_missing_take_says_so() {
    let mut s = Session::new();
    let msg = err(s.call(
        "punch_in",
        json!({
            "track": 0,
            "start_sec": 1.0,
            "end_sec": 2.0,
            "take_path": "/nowhere/nope.wav",
        }),
    ));
    assert!(msg.contains("could not read the take"), "{msg}");
}

/// A reversed region is an easy slip and must not reach slice
/// arithmetic.
#[test]
fn a_reversed_region_is_refused() {
    let mut s = Session::new();
    let retake = write_tone(&s.dir.path().join("r.wav"), 1.0, 0.2);
    let msg = err(s.call(
        "punch_in",
        json!({
            "track": 0,
            "start_sec": 6.0,
            "end_sec": 4.0,
            "take_path": retake.to_string_lossy(),
        }),
    ));
    assert!(!msg.is_empty(), "{msg}");
}

/// It is an ordinary node, so it undoes like anything else.
#[test]
fn the_punch_is_one_undoable_node() {
    let mut s = Session::new();
    let retake = write_tone(&s.dir.path().join("r.wav"), 2.0, 0.2);
    let before = s.store.head().expect("head");

    ok(s.call(
        "punch_in",
        json!({
            "track": 0,
            "start_sec": 4.0,
            "end_sec": 6.0,
            "take_path": retake.to_string_lossy(),
        }),
    ));
    assert!((s.amp_at(5.0) - 0.2).abs() < 0.02);

    s.store.set_head(before).expect("undo");
    assert!(
        (s.amp_at(5.0) - 0.5).abs() < 0.02,
        "the original line is back"
    );
}
