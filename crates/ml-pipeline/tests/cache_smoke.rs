//! Smoke tests for `ml-pipeline`.
//!
//! Tests that build a real ORT session need an ONNX Runtime library:
//! `ort` is linked `load-dynamic`, so `ORT_DYLIB_PATH` has to name one.
//! They are `#[ignore]`d, and `cargo test -p ml-pipeline -- --ignored`
//! runs them. The model is committed: `tests/fixtures/identity.onnx`, 91
//! bytes, one `Identity` node (`Y = X`, float32 `[1, 3]`), written by
//! `tests/fixtures/make_identity_onnx.py` beside it with no `onnx` package.
//!
//! **In CI** the Linux `build` job runs them against the runtime the app
//! pins. `apps/desktop/src-tauri/ml-runtime.json` names ONNX Runtime
//! 1.23.2 and its SHA-256; `scripts/fetch-ml-runtime.sh linux-x64
//! ml-runtime/linux-x64` downloads it (the job caches that directory on the
//! manifest's hash), and the job then runs
//!
//! ```text
//! ORT_DYLIB_PATH=<workspace>/ml-runtime/linux-x64/libonnxruntime.so \
//!     cargo test -p ml-pipeline --test cache_smoke -- --ignored --test-threads=1
//! ```
//!
//! The workspace `cargo test` step does not set the variable and does not
//! pass `--ignored`, so there these tests are listed as ignored. The macOS
//! and Windows jobs do not run them yet.
//!
//! **Locally**, the same two commands:
//!
//! ```text
//! export ORT_DYLIB_PATH="$(scripts/fetch-ml-runtime.sh linux-x64 ml-runtime/linux-x64)"
//! cargo test -p ml-pipeline --test cache_smoke -- --ignored
//! ```
//!
//! (`win-x64` and `osx-universal2` fetch the other platforms' libraries.)
//!
//! They used to check the library and the model at runtime and `return`
//! early, which `cargo test` counts as **passed** — the run printed `6
//! passed` while two of the six had exited on their first line. `#[ignore]`
//! says so in the output instead, and asking for them explicitly fails on
//! a missing library rather than passing quietly.
//!
//! Pure-cache tests run unconditionally — they don't touch ORT.

use std::cell::Cell;
use std::env;
use std::path::Path;

use ml_pipeline::{ContentHash, ExecProvider, InferenceCache, ModelRegistry};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct AnalysisFixture {
    sample_count: u64,
    peak: f32,
}

#[test]
fn inference_cache_round_trips() {
    let tmp = tempfile::tempdir().expect("tmpdir");
    let cache = InferenceCache::open(tmp.path()).expect("open cache");

    let key = ContentHash::from_bytes(b"input-bytes-v1");
    let computed = Cell::new(0u32);

    let first = cache
        .get_or_compute::<AnalysisFixture, _>(key, || {
            computed.set(computed.get() + 1);
            Ok(AnalysisFixture {
                sample_count: 44_100,
                peak: 0.95,
            })
        })
        .expect("first compute");
    assert_eq!(computed.get(), 1, "compute should run on first miss");
    assert_eq!(first.sample_count, 44_100);

    // Re-open the cache from a fresh handle to prove persistence
    // across instances, not just across calls.
    let cache2 = InferenceCache::open(tmp.path()).expect("reopen cache");
    let second = cache2
        .get_or_compute::<AnalysisFixture, _>(key, || {
            computed.set(computed.get() + 1);
            // If this runs the test fails — the value below is wrong on
            // purpose so a regression where compute runs twice is
            // caught even if the assertion below is removed.
            Ok(AnalysisFixture {
                sample_count: 0,
                peak: 0.0,
            })
        })
        .expect("second compute");
    assert_eq!(computed.get(), 1, "compute must NOT run on cache hit");
    assert_eq!(second, first, "cached value must round-trip exactly");

    // The on-disk path is content-addressed and lives under the
    // documented layout.
    let expected_path = tmp
        .path()
        .join(".audiograph")
        .join("inference-cache")
        .join(format!("{key}.bin"));
    assert!(
        expected_path.exists(),
        "expected cache file at {}",
        expected_path.display()
    );
}

