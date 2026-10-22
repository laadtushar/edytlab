//! The models the app can download, and the user-started download of them
//! (#383).
//!
//! [`Catalog`] is `models.json`: for each model its Hugging Face
//! repository and commit and, for every file, the URL, byte size and
//! SHA-256. [`ModelDownloads`] is what the Settings row drives: it tells
//! whether a model is on disk, starts a download on a thread of its own,
//! cancels it, and removes the model's files. The transfer itself is
//! [`ModelStore::fetch`], so everything `download.rs` promises holds: a
//! file only ever has its final name once its size and hash match, a
//! stopped download is resumed with a `Range` request, and nothing is
//! fetched over plain http.
//!
//! ## Who may download
//!
//! Only the user. [`ModelDownloads::start_download`] is called from one
//! place, the `download_ml_model` command, which the Settings row's
//! Download button invokes; `tests/only_the_user_downloads.rs` fails if a
//! tool, the agent loop or the launch code can reach it. Tools never
//! download: a model is about 139 MB, a tool runs holding the session and
//! engine locks, and the request goes to a third party.
//!
//! ## Threads and locks
//!
//! The download runs on its own `std::thread`. It takes none of the
//! caller's locks; the only locks here are the artifact lock `fetch` takes
//! (a file under the models folder) and one small `Mutex` over the table
//! of states. That one is held for a field or two, never across the
//! transfer or a call to the listener; `remove` is the exception, and
//! holds it across its few `unlink` calls on purpose, so that a start
//! cannot slip in between its check and its deletion. The listener is
//! called on whichever thread changed the state, so it must not block (the
//! app's emits an event).
//!
//! ## What a status means
//!
//! - [`ModelState::Downloading`]: a transfer is running in this process.
//! - [`ModelState::Failed`]: the last transfer ended with an error, kept
//!   in memory until the next start or remove. The partial file is kept
//!   too, for a retry to resume.
//! - [`ModelState::Ready`]: every file is at its final name with the
//!   pinned size and SHA-256 (read and hashed again, so a file that was
//!   replaced since is not "ready").
//! - [`ModelState::NotDownloaded`]: none of the above. `bytes` says how
//!   much of the model is on disk, so a cancelled download shows as one a
//!   click will resume.
//!
//! Nothing here runs a model. A model on disk is the file transcription
//! will read once it exists (#384); it does not turn anything on.

use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::Deserialize;

use crate::download::{FetchObserver, FetchProgress, ModelArtifact, ModelFile, ModelStore};
use crate::{Error, Result};

/// `crates/ml-pipeline/models.json`, the one place the models are pinned.
const MANIFEST: &str = include_str!("../models.json");

/// The id of the speech model: what `ml_model_status` and the other model
/// commands are asked about, and the directory name under the models
/// folder.
pub const WHISPER_BASE_ID: &str = "whisper-base";

/// At most one progress report per this long, so a fast connection does
/// not turn every 256 KiB into an event for the window to draw.
const REPORT_EVERY: Duration = Duration::from_millis(100);

// ---------------------------------------------------------------------
// The catalog
// ---------------------------------------------------------------------

/// One model the app can download.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogModel {
    /// `whisper-base`: the id the commands take and the folder name.
    pub id: String,
    /// What the window calls it.
    pub title: String,
    /// The Hugging Face repository, `owner/name`.
    pub repo: String,
    /// The commit of that repository the files are fetched at: 40 hex
    /// digits, never a branch.
    pub revision: String,
    /// SPDX id of the model the export was made from.
    pub license: String,
    /// The files, with their URLs, sizes and hashes.
    pub artifact: ModelArtifact,
}

impl CatalogModel {
    /// Bytes of all the files together.
    pub fn size(&self) -> u64 {
        self.artifact.files.iter().map(|f| f.size).sum()
    }
}

/// The models the app knows how to download.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Catalog {
    models: Vec<CatalogModel>,
}

#[derive(Deserialize)]
struct RawCatalog {
    models: Vec<RawModel>,
}

#[derive(Deserialize)]
struct RawModel {
    id: String,
    title: String,
    repo: String,
    revision: String,
    license: String,
    files: Vec<RawFile>,
}

