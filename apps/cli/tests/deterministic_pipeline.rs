//! Deterministic, AI-free E2E smoke for the Phase 1 single-track edit
//! pipeline (M16).
//!
//! Drives the same [`tools::ToolDispatcher`] the agent loop uses, but
//! invokes tools directly with hand-picked arguments. Proves the
//! deterministic bottom half of the stack — `load` -> `cut_range` ->
//! `normalize` -> `render_final` — works end-to-end without any
//! dependency on Anthropic, the network, or the AI's tool choices.
//!
//! The matching AI-driven test lives in `podcast_cleanup.rs` and is
//! `#[ignore]`d behind an env var.
//!
//! Fixture: a 15 s, 44.1 kHz mono 16-bit WAV with 2 s of silence
//! followed by 13 s of a 440 Hz sine at -6 dBFS. Generated into the
//! per-test temp dir so the binary fixture never lands in git.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde_json::json;
use tools::{ToolContext, ToolDispatcher, ToolResult};

const SAMPLE_RATE: u32 = 44_100;
const TOTAL_SECONDS: u32 = 15;
const SILENCE_SECONDS: u32 = 2;
const SINE_HZ: f32 = 440.0;
/// -6 dBFS in linear amplitude. The exact target the fixture is meant
/// to hit; we assert against this in the pre-normalize sanity check.
const SINE_AMPLITUDE: f32 = 0.501_187_2; // 10^(-6/20)
/// Acceptable deviation around target peaks/durations. Peaks are quoted
/// to 0.1 dB; duration to one frame.
const PEAK_TOLERANCE_DB: f32 = 0.1;

/// Write the fixture WAV. Returns the path and the precomputed silence
/// length in frames so callers can pass it straight to `cut_range`.
fn write_fixture(dir: &Path) -> (PathBuf, u64) {
    let path = dir.join("raw_podcast_intro.wav");
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(&path, spec).expect("create wav");
    let total_frames = SAMPLE_RATE * TOTAL_SECONDS;
    let silence_frames = SAMPLE_RATE * SILENCE_SECONDS;
    for i in 0..total_frames {
        let s = if i < silence_frames {
            0.0
        } else {
            let t = (i - silence_frames) as f32 / SAMPLE_RATE as f32;
            (t * SINE_HZ * std::f32::consts::TAU).sin() * SINE_AMPLITUDE
        };
        // Symmetric round-to-i16. The simple `* MAX as f32 as i16` cast
        // would saturate above +1.0 and we don't want fixture jitter
        // muddying the peak assertion.
        let q = (s * i16::MAX as f32).clamp(i16::MIN as f32, i16::MAX as f32);
        writer.write_sample(q as i16).expect("write sample");
    }
    writer.finalize().expect("finalize wav");
    (path, silence_frames as u64)
}

/// Convenience: run `dispatcher.invoke(name, args, ctx)`, panic with
/// context on any error path so a failing tool surfaces as a clear test
/// failure rather than an unwrap stack trace.
fn invoke(
    dispatcher: &ToolDispatcher,
    name: &str,
    args: serde_json::Value,
    ctx: &mut ToolContext,
) -> serde_json::Value {
    match dispatcher.invoke(name, args.clone(), ctx) {
        Ok(ToolResult::Ok(v)) => v,
        Ok(ToolResult::Error(msg)) => {
            panic!("tool {name} returned ToolResult::Error({msg}); args={args}")
        }
        Err(e) => panic!("dispatch error for {name}: {e}; args={args}"),
    }
}

/// Linear-amplitude peak across an interleaved f32 buffer, expressed in
/// dBFS. Returns -∞ for a silent buffer; callers in this test are never
/// expected to hit that branch.
fn peak_dbfs(samples: &[f32]) -> f32 {
    let peak = samples.iter().copied().map(f32::abs).fold(0.0f32, f32::max);
    if peak <= 0.0 {
        f32::NEG_INFINITY
    } else {
        20.0 * peak.log10()
    }
}