#[test]
fn cache_keys_change_on_model_hash_change() {
    // Acceptance criterion #3: invalidate on model file change. We
    // model that by folding the model hash into the input-bytes hash
    // via `ContentHash::combine`. Two different "model" hashes must
    // produce different keys (and therefore different on-disk paths).
    let tmp = tempfile::tempdir().expect("tmpdir");
    let cache = InferenceCache::open(tmp.path()).expect("open cache");

    let input = ContentHash::from_bytes(b"some-audio");
    let model_a = ContentHash::from_bytes(b"model-weights-v1");
    let model_b = ContentHash::from_bytes(b"model-weights-v2");

    let key_a = ContentHash::combine(&[input, model_a]);
    let key_b = ContentHash::combine(&[input, model_b]);

    assert_ne!(
        key_a, key_b,
        "different model hashes must produce different keys"
    );
    assert_ne!(
        cache.path_for(key_a),
        cache.path_for(key_b),
        "different keys must map to different on-disk paths"
    );

    // Also confirm same-input-same-model is stable.
    let key_a2 = ContentHash::combine(&[input, model_a]);
    assert_eq!(key_a, key_a2);
}

#[test]
fn cache_invalidate_removes_entry() {
    let tmp = tempfile::tempdir().expect("tmpdir");
    let cache = InferenceCache::open(tmp.path()).expect("open cache");
    let key = ContentHash::from_bytes(b"to-invalidate");

    let _ = cache
        .get_or_compute::<u32, _>(key, || Ok(42))
        .expect("seed cache");
    assert!(cache.path_for(key).exists());
    cache.invalidate(key).expect("invalidate");
    assert!(!cache.path_for(key).exists());
    // Idempotent: invalidating a missing key is a no-op.
    cache.invalidate(key).expect("invalidate again is no-op");
}

#[test]
fn content_hash_hex_is_lowercase_64_chars() {
    let h = ContentHash::from_bytes(b"hello");
    let s = h.to_hex();
    assert_eq!(s.len(), 64);
    assert!(
        s.chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
        "hex must be lowercase ascii hex digits, got {s}"
    );
    assert_eq!(s, format!("{h}"), "Display must match to_hex");
}

/// The fixture ONNX model these tests load, or a panic naming what is
/// missing: the library `ORT_DYLIB_PATH` should name, or the model.
///
/// It panics rather than returning `None` because it is only reached when
/// someone asked for these tests by name, and a test that returns early
/// counts as passed.
fn fixture_model() -> std::path::PathBuf {
    let library = env::var("ORT_DYLIB_PATH").unwrap_or_else(|_| {
        panic!(
            "ORT_DYLIB_PATH is not set; ml-pipeline links ORT load-dynamic, so the ignored \
             tests need it to point at the ONNX Runtime shared library. \
             scripts/fetch-ml-runtime.sh <platform> <dir> fetches the pinned one."
        )
    });
    assert!(
        Path::new(&library).is_file(),
        "ORT_DYLIB_PATH={library} is not a file; scripts/fetch-ml-runtime.sh <platform> <dir> \
         fetches the pinned ONNX Runtime"
    );
    // `crates/ml-pipeline/tests/fixtures/identity.onnx`, made by
    // `make_identity_onnx.py` in the same directory.
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("identity.onnx");
    assert!(p.is_file(), "no fixture ONNX model at {}", p.display());
    p
}

#[test]
#[ignore = "needs ORT_DYLIB_PATH (an ONNX Runtime library); run with --ignored"]
fn model_registry_caches_arcs() {
    let model_path = fixture_model();

    let registry = ModelRegistry::new();
    let a = registry
        .load("fixture", &model_path, ExecProvider::Cpu)
        .expect("first load");
    let b = registry
        .load("fixture", &model_path, ExecProvider::Cpu)
        .expect("second load");
    assert!(
        std::sync::Arc::ptr_eq(&a, &b),
        "registry must return the same Arc for the same model_id"
    );
    assert_eq!(registry.len(), 1);
}

/// Run `identity.onnx` through a session and return what came out.
fn run_identity(session: &ml_pipeline::SharedSession) -> Vec<f32> {
    let mut session = session.lock().expect("session lock");
    let input = [3.0f32, -1.0, 0.5];
    let tensor =
        ort::value::Tensor::from_array(([1usize, 3], input.to_vec())).expect("input tensor");
    let outputs = session
        .run(ort::inputs!["X" => tensor])
        .expect("run identity.onnx through the registry's session");
    let (_, data) = outputs["Y"]
        .try_extract_tensor::<f32>()
        .expect("Y is a float tensor");
    data.to_vec()
}

