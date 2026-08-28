//! A reversed time range must be reported, never panic.
//!
//! Several tools clamped `start` and `end` to the buffer length
//! *independently*, so asking for `start_sec: 10, end_sec: 5` left
//! `start > end` intact and the following `samples[start..end]` panicked
//! — killing the whole app, not just the tool call. Asking for a
//! backwards range is an easy slip for a model to make, so every tool
//! that takes a bare `start_sec` / `end_sec` pair is covered here.

use std::path::{Path, PathBuf};

use hound::{SampleFormat, WavSpec, WavWriter};
use serde_json::{json, Value};
use tempfile::TempDir;
use tools::{ToolContext, ToolDispatcher, ToolResult};

const SAMPLE_RATE: u32 = 48_000;

/// Two seconds of quiet tone, so a 10s..5s request is out of range at
/// both ends as well as reversed.
fn write_wav(dir: &Path, name: &str, channels: u16) -> PathBuf {
    let path = dir.join(name);
    let spec = WavSpec {
        channels,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut writer = WavWriter::create(&path, spec).expect("wav writer");
    for n in 0..(SAMPLE_RATE as usize * 2) {
        let t = n as f32 / SAMPLE_RATE as f32;
        let s = (2.0 * std::f32::consts::PI * 440.0 * t).sin() * 0.25;
        let q = (s * 32_767.0).round() as i16;
        for _ in 0..channels {
            writer.write_sample(q).unwrap();
        }
    }
    writer.finalize().unwrap();
    path
}

/// Every tool taking a bare seconds window, with the extra arguments
/// each one requires beyond `track` / `start_sec` / `end_sec`.
fn tools_with_a_seconds_window() -> Vec<(&'static str, Value)> {
    vec![
        ("invert", json!({})),
        ("de_esser", json!({ "threshold_db": -20.0 })),
        ("noise_gate", json!({ "threshold_db": -40.0 })),
        ("leveler", json!({ "target_db": -12.0 })),
        ("limiter", json!({ "ceiling_db": -1.0 })),
        ("click_removal", json!({})),
        ("vocal_reduction", json!({})),
    ]
}

#[test]
fn a_reversed_range_is_reported_not_panicked() {
    for channels in [1u16, 2u16] {
        for (tool, extra) in tools_with_a_seconds_window() {
            let tmp = TempDir::new().expect("tempdir");
            let mut store = session::Store::open(tmp.path()).expect("open store");
            let mut engine = audio_engine::Engine::new();
            let dispatcher = ToolDispatcher::default_dispatcher();
            let src = write_wav(tmp.path(), "in.wav", channels);

            let mut clipboard: Option<Vec<f32>> = None;
            let mut ctx = ToolContext {
                store: &mut store,
                engine: &mut engine,
                user_message: "",
                clipboard: &mut clipboard,
                allowed_tools: None,
            };

            dispatcher
                .invoke("load", json!({ "path": src.to_string_lossy() }), &mut ctx)
                .expect("load");

            let mut args = json!({ "track": 0, "start_sec": 10.0, "end_sec": 5.0 });
            for (k, v) in extra.as_object().expect("object") {
                args[k] = v.clone();
            }

            // The call itself must return, whatever it decides — a panic
            // here would have taken the process down.
            let result = dispatcher
                .invoke(tool, args, &mut ctx)
                .unwrap_or_else(|e| panic!("`{tool}` ({channels}ch) failed to dispatch: {e:?}"));

            match result {
                ToolResult::Error(msg) => assert!(
                    msg.contains("start_sec") || msg.contains("range"),
                    "`{tool}` ({channels}ch) should explain the bad range, said: {msg}"
                ),
                ToolResult::Ok(_) => {
                    panic!(
                        "`{tool}` ({channels}ch) accepted a reversed range instead of reporting it"
                    )
                }
            }
        }
    }
}

/// Negative bounds are refused rather than silently clamped to zero,
/// so a sign error is reported instead of quietly processing a
/// different region than the one asked for.
///
/// NaN and infinity are deliberately absent: JSON cannot represent
/// them, so `serde_json::Number::from_f64` returns `None` and such a
/// bound can never reach a tool through the dispatcher. The finiteness
/// check in `check_seconds_order` guards direct Rust callers only.
#[test]
fn negative_bounds_are_reported() {
    for (start, end) in [(-5.0, 1.0), (0.5, -1.0)] {
        let tmp = TempDir::new().expect("tempdir");
        let mut store = session::Store::open(tmp.path()).expect("open store");
        let mut engine = audio_engine::Engine::new();
        let dispatcher = ToolDispatcher::default_dispatcher();
        let src = write_wav(tmp.path(), "in.wav", 1);

        let mut clipboard: Option<Vec<f32>> = None;
        let mut ctx = ToolContext {
            store: &mut store,
            engine: &mut engine,
            user_message: "",
            clipboard: &mut clipboard,
            allowed_tools: None,
        };
        dispatcher
            .invoke("load", json!({ "path": src.to_string_lossy() }), &mut ctx)
            .expect("load");

        // `serde_json` cannot represent NaN/Infinity, so build the value
        // directly rather than through the `json!` macro.
        let mut args = serde_json::Map::new();
        args.insert("track".into(), json!(0));
        if let Some(n) = serde_json::Number::from_f64(start) {
            args.insert("start_sec".into(), Value::Number(n));
        }
        if let Some(n) = serde_json::Number::from_f64(end) {
            args.insert("end_sec".into(), Value::Number(n));
        }

        let result = dispatcher
            .invoke("invert", Value::Object(args), &mut ctx)
            .expect("dispatch must not panic");
        assert!(
            matches!(result, ToolResult::Error(_)),
            "invert accepted start={start} end={end}"
        );
    }
}
