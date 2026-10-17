//! Both release workflows ship the pinned ONNX Runtime, and check every
//! installer for it before anything is uploaded (#383).
//!
//! Each leg has to fetch the library (`scripts/fetch-ml-runtime.sh`), on
//! Windows stage the Visual C++ runtime it imports, and build with its
//! platform's `tauri.ml-runtime.<os>.conf.json` overlay; leave out any of
//! those and the installers build fine and carry no runtime, which nobody
//! would see until a user opened Settings. Then
//! `scripts/installer-ml-runtime.py check` opens each installer, and it has
//! to come before the upload, or an installer that fails it is already in
//! the draft. In `release-dev.yml` that means tauri-action only builds (no
//! `releaseId`), because given one it uploads inside its own step.
//!
//! `release-dev.yml` also has a dry run, for trying all of this on a
//! branch, and a dry run must not reach anything that creates or publishes
//! a release.
//!
//! ## Why a text test
//!
//! Same reason as `release_gating.rs` and `release_signed_paths.rs`: no
//! GitHub Actions step runs here, and the signed workflow has never run at
//! all. What a test can do is hold each of these in place, so that dropping
//! one in a refactor fails CI instead of the next release.

use std::path::PathBuf;

fn repo_file(rel: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("could not read {}: {e}", path.display()))
}

/// The workflow without its comment-only lines, so an explanation does
/// not count as the thing it explains.
fn effective(yaml: &str) -> String {
    yaml.lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The text of the step named `name`: from its `- name:` line to the next
/// step's, or the end of the job.
fn step<'a>(yaml: &'a str, name: &str) -> &'a str {
    let marker = format!("- name: {name}\n");
    let start = yaml
        .find(&marker)
        .unwrap_or_else(|| panic!("no step named {name:?}"));
    let rest = &yaml[start + marker.len()..];
    let end = rest
        .find("\n      - name: ")
        .or_else(|| rest.find("\n  publish:"))
        .unwrap_or(rest.len());
    &yaml[start..start + marker.len() + end]
}

/// Where in the workflow the step named `name` starts.
fn position(yaml: &str, name: &str) -> usize {
    yaml.find(&format!("- name: {name}\n"))
        .unwrap_or_else(|| panic!("no step named {name:?}"))
}

const WORKFLOWS: [&str; 2] = [
    ".github/workflows/release-dev.yml",
    ".github/workflows/release-signed.yml",
];

/// Each matrix leg, the manifest platform it bundles, and its overlay.
const LEGS: [(&str, &str, &str); 3] = [
    (
        "macos-universal",
        "osx-universal2",
        "src-tauri/tauri.ml-runtime.macos.conf.json",
    ),
    (
        "windows-x86_64",
        "win-x64",
        "src-tauri/tauri.ml-runtime.windows.conf.json",
    ),
    (
        "linux-x86_64",
        "linux-x64",
        "src-tauri/tauri.ml-runtime.linux.conf.json",
    ),
];

/// The matrix entry for one leg: from `- name: <leg>` to the next entry.
fn leg<'a>(yaml: &'a str, name: &str) -> &'a str {
    let marker = format!("- name: {name}\n");
    let start = yaml
        .find(&marker)
        .unwrap_or_else(|| panic!("no matrix leg {name:?}"));
    let rest = &yaml[start + marker.len()..];
    let end = rest
        .find("\n          - name: ")
        .or_else(|| rest.find("\n    steps:"))
        .unwrap_or(rest.len());
    &yaml[start..start + marker.len() + end]
}

