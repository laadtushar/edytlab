//! The third-party notices every installer carries (`licenses/THIRD_PARTY_NOTICES.txt`).
//!
//! The file is generated from the lockfiles by
//! `scripts/third-party-notices/generate.mjs`, and the `Notices` workflow
//! (`.github/workflows/notices.yml`) fails when it is stale; that needs
//! `cargo-about`, so it is not a test. What a plain `cargo test` can hold is
//! the rest of the chain, which fails silently otherwise:
//!
//! - the file is in `bundle.resources`, committed (a resource that is not in
//!   the tree fails `tauri-build`, as `CLAUDE.md` warns for the ONNX Runtime);
//! - it has the parts a licence asks for: the credits, the licence texts, the
//!   fonts' OFL, and the MPL-2.0 source availability;
//! - the version of `cargo-about` the script and the workflow pin is one;
//! - the installer check looks for the file in every installer.

use std::path::{Path, PathBuf};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("the repository root")
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn notices() -> String {
    read(&repo().join("apps/desktop/src-tauri/licenses/THIRD_PARTY_NOTICES.txt"))
}

#[test]
fn the_notices_are_bundled_from_a_committed_file() {
    let conf: serde_json::Value = serde_json::from_str(&read(
        &repo().join("apps/desktop/src-tauri/tauri.conf.json"),
    ))
    .expect("tauri.conf.json is JSON");
    let resources = &conf["bundle"]["resources"];
    assert_eq!(
        resources["licenses/THIRD_PARTY_NOTICES.txt"], "licenses/THIRD_PARTY_NOTICES.txt",
        "bundle.resources must carry the notices to licenses/ in the resource directory, \
         where the third_party_notices command and the installer check look"
    );
    assert!(
        !notices().trim().is_empty(),
        "THIRD_PARTY_NOTICES.txt is committed and not empty"
    );
}

#[test]
fn the_notices_carry_what_the_licences_ask_for() {
    let text = notices();
    assert!(
        !text.contains('\r'),
        "LF only, so it is the same on every checkout"
    );

    // The four parts, in order.
    let at = |needle: &str| {
        text.find(&format!("\n{needle}\n"))
            .unwrap_or_else(|| panic!("the notices have no section {needle:?}"))
    };
    let order = [
        at("CREDITS"),
        at("JAVASCRIPT PACKAGES AND FONTS"),
        at("RUST CRATES"),
        at("SOURCE AVAILABILITY (MPL-2.0)"),
    ];
    assert!(
        order.windows(2).all(|w| w[0] < w[1]),
        "sections out of order: {order:?}"
    );

    for needle in [
        // The licences, in full, with their copyright lines.
        "Permission is hereby granted, free of charge",
        "Apache License",
        "Redistribution and use in source and binary forms",
        "Mozilla Public License Version 2.0",
        "SIL OPEN FONT LICENSE Version 1.1",
        "Copyright 2024 The Geist Project Authors",
        // The code the Rust crates were derived from (MIT, BSD, Apache).
        "Copyright (c) 2022 OpenAI",
        "Copyright (c) Meta Platforms, Inc. and affiliates.",
        "Copyright (c) 2020 CNRS",
        "Copyright (c) 2001-2002 Enthought, Inc.",
        // The model licence that asks for credit, and where the source of
        // the MPL-2.0 crates is.
        "Creative Commons Attribution 4.0 International",
        "https://crates.io/crates/symphonia/",
        "https://github.com/laadtushar/edytlab",
    ] {
        assert!(text.contains(needle), "the notices lack {needle:?}");
    }
}

#[test]
fn every_mpl_crate_has_its_source_named() {
    let text = notices();
    let start = text
        .find("\nSOURCE AVAILABILITY (MPL-2.0)\n")
        .expect("the MPL-2.0 section");
    let section = &text[start..];
    let crates = section.lines().filter(|l| l.starts_with("  * ")).count();
    let sources = section
        .lines()
        .filter(|l| l.contains("Source code:  https://crates.io/crates/"))
        .count();
    let downloads = section
        .lines()
        .filter(|l| l.contains("Download:     https://static.crates.io/crates/"))
        .count();
    assert!(crates >= 1, "no MPL-2.0 crate listed");
    assert_eq!(
        (sources, downloads),
        (crates, crates),
        "each MPL-2.0 crate gets a source line"
    );
}

/// The value of `CARGO_ABOUT_VERSION` in the script and in the workflow.
fn script_and_workflow_versions() -> (String, String) {
    let script = read(&repo().join("scripts/third-party-notices/generate.mjs"));
    let in_script = script
        .lines()
        .find_map(|l| l.strip_prefix("export const CARGO_ABOUT_VERSION = \""))
        .and_then(|l| l.split('"').next())
        .expect("generate.mjs exports CARGO_ABOUT_VERSION")
        .to_string();
    let workflow = read(&repo().join(".github/workflows/notices.yml"));
    let in_workflow = workflow
        .lines()
        .find_map(|l| l.trim().strip_prefix("CARGO_ABOUT_VERSION:"))
        .expect("notices.yml sets CARGO_ABOUT_VERSION")
        .trim()
        .trim_matches('"')
        .to_string();
    (in_script, in_workflow)
}

#[test]
fn the_script_and_the_workflow_pin_one_cargo_about() {
    let (script, workflow) = script_and_workflow_versions();
    assert_eq!(
        script, workflow,
        "generate.mjs and notices.yml pin different cargo-about versions"
    );
}

#[test]
fn the_notices_workflow_does_not_gate_the_ci_run() {
    // `auto-release.yml` tags a release when the run named "CI" succeeds, so
    // a check that needs the network and a tool of its own is a workflow of
    // its own, as `audit.yml` is.
    let workflow = read(&repo().join(".github/workflows/notices.yml"));
    assert!(
        workflow.lines().any(|l| l.trim() == "name: Notices"),
        "the workflow is named Notices"
    );
    assert!(
        workflow.contains("generate.mjs --check"),
        "it runs the staleness check"
    );
    for path in ["Cargo.lock", "pnpm-lock.yaml", "THIRD_PARTY_NOTICES.txt"] {
        assert!(workflow.contains(path), "it runs when {path} changes");
    }
}

#[test]
fn the_installer_check_looks_for_the_notices() {
    let check = read(&repo().join("scripts/installer-ml-runtime.py"));
    assert!(
        check.contains("THIRD_PARTY_NOTICES.txt"),
        "scripts/installer-ml-runtime.py check must fail an installer without the notices"
    );
}
