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

/// An interior cut splits the track into two clips, and both must survive
/// into the render.
///
/// The tail-cut test above sidesteps this on purpose — it cuts at the end
/// so a single clip is left. An interior cut is where the render graph used
/// to read `clips.first()` and drop everything after the cut point: the
/// render came back the length of the *head* alone, with the rest of the
/// track silently gone.
#[test]
fn interior_cut_range_keeps_both_halves_in_the_render() {
    let (tmp, mut store, mut engine, dispatcher) = fresh();
    let src = write_sine_wav(tmp.path(), "in.wav", 0.5);
    let out = tmp.path().join("interior-cut.wav");

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

    // Cut a chunk out of the middle, leaving audio on both sides of it.
    let cut_start = original_len / 4;
    let cut_end = cut_start + 10_000;
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
        "an interior cut must shorten the render by exactly the cut, \
         not truncate it to the first clip"
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

/// Updated deliberately: `flac` used to be rejected here, and this test
/// pinned that. It now works (#92).
///
/// What replaces it is the other half of the ticket — the schema must
/// offer only what succeeds. `mp3` is gone from the enum, so a request
/// for it is refused by schema validation and the model is told which
/// formats exist, rather than being invited to pick one that returns an
/// error naming a milestone it cannot act on.
/// `bitrate_kbps` is mp3's alone. Accepting it silently on a lossless
/// format would tell the caller their setting took effect when nothing
/// read it.
#[test]
fn render_final_rejects_a_bitrate_on_a_lossless_format() {
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
    let out = tmp.path().join("out.flac");

    let msg = err(dispatcher
        .invoke(
            "render_final",
            json!({
                "node_id": id,
                "format": "flac",
                "out_path": out.to_string_lossy(),
                "bitrate_kbps": 192,
            }),
            &mut ctx,
        )
        .unwrap());
    assert!(msg.contains("mp3 only"), "got: {msg}");
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
            "add_effect",
            "add_track",
            "align_to_beat",
            "analyze_track",
            "apply_diff",
            "apply_recipe",
            "audition_effect",
            "batch_apply",
            "change_speed",
            "click_removal",
            "compact_session",
            "compare_nodes",
            "compressor",
            "copy_region",
            "create_bus",
            "cut_range",
            "cut_words",
            "de_esser",
            "distortion",
            "duck_under_speech",
            "duplicate_track",
            "echo",
            "eq",
            "export_labels",
            "export_multiple",
            "export_recipe",
            "fade",
            "fork_node",
            "gain",
            "generate_noise",
            "generate_tone",
            "high_pass_filter",
            "import_labels",
            "insert_silence",
            "invert",
            "label",
            "leveler",
            "limiter",
            "load",
            "low_pass_filter",
            "mix_to_new_track",
            "mono_to_stereo",
            "move_clip",
            "mute_track",
            "name_node",
            "noise_gate",
            "noise_reduction",
            "normalize",
            "normalize_loudness",
            "notch_filter",
            "paste_region",
            "phaser",
            "pitch_shift",
            "plot_spectrum",
            "punch_in",
            "remove_clip",
            "remove_effect",
            "remove_fillers",
            "remove_send",
            "remove_track",
            "rename_track",
            "render_final",
            "render_preview",
            "reorder_effects",
            "repeat_selection",
            "resample_track",
            "reverb",
            "reverse",
            "revert_to",
            "separate_stems",
            "set_clip_envelope",
            "set_effect_bypassed",
            "set_effect_params",
            "set_pan",
            "set_send",
            "set_sync_lock",
            "set_track_gain",
            "silence_finder",
            "silence_region",
            "solo_track",
            "split_clip",
            "stereo_to_mono",
            "stereo_widener",
            "storage_report",
            "time_shift",
            "time_stretch",
            "transcribe",
            "tremolo",
            "trim",
            "truncate_silence",
            "vocal_reduction",
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

/// `time_stretch` changes the duration and leaves the pitch alone.
///
/// It used to record a factor on the clip and change nothing; the
/// assertion was on the recorded number. What matters now is the audio,
/// so the length of the rendered clip is what gets checked.
#[test]
fn time_stretch_changes_the_duration() {
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
    let original = load["length_samples"].as_u64().unwrap();

    // factor 0.5 is half speed, so twice as long.
    let res = ok(dispatcher
        .invoke(
            "time_stretch",
            json!({ "track": 0, "factor": 0.5, "preserve_formants": false }),
            &mut ctx,
        )
        .unwrap());
    let id = session::NodeId::from_hex(res["node_id"].as_str().unwrap()).unwrap();
    let stretched = ctx.store.get(id).unwrap().state.tracks[0].clips[0].length;
    assert_eq!(
        stretched,
        original * 2,
        "factor 0.5 should double the clip's length"
    );

    // Composition comes from the audio now, not from multiplying a
    // recorded number: stretching the already-stretched track by 2.0
    // brings the duration back.
    let res2 = ok(dispatcher
        .invoke(
            "time_stretch",
            json!({ "track": 0, "factor": 2.0 }),
            &mut ctx,
        )
        .unwrap());
    let id2 = session::NodeId::from_hex(res2["node_id"].as_str().unwrap()).unwrap();
    let back = ctx.store.get(id2).unwrap().state.tracks[0].clips[0].length;
    assert_eq!(back, original, "0.5 then 2.0 should return to the original");
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

/// `pitch_shift` moves the pitch and leaves the duration alone.
///
/// The duration is the assertion that separates this from
/// `change_speed`, which can only raise pitch by shortening the audio.
#[test]
fn pitch_shift_keeps_the_duration() {
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
    let original = load["length_samples"].as_u64().unwrap();

    let res = ok(dispatcher
        .invoke(
            "pitch_shift",
            json!({ "track": 0, "semitones": 12.0, "preserve_formants": false }),
            &mut ctx,
        )
        .unwrap());
    let id = session::NodeId::from_hex(res["node_id"].as_str().unwrap()).unwrap();
    let shifted = ctx.store.get(id).unwrap().state.tracks[0].clips[0].length;
    assert_eq!(shifted, original, "an octave up must not change the length");
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
// `align_to_beat` warps the audio (#97)
// ---------------------------------------------------------------------------

/// It used to record a grid and change nothing — `applied_at_render:
/// false`, and a description telling the model to say so. This asserts
/// the audio actually moved, which is the whole ticket.
#[test]
fn align_to_beat_changes_the_audio() {
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

    let before = ctx.store.head().unwrap();
    let before_path = ctx.store.get(before).unwrap().state.tracks[0].clips[0]
        .source_path
        .clone();

    // Half-second beats pulled out to two-thirds of a second: the track
    // gets longer.
    let res = ok(dispatcher
        .invoke(
            "align_to_beat",
            json!({
                "track": 0,
                "source_beats": [0.1, 0.35, 0.6, 0.85],
                "beat_grid": [0.1, 0.45, 0.8, 1.15],
            }),
            &mut ctx,
        )
        .unwrap());

    let id = session::NodeId::from_hex(res["node_id"].as_str().unwrap()).unwrap();
    let after_path = ctx.store.get(id).unwrap().state.tracks[0].clips[0]
        .source_path
        .clone();
    assert_ne!(
        before_path, after_path,
        "a warp must write new audio, not annotate the old file"
    );

    let original = audio_decoder::decode_file(&before_path).expect("decode before");
    let warped = audio_decoder::decode_file(&after_path).expect("decode after");
    assert!(
        warped.samples.len() > original.samples.len(),
        "stretching the beats apart should lengthen the audio: {} -> {}",
        original.samples.len(),
        warped.samples.len()
    );
}

/// No stale "not applied" warning anywhere: the reason
/// `unapplied_clip_metadata.rs` existed was this tool, and with it
/// warping there is nothing left in the repo that reports a change it
/// does not make.
#[test]
fn no_tool_still_claims_it_is_unapplied() {
    let dispatcher = ToolDispatcher::default_dispatcher();
    let schemas = dispatcher.tool_schemas();
    for tool in schemas.as_array().unwrap() {
        let desc = tool["description"].as_str().unwrap_or("");
        let name = tool["name"].as_str().unwrap_or("?");
        assert!(
            !desc.contains("NOT YET APPLIED"),
            "{name} still carries the unapplied-metadata warning"
        );
        assert!(
            !desc.contains("does not read it"),
            "{name} still describes a value nothing reads"
        );
    }
}

#[test]
fn align_to_beat_rejects_a_non_monotonic_grid() {
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
            "align_to_beat",
            json!({
                "track": 0,
                "source_beats": [0.0, 0.5, 1.0],
                "beat_grid": [0.0, 0.5, 0.4],
            }),
            &mut ctx,
        )
        .unwrap());
    assert!(
        msg.contains("strictly increasing"),
        "expected strictly-increasing diagnostic, got: {msg}"
    );
}

/// Mismatched grids are refused rather than truncated. Silently warping
/// the first eight beats of a twelve-beat request is not something a
/// caller can notice.
#[test]
fn align_to_beat_refuses_mismatched_grids() {
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
            "align_to_beat",
            json!({
                "track": 0,
                "source_beats": [0.0, 0.5, 1.0],
                "beat_grid": [0.0, 0.5],
            }),
            &mut ctx,
        )
        .unwrap());
    assert!(msg.contains("same number of beats"), "got: {msg}");
    assert!(
        msg.contains("silently drop"),
        "the error should say why: {msg}"
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

/// `render_final` offered three formats and implemented one. The other
/// two errored the moment they were chosen — a schema promising
/// something the implementation refuses, the same shape as the inert
/// tools fixed in #85, except here it is the output of the whole
/// application.
#[test]
fn render_final_writes_a_flac_that_decodes() {
    let tmp = TempDir::new().expect("tempdir");
    let mut store = session::Store::open(tmp.path()).expect("open store");
    let mut engine = audio_engine::Engine::new();
    let dispatcher = ToolDispatcher::default_dispatcher();
    let src = write_sine_wav(tmp.path(), "in.wav", 0.6);
    let flac_out = tmp.path().join("out.flac");
    let wav_out = tmp.path().join("out.wav");

    let mut clipboard: Option<Vec<f32>> = None;
    let mut ctx = ToolContext {
        store: &mut store,
        engine: &mut engine,
        user_message: "",
        clipboard: &mut clipboard,
    };

    let loaded = ok(dispatcher
        .invoke("load", json!({ "path": src.to_string_lossy() }), &mut ctx)
        .unwrap());
    let node_id = loaded["node_id"].as_str().unwrap().to_string();

    for (format, path) in [("wav", &wav_out), ("flac", &flac_out)] {
        let res = ok(dispatcher
            .invoke(
                "render_final",
                json!({
                    "node_id": node_id,
                    "format": format,
                    "out_path": path.to_string_lossy(),
                }),
                &mut ctx,
            )
            .unwrap());
        assert_eq!(res["format"], json!(format));
        assert!(path.exists(), "{format} render produced no file");
    }

    // Lossless: the exported FLAC has to be the same master as the WAV,
    // not merely a similar one.
    let from_wav = audio_decoder::decode_file(&wav_out).expect("decode wav");
    let from_flac = audio_decoder::decode_file(&flac_out).expect("decode flac");
    assert_eq!(from_flac.samples, from_wav.samples);
    assert!(
        std::fs::metadata(&flac_out).unwrap().len() < std::fs::metadata(&wav_out).unwrap().len(),
        "flac should be smaller than wav"
    );
}

/// The schema must offer only what works — checked by *running* every
/// format the enum advertises, not by comparing it to a list.
///
/// The literal-comparison version of this test had to be edited to add
/// mp3, which means it was pinning my opinion of the enum rather than
/// the property. This version fails when a format is advertised and
/// broken, and passes when one is added and works.
#[test]
fn render_final_advertises_only_formats_it_supports() {
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

    let schemas = dispatcher.tool_schemas();
    let schema = schemas
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "render_final")
        .expect("render_final is registered")
        .clone();
    let formats: Vec<String> = schema["input_schema"]["properties"]["format"]["enum"]
        .as_array()
        .expect("format enum")
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert!(
        formats.contains(&"wav".to_string()),
        "wav must always be offered: {formats:?}"
    );

    for fmt in &formats {
        let out = tmp.path().join(format!("out.{fmt}"));
        let res = ok(dispatcher
            .invoke(
                "render_final",
                json!({
                    "node_id": id,
                    "format": fmt,
                    "out_path": out.to_string_lossy(),
                }),
                &mut ctx,
            )
            .unwrap_or_else(|e| panic!("{fmt} is advertised but the call failed: {e}")));
        assert_eq!(res["format"], json!(fmt));
        let len = std::fs::metadata(&out)
            .unwrap_or_else(|e| panic!("{fmt} produced no file: {e}"))
            .len();
        assert!(len > 0, "{fmt} produced an empty file");
    }
}

/// Buses were in the session schema since Phase 1 with no tool able to
/// create one and no field able to feed one. The engine half is tested
/// in `audio-engine`; this is the half that matters for #108's lesson —
/// that an agent can actually reach it.
#[test]
fn an_agent_can_create_a_bus_and_route_a_track_to_it() {
    let (tmp, mut store, mut engine, dispatcher) = fresh();
    let src = write_sine_wav(tmp.path(), "in.wav", 0.2);
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

    let created = ok(dispatcher
        .invoke("create_bus", json!({ "name": "Reverb" }), &mut ctx)
        .unwrap());
    let bus_id = created["bus_id"].as_str().expect("bus_id returned");

    let routed = ok(dispatcher
        .invoke(
            "set_send",
            json!({ "track": 0, "bus_id": bus_id, "level_db": -6.0 }),
            &mut ctx,
        )
        .unwrap());
    assert_eq!(routed["level_db"], json!(-6.0));

    // The send has to survive into the rendered mix, not just the state.
    let dry_out = tmp.path().join("dry.wav");
    let wet_out = tmp.path().join("wet.wav");
    let node = routed["node_id"].as_str().unwrap().to_string();
    ok(dispatcher
        .invoke(
            "render_final",
            json!({ "node_id": node, "format": "wav", "out_path": wet_out.to_string_lossy() }),
            &mut ctx,
        )
        .unwrap());

    let removed = ok(dispatcher
        .invoke(
            "remove_send",
            json!({ "track": 0, "bus_id": bus_id }),
            &mut ctx,
        )
        .unwrap());
    ok(dispatcher
        .invoke(
            "render_final",
            json!({
                "node_id": removed["node_id"].as_str().unwrap(),
                "format": "wav",
                "out_path": dry_out.to_string_lossy(),
            }),
            &mut ctx,
        )
        .unwrap());

    let wet = read_wav_samples(&wet_out);
    let dry = read_wav_samples(&dry_out);
    let energy = |x: &[i16]| x.iter().map(|v| (*v as f64).abs()).sum::<f64>();
    assert!(
        energy(&wet) > energy(&dry) * 1.2,
        "routing a track to a bus should add a parallel copy; wet and dry \
         are too close, so the send never reached the mix"
    );
}

/// A send to a bus that does not exist is a mistake worth naming at the
/// point it is made, rather than only when the render later refuses.
#[test]
fn set_send_rejects_an_unknown_bus_and_says_what_exists() {
    let (tmp, mut store, mut engine, dispatcher) = fresh();
    let src = write_sine_wav(tmp.path(), "in.wav", 0.2);
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
            "set_send",
            json!({
                "track": 0,
                "bus_id": "00000000-0000-4000-8000-000000000000",
                "level_db": 0.0,
            }),
            &mut ctx,
        )
        .unwrap());
    assert!(
        msg.contains("create_bus"),
        "with no buses yet, the error should say how to make one: {msg}"
    );
}

