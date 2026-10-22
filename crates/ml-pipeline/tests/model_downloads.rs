//! The user-started model download: status, start, cancel, remove (#383).
//!
//! `ModelDownloads` is what the Settings row drives through four Tauri
//! commands. These tests run it against the loopback server of
//! `support::Server`, so CI downloads nothing, with a catalog read by the
//! same parser as the shipped `models.json` (`Catalog::from_json`).
//!
//! The downloader underneath (hash, resume, cancel, locking) has its own
//! tests in `download.rs`; what is checked here is what the layer above it
//! adds: that `start` returns at once and the work goes on in the
//! background, that every change of state reaches the listener (the
//! `ml-model-progress` event), that a wrong hash ends as a reported
//! failure with nothing under a final name, that a cancel keeps the
//! partial file and the next start resumes it with a `Range` request, and
//! that `remove` deletes this model's files and nothing else.

mod support;

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use ml_pipeline::{Catalog, Error, ModelDownloads, ModelState, ModelStatus};
use sha2::{Digest, Sha256};
use support::{Route, Server};
use tempfile::TempDir;

const BIG_ROUTE: &str = "/big.onnx";
const SMALL_ROUTE: &str = "/small.json";
const OTHER_ROUTE: &str = "/other.bin";

const BIG_FILE: &str = "onnx/big.onnx";
const SMALL_FILE: &str = "small.json";

/// How long a test waits for the background download before it gives up.
const PATIENCE: Duration = Duration::from_secs(30);

fn pseudo_random(len: usize, seed: u64) -> Vec<u8> {
    let mut state = seed | 1;
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state >> 24) as u8
        })
        .collect()
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

struct Fixture {
    tmp: TempDir,
    big: Vec<u8>,
    small: Vec<u8>,
    other: Vec<u8>,
}

impl Fixture {
    fn new() -> Self {
        Self {
            tmp: tempfile::tempdir().expect("tempdir"),
            // Over a few reports' worth (256 KiB each), so there is
            // progress part-way through and something to cancel.
            big: pseudo_random(2 * 1024 * 1024 + 321, 11),
            small: br#"{"version":"1.0"}"#.to_vec(),
            other: pseudo_random(300 * 1024, 5),
        }
    }

    fn root(&self) -> PathBuf {
        self.tmp.path().join("models")
    }

    fn server(&self) -> Server {
        Server::start([
            (BIG_ROUTE, Route::new(self.big.clone())),
            (SMALL_ROUTE, Route::new(self.small.clone())),
            (OTHER_ROUTE, Route::new(self.other.clone())),
        ])
    }

    /// A server that takes a while over the big file, so a test can act
    /// while it is still arriving.
    fn slow_server(&self) -> Server {
        Server::start([
            (
                BIG_ROUTE,
                Route::new(self.big.clone()).chunk_delay(Duration::from_millis(15)),
            ),
            (SMALL_ROUTE, Route::new(self.small.clone())),
            (OTHER_ROUTE, Route::new(self.other.clone())),
        ])
    }

    /// Two models: `speech-test` (a big file and a small one) and
    /// `other-test` (one file), both served by `server`.
    fn catalog_json(&self, server: &Server) -> String {
        format!(
            r#"{{
              "models": [
                {{
                  "id": "speech-test",
                  "title": "Speech test model",
                  "repo": "example/speech-test",
                  "revision": "0123456789abcdef0123456789abcdef01234567",
                  "license": "Apache-2.0",
                  "files": [
                    {{ "path": "{BIG_FILE}", "url": "{big_url}", "size": {big_len}, "sha256": "{big_sha}" }},
                    {{ "path": "{SMALL_FILE}", "url": "{small_url}", "size": {small_len}, "sha256": "{small_sha}" }}
                  ]
                }},
                {{
                  "id": "other-test",
                  "title": "Another test model",
                  "repo": "example/other-test",
                  "revision": "0123456789abcdef0123456789abcdef01234567",
                  "license": "MIT",
                  "files": [
                    {{ "path": "other.bin", "url": "{other_url}", "size": {other_len}, "sha256": "{other_sha}" }}
                  ]
                }}
              ]
            }}"#,
            big_url = server.url(BIG_ROUTE),
            big_len = self.big.len(),
            big_sha = sha256_hex(&self.big),
            small_url = server.url(SMALL_ROUTE),
            small_len = self.small.len(),
            small_sha = sha256_hex(&self.small),
            other_url = server.url(OTHER_ROUTE),
            other_len = self.other.len(),
            other_sha = sha256_hex(&self.other),
        )
    }

    fn total(&self) -> u64 {
        (self.big.len() + self.small.len()) as u64
    }

    /// The manager and the channel its listener feeds.
    fn downloads(&self, server: &Server) -> (ModelDownloads, Receiver<ModelStatus>) {
        let catalog = Catalog::from_json(&self.catalog_json(server)).expect("test catalog");
        let (tx, rx) = mpsc::channel();
        let downloads = ModelDownloads::new(self.root(), catalog, move |status: &ModelStatus| {
            let _ = tx.send(status.clone());
        });
        (downloads, rx)
    }

    fn dir(&self) -> PathBuf {
        self.root().join("speech-test")
    }
}

