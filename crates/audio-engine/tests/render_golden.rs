//! Integration tests for the offline render path.
//!
//! Fixtures (see `crates/audio-engine/build.rs`):
//! * `OUT_DIR/sine_440hz_1s.wav`            — 1 s, 44.1 kHz, mono, 16-bit
//! * `OUT_DIR/unity_passthrough_golden.wav` — expected unity-render output
//! * `OUT_DIR/sine_440hz_10min.wav`         — perf gate fixture (large)

use std::path::{Path, PathBuf};
use std::time::Instant;

use audio_engine::{render_state_to_wav, RenderReport, TimeRange};
use session::{BusGraph, Clip, EffectInstance, SessionState, TempoMap, Track, TrackId};
use tempfile::TempDir;

fn out_dir() -> PathBuf {
    PathBuf::from(env!("OUT_DIR"))
}

fn source_wav() -> PathBuf {
    out_dir().join("sine_440hz_1s.wav")
}

fn golden_wav() -> PathBuf {
    out_dir().join("unity_passthrough_golden.wav")
}

fn ten_min_wav() -> PathBuf {
    out_dir().join("sine_440hz_10min.wav")
}

fn build_state(source: &Path, gain_db: f32) -> SessionState {
    let decoded = audio_decoder::decode_file(source).expect("decode source");
    let frames = (decoded.samples.len() / decoded.channels as usize) as u64;
    SessionState {
        tracks: vec![Track {
            id: TrackId::new(),
            name: "t1".into(),
            clips: vec![Clip {
                source_path: source.to_path_buf(),
                start_in_track: 0,
                source_offset: 0,
                length: frames,
                content_hash: None,
                time_stretch_factor: None,
                pitch_shift_semitones: None,
                beat_grid: None,
            }],
            gain_db,
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
        sample_rate: decoded.sample_rate,
        length_samples: frames,
        annotations: Vec::new(),
    }
}

fn read_int16_samples(path: &Path) -> Vec<i16> {
    let mut r = hound::WavReader::open(path).expect("open wav");
    r.samples::<i16>()
        .map(|s| s.expect("read sample"))
        .collect()
}

#[test]
fn unity_passthrough_byte_identical_to_golden() {
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("rendered.wav");
    let state = build_state(&source_wav(), 0.0);

    let report: RenderReport = render_state_to_wav(&state, &out, None).expect("render");
    assert_eq!(report.sample_rate, 44_100);
    assert_eq!(report.channels, 1);
    assert_eq!(report.frames_written, 44_100);

    let actual = std::fs::read(&out).expect("read rendered");
    let expected = std::fs::read(golden_wav()).expect("read golden");
    assert_eq!(
        actual, expected,
        "unity render must be byte-identical to source/golden"
    );

    // Defense-in-depth: the byte-equality assertion above is tautological as
    // long as the unity render takes the byte-copy fast path (since the
    // golden was created by the same byte-copy at fixture-build time). If a
    // future refactor replaces the fast path with a decode-then-encode path,
    // byte equality may legitimately drift (header/chunk-padding) but sample
    // equivalence MUST hold. Compare i16 samples directly so a regression in
    // the f32 path at unity gain is also caught here.
    let rendered_samples = read_int16_samples(&out);
    let source_samples = read_int16_samples(&source_wav());
    assert_eq!(
        rendered_samples, source_samples,
        "unity render samples must equal source samples"
    );
}

#[test]
fn gain_plus_six_db_doubles_samples() {
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("rendered.wav");
    let state = build_state(&source_wav(), 20.0 * 2f32.log10());

    render_state_to_wav(&state, &out, None).expect("render");

    let src = read_int16_samples(&source_wav());
    let dst = read_int16_samples(&out);
    assert_eq!(src.len(), dst.len());

    // The +6.02 dB factor is ~2.0 in linear space. Round-tripping through
    // i16 quantization, output sample == clamp(round(src * 2.0)) — which
    // saturates near ±1.0 dBFS. Our sine is at -3 dBFS so doubling lands
    // around ±0.99, no saturation expected.
    for (i, (s, d)) in src.iter().zip(dst.iter()).enumerate() {
        let expected = ((*s as f32) * 2.0)
            .round()
            .clamp(i16::MIN as f32, i16::MAX as f32) as i16;
        // Allow ±1 LSB for float-pow imprecision in the gain factor itself.
        let diff = (expected as i32 - *d as i32).abs();
        assert!(
            diff <= 1,
            "sample {i}: src={s} expected≈{expected} got={d} (diff={diff})"
        );
    }
}

#[test]
fn render_is_deterministic_across_runs() {
    // 100 in-process renders; assert byte-identity. Faster path uses the
    // unity passthrough byte-copy, so include a non-unity case with gain
    // to exercise the f32 path.
    let tmp = TempDir::new().unwrap();
    let state = build_state(&source_wav(), 3.0);

    let first = tmp.path().join("first.wav");
    render_state_to_wav(&state, &first, None).expect("render");
    let baseline = std::fs::read(&first).expect("read first");

    for i in 0..100 {
        let p = tmp.path().join(format!("run_{i}.wav"));
        render_state_to_wav(&state, &p, None).expect("render");
        let bytes = std::fs::read(&p).expect("read");
        assert_eq!(bytes, baseline, "run {i} differs from baseline");
    }
}

#[test]
fn render_report_peak_within_expected_range() {
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("r.wav");
    let state = build_state(&source_wav(), 0.0);
    let report = render_state_to_wav(&state, &out, None).expect("render");
    // Source is a -3 dBFS sine; tolerance for quantization + log rounding.
    assert!(
        (report.peak_dbfs + 3.0).abs() < 0.1,
        "expected peak ≈ -3 dBFS, got {}",
        report.peak_dbfs
    );
}

#[test]
fn inverted_range_returns_invalid_range_even_when_clamped() {
    // Regression: pre-fix, both endpoints clamped to total_frames before the
    // ordering check, so a swapped range with both endpoints past the source
    // length silently produced a zero-frame render. Must now be rejected.
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("r.wav");
    let state = build_state(&source_wav(), 3.0); // non-unity to force render_processed
    let bogus = TimeRange {
        start_frame: 200_000,
        end_frame: 150_000,
    };
    let err = render_state_to_wav(&state, &out, Some(bogus));
    assert!(
        err.is_err(),
        "inverted range past source length must error, got {err:?}"
    );
}

#[test]
#[ignore = "perf gate; run with `cargo test -p audio-engine -- --ignored`"]
fn ten_minute_render_under_thirty_seconds() {
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("perf.wav");
    let state = build_state(&ten_min_wav(), 3.0);

    let start = Instant::now();
    render_state_to_wav(&state, &out, None).expect("render");
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_secs_f32() < 30.0,
        "10-min render took {elapsed:?}, budget is 30 s"
    );
}
