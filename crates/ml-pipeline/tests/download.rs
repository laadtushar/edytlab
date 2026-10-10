//! The model downloader, against a loopback HTTP server (#383).
//!
//! Every test serves a small artifact from `support::Server` on
//! 127.0.0.1, so CI downloads nothing. Each has its own temp directory
//! and its own server. What the tests assert on is mostly the server's
//! record of requests: "resumed" means the second request carried
//! `Range: bytes=N-`, and "used the cache" means there was no request.

mod support;

use std::fs::{File, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use fs4::FileExt;
use ml_pipeline::{
    fetched_model_path, Error, FetchObserver, FetchProgress, ModelArtifact, ModelFile, ModelStore,
};
use sha2::{Digest, Sha256};
use support::{Hit, Route, Server};
use tempfile::TempDir;

const MODEL_ROUTE: &str = "/model.onnx";
const TOKENIZER_ROUTE: &str = "/tokenizer.json";
const MODEL_FILE: &str = "onnx/model.onnx";
const TOKENIZER_FILE: &str = "tokenizer.json";

/// Deterministic, incompressible-looking bytes (xorshift).
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
    _tmp: TempDir,
    root: PathBuf,
    model: Vec<u8>,
    tokenizer: Vec<u8>,
}

impl Fixture {
    fn new() -> Self {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path().join("models");
        Self {
            _tmp: tmp,
            root,
            // Over 256 KiB, so a fetch reports progress part-way through.
            model: pseudo_random(1024 * 1024 + 123, 7),
            tokenizer: br#"{"version":"1.0","model":{"type":"BPE"}}"#.to_vec(),
        }
    }

    fn server(&self) -> Server {
        Server::start([
            (MODEL_ROUTE, Route::new(self.model.clone())),
            (TOKENIZER_ROUTE, Route::new(self.tokenizer.clone())),
        ])
    }

    fn store(&self) -> ModelStore {
        ModelStore::new(&self.root)
    }

    fn artifact(&self, server: &Server) -> ModelArtifact {
        ModelArtifact {
            id: "tiny-model-1".into(),
            files: vec![
                ModelFile {
                    path: MODEL_FILE.into(),
                    url: server.url(MODEL_ROUTE),
                    sha256: sha256_hex(&self.model),
                    size: self.model.len() as u64,
                },
                ModelFile {
                    path: TOKENIZER_FILE.into(),
                    url: server.url(TOKENIZER_ROUTE),
                    sha256: sha256_hex(&self.tokenizer),
                    size: self.tokenizer.len() as u64,
                },
            ],
        }
    }

    fn dir(&self) -> PathBuf {
        self.root.join("tiny-model-1")
    }

    fn model_path(&self) -> PathBuf {
        self.dir().join(MODEL_FILE)
    }

    fn model_part(&self) -> PathBuf {
        self.dir().join("onnx/model.onnx.part")
    }
}

/// Records progress, and cancels once enough has arrived.
#[derive(Default)]
struct Recorder {
    events: Vec<(String, u64, u64)>,
    cancel_at: Option<u64>,
    cancelled: bool,
}

impl Recorder {
    fn cancelling_at(bytes: u64) -> Self {
        Self {
            cancel_at: Some(bytes),
            ..Self::default()
        }
    }
}

impl FetchObserver for Recorder {
    fn on_progress(&mut self, p: &FetchProgress<'_>) {
        assert_eq!(p.artifact, "tiny-model-1");
        self.events
            .push((p.file.to_string(), p.bytes_done, p.bytes_total));
        if self.cancel_at.is_some_and(|at| p.bytes_done >= at) {
            self.cancelled = true;
        }
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled
    }
}

/// Assert `path` holds exactly `expected`. Compares length and SHA-256,
/// so a failure prints two short lines rather than a megabyte of bytes.
#[track_caller]
fn assert_file_is(path: &Path, expected: &[u8]) {
    let got = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    assert_eq!(
        (got.len(), sha256_hex(&got)),
        (expected.len(), sha256_hex(expected)),
        "{} does not hold the expected bytes (length, SHA-256)",
        path.display()
    );
}

fn ranges(hits: &[Hit]) -> Vec<Option<&str>> {
    hits.iter().map(|h| h.range.as_deref()).collect()
}

fn assert_no_part_files(dir: &Path) {
    fn walk(dir: &Path, found: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, found);
            } else if p.extension().is_some_and(|x| x == "part") {
                found.push(p);
            }
        }
    }
    let mut found = Vec::new();
    walk(dir, &mut found);
    assert!(found.is_empty(), "left partial files behind: {found:?}");
}

// ---------------------------------------------------------------------------
// The happy path and the cache
// ---------------------------------------------------------------------------

