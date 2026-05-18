//! Integration tests for the M08 tool set.
//!
//! These tests drive the dispatcher end-to-end against an on-disk
//! [`session::Store`] and a real [`audio_engine::Engine`], the same way
//! the eventual AI loop will. Each `#[test]` covers one acceptance
//! criterion from the M08 plan, plus the cross-tool sequence and the
//! `gain` composition property.

use std::path::{Path, PathBuf};

use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
use serde_json::{json, Value};
use tempfile::TempDir;
use tools::{ToolContext, ToolDispatcher, ToolResult};

const SAMPLE_RATE: u32 = 48_000;

fn fresh() -> (
    TempDir,
    session::Store,
    audio_engine::Engine,
    ToolDispatcher,
) {
    let tmp = TempDir::new().expect("tempdir");
    let store = session::Store::open(tmp.path()).expect("open store");
    let engine = audio_engine::Engine::new();
    let dispatcher = ToolDispatcher::default_dispatcher();
    (tmp, store, engine, dispatcher)
}

/// Synthesize a 1-second mono sine WAV at `peak_amp`. Returns the path.
fn write_sine_wav(dir: &Path, name: &str, peak_amp: f32) -> PathBuf {
    let path = dir.join(name);
    let spec = WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut writer = WavWriter::create(&path, spec).expect("wav writer");
    let frames = SAMPLE_RATE as usize;
    let freq = 440.0_f32;
    for n in 0..frames {
        let t = n as f32 / SAMPLE_RATE as f32;
        let s = (2.0 * std::f32::consts::PI * freq * t).sin() * peak_amp;
        let q = (s * 32_768.0)
            .round()
            .clamp(i16::MIN as f32, i16::MAX as f32) as i16;
        writer.write_sample(q).unwrap();
    }
    writer.finalize().unwrap();
    path
}

/// Read all i16 samples from a WAV file.
fn read_wav_samples(path: &Path) -> Vec<i16> {
    let mut reader = WavReader::open(path).expect("open output wav");
    reader
        .samples::<i16>()
        .map(|r| r.expect("sample read"))
        .collect()
}

fn ok(result: ToolResult) -> Value {
    match result {
        ToolResult::Ok(v) => v,
        ToolResult::Error(msg) => panic!("expected Ok, got Error({msg})"),
    }
}

fn err(result: ToolResult) -> String {
    match result {
        ToolResult::Ok(v) => panic!("expected Error, got Ok({v})"),
        ToolResult::Error(msg) => msg,
    }
}

// ---------------------------------------------------------------------------
// Acceptance criterion 1: gain(0, +6.02) doubles samples.
// ---------------------------------------------------------------------------

#[test]
fn gain_plus_6dbish_doubles_samples_after_render() {
    let (tmp, mut store, mut engine, dispatcher) = fresh();
    let src = write_sine_wav(tmp.path(), "in.wav", 0.25);
    let out = tmp.path().join("out.wav");

    let mut clipboard: Option<Vec<f32>> = None;
    let mut ctx = ToolContext {
        store: &mut store,
        engine: &mut engine,
        user_message: "",
        clipboard: &mut clipboard,
    };

    let load = ok(dispatcher
        .invoke("load", json!({ "path": src.to_string_lossy() }), &mut ctx)
        .unwrap());
    let _ = load["node_id"].as_str().unwrap();

    let six_db = 20.0 * 2f32.log10();
    let gained = ok(dispatcher
        .invoke("gain", json!({ "track": 0, "db": six_db }), &mut ctx)
        .unwrap());
    let gained_id = gained["node_id"].as_str().unwrap().to_string();

    ok(dispatcher
        .invoke(
            "render_final",
            json!({
                "node_id": gained_id,
                "format": "wav",
                "out_path": out.to_string_lossy(),
            }),
            &mut ctx,
        )
        .unwrap());

    let src_samples = read_wav_samples(&src);
    let out_samples = read_wav_samples(&out);
    assert_eq!(src_samples.len(), out_samples.len());

    let mut max_diff = 0i32;
    for (a, b) in src_samples.iter().zip(out_samples.iter()) {
        let scaled = (*a as i32) * 2;
        let diff = (scaled - *b as i32).abs();
        if diff > max_diff {
            max_diff = diff;
        }
    }
    // Float quantisation gives us up to ~1 LSB rounding per sample.
    assert!(
        max_diff <= 1,
        "expected output ~= 2x input within 1 LSB, max diff was {max_diff}"
    );
}

// ---------------------------------------------------------------------------
// Acceptance criterion 2: normalize matches the previously-rendered output
// for the same fixture (golden-style: regenerate-and-compare).
// ---------------------------------------------------------------------------

#[test]
fn normalize_brings_peak_to_target_dbfs() {
    let (tmp, mut store, mut engine, dispatcher) = fresh();
    // 0.5 amplitude == -6 dBFS peak.
    let src = write_sine_wav(tmp.path(), "in.wav", 0.5);
    let out = tmp.path().join("normalized.wav");

    let mut clipboard: Option<Vec<f32>> = None;
    let mut ctx = ToolContext {
        store: &mut store,
        engine: &mut engine,
        user_message: "",
        clipboard: &mut clipboard,
    };

    ok(dispatcher
        .invoke("load", json!({ "path": src.to_string_lossy() }), &mut ctx)
        .unwrap());

    let res = ok(dispatcher
        .invoke(
            "normalize",
            json!({ "track": 0, "target_dbfs": -1.0 }),
            &mut ctx,
        )
        .unwrap());
    let new_id = res["node_id"].as_str().unwrap().to_string();
    let applied_db = res["applied_gain_db"].as_f64().unwrap() as f32;
    // peak was -6.02 dBFS, target is -1.0 -> applied ~= +5.02 dB.
    assert!((applied_db - 5.02).abs() < 0.05, "applied_db={applied_db}");

    ok(dispatcher
        .invoke(
            "render_final",
            json!({
                "node_id": new_id,
                "format": "wav",
                "out_path": out.to_string_lossy(),
            }),
            &mut ctx,
        )
        .unwrap());

    // Verify: rendered peak is within ~0.1 dB of -1 dBFS.
    let samples = read_wav_samples(&out);
    let peak = samples
        .iter()
        .map(|s| s.unsigned_abs() as i32)
        .max()
        .unwrap();
    let peak_amp = peak as f32 / 32_768.0;
    let peak_dbfs = 20.0 * peak_amp.log10();
    assert!(
        (peak_dbfs - (-1.0)).abs() < 0.1,
        "expected peak ~= -1 dBFS, got {peak_dbfs}"
    );
}

