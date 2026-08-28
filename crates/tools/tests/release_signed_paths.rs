//! The signed-release workflow must look for its artifacts where they
//! actually are.
//!
//! `apps/desktop/src-tauri` is a workspace *member* — the only
//! `[workspace]` table in the repo is the root `Cargo.toml` — so cargo
//! puts its build output in the root `target/`. There has never been an
//! `apps/desktop/src-tauri/target`, and `projectPath: apps/desktop` is
//! a tauri-action input, not a shell working directory.
//!
//! `release-signed.yml` nevertheless walked that path to find what to
//! sign and what to verify. The Windows leg died before signtool ran,
//! so no Authenticode-signed installer could be produced; the macOS
//! leg's artifacts were signed inside tauri-action but the post-hoc
//! Gatekeeper check failed on the missing directory. Both legs failing
//! gates `publish`, and the release is stranded as a permanent draft.
//!
//! ## Why a text test
//!
//! This workflow is `workflow_dispatch`-only and needs signing secrets
//! that do not exist, so it has never run — and it is the third defect
//! of this shape found in it (see also the cross-target toolchain and
//! the tag-pinned-to-the-wrong-commit fixes). Every one of them would
//! have surfaced for the first time ~40 minutes into somebody's first
//! real signed release.
//!
//! Nothing can execute a GitHub Actions step here, but a test can hold
//! the invariant: the steps take their file list from what tauri-action
//! reported, and no step reaches into a directory that does not exist.
//! Same crude spirit as `release_gating.rs`.

use std::path::PathBuf;

fn repo_file(rel: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("could not read {}: {e}", path.display()))
}

/// Lines with the comment-only ones dropped, so an explanation of the
/// old bug does not read as the bug.
fn effective_lines(yaml: &str) -> Vec<(usize, &str)> {
    yaml.lines()
        .enumerate()
        .filter(|(_, l)| !l.trim_start().starts_with('#'))
        .map(|(i, l)| (i + 1, l))
        .collect()
}

/// The premise the rest of this file rests on: there is exactly one
/// cargo workspace, rooted at the repo root. If `src-tauri` ever gains
/// its own `[workspace]` table it would get its own `target/`, and
/// every claim below would need revisiting — so assert it rather than
/// assume it.
#[test]
fn src_tauri_is_a_workspace_member_and_has_no_target_dir_of_its_own() {
    let member = repo_file("apps/desktop/src-tauri/Cargo.toml");
    assert!(
        !member.contains("[workspace]"),
        "apps/desktop/src-tauri declares its own [workspace]; it would then have \
         its own target/ and the artifact paths in release-signed.yml need rechecking"
    );

    let root = repo_file("Cargo.toml");
    assert!(
        root.contains("[workspace]"),
        "the repo root is no longer the workspace root"
    );
}

#[test]
fn no_step_reaches_into_the_nonexistent_src_tauri_target() {
    let yaml = repo_file(".github/workflows/release-signed.yml");
    let offenders: Vec<String> = effective_lines(&yaml)
        .into_iter()
        .filter(|(_, l)| l.contains("src-tauri/target"))
        .map(|(n, l)| format!("{n}: {}", l.trim()))
        .collect();

    assert!(
        offenders.is_empty(),
        "release-signed.yml still looks for artifacts under \
         apps/desktop/src-tauri/target, which cargo never creates:\n  {}",
        offenders.join("\n  ")
    );
}

/// Both steps that touch the built files must take the list from
/// tauri-action's own report, which is what the upload step already
/// did. A directory walk and an upload list that disagree is precisely
/// how unsigned bytes could reach a release.
#[test]
fn signing_and_verification_consume_the_artifact_paths_output() {
    let yaml = repo_file(".github/workflows/release-signed.yml");
    let mentions = yaml
        .matches("ARTIFACT_PATHS: ${{ steps.build.outputs.artifactPaths }}")
        .count();

    assert!(
        mentions >= 3,
        "expected the Windows signing step, the macOS verification step and the \
         upload step to each read steps.build.outputs.artifactPaths; found \
         {mentions} of the 3"
    );
}

/// A `workspaces:` entry pointing at a directory with no `Cargo.lock`
/// is a silent cache miss on every run — a ~40-minute build paying full
/// price each time, with nothing in the log to say why.
#[test]
fn the_rust_cache_names_only_the_real_workspace_root() {
    let yaml = repo_file(".github/workflows/release-signed.yml");
    let offenders: Vec<String> = effective_lines(&yaml)
        .into_iter()
        .filter(|(_, l)| l.trim() == "apps/desktop/src-tauri")
        .map(|(n, _)| n.to_string())
        .collect();

    assert!(
        offenders.is_empty(),
        "rust-cache is still given apps/desktop/src-tauri as a workspace \
         (line(s) {}), which is a workspace member, not a root",
        offenders.join(", ")
    );
}