// ---------------------------------------------------------------------------
// normalize_loudness — LUFS targeting, and the clipping decision
// ---------------------------------------------------------------------------

/// Build a session from a sine at `amp` and run `normalize_loudness`.
fn normalize_to_lufs(amp: f32, target: f32) -> (Value, PathBuf, TempDir) {
    let tmp = TempDir::new().expect("tempdir");
    let mut store = session::Store::open(tmp.path()).expect("open store");
    let mut engine = audio_engine::Engine::new();
    let dispatcher = ToolDispatcher::default_dispatcher();
    let src = write_sine_wav(tmp.path(), "in.wav", amp);
    let out = tmp.path().join("out.wav");

    let mut clipboard: Option<Vec<f32>> = None;
    let res = {
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
                "normalize_loudness",
                json!({ "track": 0, "target_lufs": target }),
                &mut ctx,
            )
            .unwrap());
        ok(dispatcher
            .invoke(
                "render_final",
                json!({
                    "node_id": res["node_id"].as_str().unwrap(),
                    "format": "wav",
                    "out_path": out.to_string_lossy(),
                }),
                &mut ctx,
            )
            .unwrap());
        res
    };
    (res, out, tmp)
}

/// Run `normalize_loudness` with arbitrary args on a fresh session.
///
/// Returns the dispatcher's own result, because a bad preset is caught
/// by the schema *before* the tool runs — and that is the better
/// failure. Tests need to see both layers.
fn normalize_with(args: Value) -> (std::result::Result<ToolResult, String>, TempDir) {
    let tmp = TempDir::new().expect("tempdir");
    let mut store = session::Store::open(tmp.path()).expect("open store");
    let mut engine = audio_engine::Engine::new();
    let dispatcher = ToolDispatcher::default_dispatcher();
    let src = write_sine_wav(tmp.path(), "in.wav", 0.02);

    let mut clipboard: Option<Vec<f32>> = None;
    let res = {
        let mut ctx = ToolContext {
            store: &mut store,
            engine: &mut engine,
            user_message: "",
            clipboard: &mut clipboard,
        };
        ok(dispatcher
            .invoke("load", json!({ "path": src.to_string_lossy() }), &mut ctx)
            .unwrap());
        dispatcher
            .invoke("normalize_loudness", args, &mut ctx)
            .map_err(|e| e.to_string())
    };
    (res, tmp)
}