/// Determinism check: re-running normalize on the same input twice
/// yields byte-identical output WAVs. This is the byte-for-byte
/// "golden" check from M08 expressed without a checked-in artifact.
#[test]
fn normalize_render_is_byte_deterministic() {
    let make = || -> Vec<u8> {
        let (tmp, mut store, mut engine, dispatcher) = fresh();
        let src = write_sine_wav(tmp.path(), "in.wav", 0.5);
        let out = tmp.path().join("normalized.wav");

        let mut clipboard: Option<Vec<f32>> = None;
        let mut ctx = ToolContext {
            store: &mut store,
            engine: &mut engine,
            user_message: "",
            clipboard: &mut clipboard,
        };

        ok(dispatcher
            .invoke("load", json!({ "path": src.to_string_lossy() }), &mut ctx)
            .unwrap());
        let res = ok(dispatcher
            .invoke(
                "normalize",
                json!({ "track": 0, "target_dbfs": -1.0 }),
                &mut ctx,
            )
            .unwrap());
        let new_id = res["node_id"].as_str().unwrap().to_string();
        ok(dispatcher
            .invoke(
                "render_final",
                json!({
                    "node_id": new_id,
                    "format": "wav",
                    "out_path": out.to_string_lossy(),
                }),
                &mut ctx,
            )
            .unwrap());
        std::fs::read(&out).unwrap()
    };

    let a = make();
    let b = make();
    assert_eq!(a, b, "normalize render is non-deterministic");
}

// ---------------------------------------------------------------------------
// Acceptance criterion 3: cut_range shortens the output by exactly the cut
// duration.
// ---------------------------------------------------------------------------

#[test]
fn cut_range_shortens_render_by_exact_duration() {
    let (tmp, mut store, mut engine, dispatcher) = fresh();
    let src = write_sine_wav(tmp.path(), "in.wav", 0.5);
    let out = tmp.path().join("cut.wav");

    let mut clipboard: Option<Vec<f32>> = None;
    let mut ctx = ToolContext {
        store: &mut store,
        engine: &mut engine,
        user_message: "",
        clipboard: &mut clipboard,
    };

    let load = ok(dispatcher
        .invoke("load", json!({ "path": src.to_string_lossy() }), &mut ctx)
        .unwrap());
    let original_len = load["length_samples"].as_u64().unwrap();

    // Cut at the tail so Phase 1's single-clip render stays correct.
    let cut_start = original_len - 10_000;
    let cut_end = original_len;
    let cut = ok(dispatcher
        .invoke(
            "cut_range",
            json!({
                "track": 0,
                "start_sample": cut_start,
                "end_sample": cut_end,
            }),
            &mut ctx,
        )
        .unwrap());
    let new_id = cut["node_id"].as_str().unwrap().to_string();
    assert_eq!(cut["removed_samples"].as_u64().unwrap(), 10_000);

    ok(dispatcher
        .invoke(
            "render_final",
            json!({
                "node_id": new_id,
                "format": "wav",
                "out_path": out.to_string_lossy(),
            }),
            &mut ctx,
        )
        .unwrap());

    let reader = WavReader::open(&out).unwrap();
    let frames = reader.duration() as u64;
    assert_eq!(
        frames,
        original_len - 10_000,
        "expected output to be original - 10_000 frames"
    );
}

// ---------------------------------------------------------------------------
// Acceptance criterion 4: every mutating tool creates exactly one new node
// parented to the prior head.
// ---------------------------------------------------------------------------

#[test]
fn each_mutating_tool_creates_one_child_node() {
    let (tmp, mut store, mut engine, dispatcher) = fresh();
    let src = write_sine_wav(tmp.path(), "in.wav", 0.5);
    let mut clipboard: Option<Vec<f32>> = None;
    let mut ctx = ToolContext {
        store: &mut store,
        engine: &mut engine,
        user_message: "",
        clipboard: &mut clipboard,
    };

    let h0 = ctx.store.head();
    assert!(h0.is_none(), "fresh store has no head");

    let load = ok(dispatcher
        .invoke("load", json!({ "path": src.to_string_lossy() }), &mut ctx)
        .unwrap());
    let h1 = session::NodeId::from_hex(load["node_id"].as_str().unwrap()).unwrap();
    let n1 = ctx.store.get(h1).unwrap();
    assert_eq!(n1.parent, None);

    let g = ok(dispatcher
        .invoke("gain", json!({ "track": 0, "db": 1.0 }), &mut ctx)
        .unwrap());
    let h2 = session::NodeId::from_hex(g["node_id"].as_str().unwrap()).unwrap();
    let n2 = ctx.store.get(h2).unwrap();
    assert_eq!(n2.parent, Some(h1), "gain must parent to load's head");

    let t = ok(dispatcher
        .invoke(
            "trim",
            json!({ "track": 0, "start_sample": 0, "end_sample": 1000 }),
            &mut ctx,
        )
        .unwrap());
    let h3 = session::NodeId::from_hex(t["node_id"].as_str().unwrap()).unwrap();
    let n3 = ctx.store.get(h3).unwrap();
    assert_eq!(n3.parent, Some(h2), "trim must parent to gain's head");
}

// ---------------------------------------------------------------------------
// Acceptance criterion 5: argument validation produces actionable text.
// ---------------------------------------------------------------------------

#[test]
fn unknown_track_index_returns_actionable_error() {
    let (tmp, mut store, mut engine, dispatcher) = fresh();
    let src = write_sine_wav(tmp.path(), "in.wav", 0.5);
    let mut clipboard: Option<Vec<f32>> = None;
    let mut ctx = ToolContext {
        store: &mut store,
        engine: &mut engine,
        user_message: "",
        clipboard: &mut clipboard,
    };

    ok(dispatcher
        .invoke("load", json!({ "path": src.to_string_lossy() }), &mut ctx)
        .unwrap());

    let msg = err(dispatcher
        .invoke("gain", json!({ "track": 3, "db": 0.0 }), &mut ctx)
        .unwrap());
    assert!(
        msg.contains("track index 3 out of range") && msg.contains("session has 1 track"),
        "actual error: {msg}"
    );
}

#[test]
fn out_of_range_samples_return_actionable_error() {
    let (tmp, mut store, mut engine, dispatcher) = fresh();
    let src = write_sine_wav(tmp.path(), "in.wav", 0.5);
    let mut clipboard: Option<Vec<f32>> = None;
    let mut ctx = ToolContext {
        store: &mut store,
        engine: &mut engine,
        user_message: "",
        clipboard: &mut clipboard,
    };

    let load = ok(dispatcher
        .invoke("load", json!({ "path": src.to_string_lossy() }), &mut ctx)
        .unwrap());
    let len = load["length_samples"].as_u64().unwrap();

    // start >= end
    let msg = err(dispatcher
        .invoke(
            "cut_range",
            json!({ "track": 0, "start_sample": 100, "end_sample": 50 }),
            &mut ctx,
        )
        .unwrap());
    assert!(
        msg.contains("start_sample") && msg.contains("end_sample"),
        "got: {msg}"
    );

    // end > track length
    let msg = err(dispatcher
        .invoke(
            "trim",
            json!({ "track": 0, "start_sample": 0, "end_sample": len + 1 }),
            &mut ctx,
        )
        .unwrap());
    assert!(msg.contains("exceeds track length"), "got: {msg}");
}

#[test]
fn no_session_loaded_returns_clear_error() {
    let (_tmp, mut store, mut engine, dispatcher) = fresh();
    let mut clipboard: Option<Vec<f32>> = None;
    let mut ctx = ToolContext {
        store: &mut store,
        engine: &mut engine,
        user_message: "",
        clipboard: &mut clipboard,
    };

    let msg = err(dispatcher
        .invoke("gain", json!({ "track": 0, "db": 1.0 }), &mut ctx)
        .unwrap());
    assert!(msg.contains("no session loaded"), "got: {msg}");
}

