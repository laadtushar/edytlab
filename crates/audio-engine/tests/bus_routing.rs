//! Sends reach the mix.
//!
//! `BusGraph` has been in the session schema since Phase 1 with no way
//! for audio to enter it — `Bus` had a name and an effect list and no
//! input. These tests pin that a send is a *parallel copy* (the track
//! still reaches master at full level), that a bus's own effects apply
//! to the summed send rather than per-track, and that a send naming a
//! bus that does not exist fails the render rather than vanishing.

use std::path::{Path, PathBuf};

use audio_engine::render_state_to_wav;
use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
use session::{Bus, BusGraph, Clip, EffectInstance, Send, SessionState, TempoMap, Track, TrackId};
use tempfile::TempDir;
use uuid::Uuid;

const SAMPLE_RATE: u32 = 44_100;
const FRAMES: usize = 44_100;

fn write_tone(dir: &Path) -> PathBuf {
    let path = dir.join("tone.wav");
    let spec = WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut w = WavWriter::create(&path, spec).expect("wav writer");
    for n in 0..FRAMES {
        let t = n as f32 / SAMPLE_RATE as f32;
        // 0.2 so the loudest case in these tests (two tracks each
        // sending at 0 dB = 4x) still fits under full scale. At 0.4 it
        // clips, and a clipped sum measures low for reasons that have
        // nothing to do with routing.
        let s = (2.0 * std::f32::consts::PI * 440.0 * t).sin() * 0.2;
        w.write_sample((s * 32_767.0) as i16).unwrap();
    }
    w.finalize().unwrap();
    path
}

fn state(source: &Path, buses: Vec<Bus>, sends: Vec<Send>) -> SessionState {
    let decoded = audio_decoder::decode_file(source).expect("decode");
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
                volume_envelope: Vec::new(),
            }],
            gain_db: 0.0,
            pan: 0.0,
            muted: false,
            soloed: false,
            effects: Vec::new(),
            sends,
        }],
        bus_routing: BusGraph { buses },
        master_chain: Vec::new(),
        tempo_map: TempoMap::default(),
        key_map: None,
        transcript: None,
        sample_rate: decoded.sample_rate,
        length_samples: frames,
        annotations: Vec::new(),
        sync_lock: false,
    }
}

fn render(buses: Vec<Bus>, sends: Vec<Send>) -> Vec<i16> {
    let tmp = TempDir::new().expect("tempdir");
    let src = write_tone(tmp.path());
    let out = tmp.path().join("out.wav");
    render_state_to_wav(&state(&src, buses, sends), &out, None).expect("render");
    let mut reader = WavReader::open(&out).expect("open out");
    reader.samples::<i16>().map(|r| r.expect("s")).collect()
}

fn rms(x: &[i16]) -> f32 {
    (x.iter().map(|v| (*v as f32).powi(2)).sum::<f32>() / x.len() as f32).sqrt()
}

fn bus(name: &str, effects: Vec<EffectInstance>) -> Bus {
    Bus {
        id: Uuid::new_v4(),
        name: name.into(),
        effects,
    }
}

fn gain(db: f32) -> EffectInstance {
    EffectInstance {
        kind: "gain".into(),
        params: serde_json::json!({ "db": db }),
        bypassed: false,
    }
}

/// The regression guard: a session with no buses renders exactly as it
/// did before buses existed.
#[test]
fn no_sends_changes_nothing() {
    let plain = render(Vec::new(), Vec::new());
    let with_unused_bus = render(vec![bus("Reverb", Vec::new())], Vec::new());
    assert_eq!(
        plain, with_unused_bus,
        "a bus nothing sends to must not affect the mix"
    );
}

/// A send is **parallel**, not a move. The track still reaches master at
/// full level and the bus adds a scaled copy on top, so the result is
/// louder — not merely different.
#[test]
fn a_send_adds_to_the_mix_rather_than_diverting() {
    let b = bus("Reverb", Vec::new());
    let plain = render(Vec::new(), Vec::new());
    let sent = render(
        vec![b.clone()],
        vec![Send {
            bus_id: b.id,
            level_db: 0.0,
        }],
    );

    let ratio = rms(&sent) / rms(&plain);
    assert!(
        (ratio - 2.0).abs() < 0.05,
        "a 0 dB send with no bus effects should double the signal \
         (original + copy), got {ratio:.3}x — a ratio near 1.0 means the \
         send was dropped, and near 0 means the track was diverted"
    );
}

