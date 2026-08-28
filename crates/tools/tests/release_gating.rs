//! The auto-release job must refuse to act on anything but a push to
//! this repository.
//!
//! `auto-release.yml` tags a commit in this repo and then dispatches
//! `release-dev.yml` at that tag, where `workflow_dispatch` executes the
//! workflow file *as it exists at the ref*. So whatever can reach this
//! job can run arbitrary code in a first-party run with a writable
//! `GITHUB_TOKEN` and repository secrets, and publish the result under a
//! genuine `v0.1.0-dev.N` tag.
//!
//! For a while, what could reach it was any fork PR. The only guard was
//! `conclusion == 'success'`, and the `branches: [main]` trigger filter
//! reads like a base-branch restriction but is not one: GitHub matches
//! it against the *triggering run's* `head_branch`, which for a
//! `pull_request`-triggered CI run is the PR's source branch. A fork
//! with a branch named `main` matched it.
//!
//! ## Why a text test
//!
//! There is no way to unit-test a GitHub Actions expression, and no
//! staging repo to fire the real event against — proving the fix by
//! execution would mean actually attempting the attack. What a test can
//! do is hold the boundary in place: these conditions are easy to drop
//! while refactoring a YAML `if:`, and their absence is silent until
//! someone exploits it.
//!
//! Deliberately crude, in the same spirit as `website_tool_docs.rs`:
//! read the workflow as text and insist the conditions appear.

use std::path::PathBuf;

fn read_workflow(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../.github/workflows")
        .join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("could not read {}: {e}", path.display()))
}

/// Collapse whitespace so a condition split across lines by a YAML
/// folded scalar still matches.
fn normalised(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[test]
fn auto_release_refuses_runs_that_are_not_pushes_to_this_repo() {
    let yaml = normalised(&read_workflow("auto-release.yml"));

    assert!(
        yaml.contains("github.event.workflow_run.conclusion == 'success'"),
        "auto-release.yml no longer requires CI to have passed"
    );

    // The condition that closes the fork-PR vector. `branches: [main]`
    // does not do this: it matches the triggering run's head branch,
    // which a fork controls.
    assert!(
        yaml.contains("github.event.workflow_run.event == 'push'"),
        "auto-release.yml no longer requires the CI run to have come from \
         a push. Without this, a fork PR whose source branch is named \
         `main` reaches the job, and it will tag the fork's commit in \
         this repo and dispatch release-dev.yml from attacker-controlled \
         source. The `branches:` trigger filter is not a substitute — it \
         matches the triggering run's head_branch."
    );

    assert!(
        yaml.contains("github.event.workflow_run.head_repository.full_name == github.repository"),
        "auto-release.yml no longer checks that the CI run belongs to \
         this repository rather than a fork"
    );
}

/// The gap was reachable partly because the header said the opposite of
/// what the workflow did — it claimed to fire "after CI completes on
/// main", so the `branches:` filter read as a boundary it never was.
#[test]
fn auto_release_documents_that_the_branch_filter_is_not_a_boundary() {
    let yaml = read_workflow("auto-release.yml");
    let header: String = yaml.lines().take_while(|l| !l.starts_with("on:")).collect();
    let header = header.to_lowercase();

    assert!(
        header.contains("head_branch"),
        "auto-release.yml's header no longer explains that `branches:` \
         matches the triggering run's head_branch. That misreading is \
         what let the fork-PR escalation sit unnoticed."
    );
}

/// Any other consumer of `workflow_run` inherits the same trust
/// problem: the triggering run may belong to a fork. Today
/// `auto-release.yml` is the only one, and this fails if that changes
/// without the new consumer being considered.
#[test]
fn auto_release_is_the_only_workflow_run_consumer() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.github/workflows");
    let mut consumers: Vec<String> = Vec::new();

    for entry in std::fs::read_dir(&dir).expect("workflows dir") {
        let path = entry.expect("dir entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("yml") {
            continue;
        }
        let text = std::fs::read_to_string(&path).expect("read workflow");
        // A `workflow_run:` trigger key, not a mention in a comment —
        // ci.yml discusses this workflow in prose.
        let triggers = text.lines().any(|l| {
            l.trim_start().starts_with("workflow_run:") && !l.trim_start().starts_with('#')
        });
        if triggers {
            consumers.push(path.file_name().unwrap().to_string_lossy().into_owned());
        }
    }

    consumers.sort();
    assert_eq!(
        consumers,
        vec!["auto-release.yml".to_string()],
        "a new `workflow_run` consumer appeared. The triggering run can \
         belong to a fork, so it must gate on `workflow_run.event` and \
         `head_repository` exactly as auto-release.yml does — then add it \
         to this list."
    );
}
