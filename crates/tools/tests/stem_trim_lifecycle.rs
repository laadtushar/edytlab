//! Opt-in test of real `DemucsStems` cleanup on Linux/glibc (#868).
//!
//! Requires `ORT_DYLIB_PATH` and a C compiler named `cc`. The tiny checked-in
//! ONNX fixture stands in for the network; it does not test separation quality.
//! Run exactly this test with:
//! `cargo test -p tools --test stem_trim_lifecycle -- --ignored --exact real_model_results_request_one_trim --nocapture`

#![cfg(all(target_os = "linux", target_env = "gnu"))]

use std::ffi::{c_char, c_void};
use std::path::PathBuf;
use std::process::Command;

use audio_decoder::DecodedAudio;
use ml_demucs::Control;
use tools::tool::separate_stems::{DemucsStems, ModelRef, StemFailure, StemModel};

const CHILD_ENV: &str = "EDYTLAB_STEM_TRIM_TEST_CHILD";

#[link(name = "dl")]
extern "C" {
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}

fn trim_count() -> std::os::raw::c_ulong {
    // SAFETY: the subprocess preloads the test library exporting this symbol;
    // RTLD_DEFAULT is null on glibc, and the NUL-terminated name is static.
    let symbol = unsafe { dlsym(std::ptr::null_mut(), c"stem_test_trim_calls".as_ptr()) };
    assert!(!symbol.is_null(), "test trim interposer was not loaded");
    // SAFETY: the fixture exports this exact no-argument C function returning
    // unsigned long. POSIX permits dlsym results to name functions.
    let count: unsafe extern "C" fn() -> std::os::raw::c_ulong =
        unsafe { std::mem::transmute(symbol) };
    // SAFETY: the function takes no arguments and only reads an atomic counter.
    unsafe { count() }
}

fn audio(rate: u32) -> DecodedAudio {
    DecodedAudio {
        samples: (0..88_200)
            .map(|i| ((i as f32) * 0.031).sin() * 0.2)
            .collect(),
        sample_rate: rate,
        channels: 2,
    }
}

fn child_case() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../ml-demucs/tests/fixtures/htdemucs_split");
    let model = ModelRef {
        folder: fixture,
        fingerprint: "stem-trim-lifecycle-fixture".to_string(),
        downloaded: false,
    };
    let dir = tempfile::tempdir().expect("temporary output directory");
    let stems = DemucsStems;
    let before = trim_count();

    let output = stems
        .separate(
            &model,
            audio(44_100),
            &dir.path().join("ok"),
            &Control::none(),
        )
        .expect("synthetic model should separate");
    assert!(output.paths.vocals.is_file());
    assert_eq!(trim_count(), before + 1, "one cleanup after success");

    let cancel = Control {
        cancel: &|| true,
        progress: &|_, _| {},
    };
    let cancelled = stems
        .separate(&model, audio(44_100), &dir.path().join("cancel"), &cancel)
        .expect_err("cancelled call must not succeed");
    assert_eq!(cancelled, StemFailure::Cancelled);
    assert!(!dir.path().join("cancel").exists());
    assert_eq!(trim_count(), before + 2, "one cleanup after cancellation");

    let error = stems
        .separate(
            &model,
            audio(0),
            &dir.path().join("invalid"),
            &Control::none(),
        )
        .expect_err("invalid sample rate must fail");
    assert!(matches!(error, StemFailure::Failed(_)), "{error:?}");
    assert!(!dir.path().join("invalid").exists());
    assert_eq!(trim_count(), before + 3, "one cleanup after an error");
}

#[test]
#[ignore = "requires ORT_DYLIB_PATH and a C compiler; tests real model lifecycle"]
fn real_model_results_request_one_trim() {
    if std::env::var_os(CHILD_ENV).is_some() {
        child_case();
        return;
    }

    let runtime = std::env::var_os("ORT_DYLIB_PATH").expect("ORT_DYLIB_PATH is required");
    assert!(
        PathBuf::from(&runtime).is_file(),
        "ORT_DYLIB_PATH must name a file"
    );
    let dir = tempfile::tempdir().expect("temporary interposer directory");
    let library = dir.path().join("trim-interposer.so");
    let source =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/stem_trim_interposer.c");
    let status = Command::new("cc")
        .args([
            "-std=c11", "-shared", "-fPIC", "-O2", "-Wall", "-Wextra", "-Werror", "-o",
        ])
        .arg(&library)
        .arg(source)
        .status()
        .expect("the cc compiler is required");
    assert!(status.success(), "building the test interposer failed");

    let mut preload = library.into_os_string();
    if let Some(existing) = std::env::var_os("LD_PRELOAD") {
        if !existing.is_empty() {
            preload.push(":");
            preload.push(existing);
        }
    }
    let status = Command::new(std::env::current_exe().expect("test executable"))
        .args([
            "--ignored",
            "--exact",
            "real_model_results_request_one_trim",
            "--nocapture",
        ])
        .env(CHILD_ENV, "1")
        .env("LD_PRELOAD", preload)
        .status()
        .expect("starting the instrumented test subprocess");
    assert!(
        status.success(),
        "instrumented lifecycle test failed: {status}"
    );
}