#[test]
fn downloads_verifies_and_caches() {
    let fx = Fixture::new();
    let server = fx.server();
    let artifact = fx.artifact(&server);
    let mut rec = Recorder::default();

    let dir = fx.store().fetch(&artifact, &mut rec).expect("fetch");

    assert_eq!(dir, fx.dir());
    assert_file_is(&fx.model_path(), &fx.model);
    assert_file_is(&dir.join(TOKENIZER_FILE), &fx.tokenizer);
    assert_no_part_files(&dir);

    let total = (fx.model.len() + fx.tokenizer.len()) as u64;
    assert!(
        rec.events.len() > 4,
        "expected progress part-way through the large file, got {:?}",
        rec.events
    );
    let mut previous = 0;
    for (file, done, of) in &rec.events {
        assert_eq!(*of, total, "bytes_total is the whole artifact ({file})");
        assert!(*done >= previous, "progress went backwards at {file}");
        previous = *done;
    }
    let (last_file, last_done, last_total) = rec.events.last().unwrap();
    assert_eq!(last_done, last_total, "the last event is 100%");
    assert_eq!(last_file, TOKENIZER_FILE);
    assert_eq!(server.hits(MODEL_ROUTE).len(), 1);
    assert_eq!(server.hits(TOKENIZER_ROUTE).len(), 1);
}

#[test]
fn second_fetch_uses_the_cache_without_network() {
    let fx = Fixture::new();
    let server = fx.server();
    let artifact = fx.artifact(&server);
    fx.store().fetch(&artifact, &mut ()).expect("first fetch");
    let after_first = server.total_hits();

    // A new store on the same root: nothing carried in memory.
    let mut rec = Recorder::default();
    let dir = ModelStore::new(&fx.root)
        .fetch(&artifact, &mut rec)
        .expect("second fetch");

    assert_eq!(dir, fx.dir());
    assert_eq!(
        server.total_hits(),
        after_first,
        "the cache needs no request"
    );
    let (_, done, total) = rec.events.last().unwrap();
    assert_eq!(done, total, "a cached fetch still ends at 100%");
}

#[test]
fn cached_is_none_before_fetch_and_never_touches_the_network() {
    let fx = Fixture::new();
    let server = fx.server();
    let artifact = fx.artifact(&server);
    let store = fx.store();

    assert_eq!(store.cached(&artifact).unwrap(), None);
    assert!(
        !fx.root.exists(),
        "asking whether a model is cached must not create anything"
    );

    store.fetch(&artifact, &mut ()).unwrap();
    assert_eq!(store.cached(&artifact).unwrap(), Some(fx.dir()));
    assert_eq!(
        server.total_hits(),
        2,
        "only the fetch went to the network, one request per file"
    );
}

#[test]
fn truncated_file_is_refused_and_fetched_again() {
    let fx = Fixture::new();
    let server = fx.server();
    let artifact = fx.artifact(&server);
    let store = fx.store();
    store.fetch(&artifact, &mut ()).unwrap();

    let model = std::fs::read(fx.model_path()).unwrap();
    std::fs::write(fx.model_path(), &model[..model.len() - 1000]).unwrap();

    assert_eq!(
        store.cached(&artifact).unwrap(),
        None,
        "a short file is not cached"
    );
    assert!(fx.model_path().exists(), "cached() deletes nothing");

    store.fetch(&artifact, &mut ()).unwrap();

    assert_file_is(&fx.model_path(), &fx.model);
    assert_eq!(
        server.hits(MODEL_ROUTE).len(),
        2,
        "the bad file is fetched once more"
    );
    assert_eq!(
        server.hits(TOKENIZER_ROUTE).len(),
        1,
        "the good one is left alone"
    );
    assert_no_part_files(&fx.dir());
}

#[test]
fn wrong_hash_file_is_refused_and_fetched_again() {
    let fx = Fixture::new();
    let server = fx.server();
    let artifact = fx.artifact(&server);
    let store = fx.store();
    store.fetch(&artifact, &mut ()).unwrap();

    // Same length, one byte different: only the hash can tell.
    let mut model = std::fs::read(fx.model_path()).unwrap();
    model[5000] ^= 0xFF;
    std::fs::write(fx.model_path(), &model).unwrap();

    assert_eq!(store.cached(&artifact).unwrap(), None);
    store.fetch(&artifact, &mut ()).unwrap();

    assert_file_is(&fx.model_path(), &fx.model);
    assert_eq!(server.hits(MODEL_ROUTE).len(), 2);
    assert_eq!(server.hits(TOKENIZER_ROUTE).len(), 1);
}

#[test]
fn server_sending_wrong_bytes_is_refused() {
    let fx = Fixture::new();
    // The right length, but not the pinned content.
    let mut bad = fx.model.clone();
    bad[100] ^= 0xFF;
    let server = Server::start([
        (MODEL_ROUTE, Route::new(bad)),
        (TOKENIZER_ROUTE, Route::new(fx.tokenizer.clone())),
    ]);
    let artifact = fx.artifact(&server);

    let err = fx
        .store()
        .fetch(&artifact, &mut ())
        .expect_err("wrong bytes must not be accepted");

    match &err {
        Error::Integrity { file, reason } => {
            assert_eq!(file, MODEL_FILE);
            assert!(reason.contains("SHA-256"), "{reason}");
        }
        other => panic!("expected Integrity, got {other:?}"),
    }
    // Refused, fetched once more from the start, refused again.
    let hits = server.hits(MODEL_ROUTE);
    assert_eq!(ranges(&hits), [None, None]);
    assert!(
        !fx.model_path().exists(),
        "unverified bytes never get the final name"
    );
    assert!(!fx.model_part().exists(), "and no partial file is kept");
    assert_eq!(
        server.hits(TOKENIZER_ROUTE).len(),
        0,
        "the fetch stops at the first bad file"
    );
}

