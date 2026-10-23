//! The unit of a clip's volume envelope (#677).
//!
//! A new envelope is counted in the clip's own frames and the renderer
//! converts it, like the clip's other positions; one stored before the unit
//! was recorded is counted in project frames and plays as it always did. On
//! a clip at the project's rate the two are the same number, so a clip there
//! renders the same whichever way its envelope is marked.

use std::path::{Path, PathBuf};

use audio_engine::render_state_to_wav;
use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
use session::{
    BusGraph, Clip, EnvelopeFrames, EnvelopePoint, SessionState, TempoMap, Track, TrackId,
};
use tempfile::TempDir;

/// One second of a steady tone, so the gain at a time is the peak there.
fn write_tone(dir: &Path, rate: u32) -> PathBuf {
    let path = dir.join(format!("tone{rate}.wav"));
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

fn render(
    project_rate: u32,
    source_rate: u32,
    points: &[(u64, f32)],
    frames: EnvelopeFrames,
) -> Vec<i16> {
    let tmp = TempDir::new().expect("tempdir");
    let source = write_tone(tmp.path(), source_rate);
    let state = SessionState {
        tracks: vec![Track {
            id: TrackId::new(),
            name: "t".into(),
            clips: vec![Clip {
                source_path: source,
                start_in_track: 0,
                source_offset: 0,
                length: u64::from(source_rate),
                content_hash: None,
                time_stretch_factor: None,
                pitch_shift_semitones: None,
                beat_grid: None,
                volume_envelope: points
                    .iter()
                    .map(|&(time_samples, gain_db)| EnvelopePoint {
                        time_samples,
                        gain_db,
                    })
                    .collect(),
                volume_envelope_frames: frames,
            }],
            gain_db: 0.0,
            pan: 0.0,
            muted: false,
            soloed: false,
            effects: Vec::new(),
            sends: Vec::new(),
        }],
        bus_routing: BusGraph::default(),
        master_chain: Vec::new(),
        tempo_map: TempoMap::default(),
        key_map: None,
        transcript: None,
        sample_rate: project_rate,
        length_samples: u64::from(source_rate),
        annotations: Vec::new(),
        sync_lock: false,
    };
    let out = tmp.path().join("out.wav");
    render_state_to_wav(&state, &out, None).expect("render");
    WavReader::open(&out)
        .expect("open out")
        .samples::<i16>()
        .map(|s| s.expect("sample"))
        .collect()
}

fn peak(x: &[i16]) -> f32 {
    x.iter()
        .map(|v| f32::from(v.unsigned_abs()))
        .fold(0.0, f32::max)
        / 32_767.0
}

/// 0 dB to half a second, -60 dB from 0.55 s: `at` and `after` are the
/// half second and the 0.55 s in whichever frames the caller counts.
fn drop_at_half(at: u64, after: u64, end: u64) -> Vec<(u64, f32)> {
    vec![(0, 0.0), (at, 0.0), (after, -60.0), (end, -60.0)]
}

#[test]
fn a_source_frame_envelope_is_converted_to_project_frames() {
    // A 16 kHz clip in an 8 kHz project: half a second is frame 8 000 of the
    // clip and 4 000 of the project.
    let pcm = render(
        8_000,
        16_000,
        &drop_at_half(8_000, 8_800, 16_000),
        EnvelopeFrames::Source,
    );
    assert_eq!(pcm.len(), 8_000);
    assert!(peak(&pcm[500..3_800]) > 0.3, "loud before the drop");
    assert!(peak(&pcm[4_500..7_000]) < 0.01, "silent after the drop");
}

#[test]
fn a_project_frame_envelope_is_read_as_it_always_was() {
    let pcm = render(
        8_000,
        16_000,
        &drop_at_half(4_000, 4_400, 8_000),
        EnvelopeFrames::Project,
    );
    assert!(peak(&pcm[500..3_800]) > 0.3, "loud before the drop");
    assert!(peak(&pcm[4_500..7_000]) < 0.01, "silent after the drop");
}

#[test]
fn a_source_frame_envelope_is_converted_the_other_way_too() {
    // An 8 kHz clip in a 16 kHz project.
    let pcm = render(
        16_000,
        8_000,
        &drop_at_half(4_000, 4_400, 8_000),
        EnvelopeFrames::Source,
    );
    assert_eq!(pcm.len(), 16_000);
    assert!(peak(&pcm[1_000..7_600]) > 0.3, "loud before the drop");
    assert!(peak(&pcm[9_000..14_000]) < 0.01, "silent after the drop");
}

#[test]
fn at_the_projects_rate_the_two_units_render_the_same_bytes() {
    let points = drop_at_half(4_000, 4_400, 8_000);
    let project = render(8_000, 8_000, &points, EnvelopeFrames::Project);
    let source = render(8_000, 8_000, &points, EnvelopeFrames::Source);
    assert_eq!(project, source);
}
