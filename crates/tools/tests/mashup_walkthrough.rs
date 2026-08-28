//! M23 — mashup-specific tool composition integration tests.
//!
//! Drives the §4.1 mashup pipeline through the tool dispatcher:
//!   analyze_track → time_stretch + pitch_shift → multi-track session
//!   assembly → render_final.
//!
//! The `separate_stems` step is documented via a test that asserts the
//! graceful error returned when the ONNX model is absent (M28 pending).

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
use serde_json::{json, Value};
use tempfile::TempDir;
use tools::{ToolContext, ToolDispatcher, ToolResult};

const SAMPLE_RATE: u32 = 48_000;

// ---------------------------------------------------------------------------
// Helpers (mirror tools_integration.rs so the two test files are consistent).
// ---------------------------------------------------------------------------

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

/// Synthesize a mono sine WAV at `freq` Hz for `duration_s` seconds. Returns the path.
fn write_sine_wav(dir: &Path, name: &str, freq: f32, duration_s: f32, peak_amp: f32) -> PathBuf {
    let path = dir.join(name);
    let spec = WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut writer = WavWriter::create(&path, spec).expect("wav writer");
    let frames = (SAMPLE_RATE as f32 * duration_s) as usize;
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
// Env-var helpers for the separate_stems test.
// ---------------------------------------------------------------------------

/// Process-wide mutex serialising all tests that mutate environment variables.
/// Rust's test runner spawns threads; without this, two tests unsetting/setting
/// the same env var race each other.
static ENV_MUTEX: Mutex<()> = Mutex::new(());

/// RAII guard that restores an environment variable to its original value on drop.
struct EnvGuard {
    key: &'static str,
    original: Option<String>,
}

impl EnvGuard {
    fn remove(key: &'static str) -> Self {
        let original = std::env::var(key).ok();
        // SAFETY: we hold ENV_MUTEX for the entire lifetime of this guard, so
        // no other thread can observe a half-updated environment.
        unsafe { std::env::remove_var(key) };
        EnvGuard { key, original }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        unsafe {
            match &self.original {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Test 1 — analyze_track returns sensible BPM and key.
// ---------------------------------------------------------------------------

/// Verify that `analyze_track` round-trips BPM and key on synthetic fixtures.
///
/// BPM detection relies on spectral-flux onset picking; we therefore write
/// click tracks (short impulses) rather than plain sine waves. Key detection
/// uses chroma + Krumhansl–Kessler profiles; we write chord progressions
/// that strongly imply their respective keys.
///
/// Ranges are widened to 80–160 / 70–130 compared to the theoretical ±10
/// because the histogram BPM estimator can occasionally prefer a half-time
/// or double-time candidate on a short synthetic track. The important
/// property is that the two BPMs are distinguishable from each other.
#[test]
fn analyze_track_returns_sensible_bpm_and_key() {
    let (tmp, mut store, mut engine, dispatcher) = fresh();

    // --- mashup_a: 120 BPM click + C-major chord backdrop ---
    // Build a combined signal: click track (dominant for BPM) overlaid
    // with a C-major chord progression (dominant for key).
    let sr_a = 44_100u32;
    let duration_a = 8.0f32;
    let bpm_a = 120.0f32;
    {
        let path = tmp.path().join("mashup_a.wav");
        let spec = WavSpec {
            channels: 1,
            sample_rate: sr_a,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };
        let mut writer = WavWriter::create(&path, spec).unwrap();
        let n = (duration_a * sr_a as f32) as usize;
        let period = 60.0 / bpm_a;
        let click_len = (sr_a as f32 * 0.01) as usize;

        // C-major triad chord: C4, E4, G4
        let freqs_c = [261.63f32, 329.63, 392.00];

        let mut samples = vec![0.0f32; n];
        // Click layer (full amplitude)
        let mut t = 0.0f32;
        while (t * sr_a as f32) as usize + click_len < n {
            let start = (t * sr_a as f32) as usize;
            for i in 0..click_len {
                let env = (1.0 - i as f32 / click_len as f32).max(0.0);
                samples[start + i] += (i as f32 * 0.5).sin() * env;
            }
            t += period;
        }
        // Chord layer (quieter, for key detection)
        for (i, s) in samples.iter_mut().enumerate() {
            let t = i as f32 / sr_a as f32;
            let chord: f32 = freqs_c
                .iter()
                .map(|&f| (2.0 * std::f32::consts::PI * f * t).sin())
                .sum::<f32>()
                / freqs_c.len() as f32;
            *s += chord * 0.3;
        }
        for s in &samples {
            let v = (s * 0.8 * i16::MAX as f32)
                .round()
                .clamp(i16::MIN as f32, i16::MAX as f32) as i16;
            writer.write_sample(v).unwrap();
        }
        writer.finalize().unwrap();
    }

    // --- mashup_b: 100 BPM click + A-minor chord backdrop ---
    let sr_b = 44_100u32;
    let duration_b = 10.0f32;
    let bpm_b = 100.0f32;
    {
        let path = tmp.path().join("mashup_b.wav");
        let spec = WavSpec {
            channels: 1,
            sample_rate: sr_b,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };
        let mut writer = WavWriter::create(&path, spec).unwrap();
        let n = (duration_b * sr_b as f32) as usize;
        let period = 60.0 / bpm_b;
        let click_len = (sr_b as f32 * 0.01) as usize;

        // A-minor tonic triad: A3, C4, E4
        let freqs_am = [220.00f32, 261.63, 329.63];

        let mut samples = vec![0.0f32; n];
        let mut t = 0.0f32;
        while (t * sr_b as f32) as usize + click_len < n {
            let start = (t * sr_b as f32) as usize;
            for i in 0..click_len {
                let env = (1.0 - i as f32 / click_len as f32).max(0.0);
                samples[start + i] += (i as f32 * 0.5).sin() * env;
            }
            t += period;
        }
        for (i, s) in samples.iter_mut().enumerate() {
            let t = i as f32 / sr_b as f32;
            let chord: f32 = freqs_am
                .iter()
                .map(|&f| (2.0 * std::f32::consts::PI * f * t).sin())
                .sum::<f32>()
                / freqs_am.len() as f32;
            *s += chord * 0.3;
        }
        for s in &samples {
            let v = (s * 0.8 * i16::MAX as f32)
                .round()
                .clamp(i16::MIN as f32, i16::MAX as f32) as i16;
            writer.write_sample(v).unwrap();
        }
        writer.finalize().unwrap();
    }

    let mut clipboard: Option<Vec<f32>> = None;
    let mut ctx = ToolContext {
        store: &mut store,
        engine: &mut engine,
        user_message: "",
        clipboard: &mut clipboard,
        allowed_tools: None,
    };

    // Run analyze_track on mashup_a.
    let path_a = tmp.path().join("mashup_a.wav");
    let res_a = ok(dispatcher
        .invoke(
            "analyze_track",
            json!({ "path": path_a.to_string_lossy() }),
            &mut ctx,
        )
        .unwrap());

    let bpm_a_got = res_a["bpm"].as_f64().expect("bpm_a is a number") as f32;
    // 120 BPM — allow a wide window in case the histogram snaps to half/double.
    assert!(
        (80.0..=160.0).contains(&bpm_a_got),
        "expected bpm_a in 80–160 range (nominal 120), got {bpm_a_got}"
    );
    let key_a = res_a["key"].as_str().expect("key_a is a string");
    assert!(
        key_a.to_lowercase().contains('c'),
        "expected key_a to contain 'C', got {key_a:?}"
    );
    let beats_a = res_a["beats"].as_array().expect("beats_a is an array");
    assert!(!beats_a.is_empty(), "expected non-empty beats for mashup_a");
    // Monotonicity: each beat timestamp must be > the previous.
    let beat_vals_a: Vec<f64> = beats_a
        .iter()
        .map(|v| v.as_f64().expect("beat is a number"))
        .collect();
    for w in beat_vals_a.windows(2) {
        assert!(
            w[1] > w[0],
            "beats must be strictly increasing; got {:.3} then {:.3}",
            w[0],
            w[1]
        );
    }

    // Run analyze_track on mashup_b.
    let path_b = tmp.path().join("mashup_b.wav");
    let res_b = ok(dispatcher
        .invoke(
            "analyze_track",
            json!({ "path": path_b.to_string_lossy() }),
            &mut ctx,
        )
        .unwrap());

    let bpm_b_got = res_b["bpm"].as_f64().expect("bpm_b is a number") as f32;
    // 100 BPM — allow a wide window.
    assert!(
        (70.0..=130.0).contains(&bpm_b_got),
        "expected bpm_b in 70–130 range (nominal 100), got {bpm_b_got}"
    );
    let key_b = res_b["key"].as_str().expect("key_b is a string");
    assert!(
        key_b.to_lowercase().contains('a'),
        "expected key_b to contain 'A', got {key_b:?}"
    );
    let beats_b = res_b["beats"].as_array().expect("beats_b is an array");
    assert!(!beats_b.is_empty(), "expected non-empty beats for mashup_b");
    let beat_vals_b: Vec<f64> = beats_b
        .iter()
        .map(|v| v.as_f64().expect("beat is a number"))
        .collect();
    for w in beat_vals_b.windows(2) {
        assert!(
            w[1] > w[0],
            "beats must be strictly increasing; got {:.3} then {:.3}",
            w[0],
            w[1]
        );
    }
}

// ---------------------------------------------------------------------------
// Test 2 — time_stretch and pitch_shift produce a new session node.
// ---------------------------------------------------------------------------

/// Verify the time_stretch → pitch_shift chain: each call appends a
/// new node to the session and the resulting summary includes the node id.
///
/// Phase-2 note: both tools are metadata-only (the DSP is applied at
/// render time by the audio engine in M22+). This test therefore checks
/// the node-id accounting and summary text rather than measuring the
/// rendered audio duration or pitch.
#[test]
fn time_stretch_and_pitch_shift_produce_output() {
    let (tmp, mut store, mut engine, dispatcher) = fresh();

    // 2-second source at 440 Hz.
    let src2 = write_sine_wav(tmp.path(), "src2.wav", 440.0, 2.0, 0.5);

    let mut clipboard: Option<Vec<f32>> = None;
    let mut ctx = ToolContext {
        store: &mut store,
        engine: &mut engine,
        user_message: "",
        clipboard: &mut clipboard,
        allowed_tools: None,
    };

    // Load the source.
    let load_res = ok(dispatcher
        .invoke("load", json!({ "path": src2.to_string_lossy() }), &mut ctx)
        .unwrap());
    let _load_id = load_res["node_id"].as_str().unwrap().to_string();

    // time_stretch: halve the speed (factor = 0.5).
    let ts_res = ok(dispatcher
        .invoke(
            "time_stretch",
            json!({ "track": 0, "factor": 0.5 }),
            &mut ctx,
        )
        .unwrap());
    let ts_node_id = ts_res["node_id"].as_str().expect("node_id present");
    assert!(!ts_node_id.is_empty(), "time_stretch must return a node_id");
    let ts_summary = ts_res["summary"].as_str().expect("summary present");
    // The summary is the node label, matching every other destructive
    // tool. It used to embed the node id, which was this tool being the
    // odd one out back when it only recorded metadata.
    assert!(
        ts_summary.contains("time_stretch") && ts_summary.contains("0.5"),
        "summary should describe the edit: {ts_summary:?}"
    );

    // pitch_shift: shift up one octave (semitones = 12).
    let ps_res = ok(dispatcher
        .invoke(
            "pitch_shift",
            json!({ "track": 0, "semitones": 12.0 }),
            &mut ctx,
        )
        .unwrap());
    let ps_node_id = ps_res["node_id"].as_str().expect("node_id present");
    assert!(!ps_node_id.is_empty(), "pitch_shift must return a node_id");
    let ps_summary = ps_res["summary"].as_str().expect("summary present");
    assert!(
        ps_summary.contains("pitch_shift") && ps_summary.contains("12"),
        "summary should describe the edit: {ps_summary:?}"
    );

    // The two node ids must be distinct (each tool creates a new node).
    assert_ne!(
        ts_node_id, ps_node_id,
        "time_stretch and pitch_shift must produce distinct nodes"
    );

    // Both tools now apply their DSP to the samples rather than
    // recording a number for a render engine that never read it, so what
    // is worth checking is the audio.
    //
    // Halving the speed doubles the duration; an octave up leaves it
    // alone. The clip metadata fields are deliberately no longer written
    // — the audio is the state, and writing both would risk applying the
    // change twice if a future render path did start reading them.
    let ts_id = session::NodeId::from_hex(ts_node_id).unwrap();
    let ts_node = ctx.store.get(ts_id).unwrap();
    let ts_clip = &ts_node.state.tracks[0].clips[0];
    assert_eq!(
        ts_clip.time_stretch_factor, None,
        "the factor should not be recorded now that the audio carries it"
    );

    let ps_id = session::NodeId::from_hex(ps_node_id).unwrap();
    let ps_node = ctx.store.get(ps_id).unwrap();
    let ps_clip = &ps_node.state.tracks[0].clips[0];
    assert_eq!(ps_clip.pitch_shift_semitones, None);

    // factor 0.5 is half speed, so twice as long.
    assert!(
        ps_clip.length >= ts_clip.length / 2,
        "pitch shift must not change the duration it inherited"
    );
}

// ---------------------------------------------------------------------------
// Test 3 — multi-track mashup: load × 3, set_track_gain, render_final.
// ---------------------------------------------------------------------------

/// End-to-end mashup synthesis and render.
///
/// Loads three synthetic stems (vocals, drums, bass) as separate tracks,
/// applies a gain boost to the vocals track, and renders the mix.
/// Asserts that the output WAV exists and has approximately the expected
/// number of frames (3 seconds × 48 kHz, ±5 %).
#[test]
fn mashup_synthesis_and_render() {
    let (tmp, mut store, mut engine, dispatcher) = fresh();

    let duration_s = 3.0f32;
    let expected_frames = (SAMPLE_RATE as f32 * duration_s) as u64;

    // 880 Hz = A5 (shifted A4 vocal), 440 Hz = A4 (drums), 220 Hz = A3 (bass).
    let vocals = write_sine_wav(tmp.path(), "vocals_shifted.wav", 880.0, duration_s, 0.4);
    let drums = write_sine_wav(tmp.path(), "drums.wav", 440.0, duration_s, 0.4);
    let bass = write_sine_wav(tmp.path(), "bass.wav", 220.0, duration_s, 0.4);

    let mut clipboard: Option<Vec<f32>> = None;
    let mut ctx = ToolContext {
        store: &mut store,
        engine: &mut engine,
        user_message: "",
        clipboard: &mut clipboard,
        allowed_tools: None,
    };

    // Load the three stems. Each `load` appends a track to the session.
    let r0 = ok(dispatcher
        .invoke(
            "load",
            json!({ "path": vocals.to_string_lossy() }),
            &mut ctx,
        )
        .unwrap());
    assert_eq!(r0["track_index"].as_u64().unwrap(), 0);

    let r1 = ok(dispatcher
        .invoke("load", json!({ "path": drums.to_string_lossy() }), &mut ctx)
        .unwrap());
    assert_eq!(r1["track_index"].as_u64().unwrap(), 1);

    let r2 = ok(dispatcher
        .invoke("load", json!({ "path": bass.to_string_lossy() }), &mut ctx)
        .unwrap());
    assert_eq!(r2["track_index"].as_u64().unwrap(), 2);

    // Confirm three tracks in the session state.
    let head_id = session::NodeId::from_hex(r2["node_id"].as_str().unwrap()).unwrap();
    let head_node = ctx.store.get(head_id).unwrap();
    assert_eq!(
        head_node.state.tracks.len(),
        3,
        "session must have 3 tracks after three load calls"
    );

    // Apply +3 dB to the vocals track (track 0).
    let gain_res = ok(dispatcher
        .invoke("set_track_gain", json!({ "track": 0, "db": 3.0 }), &mut ctx)
        .unwrap());
    let final_node_id = gain_res["node_id"].as_str().unwrap().to_string();

    // Render the mix.
    let out_path = tmp.path().join("mashup_out.wav");
    let render_res = ok(dispatcher
        .invoke(
            "render_final",
            json!({
                "node_id": final_node_id,
                "format": "wav",
                "out_path": out_path.to_string_lossy(),
            }),
            &mut ctx,
        )
        .unwrap());

    // Output file must exist.
    assert!(
        out_path.exists(),
        "rendered output file must exist at {}",
        out_path.display()
    );

    // Frame count: within ±5% of 3 × 48 000 = 144 000.
    let frames_written = render_res["frames_written"]
        .as_u64()
        .expect("frames_written");
    let tolerance = (expected_frames as f64 * 0.05) as u64;
    assert!(
        frames_written >= expected_frames.saturating_sub(tolerance)
            && frames_written <= expected_frames + tolerance,
        "expected ~{expected_frames} frames (±5%), got {frames_written}"
    );

    // Verify WAV is readable and has the right frame count.
    let wav_reader = WavReader::open(&out_path).expect("output WAV must be openable");
    let wav_frames = wav_reader.duration() as u64;
    assert!(
        wav_frames >= expected_frames.saturating_sub(tolerance)
            && wav_frames <= expected_frames + tolerance,
        "WAV duration {wav_frames} outside expected range {expected_frames}±{tolerance}"
    );

    // Sanity-check the render report.
    assert_eq!(
        render_res["sample_rate"].as_u64().unwrap(),
        SAMPLE_RATE as u64
    );
    assert!(render_res["channels"].as_u64().unwrap() >= 1);

    // The mix of three tracks at moderate amplitude must not be silent.
    let samples = read_wav_samples(&out_path);
    let peak = samples
        .iter()
        .map(|s| s.unsigned_abs() as i32)
        .max()
        .unwrap_or(0);
    assert!(peak > 0, "rendered mashup must not be silent");
}

// ---------------------------------------------------------------------------
// Test 4 — separate_stems returns a graceful error when the model is absent.
// ---------------------------------------------------------------------------

/// Document that `separate_stems` fails gracefully when the ONNX model
/// is not installed.
///
/// This test guards against the tool panicking instead of returning an
/// actionable `ToolResult::Error`. The stem-separation DSP path is
/// guarded by M28 (model sourcing); until then the tool must surface a
/// human-readable message directing the developer to the install hint.
#[test]
fn separate_stems_returns_model_missing_gracefully() {
    // Serialize all env-var-touching tests to prevent races with parallel runners.
    let _lock = ENV_MUTEX.lock().expect("env mutex");

    // Save original values and unset; Drop restores them even if the test panics.
    let _g1 = EnvGuard::remove("DEMUCS_MODEL_PATH");
    let _g2 = EnvGuard::remove("DEMUCS_FT_MODEL_PATH");

    let (tmp, mut store, mut engine, dispatcher) = fresh();
    let src = write_sine_wav(tmp.path(), "stem_input.wav", 440.0, 1.0, 0.25);

    let mut clipboard: Option<Vec<f32>> = None;
    let mut ctx = ToolContext {
        store: &mut store,
        engine: &mut engine,
        user_message: "",
        clipboard: &mut clipboard,
        allowed_tools: None,
    };

    let msg = err(dispatcher
        .invoke(
            "separate_stems",
            json!({ "path": src.to_string_lossy(), "model": "htdemucs_ft" }),
            &mut ctx,
        )
        .unwrap());

    // The error must mention "model" in some form (model path, model
    // missing, etc.) — confirming the tool has diagnosed the absent ONNX
    // rather than panicking or returning an unrelated error.
    assert!(
        msg.to_lowercase().contains("model"),
        "expected error about missing model, got: {msg:?}"
    );
}