#[test]
fn server_sending_more_than_the_pinned_size_is_refused() {
    let fx = Fixture::new();
    let mut long = fx.model.clone();
    long.extend_from_slice(&[0u8; 4096]);
    let server = Server::start([
        (MODEL_ROUTE, Route::new(long)),
        (TOKENIZER_ROUTE, Route::new(fx.tokenizer.clone())),
    ]);
    let artifact = fx.artifact(&server);

    let err = fx.store().fetch(&artifact, &mut ()).expect_err("too long");

    match &err {
        Error::Integrity { reason, .. } => assert!(reason.contains("more than"), "{reason}"),
        other => panic!("expected Integrity, got {other:?}"),
    }
    assert!(!fx.model_path().exists());
    assert!(!fx.model_part().exists());
}

// ---------------------------------------------------------------------------
// Resume, restart, cancel
// ---------------------------------------------------------------------------

#[test]
fn interrupted_download_resumes_with_range() {
    let fx = Fixture::new();
    let server = Server::start([
        (MODEL_ROUTE, Route::new(fx.model.clone()).cut_after(300_000)),
        (TOKENIZER_ROUTE, Route::new(fx.tokenizer.clone())),
    ]);
    let artifact = fx.artifact(&server);
    let store = fx.store();

    let err = store
        .fetch(&artifact, &mut ())
        .expect_err("the connection was cut");
    match &err {
        Error::Download { status: None, .. } => {}
        other => panic!("expected a Download error without a status, got {other:?}"),
    }
    let kept = std::fs::metadata(fx.model_part())
        .expect("the partial file is kept")
        .len();
    assert!(
        kept > 0 && kept < fx.model.len() as u64,
        "kept {kept} bytes"
    );
    assert!(!fx.model_path().exists());

    let mut rec = Recorder::default();
    store.fetch(&artifact, &mut rec).expect("resume");

    assert_file_is(&fx.model_path(), &fx.model);
    let hits = server.hits(MODEL_ROUTE);
    assert_eq!(hits.len(), 2);
    let expected = format!("bytes={kept}-");
    assert_eq!(hits[1].range.as_deref(), Some(expected.as_str()));
    assert_no_part_files(&fx.dir());

    // Progress picks up where the partial file left off.
    let first_in_model = rec.events.iter().find(|(f, _, _)| f == MODEL_FILE).unwrap();
    assert_eq!(
        first_in_model.1, 0,
        "the file starts at 0 before the request"
    );
    assert!(
        rec.events.iter().any(|(_, done, _)| *done == kept),
        "expected an event at the resume offset {kept}: {:?}",
        rec.events
    );
}

#[test]
fn server_ignoring_range_restarts_from_zero() {
    let fx = Fixture::new();
    let server = Server::start([
        (MODEL_ROUTE, Route::new(fx.model.clone()).ignore_range()),
        (TOKENIZER_ROUTE, Route::new(fx.tokenizer.clone())),
    ]);
    let artifact = fx.artifact(&server);

    // A correct prefix is already on disk.
    let prefix = 200_000;
    std::fs::create_dir_all(fx.model_part().parent().unwrap()).unwrap();
    std::fs::write(fx.model_part(), &fx.model[..prefix]).unwrap();

    fx.store().fetch(&artifact, &mut ()).expect("fetch");

    let got = std::fs::read(fx.model_path()).unwrap();
    assert_eq!(got.len(), fx.model.len(), "no doubled prefix");
    assert_file_is(&fx.model_path(), &fx.model);
    let expected = format!("bytes={prefix}-");
    assert_eq!(
        ranges(&server.hits(MODEL_ROUTE)),
        [Some(expected.as_str())],
        "it asked for the rest, was sent everything, and used it"
    );
    assert_no_part_files(&fx.dir());
}

#[test]
fn range_not_satisfiable_restarts_from_zero() {
    let fx = Fixture::new();
    let server = Server::start([
        (MODEL_ROUTE, Route::new(fx.model.clone()).reject_range()),
        (TOKENIZER_ROUTE, Route::new(fx.tokenizer.clone())),
    ]);
    let artifact = fx.artifact(&server);
    let prefix = 150_000;
    std::fs::create_dir_all(fx.model_part().parent().unwrap()).unwrap();
    std::fs::write(fx.model_part(), &fx.model[..prefix]).unwrap();

    fx.store().fetch(&artifact, &mut ()).expect("fetch");

    assert_file_is(&fx.model_path(), &fx.model);
    let expected = format!("bytes={prefix}-");
    assert_eq!(
        ranges(&server.hits(MODEL_ROUTE)),
        [Some(expected.as_str()), None],
        "416 sends it back to the start, without a Range"
    );
}