/// Phase 1 final smoke gate. Walk the entire deterministic-tool stack
/// using only the dispatcher; verify the rendered WAV's frame count,
/// channel/sample-rate parity, and peak amplitude.
#[test]
fn podcast_cleanup_deterministic() {
    let project = tempfile::tempdir().expect("tempdir");
    let (input_path, silence_frames) = write_fixture(project.path());

    // Sanity check the fixture: silence at the head, expected peak in
    // the body. If this drifts, every downstream assertion gets
    // confusing, so check it explicitly.
    {
        let decoded = audio_decoder::decode_file(&input_path).expect("decode fixture");
        assert_eq!(decoded.sample_rate, SAMPLE_RATE);
        assert_eq!(decoded.channels, 1);
        let head_peak = peak_dbfs(&decoded.samples[..silence_frames as usize]);
        assert!(
            head_peak.is_infinite() || head_peak < -90.0,
            "fixture head should be silent, got {head_peak} dBFS",
        );
        let body_peak = peak_dbfs(&decoded.samples[silence_frames as usize..]);
        assert!(
            (body_peak - (-6.0)).abs() < 0.2,
            "fixture body peak should be ~-6 dBFS, got {body_peak}",
        );
    }

    let dispatcher = ToolDispatcher::default_dispatcher();
    let store = Arc::new(Mutex::new(
        session::Store::open(project.path()).expect("open store"),
    ));
    let engine = Arc::new(Mutex::new(audio_engine::Engine::new()));

    // Run the pipeline. Each block reacquires the locks to mirror the
    // shape of the real agent loop, where each tool call is a separate
    // synchronous dispatch under the same shared mutexes.

    // 1. load
    let load_result = {
        let mut s = store.lock().unwrap();
        let mut e = engine.lock().unwrap();
        let mut ctx = ToolContext {
            store: &mut s,
            engine: &mut e,
        };
        invoke(
            &dispatcher,
            "load",
            json!({ "path": input_path.to_string_lossy() }),
            &mut ctx,
        )
    };
    assert_eq!(
        load_result["sample_rate"].as_u64(),
        Some(SAMPLE_RATE as u64),
    );
    assert_eq!(load_result["channels"].as_u64(), Some(1));
    assert_eq!(
        load_result["length_samples"].as_u64(),
        Some((SAMPLE_RATE * TOTAL_SECONDS) as u64),
    );

    // 2. cut_range — remove the head silence: [0, silence_frames).
    {
        let mut s = store.lock().unwrap();
        let mut e = engine.lock().unwrap();
        let mut ctx = ToolContext {
            store: &mut s,
            engine: &mut e,
        };
        invoke(
            &dispatcher,
            "cut_range",
            json!({
                "track": 0,
                "start_sample": 0,
                "end_sample": silence_frames,
            }),
            &mut ctx,
        );
    }

    // 3. normalize to -1 dBFS.
    let normalize_result = {
        let mut s = store.lock().unwrap();
        let mut e = engine.lock().unwrap();
        let mut ctx = ToolContext {
            store: &mut s,
            engine: &mut e,
        };
        invoke(
            &dispatcher,
            "normalize",
            json!({ "track": 0, "target_dbfs": -1.0 }),
            &mut ctx,
        )
    };
    // The fixture body is ~-6 dBFS, so the normalize tool should report
    // applying ~+5 dB of gain. Asserting the order-of-magnitude here
    // catches a regression in the `normalize` tool that the final-render
    // check would also notice but more obscurely.
    let applied = normalize_result["applied_gain_db"].as_f64().unwrap() as f32;
    assert!(
        (applied - 5.0).abs() < 0.5,
        "expected ~+5 dB applied, got {applied}",
    );

    // 4. render_final using the current head (= the normalized state).
    let head_id = store.lock().unwrap().head().expect("head set after ops");
    let out_path = project.path().join("out.wav");
    let render_result = {
        let mut s = store.lock().unwrap();
        let mut e = engine.lock().unwrap();
        let mut ctx = ToolContext {
            store: &mut s,
            engine: &mut e,
        };
        invoke(
            &dispatcher,
            "render_final",
            json!({
                "node_id": head_id.to_hex(),
                "format": "wav",
                "out_path": out_path.to_string_lossy(),
            }),
            &mut ctx,
        )
    };

    // The render report's own peak should already be at -1 dBFS within
    // tolerance — checking this here separates a render-side bug from a
    // decode/round-trip bug downstream.
    let report_peak = render_result["peak_dbfs"].as_f64().unwrap() as f32;
    assert!(
        (report_peak - (-1.0)).abs() < PEAK_TOLERANCE_DB,
        "render report peak should be ~-1 dBFS, got {report_peak}",
    );

    // 5. Decode the rendered file and verify duration + peak. Use
    //    `audio_decoder` (the same path the rest of the app uses) so the
    //    test exercises the same decoding contract production callers
    //    rely on.
    assert!(out_path.exists(), "render_final should write the output");
    let out = audio_decoder::decode_file(&out_path).expect("decode rendered output");
    assert_eq!(out.sample_rate, SAMPLE_RATE, "sample rate must round-trip");
    assert_eq!(out.channels, 1, "channel count must round-trip");

    let expected_frames = (SAMPLE_RATE * (TOTAL_SECONDS - SILENCE_SECONDS)) as usize;
    let actual_frames = out.samples.len() / out.channels as usize;
    let frame_delta = (actual_frames as i64 - expected_frames as i64).abs();
    assert!(
        frame_delta <= 1,
        "expected {expected_frames} frames (~13 s), got {actual_frames} ({frame_delta} frame delta)",
    );

    let out_peak = peak_dbfs(&out.samples);
    assert!(
        (out_peak - (-1.0)).abs() < PEAK_TOLERANCE_DB,
        "decoded output peak should be ~-1 dBFS, got {out_peak}",
    );

    // The first frame of the render should be from the post-silence
    // body. With SINE_HZ=440 and target=-1 dBFS, the very first sample
    // is sin(0)*gain = 0; the next frame is non-zero. We check frame 1
    // rather than frame 0 to confirm the silence is actually gone.
    let first_audible = out
        .samples
        .iter()
        .take(SAMPLE_RATE as usize / 100) // first 10 ms
        .copied()
        .map(f32::abs)
        .fold(0.0f32, f32::max);
    assert!(
        first_audible > 0.05,
        "first 10 ms after cut should contain audio, peak was {first_audible}",
    );
}
