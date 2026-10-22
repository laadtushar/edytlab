//! `.github/workflows/ml.yml` stays the opt-in workflow it says it is (#383).
//!
//! The workflow runs the tests nothing else runs: the speech model on the real
//! ONNX Runtime, and the real download. Three ways it could stop doing that
//! without anyone noticing, each of which has a check here:
//!
//! * it gets a `push` or `pull_request` trigger, and every PR downloads 139 MB
//!   from a third party (and a failure there is a red check on an unrelated
//!   change);
//! * a test it names is renamed, or loses its `#[ignore]`, and the step runs
//!   nothing or runs it on every `cargo test`;
//! * the model or the runtime it fetches stops being the pinned one.
//!
//! There is no way to run a GitHub Actions expression here, so this reads the
//! workflow as text, in the manner of `crates/tools/tests/release_gating.rs`.

use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(relative: &str) -> String {
    let path = repo_root().join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

fn workflow() -> String {
    read(".github/workflows/ml.yml")
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

#[test]
fn it_runs_only_when_asked_or_on_the_weekly_schedule() {
    let yaml = workflow();
    let mut got = triggers(&yaml);
    got.sort();
    assert_eq!(
        got,
        vec!["schedule".to_string(), "workflow_dispatch".to_string()],
        "ml.yml must be opt-in: workflow_dispatch and a schedule, never push, \
         pull_request, pull_request_target or workflow_run. A push or PR trigger \
         downloads the speech model on every change, and `auto-release.yml` \
         waits on the run named \"CI\" (not this one) so nothing needs it there."
    );
    assert!(
        code_lines(&yaml)
            .iter()
            .any(|l| l.trim().starts_with("- cron:")),
        "the schedule has no cron entry"
    );
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
        let (package, target) = (value_after("-p"), value_after("--test"));
        assert!(
            words.contains(&"--ignored"),
            "{line:?} must pass --ignored: the tests it names are ignored"
        );

        let path = format!("crates/{package}/tests/{target}.rs");
        let source = read(&path);
        let lines: Vec<&str> = source.lines().collect();
        let mut tests = 0;
        for (i, l) in lines.iter().enumerate() {
            if l.trim() != "#[test]" {
                continue;
            }
            tests += 1;
            let next = lines[i + 1..]
                .iter()
                .find(|n| !n.trim().is_empty())
                .map_or("", |n| n.trim());
            assert!(
                next.starts_with("#[ignore = \""),
                "{path}: the test at line {} is not `#[ignore = \"reason\"]` (it would run in \
                 every `cargo test`, and a skip would not be visible, #263): {next:?}",
                i + 1
            );
        }
        assert!(tests > 0, "{path} has no #[test]: the step runs nothing");
        checked.push(path);
    }
    checked.sort();
    assert_eq!(
        checked,
        vec![
            "crates/ml-pipeline/tests/model_real_download.rs".to_string(),
            "crates/ml-whisper/tests/transcribe_known_words.rs".to_string(),
        ],
        "the workflow is meant to run exactly these two test files"
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

    // The test reads the two variables, and the workflow sets both.
    let test = read("crates/ml-whisper/tests/transcribe_known_words.rs");
    for var in ["ORT_DYLIB_PATH", "WHISPER_MODEL_DIR"] {
        assert!(test.contains(var), "the test does not read {var}");
        assert!(
            yaml.contains(&format!("{var}: ${{{{ github.workspace }}}}/")),
            "ml.yml does not set {var}"
        );
    }
    // Not the version of the runtime, which belongs to the manifest.
    assert!(
        !code_lines(&yaml).iter().any(|l| l.contains("1.23")),
        "ml.yml names a runtime version; the manifest is the one place for it"
    );
}
