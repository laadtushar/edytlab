//! Smoke tests for `ml-demucs`.
//!
//! Real inference is gated on (a) an HT-Demucs ONNX export, (b) a
//! license-clean reference clip; both land in M28. Until then this
//! file covers the cache-key contract and the `separate` stub so the
//! tool layer can be wired with confidence.
//!
//! When `assets/models/htdemucs.onnx` lands, the
//! `separate_returns_not_implemented_in_phase2_sandbox` test inverts
//! into a real correlation check against the golden stems in
//! `tests/golden/stems_30s_pop_clip/`.

use std::path::Path;

use audio_decoder::DecodedAudio;
use ml_demucs::{
    hash_decoded_audio, is_oom_error, read_cached_stems, stem_dir_for, write_stems_to_dir,
    DemucsError, DemucsModel, StemBuffer, StemPaths, StemSeparationResult, STEM_FILENAMES,
    STEM_NAMES,
};
use ml_pipeline::ContentHash;

fn audio() -> DecodedAudio {
    DecodedAudio {
        samples: vec![0.0, 0.05, -0.05, 0.1, -0.1, 0.15],
        sample_rate: 44_100,
        channels: 2,
    }
}

#[test]
fn cache_key_combines_input_and_model() {
    // Acceptance criterion #2: same input + same model -> same key.
    // Different model -> different key.
    let input_hash = hash_decoded_audio(&audio());
    let model_a = ContentHash::from_bytes(b"htdemucs_ft-export-v1");
    let model_b = ContentHash::from_bytes(b"htdemucs-export-v1");

    let key_a = ContentHash::combine(&[input_hash, model_a]);
    let key_b = ContentHash::combine(&[input_hash, model_b]);
    assert_ne!(
        key_a, key_b,
        "different model hashes must produce different stem-cache keys"
    );

    // Same input, same model -> stable key. This is what the
    // tool layer relies on for "second invocation hits the cache".
    let key_a2 = ContentHash::combine(&[hash_decoded_audio(&audio()), model_a]);
    assert_eq!(key_a, key_a2);
}

#[test]
fn stem_paths_struct_serializes_round_trip() {
    // The tool's `ToolResult::Ok` shape is contract — pin it.
    let dir = Path::new("/proj/.audiograph/stem-cache/deadbeef");
    let paths = StemPaths::for_dir(dir);
    let result: StemSeparationResult = paths.clone().into();
    let json = serde_json::to_value(&result).unwrap();
    assert_eq!(
        json["vocals"].as_str().unwrap(),
        dir.join("vocals.wav").to_string_lossy()
    );
    assert_eq!(
        json["drums"].as_str().unwrap(),
        dir.join("drums.wav").to_string_lossy()
    );
    assert_eq!(
        json["bass"].as_str().unwrap(),
        dir.join("bass.wav").to_string_lossy()
    );
    assert_eq!(
        json["other"].as_str().unwrap(),
        dir.join("other.wav").to_string_lossy()
    );

    // Round-trip back to the struct.
    let back: StemSeparationResult = serde_json::from_value(json).unwrap();
    assert_eq!(back.vocals, paths.vocals);
}

#[test]
fn separate_returns_not_implemented_in_phase2_sandbox() {
    // This test is the manual gate: when assets/models/htdemucs.onnx
    // lands, `DemucsModel::load` will succeed and we'll need to
    // invert this to assert `separate` returns four stems.
    //
    // Until then we can't construct a real `DemucsModel` — the load
    // path requires the .onnx file. The contract we *can* assert is
    // that the model-missing branch errors with a structured
    // `ModelMissing` variant rather than panicking.
    let model_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("assets")
        .join("models")
        .join("htdemucs.onnx");

    if model_path.exists() {
        // M28 has landed: this should be a real correlation check
        // against tests/golden/stems_30s_pop_clip/. Surface a clear
        // failure pointing at the next step rather than silently
        // passing.
        panic!(
            "assets/models/htdemucs.onnx exists ({}) — flip this test \
             from SKIP to a real RMS-correlation assertion against \
             tests/golden/stems_30s_pop_clip/",
            model_path.display()
        );
    }

    let err = DemucsModel::load("htdemucs", &model_path).unwrap_err();
    match err {
        DemucsError::ModelMissing { .. } => {}
        other => panic!("expected ModelMissing, got {other:?}"),
    }
}

#[test]
fn stem_filenames_match_stem_names() {
    // Sanity: the two parallel arrays line up. Catches a typo
    // refactoring `STEM_NAMES` without `STEM_FILENAMES`.
    for (i, name) in STEM_NAMES.iter().enumerate() {
        assert_eq!(STEM_FILENAMES[i], format!("{name}.wav"));
    }
}

#[test]
fn read_cached_stems_returns_some_only_when_all_present() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = stem_dir_for(tmp.path(), "deadbeef");

    // Empty dir -> None.
    assert!(read_cached_stems(&dir).is_none());

    // Write stems.
    let stems = [
        StemBuffer {
            name: "vocals",
            samples: vec![0.0, 0.0],
            sample_rate: 44_100,
            channels: 2,
        },
        StemBuffer {
            name: "drums",
            samples: vec![0.0, 0.0],
            sample_rate: 44_100,
            channels: 2,
        },
        StemBuffer {
            name: "bass",
            samples: vec![0.0, 0.0],
            sample_rate: 44_100,
            channels: 2,
        },
        StemBuffer {
            name: "other",
            samples: vec![0.0, 0.0],
            sample_rate: 44_100,
            channels: 2,
        },
    ];
    let _ = write_stems_to_dir(&dir, &stems).unwrap();
    assert!(read_cached_stems(&dir).is_some(), "expected cache hit");

    // Remove one file -> miss again.
    std::fs::remove_file(dir.join("bass.wav")).unwrap();
    assert!(
        read_cached_stems(&dir).is_none(),
        "missing bass.wav must invalidate the hit"
    );
}

#[test]
fn oom_detection_drives_fallback_policy() {
    // Acceptance criterion #4 contract: the OOM detection helper is
    // what the tool uses to decide "retry with htdemucs". A unit test
    // here is the closest we can get without a contrived ONNX
    // fixture that synthesises an OOM at runtime.
    let oom = DemucsError::Ort("CUDA out of memory: tried to allocate 1.5 GiB".into());
    assert!(is_oom_error(&oom));

    // A non-OOM ORT error must NOT trigger the fallback.
    let other = DemucsError::Ort("graph optimization failed: shape mismatch".into());
    assert!(!is_oom_error(&other));

    // And the helper must not match unrelated error variants even
    // when their `Display` happens to contain the OOM substring.
    let invalid = DemucsError::InvalidAudio("out of memory in 2nd buffer".into());
    assert!(!is_oom_error(&invalid));
}