// ---------------------------------------------------------------------------
// Cross-tool sequence: load -> cut_range -> normalize -> render_final.
// ---------------------------------------------------------------------------

#[test]
fn cross_tool_sequence_load_cut_normalize_render() {
    let (tmp, mut store, mut engine, dispatcher) = fresh();
    let src = write_sine_wav(tmp.path(), "in.wav", 0.5);
    let out = tmp.path().join("seq.wav");
    let mut clipboard: Option<Vec<f32>> = None;
    let mut ctx = ToolContext {
        store: &mut store,
        engine: &mut engine,
        user_message: "",
        clipboard: &mut clipboard,
    };

    let load = ok(dispatcher
        .invoke("load", json!({ "path": src.to_string_lossy() }), &mut ctx)
        .unwrap());
    let len = load["length_samples"].as_u64().unwrap();

    ok(dispatcher
        .invoke(
            "cut_range",
            json!({
                "track": 0,
                "start_sample": len - 5000,
                "end_sample": len,
            }),
            &mut ctx,
        )
        .unwrap());

    let norm = ok(dispatcher
        .invoke(
            "normalize",
            json!({ "track": 0, "target_dbfs": -1.0 }),
            &mut ctx,
        )
        .unwrap());
    let final_id = norm["node_id"].as_str().unwrap().to_string();

    ok(dispatcher
        .invoke(
            "render_final",
            json!({
                "node_id": final_id,
                "format": "wav",
                "out_path": out.to_string_lossy(),
            }),
            &mut ctx,
        )
        .unwrap());

    let reader = WavReader::open(&out).unwrap();
    let frames = reader.duration() as u64;
    assert_eq!(frames, len - 5000);

    let samples = read_wav_samples(&out);
    let peak = samples
        .iter()
        .map(|s| s.unsigned_abs() as i32)
        .max()
        .unwrap();
    let peak_dbfs = 20.0 * (peak as f32 / 32_768.0).log10();
    assert!(
        (peak_dbfs - (-1.0)).abs() < 0.1,
        "expected normalized peak near -1 dBFS, got {peak_dbfs}"
    );
}

// ---------------------------------------------------------------------------
// Property test: composing two gains == single sum-of-dB gain.
// ---------------------------------------------------------------------------

#[test]
fn gain_composition_is_additive_in_db() {
    // A handful of representative dB pairs covering the practical range.
    let cases = [
        (0.0_f32, 0.0_f32),
        (3.0, -3.0),
        (-6.02, -6.02),
        (12.0, -7.5),
        (1.5, 4.2),
    ];

    for (a, b) in cases {
        let (tmp1, mut s1, mut e1, d1) = fresh();
        let (tmp2, mut s2, mut e2, d2) = fresh();
        let src1 = write_sine_wav(tmp1.path(), "in.wav", 0.25);
        let src2 = write_sine_wav(tmp2.path(), "in.wav", 0.25);

        // Path A: two consecutive gain calls.
        let last_a = {
            let mut clipboard: Option<Vec<f32>> = None;
            let mut ctx = ToolContext {
                store: &mut s1,
                engine: &mut e1,
                user_message: "",
                clipboard: &mut clipboard,
            };
            ok(d1
                .invoke("load", json!({ "path": src1.to_string_lossy() }), &mut ctx)
                .unwrap());
            ok(d1
                .invoke("gain", json!({ "track": 0, "db": a }), &mut ctx)
                .unwrap());
            let r = ok(d1
                .invoke("gain", json!({ "track": 0, "db": b }), &mut ctx)
                .unwrap());
            r["node_id"].as_str().unwrap().to_string()
        };

        // Path B: one gain call with the summed dB.
        let last_b = {
            let mut clipboard: Option<Vec<f32>> = None;
            let mut ctx = ToolContext {
                store: &mut s2,
                engine: &mut e2,
                user_message: "",
                clipboard: &mut clipboard,
            };
            ok(d2
                .invoke("load", json!({ "path": src2.to_string_lossy() }), &mut ctx)
                .unwrap());
            let r = ok(d2
                .invoke("gain", json!({ "track": 0, "db": a + b }), &mut ctx)
                .unwrap());
            r["node_id"].as_str().unwrap().to_string()
        };

        // Render both and compare gain field of the head state.
        let na = s1.get(session::NodeId::from_hex(&last_a).unwrap()).unwrap();
        let ga = na.state.tracks[0].gain_db;

        let nb = s2.get(session::NodeId::from_hex(&last_b).unwrap()).unwrap();
        let gb = nb.state.tracks[0].gain_db;

        assert!(
            (ga - gb).abs() < 1e-5,
            "for case ({a}, {b}): composed gain {ga} != single {gb}"
        );
    }
}

// ---------------------------------------------------------------------------
// render_preview returns a path and does NOT mutate session head.
// ---------------------------------------------------------------------------

#[test]
fn render_preview_returns_path_without_creating_node() {
    let (tmp, mut store, mut engine, dispatcher) = fresh();
    let src = write_sine_wav(tmp.path(), "in.wav", 0.25);
    let mut clipboard: Option<Vec<f32>> = None;
    let mut ctx = ToolContext {
        store: &mut store,
        engine: &mut engine,
        user_message: "",
        clipboard: &mut clipboard,
    };

    let load = ok(dispatcher
        .invoke("load", json!({ "path": src.to_string_lossy() }), &mut ctx)
        .unwrap());
    let load_id = load["node_id"].as_str().unwrap().to_string();
    let head_before = ctx.store.head();

    let preview = ok(dispatcher
        .invoke("render_preview", json!({ "node_id": load_id }), &mut ctx)
        .unwrap());
    let path = PathBuf::from(preview["path"].as_str().unwrap());
    assert!(
        path.exists(),
        "preview file should exist at {}",
        path.display()
    );

    let head_after = ctx.store.head();
    assert_eq!(
        head_before, head_after,
        "render_preview must not change head"
    );

    // Cleanup the preview tempfile we just kept.
    let _ = std::fs::remove_file(&path);
}

// ---------------------------------------------------------------------------
// render_final reports clear errors for unsupported formats.
// ---------------------------------------------------------------------------

#[test]
fn render_final_rejects_mp3_and_flac_in_phase_1() {
    let (tmp, mut store, mut engine, dispatcher) = fresh();
    let src = write_sine_wav(tmp.path(), "in.wav", 0.25);
    let mut clipboard: Option<Vec<f32>> = None;
    let mut ctx = ToolContext {
        store: &mut store,
        engine: &mut engine,
        user_message: "",
        clipboard: &mut clipboard,
    };
    let load = ok(dispatcher
        .invoke("load", json!({ "path": src.to_string_lossy() }), &mut ctx)
        .unwrap());
    let id = load["node_id"].as_str().unwrap().to_string();
    let out = tmp.path().join("out.bin");

    for fmt in ["mp3", "flac"] {
        let msg = err(dispatcher
            .invoke(
                "render_final",
                json!({
                    "node_id": id,
                    "format": fmt,
                    "out_path": out.to_string_lossy(),
                }),
                &mut ctx,
            )
            .unwrap());
        assert!(msg.contains("not supported in Phase 1"), "got: {msg}");
    }
}

