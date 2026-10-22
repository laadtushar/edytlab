//! The pinned speech model, downloaded for real (#383). `#[ignore]`d.
//!
//! CI downloads nothing: every other test in this crate serves bytes from
//! 127.0.0.1. This one fetches the three files of `models.json` from
//! huggingface.co, about 139 MB, through whatever proxy the environment
//! configures, and checks what the unit tests cannot: that the pinned
//! commit, sizes and hashes are still what the server has, that Hugging
//! Face's redirect to its CDN is followed, and that a cancelled transfer
//! resumes with a `Range` request the CDN honours.
//!
//! Run by hand, or by the opt-in ML workflow (#383 PR 7):
//!
//! ```text
//! cargo test -p ml-pipeline --test model_real_download -- --ignored --nocapture
//! ```

use std::sync::mpsc;
use std::time::{Duration, Instant};

use ml_pipeline::models::{whisper_base, WHISPER_BASE_ID};
use ml_pipeline::{Catalog, ModelDownloads, ModelState, ModelStatus, ModelStore};

const PATIENCE: Duration = Duration::from_secs(600);

fn wait_for(
    rx: &mpsc::Receiver<ModelStatus>,
    what: &str,
    pred: impl Fn(&ModelState) -> bool,
) -> ModelStatus {
    let deadline = Instant::now() + PATIENCE;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        let status = rx
            .recv_timeout(left)
            .unwrap_or_else(|_| panic!("timed out waiting for {what}"));
        if pred(&status.state) {
            return status;
        }
        if let ModelState::Failed { reason } = &status.state {
            panic!("the download failed while waiting for {what}: {reason}");
        }
    }
}

#[test]
#[ignore = "downloads about 139 MB from huggingface.co; run by hand: -- --ignored"]
fn the_pinned_speech_model_downloads_resumes_and_verifies() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().join("models");
    let (tx, rx) = mpsc::channel();
    let downloads = ModelDownloads::new(
        &root,
        Catalog::built_in().clone(),
        move |status: &ModelStatus| {
            let _ = tx.send(status.clone());
        },
    );
    let began = Instant::now();

    // First attempt: stop it part-way, so the second has something to
    // resume from.
    downloads.start_download(WHISPER_BASE_ID).expect("start");
    let part_way = wait_for(
        &rx,
        "5 MB to arrive",
        |s| matches!(s, ModelState::Downloading { received, .. } if *received >= 5_000_000),
    );
    println!("cancelling at {} bytes", part_way.bytes);
    downloads.cancel(WHISPER_BASE_ID).expect("cancel");
    let stopped = wait_for(&rx, "the cancel to land", |s| {
        matches!(s, ModelState::NotDownloaded)
    });
    assert!(stopped.bytes > 0, "a partial file is kept: {stopped:?}");
    let kept = stopped.bytes;

    // Second attempt: resumes from the partial file and finishes.
    downloads.start_download(WHISPER_BASE_ID).expect("resume");
    let done = wait_for(&rx, "ready", |s| matches!(s, ModelState::Ready));
    assert_eq!(done.bytes, 138_644_904);
    println!(
        "resumed from {kept} bytes; ready after {:.1}s",
        began.elapsed().as_secs_f64()
    );

    // Every file is where the manifest says, with the pinned hash: the
    // store re-reads and hashes them.
    let store = ModelStore::new(&root);
    let dir = store
        .cached(whisper_base())
        .expect("readable")
        .expect("every file verifies");
    assert!(dir.join("onnx/encoder_model.onnx").is_file());
    assert!(dir.join("tokenizer.json").is_file());

    // And it can be removed again, leaving nothing of it behind.
    downloads.remove(WHISPER_BASE_ID).expect("remove");
    assert!(!dir.exists(), "{}", dir.display());
}