/// Whatever the call said went wrong, from either layer.
fn why_it_failed(res: std::result::Result<ToolResult, String>) -> String {
    match res {
        Err(dispatch) => dispatch,
        Ok(ToolResult::Error(msg)) => msg,
        Ok(ToolResult::Ok(v)) => panic!("expected a failure, got {v}"),
    }
}

/// A delivery target by name, so nobody has to remember that Apple
/// Podcasts is -16 (#169). The number the preset resolves to is the
/// claim worth pinning — a preset that quietly meant something else
/// would be worse than no preset.
#[test]
fn a_preset_resolves_to_its_platform_target() {
    for (preset, expected) in [
        ("spotify", -14.0),
        ("youtube", -14.0),
        ("apple_podcasts", -16.0),
        ("broadcast", -23.0),
    ] {
        let (res, _tmp) = normalize_with(json!({ "track": 0, "preset": preset }));
        let v = ok(res.expect("a known preset passes the schema"));
        assert_eq!(
            v["target_lufs"].as_f64().unwrap(),
            expected,
            "preset {preset} resolved to the wrong target"
        );
        assert!(
            v["preset"].as_str().is_some(),
            "the result should name the preset it used: {v}"
        );
    }
}

/// An unknown platform names the ones that exist rather than failing
/// blank — the whole point is not having to know the numbers. The
/// schema catches it first, which is the better place: the caller is
/// told before any audio is measured.
#[test]
fn an_unknown_preset_lists_the_known_ones() {
    let (res, _tmp) = normalize_with(json!({ "track": 0, "preset": "tidal" }));
    let msg = why_it_failed(res);
    assert!(msg.contains("tidal"), "should name what was asked: {msg}");
    assert!(msg.contains("spotify"), "should list the known ones: {msg}");
}