// ---------------------------------------------------------------------------
// default_dispatcher exposes all Phase-1 tools (7 deterministic + transcribe).
// ---------------------------------------------------------------------------

#[test]
fn default_dispatcher_exposes_all_phase1_tools() {
    let dispatcher = ToolDispatcher::default_dispatcher();
    let names: Vec<String> = dispatcher
        .tool_schemas()
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect();
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(
        sorted,
        vec![
            "add_track",
            "align_to_beat",
            "analyze_track",
            "apply_diff",
            "compare_nodes",
            "compressor",
            "copy_region",
            "cut_range",
            "eq",
            "fade",
            "fork_node",
            "gain",
            "insert_silence",
            "label",
            "load",
            "name_node",
            "noise_reduction",
            "normalize",
            "paste_region",
            "pitch_shift",
            "remove_track",
            "rename_track",
            "render_final",
            "render_preview",
            "reverse",
            "revert_to",
            "separate_stems",
            "set_clip_envelope",
            "set_pan",
            "set_track_gain",
            "silence_region",
            "time_stretch",
            "transcribe",
            "trim",
        ]
    );
}

// ---------------------------------------------------------------------------
// M18 — `separate_stems` surfaces the model-missing case as an actionable
// error (rather than panicking), regardless of which model is requested.
// ---------------------------------------------------------------------------

#[test]
fn separate_stems_returns_actionable_error_when_model_missing() {
    // Don't leak DEMUCS_*_MODEL_PATH from the host shell into the test.
    // SAFETY: tests don't run with concurrent threads observing these
    // env vars, and we never spawn subprocesses below.
    unsafe {
        std::env::remove_var("DEMUCS_MODEL_PATH");
        std::env::remove_var("DEMUCS_FT_MODEL_PATH");
    }

    let (tmp, mut store, mut engine, dispatcher) = fresh();
    let src = write_sine_wav(tmp.path(), "in.wav", 0.25);
    let mut clipboard: Option<Vec<f32>> = None;
    let mut ctx = ToolContext {
        store: &mut store,
        engine: &mut engine,
        user_message: "",
        clipboard: &mut clipboard,
    };

    // Default model (htdemucs_ft) → looks at DEMUCS_FT_MODEL_PATH.
    let msg = err(dispatcher
        .invoke(
            "separate_stems",
            json!({ "path": src.to_string_lossy() }),
            &mut ctx,
        )
        .unwrap());
    assert!(
        msg.contains("DEMUCS_FT_MODEL_PATH") && msg.contains("scripts/fetch-models.sh"),
        "expected DEMUCS_FT_MODEL_PATH install hint, got: {msg}"
    );

    // Explicit htdemucs → DEMUCS_MODEL_PATH instead.
    let msg = err(dispatcher
        .invoke(
            "separate_stems",
            json!({ "path": src.to_string_lossy(), "model": "htdemucs" }),
            &mut ctx,
        )
        .unwrap());
    assert!(
        msg.contains("DEMUCS_MODEL_PATH") && msg.contains("scripts/fetch-models.sh"),
        "expected DEMUCS_MODEL_PATH install hint, got: {msg}"
    );
}

#[test]
fn separate_stems_rejects_unknown_model_via_schema() {
    let (tmp, mut store, mut engine, dispatcher) = fresh();
    let src = write_sine_wav(tmp.path(), "in.wav", 0.25);
    let mut clipboard: Option<Vec<f32>> = None;
    let mut ctx = ToolContext {
        store: &mut store,
        engine: &mut engine,
        user_message: "",
        clipboard: &mut clipboard,
    };

    // Schema enforces the enum, so this fails dispatch-time validation
    // (a `DispatchError`, not a `ToolResult::Error`).
    let result = dispatcher.invoke(
        "separate_stems",
        json!({ "path": src.to_string_lossy(), "model": "htdemucs_xl" }),
        &mut ctx,
    );
    let err = result.unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("htdemucs_xl") || msg.contains("enum"),
        "expected schema validation error, got: {msg}"
    );
}

#[test]
fn separate_stems_rejects_missing_input_file() {
    // SAFETY: see the env-var note above.
    unsafe {
        std::env::set_var(
            "DEMUCS_FT_MODEL_PATH",
            "/some/path/that/does/not/exist.onnx",
        );
    }

    let (_tmp, mut store, mut engine, dispatcher) = fresh();
    let mut clipboard: Option<Vec<f32>> = None;
    let mut ctx = ToolContext {
        store: &mut store,
        engine: &mut engine,
        user_message: "",
        clipboard: &mut clipboard,
    };

    let msg = err(dispatcher
        .invoke(
            "separate_stems",
            json!({ "path": "/no/such/audio.wav" }),
            &mut ctx,
        )
        .unwrap());
    assert!(
        msg.contains("file not found") && msg.contains("/no/such/audio.wav"),
        "expected file-not-found error, got: {msg}"
    );

    // Cleanup so other tests in this file aren't affected.
    unsafe {
        std::env::remove_var("DEMUCS_FT_MODEL_PATH");
    }
}

// ---------------------------------------------------------------------------
// M20 — `time_stretch` records its factor on every clip of the targeted
// track. Two consecutive calls compose multiplicatively.
// ---------------------------------------------------------------------------

#[test]
fn time_stretch_records_factor_on_clip() {
    let (tmp, mut store, mut engine, dispatcher) = fresh();
    let src = write_sine_wav(tmp.path(), "in.wav", 0.25);
    let mut clipboard: Option<Vec<f32>> = None;
    let mut ctx = ToolContext {
        store: &mut store,
        engine: &mut engine,
        user_message: "",
        clipboard: &mut clipboard,
    };

    ok(dispatcher
        .invoke("load", json!({ "path": src.to_string_lossy() }), &mut ctx)
        .unwrap());

    let res = ok(dispatcher
        .invoke(
            "time_stretch",
            json!({ "track": 0, "factor": 0.5, "preserve_formants": false }),
            &mut ctx,
        )
        .unwrap());

    let new_id = session::NodeId::from_hex(res["node_id"].as_str().unwrap()).unwrap();
    let node = ctx.store.get(new_id).unwrap();
    let factor = node.state.tracks[0].clips[0].time_stretch_factor;
    assert_eq!(
        factor,
        Some(0.5),
        "expected factor 0.5 on clip, got {factor:?}"
    );
    assert!(res["summary"]
        .as_str()
        .unwrap()
        .contains("applied at next render"));

    // Compose: a second 2.0 should bring us back to identity (1.0).
    let res2 = ok(dispatcher
        .invoke(
            "time_stretch",
            json!({ "track": 0, "factor": 2.0 }),
            &mut ctx,
        )
        .unwrap());
    let id2 = session::NodeId::from_hex(res2["node_id"].as_str().unwrap()).unwrap();
    let node2 = ctx.store.get(id2).unwrap();
    let composed = node2.state.tracks[0].clips[0].time_stretch_factor.unwrap();
    assert!(
        (composed - 1.0).abs() < 1e-5,
        "expected composed factor ~1.0, got {composed}"
    );
}

