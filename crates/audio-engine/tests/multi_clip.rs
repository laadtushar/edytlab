//! Multi-clip track render tests.
//!
//! A track is a `Vec<Clip>`, and two tools already produce more than one
//! of them: `cut_range` with an interior range, and `split_clip`. The
//! render graph only ever read `clips.first()`, so everything after the
//! first clip was silently dropped from the mixdown — an interior cut
//! kept the head of the track and threw away the tail, with no error
//! anywhere to say so.
//!
//! These tests assemble the post-cut state directly rather than going
//! through the tools, so they pin the engine's contract independently of
//! how the clips came to exist.

use std::path::{Path, PathBuf};

use audio_engine::render_state_to_wav;
use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
use session::{BusGraph, Clip, EffectInstance, SessionState, TempoMap, Track, TrackId};
use tempfile::TempDir;

const RATE: u32 = 8_000;

/// A ramp is used rather than a sine because every frame is then
/// individually identifiable: sample `n` has value `n / frames`, so an
/// assertion can say exactly *which* part of the source came out.
fn write_ramp_wav(dir: &Path, name: &str, frames: usize) -> PathBuf {
    let path = dir.join(name);
    let spec = WavSpec {
        channels: 1,
        sample_rate: RATE,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut writer = WavWriter::create(&path, spec).expect("wav writer");
    for n in 0..frames {
        let s = n as f32 / frames as f32;
        let q = (s * 32_768.0)
            .round()
            .clamp(i16::MIN as f32, i16::MAX as f32) as i16;
        writer.write_sample(q).unwrap();
    }
    writer.finalize().unwrap();
    path
}

fn clip(source: &Path, start_in_track: u64, source_offset: u64, length: u64) -> Clip {
    Clip {
        source_path: source.to_path_buf(),
        start_in_track,
        source_offset,
        length,
        content_hash: None,
        time_stretch_factor: None,
        pitch_shift_semitones: None,
        beat_grid: None,
        volume_envelope: Vec::new(),
    }
}

fn session_with(clips: Vec<Clip>) -> SessionState {
    let length = clips
        .iter()
        .map(|c| c.start_in_track + c.length)
        .max()
        .unwrap_or(0);
    SessionState {
        tracks: vec![Track {
            id: TrackId::new(),
            name: "t".into(),
            clips,
            gain_db: 0.0,
            pan: 0.0,
            muted: false,
            soloed: false,
            effects: Vec::<EffectInstance>::new(),
        }],
        bus_routing: BusGraph::default(),
        master_chain: Vec::new(),
        tempo_map: TempoMap::default(),
        key_map: None,
        transcript: None,
        sample_rate: RATE,
        length_samples: length,
        annotations: Vec::new(),
    }
}

fn read_mono(path: &Path) -> Vec<f32> {
    let mut reader = WavReader::open(path).expect("open out");
    let chans = reader.spec().channels as usize;
    let samples: Vec<i16> = reader.samples::<i16>().map(|s| s.unwrap()).collect();
    samples
        .chunks(chans)
        .map(|f| f[0] as f32 / 32_768.0)
        .collect()
}

/// The shape `cut_range` produces for an interior cut: two clips from one
/// source, laid end to end, with the middle of the source missing.
#[test]
fn interior_cut_keeps_the_tail_of_the_track() {
    let tmp = TempDir::new().unwrap();
    let src = write_ramp_wav(tmp.path(), "ramp.wav", 800);
    let out = tmp.path().join("out.wav");

    // Cut source frames [200, 600). What survives is [0,200) followed by
    // [600,800) — 400 frames, with the second half starting at 200.
    let state = session_with(vec![clip(&src, 0, 0, 200), clip(&src, 200, 600, 200)]);

    let report = render_state_to_wav(&state, &out, None).expect("render");
    assert_eq!(
        report.frames_written, 400,
        "both clips must be rendered, not just the first"
    );

    let got = read_mono(&out);
    assert_eq!(got.len(), 400);

    // First half is the head of the ramp.
    assert!(
        (got[0] - 0.0).abs() < 0.01,
        "clip 0 should start at the ramp's origin, got {}",
        got[0]
    );
    assert!(
        (got[199] - 199.0 / 800.0).abs() < 0.01,
        "clip 0 should end just before the cut, got {}",
        got[199]
    );
    // Second half jumps to where the cut resumed — this is the assertion
    // that fails outright on the old engine, which wrote silence here.
    assert!(
        (got[200] - 600.0 / 800.0).abs() < 0.01,
        "clip 1 should resume after the cut, got {}",
        got[200]
    );
    assert!(
        (got[399] - 799.0 / 800.0).abs() < 0.01,
        "clip 1 should run to the end of the source, got {}",
        got[399]
    );
}

/// Clips need not be contiguous. A gap between them is silence, not a
/// splice — the second clip stays where its `start_in_track` puts it.
#[test]
fn gap_between_clips_renders_as_silence() {
    let tmp = TempDir::new().unwrap();
    let src = write_ramp_wav(tmp.path(), "ramp.wav", 800);
    let out = tmp.path().join("out.wav");

    let state = session_with(vec![
        clip(&src, 0, 0, 100),
        clip(&src, 300, 0, 100), // 200 frames of silence in between
    ]);

    let report = render_state_to_wav(&state, &out, None).expect("render");
    assert_eq!(report.frames_written, 400);

    let got = read_mono(&out);
    let gap_peak = got[100..300].iter().fold(0.0f32, |m, s| m.max(s.abs()));
    assert!(gap_peak < 0.001, "gap must be silent, peak was {gap_peak}");
    assert!(
        (got[300] - 0.0).abs() < 0.01,
        "the second clip restarts at the ramp's origin, got {}",
        got[300]
    );
}

/// Overlapping clips sum, the same way two tracks do. Nothing in the
/// model forbids the overlap, so the engine must not silently drop one.
#[test]
fn overlapping_clips_sum() {
    let tmp = TempDir::new().unwrap();
    let src = write_ramp_wav(tmp.path(), "ramp.wav", 800);
    let out = tmp.path().join("out.wav");

    // Both clips play frames [0,100) of the ramp, fully overlapped, so
    // every output frame is exactly twice the source. The head of the ramp
    // is deliberately quiet — doubling anything past halfway would clip at
    // full scale and the assertion would be measuring the limiter.
    let state = session_with(vec![clip(&src, 0, 0, 100), clip(&src, 0, 0, 100)]);

    render_state_to_wav(&state, &out, None).expect("render");
    let got = read_mono(&out);
    assert_eq!(got.len(), 100);
    let expected = 2.0 * 50.0 / 800.0;
    assert!(
        (got[50] - expected).abs() < 0.01,
        "overlapping clips should sum: expected {expected}, got {}",
        got[50]
    );
}

/// A single clip that starts partway into the track is delayed, not
/// slid back to zero.
#[test]
fn clip_offset_from_track_start_is_honoured() {
    let tmp = TempDir::new().unwrap();
    let src = write_ramp_wav(tmp.path(), "ramp.wav", 800);
    let out = tmp.path().join("out.wav");

    let state = session_with(vec![clip(&src, 250, 0, 100)]);

    let report = render_state_to_wav(&state, &out, None).expect("render");
    assert_eq!(report.frames_written, 350);

    let got = read_mono(&out);
    let lead_peak = got[..250].iter().fold(0.0f32, |m, s| m.max(s.abs()));
    assert!(
        lead_peak < 0.001,
        "leading silence expected, peak was {lead_peak}"
    );
    assert!(
        (got[251] - 1.0 / 800.0).abs() < 0.01,
        "audio should start at frame 250, got {}",
        got[251]
    );
}