#[test]
fn corrupt_partial_file_recovers_with_one_full_refetch() {
    let fx = Fixture::new();
    let server = fx.server();
    let artifact = fx.artifact(&server);

    // A partial file whose bytes are not a prefix of the model.
    let prefix = 250_000;
    let mut junk = fx.model[..prefix].to_vec();
    junk[10] ^= 0xFF;
    std::fs::create_dir_all(fx.model_part().parent().unwrap()).unwrap();
    std::fs::write(fx.model_part(), &junk).unwrap();

    fx.store().fetch(&artifact, &mut ()).expect("fetch");

    assert_file_is(&fx.model_path(), &fx.model);
    let expected = format!("bytes={prefix}-");
    assert_eq!(
        ranges(&server.hits(MODEL_ROUTE)),
        [Some(expected.as_str()), None],
        "one resume that fails verification, then exactly one full download"
    );
    assert_no_part_files(&fx.dir());
}

#[test]
fn a_complete_partial_file_needs_no_request() {
    let fx = Fixture::new();
    let server = fx.server();
    let artifact = fx.artifact(&server);
    // Cut off between the last byte and the rename.
    std::fs::create_dir_all(fx.model_part().parent().unwrap()).unwrap();
    std::fs::write(fx.model_part(), &fx.model).unwrap();

    fx.store().fetch(&artifact, &mut ()).expect("fetch");

    assert_file_is(&fx.model_path(), &fx.model);
    assert_eq!(server.hits(MODEL_ROUTE).len(), 0);
    assert_no_part_files(&fx.dir());
}

#[test]
fn cancel_keeps_the_partial_file_for_resume() {
    let fx = Fixture::new();
    let server = fx.server();
    let artifact = fx.artifact(&server);
    let store = fx.store();

    let mut rec = Recorder::cancelling_at(256 * 1024);
    let err = store
        .fetch(&artifact, &mut rec)
        .expect_err("cancelled part-way");

    assert!(matches!(err, Error::Cancelled), "{err:?}");
    let kept = std::fs::metadata(fx.model_part())
        .expect("the partial file is kept")
        .len();
    assert!(
        kept >= 256 * 1024 && kept < fx.model.len() as u64,
        "kept {kept} of {}",
        fx.model.len()
    );
    assert!(
        !fx.model_path().exists(),
        "a cancelled file is never given its final name"
    );
    assert_eq!(store.cached(&artifact).unwrap(), None);

    store.fetch(&artifact, &mut ()).expect("resume");
    assert_file_is(&fx.model_path(), &fx.model);
    let hits = server.hits(MODEL_ROUTE);
    let expected = format!("bytes={kept}-");
    assert_eq!(ranges(&hits), [None, Some(expected.as_str())]);
}

#[test]
fn cancel_does_not_wait_for_a_server_that_has_stopped_answering() {
    let fx = Fixture::new();
    let server = Server::start([
        (
            MODEL_ROUTE,
            Route::new(fx.model.clone()).head_delay(Duration::from_secs(30)),
        ),
        (TOKENIZER_ROUTE, Route::new(fx.tokenizer.clone())),
    ]);
    let artifact = fx.artifact(&server);

    struct AfterAWhile(Instant);
    impl FetchObserver for AfterAWhile {
        fn is_cancelled(&self) -> bool {
            self.0.elapsed() > Duration::from_millis(300)
        }
    }

    let started = Instant::now();
    let err = fx
        .store()
        .fetch(&artifact, &mut AfterAWhile(started))
        .expect_err("cancelled while waiting");

    assert!(matches!(err, Error::Cancelled), "{err:?}");
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "cancel took {:?}: it waited for the server",
        started.elapsed()
    );
    assert!(!fx.model_path().exists());
}

// ---------------------------------------------------------------------------
// Errors and hostile manifests
// ---------------------------------------------------------------------------

#[test]
fn http_error_is_reported_and_nothing_is_cached() {
    let fx = Fixture::new();
    let server = Server::start([
        (MODEL_ROUTE, Route::new(Vec::new()).status(404)),
        (TOKENIZER_ROUTE, Route::new(fx.tokenizer.clone())),
    ]);
    let artifact = fx.artifact(&server);
    let store = fx.store();

    let err = store
        .fetch(&artifact, &mut ())
        .expect_err("a 404 is not a model");

    match &err {
        Error::Download { status, url, .. } => {
            assert_eq!(*status, Some(404));
            assert_eq!(url, &server.url(MODEL_ROUTE));
        }
        other => panic!("expected Download, got {other:?}"),
    }
    assert!(!fx.model_path().exists());
    assert_no_part_files(&fx.dir());
    assert_eq!(store.cached(&artifact).unwrap(), None);
    assert_eq!(server.hits(MODEL_ROUTE).len(), 1, "a 404 is not retried");
}