#[test]
fn time_stretch_rejects_non_positive_factor() {
    let (tmp, mut store, mut engine, dispatcher) = fresh();
    let src = write_sine_wav(tmp.path(), "in.wav", 0.25);
    let mut clipboard: Option<Vec<f32>> = None;
    let mut ctx = ToolContext {
        store: &mut store,
        engine: &mut engine,
        user_message: "",
        clipboard: &mut clipboard,
    };
    ok(dispatcher
        .invoke("load", json!({ "path": src.to_string_lossy() }), &mut ctx)
        .unwrap());

    let msg = err(dispatcher
        .invoke(
            "time_stretch",
            json!({ "track": 0, "factor": 0.0 }),
            &mut ctx,
        )
        .unwrap());
    assert!(msg.contains("invalid factor"), "got: {msg}");
}

// ---------------------------------------------------------------------------
// M20 — `pitch_shift` records semitones on every clip; composes additively.
// ---------------------------------------------------------------------------

#[test]
fn pitch_shift_records_semitones_on_clip() {
    let (tmp, mut store, mut engine, dispatcher) = fresh();
    let src = write_sine_wav(tmp.path(), "in.wav", 0.25);
    let mut clipboard: Option<Vec<f32>> = None;
    let mut ctx = ToolContext {
        store: &mut store,
        engine: &mut engine,
        user_message: "",
        clipboard: &mut clipboard,
    };
    ok(dispatcher
        .invoke("load", json!({ "path": src.to_string_lossy() }), &mut ctx)
        .unwrap());

    let res = ok(dispatcher
        .invoke(
            "pitch_shift",
            json!({ "track": 0, "semitones": 12.0, "preserve_formants": true }),
            &mut ctx,
        )
        .unwrap());
    let id = session::NodeId::from_hex(res["node_id"].as_str().unwrap()).unwrap();
    let node = ctx.store.get(id).unwrap();
    assert_eq!(
        node.state.tracks[0].clips[0].pitch_shift_semitones,
        Some(12.0)
    );

    // Compose +12 then -12 → 0 → stored as `None`.
    let res2 = ok(dispatcher
        .invoke(
            "pitch_shift",
            json!({ "track": 0, "semitones": -12.0 }),
            &mut ctx,
        )
        .unwrap());
    let id2 = session::NodeId::from_hex(res2["node_id"].as_str().unwrap()).unwrap();
    let node2 = ctx.store.get(id2).unwrap();
    assert_eq!(node2.state.tracks[0].clips[0].pitch_shift_semitones, None);
}

#[test]
fn pitch_shift_rejects_out_of_range_semitones() {
    let (tmp, mut store, mut engine, dispatcher) = fresh();
    let src = write_sine_wav(tmp.path(), "in.wav", 0.25);
    let mut clipboard: Option<Vec<f32>> = None;
    let mut ctx = ToolContext {
        store: &mut store,
        engine: &mut engine,
        user_message: "",
        clipboard: &mut clipboard,
    };
    ok(dispatcher
        .invoke("load", json!({ "path": src.to_string_lossy() }), &mut ctx)
        .unwrap());

    let msg = err(dispatcher
        .invoke(
            "pitch_shift",
            json!({ "track": 0, "semitones": 100.0 }),
            &mut ctx,
        )
        .unwrap());
    assert!(msg.contains("invalid semitones"), "got: {msg}");
}

// ---------------------------------------------------------------------------
// M20 — `align_to_beat` records a beat grid on every clip.
// ---------------------------------------------------------------------------

#[test]
fn align_to_beat_records_grid_on_clip() {
    let (tmp, mut store, mut engine, dispatcher) = fresh();
    let src = write_sine_wav(tmp.path(), "in.wav", 0.25);
    let mut clipboard: Option<Vec<f32>> = None;
    let mut ctx = ToolContext {
        store: &mut store,
        engine: &mut engine,
        user_message: "",
        clipboard: &mut clipboard,
    };
    ok(dispatcher
        .invoke("load", json!({ "path": src.to_string_lossy() }), &mut ctx)
        .unwrap());

    let grid = vec![0.0, 0.5, 1.0, 1.5];
    let res = ok(dispatcher
        .invoke(
            "align_to_beat",
            json!({ "track": 0, "beat_grid": grid }),
            &mut ctx,
        )
        .unwrap());
    let id = session::NodeId::from_hex(res["node_id"].as_str().unwrap()).unwrap();
    let node = ctx.store.get(id).unwrap();
    let stored = node.state.tracks[0].clips[0].beat_grid.as_ref().unwrap();
    assert_eq!(stored, &vec![0.0, 0.5, 1.0, 1.5]);
    assert_eq!(res["beats"].as_u64().unwrap(), 4);
}

#[test]
fn align_to_beat_rejects_non_monotonic_grid() {
    let (tmp, mut store, mut engine, dispatcher) = fresh();
    let src = write_sine_wav(tmp.path(), "in.wav", 0.25);
    let mut clipboard: Option<Vec<f32>> = None;
    let mut ctx = ToolContext {
        store: &mut store,
        engine: &mut engine,
        user_message: "",
        clipboard: &mut clipboard,
    };
    ok(dispatcher
        .invoke("load", json!({ "path": src.to_string_lossy() }), &mut ctx)
        .unwrap());

    // Out of order — 0.4 follows 0.5.
    let msg = err(dispatcher
        .invoke(
            "align_to_beat",
            json!({ "track": 0, "beat_grid": [0.0, 0.5, 0.4] }),
            &mut ctx,
        )
        .unwrap());
    assert!(
        msg.contains("strictly increasing"),
        "expected strictly-increasing diagnostic, got: {msg}"
    );
}

#[test]
fn align_to_beat_rejects_empty_grid() {
    let (tmp, mut store, mut engine, dispatcher) = fresh();
    let src = write_sine_wav(tmp.path(), "in.wav", 0.25);
    let mut clipboard: Option<Vec<f32>> = None;
    let mut ctx = ToolContext {
        store: &mut store,
        engine: &mut engine,
        user_message: "",
        clipboard: &mut clipboard,
    };
    ok(dispatcher
        .invoke("load", json!({ "path": src.to_string_lossy() }), &mut ctx)
        .unwrap());

    // Schema's minItems=1 rejects an empty array at validation time, so
    // this surfaces as a `DispatchError`, not a `ToolResult::Error`.
    let r = dispatcher.invoke(
        "align_to_beat",
        json!({ "track": 0, "beat_grid": [] }),
        &mut ctx,
    );
    let err = r.unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("less than 1 item")
            || msg.contains("minItems")
            || msg.contains("shorter than"),
        "expected schema validation error, got: {msg}"
    );
}

// ---------------------------------------------------------------------------
// M21 — multi-track session state.
// ---------------------------------------------------------------------------

