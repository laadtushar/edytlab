//! `transcribe` and `separate_stems` without an ONNX Runtime library (#383).
//!
//! Point the model variables at any path that exists and the tool tries to
//! load it (`WHISPER_MODEL_PATH` names a model folder and `DEMUCS_MODEL_PATH` the
//! folder of the htdemucs model; this test gives both a file, because the
//! runtime is looked for before the path is looked at). With `ort`'s `load-dynamic`, that used to hang inside
//! `ToolDispatcher::invoke` when the library was missing (`ort` 2.0.0-rc.12
//! deadlocked on a library it cannot load; rc.13 panics instead), and the
//! agent loop calls `invoke` holding the dispatcher, store and engine
//! mutexes, so one `transcribe` call froze the agent.
//!
//! Both tools must answer with a `ToolResult::Error` instead. Both run (#384,
//! #385), so the error is the setup problem and says what the user does: point
//! `ORT_DYLIB_PATH` at a copy (a build from source has none). `transcribe`'s
//! comes under the marker the window turns into a card; `separate_stems`'s has
//! none (the card speaks of the speech model) and tells the agent to leave it
//! to the user, as `diarize`'s does.
//!
//! A single test in its own binary, because it edits the process
//! environment and its watchdog ends the whole process.

use serde_json::json;
use std::io::Write;
use std::time::Duration;
use tools::ToolResult;

#[path = "support/harness.rs"]
mod harness;
use harness::{write_sine, Session};

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
            "a model tool did not return within 60 s without ONNX Runtime: a library that cannot \
             be loaded must be answered with an error by ml_pipeline::runtime::ensure (`ort` rc.12 \
             deadlocked here)"
        );
        std::process::abort();
    });
}

#[test]
fn model_tools_report_a_missing_runtime_instead_of_hanging() {
    watchdog();
    let mut s = Session::empty();
    let wav = s.dir.path().join("in.wav");
    write_sine(&wav, 16_000, 440.0, 0.25, 16_000);
    // Exists, so the tools get past "model not found" and reach the load.
    let model = s.dir.path().join("model.onnx");
    std::fs::write(&model, b"not an onnx model").expect("write");

    // SAFETY: this is the only test in this binary, so no other thread
    // reads the environment while it is edited.
    unsafe {
        std::env::remove_var("ORT_DYLIB_PATH");
        std::env::set_var("WHISPER_MODEL_PATH", &model);
        std::env::set_var("DEMUCS_MODEL_PATH", &model);
    }

    // `transcribe` needs a session to put its transcript in.
    let loaded = s
        .try_call("load", json!({ "path": wav.to_string_lossy() }))
        .expect("load dispatches");
    assert!(
        matches!(loaded, ToolResult::Ok(_)),
        "load failed: {loaded:?}"
    );

    // The marker is the tool's, not a string this file spells.
    let result = s
        .try_call("transcribe", json!({ "track": 0 }))
        .expect("transcribe dispatches");
    let ToolResult::Error(msg) = result else {
        panic!("transcribe: expected an error, got a success");
    };
    assert!(
        msg.starts_with(tools::tool::transcribe::MARKER_RUNTIME_UNAVAILABLE),
        "transcribe: {msg}"
    );
    assert!(msg.contains("ONNX Runtime"), "transcribe: {msg}");
    assert!(
        msg.contains("ORT_DYLIB_PATH"),
        "transcribe: a build from source is told how to point at a copy: {msg}"
    );
    assert!(
        !msg.contains("not implemented"),
        "transcribe runs once the runtime is there: {msg}"
    );
    assert!(
        !msg.contains(&s.dir.path().to_string_lossy().to_string()),
        "transcribe: the answer goes to the model provider, and names no local path: {msg}"
    );

    let calls = [
        ("separate_stems", json!({ "path": wav.to_string_lossy() })),
        (
            "separate_stems",
            json!({ "path": wav.to_string_lossy(), "model": "htdemucs" }),
        ),
    ];
    for (name, args) in calls {
        let result = s
            .try_call(name, args.clone())
            .unwrap_or_else(|e| panic!("{name} {args}: dispatcher error {e}"));
        let ToolResult::Error(msg) = result else {
            panic!("{name} {args}: expected an error, got a success");
        };
        assert!(msg.contains("ONNX Runtime"), "{name} {args}: {msg}");
        assert!(
            !msg.contains("not implemented"),
            "{name} {args}: stem separation runs once the runtime is there: {msg}"
        );
        // Like `diarize`'s: the user is told how to point at a copy (a build
        // from source has none), the agent is told to leave it to them, and
        // the answer names no local path and carries no setup marker (the
        // card speaks of the speech model).
        assert!(msg.contains("ORT_DYLIB_PATH"), "{name} {args}: {msg}");
        assert!(msg.contains("Tell the user"), "{name} {args}: {msg}");
        assert!(
            msg.contains("Do not try to install"),
            "{name} {args}: {msg}"
        );
        assert!(
            !msg.contains(&s.dir.path().to_string_lossy().to_string()),
            "{name} {args}: the answer goes to the model provider, and names no local path: {msg}"
        );
        assert!(
            !msg.starts_with(tools::tool::transcribe::MARKER_RUNTIME_UNAVAILABLE),
            "{name} {args}: {msg}"
        );
    }
}