#[test]
#[ignore = "needs ORT_DYLIB_PATH (an ONNX Runtime library); run with --ignored"]
fn a_replaced_file_drops_the_session_of_its_earlier_version() {
    // #658: a new version of a model file is a new session, and the old one
    // stayed in the registry for the life of the process.
    let model_path = fixture_model();
    let registry = ModelRegistry::new();
    let load = |family: &str, version: &str| {
        registry
            .load_versioned(family, version, &model_path, ExecProvider::Cpu)
            .expect("load")
    };

    let v1 = load("encoder", "100:1");
    let v1_weak = std::sync::Arc::downgrade(&v1);
    assert!(
        std::sync::Arc::ptr_eq(&v1, &load("encoder", "100:1")),
        "the same version is the same session"
    );
    assert_eq!(registry.len(), 1);

    // Another family and an id from plain `load` are not versions of it.
    let other_family = load("decoder", "100:1");
    let plain = registry
        .load("encoder-plain", &model_path, ExecProvider::Cpu)
        .expect("plain load");
    assert_eq!(registry.len(), 3);

    // The file is replaced: the new version is loaded, the old one dropped.
    drop(v1);
    assert!(
        v1_weak.upgrade().is_some(),
        "the registry keeps a session nobody is using, until its file is replaced"
    );
    let v2 = load("encoder", "100:2");
    assert_eq!(registry.len(), 3, "two sessions in the registry, not four");
    assert!(
        v1_weak.upgrade().is_none(),
        "nothing used the first session and the registry still held it"
    );
    assert_eq!(run_identity(&v2), [3.0, -1.0, 0.5]);

    // The other family and the plain id are untouched.
    assert!(std::sync::Arc::ptr_eq(
        &other_family,
        &load("decoder", "100:1")
    ));
    assert!(std::sync::Arc::ptr_eq(
        &plain,
        &registry
            .load("encoder-plain", &model_path, ExecProvider::Cpu)
            .expect("plain load again")
    ));
    assert_eq!(registry.len(), 3);

    // A holder of the old version keeps a working session after it is replaced.
    let in_use = load("encoder", "100:3");
    let in_use_weak = std::sync::Arc::downgrade(&in_use);
    let _v4 = load("encoder", "100:4");
    assert_eq!(registry.len(), 3);
    assert_eq!(run_identity(&in_use), [3.0, -1.0, 0.5]);
    drop(in_use);
    assert!(
        in_use_weak.upgrade().is_none(),
        "freed with its last holder"
    );
}

#[test]
#[ignore = "needs ORT_DYLIB_PATH (an ONNX Runtime library); run with --ignored"]
fn a_failed_load_of_a_new_version_evicts_nothing() {
    let model_path = fixture_model();
    let registry = ModelRegistry::new();
    let v1 = registry
        .load_versioned("encoder", "1", &model_path, ExecProvider::Cpu)
        .expect("load");
    let missing = Path::new("/definitely/does/not/exist.onnx");
    registry
        .load_versioned("encoder", "2", missing, ExecProvider::Cpu)
        .expect_err("the file is not there");
    assert_eq!(registry.len(), 1, "the earlier version is still loaded");
    assert!(std::sync::Arc::ptr_eq(
        &v1,
        &registry
            .load_versioned("encoder", "1", &model_path, ExecProvider::Cpu)
            .expect("load again")
    ));
}

#[test]
#[ignore = "needs ORT_DYLIB_PATH (an ONNX Runtime library); run with --ignored"]
fn a_session_from_the_registry_can_be_run() {
    // `ort` 2.0.0-rc.13's `Session::run` takes `&mut self`, and the registry
    // used to hand out a bare `Arc<Session>`, which nothing could run: the
    // Whisper decoder (#384) was its first would-be caller. The session now
    // comes behind a mutex; take the lock, run the model, and check the
    // second holder of the same Arc sees the same, working, session.
    let model_path = fixture_model();
    let registry = ModelRegistry::new();
    let shared = registry
        .load("fixture-run", &model_path, ExecProvider::Cpu)
        .expect("load");
    let again = registry
        .load("fixture-run", &model_path, ExecProvider::Cpu)
        .expect("load again");

    for handle in [&shared, &again] {
        let mut session = handle.lock().expect("session lock");
        let input = [3.0f32, -1.0, 0.5];
        let tensor =
            ort::value::Tensor::from_array(([1usize, 3], input.to_vec())).expect("input tensor");
        let outputs = session
            .run(ort::inputs!["X" => tensor])
            .expect("run identity.onnx through the registry's session");
        let (_, data) = outputs["Y"]
            .try_extract_tensor::<f32>()
            .expect("Y is a float tensor");
        assert_eq!(data, &input[..]);
    }
}