/// Two consecutive `load`s append a second track instead of replacing the
/// first.
#[test]
fn load_then_load_appends_track() {
    let (tmp, mut store, mut engine, dispatcher) = fresh();
    let a = write_sine_wav(tmp.path(), "a.wav", 0.25);
    let b = write_sine_wav(tmp.path(), "b.wav", 0.10);

    let mut clipboard: Option<Vec<f32>> = None;
    let mut ctx = ToolContext {
        store: &mut store,
        engine: &mut engine,
        user_message: "",
        clipboard: &mut clipboard,
    };

    let first = ok(dispatcher
        .invoke("load", json!({ "path": a.to_string_lossy() }), &mut ctx)
        .unwrap());
    assert_eq!(first["track_index"].as_u64().unwrap(), 0);

    let second = ok(dispatcher
        .invoke("load", json!({ "path": b.to_string_lossy() }), &mut ctx)
        .unwrap());
    assert_eq!(second["track_index"].as_u64().unwrap(), 1);

    let id = session::NodeId::from_hex(second["node_id"].as_str().unwrap()).unwrap();
    let node = ctx.store.get(id).unwrap();
    assert_eq!(node.state.tracks.len(), 2, "second load must append");
    assert_eq!(
        node.state.tracks[0].clips[0].source_path, a,
        "track 0 must be the first-loaded source"
    );
    assert_eq!(
        node.state.tracks[1].clips[0].source_path, b,
        "track 1 must be the second-loaded source"
    );
}

#[test]
fn add_track_creates_empty_track() {
    let (tmp, mut store, mut engine, dispatcher) = fresh();
    let src = write_sine_wav(tmp.path(), "in.wav", 0.25);
    let mut clipboard: Option<Vec<f32>> = None;
    let mut ctx = ToolContext {
        store: &mut store,
        engine: &mut engine,
        user_message: "",
        clipboard: &mut clipboard,
    };
    ok(dispatcher
        .invoke("load", json!({ "path": src.to_string_lossy() }), &mut ctx)
        .unwrap());

    let res = ok(dispatcher
        .invoke("add_track", json!({ "name": "instrumental" }), &mut ctx)
        .unwrap());
    assert_eq!(res["track_index"].as_u64().unwrap(), 1);

    let id = session::NodeId::from_hex(res["node_id"].as_str().unwrap()).unwrap();
    let node = ctx.store.get(id).unwrap();
    assert_eq!(node.state.tracks.len(), 2);
    assert_eq!(node.state.tracks[1].name, "instrumental");
    assert!(
        node.state.tracks[1].clips.is_empty(),
        "added track must start with no clips"
    );
}

#[test]
fn add_track_without_session_returns_clear_error() {
    let (_tmp, mut store, mut engine, dispatcher) = fresh();
    let mut clipboard: Option<Vec<f32>> = None;
    let mut ctx = ToolContext {
        store: &mut store,
        engine: &mut engine,
        user_message: "",
        clipboard: &mut clipboard,
    };
    let msg = err(dispatcher.invoke("add_track", json!({}), &mut ctx).unwrap());
    assert!(msg.contains("no session loaded"), "got: {msg}");
}

#[test]
fn remove_track_drops_clips_and_shifts_indices() {
    let (tmp, mut store, mut engine, dispatcher) = fresh();
    let a = write_sine_wav(tmp.path(), "a.wav", 0.25);
    let b = write_sine_wav(tmp.path(), "b.wav", 0.10);
    let c = write_sine_wav(tmp.path(), "c.wav", 0.05);

    let mut clipboard: Option<Vec<f32>> = None;
    let mut ctx = ToolContext {
        store: &mut store,
        engine: &mut engine,
        user_message: "",
        clipboard: &mut clipboard,
    };
    ok(dispatcher
        .invoke("load", json!({ "path": a.to_string_lossy() }), &mut ctx)
        .unwrap());
    ok(dispatcher
        .invoke("load", json!({ "path": b.to_string_lossy() }), &mut ctx)
        .unwrap());
    ok(dispatcher
        .invoke("load", json!({ "path": c.to_string_lossy() }), &mut ctx)
        .unwrap());

    // Drop track 1 (the b track). c shifts from index 2 to index 1.
    let res = ok(dispatcher
        .invoke("remove_track", json!({ "track": 1 }), &mut ctx)
        .unwrap());
    assert_eq!(res["track_count"].as_u64().unwrap(), 2);
    assert_eq!(res["removed_track_name"].as_str().unwrap(), "b");

    let id = session::NodeId::from_hex(res["node_id"].as_str().unwrap()).unwrap();
    let node = ctx.store.get(id).unwrap();
    assert_eq!(node.state.tracks.len(), 2);
    assert_eq!(node.state.tracks[0].clips[0].source_path, a);
    assert_eq!(
        node.state.tracks[1].clips[0].source_path, c,
        "c must shift down to index 1"
    );
}

#[test]
fn remove_track_rejects_out_of_range_index() {
    let (tmp, mut store, mut engine, dispatcher) = fresh();
    let src = write_sine_wav(tmp.path(), "in.wav", 0.25);
    let mut clipboard: Option<Vec<f32>> = None;
    let mut ctx = ToolContext {
        store: &mut store,
        engine: &mut engine,
        user_message: "",
        clipboard: &mut clipboard,
    };
    ok(dispatcher
        .invoke("load", json!({ "path": src.to_string_lossy() }), &mut ctx)
        .unwrap());
    let msg = err(dispatcher
        .invoke("remove_track", json!({ "track": 5 }), &mut ctx)
        .unwrap());
    assert!(
        msg.contains("out of range"),
        "expected out-of-range diagnostic, got: {msg}"
    );
}

#[test]
fn set_track_gain_changes_render_amplitude() {
    let (tmp, mut store, mut engine, dispatcher) = fresh();
    let src = write_sine_wav(tmp.path(), "in.wav", 0.25);
    let out_loud = tmp.path().join("loud.wav");
    let out_quiet = tmp.path().join("quiet.wav");

    let mut clipboard: Option<Vec<f32>> = None;
    let mut ctx = ToolContext {
        store: &mut store,
        engine: &mut engine,
        user_message: "",
        clipboard: &mut clipboard,
    };
    ok(dispatcher
        .invoke("load", json!({ "path": src.to_string_lossy() }), &mut ctx)
        .unwrap());

    // First, apply +6 dB and render. set_track_gain is absolute so this
    // sets the track to exactly +6 dB regardless of prior state.
    let six_db = 20.0 * 2f32.log10();
    let r1 = ok(dispatcher
        .invoke(
            "set_track_gain",
            json!({ "track": 0, "db": six_db }),
            &mut ctx,
        )
        .unwrap());
    assert!((r1["track_gain_db"].as_f64().unwrap() as f32 - six_db).abs() < 1e-3);
    let id1 = r1["node_id"].as_str().unwrap().to_string();
    ok(dispatcher
        .invoke(
            "render_final",
            json!({
                "node_id": id1,
                "format": "wav",
                "out_path": out_loud.to_string_lossy(),
            }),
            &mut ctx,
        )
        .unwrap());

    // Now set to -6 dB (absolute, not additive). If this were additive
    // it'd be 0 dB; absolute means the rendered output is half-amplitude.
    let r2 = ok(dispatcher
        .invoke(
            "set_track_gain",
            json!({ "track": 0, "db": -six_db }),
            &mut ctx,
        )
        .unwrap());
    assert!((r2["track_gain_db"].as_f64().unwrap() as f32 + six_db).abs() < 1e-3);
    let id2 = r2["node_id"].as_str().unwrap().to_string();
    ok(dispatcher
        .invoke(
            "render_final",
            json!({
                "node_id": id2,
                "format": "wav",
                "out_path": out_quiet.to_string_lossy(),
            }),
            &mut ctx,
        )
        .unwrap());

    // Loud peak / quiet peak should be ≈ 4× (each step is 2×, set_track
    // is absolute so the second call wipes the first).
    let loud = read_wav_samples(&out_loud);
    let quiet = read_wav_samples(&out_quiet);
    let peak = |s: &[i16]| s.iter().map(|x| x.unsigned_abs() as i32).max().unwrap();
    let p_loud = peak(&loud) as f32;
    let p_quiet = peak(&quiet) as f32;
    let ratio = p_loud / p_quiet;
    assert!(
        (ratio - 4.0).abs() < 0.1,
        "expected loud/quiet ≈ 4.0, got {ratio} (loud={p_loud} quiet={p_quiet})"
    );
}

