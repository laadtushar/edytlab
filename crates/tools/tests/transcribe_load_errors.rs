//! `transcribe` with an ONNX Runtime library present, and a model that is not
//! there or does not load (#384).
//!
//! `ml_runtime_missing.rs` covers the machine with no runtime, which is every
//! machine CI has. With one, the tool gets past the runtime check and meets the
//! model. Each way that can go wrong is a setup problem the user fixes, and the
//! tool answers it under its marker (the window turns that into a card) with
//! what the user does:
//!
//! * `WHISPER_MODEL_PATH` names a file, a folder that is not there, or a folder
//!   whose files are not the export: `[speech_model_path_invalid]`;
//! * it is not set and no model was downloaded (no folder is registered, which
//!   is what the CLI and this test are): `[speech_model_not_downloaded]`.
//!
//! The pure mapping is unit-tested in `src/tool/transcribe.rs`, in CI. This
//! is the same through the real tool, the real runtime and the real loader.
//!
//! `#[ignore]`d because it needs the library (`ORT_DYLIB_PATH`,
//! `scripts/fetch-ml-runtime.sh`); asked for with `--ignored` it **fails** if
//! that is not set, rather than passing without having run anything (#263).
//!
//! ```text
//! ORT_DYLIB_PATH=/path/to/libonnxruntime.so \
//!   cargo test -p tools --test transcribe_load_errors -- --ignored
//! ```
//!
//! A single test in its own binary, because it edits the process environment.

use std::path::Path;

use serde_json::json;
use std::io::Write;
use std::time::Duration;
use tools::tool::transcribe::{MARKER_MODEL_NOT_DOWNLOADED, MARKER_MODEL_PATH_INVALID};
use tools::ToolResult;

#[path = "support/harness.rs"]
mod harness;
use harness::{write_sine, Session};

/// End the run, rather than stall it, if a tool never returns.
fn watchdog() {
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(120));
        let _ = writeln!(std::io::stderr(), "transcribe did not return within 120 s");
        std::process::abort();
    });
}

#[test]
#[ignore = "needs ONNX Runtime: set ORT_DYLIB_PATH to the library and run with --ignored"]
fn a_model_that_is_not_there_is_a_setup_problem_the_user_can_fix() {
    watchdog();
    let library = std::env::var_os("ORT_DYLIB_PATH").unwrap_or_else(|| {
        panic!(
            "ORT_DYLIB_PATH is not set, so this test cannot run. It was asked for with --ignored, \
             and a skip is not an answer (#263). scripts/fetch-ml-runtime.sh <platform> <dir> \
             prints the library."
        )
    });
    assert!(
        Path::new(&library).is_file(),
        "ORT_DYLIB_PATH is {library:?}, which is not a file"
    );

    let mut s = Session::empty();
    let wav = s.dir.path().join("in.wav");
    write_sine(&wav, 16_000, 440.0, 0.25, 16_000);

    // 1. A file where the folder belongs: what WHISPER_MODEL_PATH used to name.
    let file = s.dir.path().join("model.onnx");
    std::fs::write(&file, b"not an onnx model").expect("write");

    // 2. A folder with the three files, and a tokenizer that is not JSON.
    let folder = s.dir.path().join("whisper");
    std::fs::create_dir_all(folder.join("onnx")).expect("mkdir");
    std::fs::write(folder.join(ml_whisper::ENCODER_FILE), b"x").expect("write");
    std::fs::write(folder.join(ml_whisper::DECODER_FILE), b"x").expect("write");
    std::fs::write(folder.join(ml_whisper::TOKENIZER_FILE), b"this is not json").expect("write");

    // 3. A folder with the files missing.
    let empty = s.dir.path().join("empty");
    std::fs::create_dir_all(&empty).expect("mkdir");

    // 4. A folder that is not there.
    let gone = s.dir.path().join("gone");

    let loaded = s
        .try_call("load", json!({ "path": wav.to_string_lossy() }))
        .expect("load dispatches");
    assert!(
        matches!(loaded, ToolResult::Ok(_)),
        "load failed: {loaded:?}"
    );

    for (what, path) in [
        ("a .onnx file", &file),
        ("a folder with a tokenizer that does not parse", &folder),
        ("a folder with no model in it", &empty),
        ("a folder that is not there", &gone),
    ] {
        // SAFETY: this is the only test in this binary, so no other thread
        // reads the environment while it is edited.
        unsafe { std::env::set_var("WHISPER_MODEL_PATH", path) };
        let result = s
            .try_call("transcribe", json!({ "track": 0 }))
            .unwrap_or_else(|e| panic!("{what}: dispatcher error {e}"));
        let ToolResult::Error(msg) = result else {
            panic!("{what}: expected an error, got a success");
        };
        assert!(msg.starts_with(MARKER_MODEL_PATH_INVALID), "{what}: {msg}");
        assert!(
            msg.contains("WHISPER_MODEL_PATH") && msg.contains("unset"),
            "{what}: the answer does not say what to do: {msg}"
        );
        // The load error names files and paths. It is logged, not handed on.
        assert!(
            !msg.contains("protobuf")
                && !msg.contains("expected Whisper export")
                && !msg.contains(&s.dir.path().to_string_lossy().to_string()),
            "{what}: the answer hands the agent a load error or a path: {msg}"
        );
    }

    // Nothing set, and no folder registered: the user has not downloaded it.
    // SAFETY: as above.
    unsafe { std::env::remove_var("WHISPER_MODEL_PATH") };
    let result = s
        .try_call("transcribe", json!({ "track": 0 }))
        .expect("dispatches");
    let ToolResult::Error(msg) = result else {
        panic!("expected an error, got a success");
    };
    assert!(msg.starts_with(MARKER_MODEL_NOT_DOWNLOADED), "{msg}");
    assert!(msg.contains("Settings → Account"), "{msg}");
}