/// A number and a name together is a mistake, not a preference.
#[test]
fn a_preset_and_a_number_together_is_refused() {
    let (res, _tmp) =
        normalize_with(json!({ "track": 0, "preset": "spotify", "target_lufs": -23.0 }));
    let msg = why_it_failed(res);
    assert!(msg.contains("not both"), "got {msg}");
}

/// Neither is also refused, and the refusal is a menu.
#[test]
fn no_target_at_all_lists_the_presets() {
    let (res, _tmp) = normalize_with(json!({ "track": 0 }));
    let msg = why_it_failed(res);
    assert!(msg.contains("broadcast"), "got {msg}");
}

/// The custom value still works — presets are an addition, not a
/// replacement.
#[test]
fn an_explicit_target_is_still_accepted() {
    let (res, _tmp) = normalize_with(json!({ "track": 0, "target_lufs": -18.5 }));
    let v = ok(res.expect("a plain number needs no preset"));
    assert_eq!(v["target_lufs"].as_f64().unwrap(), -18.5);
    assert!(v["preset"].is_null(), "no preset was used: {v}");
}

/// The acceptance criterion: a quiet source lands on the target after
/// render, measured with the same EBU R128 implementation.
#[test]
fn a_quiet_track_reaches_its_lufs_target_after_render() {
    // Quiet enough that the required boost has headroom under -1 dBFS.
    let (res, out, _tmp) = normalize_to_lufs(0.02, -20.0);
    assert_eq!(
        res["capped_by_ceiling"],
        json!(false),
        "this fixture should have headroom: {}",
        res["summary"]
    );

    let decoded = audio_decoder::decode_file(&out).expect("decode render");
    let achieved = audio_analysis::loudness::integrated_lufs(
        &decoded.samples,
        decoded.sample_rate,
        decoded.channels,
    )
    .expect("measure render");

    assert!(
        (achieved - (-20.0)).abs() < 0.5,
        "rendered loudness {achieved:.2} LUFS is not within 0.5 of the -20 target"
    );
}

