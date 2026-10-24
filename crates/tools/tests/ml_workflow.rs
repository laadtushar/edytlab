//! `.github/workflows/ml.yml` stays the opt-in workflow it says it is (#383).
//!
//! The workflow runs the tests nothing else runs: the speech model on the real
//! ONNX Runtime, the `transcribe` tool on it, and the real download. Ways it
//! could stop doing that without anyone noticing, each of which has a check
//! here:
//!
//! * it gets a `push`, `pull_request` or `schedule` trigger, and every PR (or
//!   every week) downloads 139 MB from a third party, and a failure there is a
//!   red check on an unrelated change;
//! * a test it names is renamed, or loses its `#[ignore]`, and the step runs
//!   nothing or runs it on every `cargo test`;
//! * the model or the runtime it fetches stops being the pinned one, or the
//!   variable it hands the model folder in is not the one the tests read;
//! * a damaged cache restore is trusted instead of checked.
//!
//! There is no way to run a GitHub Actions expression here, so this reads the
//! workflow as text, in the manner of `release_gating.rs` and
//! `release_ml_runtime.rs`, which sit beside it for the same reason. The last
//! test does run `scripts/fetch-ml-model.sh`, against a folder it damages.

use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::process::Command;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Line endings as `\n`: on Windows git checks the repository out with
/// `\r\n`, and the step lookups below search for text that ends in `\n`.
fn lf(text: &str) -> String {
    text.replace("\r\n", "\n")
}

fn read(relative: &str) -> String {
    let path = repo_root().join(relative);
    lf(&std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display())))
}

fn workflow() -> String {
    read(".github/workflows/ml.yml")
}

/// Every `.rs` file under `relative` (a folder of the repository), joined.
fn read_rust_sources(relative: &str) -> String {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()))
            .flatten()
        {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }
    let mut files = Vec::new();
    walk(&repo_root().join(relative), &mut files);
    files.sort();
    files
        .iter()
        .map(|f| lf(&std::fs::read_to_string(f).expect("a source file reads")))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Lines that are YAML, not comments.
fn code_lines(text: &str) -> Vec<&str> {
    text.lines()
        .filter(|l| !l.trim_start().starts_with('#') && !l.trim().is_empty())
        .collect()
}

/// The keys directly under `on:`.
fn triggers(yaml: &str) -> Vec<String> {
    let lines = code_lines(yaml);
    let start = lines
        .iter()
        .position(|l| l.trim_end() == "on:")
        .expect("the workflow has an `on:` block");
    lines[start + 1..]
        .iter()
        .take_while(|l| l.starts_with(' '))
        .filter(|l| l.starts_with("  ") && !l.starts_with("   "))
        .map(|l| l.trim().trim_end_matches(':').to_string())
        .collect()
}

/// The lines of the step named `name`, after its `- name:` line.
fn step_body<'a>(yaml: &'a str, name: &str) -> impl Iterator<Item = &'a str> {
    let marker = format!("- name: {name}\n");
    let at = yaml
        .find(&marker)
        .unwrap_or_else(|| panic!("no step {name:?}"));
    yaml[at + marker.len()..]
        .lines()
        .take_while(|l| !l.trim_start().starts_with("- name:"))
}

#[test]
fn the_workflow_reads_the_same_with_windows_line_endings() {
    // A Windows checkout has `\r\n` in every file; `read` must hand back what
    // a Linux one does, or the step lookups find nothing.
    let yaml = workflow();
    let crlf = yaml.replace('\n', "\r\n");
    assert_ne!(crlf, yaml);
    assert_eq!(lf(&crlf), yaml);
    assert_eq!(
        step_body(&lf(&crlf), "Fetch ONNX Runtime").count(),
        step_body(&yaml, "Fetch ONNX Runtime").count()
    );
    assert!(step_body(&yaml, "Fetch ONNX Runtime").count() > 0);
}

#[test]
fn it_runs_only_when_dispatched() {
    let yaml = workflow();
    assert_eq!(
        triggers(&yaml),
        vec!["workflow_dispatch".to_string()],
        "ml.yml must be opt-in: workflow_dispatch and nothing else. Not push, \
         pull_request, pull_request_target, workflow_run or schedule: each \
         downloads the speech model without anyone asking, and `auto-release.yml` \
         waits on the run named \"CI\" (not this one) so nothing needs it there."
    );
}

#[test]
fn it_installs_no_toolchain_components_nothing_uses() {
    // rust-toolchain.toml lists rustfmt and clippy, so rustup has them; this
    // workflow runs neither, and a `components:` here is a download per run.
    for line in code_lines(&workflow()) {
        assert!(
            !line.trim_start().starts_with("components:"),
            "ml.yml installs toolchain components ({line:?}); it runs neither rustfmt nor clippy"
        );
    }
}