/// Wait for the next status the listener is told about that satisfies
/// `pred`, and return it together with every status seen on the way.
#[track_caller]
fn wait_for(
    rx: &Receiver<ModelStatus>,
    what: &str,
    pred: impl Fn(&ModelStatus) -> bool,
) -> (ModelStatus, Vec<ModelStatus>) {
    let deadline = Instant::now() + PATIENCE;
    let mut seen = Vec::new();
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(left) {
            Ok(status) => {
                seen.push(status.clone());
                if pred(&status) {
                    return (status, seen);
                }
            }
            Err(_) => panic!("timed out waiting for {what}; saw {seen:#?}"),
        }
    }
}

fn is_ready(s: &ModelStatus) -> bool {
    s.state == ModelState::Ready
}

fn is_not_downloaded(s: &ModelStatus) -> bool {
    s.state == ModelState::NotDownloaded
}

fn is_failed(s: &ModelStatus) -> bool {
    matches!(s.state, ModelState::Failed { .. })
}

fn files_under(dir: &Path) -> Vec<String> {
    fn walk(dir: &Path, base: &Path, out: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, base, out);
            } else {
                out.push(
                    p.strip_prefix(base)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/"),
                );
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

// ---------------------------------------------------------------------------
// Status
// ---------------------------------------------------------------------------

#[test]
fn nothing_downloaded_is_not_downloaded_and_asking_costs_no_request() {
    let fx = Fixture::new();
    let server = fx.server();
    let (downloads, _rx) = fx.downloads(&server);

    let status = downloads.status("speech-test").expect("status");
    assert_eq!(status.state, ModelState::NotDownloaded);
    assert_eq!(status.bytes, 0);
    assert_eq!(status.size, fx.total(), "the size is the pinned total");
    assert_eq!(server.total_hits(), 0, "a status never touches the network");
    assert!(
        !fx.root().exists(),
        "asking for a status creates nothing on disk"
    );
}

#[test]
fn an_unknown_model_is_refused_and_nothing_is_created() {
    let fx = Fixture::new();
    let server = fx.server();
    let (downloads, _rx) = fx.downloads(&server);

    for id in [
        "whisper-base",
        "",
        "../speech-test",
        "speech-test/..",
        "SPEECH-TEST",
    ] {
        assert!(
            matches!(downloads.status(id), Err(Error::UnknownModel(_))),
            "status {id:?}"
        );
        assert!(
            matches!(downloads.start_download(id), Err(Error::UnknownModel(_))),
            "start {id:?}"
        );
        assert!(
            matches!(downloads.cancel(id), Err(Error::UnknownModel(_))),
            "cancel {id:?}"
        );
        assert!(
            matches!(downloads.remove(id), Err(Error::UnknownModel(_))),
            "remove {id:?}"
        );
    }
    assert_eq!(server.total_hits(), 0);
    assert!(!fx.root().exists());
}

// ---------------------------------------------------------------------------
// Start
// ---------------------------------------------------------------------------

#[test]
fn start_returns_at_once_and_the_download_finishes_in_the_background() {
    let fx = Fixture::new();
    let server = fx.slow_server();
    let (downloads, rx) = fx.downloads(&server);

    let began = Instant::now();
    let status = downloads.start_download("speech-test").expect("start");
    let returned_after = began.elapsed();
    assert!(
        matches!(status.state, ModelState::Downloading { received: 0, total } if total == fx.total()),
        "start answers with the state it began: {status:?}"
    );

    // The slow server takes well over a second for the big file; `start`
    // did not wait for it.
    assert!(
        returned_after < Duration::from_millis(500),
        "start waited {returned_after:?}"
    );
    // And the model is not ready yet: the status is the running download.
    let mid = downloads.status("speech-test").expect("status");
    assert!(
        matches!(mid.state, ModelState::Downloading { .. }),
        "still downloading: {mid:?}"
    );

    let (done, seen) = wait_for(&rx, "ready", is_ready);
    assert_eq!(done.bytes, fx.total());
    assert_eq!(done.size, fx.total());

    // Everything the listener heard climbs to the total and ends once at
    // Ready; no Ready is followed by anything else.
    let mut last = 0;
    for s in &seen {
        if let ModelState::Downloading { received, total } = s.state {
            assert_eq!(total, fx.total());
            assert!(received >= last, "progress went back: {seen:#?}");
            assert!(received <= total);
            last = received;
        }
    }
    assert!(last > 0, "no progress was reported: {seen:#?}");
    assert_eq!(seen.iter().filter(|s| is_ready(s)).count(), 1);
    assert!(is_ready(seen.last().unwrap()));

    // On disk: the final names only, with the pinned bytes.
    assert_eq!(files_under(&fx.dir()), [BIG_FILE, SMALL_FILE]);
    assert_eq!(std::fs::read(fx.dir().join(BIG_FILE)).unwrap(), fx.big);
    assert_eq!(std::fs::read(fx.dir().join(SMALL_FILE)).unwrap(), fx.small);

    let after = downloads.status("speech-test").expect("status");
    assert_eq!(after.state, ModelState::Ready);
    assert_eq!(after.bytes, fx.total());
}

#[test]
fn starting_twice_runs_one_download() {
    let fx = Fixture::new();
    let server = fx.slow_server();
    let (downloads, rx) = fx.downloads(&server);

    downloads.start_download("speech-test").expect("start");
    let again = downloads
        .start_download("speech-test")
        .expect("start again");
    assert!(
        matches!(again.state, ModelState::Downloading { .. }),
        "{again:?}"
    );
    wait_for(&rx, "ready", is_ready);

    assert_eq!(server.hits(BIG_ROUTE).len(), 1, "one request for the file");
    assert_eq!(server.hits(SMALL_ROUTE).len(), 1);
}

#[test]
fn a_model_that_is_ready_starts_no_request() {
    let fx = Fixture::new();
    let server = fx.server();
    let (downloads, rx) = fx.downloads(&server);
    downloads.start_download("speech-test").expect("start");
    wait_for(&rx, "ready", is_ready);
    let requests = server.total_hits();

    downloads
        .start_download("speech-test")
        .expect("start again");
    wait_for(&rx, "ready again", is_ready);
    assert_eq!(
        server.total_hits(),
        requests,
        "the files were verified, not fetched"
    );
}

// ---------------------------------------------------------------------------
// Failure
// ---------------------------------------------------------------------------

#[test]
fn a_file_with_the_wrong_hash_is_refused_and_reported_as_failed() {
    let fx = Fixture::new();
    let server = fx.server();
    // The server sends other bytes of the right length: what a corrupted
    // mirror or a swapped file looks like.
    let mut wrong = fx.big.clone();
    wrong[1000] ^= 0xff;
    server.set_route(BIG_ROUTE, Route::new(wrong));
    let (downloads, rx) = fx.downloads(&server);

    downloads.start_download("speech-test").expect("start");
    let (failed, _) = wait_for(&rx, "failed", is_failed);
    let ModelState::Failed { reason } = &failed.state else {
        unreachable!()
    };
    assert!(
        reason.contains("SHA-256") && reason.contains(BIG_FILE),
        "the reason names the file and the hash: {reason}"
    );

    // Never given its final name, and no half file is left to be taken
    // for a model.
    assert!(!fx.dir().join(BIG_FILE).exists());
    assert!(
        !files_under(&fx.dir()).iter().any(|f| f.ends_with(".part")),
        "{:?}",
        files_under(&fx.dir())
    );
    // The failure is what a status says afterwards, until the next start.
    let status = downloads.status("speech-test").expect("status");
    assert!(is_failed(&status), "{status:?}");
    assert_ne!(status.state, ModelState::Ready);

    // The server is fixed; Retry is a start, and it works.
    server.set_route(BIG_ROUTE, Route::new(fx.big.clone()));
    downloads.start_download("speech-test").expect("retry");
    wait_for(&rx, "ready after retry", is_ready);
    assert_eq!(std::fs::read(fx.dir().join(BIG_FILE)).unwrap(), fx.big);
}

#[test]
fn an_http_error_is_a_failure_with_the_status_in_its_reason() {
    let fx = Fixture::new();
    let server = fx.server();
    server.set_route(SMALL_ROUTE, Route::new(Vec::new()).status(404));
    let (downloads, rx) = fx.downloads(&server);

    downloads.start_download("speech-test").expect("start");
    let (failed, _) = wait_for(&rx, "failed", is_failed);
    let ModelState::Failed { reason } = &failed.state else {
        unreachable!()
    };
    assert!(reason.contains("404"), "{reason}");
    // The big file arrived and verified before the small one failed; it
    // is kept, so a retry does not fetch it again.
    assert!(fx.dir().join(BIG_FILE).exists());
    assert_eq!(failed.bytes, fx.big.len() as u64);
}

#[test]
fn a_tampered_file_is_not_ready_and_a_start_fetches_it_again() {
    let fx = Fixture::new();
    let server = fx.server();
    let (downloads, rx) = fx.downloads(&server);
    downloads.start_download("speech-test").expect("start");
    wait_for(&rx, "ready", is_ready);

    // Same length, different bytes: a size check would call this ready.
    let path = fx.dir().join(BIG_FILE);
    let mut bytes = std::fs::read(&path).unwrap();
    bytes[10] ^= 0x55;
    std::fs::write(&path, bytes).unwrap();

    let status = downloads.status("speech-test").expect("status");
    assert_eq!(status.state, ModelState::NotDownloaded, "{status:?}");

    downloads.start_download("speech-test").expect("start");
    wait_for(&rx, "ready again", is_ready);
    assert_eq!(std::fs::read(&path).unwrap(), fx.big);
}

// ---------------------------------------------------------------------------
// The read-only lookup
// ---------------------------------------------------------------------------

#[test]
fn cached_finds_a_verified_model_and_never_touches_the_network() {
    let fx = Fixture::new();
    let server = fx.server();
    let (downloads, rx) = fx.downloads(&server);

    assert_eq!(downloads.cached("speech-test").expect("lookup"), None);
    assert!(!fx.root().exists(), "a lookup creates nothing");
    assert_eq!(server.total_hits(), 0);

    downloads.start_download("speech-test").expect("start");
    wait_for(&rx, "ready", is_ready);
    let requests = server.total_hits();
    assert_eq!(
        downloads.cached("speech-test").expect("lookup"),
        Some(fx.dir())
    );

    // Same length, different bytes: not verified, so not found.
    let path = fx.dir().join(BIG_FILE);
    let mut bytes = std::fs::read(&path).unwrap();
    bytes[10] ^= 0x55;
    std::fs::write(&path, bytes).unwrap();
    assert_eq!(downloads.cached("speech-test").expect("lookup"), None);
    assert_eq!(server.total_hits(), requests, "a lookup downloads nothing");

    assert!(matches!(
        downloads.cached("../speech-test"),
        Err(Error::UnknownModel(_))
    ));
}

// ---------------------------------------------------------------------------
// Cancel and resume
// ---------------------------------------------------------------------------

#[test]
fn cancel_stops_the_download_keeps_the_partial_file_and_the_next_start_resumes_it() {
    let fx = Fixture::new();
    let server = fx.slow_server();
    let (downloads, rx) = fx.downloads(&server);

    downloads.start_download("speech-test").expect("start");
    // Let some of the big file arrive.
    wait_for(
        &rx,
        "some progress",
        |s| matches!(s.state, ModelState::Downloading { received, .. } if received >= 512 * 1024),
    );

    downloads.cancel("speech-test").expect("cancel");
    let (stopped, _) = wait_for(&rx, "the cancel to land", is_not_downloaded);
    assert!(
        stopped.bytes > 0 && stopped.bytes < fx.total(),
        "a part of the file is kept: {stopped:?}"
    );

    // Not given its final name; the partial is there for a resume.
    assert!(!fx.dir().join(BIG_FILE).exists());
    assert!(fx.dir().join("onnx/big.onnx.part").exists());
    let status = downloads.status("speech-test").expect("status");
    assert_eq!(status.state, ModelState::NotDownloaded);
    assert_eq!(status.bytes, stopped.bytes);

    // The second start asks for the rest, from where the first stopped.
    downloads.start_download("speech-test").expect("resume");
    wait_for(&rx, "ready after resume", is_ready);
    assert_eq!(std::fs::read(fx.dir().join(BIG_FILE)).unwrap(), fx.big);
    let hits = server.hits(BIG_ROUTE);
    assert_eq!(hits.len(), 2, "{hits:?}");
    assert_eq!(hits[0].range, None);
    let range = hits[1].range.as_deref().expect("the resume sent a Range");
    let from: u64 = range
        .strip_prefix("bytes=")
        .and_then(|r| r.strip_suffix('-'))
        .and_then(|n| n.parse().ok())
        .unwrap_or_else(|| panic!("unexpected Range {range:?}"));
    assert!(
        from > 0 && from < fx.big.len() as u64,
        "resumed from {from}"
    );
}

#[test]
fn cancel_with_nothing_running_changes_nothing() {
    let fx = Fixture::new();
    let server = fx.server();
    let (downloads, _rx) = fx.downloads(&server);
    let status = downloads.cancel("speech-test").expect("cancel");
    assert_eq!(status.state, ModelState::NotDownloaded);
    assert_eq!(server.total_hits(), 0);
}

// ---------------------------------------------------------------------------
// Remove
// ---------------------------------------------------------------------------

#[test]
fn remove_deletes_this_models_files_and_nothing_else() {
    let fx = Fixture::new();
    let server = fx.server();
    let (downloads, rx) = fx.downloads(&server);
    downloads.start_download("speech-test").expect("start");
    wait_for(&rx, "ready", is_ready);
    downloads.start_download("other-test").expect("start other");
    wait_for(&rx, "other ready", |s| {
        s.model == "other-test" && is_ready(s)
    });

    // Things that are not the model's: a note in its own folder, a file in
    // the models folder, another model's folder (downloaded above).
    std::fs::write(fx.dir().join("notes.txt"), "mine").unwrap();
    std::fs::write(fx.root().join("keep.bin"), "mine too").unwrap();

    let status = downloads.remove("speech-test").expect("remove");
    assert_eq!(status.state, ModelState::NotDownloaded);
    assert_eq!(status.bytes, 0);

    assert!(!fx.dir().join(BIG_FILE).exists());
    assert!(!fx.dir().join(SMALL_FILE).exists());
    assert!(
        !fx.dir().join("onnx").exists(),
        "the emptied sub-folder goes too"
    );
    assert_eq!(
        std::fs::read_to_string(fx.dir().join("notes.txt")).unwrap(),
        "mine",
        "a file that is not the model's stays"
    );
    assert_eq!(
        std::fs::read_to_string(fx.root().join("keep.bin")).unwrap(),
        "mine too"
    );
    // The other model is untouched and still ready.
    assert_eq!(
        downloads.status("other-test").expect("status").state,
        ModelState::Ready
    );
    assert_eq!(
        std::fs::read(fx.root().join("other-test/other.bin")).unwrap(),
        fx.other
    );
    // The listener was told.
    let (heard, _) = wait_for(&rx, "the removal", |s| {
        s.model == "speech-test" && is_not_downloaded(s)
    });
    assert_eq!(heard.bytes, 0);
}

#[test]
fn remove_of_a_folder_with_only_the_models_files_leaves_no_folder_behind() {
    let fx = Fixture::new();
    let server = fx.server();
    let (downloads, rx) = fx.downloads(&server);
    downloads.start_download("speech-test").expect("start");
    wait_for(&rx, "ready", is_ready);

    downloads.remove("speech-test").expect("remove");
    assert!(!fx.dir().exists(), "{:?}", files_under(&fx.root()));
}

#[test]
fn remove_clears_a_partial_download_and_a_failure() {
    let fx = Fixture::new();
    let server = fx.slow_server();
    let (downloads, rx) = fx.downloads(&server);
    downloads.start_download("speech-test").expect("start");
    wait_for(
        &rx,
        "some progress",
        |s| matches!(s.state, ModelState::Downloading { received, .. } if received >= 512 * 1024),
    );
    downloads.cancel("speech-test").expect("cancel");
    wait_for(&rx, "the cancel to land", is_not_downloaded);
    assert!(fx.dir().join("onnx/big.onnx.part").exists());

    let status = downloads.remove("speech-test").expect("remove");
    assert_eq!(status.state, ModelState::NotDownloaded);
    assert_eq!(status.bytes, 0, "the partial file is gone");
    assert!(!fx.dir().exists());

    // A failure is cleared by it too.
    server.set_route(SMALL_ROUTE, Route::new(Vec::new()).status(500));
    server.set_route(BIG_ROUTE, Route::new(fx.big.clone()));
    downloads.start_download("speech-test").expect("start");
    wait_for(&rx, "failed", is_failed);
    let status = downloads.remove("speech-test").expect("remove");
    assert_eq!(status.state, ModelState::NotDownloaded);
    assert_eq!(
        downloads.status("speech-test").unwrap().state,
        ModelState::NotDownloaded
    );
}

#[test]
fn remove_of_a_model_that_was_never_downloaded_is_a_no_op_that_creates_nothing() {
    let fx = Fixture::new();
    let server = fx.server();
    let (downloads, _rx) = fx.downloads(&server);

    let status = downloads.remove("speech-test").expect("remove");
    assert_eq!(status.state, ModelState::NotDownloaded);
    assert!(!fx.root().exists());
}

#[test]
fn remove_is_refused_while_the_download_runs_and_the_download_goes_on() {
    let fx = Fixture::new();
    let server = fx.slow_server();
    let (downloads, rx) = fx.downloads(&server);
    downloads.start_download("speech-test").expect("start");
    wait_for(
        &rx,
        "some progress",
        |s| matches!(s.state, ModelState::Downloading { received, .. } if received > 0),
    );

    let refused = downloads.remove("speech-test");
    assert!(
        matches!(refused, Err(Error::Busy { .. })),
        "removing under a running download: {refused:?}"
    );
    wait_for(&rx, "ready", is_ready);
    assert_eq!(std::fs::read(fx.dir().join(BIG_FILE)).unwrap(), fx.big);
}

// ---------------------------------------------------------------------------
// The shipped manifest, through the same parser
// ---------------------------------------------------------------------------

/// A one-model catalog with this id and file path, for the parser tests.
fn catalog_with(id: &str, path: &str) -> String {
    format!(
        r#"{{ "models": [ {{
            "id": "{id}", "title": "x", "repo": "a/b",
            "revision": "0123456789abcdef0123456789abcdef01234567", "license": "MIT",
            "files": [ {{ "path": "{path}", "url": "https://example.com/m.onnx", "size": 1,
                         "sha256": "0000000000000000000000000000000000000000000000000000000000000000" }} ]
        }} ] }}"#
    )
}

#[test]
fn a_catalog_that_would_write_outside_the_store_is_refused() {
    assert!(Catalog::from_json(&catalog_with("fine", "m.onnx")).is_ok());
    for (id, path) in [
        ("../escape", "m.onnx"),
        ("fine", "../m.onnx"),
        ("fine", "/abs/m.onnx"),
    ] {
        assert!(
            matches!(
                Catalog::from_json(&catalog_with(id, path)),
                Err(Error::InvalidManifest(_))
            ),
            "id {id:?}, path {path:?}"
        );
    }
    assert!(matches!(
        Catalog::from_json("not json"),
        Err(Error::InvalidManifest(_))
    ));
}
