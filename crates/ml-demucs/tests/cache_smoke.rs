//! Smoke tests for the parts of `ml-demucs` that need no model: the cache
//! layout the `separate_stems` tool relies on, and the result shape.

use std::path::Path;

use ml_demucs::{
    is_oom_error, read_cached_stems, stem_dir_for, DemucsError, Pending, StemPaths,
    StemSeparationResult, STEM_FILENAMES, STEM_NAMES,
};

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

    // Nothing there -> None.
    assert!(read_cached_stems(&dir).is_none());

    // An entry is made whole or not at all: a hit appears when the
    // pending directory is published.
    let pending = Pending::create(&dir).unwrap();
    for file in StemPaths::for_dir(pending.dir()).iter() {
        std::fs::write(file, b"RIFF").unwrap();
    }
    assert!(read_cached_stems(&dir).is_none(), "not yet published");
    pending.publish().unwrap();
    assert!(read_cached_stems(&dir).is_some(), "expected cache hit");

    // Remove one file -> miss again.
    std::fs::remove_file(dir.join("bass.wav")).unwrap();
    assert!(
        read_cached_stems(&dir).is_none(),
        "missing bass.wav must invalidate the hit"
    );
}

#[test]
fn oom_detection_tells_out_of_memory_from_other_failures() {
    // The OOM detection helper is what the tool uses to report an
    // out-of-memory failure as one. There is no retry: htdemucs is the
    // only model, so there is nothing smaller to fall back to.
    let oom = DemucsError::Ort("CUDA out of memory: tried to allocate 1.5 GiB".into());
    assert!(is_oom_error(&oom));

    // A non-OOM ORT error must NOT be reported as one.
    let other = DemucsError::Ort("graph optimization failed: shape mismatch".into());
    assert!(!is_oom_error(&other));

    // And the helper must not match unrelated error variants even
    // when their `Display` happens to contain the OOM substring.
    let invalid = DemucsError::InvalidAudio("out of memory in 2nd buffer".into());
    assert!(!is_oom_error(&invalid));
}
