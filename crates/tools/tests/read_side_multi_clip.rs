//! The read side of the single-clip assumption.
//!
//! Four tools answered questions about the track by looking at
//! `clips[0]`'s *source file*. That is a different question. After a cut
//! the file still contains the audio the cut removed, and on a split
//! track the file covers only the part the first clip points at — so the
//! answers came back about the file on disk rather than about the
//! timeline the user asked about.
//!
//! `normalize` is the one that does damage rather than merely misreport:
//! the gain it computes lands on the whole track, so a peak measured from
//! a quiet head over-boosts a louder tail straight into clipping.

use std::path::{Path, PathBuf};

use hound::{SampleFormat, WavSpec, WavWriter};
use serde_json::{json, Value};
use tempfile::TempDir;
use tools::{ToolContext, ToolDispatcher, ToolResult};

const SAMPLE_RATE: u32 = 8_000;

fn ok(result: ToolResult) -> Value {
    match result {
        ToolResult::Ok(v) => v,
        ToolResult::Error(msg) => panic!("expected Ok, got Error({msg})"),
    }
}

/// A source in three equal thirds: quiet, silent, loud.
///
/// Cutting the silent middle out leaves a split track whose two clips
/// have very different peaks — which is what separates "measured the
/// track" from "measured the first clip".
fn write_quiet_then_loud(dir: &Path, frames_per_third: usize) -> PathBuf {
    let path = dir.join("in.wav");
    let spec = WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut writer = WavWriter::create(&path, spec).expect("wav writer");
    for n in 0..frames_per_third * 3 {
        let amp = match n / frames_per_third {
            0 => 0.1, // quiet head
            1 => 0.0, // silence
            _ => 0.8, // loud tail
        };
        let t = n as f32 / SAMPLE_RATE as f32;
        let s = (2.0 * std::f32::consts::PI * 500.0 * t).sin() * amp;
        let q = (s * 32_768.0)
            .round()
            .clamp(i16::MIN as f32, i16::MAX as f32) as i16;
        writer.write_sample(q).unwrap();
    }
    writer.finalize().unwrap();
    path
}

struct Session {
    _tmp: TempDir,
    store: session::Store,
    engine: audio_engine::Engine,
    dispatcher: ToolDispatcher,
}

/// Load the three-part source and cut the silent middle out, leaving two
/// clips: a quiet 0.1 head and a loud 0.8 tail.
fn split_session(frames_per_third: usize) -> Session {
    let tmp = TempDir::new().expect("tempdir");
    let mut store = session::Store::open(tmp.path()).expect("open store");
    let mut engine = audio_engine::Engine::new();
    let dispatcher = ToolDispatcher::default_dispatcher();
    let src = write_quiet_then_loud(tmp.path(), frames_per_third);

    let mut clipboard: Option<Vec<f32>> = None;
    {
        let mut ctx = ToolContext {
            store: &mut store,
            engine: &mut engine,
            user_message: "",
            clipboard: &mut clipboard,
            allowed_tools: None,
        };
        ok(dispatcher
            .invoke("load", json!({ "path": src.to_string_lossy() }), &mut ctx)
            .unwrap());
        ok(dispatcher
            .invoke(
                "cut_range",
                json!({
                    "track": 0,
                    "start_sample": frames_per_third,
                    "end_sample": frames_per_third * 2,
                }),
                &mut ctx,
            )
            .unwrap());
    }

    Session {
        store,
        engine,
        dispatcher,
        _tmp: tmp,
    }
}

