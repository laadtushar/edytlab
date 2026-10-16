//! The directory the app registers is searched by `ensure` (#383).
//!
//! `runtime::set_bundle_dir` is how the installer's layout reaches the
//! loader: the app's setup hook registers `Contents/Frameworks` on macOS and
//! Tauri's resource directory on Linux, and `ensure()` looks there before
//! it looks next to the executable. The unit tests in `runtime.rs` cover the
//! order over explicit inputs; this checks the real thing end to end, in a
//! process of its own because the directory is set once per process.
//!
//! The "library" is a file that is not one, so nothing real is loaded: that
//! the error names the bundle's copy shows `ensure` went there, and that it
//! stopped there (instead of falling through to the executable's directory,
//! where `current_exe()` has no such file anyway) is the existing rule that
//! a candidate that is there but fails is the answer.
//!
//! One test per binary on purpose: it edits the process environment
//! (`ORT_DYLIB_PATH`) and sets the once-only bundle directory.

use ml_pipeline::runtime;
use ml_pipeline::Error;

#[test]
fn the_registered_bundle_directory_is_searched_before_the_executable() {
    // SAFETY: this is the only test in this binary, so no other thread
    // reads the environment while it is edited.
    unsafe {
        std::env::remove_var("ORT_DYLIB_PATH");
    }

    // Before anything is registered, there is no bundle directory, and the
    // candidates are the executable's directory alone.
    assert_eq!(runtime::bundle_dir(), None);
    assert!(
        runtime::candidate_paths().len() <= 1,
        "only the executable's directory"
    );

    // A relative directory would make every `ensure` fail with "not an
    // absolute path", so it is refused up front and nothing is set.
    assert!(!runtime::set_bundle_dir("Frameworks".into()));
    assert_eq!(runtime::bundle_dir(), None);

    let dir = tempfile::tempdir().expect("tempdir");
    let bundle = dir.path().join("Frameworks");
    std::fs::create_dir_all(&bundle).expect("mkdir");
    let planted = bundle.join(runtime::library_file_name());
    std::fs::write(&planted, b"this is not a shared library").expect("write");

    assert!(
        runtime::set_bundle_dir(bundle.clone()),
        "first call sets it"
    );
    assert_eq!(runtime::bundle_dir(), Some(bundle.as_path()));
    // Once: a second registration (even of the same directory) is refused
    // and leaves the first in place, so nothing can redirect the search
    // after startup.
    assert!(!runtime::set_bundle_dir(dir.path().join("elsewhere")));
    assert_eq!(runtime::bundle_dir(), Some(bundle.as_path()));

    let candidates = runtime::candidate_paths();
    assert_eq!(
        candidates.first(),
        Some(&planted),
        "with ORT_DYLIB_PATH unset the bundle comes first: {candidates:?}"
    );
    assert_eq!(
        candidates.len(),
        2,
        "the bundle, then the executable's directory: {candidates:?}"
    );

    match runtime::ensure() {
        Err(Error::RuntimeLoad { path, reason }) => {
            assert_eq!(path, planted);
            assert!(!reason.is_empty());
        }
        other => panic!("expected RuntimeLoad for the bundle's copy, got {other:?}"),
    }
    // The status report goes the same way and says the same thing.
    match runtime::info() {
        Err(Error::RuntimeLoad { path, .. }) => assert_eq!(path, planted),
        other => panic!("expected RuntimeLoad from info(), got {other:?}"),
    }
}
