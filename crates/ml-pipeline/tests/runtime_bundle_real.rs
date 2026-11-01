//! A real ONNX Runtime in the bundle directory is the one that loads, and
//! is reported as bundled (#383).
//!
//! `runtime_bundle.rs` shows the registered directory is searched, with a
//! file that is not a library. This puts the pinned library there, as an
//! installer does, takes `ORT_DYLIB_PATH` away, and checks what the app's
//! Settings line says: which file, which version, and that the installer
//! put it there.
//!
//! `#[ignore]`d like the other tests that load a real library, and for the
//! same reason: `ORT_DYLIB_PATH` names the copy to install into the
//! fake bundle (`scripts/fetch-ml-runtime.sh <platform> <dir>` fetches it).
//! CI runs it beside `cache_smoke`. A process of its own: the bundle
//! directory is set once, and a loaded library stays loaded.

use std::path::Path;

use ml_pipeline::runtime::{self, Origin};

mod ort_support;

#[test]
#[ignore = "needs ORT_DYLIB_PATH (an ONNX Runtime library); run with --ignored"]
fn the_pinned_library_in_the_bundle_directory_loads_and_is_reported_as_bundled() {
    let library = ort_support::ort_dylib_path();
    assert!(Path::new(&library).is_file(), "{library} is not a file");

    let manifest = ort_support::pinned_runtime_manifest();

    // The installer's job, done by hand: the library under the name
    // `runtime::ensure` looks for, in a directory of its own.
    let dir = tempfile::tempdir().expect("tempdir");
    let bundle = dir.path().join("Frameworks");
    std::fs::create_dir_all(&bundle).expect("mkdir");
    let installed = bundle.join(runtime::library_file_name());
    std::fs::copy(&library, &installed).expect("copy the library into the bundle");

    // SAFETY: this is the only test in this binary, so no other thread
    // reads the environment while it is edited.
    unsafe {
        std::env::remove_var("ORT_DYLIB_PATH");
    }
    assert!(runtime::set_bundle_dir(bundle));

    let info = runtime::info().expect("the bundled ONNX Runtime loads");
    assert_eq!(info.path, installed);
    assert_eq!(info.origin, Origin::Bundle);
    assert!(info.bundled());
    assert_eq!(Some(info.version.as_str()), manifest["version"].as_str());
    assert_eq!(runtime::ensure().expect("ensure"), installed);
}
