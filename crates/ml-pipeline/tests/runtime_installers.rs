//! The release installers put the pinned ONNX Runtime where the loader
//! looks for it (#383).
//!
//! The release workflows fetch the library and its notices with
//! `scripts/fetch-ml-runtime.sh` into `apps/desktop/src-tauri/ml-runtime/`
//! (and, on Windows, stage the Visual C++ runtime DLLs beside them with
//! `scripts/installer-ml-runtime.py stage-vc-runtime`), then build with one
//! of three Tauri config overlays, `tauri.ml-runtime.<os>.conf.json`, passed
//! with `--config`. The overlay is what tells the bundler where each file
//! goes, and `runtime::bundle_dir_for` is what tells the app where to look,
//! so this checks the two agree, platform by platform, with no bundler and
//! no network:
//!
//! - macOS: `bundle.macOS.frameworks` copies the dylib into
//!   `Contents/Frameworks` under its own file name (and Tauri signs it with
//!   the app), which is the bundle directory for an executable at
//!   `Contents/MacOS`.
//! - Linux: a `bundle.resources` entry whose target is the bare file name
//!   puts the library in the resource directory itself (`/usr/lib/edytlab`
//!   in a deb, `usr/lib/edytlab` inside the AppImage), which is the bundle
//!   directory there.
//! - Windows: the resource directory is the install folder, beside the exe,
//!   where the loader looks and where `LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR`
//!   finds the Visual C++ runtime the library imports.
//!
//! The base `tauri.conf.json` names none of these files: a plain `tauri
//! build` or `cargo build` has nothing fetched, and tauri-build fails on a
//! resource path that does not exist. Only the release workflows pass an
//! overlay. What the installers actually contain is checked after each
//! release build by `scripts/installer-ml-runtime.py check`.

use std::path::{Path, PathBuf};

use ml_pipeline::runtime;
use serde_json::{Map, Value};

fn src_tauri() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("apps")
        .join("desktop")
        .join("src-tauri")
}

fn json(path: &Path) -> Value {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{} is not JSON: {e}", path.display()))
}

fn manifest() -> Value {
    json(&src_tauri().join("ml-runtime.json"))
}

/// Each overlay and the manifest platform it bundles.
const OVERLAYS: [(&str, &str); 3] = [
    ("tauri.ml-runtime.linux.conf.json", "linux-x64"),
    ("tauri.ml-runtime.macos.conf.json", "osx-universal2"),
    ("tauri.ml-runtime.windows.conf.json", "win-x64"),
];

/// Where the release workflows stage the Visual C++ runtime DLLs.
const VC_RUNTIME_DIR: &str = "ml-runtime/vc-runtime-x64";

/// The notice for the Visual C++ runtime, kept in the repository.
const VC_RUNTIME_NOTICE: &str = "licenses/vc-runtime/NOTICE.txt";

fn resources(overlay: &Value) -> &Map<String, Value> {
    overlay["bundle"]["resources"]
        .as_object()
        .expect("an overlay's `bundle.resources` must be a map from source to target")
}

fn library_name(m: &Value, platform: &str) -> String {
    m["platforms"][platform]["install_as"]
        .as_str()
        .expect("every platform has an `install_as`")
        .to_string()
}

/// Every source -> target pair an overlay must hold for `platform`, the
/// library excepted on macOS (it goes in `frameworks`, not `resources`).
fn expected_resources(m: &Value, platform: &str) -> Vec<(String, String)> {
    let mut want = Vec::new();
    if platform != "osx-universal2" {
        let library = library_name(m, platform);
        // The bare name: the resource directory itself.
        want.push((format!("ml-runtime/{platform}/{library}"), library));
    }
    for notice in m["notices"].as_array().expect("`notices` is an array") {
        let notice = notice.as_str().expect("a notice is a file name");
        want.push((
            format!("ml-runtime/{platform}/{notice}"),
            format!("licenses/onnxruntime/{notice}"),
        ));
    }
    if let Some(vc) = m["platforms"][platform].get("vc_runtime") {
        for file in vc["files"].as_array().expect("`vc_runtime.files`") {
            let file = file.as_str().expect("a file name");
            // Beside onnxruntime.dll, which loads them from its own folder.
            want.push((format!("{VC_RUNTIME_DIR}/{file}"), file.to_string()));
        }
        want.push((
            VC_RUNTIME_NOTICE.to_string(),
            "licenses/vc-runtime/NOTICE.txt".to_string(),
        ));
    }
    want.sort();
    want
}

#[test]
fn the_base_config_bundles_no_runtime() {
    let text = std::fs::read_to_string(src_tauri().join("tauri.conf.json"))
        .expect("tauri.conf.json is readable");
    assert!(
        !text.contains("ml-runtime/") && !text.contains("onnxruntime"),
        "tauri.conf.json must not name the fetched runtime: a plain build has not fetched it, \
         and tauri-build fails on a resource that does not exist. It belongs in the \
         tauri.ml-runtime.<os>.conf.json overlays, which only the release workflows pass."
    );
    let base = json(&src_tauri().join("tauri.conf.json"));
    assert!(
        base["bundle"]["macOS"].get("frameworks").is_none(),
        "the dylib is added to `frameworks` by the macOS overlay only"
    );
}