/// Two-source workflow: `load a; load b; render_final` produces a mix
/// where both tones are audible. End-to-end through the tool dispatcher.
#[test]
fn two_loads_then_render_produces_mixdown() {
    let (tmp, mut store, mut engine, dispatcher) = fresh();
    let a = write_sine_wav(tmp.path(), "a.wav", 0.4);
    let b = write_sine_wav(tmp.path(), "b.wav", 0.4);
    let out = tmp.path().join("mix.wav");

    let mut clipboard: Option<Vec<f32>> = None;
    let mut ctx = ToolContext {
        store: &mut store,
        engine: &mut engine,
        user_message: "",
        clipboard: &mut clipboard,
    };
    ok(dispatcher
        .invoke("load", json!({ "path": a.to_string_lossy() }), &mut ctx)
        .unwrap());
    let r = ok(dispatcher
        .invoke("load", json!({ "path": b.to_string_lossy() }), &mut ctx)
        .unwrap());
    let id = r["node_id"].as_str().unwrap().to_string();

    let report = ok(dispatcher
        .invoke(
            "render_final",
            json!({
                "node_id": id,
                "format": "wav",
                "out_path": out.to_string_lossy(),
            }),
            &mut ctx,
        )
        .unwrap());
    assert_eq!(report["channels"].as_u64().unwrap(), 1);
    assert_eq!(report["sample_rate"].as_u64().unwrap(), SAMPLE_RATE as u64);

    // Two correlated 440 Hz sines at amplitude 0.4 sum to 0.8 peak.
    let samples = read_wav_samples(&out);
    let peak = samples
        .iter()
        .map(|s| s.unsigned_abs() as i32)
        .max()
        .unwrap();
    let amp = peak as f32 / 32_768.0;
    assert!(
        amp > 0.7 && amp < 0.9,
        "expected mixed peak ≈ 0.8, got {amp}"
    );
}

// ---------------------------------------------------------------------------
// M24 — branching DAG ops (fork_node, apply_diff, compare_nodes,
// revert_to, name_node).
// ---------------------------------------------------------------------------

/// `fork_node` defaults to current head, sets head to it, and returns
/// the parent's id.
#[test]
fn fork_node_creates_sibling_at_existing_head() {
    let (tmp, mut store, mut engine, dispatcher) = fresh();
    let src = write_sine_wav(tmp.path(), "in.wav", 0.25);
    let mut clipboard: Option<Vec<f32>> = None;
    let mut ctx = ToolContext {
        store: &mut store,
        engine: &mut engine,
        user_message: "",
        clipboard: &mut clipboard,
    };

    let load = ok(dispatcher
        .invoke("load", json!({ "path": src.to_string_lossy() }), &mut ctx)
        .unwrap());
    let head_hex = load["node_id"].as_str().unwrap().to_string();
    let head_id = session::NodeId::from_hex(&head_hex).unwrap();

    // Move head off via gain.
    let gained = ok(dispatcher
        .invoke("gain", json!({ "track": 0, "db": 1.0 }), &mut ctx)
        .unwrap());
    let _gained_id = gained["node_id"].as_str().unwrap().to_string();
    assert_ne!(ctx.store.head().unwrap(), head_id);

    // Fork back to the original load.
    let res = ok(dispatcher
        .invoke("fork_node", json!({ "from": head_hex }), &mut ctx)
        .unwrap());
    assert_eq!(res["node_id"].as_str().unwrap(), head_id.to_hex());
    assert_eq!(ctx.store.head(), Some(head_id));

    // Now an edit branches off the original head.
    let g2 = ok(dispatcher
        .invoke("gain", json!({ "track": 0, "db": 2.0 }), &mut ctx)
        .unwrap());
    let g2_id = session::NodeId::from_hex(g2["node_id"].as_str().unwrap()).unwrap();
    let n = ctx.store.get(g2_id).unwrap();
    assert_eq!(
        n.parent,
        Some(head_id),
        "edit must branch off the fork point"
    );
}

#[test]
fn fork_node_defaults_to_head() {
    let (tmp, mut store, mut engine, dispatcher) = fresh();
    let src = write_sine_wav(tmp.path(), "in.wav", 0.25);
    let mut clipboard: Option<Vec<f32>> = None;
    let mut ctx = ToolContext {
        store: &mut store,
        engine: &mut engine,
        user_message: "",
        clipboard: &mut clipboard,
    };
    let load = ok(dispatcher
        .invoke("load", json!({ "path": src.to_string_lossy() }), &mut ctx)
        .unwrap());
    let head = load["node_id"].as_str().unwrap();
    let r = ok(dispatcher.invoke("fork_node", json!({}), &mut ctx).unwrap());
    assert_eq!(r["node_id"].as_str().unwrap(), head);
}

#[test]
fn fork_node_without_session_returns_error() {
    let (_tmp, mut store, mut engine, dispatcher) = fresh();
    let mut clipboard: Option<Vec<f32>> = None;
    let mut ctx = ToolContext {
        store: &mut store,
        engine: &mut engine,
        user_message: "",
        clipboard: &mut clipboard,
    };
    let msg = err(dispatcher.invoke("fork_node", json!({}), &mut ctx).unwrap());
    assert!(msg.contains("no session loaded"), "got: {msg}");
}