#[derive(Deserialize)]
struct RawFile {
    path: String,
    url: String,
    size: u64,
    sha256: String,
}

impl Catalog {
    /// The shipped `models.json`.
    pub fn built_in() -> &'static Catalog {
        static CATALOG: OnceLock<Catalog> = OnceLock::new();
        CATALOG.get_or_init(|| {
            Catalog::from_json(MANIFEST)
                .expect("models.json is a valid manifest (tests/model_manifest.rs checks it)")
        })
    }

    /// Parse and check a manifest. The same parser reads the shipped file
    /// and the ones the tests serve from a local server.
    ///
    /// Everything [`ModelStore`] checks before it touches a disk is
    /// checked here (ids and paths that cannot leave the store, https
    /// URLs, 64-digit hashes), and a revision must be a commit.
    pub fn from_json(text: &str) -> Result<Self> {
        let raw: RawCatalog = serde_json::from_str(text)
            .map_err(|e| Error::InvalidManifest(format!("not a model manifest: {e}")))?;
        let mut models: Vec<CatalogModel> = Vec::with_capacity(raw.models.len());
        for m in raw.models {
            let revision_ok = m.revision.len() == 40
                && m.revision
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
            if !revision_ok {
                return Err(Error::InvalidManifest(format!(
                    "{:?}: revision must be a commit (40 lowercase hex digits), got {:?}",
                    m.id, m.revision
                )));
            }
            let artifact = ModelArtifact {
                id: m.id.clone(),
                files: m
                    .files
                    .into_iter()
                    .map(|f| ModelFile {
                        path: f.path,
                        url: f.url,
                        sha256: f.sha256,
                        size: f.size,
                    })
                    .collect(),
            };
            // The store's own validation: id, paths, urls, digests, sizes.
            ModelStore::new(PathBuf::new()).artifact_dir(&artifact)?;
            if models.iter().any(|other| other.id == m.id) {
                return Err(Error::InvalidManifest(format!(
                    "{:?} is listed twice",
                    m.id
                )));
            }
            models.push(CatalogModel {
                id: m.id,
                title: m.title,
                repo: m.repo,
                revision: m.revision,
                license: m.license,
                artifact,
            });
        }
        Ok(Catalog { models })
    }

    pub fn models(&self) -> &[CatalogModel] {
        &self.models
    }

    pub fn get(&self, id: &str) -> Option<&CatalogModel> {
        self.models.iter().find(|m| m.id == id)
    }
}

/// The speech model's files, as pinned in `models.json`: what transcription
/// reads once it exists (#384). That tool will find the model with
/// [`cached`]`(WHISPER_BASE_ID)` and, when it is `None`, say the model is not
/// downloaded; it will never fetch it (`tests/only_the_user_downloads.rs`
/// keeps the downloader out of every crate but this one).
pub fn whisper_base() -> &'static ModelArtifact {
    &Catalog::built_in()
        .get(WHISPER_BASE_ID)
        .expect("models.json pins whisper-base (tests/model_manifest.rs checks it)")
        .artifact
}

/// Where the models folder is, registered once at launch by the app
/// ([`set_models_dir`]) so that code that only *reads* a model can find it
/// without being able to download one.
static MODELS_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Register the models folder (`<app local data>/models`), once. Returns
/// `false` if one was registered already, and keeps the first. Reads no
/// disk and makes no request.
pub fn set_models_dir(dir: PathBuf) -> bool {
    MODELS_DIR.set(dir).is_ok()
}

/// The directory of model `id` if every one of its files is on disk with
/// the pinned size and SHA-256, otherwise `None`: the read-only way for a
/// tool to find a model (#384). It never downloads anything and creates or
/// deletes nothing. It reads and hashes the files, a fraction of a second
/// for the 139 MB speech model, so do not call it per frame.
///
/// `None` also when no models folder was registered ([`set_models_dir`]),
/// when `id` is not in the catalog, and when the folder cannot be read.
pub fn cached(id: &str) -> Option<PathBuf> {
    cached_in(MODELS_DIR.get()?, Catalog::built_in(), id)
}

