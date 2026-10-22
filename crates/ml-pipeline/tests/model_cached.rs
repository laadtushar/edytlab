//! `ml_pipeline::models::cached`: the read-only way to find a model (#383).
//!
//! A tool that needs the speech model (#384) must be able to find it
//! without being able to download it, and `tests/only_the_user_downloads.rs`
//! keeps the downloader out of every crate but this one. So the lookup is a
//! free function over a folder the app registers once at launch. It is a
//! process-wide setting, so this file has the one test that sets it.
//! `ModelDownloads::cached`, which shares its code, is tested against a local
//! server in `model_downloads.rs`, where a model really is on disk.

use ml_pipeline::models::{cached, set_models_dir, whisper_base, WHISPER_BASE_ID};

#[test]
fn cached_is_none_until_a_folder_is_registered_and_the_files_verify() {
    // No folder yet: nothing to find, and nothing created.
    assert_eq!(cached(WHISPER_BASE_ID), None);

    let tmp = tempfile::tempdir().expect("tempdir");
    let models = tmp.path().join("models");
    assert!(set_models_dir(models.clone()));
    assert!(
        !set_models_dir(tmp.path().join("elsewhere")),
        "the first registration is kept"
    );

    // Nothing downloaded.
    assert_eq!(cached(WHISPER_BASE_ID), None);
    assert!(!models.exists(), "a lookup creates nothing");

    // Files of the right names but not the pinned bytes: not verified.
    let dir = models.join(WHISPER_BASE_ID);
    for file in &whisper_base().files {
        let path = dir.join(&file.path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, vec![0u8; file.size as usize % 4096]).unwrap();
    }
    assert_eq!(cached(WHISPER_BASE_ID), None);

    // An id that is not in the catalog is not found either.
    assert_eq!(cached("../whisper-base"), None);
    assert_eq!(cached("no-such-model"), None);
}
