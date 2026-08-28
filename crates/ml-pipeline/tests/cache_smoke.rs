//! Smoke tests for `ml-pipeline`.
//!
//! Tests that build a real ORT session need two things this repo does
//! not currently have: `ORT_DYLIB_PATH` (load-dynamic) and a fixture
//! model at `tests/fixtures/identity.onnx`. No workflow sets the first
//! and the second is not committed, so they are `#[ignore]`d and run
//! with `cargo test -p ml-pipeline -- --ignored`.
//!
//! They used to check both at runtime and `return` early, which
//! `cargo test` counts as **passed** — the run printed `6 passed` while
//! two of the six had exited on their first line. `#[ignore]` says so in
//! the output instead, and asking for them explicitly now fails on the
//! missing prerequisite rather than passing quietly.
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
/// missing.
///
/// M17 shipped without one — generating a real ONNX export is M18+
/// territory — so today this always panics, which is why its callers are
/// `#[ignore]`d. It panics rather than returning `None` because it is
/// only reached when someone asked for these tests by name.
fn fixture_model() -> std::path::PathBuf {
    assert!(
        env::var("ORT_DYLIB_PATH").is_ok(),
        "ORT_DYLIB_PATH is not set; ml-pipeline links ORT load-dynamic, so the \
         ignored tests need it to point at the ONNX Runtime shared library"
    );
    // Convention: `crates/ml-pipeline/tests/fixtures/identity.onnx`.
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("identity.onnx");
    assert!(
        p.exists(),
        "no fixture ONNX model at {} (not committed yet; M18+)",
        p.display()
    );
    p
}

#[test]
#[ignore = "needs ORT_DYLIB_PATH and a fixture ONNX model; run with --ignored"]
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

#[test]
#[ignore = "needs ORT_DYLIB_PATH and a fixture ONNX model; run with --ignored"]
fn coreml_ep_falls_back_to_cpu_on_non_mac() {
    let model_path = fixture_model();

    // On non-mac targets the CoreML branch warns and falls through to
    // CPU; on mac it actually loads CoreML. Both should produce a
    // valid Arc<Session> and not panic.
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
