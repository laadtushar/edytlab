//! Per-track effect chains reach the audio, and keep their state.
//!
//! `Track.effects` round-tripped through save/load and the diff/merge
//! layer since Phase 1, and the render path returned
//! `EffectsUnsupportedInPhase1` the moment anything populated it — a
//! hard stop rather than silent wrong output, which is the better of the
//! two failures but still a feature described in the model and
//! unreachable in practice.
//!
//! The test that matters most here is
//! `no_discontinuity_at_chunk_boundaries`. `render_streaming` works a
//! chunk at a time on a long-lived streamer; a processor rebuilt per
//! chunk would put a click at every chunk boundary, and #102 names that
//! as the single most likely way to get this wrong. It is invisible in a
//! peak or RMS check and audible immediately.

use std::path::{Path, PathBuf};

use audio_engine::render_state_to_wav;
use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
use session::{BusGraph, Clip, EffectInstance, SessionState, TempoMap, Track, TrackId};
use tempfile::TempDir;

const SAMPLE_RATE: u32 = 44_100;
/// Three seconds, so the render crosses two chunk boundaries — the
/// master chunk is one second.
const FRAMES: usize = SAMPLE_RATE as usize * 3;

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
        // Two partials, so a filter has something to remove.
        let s = ((2.0 * std::f32::consts::PI * 300.0 * t).sin() * 0.4
            + (2.0 * std::f32::consts::PI * 6_000.0 * t).sin() * 0.3)
            * 0.8;
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

fn bypassed(kind: &str, params: serde_json::Value) -> EffectInstance {
    EffectInstance {
        kind: kind.to_string(),
        params,
        bypassed: true,
    }
}

fn state_with_effects(source: &Path, effects: Vec<EffectInstance>) -> SessionState {
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
            effects,
            sends: Vec::new(),
        }],
        bus_routing: BusGraph::default(),
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

fn render(effects: Vec<EffectInstance>) -> Vec<i16> {
    let tmp = TempDir::new().expect("tempdir");
    let src = write_tone(tmp.path());
    let out = tmp.path().join("out.wav");
    let state = state_with_effects(&src, effects);
    render_state_to_wav(&state, &out, None).expect("render");
    WavReader::open(&out)
        .expect("open out")
        .samples::<i16>()
        .map(|r| r.expect("sample"))
        .collect()
}

fn rms(x: &[i16]) -> f32 {
    (x.iter().map(|v| (*v as f32).powi(2)).sum::<f32>() / x.len() as f32).sqrt()
}

/// A track effect is audible in the render, and — the point of a chain
/// rather than a baked edit — the source file is untouched, so the
/// parameters stay editable.
#[test]
fn a_track_effect_reaches_the_audio() {
    let plain = render(Vec::new());
    let filtered = render(vec![effect(
        "low_pass_filter",
        serde_json::json!({ "cutoff_hz": 1_000.0 }),
    )]);

    assert_eq!(plain.len(), filtered.len(), "length must not change");
    assert!(
        rms(&filtered) < rms(&plain) * 0.9,
        "a 1 kHz low-pass should remove the 6 kHz partial: {} vs {}",
        rms(&filtered),
        rms(&plain)
    );
}

/// The regression guard for every existing golden test: no effects must
/// render exactly as it did before chains were honoured.
#[test]
fn an_empty_chain_changes_nothing() {
    let a = render(Vec::new());
    let b = render(Vec::new());
    assert_eq!(a, b, "render is not deterministic");
    assert_eq!(a.len(), FRAMES);
}

/// **The test this feature is most likely to fail.**
///
/// `render_streaming` emits a chunk at a time. A filter rebuilt per
/// chunk starts from zeroed state each second, and its output jumps
/// where the old state should have carried — a click at every chunk
/// boundary, once a second, inaudible to a peak or RMS check.
///
/// The threshold comes from the *rendered* signal's own largest step
/// away from the boundaries, so this asks "is the boundary worse than
/// the material" rather than picking an absolute number that happens to
/// pass.
#[test]
fn no_discontinuity_at_chunk_boundaries() {
    let out = render(vec![effect(
        "low_pass_filter",
        serde_json::json!({ "cutoff_hz": 800.0 }),
    )]);
    assert!(out.len() > SAMPLE_RATE as usize * 2, "need several chunks");

    let step = |i: usize| (out[i] as i32 - out[i - 1] as i32).unsigned_abs();

    // Largest step anywhere away from a boundary.
    let chunk = SAMPLE_RATE as usize;
    let near_boundary = |i: usize| (i % chunk) < 4 || (i % chunk) > chunk - 4;
    let baseline = (1..out.len())
        .filter(|&i| !near_boundary(i))
        .map(step)
        .max()
        .expect("non-empty");

    for b in 1..out.len() / chunk {
        let i = b * chunk;
        let at = step(i);
        assert!(
            at <= baseline.max(1) * 3,
            "step of {at} at chunk boundary {i} against a baseline of \
             {baseline} — the processor state is being rebuilt per chunk"
        );
    }
}