/// The decision this ticket exists to make explicitly. Gain enough to
/// hit a loud target on already-loud material would clip; the tool caps
/// instead and says by how much it fell short, rather than reporting
/// success while wrecking the audio.
#[test]
fn a_target_that_would_clip_is_capped_and_reports_the_shortfall() {
    // Near full scale already: peak is -0.9 dBFS, so the -1 dBFS
    // ceiling leaves -0.1 dB of headroom. A sine at this amplitude
    // measures about -4.6 LUFS, so asking for -3 needs roughly +1.6 dB
    // that it cannot have.
    let (res, out, _tmp) = normalize_to_lufs(0.9, -3.0);

    assert_eq!(
        res["capped_by_ceiling"],
        json!(true),
        "expected the ceiling to bite: {}",
        res["summary"]
    );
    let shortfall = res["shortfall_db"].as_f64().expect("shortfall reported");
    assert!(
        shortfall > 0.0,
        "a capped normalise must report a positive shortfall, got {shortfall}"
    );
    assert!(
        res["applied_gain_db"].as_f64().unwrap() < res["requested_gain_db"].as_f64().unwrap(),
        "applied gain should be less than requested when capped"
    );

    // And the audio must actually stay under the ceiling.
    let pcm = read_wav_samples(&out);
    let peak = pcm.iter().fold(0i16, |m, v| m.max(v.abs())) as f32 / 32_768.0;
    let peak_dbfs = 20.0 * peak.log10();
    assert!(
        peak_dbfs <= -0.9,
        "peak {peak_dbfs:.2} dBFS exceeded the -1 dBFS ceiling"
    );
}