/// The level actually scales the copy.
#[test]
fn send_level_scales_the_copy() {
    let b = bus("Reverb", Vec::new());
    let plain = render(Vec::new(), Vec::new());
    let sent = render(
        vec![b.clone()],
        vec![Send {
            bus_id: b.id,
            level_db: -6.0,
        }],
    );

    // original (1.0) + copy (~0.501) = ~1.501
    let ratio = rms(&sent) / rms(&plain);
    assert!(
        (ratio - 1.501).abs() < 0.05,
        "a -6 dB send should add roughly half the signal, got {ratio:.3}x"
    );
}

/// A bus's effects apply to what it received. Attenuating the bus to
/// silence must leave exactly the dry track.
#[test]
fn bus_effects_process_the_send_not_the_master() {
    let b = bus("Reverb", vec![gain(-120.0)]);
    let plain = render(Vec::new(), Vec::new());
    let sent = render(
        vec![b.clone()],
        vec![Send {
            bus_id: b.id,
            level_db: 0.0,
        }],
    );

    let ratio = rms(&sent) / rms(&plain);
    assert!(
        (ratio - 1.0).abs() < 0.01,
        "a bus attenuated to silence should contribute nothing, leaving \
         the dry track — got {ratio:.3}x"
    );
}

/// Two tracks feeding one bus is the point of a bus: one reverb, not
/// two. The bus chain must see their sum.
#[test]
fn a_bus_sums_sends_from_several_tracks() {
    let tmp = TempDir::new().expect("tempdir");
    let src = write_tone(tmp.path());
    let out = tmp.path().join("out.wav");

    let b = bus("Reverb", Vec::new());
    let mut st = state(
        &src,
        vec![b.clone()],
        vec![Send {
            bus_id: b.id,
            level_db: 0.0,
        }],
    );
    // Second identical track, also sending.
    let second = st.tracks[0].clone();
    st.tracks.push(Track {
        id: TrackId::new(),
        ..second
    });

    render_state_to_wav(&st, &out, None).expect("render");
    let mut reader = WavReader::open(&out).expect("open out");
    let pcm: Vec<i16> = reader.samples::<i16>().map(|r| r.expect("s")).collect();

    let one_track_dry = render(Vec::new(), Vec::new());
    // 2 tracks dry + 2 copies through the bus = 4x one dry track.
    let ratio = rms(&pcm) / rms(&one_track_dry);
    assert!(
        (ratio - 4.0).abs() < 0.15,
        "two tracks each sending at 0 dB should give 4x one dry track, \
         got {ratio:.3}x"
    );
}

/// Dropping a send silently would mean the mix is missing a signal path
/// the session says is there — the failure `master_chain` had before
/// #110.
#[test]
fn a_send_to_a_missing_bus_fails_the_render() {
    let tmp = TempDir::new().expect("tempdir");
    let src = write_tone(tmp.path());
    let out = tmp.path().join("out.wav");
    let orphan = Uuid::new_v4();

    let st = state(
        &src,
        Vec::new(),
        vec![Send {
            bus_id: orphan,
            level_db: 0.0,
        }],
    );
    let err = render_state_to_wav(&st, &out, None)
        .expect_err("a send to a bus that does not exist must fail the render");
    assert!(err.to_string().contains(&orphan.to_string()), "got {err}");
}

/// The unity fast path byte-copies the source and never opens the
/// mixer, so it has to decline when a send is present — the same trap
/// the master chain hit in #110.
#[test]
fn the_unity_fast_path_declines_when_a_send_exists() {
    let b = bus("Reverb", Vec::new());
    let plain = render(Vec::new(), Vec::new());
    let sent = render(
        vec![b.clone()],
        vec![Send {
            bus_id: b.id,
            level_db: 0.0,
        }],
    );
    assert_ne!(
        plain, sent,
        "identical output means the render was served by the copy path \
         and the send never happened"
    );
}