#[test]
fn each_overlay_bundles_exactly_the_runtime_its_notices_and_on_windows_the_vc_runtime() {
    let m = manifest();
    for (file, platform) in OVERLAYS {
        let overlay = json(&src_tauri().join(file));
        let top: Vec<&String> = overlay
            .as_object()
            .expect("an overlay is an object")
            .keys()
            .filter(|k| *k != "$schema")
            .collect();
        assert_eq!(
            top,
            vec!["bundle"],
            "{file}: an overlay adds bundle entries and nothing else"
        );

        let mut got: Vec<(String, String)> = resources(&overlay)
            .iter()
            .map(|(src, dst)| {
                (
                    src.clone(),
                    dst.as_str()
                        .unwrap_or_else(|| panic!("{file}: the target of {src} is not a string"))
                        .to_string(),
                )
            })
            .collect();
        got.sort();
        assert_eq!(
            got,
            expected_resources(&m, platform),
            "{file}: `bundle.resources` must map the fetched files, and only those, to where \
             the app and the licence notices are expected"
        );

        for (src, dst) in &got {
            // The MSI keeps a resource's directory but not a new file name:
            // tauri-bundler writes each one as a WiX `<File Source=...>`
            // with no `Name`, so it installs under the source's own name.
            // The first dry run caught the VC++ notice landing as
            // `licenses/vc-runtime/vc-runtime-NOTICE.txt` in the MSI.
            assert_eq!(
                Path::new(src).file_name(),
                Path::new(dst).file_name(),
                "{file}: {src} -> {dst} renames the file, which the MSI does not do"
            );
            let staged = src.starts_with("ml-runtime/");
            assert!(
                staged || src_tauri().join(src).is_file(),
                "{file}: {src} is neither fetched into ml-runtime/ nor a file in the repository"
            );
        }
    }
}

#[test]
fn the_macos_overlay_puts_the_dylib_where_the_app_looks() {
    let m = manifest();
    let overlay = json(&src_tauri().join("tauri.ml-runtime.macos.conf.json"));
    let library = library_name(&m, "osx-universal2");
    let frameworks: Vec<&str> = overlay["bundle"]["macOS"]["frameworks"]
        .as_array()
        .expect("the macOS overlay lists the dylib in `bundle.macOS.frameworks`")
        .iter()
        .map(|f| f.as_str().expect("a framework is a path"))
        .collect();
    let wanted = format!("ml-runtime/osx-universal2/{library}");
    assert_eq!(frameworks, vec![wanted.as_str()]);
    // The bundler takes a path ending in `.dylib` as a library and copies
    // it into Contents/Frameworks under its file name.
    assert!(wanted.ends_with(".dylib"));

    // Off the temp directory so the path is absolute on every OS, as in
    // `runtime`'s own layout test.
    let app = std::env::temp_dir()
        .join("Applications")
        .join("edytlab.app");
    let exe = app.join("Contents").join("MacOS").join("edytlab");
    let copied = app.join("Contents").join("Frameworks").join(&library);
    let looks_in = runtime::bundle_dir_for("macos", Some(&exe), None)
        .expect("an executable in an app bundle has a bundle directory");
    assert_eq!(looks_in.join(&library), copied);
}

#[test]
fn the_linux_and_windows_overlays_put_the_library_where_the_app_looks() {
    let m = manifest();

    // Linux: the resource directory is the bundle directory, and a target
    // that is the bare file name is the resource directory itself.
    let library = library_name(&m, "linux-x64");
    let overlay = json(&src_tauri().join("tauri.ml-runtime.linux.conf.json"));
    let target = resources(&overlay)[&format!("ml-runtime/linux-x64/{library}")]
        .as_str()
        .expect("a target");
    // A deb's root and an AppImage's mount, off the temp directory so the
    // paths are absolute on every OS.
    let base = std::env::temp_dir();
    for root in [base.clone(), base.join(".mount_edytlabXYZ")] {
        let exe = root.join("usr").join("bin").join("edytlab");
        let resource_dir = root.join("usr").join("lib").join("edytlab");
        let looks_in = runtime::bundle_dir_for("linux", Some(&exe), Some(&resource_dir))
            .expect("a deb or AppImage has a resource directory");
        assert_eq!(looks_in.join(&library), resource_dir.join(target));
    }

    // Windows: no bundle directory; the loader looks beside the exe, and
    // the resource directory is the install folder the exe is in. So the
    // target is the bare name, and the DLLs the library imports sit there
    // too, where `LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR` finds them.
    let library = library_name(&m, "win-x64");
    let overlay = json(&src_tauri().join("tauri.ml-runtime.windows.conf.json"));
    assert_eq!(
        runtime::bundle_dir_for("windows", Some(Path::new(r"C:\edytlab\edytlab.exe")), None),
        None
    );
    let res = resources(&overlay);
    assert_eq!(
        res[&format!("ml-runtime/win-x64/{library}")].as_str(),
        Some(library.as_str())
    );
    for file in m["platforms"]["win-x64"]["vc_runtime"]["files"]
        .as_array()
        .expect("`vc_runtime.files`")
    {
        let file = file.as_str().expect("a file name");
        assert_eq!(
            res[&format!("{VC_RUNTIME_DIR}/{file}")].as_str(),
            Some(file),
            "{file} must sit beside {library}"
        );
    }
}