fn cached_in(dir: &std::path::Path, catalog: &Catalog, id: &str) -> Option<PathBuf> {
    let model = catalog.get(id)?;
    match ModelStore::new(dir).cached(&model.artifact) {
        Ok(found) => found,
        Err(e) => {
            tracing::warn!(model = %id, error = %e, "could not read the models folder");
            None
        }
    }
}

// ---------------------------------------------------------------------
// Status
// ---------------------------------------------------------------------

/// Where a model is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelState {
    /// Not (completely) on disk, and nothing is downloading it.
    NotDownloaded,
    /// A transfer is running. `received` counts bytes of all the model's
    /// files, climbing to `total`.
    Downloading { received: u64, total: u64 },
    /// Every file is on disk and verified.
    Ready,
    /// The last transfer ended in an error.
    Failed { reason: String },
}

/// A model's state, and how much of it is on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelStatus {
    pub model: String,
    pub state: ModelState,
    /// Bytes of this model on disk now: all of it when ready, the partial
    /// files' when a download was cancelled or failed, the bytes received
    /// while downloading.
    pub bytes: u64,
    /// Bytes of the whole model, as pinned.
    pub size: u64,
}

// ---------------------------------------------------------------------
// The manager
// ---------------------------------------------------------------------

type Listener = Box<dyn Fn(&ModelStatus) + Send + Sync>;

enum Phase {
    /// No transfer in this process: the disk says where the model is.
    Idle,
    Running {
        received: u64,
        total: u64,
    },
    Failed(String),
}

struct Slot {
    phase: Phase,
    /// Set by [`ModelDownloads::cancel`]; the running transfer polls it.
    cancel: Arc<AtomicBool>,
}

struct Inner {
    store: ModelStore,
    catalog: Catalog,
    listener: Listener,
    slots: Mutex<HashMap<String, Slot>>,
}

/// Starts, watches, cancels and removes the downloads of a [`Catalog`]'s
/// models, under one models folder. Cheap to clone; clones share state.
#[derive(Clone)]
pub struct ModelDownloads {
    inner: Arc<Inner>,
}

impl Inner {
    fn model(&self, id: &str) -> Result<&CatalogModel> {
        self.catalog
            .get(id)
            .ok_or_else(|| Error::UnknownModel(id.to_string()))
    }

    fn slots(&self) -> std::sync::MutexGuard<'_, HashMap<String, Slot>> {
        self.slots.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn notify(&self, status: &ModelStatus) {
        (self.listener)(status);
    }

    fn status_of(&self, model: &CatalogModel, state: ModelState, bytes: u64) -> ModelStatus {
        ModelStatus {
            model: model.id.clone(),
            state,
            bytes,
            size: model.size(),
        }
    }

    /// Bytes on disk, or 0 if the folder cannot be read (a status is
    /// still worth showing).
    fn bytes_on_disk(&self, model: &CatalogModel) -> u64 {
        self.store
            .on_disk(&model.artifact)
            .map(|d| d.bytes)
            .unwrap_or(0)
    }

    /// The status for what the disk says, with no transfer running.
    fn status_from_disk(&self, model: &CatalogModel) -> Result<ModelStatus> {
        let usage = self.store.on_disk(&model.artifact)?;
        // Hashing reads the files, so it is only done when all of them are
        // there at their pinned sizes.
        if usage.complete && self.store.cached(&model.artifact)?.is_some() {
            return Ok(self.status_of(model, ModelState::Ready, model.size()));
        }
        Ok(self.status_of(model, ModelState::NotDownloaded, usage.bytes))
    }

    fn status(&self, model: &CatalogModel) -> Result<ModelStatus> {
        enum Seen {
            Running(u64, u64),
            Failed(String),
            Idle,
        }
        let seen = match self.slots().get(&model.id).map(|s| &s.phase) {
            Some(Phase::Running { received, total }) => Seen::Running(*received, *total),
            Some(Phase::Failed(reason)) => Seen::Failed(reason.clone()),
            Some(Phase::Idle) | None => Seen::Idle,
        };
        match seen {
            Seen::Running(received, total) => {
                Ok(self.status_of(model, ModelState::Downloading { received, total }, received))
            }
            Seen::Failed(reason) => {
                let bytes = self.bytes_on_disk(model);
                Ok(self.status_of(model, ModelState::Failed { reason }, bytes))
            }
            Seen::Idle => self.status_from_disk(model),
        }
    }

