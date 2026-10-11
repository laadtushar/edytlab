//! A missing ONNX Runtime library is an error, not a hang or a panic (#383).
//!
//! `WhisperModel::load` used to call `Session::builder()` for any model
//! file that existed, and `ort` 2.0.0-rc.12 deadlocked when it could not
//! load the runtime (rc.13 panics; see `ml_pipeline::runtime`). This test
//! never returned before `ml_pipeline::runtime::ensure` was put in front
//! of it; the watchdog turns a stall into a failure.
//!
//! One test per binary on purpose: it edits the process environment
//! (`ORT_DYLIB_PATH`), which nothing else here may observe, and the
//! watchdog ends the whole process.

use std::io::Write;
use std::time::Duration;

use ml_whisper::{WhisperError, WhisperModel};

/// Fail the run, rather than stall it, if the load never returns.
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
            "loading a Whisper model without ONNX Runtime did not return within 60 s: a library \
             that cannot be loaded must be answered with an error (`ort` rc.12 deadlocked here)"
        );
        std::process::abort();
    });
}

#[test]
fn loading_a_model_without_the_runtime_is_an_error_not_a_hang() {
    // SAFETY: this is the only test in this binary, so no other thread
    // reads the environment while it is edited.
    unsafe {
        std::env::remove_var("ORT_DYLIB_PATH");
    }
    watchdog();

    let dir = tempfile::tempdir().expect("tempdir");
    let model = dir.path().join("whisper.onnx");
    std::fs::write(&model, b"not an onnx model").expect("write");

    let err = WhisperModel::load(&model).expect_err("there is no runtime to load a model with");
    let WhisperError::RuntimeUnavailable(reason) = &err else {
        panic!("expected RuntimeUnavailable, got {err:?}");
    };
    assert!(reason.contains("ONNX Runtime"), "{reason}");

    // Installing the runtime would not make transcription work, and the
    // message must not let anyone think it would.
    let msg = err.to_string();
    assert!(msg.contains("not implemented in this build"), "{msg}");
}
