//! Track pan has to reach the mixdown.
//!
//! `set_pan` is a registered tool whose description promises "-1.0 = full
//! left, 0.0 = centre, 1.0 = full right", and it duly writes `track.pan`
//! into the session. The render graph read that field in exactly one
//! place — a guard that disables the byte-copy fast path — and never
//! applied it. Panning a track was silent in both senses: nothing moved,
//! and nothing said so.

use std::path::{Path, PathBuf};

use audio_engine::render_state_to_wav;
use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
use session::{BusGraph, Clip, EffectInstance, SessionState, TempoMap, Track, TrackId};
use tempfile::TempDir;

const RATE: u32 = 8_000;
const FRAMES: usize = 800;

fn write_tone(dir: &Path, name: &str, channels: u16) -> PathBuf {
    let path = dir.join(name);
    let spec = WavSpec {
        channels,
        sample_rate: RATE,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut writer = WavWriter::create(&path, spec).expect("wav writer");
    for n in 0..FRAMES {
        let t = n as f32 / RATE as f32;
        let s = (2.0 * std::f32::consts::PI * 440.0 * t).sin() * 0.5;
        let q = (s * 32_768.0)
            .round()
            .clamp(i16::MIN as f32, i16::MAX as f32) as i16;
        for _ in 0..channels {
            writer.write_sample(q).unwrap();
        }
    }
    writer.finalize().unwrap();
    path
}

fn session_with_pan(source: &Path, pan: f32) -> SessionState {
    let decoded = audio_decoder::decode_file(source).expect("decode");
    let frames = (decoded.samples.len() / decoded.channels as usize) as u64;
    SessionState {
        tracks: vec![Track {
            id: TrackId::new(),
            name: "t".into(),
            clips: vec![Clip {
                source_path: source.to_path_buf(),
                start_in_track: 0,
                source_offset: 0,
                length: frames,
                content_hash: None,
                time_stretch_factor: None,
                pitch_shift_semitones: None,
                beat_grid: None,
                volume_envelope: Vec::new(),
            }],
            gain_db: 0.0,
            pan,
            muted: false,
            soloed: false,
            effects: Vec::<EffectInstance>::new(),
            sends: Vec::new(),
        }],
        bus_routing: BusGraph::default(),
        master_chain: Vec::new(),
        tempo_map: TempoMap::default(),
        key_map: None,
        transcript: None,
        sample_rate: RATE,
        length_samples: frames,
        annotations: Vec::new(),
    }
}

/// Peak of each channel in the rendered file.
fn channel_peaks(path: &Path) -> Vec<f32> {
    let mut reader = WavReader::open(path).expect("open out");
    let chans = reader.spec().channels as usize;
    let samples: Vec<i16> = reader.samples::<i16>().map(|r| r.unwrap()).collect();
    let mut peaks = vec![0.0f32; chans];
    for (i, s) in samples.iter().enumerate() {
        let v = (*s as f32 / 32_768.0).abs();
        let ch = i % chans;
        if v > peaks[ch] {
            peaks[ch] = v;
        }
    }
    peaks
}

/// Hard left means the right channel is silent.
#[test]
fn hard_left_empties_the_right_channel() {
    let tmp = TempDir::new().unwrap();
    let src = write_tone(tmp.path(), "stereo.wav", 2);
    let out = tmp.path().join("out.wav");

    render_state_to_wav(&session_with_pan(&src, -1.0), &out, None).expect("render");
    let peaks = channel_peaks(&out);

    assert_eq!(peaks.len(), 2, "a panned render must be stereo");
    assert!(
        peaks[0] > 0.4,
        "the left channel should keep the signal, peak was {}",
        peaks[0]
    );
    assert!(
        peaks[1] < 0.001,
        "hard left must empty the right channel, peak was {}",
        peaks[1]
    );
}

#[test]
fn hard_right_empties_the_left_channel() {
    let tmp = TempDir::new().unwrap();
    let src = write_tone(tmp.path(), "stereo.wav", 2);
    let out = tmp.path().join("out.wav");

    render_state_to_wav(&session_with_pan(&src, 1.0), &out, None).expect("render");
    let peaks = channel_peaks(&out);

    assert!(peaks[0] < 0.001, "peak was {}", peaks[0]);
    assert!(peaks[1] > 0.4, "peak was {}", peaks[1]);
}

/// Half-left attenuates the right without silencing it.
#[test]
fn partial_pan_attenuates_the_far_side() {
    let tmp = TempDir::new().unwrap();
    let src = write_tone(tmp.path(), "stereo.wav", 2);
    let out = tmp.path().join("out.wav");

    render_state_to_wav(&session_with_pan(&src, -0.5), &out, None).expect("render");
    let peaks = channel_peaks(&out);

    assert!(
        (peaks[0] - 0.5).abs() < 0.02,
        "the near side stays at unity, got {}",
        peaks[0]
    );
    assert!(
        (peaks[1] - 0.25).abs() < 0.02,
        "half-left should halve the right channel, got {}",
        peaks[1]
    );
}

/// Centre must be bit-for-bit what it was before pan existed.
///
/// Every track defaults to pan 0, so a pan law that attenuates the centre
/// — as a constant-power law does, by 3 dB — would quietly change the
/// output of every session ever rendered.
#[test]
fn centre_is_unity_and_unchanged() {
    let tmp = TempDir::new().unwrap();
    let src = write_tone(tmp.path(), "stereo.wav", 2);
    let out = tmp.path().join("out.wav");

    render_state_to_wav(&session_with_pan(&src, 0.0), &out, None).expect("render");
    let peaks = channel_peaks(&out);

    assert!(
        (peaks[0] - 0.5).abs() < 0.005 && (peaks[1] - 0.5).abs() < 0.005,
        "centre must pass through untouched, got {peaks:?}"
    );
}

/// A mono source can still be panned — the mix widens to stereo to hold
/// it, rather than silently discarding the pan.
#[test]
fn a_mono_source_panned_hard_left_still_renders_stereo() {
    let tmp = TempDir::new().unwrap();
    let src = write_tone(tmp.path(), "mono.wav", 1);
    let out = tmp.path().join("out.wav");

    render_state_to_wav(&session_with_pan(&src, -1.0), &out, None).expect("render");
    let peaks = channel_peaks(&out);

    assert_eq!(
        peaks.len(),
        2,
        "panning a mono track has to widen the mix, or the pan is lost"
    );
    assert!(peaks[0] > 0.4, "peak was {}", peaks[0]);
    assert!(peaks[1] < 0.001, "peak was {}", peaks[1]);
}