/// Loudness must be measured across the whole timeline. Reading
/// `clips[0]`'s source file measures audio a cut removed, so the gain
/// would be set from material the listener never hears.
#[test]
fn loudness_is_measured_across_a_split_track() {
    let tmp = TempDir::new().expect("tempdir");
    let mut store = session::Store::open(tmp.path()).expect("open store");
    let mut engine = audio_engine::Engine::new();
    let dispatcher = ToolDispatcher::default_dispatcher();
    let src = write_sine_wav(tmp.path(), "in.wav", 0.05);
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
    ok(dispatcher
        .invoke(
            "cut_range",
            json!({ "track": 0, "start_sample": 2000, "end_sample": 3000 }),
            &mut ctx,
        )
        .unwrap());

    let res = ok(dispatcher
        .invoke(
            "normalize_loudness",
            json!({ "track": 0, "target_lufs": -20.0 }),
            &mut ctx,
        )
        .unwrap());
    assert!(
        res["measured_lufs"].as_f64().unwrap() > -70.0,
        "a split track must still measure; got {}",
        res["measured_lufs"]
    );
}

#[test]
fn normalize_loudness_rejects_absurd_targets() {
    let (tmp, mut store, mut engine, dispatcher) = fresh();
    let src = write_sine_wav(tmp.path(), "in.wav", 0.2);
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
            "normalize_loudness",
            json!({ "track": 0, "target_lufs": 6.0 }),
            &mut ctx,
        )
        .unwrap());
    assert!(
        msg.contains("-14") || msg.contains("<= 0"),
        "a positive target should be rejected with guidance: {msg}"
    );
}

// ---------------------------------------------------------------------------
// move_clip / remove_clip — single-clip placement (#103)
// ---------------------------------------------------------------------------

/// Load a 1 s file and cut an interior range, which leaves two clips.
/// Returns the head after the cut.
fn two_clip_session(dispatcher: &ToolDispatcher, ctx: &mut ToolContext, tmp: &Path) -> String {
    let src = write_sine_wav(tmp, "in.wav", 0.5);
    ok(dispatcher
        .invoke("load", json!({ "path": src.to_string_lossy() }), ctx)
        .unwrap());
    let res = ok(dispatcher
        .invoke(
            "cut_range",
            json!({
                "track": 0,
                "start_sample": SAMPLE_RATE as u64 / 4,
                "end_sample": SAMPLE_RATE as u64 / 2,
            }),
            ctx,
        )
        .unwrap());
    res["node_id"].as_str().unwrap().to_string()
}

fn clips_at_head(store: &session::Store) -> Vec<session::Clip> {
    let head = store.head().expect("head");
    store.get(head).expect("node").state.tracks[0].clips.clone()
}

#[test]
fn move_clip_moves_one_clip_and_leaves_the_others() {
    let (tmp, mut store, mut engine, dispatcher) = fresh();
    let mut clipboard: Option<Vec<f32>> = None;
    {
        let mut ctx = ToolContext {
            store: &mut store,
            engine: &mut engine,
            user_message: "",
            clipboard: &mut clipboard,
        };
        two_clip_session(&dispatcher, &mut ctx, tmp.path());
    }
    let before = clips_at_head(&store);
    assert!(
        before.len() >= 2,
        "the fixture needs an interior cut to produce two clips; got {}",
        before.len()
    );
    let first_start = before[0].start_in_track;

    {
        let mut ctx = ToolContext {
            store: &mut store,
            engine: &mut engine,
            user_message: "",
            clipboard: &mut clipboard,
        };
        ok(dispatcher
            .invoke(
                "move_clip",
                json!({ "track": 0, "clip_index": 1, "start_sec": 5.0 }),
                &mut ctx,
            )
            .unwrap());
    }

    let after = clips_at_head(&store);
    assert_eq!(
        after.len(),
        before.len(),
        "moving must not add or drop clips"
    );
    assert_eq!(
        after[0].start_in_track, first_start,
        "the other clip must not move — that is what time_shift is for"
    );
    assert_eq!(after[1].start_in_track, 5 * SAMPLE_RATE as u64);
}

/// The session's length is the furthest clip end, so dragging a clip
/// out to 5 s makes the timeline longer. Leaving it stale would make a
/// render stop before the clip the user just placed.
#[test]
fn moving_a_clip_later_extends_the_session_length() {
    let (tmp, mut store, mut engine, dispatcher) = fresh();
    let mut clipboard: Option<Vec<f32>> = None;
    {
        let mut ctx = ToolContext {
            store: &mut store,
            engine: &mut engine,
            user_message: "",
            clipboard: &mut clipboard,
        };
        two_clip_session(&dispatcher, &mut ctx, tmp.path());
        ok(dispatcher
            .invoke(
                "move_clip",
                json!({ "track": 0, "clip_index": 1, "start_sec": 5.0 }),
                &mut ctx,
            )
            .unwrap());
    }
    let head = store.head().unwrap();
    let state = store.get(head).unwrap().state;
    let furthest = state
        .tracks
        .iter()
        .flat_map(|t| t.clips.iter().map(|c| c.start_in_track + c.length))
        .max()
        .unwrap();
    assert_eq!(state.length_samples, furthest);
    assert!(state.length_samples > 5 * SAMPLE_RATE as u64);
}

