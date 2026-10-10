//! Verified, resumable model downloads.
//!
//! A [`ModelArtifact`] pins what a model is: every file's URL, size and
//! SHA-256. [`ModelStore::fetch`] makes that artifact exist on disk under
//! `<root>/<id>/`, or fails saying why. A file is only ever at its final
//! name once it has the pinned size and hash:
//!
//! - Bytes are streamed to `<file>.part`, hashed as they arrive.
//! - A download that stops (network error, cancel, crash) leaves the
//!   `.part` behind. The next fetch asks for the rest with a `Range`
//!   header, after re-hashing what is already there. A server that
//!   ignores the range (200) or refuses it (416) just means starting over.
//! - When the byte count reaches the pinned size, the hash is compared
//!   and only then is the `.part` renamed into place. A mismatch is
//!   refused and the file is fetched again, once, from scratch; a second
//!   mismatch is an [`Error::Integrity`].
//! - A file already at its final name is trusted only after it is
//!   re-hashed. A truncated or tampered one is deleted and fetched again.
//!
//! [`ModelStore::cached`] answers "is it all here?" without any network
//! and without deleting anything.
//!
//! ## Guarantees worth knowing
//!
//! - **The manifest is validated before any I/O.** An artifact id or file
//!   path that could leave the store (`..`, an absolute path, a
//!   backslash, a drive letter) is an [`Error::InvalidManifest`].
//! - **Fetches of one artifact are single-flight** within the process: a
//!   second caller waits, then finds the cache already valid. Locking
//!   across processes is out of scope; edytlab is a single-instance app.
//! - **It is safe to call from async code.** `reqwest::blocking` panics
//!   when used on a thread that is inside a tokio runtime, and tools run
//!   inside the async `send_message` command. So the HTTP request runs on
//!   its own thread and the caller only waits on a channel, the same shape
//!   as `crates/mcp/src/http.rs`. That thread is also why cancelling
//!   returns promptly even if the connection is stalled.
//! - **Cancellation** is polled through [`FetchObserver::is_cancelled`]
//!   after each chunk and while waiting for the server to answer. It keeps
//!   the `.part` for a later resume.
//!
//! ## Status
//!
//! Nothing calls this yet and no model is pinned: shipping a manifest
//! needs the model files, and the transcription and stem-separation code
//! that would use them is not implemented (#384, #385). The download UI
//! and the Tauri command that would drive it are later parts of #383.
//! Tools must never download from inside a call, since a model is
//! hundreds of megabytes and a tool runs holding the session and engine
//! locks.

use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::sync::Mutex;
use std::time::Duration;

use sha2::{Digest, Sha256};

use crate::{Error, Result};

/// How long the HTTP thread waits to establish a connection.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// How long one `read` may block before the download counts as stalled.
///
/// This is not a limit on the whole download. In `reqwest::blocking` the
/// client timeout applies to each `Read::read` of the body (and to waiting
/// for the response headers), so a slow connection that keeps delivering
/// bytes is never cut off. That is only true when the body is streamed
/// through `Read`: `Response::bytes()` applies it to the whole body.
const STALL_TIMEOUT: Duration = Duration::from_secs(60);

/// Size of one read from the socket, and so of one channel message.
const CHUNK: usize = 64 * 1024;

/// Progress is reported once at least this many bytes have arrived since
/// the last report.
const REPORT_EVERY: u64 = 256 * 1024;

/// How often a waiting caller looks at [`FetchObserver::is_cancelled`].
const CANCEL_POLL: Duration = Duration::from_millis(50);

/// One file of a model artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelFile {
    /// Path under the artifact's directory: relative, `/`-separated,
    /// no `.` or `..` components.
    pub path: String,
    /// Where to download it from.
    pub url: String,
    /// Expected SHA-256 of the whole file: 64 lowercase hex digits.
    pub sha256: String,
    /// Expected size in bytes.
    pub size: u64,
}

/// A named set of files that together make one model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelArtifact {
    /// Directory name under the store root: lowercase letters, digits,
    /// `.`, `_` and `-`, starting with a letter or digit.
    pub id: String,
    pub files: Vec<ModelFile>,
}

