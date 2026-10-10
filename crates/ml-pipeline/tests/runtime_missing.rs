//! A missing ONNX Runtime library is an error, not a hang (#383).
//!
//! With `load-dynamic`, `ort` deadlocks when it cannot load the library
//! (see `ml_pipeline::runtime`). `ModelRegistry::load` used to reach that
//! for any model file that existed, so this test never returned before
//! `runtime::ensure` was in front of it. The watchdog turns that into a
//! failure.
//!
//! One test per binary on purpose: it edits the process environment
//! (`ORT_DYLIB_PATH`), which nothing else here may observe, and the
//! watchdog ends the whole process.

use std::io::Write;
use std::time::Duration;

use ml_pipeline::{Error, ExecProvider, ModelRegistry};

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
            "loading a model without ONNX Runtime did not return within 60 s: `ort` deadlocks on \
             a library it cannot load, which runtime::ensure exists to prevent"
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
    let model = dir.path().join("garbage.onnx");
    std::fs::write(&model, b"not an onnx model").expect("write");

    let registry = ModelRegistry::new();
    let err = registry
        .load("garbage", &model, ExecProvider::Cpu)
        .expect_err("there is no runtime to load a model with");

    let Error::MissingRuntime { searched } = &err else {
        panic!("expected MissingRuntime, got {err:?}");
    };
    assert!(
        !searched.is_empty(),
        "the error must say where it looked, so the user knows where a copy would be found"
    );
    assert!(err.to_string().contains("ONNX Runtime"), "{err}");

    // The failed load left nothing behind that would answer for the
    // model later.
    assert!(registry.is_empty());
}