#[test]
fn an_unreachable_server_is_a_download_error() {
    let fx = Fixture::new();
    let server = fx.server();
    let artifact = fx.artifact(&server);
    drop(server); // Nothing is listening any more.

    let err = fx
        .store()
        .fetch(&artifact, &mut ())
        .expect_err("nobody to ask");

    match &err {
        Error::Download {
            status: None,
            reason,
            ..
        } => assert!(!reason.is_empty()),
        other => panic!("expected Download, got {other:?}"),
    }
}

#[test]
fn manifest_cannot_escape_the_store() {
    let fx = Fixture::new();
    let server = fx.server();
    let good = fx.artifact(&server);
    let store = fx.store();

    let with_path = |path: &str| {
        let mut a = good.clone();
        a.files[0].path = path.into();
        a
    };
    let with_id = |id: &str| {
        let mut a = good.clone();
        a.id = id.into();
        a
    };
    let mut cases: Vec<(&str, ModelArtifact)> = vec![
        ("dot-dot", with_path("../x")),
        ("absolute", with_path("/abs")),
        ("traversal in the middle", with_path("a/../../b")),
        ("empty path", with_path("")),
        ("backslash", with_path("a\\b")),
        ("windows drive", with_path("C:\\x")),
        ("drive without a slash", with_path("C:x")),
        ("current dir", with_path("./a")),
        ("empty segment", with_path("a//b")),
        ("trailing slash", with_path("a/")),
        ("partial suffix", with_path("a.PART")),
        ("id with dot-dot", with_id("../x")),
        ("id with a slash", with_id("a/b")),
        ("id with capitals", with_id("Model")),
        ("empty id", with_id("")),
        ("id starting with a dot", with_id(".hidden")),
    ];
    let bad_hash = |h: &str| {
        let mut a = good.clone();
        a.files[0].sha256 = h.into();
        a
    };
    cases.push(("short sha256", bad_hash("abc")));
    cases.push((
        "uppercase sha256",
        bad_hash(&sha256_hex(&fx.model).to_uppercase()),
    ));
    cases.push(("non-hex sha256", bad_hash(&"z".repeat(64))));
    let mut dup = good.clone();
    dup.files[1].path = MODEL_FILE.to_uppercase();
    cases.push(("duplicate path, ignoring case", dup));
    let mut nested = good.clone();
    nested.files[1].path = format!("{MODEL_FILE}/inner");
    cases.push(("a file inside a file", nested));
    let mut empty = good.clone();
    empty.files.clear();
    cases.push(("no files", empty));
    let mut zero = good.clone();
    zero.files[0].size = 0;
    cases.push(("zero size", zero));
    let with_url = |url: &str| {
        let mut a = good.clone();
        a.files[0].url = url.into();
        a
    };
    cases.push(("non-http url", with_url("file:///etc/passwd")));
    // Model files come over https. Plain http is for this machine only.
    cases.push((
        "plain http to a remote host",
        with_url("http://example.com/model.onnx"),
    ));
    cases.push((
        "loopback name as a prefix of a remote host",
        with_url("http://localhost.example.com/model.onnx"),
    ));
    cases.push((
        "loopback address as the userinfo of a remote host",
        with_url("http://127.0.0.1@example.com/model.onnx"),
    ));
    cases.push(("url with no host", with_url("https://")));
    // Artifact `x` locks `<root>/x.lock`, so an artifact called `x.lock`
    // would need that very path for its directory.
    cases.push(("id that is another artifact's lock file", with_id("x.lock")));

    for (name, artifact) in &cases {
        assert!(
            matches!(
                store.fetch(artifact, &mut ()),
                Err(Error::InvalidManifest(_))
            ),
            "{name}: fetch accepted it"
        );
        assert!(
            matches!(store.cached(artifact), Err(Error::InvalidManifest(_))),
            "{name}: cached accepted it"
        );
        assert!(
            matches!(store.artifact_dir(artifact), Err(Error::InvalidManifest(_))),
            "{name}: artifact_dir accepted it"
        );
    }
    assert_eq!(
        server.total_hits(),
        0,
        "an invalid manifest makes no request"
    );
    assert!(!fx.root.exists(), "and creates nothing");
    let parent = fx.root.parent().unwrap();
    for stray in ["x", "b", "abs"] {
        assert!(
            !parent.join(stray).exists(),
            "{stray} was created outside the store"
        );
    }
}

#[test]
fn only_https_and_loopback_http_urls_are_accepted() {
    let fx = Fixture::new();
    let server = fx.server();
    let store = fx.store();
    let with_url = |url: &str| {
        let mut a = fx.artifact(&server);
        a.files[0].url = url.into();
        a
    };

    for ok in [
        "https://example.com/m.onnx",
        "https://huggingface.co/a/b/resolve/main/m.onnx?download=true",
        "http://127.0.0.1:8080/m.onnx",
        "http://localhost:8080/m.onnx",
        "http://[::1]:8080/m.onnx",
    ] {
        store
            .artifact_dir(&with_url(ok))
            .unwrap_or_else(|e| panic!("{ok} was refused: {e}"));
    }
    let err = store
        .artifact_dir(&with_url("http://example.com/m.onnx"))
        .expect_err("plain http to a remote host");
    assert!(
        err.to_string().contains("https"),
        "the error should say what is allowed: {err}"
    );
}