/// A progress event from [`ModelStore::fetch`].
///
/// `bytes_done` counts verified-or-in-flight bytes across the whole
/// artifact, so it climbs to `bytes_total`. It only goes backwards when
/// a download has to start over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FetchProgress<'a> {
    pub artifact: &'a str,
    /// The file being worked on.
    pub file: &'a str,
    pub bytes_done: u64,
    pub bytes_total: u64,
}

/// Receives progress and can cancel a fetch. Both methods default to doing
/// nothing, and `()` is the observer that ignores everything.
pub trait FetchObserver {
    /// Called at the start of each file, whenever 256 KiB or more has
    /// arrived since the last call, and at the end of each file. The last
    /// call of a successful fetch has `bytes_done == bytes_total`.
    fn on_progress(&mut self, _progress: &FetchProgress<'_>) {}

    /// Polled after every chunk, and while waiting for the server. Return
    /// `true` to stop: the fetch ends with [`Error::Cancelled`] and keeps
    /// its partial file so a later fetch can resume.
    fn is_cancelled(&self) -> bool {
        false
    }
}

impl FetchObserver for () {}

/// A directory of downloaded model artifacts.
#[derive(Debug, Clone)]
pub struct ModelStore {
    root: PathBuf,
}

impl ModelStore {
    /// A store rooted at `root`. Nothing is created until a fetch.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Where `artifact` lives: `<root>/<id>`. Validates the manifest, so a
    /// hostile id cannot name a directory outside the store.
    pub fn artifact_dir(&self, artifact: &ModelArtifact) -> Result<PathBuf> {
        validate(artifact)?;
        Ok(self.root.join(&artifact.id))
    }

    /// The artifact's directory if every file is there with its pinned
    /// size and SHA-256, otherwise `None`.
    ///
    /// Makes no network request and deletes nothing. It does read every
    /// file to hash it, which for a model is a fraction of a second per
    /// hundred megabytes, so do not call it per frame.
    pub fn cached(&self, artifact: &ModelArtifact) -> Result<Option<PathBuf>> {
        let dir = self.artifact_dir(artifact)?;
        for file in &artifact.files {
            if !is_verified(&dir.join(&file.path), file)? {
                return Ok(None);
            }
        }
        Ok(Some(dir))
    }