#[test]
fn it_needs_no_secrets() {
    let yaml = workflow();
    for line in code_lines(&yaml) {
        assert!(
            !line.contains("secrets."),
            "ml.yml reads a secret ({line:?}); the model is public and the runtime is a release asset"
        );
    }
    // And it does not write: no token beyond reading the repository.
    let lines = code_lines(&yaml);
    assert!(
        lines.iter().any(|l| l.trim() == "contents: read"),
        "ml.yml should hold `contents: read`"
    );
    assert!(
        !lines.iter().any(|l| l.trim_end().ends_with(": write")),
        "ml.yml grants a write permission it has no use for"
    );
}

#[test]
fn every_action_is_pinned_to_a_released_version() {
    let yaml = workflow();
    let mut seen = 0;
    for line in code_lines(&yaml) {
        let Some(rest) = line
            .trim()
            .strip_prefix("uses:")
            .or_else(|| line.trim().strip_prefix("- uses:"))
        else {
            continue;
        };
        seen += 1;
        let reference = rest.trim().split('@').nth(1).unwrap_or("");
        let major = reference
            .strip_prefix('v')
            .is_some_and(|v| !v.is_empty() && v.chars().all(|c| c.is_ascii_digit() || c == '.'));
        // `dtolnay/rust-toolchain@stable` is how ci.yml bootstraps rustup.
        assert!(
            major || reference == "stable",
            "{line:?} is not pinned to a version tag (ci.yml pins `@vN`)"
        );
    }
    assert!(seen >= 4, "found {seen} `uses:` lines; the parse is broken");
}

#[test]
fn it_runs_ignored_tests_that_exist_and_are_ignored_with_a_reason() {
    let yaml = workflow();
    let mut checked = Vec::new();
    for line in code_lines(&yaml) {
        let line = line.trim();
        let Some(command) = line.strip_prefix("run: cargo test ") else {
            continue;
        };
        let words: Vec<&str> = command.split_whitespace().collect();
        let value_after = |flag: &str| {
            words
                .iter()
                .position(|w| *w == flag)
                .and_then(|i| words.get(i + 1))
                .unwrap_or_else(|| panic!("{line:?} has no {flag}"))
                .to_string()
        };
        let package = value_after("-p");
        // `--lib` runs the ignored tests inside the crate's own source (the ones
        // that need a crate-private type, such as the decoder's cache); the
        // rest name an integration test file with `--test`.
        let (path, source) = if words.contains(&"--lib") {
            let dir = format!("crates/{package}/src");
            (format!("{dir} (--lib)"), read_rust_sources(&dir))
        } else {
            let path = format!("crates/{package}/tests/{}.rs", value_after("--test"));
            let source = read(&path);
            (path, source)
        };
        let after_dashes = words
            .iter()
            .position(|w| *w == "--")
            .map_or(&[][..], |i| &words[i + 1..]);
        assert!(
            after_dashes.contains(&"--ignored"),
            "{line:?} must pass --ignored to the test binary: the tests it names are ignored"
        );

        let lines: Vec<&str> = source.lines().collect();
        let mut ignored = 0;
        for (i, l) in lines.iter().enumerate() {
            if l.trim() != "#[test]" {
                continue;
            }
            let next = lines[i + 1..]
                .iter()
                .find(|n| !n.trim().is_empty())
                .map_or("", |n| n.trim());
            // Some of these files also hold tests that need nothing and run
            // in every `cargo test`; `--ignored` leaves those out here.
            if next.starts_with("#[ignore") {
                assert!(
                    next.starts_with("#[ignore = \""),
                    "{path}: the ignored test at line {} has no reason (a skip must say why, #263): {next:?}",
                    i + 1
                );
                ignored += 1;
            }
        }
        assert!(
            ignored > 0,
            "{path} has no ignored test: the step runs nothing"
        );
        checked.push(path);
    }
    checked.sort();
    assert_eq!(
        checked,
        vec![
            "crates/ml-pipeline/tests/model_real_download.rs".to_string(),
            "crates/ml-whisper/src (--lib)".to_string(),
            "crates/ml-whisper/tests/transcribe_known_words.rs".to_string(),
            "crates/ml-whisper/tests/transcribe_smoke.rs".to_string(),
            "crates/tools/tests/transcribe_end_to_end.rs".to_string(),
            "crates/tools/tests/transcribe_load_errors.rs".to_string(),
        ],
        "the workflow is meant to run exactly these five test files and the ignored tests in ml-whisper's source"
    );
}