// ---------------------------------------------------------------------------
// Concurrency
// ---------------------------------------------------------------------------

#[test]
fn concurrent_fetches_of_one_artifact_download_once() {
    let fx = Fixture::new();
    let server = Server::start([
        (
            MODEL_ROUTE,
            // Slow enough that the other threads arrive mid-download.
            Route::new(fx.model.clone()).chunk_delay(Duration::from_millis(10)),
        ),
        (TOKENIZER_ROUTE, Route::new(fx.tokenizer.clone())),
    ]);
    let artifact = fx.artifact(&server);
    let store = fx.store();

    let results: Vec<_> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..4)
            .map(|_| s.spawn(|| store.fetch(&artifact, &mut ())))
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });

    for r in &results {
        assert_eq!(r.as_ref().expect("every caller succeeds"), &fx.dir());
    }
    assert_file_is(&fx.model_path(), &fx.model);
    assert_eq!(server.hits(MODEL_ROUTE).len(), 1, "one download, not four");
    assert_eq!(server.hits(TOKENIZER_ROUTE).len(), 1);
}

#[test]
fn fetch_works_from_inside_a_tokio_runtime() {
    // Tools run inside the async `send_message` command. `reqwest::blocking`
    // panics when used on a thread that is inside a runtime, which is why
    // the request runs on its own thread. This is the test that panics if
    // that thread is ever removed.
    let fx = Fixture::new();
    let server = fx.server();
    let artifact = fx.artifact(&server);
    let store = fx.store();

    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("runtime");
    let dir = rt
        .block_on(async { store.fetch(&artifact, &mut ()) })
        .expect("fetch inside a runtime");

    assert_eq!(dir, fx.dir());
    assert_file_is(&fx.model_path(), &fx.model);
}

// ---------------------------------------------------------------------------
// The function callers use
// ---------------------------------------------------------------------------

/// `fetched_model_path` used to be `fn(&str) -> Err(MissingRuntime)` and
/// nothing else, whatever it was given.
#[test]
fn fetched_model_path_downloads_and_verifies() {
    let fx = Fixture::new();
    let server = fx.server();
    let artifact = fx.artifact(&server);
    let store = fx.store();

    let dir = fetched_model_path(&store, &artifact, &mut ()).expect("fetch");

    assert_eq!(dir, fx.dir());
    assert_file_is(&fx.model_path(), &fx.model);
    assert_file_is(&dir.join(TOKENIZER_FILE), &fx.tokenizer);
}

// ---------------------------------------------------------------------------
// Redirects
// ---------------------------------------------------------------------------

const HOPS: [&str; 6] = ["/hop0", "/hop1", "/hop2", "/hop3", "/hop4", "/hop5"];

