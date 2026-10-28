//! Cancellation stops range rendering at its existing streaming boundaries.

use std::cell::Cell;
use std::path::Path;

use audio_engine::{
    render_state_range_to_wav_cancellable, render_state_to_wav, Engine, Error, RenderOutcome,
    TimeRange,
};
use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
use session::{BusGraph, Clip, EffectInstance, SessionState, TempoMap, Track, TrackId};
use tempfile::TempDir;

const SOURCE_RATE: u32 = 44_100;
const PROJECT_RATE: u32 = 48_000;

fn fixture(dir: &Path) -> SessionState {
    let source = dir.join("source.wav");
    let mut writer = WavWriter::create(
        &source,
        WavSpec {
            channels: 1,
            sample_rate: SOURCE_RATE,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        },
    )
    .unwrap();
    for frame in 0..SOURCE_RATE * 6 {
        let t = frame as f32 / SOURCE_RATE as f32;
        let sample = (2.0 * std::f32::consts::PI * 330.0 * t).sin() * 0.4
            + (2.0 * std::f32::consts::PI * 6_000.0 * t).sin() * 0.2;
        writer.write_sample((sample * 32_767.0) as i16).unwrap();
    }
    writer.finalize().unwrap();
    SessionState {
        tracks: vec![Track {
            id: TrackId::new(),
            name: "Voice".into(),
            clips: vec![Clip {
                source_path: source,
                start_in_track: 0,
                source_offset: 0,
                length: (SOURCE_RATE * 6) as u64,
                content_hash: None,
                time_stretch_factor: None,
                pitch_shift_semitones: None,
                beat_grid: None,
                volume_envelope: Vec::new(),
                volume_envelope_frames: Default::default(),
            }],
            gain_db: -2.0,
            pan: 0.0,
            muted: false,
            soloed: false,
            effects: vec![
                EffectInstance {
                    kind: "low_pass_filter".into(),
                    params: serde_json::json!({ "cutoff_hz": 800.0 }),
                    bypassed: false,
                },
                EffectInstance {
                    kind: "wobbulator".into(),
                    params: serde_json::json!({}),
                    bypassed: false,
                },
            ],
            sends: Vec::new(),
        }],
        bus_routing: BusGraph::default(),
        master_chain: Vec::new(),
        tempo_map: TempoMap::default(),
        key_map: None,
        transcript: None,
        sample_rate: PROJECT_RATE,
        length_samples: (PROJECT_RATE * 6) as u64,
        annotations: Vec::new(),
        sync_lock: false,
    }
}

fn range(start_frame: u64, end_frame: u64) -> TimeRange {
    TimeRange {
        start_frame,
        end_frame,
    }
}

#[test]
fn pre_cancel_leaves_absent_and_existing_outputs_untouched_before_graph_build() {
    let dir = TempDir::new().unwrap();
    let mut state = fixture(dir.path());
    // Without cancellation this fails graph construction with NoTrack.
    state.tracks.clear();
    let span = range(0, PROJECT_RATE as u64);
    let absent = dir.path().join("absent.wav");
    assert!(matches!(
        render_state_range_to_wav_cancellable(&state, &absent, span, &|| true).unwrap(),
        RenderOutcome::Cancelled
    ));
    assert!(!absent.exists());

    let existing = dir.path().join("existing.wav");
    std::fs::write(&existing, b"keep this output").unwrap();
    assert!(matches!(
        Engine::new()
            .render_range_to_wav_cancellable(&state, &existing, span, &|| true)
            .unwrap(),
        RenderOutcome::Cancelled
    ));
    assert_eq!(std::fs::read(existing).unwrap(), b"keep this output");
}

#[test]
fn cancellation_during_prefix_warmup_writes_no_output_frames() {
    let dir = TempDir::new().unwrap();
    let state = fixture(dir.path());
    let out = dir.path().join("prefix.wav");
    let checks = Cell::new(0);
    let cancel = || {
        if !out.exists() {
            return false;
        }
        checks.set(checks.get() + 1);
        // Cancel before the second prefix block of a three-second
        // warmup; preparation checks do not count as streaming work.
        checks.get() >= 2
    };
    let outcome = render_state_range_to_wav_cancellable(
        &state,
        &out,
        range((PROJECT_RATE * 3) as u64 + 17, (PROJECT_RATE * 5) as u64),
        &cancel,
    )
    .unwrap();
    assert!(matches!(outcome, RenderOutcome::Cancelled));
    assert_eq!(WavReader::open(out).unwrap().duration(), 0);
}

