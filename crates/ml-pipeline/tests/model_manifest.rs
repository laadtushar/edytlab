//! `crates/ml-pipeline/models.json` pins the speech model, and the copy
//! that describes the download agrees with it (#383).
//!
//! The manifest is the one place the model is written down: its Hugging
//! Face repository and commit, and for each file the URL, byte size and
//! SHA-256. The app reads it (`Catalog::built_in`, included at build
//! time), and so does this file. The values here are the table on #384,
//! checked against the real files on 2026-10-22 (each file downloaded
//! through the sandbox's proxy and hashed), so editing the manifest to
//! anything else fails here and has to be a reviewed change, here too.
//!
//! Nothing in this file touches the network. That the digests are *right*
//! was checked once, by hand, against the real files; the ignored test in
//! `model_real_download.rs` checks it again whenever someone runs it.
//!
//! The second half is about words. A download from Hugging Face is a new
//! outbound request, so CLAUDE.md says where the privacy copy has to
//! change with it. The size and the host are written in several places by
//! hand; these tests read those places and fail if one still says nothing
//! is downloaded, or names a size other than the manifest's.

use std::path::{Path, PathBuf};

use ml_pipeline::models::{whisper_base, WHISPER_BASE_ID};
use ml_pipeline::{Catalog, ModelStore};

fn repo_file(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join(relative)
}

fn read(relative: &str) -> String {
    let path = repo_file(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

const REVISION: &str = "608c49e61301901684bc36cac8f74b95ff6b5a8e";

/// (path, bytes, sha256), the table on #384.
const PINNED: [(&str, u64, &str); 3] = [
    (
        "onnx/encoder_model.onnx",
        82_451_730,
        "7fcea817bb2be4d86729b521e5a7fcbec28fa743edfed67e882b33ff15852540",
    ),
    (
        "onnx/decoder_model_merged_int8.onnx",
        53_712_708,
        "cf9a8d5bcddc0917a0078135b484cedcaf44f28909cd91910abd29dced9171db",
    ),
    (
        "tokenizer.json",
        2_480_466,
        "27fc476bfe7f17299480be2273fc0608e4d5a99aba2ab5dec5374b4482d1a566",
    ),
];

#[test]
fn the_manifest_parses_and_names_whisper_base() {
    let catalog = Catalog::built_in();
    let model = catalog
        .get(WHISPER_BASE_ID)
        .expect("whisper-base is pinned");
    assert_eq!(WHISPER_BASE_ID, "whisper-base");
    assert_eq!(model.id, "whisper-base");
    assert_eq!(model.repo, "onnx-community/whisper-base_timestamped");
    assert_eq!(model.revision, REVISION);
    assert_eq!(model.license, "Apache-2.0");
    // The store's own checks (id, paths, https, digests) pass for it.
    ModelStore::new("unused-root")
        .artifact_dir(&model.artifact)
        .expect("a valid artifact");
    assert_eq!(catalog.models().len(), 1, "one model is pinned");
}

#[test]
fn every_file_is_the_one_checked_on_2026_10_22() {
    let model = Catalog::built_in().get(WHISPER_BASE_ID).unwrap();
    let got: Vec<(&str, u64, &str)> = model
        .artifact
        .files
        .iter()
        .map(|f| (f.path.as_str(), f.size, f.sha256.as_str()))
        .collect();
    assert_eq!(got, PINNED);
    assert_eq!(model.size(), 138_644_904);
}

#[test]
fn urls_are_hugging_face_at_the_pinned_commit_and_nothing_else() {
    let model = Catalog::built_in().get(WHISPER_BASE_ID).unwrap();
    assert_eq!(model.revision.len(), 40, "a commit, not a branch name");
    assert!(
        model.revision.chars().all(|c| c.is_ascii_hexdigit()),
        "{}",
        model.revision
    );
    for file in &model.artifact.files {
        assert_eq!(
            file.url,
            format!(
                "https://huggingface.co/{}/resolve/{}/{}",
                model.repo, model.revision, file.path
            ),
            "{} must be fetched from the pinned commit, never from `main`",
            file.path
        );
    }
}

#[test]
fn whisper_base_is_the_manifests_artifact() {
    let model = Catalog::built_in().get(WHISPER_BASE_ID).unwrap();
    assert_eq!(whisper_base(), &model.artifact);
}

// ---------------------------------------------------------------------------
// The copy
// ---------------------------------------------------------------------------

/// "139" for 138,644,904 bytes: megabytes of 1,000,000, rounded, as the
/// Settings row and the pages write it.
fn size_in_mb() -> String {
    let bytes = Catalog::built_in().get(WHISPER_BASE_ID).unwrap().size();
    format!("{} MB", (bytes + 500_000) / 1_000_000)
}

#[test]
fn the_size_in_the_copy_is_the_manifests() {
    assert_eq!(size_in_mb(), "139 MB");
}

/// `text` with every run of whitespace made one space, so a phrase that
/// the JSX or the Markdown wraps across lines can be matched whole.
fn flat(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[test]
fn the_privacy_copy_names_the_host_and_the_size() {
    let size = size_in_mb();
    for page in [
        "website/app/privacy/page.tsx",
        "website/app/docs/user-guide/page.tsx",
    ] {
        let text = flat(&read(page));
        assert!(
            text.contains("huggingface.co"),
            "{page} must name huggingface.co, the host the download goes to"
        );
        assert!(
            text.contains(&size),
            "{page} must give the model's size ({size})"
        );
    }
    for page in [
        "website/app/use-cases/local-ai-audio-editor/page.tsx",
        "marketing/press-kit.md",
    ] {
        let text = flat(&read(page));
        assert!(
            text.contains("Hugging Face") || text.contains("huggingface.co"),
            "{page} must say where the speech model is downloaded from"
        );
        assert!(
            text.contains(&size),
            "{page} must give the model's size ({size})"
        );
    }
}

#[test]
fn the_privacy_page_says_what_hugging_face_sees_and_that_the_user_starts_it() {
    let flat = flat(&read("website/app/privacy/page.tsx"));
    for phrase in [
        "only when you click Download",
        "IP address",
        "nothing about your audio",
    ] {
        assert!(
            flat.contains(phrase),
            "the privacy page must say {phrase:?}"
        );
    }
}