#[test]
fn model_real_download_says_what_the_workflow_does() {
    // That test's header sends the reader to "the opt-in ML workflow"; it must
    // be one that runs it.
    let header = read("crates/ml-pipeline/tests/model_real_download.rs");
    assert!(
        header.contains("opt-in ML workflow"),
        "the header changed; update this check"
    );
    assert!(
        workflow().contains("--test model_real_download -- --ignored"),
        "ml.yml does not run model_real_download, which its header says it does"
    );
}

#[test]
fn it_fetches_the_pinned_model_and_runtime_and_hands_them_to_the_test() {
    let yaml = workflow();

    // The scripts exist, and name what the manifests pin.
    for script in ["scripts/fetch-ml-runtime.sh", "scripts/fetch-ml-model.sh"] {
        assert!(repo_root().join(script).is_file(), "{script} is missing");
        assert!(yaml.contains(script), "ml.yml does not run {script}");
    }
    assert!(
        yaml.contains("fetch-ml-runtime.sh linux-x64 ml-runtime/linux-x64"),
        "the runtime is the linux-x64 entry of ml-runtime.json"
    );
    let model = ml_pipeline::models::WHISPER_BASE_ID;
    assert!(
        ml_pipeline::Catalog::built_in().get(model).is_some(),
        "models.json does not pin {model}"
    );
    assert!(
        yaml.contains(&format!("fetch-ml-model.sh {model} ml-models/{model}")),
        "ml.yml does not fetch {model} with fetch-ml-model.sh"
    );

    // The caches are keyed by the manifests, so a changed pin misses them.
    for manifest in [
        "apps/desktop/src-tauri/ml-runtime.json",
        "crates/ml-pipeline/models.json",
    ] {
        assert!(
            yaml.contains(&format!("hashFiles('{manifest}')")),
            "no cache key is made from {manifest}"
        );
    }

    // The tests read the two variables, and the workflow sets both. The model
    // one names the model's folder (#652), and was `WHISPER_MODEL_DIR` before.
    for file in [
        "crates/ml-whisper/tests/transcribe_known_words.rs",
        "crates/tools/tests/transcribe_end_to_end.rs",
    ] {
        let test = read(file);
        for var in ["ORT_DYLIB_PATH", "WHISPER_MODEL_PATH"] {
            assert!(test.contains(var), "{file} does not read {var}");
        }
    }
    for var in ["ORT_DYLIB_PATH", "WHISPER_MODEL_PATH"] {
        assert!(
            yaml.contains(&format!("{var}: ${{{{ github.workspace }}}}/")),
            "ml.yml does not set {var}"
        );
    }
    for file in [
        ".github/workflows/ml.yml",
        "scripts/fetch-ml-model.sh",
        "docs/development-guide.md",
    ] {
        assert!(
            !read(file).contains("WHISPER_MODEL_DIR"),
            "{file} names WHISPER_MODEL_DIR, a variable nothing reads"
        );
    }
    // The fetch steps run on a cache hit as well, which is what checks it.
    for name in ["Fetch ONNX Runtime", "Fetch the whisper-base model"] {
        for line in step_body(&yaml, name) {
            assert!(
                !line.trim_start().starts_with("if:"),
                "the {name:?} step has a condition ({line:?}); it must run on a cache hit to check it"
            );
        }
    }
    // Not the version of the runtime, which belongs to the manifest.
    assert!(
        !code_lines(&yaml).iter().any(|l| l.contains("1.23")),
        "ml.yml names a runtime version; the manifest is the one place for it"
    );
}

/// A manifest that pins two small files served from disk (`curl` reads
/// `file://`), laid out the way the script finds it: `<root>/scripts/` and
/// `<root>/crates/ml-pipeline/models.json`. Returns the root and the folder
/// the files are served from.
#[cfg(unix)]
fn fake_repo(tmp: &Path, serve: &[(&str, &[u8])]) -> PathBuf {
    use std::fmt::Write as _;
    // Hashed here, not by spawning sha256sum, which macOS does not have.
    let sha = |bytes: &[u8]| {
        use sha2::{Digest, Sha256};
        Sha256::digest(bytes)
            .iter()
            .fold(String::new(), |mut hex, b| {
                let _ = write!(hex, "{b:02x}");
                hex
            })
    };
    let root = tmp.join("repo");
    std::fs::create_dir_all(root.join("scripts")).unwrap();
    std::fs::create_dir_all(root.join("crates/ml-pipeline")).unwrap();
    std::fs::copy(
        repo_root().join("scripts/fetch-ml-model.sh"),
        root.join("scripts/fetch-ml-model.sh"),
    )
    .unwrap();
    let served = tmp.join("served");
    let mut files = String::new();
    for (i, (path, bytes)) in serve.iter().enumerate() {
        let at = served.join(path);
        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
        std::fs::write(&at, bytes).unwrap();
        write!(
            files,
            "{}{{\"path\":\"{path}\",\"url\":\"file://{}\",\"size\":{},\"sha256\":\"{}\"}}",
            if i == 0 { "" } else { "," },
            at.display(),
            bytes.len(),
            sha(bytes)
        )
        .unwrap();
    }
    std::fs::write(
        root.join("crates/ml-pipeline/models.json"),
        format!("{{\"models\":[{{\"id\":\"tiny\",\"files\":[{files}]}}]}}"),
    )
    .unwrap();
    root
}