    /// Make `artifact` exist under [`Self::artifact_dir`], downloading
    /// whatever is missing, partial or wrong, and return that directory.
    ///
    /// Blocks until done. Safe to call from inside a tokio runtime (see
    /// the module doc), but do not call it from a tool: it can run for
    /// minutes. Errors: [`Error::InvalidManifest`] (nothing touched),
    /// [`Error::Download`] (network or HTTP; a partial file is kept for
    /// resume), [`Error::Integrity`] (the server's bytes were wrong twice
    /// in a row), [`Error::Cancelled`], and [`Error::Io`] for the disk.
    pub fn fetch(
        &self,
        artifact: &ModelArtifact,
        observer: &mut dyn FetchObserver,
    ) -> Result<PathBuf> {
        let dir = self.artifact_dir(artifact)?;
        let _single_flight = lock_artifact(&dir, observer)?;

        // `validate` has checked that the sizes add up without overflow.
        let total: u64 = artifact.files.iter().map(|f| f.size).sum();
        let mut done = 0u64;
        fs::create_dir_all(&dir)?;

        for file in &artifact.files {
            if observer.is_cancelled() {
                return Err(Error::Cancelled);
            }
            let report = |observer: &mut dyn FetchObserver, bytes_done: u64| {
                observer.on_progress(&FetchProgress {
                    artifact: &artifact.id,
                    file: &file.path,
                    bytes_done,
                    bytes_total: total,
                });
            };
            report(observer, done);

            let final_path = dir.join(&file.path);
            if let Some(parent) = final_path.parent() {
                fs::create_dir_all(parent)?;
            }

            if final_path.exists() {
                if is_verified(&final_path, file)? {
                    // A finished file with a leftover partial is stale.
                    let _ = fs::remove_file(part_path(&final_path));
                    done += file.size;
                    report(observer, done);
                    continue;
                }
                tracing::warn!(
                    artifact = %artifact.id,
                    file = %file.path,
                    "cached model file has the wrong size or hash; fetching it again"
                );
                remove_existing(&final_path)?;
            }

            let ctx = FileJob {
                artifact: &artifact.id,
                file,
                final_path: &final_path,
                part: part_path(&final_path),
                base: done,
                total,
            };
            download_file(&ctx, observer)?;
            done += file.size;
            report(observer, done);
        }
        debug_assert_eq!(done, total);
        Ok(dir)
    }
}

/// Resolve `artifact` to a verified local directory, downloading it if
/// needed. [`ModelStore::fetch`] under the name callers asked for.
pub fn fetched_model_path(
    store: &ModelStore,
    artifact: &ModelArtifact,
    observer: &mut dyn FetchObserver,
) -> Result<PathBuf> {
    store.fetch(artifact, observer)
}

// ---------------------------------------------------------------------
// Manifest validation
// ---------------------------------------------------------------------

fn invalid<T>(reason: impl Into<String>) -> Result<T> {
    Err(Error::InvalidManifest(reason.into()))
}

fn validate(artifact: &ModelArtifact) -> Result<()> {
    let id = &artifact.id;
    let id_ok = id
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-'))
        && !id.contains("..");
    if !id_ok {
        return invalid(format!(
            "artifact id {id:?} must be lowercase letters, digits, '.', '_' or '-', start with a \
             letter or digit, and not contain \"..\""
        ));
    }
    if artifact.files.is_empty() {
        return invalid(format!("artifact {id:?} has no files"));
    }

    let mut seen: Vec<String> = Vec::with_capacity(artifact.files.len());
    let mut total = 0u64;
    for file in &artifact.files {
        validate_path(&file.path)?;
        if !(file.url.starts_with("https://") || file.url.starts_with("http://")) {
            return invalid(format!(
                "{:?}: url must be http or https, got {:?}",
                file.path, file.url
            ));
        }
        let hex_ok = file.sha256.len() == 64
            && file
                .sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        if !hex_ok {
            return invalid(format!(
                "{:?}: sha256 must be 64 lowercase hex digits",
                file.path
            ));
        }
        if file.size == 0 {
            return invalid(format!("{:?}: size must be greater than zero", file.path));
        }
        total = match total.checked_add(file.size) {
            Some(t) => t,
            None => return invalid("file sizes add up to more than a u64"),
        };

        // Compared case-insensitively: the store may sit on a filesystem
        // that treats `A` and `a` as one file.
        let key = file.path.to_ascii_lowercase();
        for other in &seen {
            if *other == key {
                return invalid(format!("{:?} is listed twice", file.path));
            }
            if other.starts_with(&format!("{key}/")) || key.starts_with(&format!("{other}/")) {
                return invalid(format!(
                    "{:?} is both a file and a directory holding another file",
                    file.path
                ));
            }
        }
        seen.push(key);
    }
    Ok(())
}

fn validate_path(path: &str) -> Result<()> {
    if path.is_empty() {
        return invalid("a file path is empty");
    }
    if path.contains('\\') || path.contains(':') || path.chars().any(char::is_control) {
        return invalid(format!(
            "file path {path:?} must use '/' separators and no ':' or control characters"
        ));
    }
    // `.part` is the suffix of the partial file, so a final name with it
    // would collide with another file's partial.
    if path.to_ascii_lowercase().ends_with(".part") {
        return invalid(format!("file path {path:?} must not end in \".part\""));
    }
    let p = Path::new(path);
    if p.is_absolute() || p.has_root() {
        return invalid(format!("file path {path:?} must be relative"));
    }
    // `Component::Normal` only: no `..`, `.`, root or drive prefix. An
    // empty segment ("a//b") or trailing slash is dropped by `components`
    // and would hide a mistake, so require the text to round-trip.
    let mut rebuilt = Vec::new();
    for c in p.components() {
        match c {
            Component::Normal(s) => rebuilt.push(s.to_string_lossy().into_owned()),
            _ => {
                return invalid(format!(
                    "file path {path:?} must not contain '.', '..' or a root"
                ))
            }
        }
    }
    if rebuilt.join("/") != path {
        return invalid(format!("file path {path:?} is not in canonical form"));
    }
    Ok(())
}

// ---------------------------------------------------------------------
// Locking
// ---------------------------------------------------------------------

/// The artifact directories being fetched right now by this process.
static IN_FLIGHT: Mutex<BTreeSet<PathBuf>> = Mutex::new(BTreeSet::new());

/// Held for the length of a fetch of one artifact directory; dropping it
/// (also when the fetch panics) lets the next caller in.
struct Flight(PathBuf);

impl Drop for Flight {
    fn drop(&mut self) {
        IN_FLIGHT
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.0);
    }
}

