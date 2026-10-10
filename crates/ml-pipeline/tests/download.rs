//! The model downloader, against a loopback HTTP server (#383).
//!
//! Every test serves a small artifact from `support::Server` on
//! 127.0.0.1, so CI downloads nothing. Each has its own temp directory
//! and its own server. What the tests assert on is mostly the server's
//! record of requests: "resumed" means the second request carried
//! `Range: bytes=N-`, and "used the cache" means there was no request.

mod support;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

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
    let mut scheme = good.clone();
    scheme.files[0].url = "file:///etc/passwd".into();
    cases.push(("non-http url", scheme));

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