/// A server where `HOPS[0]` starts a chain of `redirects` redirects, each
/// hop pointing at the next and the last at the model.
fn redirect_chain(fx: &Fixture, redirects: usize) -> Server {
    assert!((1..=HOPS.len()).contains(&redirects));
    let mut routes: Vec<(&'static str, Route)> = Vec::new();
    for (i, hop) in HOPS.iter().take(redirects).enumerate() {
        let to = if i + 1 == redirects {
            MODEL_ROUTE
        } else {
            HOPS[i + 1]
        };
        routes.push((*hop, Route::new(Vec::new()).redirect(to)));
    }
    routes.push((MODEL_ROUTE, Route::new(fx.model.clone())));
    routes.push((TOKENIZER_ROUTE, Route::new(fx.tokenizer.clone())));
    Server::start(routes)
}

#[test]
fn a_few_redirects_are_followed() {
    let fx = Fixture::new();
    let server = redirect_chain(&fx, 5);
    let mut artifact = fx.artifact(&server);
    artifact.files[0].url = server.url(HOPS[0]);

    fx.store().fetch(&artifact, &mut ()).expect("fetch");

    assert_file_is(&fx.model_path(), &fx.model);
    assert_eq!(server.hits(MODEL_ROUTE).len(), 1);
}

#[test]
fn a_redirect_chain_over_the_limit_is_a_download_error() {
    let fx = Fixture::new();
    // Six redirects, one more than allowed.
    let server = redirect_chain(&fx, 6);
    let mut artifact = fx.artifact(&server);
    artifact.files[0].url = server.url(HOPS[0]);

    let err = fx
        .store()
        .fetch(&artifact, &mut ())
        .expect_err("too many redirects");

    match &err {
        Error::Download {
            status: None,
            reason,
            ..
        } => assert!(reason.contains("redirect"), "{reason}"),
        other => panic!("expected a Download error, got {other:?}"),
    }
    assert_eq!(
        server.hits(MODEL_ROUTE).len(),
        0,
        "the model was never reached"
    );
    assert!(!fx.model_path().exists());
    assert_no_part_files(&fx.dir());
}

#[test]
fn a_redirect_to_plain_http_on_another_host_is_refused() {
    let fx = Fixture::new();
    // `.invalid` never resolves, so if this were followed the failure would
    // be a DNS error. The refusal has to come first, and has to say why.
    let server = Server::start([
        (
            MODEL_ROUTE,
            Route::new(Vec::new()).redirect("http://example.invalid/model.onnx"),
        ),
        (TOKENIZER_ROUTE, Route::new(fx.tokenizer.clone())),
    ]);
    let artifact = fx.artifact(&server);

    let err = fx
        .store()
        .fetch(&artifact, &mut ())
        .expect_err("a downgrade to http");

    match &err {
        Error::Download { reason, .. } => assert!(reason.contains("https only"), "{reason}"),
        other => panic!("expected a Download error, got {other:?}"),
    }
    assert!(!fx.model_path().exists());
}

// ---------------------------------------------------------------------------
// Another writer on the same files
// ---------------------------------------------------------------------------

/// What a second writer does to the `.part` file of the model.
#[derive(Clone, Copy, Debug)]
enum Tamper {
    /// Overwrite bytes the download has already written. The length stays
    /// right, so only a hash can tell.
    Overwrite,
    /// Truncate the file to nothing. The download carries on writing at its
    /// own offset, leaving a hole of zeros behind it.
    Truncate,
    /// Extend the file to this many bytes, past the pinned size.
    Grow(u64),
}

/// A second writer on the `.part` while `fetch` streams into it, standing in
/// for another edytlab process fetching the same artifact (open and append on
/// a resume, or `File::create` on a restart).
///
/// Acts part-way through the model, once per download attempt when `repeat`
/// is set and only on the first attempt otherwise. A fetch that starts a
/// file over reports progress from zero again, which re-arms it.
struct SecondWriter {
    part: PathBuf,
    how: Tamper,
    repeat: bool,
    armed: bool,
    acted: u32,
}

impl SecondWriter {
    fn new(part: PathBuf, how: Tamper, repeat: bool) -> Self {
        Self {
            part,
            how,
            repeat,
            armed: true,
            acted: 0,
        }
    }
}

impl FetchObserver for SecondWriter {
    fn on_progress(&mut self, p: &FetchProgress<'_>) {
        if p.file != MODEL_FILE {
            return;
        }
        if p.bytes_done < 400_000 {
            self.armed = self.repeat || self.acted == 0;
            return;
        }
        if !self.armed {
            return;
        }
        self.armed = false;
        self.acted += 1;
        let mut f = OpenOptions::new()
            .write(true)
            .open(&self.part)
            .expect("the partial file is there to be tampered with");
        match self.how {
            Tamper::Overwrite => {
                f.seek(SeekFrom::Start(10)).unwrap();
                f.write_all(&[0xA5; 64]).unwrap();
            }
            Tamper::Truncate => f.set_len(0).unwrap(),
            Tamper::Grow(to) => f.set_len(to).unwrap(),
        }
    }
}

type TamperFor = fn(u64) -> Tamper;

/// Each way a second writer can leave the `.part`, given the pinned size.
const TAMPERS: [(&str, TamperFor); 3] = [
    ("overwritten in place", |_| Tamper::Overwrite),
    ("truncated", |_| Tamper::Truncate),
    ("grown past the pinned size", |size| {
        Tamper::Grow(size + 4096)
    }),
];

/// Whatever a second writer does to the `.part`, a fetch that returns `Ok`
/// has put a verified file at the final name. The hash `finish` checks is
/// the one of the bytes on disk, not of the bytes this process streamed.
#[test]
fn a_second_writer_never_gets_an_unverified_file_renamed_into_place() {
    for (what, tamper) in TAMPERS {
        let fx = Fixture::new();
        let server = fx.server();
        let artifact = fx.artifact(&server);
        let store = fx.store();
        let size = fx.model.len() as u64;

        let mut meddler = SecondWriter::new(fx.model_part(), tamper(size), false);
        let result = store.fetch(&artifact, &mut meddler);

        assert_eq!(meddler.acted, 1, "{what}: the second writer never ran");
        let dir = result.unwrap_or_else(|e| panic!("{what}: the second attempt is clean: {e}"));
        assert_eq!(dir, fx.dir());
        assert!(
            store.cached(&artifact).unwrap().is_some(),
            "{what}: fetch returned Ok but the artifact does not verify"
        );
        assert_file_is(&fx.model_path(), &fx.model);
        assert_eq!(
            server.hits(MODEL_ROUTE).len(),
            2,
            "{what}: the corrupt file is refused and fetched again, once"
        );
        assert_no_part_files(&dir);
    }
}

#[test]
fn a_second_writer_that_keeps_interfering_is_an_integrity_error() {
    for (what, tamper) in TAMPERS {
        let fx = Fixture::new();
        let server = fx.server();
        let artifact = fx.artifact(&server);
        let store = fx.store();
        let size = fx.model.len() as u64;

        let mut meddler = SecondWriter::new(fx.model_part(), tamper(size), true);
        let err = store
            .fetch(&artifact, &mut meddler)
            .expect_err("the file is wrong both times");

        match &err {
            Error::Integrity { file, reason } => {
                assert_eq!(file, MODEL_FILE, "{what}");
                assert!(reason.contains("on disk"), "{what}: {reason}");
            }
            other => panic!("{what}: expected Integrity, got {other:?}"),
        }
        assert!(
            !fx.model_path().exists(),
            "{what}: an unverified file was given its final name"
        );
        assert_eq!(store.cached(&artifact).unwrap(), None);
        assert_no_part_files(&fx.dir());
    }
}

// ---------------------------------------------------------------------------
// The cross-process lock
// ---------------------------------------------------------------------------

fn lock_file_path(root: &Path) -> PathBuf {
    root.join("tiny-model-1.lock")
}

/// Take the lock another edytlab process would hold while fetching
/// `tiny-model-1`. A second open of the file is another holder to the OS
/// (`flock` and `LockFileEx` both lock per open file, not per process), so
/// this runs into the same thing a second process would. Dropping the
/// returned file releases it.
fn hold_the_lock_like_another_process(root: &Path) -> File {
    std::fs::create_dir_all(root).unwrap();
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock_file_path(root))
        .unwrap();
    FileExt::try_lock(&file).expect("nobody else holds the lock yet");
    file
}

