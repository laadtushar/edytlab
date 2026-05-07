//! Content-hashed inference cache.
//!
//! On-disk store rooted at `<project>/.audiograph/inference-cache/`. Keys
//! are blake3 hashes of arbitrary input bytes; callers fold the model
//! file's hash into the key so that swapping the model invalidates
//! prior entries (see [`ContentHash::from_bytes`] +
//! [`ContentHash::combine`]).
//!
//! Values are JSON-serialized and atomically written via
//! `tempfile::NamedTempFile::persist` so a crashed/killed process cannot
//! leave half-written cache files behind.
//!
//! The cache is intentionally agnostic about value shape — every tool
//! defines its own `T: Serialize + DeserializeOwned`. Stems-on-disk
//! tools store paths; analysis tools store small structs of numbers.

use std::fmt;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::{Error, Result};

/// 32-byte blake3 digest used as a content-addressed key.
///
/// The inner array is private so the hex encoding stays this type's
/// responsibility — callers always go through [`ContentHash::from_bytes`]
/// (or [`ContentHash::combine`]) to construct one and through `Display`
/// to render one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ContentHash([u8; 32]);

impl ContentHash {
    /// Compute the blake3 hash of `bytes`.
    pub fn from_bytes(bytes: &[u8]) -> Self {
        Self(*blake3::hash(bytes).as_bytes())
    }

    /// Combine multiple component hashes into a single key. Useful for
    /// folding the model-file hash into the input-bytes hash so the
    /// cache invalidates when either changes.
    ///
    /// Order matters: `combine(&[a, b]) != combine(&[b, a])` in general.
    pub fn combine(parts: &[ContentHash]) -> Self {
        let mut hasher = blake3::Hasher::new();
        for p in parts {
            hasher.update(&p.0);
        }
        Self(*hasher.finalize().as_bytes())
    }

    /// Borrow the raw 32-byte digest.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Render as lowercase hex (64 chars). Used for the on-disk filename.
    pub fn to_hex(&self) -> String {
        let mut s = String::with_capacity(64);
        for b in &self.0 {
            // Two-char lowercase hex per byte; using fmt::Write rather
            // than format! avoids the intermediate String allocation
            // that `format!("{b:02x}")` would do per iteration.
            use std::fmt::Write as _;
            let _ = write!(&mut s, "{b:02x}");
        }
        s
    }
}

impl fmt::Display for ContentHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

/// Content-addressed inference cache rooted under
/// `<project>/.audiograph/inference-cache/`.
///
/// Cheap to clone (just a `PathBuf`); a single instance is fine to share
/// across tool invocations within a process.
#[derive(Debug, Clone)]
pub struct InferenceCache {
    base_dir: PathBuf,
}

impl InferenceCache {
    /// Open (creating if missing) the cache directory under the project
    /// directory. The standard layout is `<project>/.audiograph/inference-cache/`.
    pub fn open(project_dir: &Path) -> Result<Self> {
        let base_dir = project_dir.join(".audiograph").join("inference-cache");
        fs::create_dir_all(&base_dir)?;
        Ok(Self { base_dir })
    }

    /// Root directory of the cache (mostly for tests / diagnostics).
    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }

    /// Resolve the on-disk path for `key` (does not check existence).
    pub fn path_for(&self, key: ContentHash) -> PathBuf {
        self.base_dir.join(format!("{}.bin", key.to_hex()))
    }

    /// Look up `key`; if missing, run `compute`, persist the result, and
    /// return it. The persist step is atomic (tempfile + rename) so
    /// concurrent or crashed writers can't leave a half-serialized
    /// entry.
    ///
    /// Returns whatever `compute` returns; serialization errors are
    /// surfaced as [`Error::Json`], cache I/O failures as [`Error::Cache`].
    pub fn get_or_compute<T, F>(&self, key: ContentHash, compute: F) -> Result<T>
    where
        T: Serialize + DeserializeOwned,
        F: FnOnce() -> Result<T>,
    {
        let path = self.path_for(key);
        if path.exists() {
            // Best-effort read. If the file is corrupt (truncated,
            // wrong schema after a refactor, etc.) we fall through to
            // recomputing rather than poisoning the call site — a
            // corrupt cache entry should never block the tool.
            match fs::read(&path) {
                Ok(bytes) => match serde_json::from_slice::<T>(&bytes) {
                    Ok(v) => return Ok(v),
                    Err(_) => {
                        tracing::warn!(
                            cache_path = %path.display(),
                            "inference-cache entry failed to deserialize; recomputing"
                        );
                    }
                },
                Err(e) => {
                    tracing::warn!(
                        cache_path = %path.display(),
                        error = %e,
                        "inference-cache read failed; recomputing"
                    );
                }
            }
        }

        let value = compute()?;
        let bytes = serde_json::to_vec(&value)?;
        // Caching is an optimisation, not a correctness gate. If we
        // computed the value successfully, the caller should get it
        // back even if the disk write fails (full disk, perms,
        // read-only mount). We log the failure at warn level so it
        // surfaces in operator dashboards but don't propagate it.
        let persist_result = (|| -> Result<()> {
            // Atomic write: tempfile in the same dir + rename.
            let mut tmp = tempfile::NamedTempFile::new_in(&self.base_dir)
                .map_err(|e| Error::Cache(format!("tempfile create failed: {e}")))?;
            tmp.write_all(&bytes)
                .map_err(|e| Error::Cache(format!("tempfile write failed: {e}")))?;
            tmp.as_file()
                .sync_all()
                .map_err(|e| Error::Cache(format!("tempfile fsync failed: {e}")))?;
            tmp.persist(&path)
                .map_err(|e| Error::Cache(format!("tempfile persist failed: {e}")))?;
            Ok(())
        })();
        if let Err(e) = persist_result {
            tracing::warn!(
                cache_path = %path.display(),
                error = %e,
                "inference-cache persist failed; returning computed value anyway"
            );
        }
        Ok(value)
    }

    /// Delete the cache entry for `key` (no-op if missing).
    pub fn invalidate(&self, key: ContentHash) -> Result<()> {
        let path = self.path_for(key);
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(Error::Io(e)),
        }
    }
}