#[test]
fn every_leg_bundles_its_platforms_runtime_with_its_overlay() {
    for workflow in WORKFLOWS {
        let yaml = effective(&repo_file(workflow));
        for (name, platform, overlay) in LEGS {
            let entry = leg(&yaml, name);
            assert!(
                entry.contains(&format!("ml_runtime: {platform}\n")),
                "{workflow}: the {name} leg must say which runtime it bundles \
                 (`ml_runtime: {platform}`)"
            );
            let args = entry
                .lines()
                .find(|l| l.trim_start().starts_with("args:"))
                .unwrap_or_else(|| panic!("{workflow}: the {name} leg has no args"));
            assert!(
                args.contains(&format!("--config {overlay}")),
                "{workflow}: the {name} leg must build with `--config {overlay}`, or its \
                 installers carry no runtime. Got: {args}"
            );
            assert!(
                repo_file(&format!("apps/desktop/{overlay}")).contains("ml-runtime/"),
                "{overlay} should exist and bundle the fetched runtime"
            );
        }
    }
}

#[test]
fn every_leg_fetches_the_runtime_and_windows_stages_the_vc_runtime_before_building() {
    for workflow in WORKFLOWS {
        let yaml = effective(&repo_file(workflow));
        let fetch = step(&yaml, "Fetch the pinned ONNX Runtime");
        assert!(
            fetch.contains(
                "scripts/fetch-ml-runtime.sh ${{ matrix.platform.ml_runtime }} \
                 apps/desktop/src-tauri/ml-runtime/${{ matrix.platform.ml_runtime }}"
            ),
            "{workflow}: fetch into the folder the overlays bundle from:\n{fetch}"
        );
        assert!(
            !fetch.contains("if:"),
            "{workflow}: every leg fetches the runtime"
        );

        let stage = step(&yaml, "Stage the Visual C++ runtime beside it");
        assert!(
            stage.contains("if: matrix.platform.runner == 'windows-latest'")
                && stage.contains(
                    "installer-ml-runtime.py stage-vc-runtime \
                     apps/desktop/src-tauri/ml-runtime/vc-runtime-x64"
                ),
            "{workflow}: the Windows leg stages the Visual C++ runtime where the Windows \
             overlay bundles it from:\n{stage}"
        );

        let build = yaml
            .find("uses: tauri-apps/tauri-action@")
            .unwrap_or_else(|| panic!("{workflow}: no tauri-action step"));
        assert!(
            position(&yaml, "Fetch the pinned ONNX Runtime") < build
                && position(&yaml, "Stage the Visual C++ runtime beside it") < build,
            "{workflow}: the runtime has to be there before tauri-action builds"
        );
    }
}

#[test]
fn every_installer_is_checked_after_the_build_and_before_the_upload() {
    for workflow in WORKFLOWS {
        let yaml = effective(&repo_file(workflow));
        let check = step(&yaml, "Check the ONNX Runtime in every installer");
        assert!(
            check.contains("installer-ml-runtime.py check")
                && check.contains("${{ matrix.platform.ml_runtime }}")
                && check.contains("--paths-json \"$ARTIFACT_PATHS\"")
                && check.contains("ARTIFACT_PATHS: ${{ steps.build.outputs.artifactPaths }}"),
            "{workflow}: check what tauri-action reported it built:\n{check}"
        );
        assert!(
            !check.contains("if:") && !check.contains("continue-on-error"),
            "{workflow}: the check runs on every leg and its failure stops the leg"
        );

        let build = yaml
            .find("uses: tauri-apps/tauri-action@")
            .unwrap_or_else(|| panic!("{workflow}: no tauri-action step"));
        let checked = position(&yaml, "Check the ONNX Runtime in every installer");
        let upload = yaml
            .find("https://uploads.github.com/")
            .unwrap_or_else(|| panic!("{workflow}: no upload step"));
        assert!(
            build < checked && checked < upload,
            "{workflow}: build, then check, then upload"
        );

        // Given `releaseId` or `tagName`, tauri-action uploads inside its
        // own step, before any check could run.
        let action = &yaml[build..checked];
        assert!(
            !action.contains("releaseId:") && !action.contains("tagName:"),
            "{workflow}: tauri-action must only build; the upload comes after the check"
        );
    }
}

