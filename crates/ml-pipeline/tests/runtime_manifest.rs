//! `apps/desktop/src-tauri/ml-runtime.json` is well formed (#383).
//!
//! That file is the one place the pinned ONNX Runtime is written down:
//! its version, and for each platform the archive to fetch, its size and
//! SHA-256, and which file inside it is the library.
//! `scripts/fetch-ml-runtime.sh` downloads from it and CI caches on its
//! hash, and the release workflows will bundle what it fetches. A typo in
//! it would only show when something downloads from a mangled URL or
//! compares against a digest that cannot match, so this checks its shape
//! on every `cargo test`, with no network: the URLs point at Microsoft's
//! release for the pinned version, the digests are SHA-256, and the
//! installed names are the ones `runtime::ensure` looks for.
//!
//! It cannot check that a digest is *right*. The fetch script does that,
//! against the real archive, and the Linux CI job runs it.

use std::path::Path;

use ml_pipeline::runtime;
use serde_json::Value;

/// `<repo>/apps/desktop/src-tauri/ml-runtime.json`, read from this crate's
/// directory so the test runs from any working directory.
fn manifest() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("apps")
        .join("desktop")
        .join("src-tauri")
        .join("ml-runtime.json");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read the runtime manifest {}: {e}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("the runtime manifest {} is not JSON: {e}", path.display()))
}

fn text<'a>(value: &'a Value, key: &str, what: &str) -> &'a str {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("{what} needs a string `{key}`, got {value}"))
}

/// The platforms the manifest must pin, with the file name the app looks
/// for on each (`runtime::library_file_name`) and the archive extension
/// Microsoft publishes for it.
const PLATFORMS: [(&str, &str, &str); 3] = [
    ("linux-x64", "libonnxruntime.so", ".tgz"),
    ("win-x64", "onnxruntime.dll", ".zip"),
    ("osx-universal2", "libonnxruntime.dylib", ".tgz"),
];

#[test]
fn version_and_macos_floor_are_plain_numbers() {
    let m = manifest();
    let version = text(&m, "version", "the manifest");
    let parts: Vec<&str> = version.split('.').collect();
    assert!(
        parts.len() == 3
            && parts
                .iter()
                .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit())),
        "`version` must be MAJOR.MINOR.PATCH, got {version:?}"
    );

    let min_macos = text(&m, "min_macos", "the manifest");
    let parts: Vec<&str> = min_macos.split('.').collect();
    assert!(
        parts.len() == 2
            && parts
                .iter()
                .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit())),
        "`min_macos` must be MAJOR.MINOR, got {min_macos:?}"
    );
}

#[test]
fn pinned_runtime_is_new_enough_for_the_ort_binding() {
    // `ort` refuses a library older than the API level it was built for
    // (`ort::MINOR_VERSION`, from its `api-NN` feature), so the pin must
    // not be older than that level or the app could never load it.
    let m = manifest();
    let version = text(&m, "version", "the manifest");
    let minor: u32 = version
        .split('.')
        .nth(1)
        .and_then(|p| p.parse().ok())
        .unwrap_or_else(|| panic!("no minor version in {version:?}"));
    assert!(
        minor >= ort::MINOR_VERSION,
        "ml-runtime.json pins ONNX Runtime {version}, but `ort` is built for API level 1.{} and \
         refuses anything older. Raise the pin, or lower the `api-NN` feature on `ort`.",
        ort::MINOR_VERSION,
    );
}

#[test]
fn every_platform_is_pinned_to_the_release_download_for_the_version() {
    let m = manifest();
    let version = text(&m, "version", "the manifest");
    let base = format!("https://github.com/microsoft/onnxruntime/releases/download/v{version}/");
    let platforms = m
        .get("platforms")
        .and_then(Value::as_object)
        .expect("the manifest needs a `platforms` object");

    let mut names: Vec<&str> = platforms.keys().map(String::as_str).collect();
    names.sort_unstable();
    let mut expected: Vec<&str> = PLATFORMS.iter().map(|(name, _, _)| *name).collect();
    expected.sort_unstable();
    assert_eq!(
        names, expected,
        "the manifest must pin exactly these platforms"
    );

    for (name, install_as, extension) in PLATFORMS {
        let p = &platforms[name];
        let what = format!("platform {name}");

        let url = text(p, "url", &what);
        assert!(
            url.starts_with(&base),
            "{name}: `url` must start with {base} (https, Microsoft's release for {version}), \
             got {url}"
        );
        let file = &url[base.len()..];
        assert!(
            !file.is_empty() && !file.contains(['/', '?', '#']),
            "{name}: `url` must name one release asset, got {url}"
        );
        assert!(
            file.contains(version),
            "{name}: the asset name {file} does not contain the version {version}"
        );
        assert!(
            file.ends_with(extension),
            "{name}: the asset {file} should be a {extension} archive"
        );

        let sha256 = text(p, "sha256", &what);
        assert!(
            sha256.len() == 64
                && sha256
                    .chars()
                    .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)),
            "{name}: `sha256` must be 64 lowercase hex characters, got {sha256:?}"
        );

        let size = p
            .get("size")
            .and_then(Value::as_u64)
            .unwrap_or_else(|| panic!("{name}: `size` must be a whole number of bytes"));
        assert!(size > 0, "{name}: `size` must be positive");

        // The library inside the archive: a plain relative path whose first
        // part is the one directory Microsoft puts everything in, named for
        // the version. (Some archives write every name as "./dir/...".)
        let member = text(p, "member", &what);
        let member = member.strip_prefix("./").unwrap_or(member);
        let parts: Vec<&str> = member.split('/').collect();
        assert!(
            parts.len() >= 2 && parts.iter().all(|part| !part.is_empty() && *part != ".."),
            "{name}: `member` {member:?} must be a relative path below the archive's top directory"
        );
        assert!(
            parts[0].contains(version),
            "{name}: `member` {member:?} should sit in a top directory named for {version}"
        );

        assert_eq!(
            text(p, "install_as", &what),
            install_as,
            "{name}: `install_as` is the file name the app looks for"
        );
    }
}

#[test]
fn notices_are_plain_file_names() {
    // They are copied from the archive's top directory next to the
    // library, so each must be a bare name.
    let m = manifest();
    let notices = m
        .get("notices")
        .and_then(Value::as_array)
        .expect("the manifest needs a `notices` array");
    let names: Vec<&str> = notices.iter().filter_map(Value::as_str).collect();
    assert_eq!(names.len(), notices.len(), "`notices` must be strings");
    for required in ["LICENSE", "ThirdPartyNotices.txt"] {
        assert!(
            names.contains(&required),
            "the runtime's {required} must ship with it: add it to `notices`"
        );
    }
    for name in names {
        assert!(
            !name.is_empty() && !name.contains(['/', '\\']) && name != "..",
            "a notice must be a bare file name, got {name:?}"
        );
    }
}

#[test]
fn the_name_for_this_platform_is_the_one_the_gate_looks_for() {
    let m = manifest();
    let here = if cfg!(target_os = "windows") {
        "win-x64"
    } else if cfg!(target_os = "macos") {
        "osx-universal2"
    } else {
        "linux-x64"
    };
    assert_eq!(
        text(&m["platforms"][here], "install_as", here),
        runtime::library_file_name(),
        "`runtime::ensure` looks for this name next to the executable, so an installer has to \
         put the library there under it"
    );
}
