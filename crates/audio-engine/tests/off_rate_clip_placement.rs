//! A clip whose source rate differs from the project's lands where it
//! was asked to (#234).
//!
//! `Clip::start_in_track` is counted in the **source's** frames — the
//! renderer converts it with `to_project_frames(f, source_rate,
//! project_rate)`, and `flatten_track` indexes the decoded source with
//! it. Several call sites converted seconds using
//! `SessionState::sample_rate` instead, which is a different number
//! whenever a session holds a source at another rate. Mixed-rate
//! sessions are supported by design: the project rate is whatever the
//! first load established, and off-rate sources are resampled at render
//! time.
//!
//! The audit's probe: project 8 000 Hz, a 16 000 Hz clip written at
//! `start_in_track = 8_000` — what a session-rate conversion produces
//! for "1.0 s" — rendered its first audible frame at 0.532 s.
//!
//! This test states the invariant from the render's side, so it holds
//! whatever the writers do: place a clip at a known second in its own
//! domain, and the audio has to start there.

use std::path::{Path, PathBuf};

use audio_engine::render_state_to_wav;
use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
use session::{BusGraph, Clip, SessionState, TempoMap, Track, TrackId};
use tempfile::TempDir;

/// A full-scale-ish tone at `rate`, one second long, so "where does the
/// audio start" is unambiguous.
fn write_tone(dir: &Path, name: &str, rate: u32) -> PathBuf {
    let path = dir.join(name);
    let spec = WavSpec {
        channels: 1,
        sample_rate: rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut w = WavWriter::create(&path, spec).expect("wav writer");
    for n in 0..rate {
        let t = n as f32 / rate as f32;
        let s = (2.0 * std::f32::consts::PI * 440.0 * t).sin() * 0.5;
        w.write_sample((s * 32_767.0) as i16).unwrap();
    }
    w.finalize().unwrap();
    path
}

fn state_with_clip(
    project_rate: u32,
    source: PathBuf,
    start_in_track: u64,
    length: u64,
) -> SessionState {
    SessionState {
        tracks: vec![Track {
            id: TrackId::new(),
            name: "bed".into(),
            clips: vec![Clip {
                source_path: source,
                start_in_track,
                source_offset: 0,
                length,
                content_hash: None,
                time_stretch_factor: None,
                pitch_shift_semitones: None,
                beat_grid: None,
                volume_envelope: Vec::new(),
            }],
            gain_db: 0.0,
            pan: 0.0,
            muted: false,
            soloed: false,
            effects: Vec::new(),
            sends: Vec::new(),
        }],
        bus_routing: BusGraph { buses: Vec::new() },
        master_chain: Vec::new(),
        tempo_map: TempoMap::default(),
        key_map: None,
        transcript: None,
        sample_rate: project_rate,
        // The furthest point the clip reaches, in its own frames.
        length_samples: start_in_track + length,
        annotations: Vec::new(),
        sync_lock: false,
    }
}

/// First frame whose magnitude clears the noise floor, in seconds.
fn first_audible_sec(path: &Path) -> f64 {
    let mut reader = WavReader::open(path).expect("open render");
    let rate = reader.spec().sample_rate as f64;
    let channels = reader.spec().channels as usize;
    for (i, s) in reader.samples::<i16>().enumerate() {
        if s.expect("sample").unsigned_abs() > 800 {
            return (i / channels) as f64 / rate;
        }
    }
    f64::INFINITY
}

#[test]
fn an_off_rate_clip_starts_at_the_second_its_own_frames_name() {
    let tmp = TempDir::new().expect("tempdir");
    let project_rate = 8_000;
    let source_rate = 16_000;
    let src = write_tone(tmp.path(), "bed.wav", source_rate);

    // One second in, expressed in the clip's own domain — which is what
    // `Clip`'s documentation says the field means.
    let state = state_with_clip(
        project_rate,
        src,
        u64::from(source_rate),
        u64::from(source_rate),
    );

    let out = tmp.path().join("out.wav");
    render_state_to_wav(&state, &out, None).expect("render");

    let at = first_audible_sec(&out);
    // Measured 1.032 s, not 1.000. The 32 ms is the rubato resampler's
    // own latency, which nothing compensates — that is #242, a separate
    // open defect, and it applies to every off-rate track equally. The
    // tolerance admits it deliberately rather than hiding it: the
    // failure this test exists for is 0.532 s, half a second out, and
    // 60 ms is nowhere near it. Tighten this to a few milliseconds once
    // #242 lands.
    assert!(
        (at - 1.0).abs() < 0.06,
        "a clip placed at 1.0 s in its own frames rendered at {at:.3} s"
    );
}

/// The same placement at the project's own rate, so the test above
/// cannot pass by the two rates being confused in a compensating way.
#[test]
fn a_matched_rate_clip_still_starts_where_it_is_placed() {
    let tmp = TempDir::new().expect("tempdir");
    let rate = 8_000;
    let src = write_tone(tmp.path(), "bed.wav", rate);
    let state = state_with_clip(rate, src, u64::from(rate), u64::from(rate));

    let out = tmp.path().join("out.wav");
    render_state_to_wav(&state, &out, None).expect("render");

    let at = first_audible_sec(&out);
    assert!(
        (at - 1.0).abs() < 0.02,
        "a same-rate clip placed at 1.0 s rendered at {at:.3} s"
    );
}

/// What a session-rate conversion would have written for "1.0 s", shown
/// landing somewhere else.
///
/// This is the bug as a value rather than a story: `move_clip` used to
/// compute `start_sec * SessionState::sample_rate`, which for this
/// session is 8 000 — and 8 000 source frames of a 16 kHz file is half
/// a second, not one.
#[test]
fn the_session_rate_conversion_lands_in_the_wrong_place() {
    let tmp = TempDir::new().expect("tempdir");
    let project_rate = 8_000;
    let source_rate = 16_000;
    let src = write_tone(tmp.path(), "bed.wav", source_rate);

    let wrong = f64::from(project_rate) * 1.0; // what the old code wrote
    let state = state_with_clip(project_rate, src, wrong as u64, u64::from(source_rate));

    let out = tmp.path().join("out.wav");
    render_state_to_wav(&state, &out, None).expect("render");

    let at = first_audible_sec(&out);
    assert!(
        at < 0.75,
        "the session-rate conversion was expected to land early, but it \
         rendered at {at:.3} s — if this now lands at 1.0 s the frame \
         domain has changed and this file's premise needs rechecking"
    );
}
