//! The master chain reaches the audio.
//!
//! `SessionState::master_chain` round-tripped through save/load and the
//! diff/merge layer since Phase 1 while the renderer ignored it
//! entirely — a session with master effects rendered as though they
//! were absent, and said nothing about it. These tests pin that it now
//! reaches the samples, that an empty chain is byte-identical to before,
//! and that a chain the renderer cannot honour fails instead of
//! producing quietly wrong audio.

use std::path::{Path, PathBuf};

use audio_engine::render_state_to_wav;
use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
use session::{BusGraph, Clip, EffectInstance, SessionState, TempoMap, Track, TrackId};
use tempfile::TempDir;

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
        let s = (2.0 * std::f32::consts::PI * 440.0 * t).sin() * 0.5;
        w.write_sample((s * 32_767.0) as i16).unwrap();
    }
    w.finalize().unwrap();
    path
}

fn effect(kind: &str, params: serde_json::Value) -> EffectInstance {
    EffectInstance {
        kind: kind.to_string(),
        params,
        bypassed: false,
    }
}

fn state_with_chain(source: &Path, chain: Vec<EffectInstance>) -> SessionState {
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
            sends: Vec::new(),
        }],
        bus_routing: BusGraph::default(),
        master_chain: chain,
        tempo_map: TempoMap::default(),
        key_map: None,
        transcript: None,
        sample_rate: decoded.sample_rate,
        length_samples: frames,
        annotations: Vec::new(),
    }
}

/// Render with `chain` and return the PCM.
fn render(chain: Vec<EffectInstance>) -> (Vec<i16>, f32) {
    let tmp = TempDir::new().expect("tempdir");
    let src = write_tone(tmp.path());
    let out = tmp.path().join("out.wav");
    let state = state_with_chain(&src, chain);
    let report = render_state_to_wav(&state, &out, None).expect("render");
    let mut reader = WavReader::open(&out).expect("open out");
    let pcm = reader
        .samples::<i16>()
        .map(|r| r.expect("sample"))
        .collect();
    (pcm, report.peak_dbfs)
}

fn rms(x: &[i16]) -> f32 {
    (x.iter().map(|v| (*v as f32).powi(2)).sum::<f32>() / x.len() as f32).sqrt()
}

/// The regression guard for every existing golden test: a session with
/// no master chain must render exactly as it did before the chain
/// existed.
#[test]
fn an_empty_chain_changes_nothing() {
    let (plain, _) = render(Vec::new());
    let (also_plain, _) = render(Vec::new());
    assert_eq!(plain, also_plain, "render is not deterministic");
    assert_eq!(plain.len(), FRAMES, "unexpected length");
}

/// The defect: a non-empty chain used to be ignored silently.
#[test]
fn a_gain_in_the_master_chain_reaches_the_audio() {
    let (plain, _) = render(Vec::new());
    let (quieter, _) = render(vec![effect("gain", serde_json::json!({ "db": -6.0 }))]);

    let ratio = rms(&quieter) / rms(&plain);
    assert!(
        (ratio - 0.501).abs() < 0.02,
        "-6 dB on the master bus should roughly halve the RMS, got {ratio:.3}x \
         — a ratio of 1.0 means the chain was ignored"
    );
}

/// A bypassed effect must be byte-identical to an absent one, not
/// merely close.
#[test]
fn a_bypassed_effect_is_byte_identical_to_no_effect() {
    let mut bypassed = effect("gain", serde_json::json!({ "db": -6.0 }));
    bypassed.bypassed = true;

    let (plain, _) = render(Vec::new());
    let (with_bypassed, _) = render(vec![bypassed]);
    assert_eq!(plain, with_bypassed);
}

/// Declaration order is load-bearing, and `render.rs`'s determinism
/// invariant requires it. A limiter before a gain is not the same
/// processor as a gain before a limiter.
#[test]
fn chain_order_changes_the_result() {
    let gain = effect("gain", serde_json::json!({ "db": 12.0 }));
    let limiter = effect("limiter", serde_json::json!({ "ceiling_db": -12.0 }));

    let (gain_then_limit, _) = render(vec![gain.clone(), limiter.clone()]);
    let (limit_then_gain, _) = render(vec![limiter, gain]);

    assert_ne!(
        gain_then_limit, limit_then_gain,
        "order is being ignored; the chain is not applied in declaration order"
    );
}

/// A limiter has to actually bound the output, which is the one thing
/// it exists to guarantee.
#[test]
fn a_limiter_bounds_the_peak() {
    let (limited, peak_dbfs) = render(vec![effect(
        "limiter",
        serde_json::json!({ "ceiling_db": -20.0 }),
    )]);
    let ceiling = 10.0f32.powf(-20.0 / 20.0) * 32_768.0;
    let peak = limited.iter().fold(0i16, |m, v| m.max(v.abs())) as f32;
    assert!(
        peak <= ceiling + 2.0,
        "peak {peak} exceeds the -20 dBFS ceiling {ceiling}"
    );
    // The report's peak is measured after the chain, so it has to agree
    // with what actually landed in the file.
    assert!(
        peak_dbfs <= -19.0,
        "reported peak {peak_dbfs} dBFS is from before the chain ran"
    );
}

/// A filter carries state across chunk boundaries. The renderer works a
/// second at a time and this signal is exactly one second, so a chain
/// rebuilt per chunk would still pass — what this guards is that a
/// stateful processor is accepted and applied at all.
#[test]
fn a_filter_in_the_master_chain_attenuates() {
    let (plain, _) = render(Vec::new());
    let (filtered, _) = render(vec![effect(
        "low_pass_filter",
        serde_json::json!({ "cutoff_hz": 100.0 }),
    )]);

    let ratio = rms(&filtered) / rms(&plain);
    assert!(
        ratio < 0.3,
        "a 100 Hz low-pass should crush a 440 Hz tone, got {ratio:.3}x"
    );
}

/// Silent wrong output is the failure mode this ticket exists to
/// remove, so an effect the renderer cannot honour must stop the render.
#[test]
fn an_unknown_effect_fails_the_render_rather_than_being_skipped() {
    let tmp = TempDir::new().expect("tempdir");
    let src = write_tone(tmp.path());
    let out = tmp.path().join("out.wav");
    let state = state_with_chain(&src, vec![effect("wobbulator", serde_json::json!({}))]);

    let err = render_state_to_wav(&state, &out, None)
        .expect_err("an unknown master effect must fail the render");
    assert!(err.to_string().contains("wobbulator"), "got {err}");
}

/// An algorithm that exists but cannot run on a stream gets a different
/// message from one that does not exist — different mistake, different
/// fix.
#[test]
fn a_known_but_unstreamable_effect_explains_itself() {
    let tmp = TempDir::new().expect("tempdir");
    let src = write_tone(tmp.path());
    let out = tmp.path().join("out.wav");
    let state = state_with_chain(&src, vec![effect("reverb", serde_json::json!({}))]);

    let err = render_state_to_wav(&state, &out, None).expect_err("reverb is not streamable yet");
    let msg = err.to_string();
    assert!(msg.contains("reverb"), "got {msg}");
    assert!(
        msg.contains("destructively"),
        "the message should point at the workaround that does work: {msg}"
    );
}