#[test]
fn a_fetch_waits_while_another_process_holds_the_artifact_lock() {
    let fx = Fixture::new();
    let server = fx.server();
    let artifact = fx.artifact(&server);
    let store = fx.store();

    std::thread::scope(|s| {
        // Taken inside the scope so that a failed assertion below releases
        // it while unwinding. Otherwise the scope would wait for ever on a
        // fetch that is waiting for the lock.
        let other = hold_the_lock_like_another_process(&fx.root);

        let fetch = s.spawn(|| store.fetch(&artifact, &mut ()));
        std::thread::sleep(Duration::from_millis(600));
        assert!(
            !fetch.is_finished(),
            "the fetch did not wait for the lock holder"
        );
        assert_eq!(server.total_hits(), 0, "it downloaded while locked out");
        assert!(!fx.dir().exists(), "it wrote while locked out");

        drop(other);
        let dir = fetch
            .join()
            .unwrap()
            .expect("the fetch goes ahead once the lock is free");
        assert_eq!(dir, fx.dir());
    });

    assert_file_is(&fx.model_path(), &fx.model);
    assert_eq!(server.hits(MODEL_ROUTE).len(), 1);
}

#[test]
fn waiting_for_the_artifact_lock_can_be_cancelled() {
    let fx = Fixture::new();
    let server = fx.server();
    let artifact = fx.artifact(&server);
    let _other = hold_the_lock_like_another_process(&fx.root);

    struct AfterAWhile(Instant);
    impl FetchObserver for AfterAWhile {
        fn is_cancelled(&self) -> bool {
            self.0.elapsed() > Duration::from_millis(300)
        }
    }

    let started = Instant::now();
    let err = fx
        .store()
        .fetch(&artifact, &mut AfterAWhile(started))
        .expect_err("cancelled while waiting for the lock");

    assert!(matches!(err, Error::Cancelled), "{err:?}");
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "cancel took {:?}: it blocked on the lock",
        started.elapsed()
    );
    assert_eq!(server.total_hits(), 0);
    assert!(!fx.dir().exists());
}

#[test]
fn the_lock_is_released_however_a_fetch_ends() {
    let fx = Fixture::new();
    let server = fx.server();
    let artifact = fx.artifact(&server);
    let store = fx.store();
    let lock_is_free = || {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock_file_path(&fx.root))
            .expect("open the lock file");
        FileExt::try_lock(&file).is_ok()
    };

    store.fetch(&artifact, &mut ()).expect("fetch");
    assert!(
        lock_file_path(&fx.root).is_file(),
        "a fetch leaves its lock file behind: removing it would let two processes lock two files"
    );
    assert!(lock_is_free(), "still held after a successful fetch");

    // A failure.
    std::fs::remove_file(fx.model_path()).unwrap();
    server.set_route(MODEL_ROUTE, Route::new(Vec::new()).status(500));
    store.fetch(&artifact, &mut ()).expect_err("a 500");
    assert!(lock_is_free(), "still held after a failed fetch");

    // A cancel.
    server.set_route(MODEL_ROUTE, Route::new(fx.model.clone()));
    let mut rec = Recorder::cancelling_at(256 * 1024);
    let err = store.fetch(&artifact, &mut rec).expect_err("cancelled");
    assert!(matches!(err, Error::Cancelled), "{err:?}");
    assert!(lock_is_free(), "still held after a cancelled fetch");
}
