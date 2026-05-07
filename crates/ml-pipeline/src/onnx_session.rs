//! ONNX Runtime session registry.
//!
//! The registry hands out `Arc<ort::Session>` keyed by a caller-provided
//! `model_id`. Sessions are lazily built on first request via
//! [`ModelRegistry::load`] and cached so subsequent loads of the same
//! `model_id` clone the existing `Arc` rather than rebuilding the session.
//!
//! Execution-provider selection is delegated to [`ExecProvider`]:
//!
//! - [`ExecProvider::Cpu`]   — no EP appended, ORT defaults to CPU.
//! - [`ExecProvider::CoreML`] — only honored on macOS; on other targets
//!   we log a `tracing::warn!` and fall through to CPU rather than
//!   panicking. This keeps the dev sandbox (Linux) and Windows builds
//!   green when a caller blindly requests `default_for_platform()`.
//! - [`ExecProvider::Cuda`]  — only honored on non-mac targets; on mac
//!   we warn and fall back to CPU. Real CUDA usage requires the host
//!   to install ONNX Runtime built with CUDA support.
//!
//! `ort` is configured with `load-dynamic` (matching `ml-whisper`'s
//! Phase-1 setup), so session creation will fail with a helpful
//! `Error::Ort` if `ORT_DYLIB_PATH` is unset or the dylib is missing.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, RwLock};

use ort::session::{builder::SessionBuilder, Session};

use crate::{Error, Result};

/// ONNX Runtime execution provider preference.
///
/// `default_for_platform()` returns the canonical preference: CoreML on
/// macOS, CPU elsewhere. CUDA is opt-in (the user must select it
/// explicitly).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecProvider {
    /// Default ORT CPU execution provider. Always available.
    Cpu,
    /// Apple CoreML EP. Honored on macOS; falls back to CPU on other
    /// targets with a `tracing::warn!`.
    CoreML,
    /// NVIDIA CUDA EP. Honored on non-mac targets when the host has a
    /// CUDA-enabled ONNX Runtime; falls back to CPU on macOS with a
    /// `tracing::warn!`.
    Cuda,
}

impl ExecProvider {
    /// Canonical platform default: CoreML on macOS, CPU elsewhere.
    pub fn default_for_platform() -> Self {
        if cfg!(target_os = "macos") {
            ExecProvider::CoreML
        } else {
            ExecProvider::Cpu
        }
    }
}

/// Registry of lazily-loaded `ort::Session`s keyed by `model_id`.
///
/// Designed to live as a process-wide singleton: clone the `Arc<Session>`
/// returned by [`load`] into each tool invocation rather than rebuilding
/// the session per call.
///
/// [`load`]: ModelRegistry::load
#[derive(Debug, Default)]
pub struct ModelRegistry {
    inner: Arc<RwLock<HashMap<String, Arc<Session>>>>,
}

impl ModelRegistry {
    /// Construct an empty registry. No I/O happens here.
    pub fn new() -> Self {
        Self::default()
    }

    /// Return the cached session for `model_id`, or build a new one
    /// from `path` using execution provider `ep` and cache it.
    ///
    /// Loading the same `model_id` twice always returns the same
    /// `Arc<Session>` (acceptance criterion #1). Callers that want to
    /// force a reload should pick a new `model_id` (e.g. include the
    /// model file's content hash in the id).
    pub fn load(&self, model_id: &str, path: &Path, ep: ExecProvider) -> Result<Arc<Session>> {
        // Fast path: shared read lock.
        if let Some(existing) = self
            .inner
            .read()
            .map_err(|e| Error::Ort(format!("registry read lock poisoned: {e}")))?
            .get(model_id)
            .cloned()
        {
            return Ok(existing);
        }

        // Build outside the write lock so a slow `commit_from_file`
        // doesn't block other readers.
        let session = build_session(path, ep)?;
        let arc = Arc::new(session);

        let mut guard = self
            .inner
            .write()
            .map_err(|e| Error::Ort(format!("registry write lock poisoned: {e}")))?;
        // A racing call could have inserted in the meantime — preserve
        // the first winner so callers continue to share a single Arc.
        let final_arc = guard.entry(model_id.to_owned()).or_insert(arc).clone();
        Ok(final_arc)
    }

    /// Number of cached sessions (mostly for diagnostics / tests).
    pub fn len(&self) -> usize {
        self.inner.read().map(|g| g.len()).unwrap_or(0)
    }

    /// Whether the registry has no cached sessions.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Build an `ort::Session` from `path` with the requested EP, applying
/// the platform fallback rules described in the [`ExecProvider`] docs.
fn build_session(path: &Path, ep: ExecProvider) -> Result<Session> {
    if !path.exists() {
        return Err(Error::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("model file not found: {}", path.display()),
        )));
    }

    let builder = Session::builder()?;
    let mut builder = apply_execution_provider(builder, ep)?;
    let session = builder.commit_from_file(path)?;
    Ok(session)
}

/// Append the requested EP to `builder`, applying platform-specific
/// fallbacks. Returns the (possibly-unchanged) builder.
fn apply_execution_provider(builder: SessionBuilder, ep: ExecProvider) -> Result<SessionBuilder> {
    match ep {
        ExecProvider::Cpu => Ok(builder),
        ExecProvider::CoreML => {
            #[cfg(target_os = "macos")]
            {
                use ort::execution_providers::CoreMLExecutionProvider;
                // `with_execution_providers` returns an `ort::Error<R>`
                // (recoverable error carrying the builder back) rather
                // than a plain `ort::Error`, so we can't use `?` here —
                // map it through our `Error::Ort(String)` wrapper.
                builder
                    .with_execution_providers([CoreMLExecutionProvider::default().build()])
                    .map_err(|e| Error::Ort(e.to_string()))
            }
            #[cfg(not(target_os = "macos"))]
            {
                tracing::warn!(
                    "CoreML execution provider requested on non-macOS target; falling back to CPU"
                );
                Ok(builder)
            }
        }
        ExecProvider::Cuda => {
            #[cfg(not(target_os = "macos"))]
            {
                use ort::execution_providers::CUDAExecutionProvider;
                builder
                    .with_execution_providers([CUDAExecutionProvider::default().build()])
                    .map_err(|e| Error::Ort(e.to_string()))
            }
            #[cfg(target_os = "macos")]
            {
                tracing::warn!(
                    "CUDA execution provider requested on macOS; falling back to CPU (use CoreML instead)"
                );
                Ok(builder)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_provider_matches_platform_cfg() {
        let p = ExecProvider::default_for_platform();
        if cfg!(target_os = "macos") {
            assert_eq!(p, ExecProvider::CoreML);
        } else {
            assert_eq!(p, ExecProvider::Cpu);
        }
    }

    #[test]
    fn registry_starts_empty() {
        let r = ModelRegistry::new();
        assert!(r.is_empty());
        assert_eq!(r.len(), 0);
    }

    #[test]
    fn load_reports_missing_model_file() {
        let r = ModelRegistry::new();
        let err = r
            .load(
                "nope",
                Path::new("/definitely/does/not/exist.onnx"),
                ExecProvider::Cpu,
            )
            .unwrap_err();
        // We turn the missing file into an `Io(NotFound)` before ORT
        // gets a chance to look at it, so callers get a structured
        // error regardless of whether the runtime is installed.
        match err {
            Error::Io(e) => assert_eq!(e.kind(), std::io::ErrorKind::NotFound),
            other => panic!("expected Io(NotFound), got {other:?}"),
        }
    }
}