/// The peak has to come from the whole track, not its first clip.
///
/// The tail is 0.8 and the head 0.1 — 18 dB apart. Measuring the head
/// alone asks for 18 dB more gain than the track can take, and every
/// sample of that tail clips.
#[test]
fn normalize_measures_the_whole_track() {
    let mut s = split_session(2_000);
    let mut clipboard: Option<Vec<f32>> = None;
    let mut ctx = ToolContext {
        store: &mut s.store,
        engine: &mut s.engine,
        user_message: "",
        clipboard: &mut clipboard,
        allowed_tools: None,
    };

    let res = ok(s
        .dispatcher
        .invoke(
            "normalize",
            json!({ "track": 0, "target_dbfs": -1.0 }),
            &mut ctx,
        )
        .unwrap());

    let peak_dbfs = res["source_peak_dbfs"].as_f64().unwrap() as f32;
    let applied = res["applied_gain_db"].as_f64().unwrap() as f32;

    // 0.8 is about -1.94 dBFS; 0.1 is about -20 dBFS.
    assert!(
        (peak_dbfs - -1.94).abs() < 0.5,
        "the loud tail is the track's peak; got {peak_dbfs} dBFS, which looks \
         like the quiet head was measured instead"
    );
    // So barely any gain is needed. Measuring the head would have asked
    // for around +19 dB and clipped the tail flat.
    assert!(
        applied < 2.0,
        "expected roughly a decibel of gain, got {applied} dB — enough to clip \
         the tail into a square wave"
    );
}

/// Silence positions are positions on the track.
///
/// The cut removed the silent middle, so there is no silence left. The
/// old code scanned the *source file*, which still contains it, and
/// reported a gap that no longer exists.
#[test]
fn silence_finder_reports_positions_on_the_track() {
    let mut s = split_session(4_000);
    let mut clipboard: Option<Vec<f32>> = None;
    let mut ctx = ToolContext {
        store: &mut s.store,
        engine: &mut s.engine,
        user_message: "",
        clipboard: &mut clipboard,
        allowed_tools: None,
    };

    let res = ok(s
        .dispatcher
        .invoke(
            "silence_finder",
            json!({ "track": 0, "threshold_db": -40.0, "min_silence_ms": 100.0 }),
            &mut ctx,
        )
        .unwrap());

    let count = res["count"].as_u64().unwrap();
    assert_eq!(
        count, 0,
        "the cut removed the only silent stretch; {count} region(s) reported \
         means the source file was scanned rather than the track"
    );
}

/// A spectrum request in seconds is a request about the track.
///
/// Frames 4000..8000 of the *track* are the loud tail. In the source file
/// that same span is the silence the cut removed, so the old code
/// returned a spectrum of near-nothing.
#[test]
fn plot_spectrum_windows_the_track_not_the_source_file() {
    let mut s = split_session(4_000);
    let mut clipboard: Option<Vec<f32>> = None;
    let mut ctx = ToolContext {
        store: &mut s.store,
        engine: &mut s.engine,
        user_message: "",
        clipboard: &mut clipboard,
        allowed_tools: None,
    };

    let res = ok(s
        .dispatcher
        .invoke(
            "plot_spectrum",
            json!({ "track": 0, "start_sec": 0.5, "end_sec": 1.0 }),
            &mut ctx,
        )
        .unwrap());

    let points = res["points"].as_array().unwrap();
    let peak_db = points
        .iter()
        .map(|p| p["db"].as_f64().unwrap())
        .fold(f64::NEG_INFINITY, f64::max);

    assert!(
        peak_db > -40.0,
        "0.5s-1.0s of the track is the loud tail; a peak of {peak_db} dB is \
         the silence that only exists in the source file"
    );
}

/// Copying a span that lives in the second clip must copy that audio.
#[test]
fn copy_region_reads_the_track() {
    let mut s = split_session(4_000);
    let mut clipboard: Option<Vec<f32>> = None;
    let mut ctx = ToolContext {
        store: &mut s.store,
        engine: &mut s.engine,
        user_message: "",
        clipboard: &mut clipboard,
        allowed_tools: None,
    };

    ok(s.dispatcher
        .invoke(
            "copy_region",
            json!({ "track": 0, "range": { "start_sec": 0.6, "end_sec": 0.9 } }),
            &mut ctx,
        )
        .unwrap());

    let copied = clipboard.expect("clipboard should hold the copied region");
    let peak = copied.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    assert!(
        peak > 0.5,
        "that span is the loud (0.8) tail; a peak of {peak} is the silence \
         sitting at the same offset in the source file"
    );
}