/// Wait until no other fetch of `dir` is running in this process, then
/// claim it.
///
/// Polls rather than blocks on a lock, so a caller waiting behind a long
/// download can still be cancelled.
fn lock_artifact(dir: &Path, observer: &dyn FetchObserver) -> Result<Flight> {
    loop {
        let claimed = IN_FLIGHT
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(dir.to_path_buf());
        if claimed {
            return Ok(Flight(dir.to_path_buf()));
        }
        if observer.is_cancelled() {
            return Err(Error::Cancelled);
        }
        std::thread::sleep(CANCEL_POLL);
    }
}

// ---------------------------------------------------------------------
// One file
// ---------------------------------------------------------------------

struct FileJob<'a> {
    artifact: &'a str,
    file: &'a ModelFile,
    final_path: &'a Path,
    part: PathBuf,
    /// Bytes of the artifact finished before this file.
    base: u64,
    total: u64,
}

fn part_path(final_path: &Path) -> PathBuf {
    // Appended to the whole file name. `with_extension` would replace an
    // existing one, so `model.onnx` and `model.bin` would share a partial.
    let mut name = final_path.file_name().unwrap_or_default().to_os_string();
    name.push(".part");
    final_path.with_file_name(name)
}

fn remove_existing(path: &Path) -> Result<()> {
    if path.is_dir() {
        fs::remove_dir_all(path)?;
    } else {
        fs::remove_file(path)?;
    }
    Ok(())
}

/// Whether `path` is a regular file with the pinned size and SHA-256.
fn is_verified(path: &Path, file: &ModelFile) -> Result<bool> {
    let meta = match fs::metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e.into()),
    };
    if !meta.is_file() || meta.len() != file.size {
        return Ok(false);
    }
    let mut hasher = Sha256::new();
    hash_file_into(path, &mut hasher)?;
    Ok(hex(&hasher.finalize()) == file.sha256)
}

fn hash_file_into(path: &Path, hasher: &mut Sha256) -> Result<()> {
    let mut f = File::open(path)?;
    let mut buf = vec![0u8; CHUNK];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            return Ok(());
        }
        hasher.update(&buf[..n]);
    }
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}

/// Download one file to its `.part`, verify it, and rename it into place.
///
/// Resumes a partial file if there is one. If the bytes then fail
/// verification, it starts over from nothing, once.
fn download_file(job: &FileJob<'_>, observer: &mut dyn FetchObserver) -> Result<()> {
    match attempt(job, observer, true) {
        Err(Error::Integrity { file, reason }) => {
            tracing::warn!(
                file = %file,
                %reason,
                "downloaded model file failed verification; fetching it again from the start"
            );
            let _ = fs::remove_file(&job.part);
            match attempt(job, observer, false) {
                Err(e @ Error::Integrity { .. }) => {
                    let _ = fs::remove_file(&job.part);
                    Err(e)
                }
                other => other,
            }
        }
        other => other,
    }
}

fn integrity(job: &FileJob<'_>, reason: impl Into<String>) -> Error {
    Error::Integrity {
        file: job.file.path.clone(),
        reason: reason.into(),
    }
}

fn download_error(job: &FileJob<'_>, status: Option<u16>, reason: impl Into<String>) -> Error {
    Error::Download {
        url: job.file.url.clone(),
        status,
        reason: reason.into(),
    }
}

