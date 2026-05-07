//! Smoke tests for `ml-pipeline`.
//!
//! Tests that build a real ORT session are gated on `ORT_DYLIB_PATH`
//! being set (load-dynamic). When the env var is absent we print a
//! `cargo test`-friendly notice and exit 0 so the Linux sandbox stays
//! green. The macOS and Windows CI matrices set the env var and run
//! the gated tests for real.
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

/// Locate the test-fixture ONNX model, if one exists. M17 ships without
/// one — generating a real ONNX export is M18+ territory — so this
/// returns `None` until a fixture lands.
fn try_find_fixture_model() -> Option<std::path::PathBuf> {
    // Convention: `crates/ml-pipeline/tests/fixtures/identity.onnx`.
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("identity.onnx");
    p.exists().then_some(p)
}

#[test]
fn model_registry_caches_arcs() {
    if env::var("ORT_DYLIB_PATH").is_err() {
        eprintln!("SKIP model_registry_caches_arcs: ORT_DYLIB_PATH unset (load-dynamic test gate)");
        return;
    }
    let Some(model_path) = try_find_fixture_model() else {
        eprintln!(
            "SKIP model_registry_caches_arcs: no fixture ONNX model at \
             tests/fixtures/identity.onnx (will be added in M18+)"
        );
        return;
    };

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
fn coreml_ep_falls_back_to_cpu_on_non_mac() {
    if env::var("ORT_DYLIB_PATH").is_err() {
        eprintln!("SKIP coreml_ep_falls_back_to_cpu_on_non_mac: ORT_DYLIB_PATH unset");
        return;
    }
    let Some(model_path) = try_find_fixture_model() else {
        eprintln!("SKIP coreml_ep_falls_back_to_cpu_on_non_mac: no fixture ONNX model");
        return;
    };

    // On non-mac targets the CoreML branch warns and falls through to
    // CPU; on mac it actually loads CoreML. Both should produce a
    // valid Arc<Session> and not panic.
    let registry = ModelRegistry::new();
    let s = registry
        .load("fixture-coreml", &model_path, ExecProvider::CoreML)
        .expect("CoreML EP must not panic on any target");
    assert!(std::sync::Arc::strong_count(&s) >= 1);
}