#[test]
fn cancellation_between_output_blocks_leaves_only_one_block() {
    let dir = TempDir::new().unwrap();
    let state = fixture(dir.path());
    let out = dir.path().join("partial.wav");
    let checks = Cell::new(0);
    let cancel = || {
        if !out.exists() {
            return false;
        }
        checks.set(checks.get() + 1);
        checks.get() >= 2
    };
    let outcome = render_state_range_to_wav_cancellable(
        &state,
        &out,
        range(0, (PROJECT_RATE * 5) as u64),
        &cancel,
    )
    .unwrap();
    assert!(matches!(outcome, RenderOutcome::Cancelled));
    assert_eq!(WavReader::open(out).unwrap().duration(), PROJECT_RATE);
}

#[test]
fn cancellation_after_the_final_partial_block_is_not_complete() {
    let dir = TempDir::new().unwrap();
    let state = fixture(dir.path());
    let out = dir.path().join("final-block.wav");
    let checks = Cell::new(0);
    let cancel = || {
        if !out.exists() {
            return false;
        }
        checks.set(checks.get() + 1);
        // Two output blocks followed by the final streaming check.
        checks.get() >= 3
    };
    let outcome = render_state_range_to_wav_cancellable(
        &state,
        &out,
        range(0, PROJECT_RATE as u64 + 17),
        &cancel,
    )
    .unwrap();
    assert!(matches!(outcome, RenderOutcome::Cancelled));
    assert_eq!(WavReader::open(out).unwrap().duration(), PROJECT_RATE + 17);
}

#[test]
fn uncancelled_non_aligned_resampled_filtered_range_preserves_bytes_and_report() {
    let dir = TempDir::new().unwrap();
    let state = fixture(dir.path());
    let legacy = dir.path().join("legacy.wav");
    let controlled = dir.path().join("controlled.wav");
    let span = range(PROJECT_RATE as u64 + 17, (PROJECT_RATE * 4) as u64 + 123);
    let expected = render_state_to_wav(&state, &legacy, Some(span)).unwrap();
    let RenderOutcome::Complete(actual) = Engine::new()
        .render_range_to_wav_cancellable(&state, &controlled, span, &|| false)
        .unwrap()
    else {
        panic!("a false cancellation callback must complete");
    };
    assert_eq!(
        std::fs::read(controlled).unwrap(),
        std::fs::read(legacy).unwrap()
    );
    assert_eq!(actual.frames_written, 144_106);
    assert_eq!(actual.frames_written, expected.frames_written);
    assert_eq!(actual.sample_rate, expected.sample_rate);
    assert_eq!(actual.channels, expected.channels);
    assert_eq!(actual.peak_dbfs.to_bits(), expected.peak_dbfs.to_bits());
    assert!(!actual.warnings.is_empty());
    assert_eq!(actual.warnings, expected.warnings);
}

#[test]
fn uncancelled_errors_keep_their_existing_variants() {
    let dir = TempDir::new().unwrap();
    let mut state = fixture(dir.path());
    let out = dir.path().join("invalid.wav");
    for span in [range(5, 5), range(20, 10), range(400_000, 400_100)] {
        assert!(matches!(
            render_state_to_wav(&state, &out, Some(span)),
            Err(Error::InvalidRange)
        ));
        assert!(matches!(
            render_state_range_to_wav_cancellable(&state, &out, span, &|| false),
            Err(Error::InvalidRange)
        ));
        assert!(!out.exists());
    }
    let missing_parent = dir.path().join("missing").join("out.wav");
    assert!(matches!(
        render_state_to_wav(&state, &missing_parent, Some(range(0, 100))),
        Err(Error::Wav(_))
    ));
    assert!(matches!(
        render_state_range_to_wav_cancellable(&state, &missing_parent, range(0, 100), &|| false),
        Err(Error::Wav(_))
    ));
    state.tracks.clear();
    assert!(matches!(
        render_state_range_to_wav_cancellable(&state, &out, range(0, 100), &|| false),
        Err(Error::NoTrack)
    ));
}