/// Clips are rendered and drawn in vector order, so a clip dragged
/// before its neighbour has to be reordered — otherwise the session is
/// right and the waveform is wrong.
#[test]
fn a_clip_dragged_before_its_neighbour_is_reordered() {
    let (tmp, mut store, mut engine, dispatcher) = fresh();
    let mut clipboard: Option<Vec<f32>> = None;
    {
        let mut ctx = ToolContext {
            store: &mut store,
            engine: &mut engine,
            user_message: "",
            clipboard: &mut clipboard,
        };
        two_clip_session(&dispatcher, &mut ctx, tmp.path());
        // Push clip 0 past clip 1.
        ok(dispatcher
            .invoke(
                "move_clip",
                json!({ "track": 0, "clip_index": 0, "start_sec": 9.0 }),
                &mut ctx,
            )
            .unwrap());
    }
    let clips = clips_at_head(&store);
    assert!(
        clips
            .windows(2)
            .all(|w| w[0].start_in_track <= w[1].start_in_track),
        "clips must stay in start order: {:?}",
        clips.iter().map(|c| c.start_in_track).collect::<Vec<_>>()
    );
}

#[test]
fn move_clip_rejects_a_negative_start_and_a_bad_index() {
    let (tmp, mut store, mut engine, dispatcher) = fresh();
    let mut clipboard: Option<Vec<f32>> = None;
    let mut ctx = ToolContext {
        store: &mut store,
        engine: &mut engine,
        user_message: "",
        clipboard: &mut clipboard,
    };
    two_clip_session(&dispatcher, &mut ctx, tmp.path());

    // A negative start is refused by the schema, before the tool body
    // runs — which is the right layer: the model gets a machine-readable
    // reason rather than prose. That makes it a dispatch error rather
    // than a `ToolResult::Error`, so it is asserted differently from the
    // index check below.
    let e = dispatcher
        .invoke(
            "move_clip",
            json!({ "track": 0, "clip_index": 0, "start_sec": -1.0 }),
            &mut ctx,
        )
        .expect_err("a negative start must not reach the session");
    assert!(
        format!("{e}").contains("start_sec"),
        "the error should name the offending field: {e}"
    );

    let msg = err(dispatcher
        .invoke(
            "move_clip",
            json!({ "track": 0, "clip_index": 99, "start_sec": 1.0 }),
            &mut ctx,
        )
        .unwrap());
    assert!(msg.contains("clip_index"), "got: {msg}");
}

#[test]
fn remove_clip_drops_one_clip_and_keeps_the_rest() {
    let (tmp, mut store, mut engine, dispatcher) = fresh();
    let mut clipboard: Option<Vec<f32>> = None;
    {
        let mut ctx = ToolContext {
            store: &mut store,
            engine: &mut engine,
            user_message: "",
            clipboard: &mut clipboard,
        };
        two_clip_session(&dispatcher, &mut ctx, tmp.path());
    }
    let before = clips_at_head(&store);
    let survivor = before[1].start_in_track;

    {
        let mut ctx = ToolContext {
            store: &mut store,
            engine: &mut engine,
            user_message: "",
            clipboard: &mut clipboard,
        };
        ok(dispatcher
            .invoke(
                "remove_clip",
                json!({ "track": 0, "clip_index": 0 }),
                &mut ctx,
            )
            .unwrap());
    }

    let after = clips_at_head(&store);
    assert_eq!(after.len(), before.len() - 1);
    assert_eq!(
        after[0].start_in_track, survivor,
        "removing a clip must not move the ones that remain"
    );
}

#[test]
fn remove_clip_rejects_a_bad_index() {
    let (tmp, mut store, mut engine, dispatcher) = fresh();
    let mut clipboard: Option<Vec<f32>> = None;
    let mut ctx = ToolContext {
        store: &mut store,
        engine: &mut engine,
        user_message: "",
        clipboard: &mut clipboard,
    };
    two_clip_session(&dispatcher, &mut ctx, tmp.path());
    let msg = err(dispatcher
        .invoke(
            "remove_clip",
            json!({ "track": 0, "clip_index": 99 }),
            &mut ctx,
        )
        .unwrap());
    assert!(msg.contains("clip_index"), "got: {msg}");
}

// ---------------------------------------------------------------------------
// MP3 export (#93)
// ---------------------------------------------------------------------------