#[cfg(unix)]
fn fetch(root: &Path, out: &Path) -> std::process::Output {
    Command::new("bash")
        .arg(root.join("scripts/fetch-ml-model.sh"))
        .args(["tiny"])
        .arg(out)
        .output()
        .expect("bash runs the script")
}

/// What a cache restore can hand the fetch step: a file that is the wrong
/// size, one that is the right size with other bytes, and one that is gone.
/// The script downloads each again and checks the download, so the run ends
/// with the pinned bytes (the comment at the top of `ml.yml` says so), and
/// when the source is itself wrong, it fails and keeps nothing under the name.
#[cfg(unix)]
#[test]
fn a_damaged_cache_is_downloaded_again_and_checked_not_trusted() {
    let tmp = tempfile::tempdir().unwrap();
    let encoder = b"the encoder bytes".as_slice();
    let tokenizer = b"{\"tokenizer\":true}".as_slice();
    let root = fetch_root(tmp.path(), encoder, tokenizer);
    let out = tmp.path().join("cache");

    // Cold: both are downloaded.
    let run = fetch(&root, &out);
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        std::fs::read(out.join("onnx/encoder_model.onnx")).unwrap(),
        encoder
    );
    // stdout is the folder and nothing else (compared by identity: macOS's
    // temp folder is a symlink, and the script prints the path it was given).
    assert_eq!(
        Path::new(String::from_utf8_lossy(&run.stdout).trim())
            .canonicalize()
            .unwrap(),
        out.canonicalize().unwrap(),
        "stdout: {}",
        String::from_utf8_lossy(&run.stdout)
    );

    // Warm: nothing is downloaded.
    let run = fetch(&root, &out);
    assert!(run.status.success());
    assert!(
        String::from_utf8_lossy(&run.stderr).contains("(0 file(s) downloaded)"),
        "a good cache should download nothing: {}",
        String::from_utf8_lossy(&run.stderr)
    );

    // Damaged: wrong size, same size with other bytes, and missing.
    std::fs::write(out.join("onnx/encoder_model.onnx"), b"short").unwrap();
    std::fs::write(out.join("tokenizer.json"), vec![b'x'; tokenizer.len()]).unwrap();
    let run = fetch(&root, &out);
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(run.status.success(), "{stderr}");
    assert!(stderr.contains("(2 file(s) downloaded)"), "{stderr}");
    assert!(
        stderr.contains("not the pinned file"),
        "it should say why: {stderr}"
    );
    assert_eq!(
        std::fs::read(out.join("onnx/encoder_model.onnx")).unwrap(),
        encoder
    );
    assert_eq!(
        std::fs::read(out.join("tokenizer.json")).unwrap(),
        tokenizer
    );
    std::fs::remove_file(out.join("tokenizer.json")).unwrap();
    let run = fetch(&root, &out);
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        std::fs::read(out.join("tokenizer.json")).unwrap(),
        tokenizer
    );

    // The source itself is wrong (the server changed the bytes under the
    // pin): the step fails, and the damaged file is not left under its name.
    std::fs::write(out.join("onnx/encoder_model.onnx"), b"short").unwrap();
    std::fs::write(
        tmp.path().join("served/onnx/encoder_model.onnx"),
        b"the encoder bytes, changed upstream",
    )
    .unwrap();
    let run = fetch(&root, &out);
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(
        !run.status.success(),
        "a download that is not the pin must fail the step"
    );
    assert!(stderr.contains("the manifest says"), "{stderr}");
    assert!(
        !out.join("onnx/encoder_model.onnx").exists()
            && !out.join("onnx/encoder_model.onnx.part").exists(),
        "nothing may be left under the name, or beside it"
    );
}

#[cfg(unix)]
fn fetch_root(tmp: &Path, encoder: &[u8], tokenizer: &[u8]) -> PathBuf {
    fake_repo(
        tmp,
        &[
            ("onnx/encoder_model.onnx", encoder),
            ("tokenizer.json", tokenizer),
        ],
    )
}