    /// The worker's last act: record how the transfer ended, then tell the
    /// listener.
    fn finish(&self, model: &CatalogModel, result: std::thread::Result<Result<PathBuf>>) {
        let (phase, status) = match result {
            Ok(Ok(_)) => (
                Phase::Idle,
                self.status_of(model, ModelState::Ready, model.size()),
            ),
            Ok(Err(Error::Cancelled)) => (
                Phase::Idle,
                self.status_of(model, ModelState::NotDownloaded, self.bytes_on_disk(model)),
            ),
            Ok(Err(e)) => {
                let reason = e.to_string();
                tracing::warn!(model = %model.id, %reason, "model download failed");
                (
                    Phase::Failed(reason.clone()),
                    self.status_of(
                        model,
                        ModelState::Failed { reason },
                        self.bytes_on_disk(model),
                    ),
                )
            }
            // Reached only in a debug or test build: a release build aborts
            // on a panic (see the `catch_unwind` in `start_download`).
            Err(_) => {
                let reason = "the download stopped unexpectedly".to_string();
                tracing::error!(model = %model.id, "the model download thread panicked");
                (
                    Phase::Failed(reason.clone()),
                    self.status_of(
                        model,
                        ModelState::Failed { reason },
                        self.bytes_on_disk(model),
                    ),
                )
            }
        };
        if let Some(slot) = self.slots().get_mut(&model.id) {
            slot.phase = phase;
        }
        self.notify(&status);
    }
}

/// What [`ModelStore::fetch`] reports to: it writes the state table and
/// tells the listener, and stops when the user cancelled.
struct Reporter {
    inner: Arc<Inner>,
    model: CatalogModel,
    cancel: Arc<AtomicBool>,
    last_report: Option<Instant>,
}

impl FetchObserver for Reporter {
    fn on_progress(&mut self, p: &FetchProgress<'_>) {
        let (received, total) = (p.bytes_done, p.bytes_total);
        {
            let mut slots = self.inner.slots();
            if let Some(slot) = slots.get_mut(&self.model.id) {
                if matches!(slot.phase, Phase::Running { .. }) {
                    slot.phase = Phase::Running { received, total };
                }
            }
        }
        let due = self
            .last_report
            .is_none_or(|at| at.elapsed() >= REPORT_EVERY)
            || received >= total;
        if due {
            self.last_report = Some(Instant::now());
            let status = self.inner.status_of(
                &self.model,
                ModelState::Downloading { received, total },
                received,
            );
            self.inner.notify(&status);
        }
    }

    fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }
}