/// `apply_diff` with three branch specs produces three sibling nodes
/// off the parent, with deterministic ids and all written to disk
/// (transactional: head moves only after every branch is durable).
#[test]
fn apply_diff_with_three_branches_creates_three_nodes() {
    let (tmp, mut store, mut engine, dispatcher) = fresh();
    let src = write_sine_wav(tmp.path(), "in.wav", 0.25);
    let mut clipboard: Option<Vec<f32>> = None;
    let mut ctx = ToolContext {
        store: &mut store,
        engine: &mut engine,
        user_message: "",
        clipboard: &mut clipboard,
    };
    let load = ok(dispatcher
        .invoke("load", json!({ "path": src.to_string_lossy() }), &mut ctx)
        .unwrap());
    let parent_hex = load["node_id"].as_str().unwrap().to_string();
    let parent_id = session::NodeId::from_hex(&parent_hex).unwrap();
    let parent_state = ctx.store.get(parent_id).unwrap().state;
    let track_id = parent_state.tracks[0].id.clone();
    let track_id_value = serde_json::to_value(&track_id).unwrap();

    // Three "track gain" diffs at different dB.
    let make_diff = |db: f32| -> serde_json::Value {
        json!({
            "added": [],
            "removed": [],
            "modified": [
                [
                    {
                        "op": "track_gain",
                        "track_id": track_id_value,
                        "value": parent_state.tracks[0].gain_db,
                    },
                    {
                        "op": "track_gain",
                        "track_id": track_id_value,
                        "value": db,
                    }
                ]
            ]
        })
    };

    // Three distinct, non-zero gains so each branch produces a distinct
    // content hash off the parent (a 0.0 diff would collide with parent).
    let res = ok(dispatcher
        .invoke(
            "apply_diff",
            json!({
                "from_node": parent_hex,
                "branches": [
                    { "ops": make_diff(-6.0), "label": "quiet" },
                    { "ops": make_diff(-3.0), "label": "neutral" },
                    { "ops": make_diff(6.0), "label": "loud" },
                ]
            }),
            &mut ctx,
        )
        .unwrap());

    let branches = res["branches"].as_array().unwrap();
    assert_eq!(branches.len(), 3, "expected 3 branches");
    let ids: Vec<session::NodeId> = branches
        .iter()
        .map(|v| session::NodeId::from_hex(v.as_str().unwrap()).unwrap())
        .collect();

    // All three ids are distinct (different gains -> different state -> different hashes).
    assert_ne!(ids[0], ids[1]);
    assert_ne!(ids[1], ids[2]);
    assert_ne!(ids[0], ids[2]);

    // All three nodes parent off the original parent and exist on disk.
    for (i, id) in ids.iter().enumerate() {
        let n = ctx.store.get(*id).unwrap();
        assert_eq!(n.parent, Some(parent_id), "branch {i} parent mismatch");
    }

    // Head should now be the LAST branch (mirrors `append_branches`
    // semantics; documented in the store).
    assert_eq!(ctx.store.head(), Some(*ids.last().unwrap()));
}

#[test]
fn apply_diff_rejects_unknown_parent() {
    let (_tmp, mut store, mut engine, dispatcher) = fresh();
    let mut clipboard: Option<Vec<f32>> = None;
    let mut ctx = ToolContext {
        store: &mut store,
        engine: &mut engine,
        user_message: "",
        clipboard: &mut clipboard,
    };
    let bogus = "0".repeat(64);
    let msg = err(dispatcher
        .invoke(
            "apply_diff",
            json!({
                "from_node": bogus,
                "branches": [
                    { "ops": { "added": [], "removed": [], "modified": [] } }
                ]
            }),
            &mut ctx,
        )
        .unwrap());
    assert!(msg.contains("parent not found"), "got: {msg}");
}

/// `compare_nodes` returns a SessionDiff with at least one modified op
/// when the two nodes differ.
#[test]
fn compare_nodes_returns_serialised_diff() {
    let (tmp, mut store, mut engine, dispatcher) = fresh();
    let src = write_sine_wav(tmp.path(), "in.wav", 0.25);
    let mut clipboard: Option<Vec<f32>> = None;
    let mut ctx = ToolContext {
        store: &mut store,
        engine: &mut engine,
        user_message: "",
        clipboard: &mut clipboard,
    };
    let load = ok(dispatcher
        .invoke("load", json!({ "path": src.to_string_lossy() }), &mut ctx)
        .unwrap());
    let a = load["node_id"].as_str().unwrap().to_string();

    let g = ok(dispatcher
        .invoke("gain", json!({ "track": 0, "db": -3.0 }), &mut ctx)
        .unwrap());
    let b = g["node_id"].as_str().unwrap().to_string();

    let res = ok(dispatcher
        .invoke("compare_nodes", json!({ "a": a, "b": b }), &mut ctx)
        .unwrap());
    let diff = &res["diff"];
    assert!(diff.is_object());
    let modified = diff["modified"].as_array().unwrap();
    assert!(!modified.is_empty(), "expected at least one modified op");
    let summary = res["summary"].as_str().unwrap();
    assert!(summary.contains("modified"), "summary: {summary}");
}

/// `revert_to` moves head to the target state. Content addressing
/// makes the new node id == target id when state is byte-equal.
#[test]
fn revert_to_moves_head_back() {
    let (tmp, mut store, mut engine, dispatcher) = fresh();
    let src = write_sine_wav(tmp.path(), "in.wav", 0.25);
    let mut clipboard: Option<Vec<f32>> = None;
    let mut ctx = ToolContext {
        store: &mut store,
        engine: &mut engine,
        user_message: "",
        clipboard: &mut clipboard,
    };
    let load = ok(dispatcher
        .invoke("load", json!({ "path": src.to_string_lossy() }), &mut ctx)
        .unwrap());
    let v0 = load["node_id"].as_str().unwrap().to_string();

    ok(dispatcher
        .invoke("gain", json!({ "track": 0, "db": -3.0 }), &mut ctx)
        .unwrap());
    ok(dispatcher
        .invoke("gain", json!({ "track": 0, "db": -3.0 }), &mut ctx)
        .unwrap());

    let res = ok(dispatcher
        .invoke("revert_to", json!({ "target": v0 }), &mut ctx)
        .unwrap());
    let new_head = res["node_id"].as_str().unwrap().to_string();
    assert_eq!(new_head, v0, "byte-equal state -> same content hash");
    let head = ctx.store.head().unwrap();
    assert_eq!(head.to_hex(), v0);
}

/// `name_node` overwrites an existing label without changing the node id.
#[test]
fn name_node_overwrites_existing_label() {
    let (tmp, mut store, mut engine, dispatcher) = fresh();
    let src = write_sine_wav(tmp.path(), "in.wav", 0.25);
    let mut clipboard: Option<Vec<f32>> = None;
    let mut ctx = ToolContext {
        store: &mut store,
        engine: &mut engine,
        user_message: "",
        clipboard: &mut clipboard,
    };
    let load = ok(dispatcher
        .invoke("load", json!({ "path": src.to_string_lossy() }), &mut ctx)
        .unwrap());
    let id_hex = load["node_id"].as_str().unwrap().to_string();
    let id = session::NodeId::from_hex(&id_hex).unwrap();

    let original_label = ctx.store.get(id).unwrap().label.clone();
    assert!(original_label.is_some(), "load tool should set a label");

    // Overwrite with a custom label.
    let res = ok(dispatcher
        .invoke(
            "name_node",
            json!({ "node_id": id_hex, "label": "v1 candidate" }),
            &mut ctx,
        )
        .unwrap());
    assert_eq!(res["label"].as_str().unwrap(), "v1 candidate");

    // Reading the node from disk must reflect the new label and SAME id.
    let n = ctx.store.get(id).unwrap();
    assert_eq!(n.label.as_deref(), Some("v1 candidate"));
    assert_eq!(n.id, id, "name_node must not change content hash");

    // Empty string clears the label.
    ok(dispatcher
        .invoke(
            "name_node",
            json!({ "node_id": id_hex, "label": "" }),
            &mut ctx,
        )
        .unwrap());
    let n = ctx.store.get(id).unwrap();
    assert!(n.label.is_none(), "empty string should clear label");
}