#[test]
fn the_signed_macos_build_checks_the_dylibs_signature_not_its_bytes() {
    let yaml = effective(&repo_file(".github/workflows/release-signed.yml"));
    assert!(
        leg(&yaml, "macos-universal").contains("ml_runtime_check: --signed"),
        "Tauri re-signs the dylib in a signed build, so the macOS check needs --signed"
    );
    for name in ["windows-x86_64", "linux-x86_64"] {
        assert!(
            leg(&yaml, name).contains("ml_runtime_check: \"\""),
            "only the macOS build rewrites the library it bundles ({name})"
        );
    }
    assert!(step(&yaml, "Check the ONNX Runtime in every installer")
        .contains("check ${{ matrix.platform.ml_runtime_check }}"));
}

#[test]
fn a_dry_run_creates_uploads_and_publishes_nothing() {
    let yaml = effective(&repo_file(".github/workflows/release-dev.yml"));
    assert!(
        yaml.contains("      dry_run:\n") && yaml.contains("type: boolean"),
        "release-dev.yml has a `dry_run` dispatch input"
    );

    // A job: its `  <name>:` line and every line up to the next job's.
    let job = |name: &str| {
        let mut lines = yaml.lines().skip_while(|l| *l != format!("  {name}:"));
        let first = lines.next().unwrap_or_else(|| panic!("no job {name}"));
        let body: Vec<&str> = lines
            .take_while(|l| !(l.starts_with("  ") && !l.starts_with("   ") && l.ends_with(':')))
            .collect();
        format!("{first}\n{}", body.join("\n"))
    };

    let create = job("create-release");
    assert!(
        create.contains("if: ${{ !inputs.dry_run }}"),
        "create-release must not run in a dry run"
    );
    let publish = job("publish");
    assert!(
        publish.contains("if: ${{ !inputs.dry_run && needs.build.result == 'success' }}"),
        "publish must not run in a dry run"
    );
    // The build runs in a dry run even though its need was skipped, and
    // only then.
    assert!(yaml.contains(
        "if: ${{ !cancelled() && (needs.create-release.result == 'success' || \
         (inputs.dry_run && needs.create-release.result == 'skipped')) }}"
    ));

    let upload = step(&yaml, "Upload the checked installers to the draft release");
    assert!(
        upload.contains("if: ${{ !inputs.dry_run }}"),
        "a dry run uploads nothing to a release"
    );
    let keep = step(&yaml, "Keep the installers (dry run only)");
    assert!(keep.contains("if: ${{ inputs.dry_run }}"));
}

/// The Windows jq ends each line it writes with a carriage return, which
/// then ends every path read from it: every installer is "not a file" and
/// nothing uploads. Both upload steps read tauri-action's paths with jq.
#[test]
fn the_upload_steps_drop_the_carriage_return_jq_writes_on_windows() {
    for workflow in WORKFLOWS {
        let yaml = effective(&repo_file(workflow));
        assert!(
            yaml.contains("jq -r '.[]' | tr -d '\\r')"),
            "{workflow}: strip the CR from jq's output before using the paths"
        );
    }
}

/// macOS runners run `shell: bash` steps with the system bash 3.2, which has
/// no `mapfile` or `readarray`, no associative arrays and no case-changing
/// expansions. The first real dev release after #615 died on `mapfile` in
/// the upload step ("mapfile: command not found"); the dry runs had skipped
/// that step, so nothing caught it before.
#[test]
fn the_release_steps_use_nothing_bash_3_2_lacks() {
    for workflow in WORKFLOWS {
        let yaml = effective(&repo_file(workflow));
        for bash4 in ["mapfile ", "readarray ", "declare -A", ",,}", "^^}"] {
            assert!(
                !yaml.contains(bash4),
                "{workflow}: `{bash4}` needs bash 4, and macOS runners run bash 3.2"
            );
        }
    }
}
