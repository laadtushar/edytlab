//! `transcribe` and `separate_stems` without an ONNX Runtime library (#383).
//!
//! Point the model variables at any file that exists and the tool loads
//! it. With `ort`'s `load-dynamic`, that used to hang inside
//! `ToolDispatcher::invoke` when the library was missing (`ort` deadlocks
//! on a library it cannot load), and the agent loop calls `invoke`
//! holding the dispatcher, store and engine mutexes, so one `transcribe`
//! call froze the agent.
//!
//! Both tools must answer with a `ToolResult::Error` instead, and the
//! error must still say the feature is not implemented: installing the
//! runtime does not make either one work (#384, #385).
//!
//! A single test in its own binary, because it edits the process
//! environment and its watchdog ends the whole process.

use std::io::Write;
use std::path::Path;
use std::time::Duration;

use hound::{SampleFormat, WavSpec, WavWriter};
use serde_json::json;
use tools::{ToolContext, ToolDispatcher, ToolResult};

fn write_sine_wav(path: &Path) {
    let spec = WavSpec {
        channels: 1,
        sample_rate: 16_000,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut writer = WavWriter::create(path, spec).expect("wav writer");
    for n in 0..16_000u32 {
        let t = n as f32 / 16_000.0;
        let s = (2.0 * std::f32::consts::PI * 440.0 * t).sin() * 0.25;
        writer.write_sample((s * 32_767.0) as i16).unwrap();
    }
    writer.finalize().unwrap();
}

/// Fail the run, rather than stall it, if a tool never returns.
///
/// Aborts rather than exiting: `exit` runs `ort`'s exit handler, which
/// waits on a lock the stuck thread holds, so it would hang as well. The
/// message is written straight to stderr because the test harness
/// captures `eprintln!`.
fn watchdog() {
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(60));
        let _ = writeln!(
            std::io::stderr(),
            "a model tool did not return within 60 s without ONNX Runtime: `ort` deadlocks on a \
             library it cannot load, which ml_pipeline::runtime::ensure exists to prevent"
        );
        std::process::abort();
    });
}

#[test]
fn model_tools_report_a_missing_runtime_instead_of_hanging() {
    watchdog();
    let tmp = tempfile::tempdir().expect("tempdir");
    let wav = tmp.path().join("in.wav");
    write_sine_wav(&wav);
    // Exists, so the tools get past "model not found" and reach the load.
    let model = tmp.path().join("model.onnx");
    std::fs::write(&model, b"not an onnx model").expect("write");

    // SAFETY: this is the only test in this binary, so no other thread
    // reads the environment while it is edited.
    unsafe {
        std::env::remove_var("ORT_DYLIB_PATH");
        std::env::set_var("WHISPER_MODEL_PATH", &model);
        std::env::set_var("DEMUCS_MODEL_PATH", &model);
        std::env::set_var("DEMUCS_FT_MODEL_PATH", &model);
    }

    let mut store = session::Store::open(tmp.path()).expect("open store");
    let mut engine = audio_engine::Engine::new();
    let dispatcher = ToolDispatcher::default_dispatcher();
    let mut clipboard: Option<tools::Clipboard> = None;
    let mut ctx = ToolContext {
        store: &mut store,
        engine: &mut engine,
        user_message: "",
        clipboard: &mut clipboard,
        allowed_tools: None,
    };

    let calls = [
        ("transcribe", json!({ "path": wav.to_string_lossy() })),
        ("separate_stems", json!({ "path": wav.to_string_lossy() })),
        (
            "separate_stems",
            json!({ "path": wav.to_string_lossy(), "model": "htdemucs" }),
        ),
    ];
    for (name, args) in calls {
        let result = dispatcher
            .invoke(name, args.clone(), &mut ctx)
            .unwrap_or_else(|e| panic!("{name} {args}: dispatcher error {e}"));
        let ToolResult::Error(msg) = result else {
            panic!("{name} {args}: expected an error, got a success");
        };
        assert!(msg.contains("ONNX Runtime"), "{name} {args}: {msg}");
        assert!(
            msg.contains("not implemented in this build"),
            "{name} {args}: the error must not suggest the runtime is the missing piece: {msg}"
        );
        assert_eq!(
            msg.matches("not implemented in this build").count(),
            1,
            "{name} {args}: it should say so once: {msg}"
        );
        // Whatever the agent is told it may act on. Pointing it at a local
        // copy of the library is a step it would try, and it gets nowhere.
        assert!(
            !msg.contains("ORT_DYLIB_PATH"),
            "{name} {args}: the error offers the agent a setup step: {msg}"
        );
    }
}