/// One pass at getting the file: pick the start offset, ask the server,
/// stream, verify, rename.
fn attempt(job: &FileJob<'_>, observer: &mut dyn FetchObserver, resume: bool) -> Result<()> {
    let size = job.file.size;

    let mut from = if resume {
        match fs::metadata(&job.part) {
            Ok(m) if m.is_file() => m.len(),
            Ok(_) => {
                remove_existing(&job.part)?;
                0
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => 0,
            Err(e) => return Err(e.into()),
        }
    } else {
        let _ = fs::remove_file(&job.part);
        0
    };
    if from > size {
        // Longer than the file can be, so it is not a prefix of it.
        let _ = fs::remove_file(&job.part);
        from = 0;
    }

    let mut hasher = Sha256::new();
    if from > 0 {
        hash_file_into(&job.part, &mut hasher)?;
    }
    if from == size {
        // The whole file is already here (a download that was cut off
        // between the last byte and the rename). No request needed.
        return finish(job, hasher);
    }

    let rx = spawn_worker(job.file.url.clone(), from).map_err(|e| {
        download_error(
            job,
            None,
            format!("could not start the download thread: {e}"),
        )
    })?;

    // The server's answer decides where we are.
    let (status, content_range) = match wait_for(&rx, observer)? {
        Msg::Head {
            status,
            content_range,
        } => (status, content_range),
        Msg::Fail(reason) => return Err(download_error(job, None, reason)),
        _ => {
            return Err(download_error(
                job,
                None,
                "download thread sent data before a status",
            ))
        }
    };
    let mut written = from;
    let mut out = match status {
        206 if from > 0 && range_starts_at(content_range.as_deref(), from) => {
            OpenOptions::new().append(true).open(&job.part)?
        }
        // A partial answer that does not start where we asked cannot be
        // appended. Start over, which the caller does for an Integrity.
        206 if from > 0 => {
            return Err(integrity(
                job,
                format!(
                    "the server answered a request from byte {from} with Content-Range {content_range:?}"
                ),
            ));
        }
        200 => {
            // Full body: either the first request, or a server that
            // ignored `Range`. Whatever we had is not a prefix to keep.
            written = 0;
            hasher = Sha256::new();
            File::create(&job.part)?
        }
        416 if from > 0 => {
            // The server says our offset is past its file. Start over.
            drop(rx);
            let _ = fs::remove_file(&job.part);
            return attempt(job, observer, false);
        }
        other => {
            let reason = reqwest::StatusCode::from_u16(other)
                .ok()
                .and_then(|s| s.canonical_reason())
                .unwrap_or("unexpected response");
            return Err(download_error(job, Some(other), reason));
        }
    };

    let mut since_report = 0u64;
    let report = |observer: &mut dyn FetchObserver, written: u64| {
        observer.on_progress(&FetchProgress {
            artifact: job.artifact,
            file: &job.file.path,
            bytes_done: job.base + written,
            bytes_total: job.total,
        });
    };
    report(observer, written);

    loop {
        match wait_for(&rx, observer)? {
            Msg::Chunk(bytes) => {
                if written + bytes.len() as u64 > size {
                    drop(out);
                    drop(rx);
                    let _ = fs::remove_file(&job.part);
                    return Err(integrity(
                        job,
                        format!("the server sent more than {size} bytes"),
                    ));
                }
                out.write_all(&bytes)?;
                hasher.update(&bytes);
                written += bytes.len() as u64;
                since_report += bytes.len() as u64;
                if since_report >= REPORT_EVERY {
                    since_report = 0;
                    report(observer, written);
                }
                if observer.is_cancelled() {
                    let _ = out.sync_all();
                    drop(out);
                    drop(rx);
                    return Err(Error::Cancelled);
                }
            }
            Msg::End => {
                if written < size {
                    let _ = out.sync_all();
                    return Err(download_error(
                        job,
                        None,
                        format!("connection closed after {written} of {size} bytes"),
                    ));
                }
                out.sync_all()?;
                // Windows will not rename a file that is still open.
                drop(out);
                return finish(job, hasher);
            }
            Msg::Fail(reason) => {
                let _ = out.sync_all();
                return Err(download_error(job, None, reason));
            }
            Msg::Head { .. } => {
                return Err(download_error(
                    job,
                    None,
                    "download thread sent a second status",
                ));
            }
        }
    }
}

/// Compare the hash of a complete `.part` and move it into place.
fn finish(job: &FileJob<'_>, hasher: Sha256) -> Result<()> {
    let got = hex(&hasher.finalize());
    if got != job.file.sha256 {
        return Err(integrity(
            job,
            format!("SHA-256 is {got}, expected {}", job.file.sha256),
        ));
    }
    fs::rename(&job.part, job.final_path)?;
    Ok(())
}

fn range_starts_at(content_range: Option<&str>, from: u64) -> bool {
    content_range.is_some_and(|v| v.trim().starts_with(&format!("bytes {from}-")))
}

// ---------------------------------------------------------------------
// The HTTP thread
// ---------------------------------------------------------------------

enum Msg {
    Head {
        status: u16,
        content_range: Option<String>,
    },
    Chunk(Vec<u8>),
    End,
    Fail(String),
}

/// Start the thread that makes the request, and return the channel its
/// answer arrives on.
///
/// The thread is detached on purpose. Joining it would make a cancel wait
/// for a stalled socket; instead the caller drops the receiver, the
/// thread's next `send` fails, and it exits on its own. It owns only
/// plain data.
fn spawn_worker(url: String, from: u64) -> io::Result<Receiver<Msg>> {
    let (tx, rx) = mpsc::sync_channel(4);
    std::thread::Builder::new()
        .name("model-download".into())
        .spawn(move || {
            if let Err(reason) = run_request(&url, from, &tx) {
                let _ = tx.send(Msg::Fail(reason));
            }
        })?;
    Ok(rx)
}

fn run_request(url: &str, from: u64, tx: &SyncSender<Msg>) -> std::result::Result<(), String> {
    // Built here, not by the caller: `reqwest::blocking::Client` panics
    // when created or dropped on a thread inside a tokio runtime.
    let mut builder = reqwest::blocking::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(STALL_TIMEOUT)
        .user_agent(concat!("edytlab/", env!("CARGO_PKG_VERSION")));
    if is_loopback(url) {
        // Never send a request for this machine through a proxy.
        builder = builder.no_proxy();
    }
    let client = builder.build().map_err(|e| describe(&e))?;

    let mut request = client
        .get(url)
        // A model file is already compressed, and a byte range of a
        // compressed body is not a range of the file.
        .header(reqwest::header::ACCEPT_ENCODING, "identity");
    if from > 0 {
        request = request.header(reqwest::header::RANGE, format!("bytes={from}-"));
    }
    let mut response = request.send().map_err(|e| describe(&e))?;

    let status = response.status().as_u16();
    let content_range = response
        .headers()
        .get(reqwest::header::CONTENT_RANGE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    if tx
        .send(Msg::Head {
            status,
            content_range,
        })
        .is_err()
    {
        return Ok(());
    }
    if status != 200 && status != 206 {
        // The caller reports it; the body (an error page) is not wanted.
        return Ok(());
    }

    // Streamed through `Read`, never `.bytes()`: see STALL_TIMEOUT.
    let mut buf = vec![0u8; CHUNK];
    loop {
        let n = response.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            let _ = tx.send(Msg::End);
            return Ok(());
        }
        if tx.send(Msg::Chunk(buf[..n].to_vec())).is_err() {
            return Ok(());
        }
    }
}

fn is_loopback(url: &str) -> bool {
    reqwest::Url::parse(url)
        .is_ok_and(|u| matches!(u.host_str(), Some("127.0.0.1" | "[::1]" | "localhost")))
}

/// A `reqwest` error with its causes: the top-level message alone is
/// usually just "error sending request".
fn describe(err: &reqwest::Error) -> String {
    use std::error::Error as _;
    let mut out = err.to_string();
    let mut source = err.source();
    while let Some(cause) = source {
        let text = cause.to_string();
        if !out.contains(&text) {
            out.push_str(": ");
            out.push_str(&text);
        }
        source = cause.source();
    }
    out
}

/// Wait for the next message from the HTTP thread, looking at
/// cancellation while it does.
fn wait_for(rx: &Receiver<Msg>, observer: &dyn FetchObserver) -> Result<Msg> {
    loop {
        match rx.recv_timeout(CANCEL_POLL) {
            Ok(msg) => return Ok(msg),
            Err(RecvTimeoutError::Timeout) => {
                if observer.is_cancelled() {
                    return Err(Error::Cancelled);
                }
            }
            Err(RecvTimeoutError::Disconnected) => {
                return Ok(Msg::Fail("the download thread ended unexpectedly".into()));
            }
        }
    }
}