/// The acceptance criterion, with a tolerance rather than equality.
///
/// FLAC's round-trip test asserts sample equality because FLAC is
/// lossless. MP3 is not, so asserting equality here would either fail or
/// — worse — be written loose enough to pass on garbage. What is
/// actually claimed is that the decoded audio *is the same signal*:
/// correlated with the source and at the same level.
///
/// Correlation is taken at the best lag. MP3 carries encoder and decoder
/// delay (~2800 samples at 44.1 kHz), so a sample-aligned comparison
/// would fail on a perfect codec.
#[test]
fn mp3_export_round_trips_within_tolerance() {
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
    let id = load["node_id"].as_str().unwrap().to_string();

    let wav_out = tmp.path().join("ref.wav");
    let mp3_out = tmp.path().join("out.mp3");
    for (fmt, path) in [("wav", &wav_out), ("mp3", &mp3_out)] {
        ok(dispatcher
            .invoke(
                "render_final",
                json!({
                    "node_id": id,
                    "format": fmt,
                    "out_path": path.to_string_lossy(),
                }),
                &mut ctx,
            )
            .unwrap());
    }

    let reference = audio_decoder::decode_file(&wav_out).expect("decode wav");
    let decoded = audio_decoder::decode_file(&mp3_out).expect("decode mp3");
    assert_eq!(decoded.sample_rate, reference.sample_rate);
    assert_eq!(decoded.channels, reference.channels);

    // Skip the first 50 ms of the reference, where the decoder is still
    // filling its overlap buffers, and compare a half-second window.
    let sr = reference.sample_rate as usize;
    let a: Vec<f32> = reference
        .samples
        .iter()
        .skip(sr / 20)
        .take(sr / 2)
        .copied()
        .collect();
    let ea = a.iter().map(|v| v * v).sum::<f32>().sqrt();
    let mut best = 0.0f32;
    let mut best_lag = 0usize;
    for lag in 0..4000usize {
        if lag + a.len() > decoded.samples.len() {
            break;
        }
        let w = &decoded.samples[lag..lag + a.len()];
        let eb = w.iter().map(|v| v * v).sum::<f32>().sqrt();
        if ea <= 0.0 || eb <= 0.0 {
            continue;
        }
        let dot: f32 = a.iter().zip(w).map(|(x, y)| x * y).sum();
        let c = dot / (ea * eb);
        if c > best {
            best = c;
            best_lag = lag;
        }
    }
    assert!(
        best > 0.98,
        "mp3 round trip correlates only {best:.4} with the source — \
         that is not the same signal"
    );

    let w = &decoded.samples[best_lag..best_lag + a.len()];
    let rms_in = (a.iter().map(|v| v * v).sum::<f32>() / a.len() as f32).sqrt();
    let rms_out = (w.iter().map(|v| v * v).sum::<f32>() / w.len() as f32).sqrt();
    let db = 20.0 * (rms_out / rms_in).log10();
    assert!(
        db.abs() < 1.0,
        "mp3 round trip changed the level by {db:.2} dB"
    );

    // And it should actually be compressed.
    let wav_len = std::fs::metadata(&wav_out).unwrap().len();
    let mp3_len = std::fs::metadata(&mp3_out).unwrap().len();
    assert!(
        mp3_len < wav_len,
        "mp3 ({mp3_len} B) should be smaller than wav ({wav_len} B)"
    );
}

/// A lower bitrate must produce a smaller file. Without this, a
/// `bitrate_kbps` that was parsed and then ignored would look fine —
/// the round-trip test above passes either way.
#[test]
fn mp3_bitrate_argument_reaches_the_encoder() {
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
    let id = load["node_id"].as_str().unwrap().to_string();

    let mut sizes = Vec::new();
    for kbps in [64u32, 320] {
        let out = tmp.path().join(format!("out{kbps}.mp3"));
        let res = ok(dispatcher
            .invoke(
                "render_final",
                json!({
                    "node_id": id,
                    "format": "mp3",
                    "out_path": out.to_string_lossy(),
                    "bitrate_kbps": kbps,
                }),
                &mut ctx,
            )
            .unwrap());
        assert_eq!(res["bitrate_kbps"], json!(kbps));
        sizes.push(std::fs::metadata(&out).unwrap().len());
    }
    assert!(
        sizes[0] < sizes[1],
        "64 kbps ({} B) should be smaller than 320 kbps ({} B)",
        sizes[0],
        sizes[1]
    );
}

/// The default is reported, so a caller that did not ask can still see
/// what they got.
#[test]
fn mp3_reports_the_default_bitrate_when_none_is_given() {
    let (tmp, mut store, mut engine, dispatcher) = fresh();
    let src = write_sine_wav(tmp.path(), "in.wav", 0.3);
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
    let out = tmp.path().join("out.mp3");
    let res = ok(dispatcher
        .invoke(
            "render_final",
            json!({
                "node_id": id,
                "format": "mp3",
                "out_path": out.to_string_lossy(),
            }),
            &mut ctx,
        )
        .unwrap());
    assert_eq!(res["bitrate_kbps"], json!(192));
}