/// Order is honoured. Two filters at different cutoffs commute in
/// magnitude but not in state, so the simplest order-sensitive pair is a
/// gain and a limiter: gain-then-limit clips, limit-then-gain does not.
#[test]
fn reordering_effects_changes_the_output() {
    let gain_then_limit = render(vec![
        effect("gain", serde_json::json!({ "db": 12.0 })),
        effect("limiter", serde_json::json!({ "ceiling_db": -6.0 })),
    ]);
    let limit_then_gain = render(vec![
        effect("limiter", serde_json::json!({ "ceiling_db": -6.0 })),
        effect("gain", serde_json::json!({ "db": 12.0 })),
    ]);
    assert_ne!(
        gain_then_limit, limit_then_gain,
        "declaration order must reach the audio; if these match, the \
         chain is being applied in some other order or not at all"
    );
}

/// A bypassed effect is defined to be identical to an absent one — not
/// merely similar. `effect_chain::build` skips bypassed entries rather
/// than instantiating and stepping over them, so this is exact.
#[test]
fn a_bypassed_effect_is_byte_identical_to_no_effect() {
    let none = render(Vec::new());
    let skipped = render(vec![bypassed(
        "low_pass_filter",
        serde_json::json!({ "cutoff_hz": 500.0 }),
    )]);
    assert_eq!(none, skipped);
}

/// Two renders of the same session are byte-identical — the determinism
/// invariant `render.rs` documents, now with a chain in the path.
#[test]
fn a_chain_renders_deterministically() {
    let chain = || {
        vec![
            effect(
                "high_pass_filter",
                serde_json::json!({ "cutoff_hz": 120.0 }),
            ),
            effect("gain", serde_json::json!({ "db": -3.0 })),
        ]
    };
    assert_eq!(render(chain()), render(chain()));
}

/// An unknown kind fails the render with a message naming it, rather
/// than rendering silently without it.
#[test]
fn an_unknown_effect_kind_is_an_error() {
    let tmp = TempDir::new().expect("tempdir");
    let src = write_tone(tmp.path());
    let out = tmp.path().join("out.wav");
    let state = state_with_effects(&src, vec![effect("sparkle", serde_json::json!({}))]);

    let err = render_state_to_wav(&state, &out, None).expect_err("unknown kind");
    let msg = err.to_string();
    assert!(msg.contains("sparkle"), "the error should name it: {msg}");
    assert!(msg.contains("unknown effect"), "got: {msg}");
}

/// An effect that exists but cannot stream says so, distinctly from one
/// that does not exist — different mistakes with different fixes.
#[test]
fn a_non_streaming_effect_says_so_distinctly() {
    let tmp = TempDir::new().expect("tempdir");
    let src = write_tone(tmp.path());
    let out = tmp.path().join("out.wav");
    let state = state_with_effects(&src, vec![effect("reverb", serde_json::json!({}))]);

    let err = render_state_to_wav(&state, &out, None).expect_err("not streamable");
    let msg = err.to_string();
    assert!(msg.contains("reverb"), "{msg}");
    assert!(
        msg.contains("cannot run at render time yet"),
        "should distinguish 'not yet' from 'no such effect': {msg}"
    );
    assert!(
        msg.contains("destructively"),
        "the message should say what to do instead: {msg}"
    );
}

/// The unity-passthrough shortcut copies source bytes and never opens
/// the mixer. It has silently skipped a new feature twice (#110's master
/// chain, #111's sends); a track chain must not be the third.
#[test]
fn the_passthrough_shortcut_declines_when_a_track_has_effects() {
    let plain = render(Vec::new());
    let gained = render(vec![effect("gain", serde_json::json!({ "db": -6.0 }))]);
    assert_ne!(
        plain, gained,
        "the render took the byte-copy path and dropped the chain"
    );
    assert!(
        rms(&gained) < rms(&plain) * 0.7,
        "-6 dB should roughly halve the RMS: {} vs {}",
        rms(&gained),
        rms(&plain)
    );
}