impl ModelDownloads {
    /// Downloads of `catalog`'s models into `<root>/<model id>/`.
    ///
    /// `on_change` hears every change of state: the start, progress (at
    /// most every 100 ms), and each way a transfer ends. It runs on the
    /// thread that made the change and must not block.
    pub fn new(
        root: impl Into<PathBuf>,
        catalog: Catalog,
        on_change: impl Fn(&ModelStatus) + Send + Sync + 'static,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                store: ModelStore::new(root),
                catalog,
                listener: Box::new(on_change),
                slots: Mutex::new(HashMap::new()),
            }),
        }
    }

    /// The catalog this manager serves.
    pub fn catalog(&self) -> &Catalog {
        &self.inner.catalog
    }

    /// The directory of `id` if all its files are on disk and verified, as
    /// the free function [`cached`] answers for the registered folder. Reads
    /// and hashes the files; no request, nothing created.
    pub fn cached(&self, id: &str) -> Result<Option<PathBuf>> {
        let model = self.inner.model(id)?;
        self.inner.store.cached(&model.artifact)
    }

    /// Where `id` is. Reads the disk, and hashes the files when they are
    /// all there, so a caller on an async runtime should run it on the
    /// blocking pool. Makes no request and creates nothing.
    pub fn status(&self, id: &str) -> Result<ModelStatus> {
        let model = self.inner.model(id)?;
        self.inner.status(model)
    }

    /// Begin downloading `id` on a thread of its own and return at once
    /// with the state it started in. A download already running is left
    /// alone (the answer is its state). A partial file from an earlier
    /// attempt is resumed; a complete model is verified, not fetched.
    ///
    /// **Called from one place, the `download_ml_model` command**, which
    /// the user's click reaches; see the module doc.
    pub fn start_download(&self, id: &str) -> Result<ModelStatus> {
        let model = self.inner.model(id)?.clone();
        let total = model.size();
        let cancel = {
            let mut slots = self.inner.slots();
            let slot = slots.entry(model.id.clone()).or_insert_with(|| Slot {
                phase: Phase::Idle,
                cancel: Arc::new(AtomicBool::new(false)),
            });
            if let Phase::Running { received, total } = slot.phase {
                let status = self.inner.status_of(
                    &model,
                    ModelState::Downloading { received, total },
                    received,
                );
                return Ok(status);
            }
            slot.cancel = Arc::new(AtomicBool::new(false));
            slot.phase = Phase::Running { received: 0, total };
            Arc::clone(&slot.cancel)
        };

        let started =
            self.inner
                .status_of(&model, ModelState::Downloading { received: 0, total }, 0);
        self.inner.notify(&started);

        let inner = Arc::clone(&self.inner);
        let worker_model = model.clone();
        let spawned = std::thread::Builder::new()
            .name("model-download-worker".into())
            .spawn(move || {
                let mut reporter = Reporter {
                    inner: Arc::clone(&inner),
                    model: worker_model.clone(),
                    cancel,
                    last_report: None,
                };
                // A panic in the transfer is caught and shown as a failure in
                // a debug or test build. The release profile sets
                // `panic = "abort"` (root `Cargo.toml`), so in a shipped
                // build a panic ends the app instead and this catches
                // nothing: nothing may rely on it.
                let result = catch_unwind(AssertUnwindSafe(|| {
                    inner.store.fetch(&worker_model.artifact, &mut reporter)
                }));
                inner.finish(&worker_model, result);
            });
        if let Err(e) = spawned {
            let reason = format!("could not start the download thread: {e}");
            let status = self.inner.status_of(
                &model,
                ModelState::Failed {
                    reason: reason.clone(),
                },
                self.inner.bytes_on_disk(&model),
            );
            if let Some(slot) = self.inner.slots().get_mut(&model.id) {
                slot.phase = Phase::Failed(reason);
            }
            self.inner.notify(&status);
            return Ok(status);
        }
        Ok(started)
    }

    /// Ask a running download to stop. It stops at its next chunk and keeps
    /// its partial file; the listener is told when it has. Does nothing when
    /// nothing is running. Answers with the state at the time of the call.
    pub fn cancel(&self, id: &str) -> Result<ModelStatus> {
        let model = self.inner.model(id)?;
        if let Some(slot) = self.inner.slots().get(&model.id) {
            if matches!(slot.phase, Phase::Running { .. }) {
                slot.cancel.store(true, Ordering::SeqCst);
            }
        }
        self.inner.status(model)
    }

    /// Delete `id`'s files from the models folder: the files the manifest
    /// lists, their partial files, and the folders that leave empty.
    /// Anything else in the folder stays, and so does every other model.
    ///
    /// Refused ([`Error::Busy`]) while a download of it runs here or in
    /// another process; cancel it first. Clears a recorded failure.
    pub fn remove(&self, id: &str) -> Result<ModelStatus> {
        let model = self.inner.model(id)?;
        {
            // Held across the deletion, so a start cannot begin between the
            // check and the unlink. The deletion is a few `unlink` calls.
            let mut slots = self.inner.slots();
            if let Some(slot) = slots.get(&model.id) {
                if matches!(slot.phase, Phase::Running { .. }) {
                    return Err(Error::Busy {
                        artifact: model.id.clone(),
                    });
                }
            }
            self.inner.store.remove(&model.artifact)?;
            if let Some(slot) = slots.get_mut(&model.id) {
                slot.phase = Phase::Idle;
            }
        }
        let status = self.inner.status_of(model, ModelState::NotDownloaded, 0);
        self.inner.notify(&status);
        Ok(status)
    }
}

impl std::fmt::Debug for ModelDownloads {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ModelDownloads")
            .field("root", &self.inner.store.root())
            .field("models", &self.inner.catalog.models().len())
            .finish()
    }
}