#[test]
#[ignore = "needs ORT_DYLIB_PATH (an ONNX Runtime library); run with --ignored"]
fn coreml_ep_falls_back_to_cpu_on_non_mac() {
    let model_path = fixture_model();

    // On non-mac targets the CoreML branch warns and falls through to
    // CPU; on mac it actually loads CoreML. Both should produce a
    // valid SharedSession and not panic.
    let registry = ModelRegistry::new();
    let s = registry
        .load("fixture-coreml", &model_path, ExecProvider::CoreML)
        .expect("CoreML EP must not panic on any target");

    // `strong_count(&s) >= 1` was the old assertion, and it is true of
    // any Arc the caller is holding — it would pass against a registry
    // that stored nothing. What the fallback has to produce is a
    // *cacheable* session, so ask the registry for it again.
    assert_eq!(registry.len(), 1, "the fallback session was not cached");
    let again = registry
        .load("fixture-coreml", &model_path, ExecProvider::CoreML)
        .expect("second load");
    assert!(
        std::sync::Arc::ptr_eq(&s, &again),
        "a second load rebuilt the session instead of returning the cached Arc"
    );
}

#[test]
#[ignore = "needs ORT_DYLIB_PATH (an ONNX Runtime library); run with --ignored"]
fn identity_model_returns_its_input() {
    // Loading a model proves the library opens; running it proves the
    // library computes. Y = Identity(X), so what comes out must be exactly
    // what went in, bit for bit, including a negative, a fraction and zero.
    let model_path = fixture_model();
    ml_pipeline::runtime::ensure().expect("the ONNX Runtime library ORT_DYLIB_PATH names");

    let mut session = ort::session::Session::builder()
        .expect("session builder")
        .commit_from_file(&model_path)
        .expect("load identity.onnx");
    let input = [1.0f32, -2.5, 0.0];
    let tensor =
        ort::value::Tensor::from_array(([1usize, 3], input.to_vec())).expect("input tensor");
    let outputs = session
        .run(ort::inputs!["X" => tensor])
        .expect("run identity.onnx");
    let (shape, data) = outputs["Y"]
        .try_extract_tensor::<f32>()
        .expect("Y is a float tensor");

    assert_eq!(&shape[..], &[1, 3], "Identity keeps the shape");
    assert_eq!(data, &input[..], "Identity must return its input");
}

#[test]
#[ignore = "needs ORT_DYLIB_PATH (an ONNX Runtime library); run with --ignored"]
fn runtime_info_reports_the_library_that_loaded() {
    // What Settings shows (`ml_runtime_status`): the file that loaded, the
    // version the library itself reports, and where it was found. The
    // version must be the one `ml-runtime.json` pins: a library of another
    // version would run, and the status line would say so.
    let library = env::var("ORT_DYLIB_PATH").unwrap_or_else(|_| {
        panic!("ORT_DYLIB_PATH is not set; scripts/fetch-ml-runtime.sh <platform> <dir> fetches it")
    });
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("apps")
        .join("desktop")
        .join("src-tauri")
        .join("ml-runtime.json");
    let manifest: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&manifest).expect("read the manifest"))
            .expect("the manifest is JSON");

    let info = ml_pipeline::runtime::info().expect("the ONNX Runtime library ORT_DYLIB_PATH names");

    assert_eq!(info.path, Path::new(&library), "the file that loaded");
    assert_eq!(
        Some(info.version.as_str()),
        manifest["version"].as_str(),
        "the library's own version string is the pinned version"
    );
    assert_eq!(info.origin, ml_pipeline::runtime::Origin::Environment);
    assert!(
        !info.bundled(),
        "ORT_DYLIB_PATH is not the installer's copy"
    );
    // The same answer from `ensure` and from a second `info`: one library
    // per process, loaded once.
    assert_eq!(ml_pipeline::runtime::ensure().expect("ensure"), info.path);
    assert_eq!(ml_pipeline::runtime::info().expect("again"), info);
}
