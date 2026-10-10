//! The ONNX Runtime library gate.
//!
//! `ort` is built with `load-dynamic`: the ONNX Runtime library is
//! `dlopen`ed the first time any `ort` API is used. **`ort` cannot report
//! a library that fails to load.** In 2.0.0-rc.12 the failure is turned
//! into an `ort::Error`, building that error calls `ort::api()`, and
//! `ort::api()` re-enters the library initialisation that is still
//! running on the same thread. The thread blocks forever. (The code reads
//! as a panic, `.expect("Failed to load ONNX Runtime dylib")`, but that is
//! never reached.) The same happens through `ort::init_from`, which looks
//! as if it returns the error.
//!
//! Before this module, setting `WHISPER_MODEL_PATH` or
//! `DEMUCS_*_MODEL_PATH` to any existing file made `Session::builder()`
//! hang inside `ToolDispatcher::invoke`. The agent loop calls that holding
//! the dispatcher, store and engine mutexes, so the whole agent froze.
//!
//! So **every code path that builds an `ort` session calls [`ensure`]
//! first**, and `ensure` never hands `ort` a library that could fail:
//!
//! 1. It finds the library itself (see below).
//! 2. It opens it with `libloading` and makes the checks `ort` makes: the
//!    `OrtGetApiBase` entry point exists, the version is at least
//!    1.[`ort::MINOR_VERSION`], and the C API of that version is there.
//!    Any failure is an [`Error::RuntimeLoad`], and `ort` was never
//!    touched, so a later call can try again once the file is fixed.
//! 3. Only then does it call `ort::init_from` on that path. `ort` finds
//!    the library already open and succeeds, and from then on it never
//!    goes looking for the library itself.
//!
//! ## Where it looks
//!
//! In order, the first file that exists wins:
//!
//! 1. `ORT_DYLIB_PATH`, if set and non-empty. An absolute path is used
//!    as is; a relative one is tried against the executable's directory
//!    (which is what `ort` does), then the working directory.
//! 2. [`library_file_name`] next to the executable.
//!
//! There is deliberately **no bare-name or system-wide search**. Asking
//! the loader for plain `onnxruntime.dll` can pick up an older copy from
//! `System32`, and any system library may be the wrong version for the
//! `ort` we are built against. A library is loaded by absolute path, or
//! not at all.
//!
//! A path that exists but fails the checks ends the search with
//! [`Error::RuntimeLoad`]. Quietly falling through to the next candidate
//! would run something other than what the user pointed at.

use std::ffi::{CStr, OsStr};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use crate::{Error, Result};

/// The library `ensure` found and loaded, once it has.
static LOADED: OnceLock<PathBuf> = OnceLock::new();

/// Our own handle on the loaded library, kept for the life of the process
/// so the library is never unloaded under `ort`.
static PINNED: OnceLock<libloading::Library> = OnceLock::new();

/// Serialises the load, so two threads racing on a cold start do not
/// both open the library and call `init_from`.
static LOAD_LOCK: Mutex<()> = Mutex::new(());

/// The C signature of `OrtGetApiBase`, as `ort-sys` declares it.
type GetApiBase = unsafe extern "system" fn() -> *const ort::sys::OrtApiBase;

/// File name of the ONNX Runtime library on this platform.
pub fn library_file_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "onnxruntime.dll"
    } else if cfg!(target_os = "macos") {
        "libonnxruntime.dylib"
    } else {
        "libonnxruntime.so"
    }
}

/// Every place [`ensure`] will look, in order. See the module doc.
pub fn candidate_paths() -> Vec<PathBuf> {
    let exe = std::env::current_exe().ok();
    let cwd = std::env::current_dir().ok();
    candidates_from(
        std::env::var_os("ORT_DYLIB_PATH").as_deref(),
        exe.as_deref().and_then(Path::parent),
        cwd.as_deref(),
    )
}

/// [`candidate_paths`] with its three inputs passed in, so the order can
/// be tested without touching the process environment.
fn candidates_from(
    env_path: Option<&OsStr>,
    exe_dir: Option<&Path>,
    cwd: Option<&Path>,
) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    let mut push = |p: PathBuf| {
        if !out.contains(&p) {
            out.push(p);
        }
    };

    if let Some(raw) = env_path.filter(|v| !v.is_empty()) {
        let raw = Path::new(raw);
        if raw.is_absolute() {
            push(raw.to_path_buf());
        } else {
            if let Some(dir) = exe_dir {
                push(dir.join(raw));
            }
            if let Some(dir) = cwd {
                push(dir.join(raw));
            }
        }
    }
    if let Some(dir) = exe_dir {
        push(dir.join(library_file_name()));
    }
    out
}

/// Make sure an ONNX Runtime library is loaded, loading it if needed.
///
/// Returns the path it loaded. Cheap after the first success: that is a
/// lock-free read. After a failure nothing is remembered, so a later
/// call retries — the user may have put the library in place since.
pub fn ensure() -> Result<PathBuf> {
    ensure_with(&candidate_paths())
}

/// [`ensure`] over an explicit candidate list: the first one that is a
/// file is checked and loaded, or [`Error::MissingRuntime`] if none is.
///
/// Once any library has loaded, this returns that path whatever
/// `candidates` says: `ort` holds one library per process.
pub fn ensure_with(candidates: &[PathBuf]) -> Result<PathBuf> {
    if let Some(loaded) = LOADED.get() {
        return Ok(loaded.clone());
    }
    let _guard = LOAD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(loaded) = LOADED.get() {
        return Ok(loaded.clone());
    }

    let Some(index) = candidates.iter().position(|p| p.is_file()) else {
        return Err(Error::MissingRuntime {
            searched: candidates.to_vec(),
        });
    };
    let path = &candidates[index];
    for skipped in &candidates[..index] {
        tracing::warn!(
            skipped = %skipped.display(),
            using = %path.display(),
            "no ONNX Runtime library at an earlier location; using a later one"
        );
    }

    // Only this one is tried: it is the file the user's configuration
    // (or the install layout) points at, and a failure is theirs to hear.
    let library = check_library(path).map_err(|reason| Error::RuntimeLoad {
        path: path.clone(),
        reason,
    })?;

    // It passed every check `ort` makes, so `ort` loads it too: it opens
    // the same file and the loader hands back the library already open.
    // (A file swapped between the two opens is the one way left for `ort`
    // to fail here, and then it blocks rather than erroring; see the
    // module doc. There is no way to rule that out from outside `ort`.)
    let builder = ort::init_from(path).map_err(|e| Error::RuntimeLoad {
        path: path.clone(),
        reason: e.to_string(),
    })?;
    // `commit` returns false when an environment is already configured.
    // That only means our name and telemetry choice are not applied; the
    // library is loaded either way, so it is not an error.
    let _ = builder
        .with_name("edytlab")
        // `ort` turns telemetry on by default. Audio, and what is done to
        // it, stays on the machine.
        .with_telemetry(false)
        .commit();

    let _ = PINNED.set(library);
    let _ = LOADED.set(path.clone());
    Ok(path.clone())
}

/// Open `path` and make the checks `ort` makes when it loads a library,
/// without going through `ort` (which cannot report a failure; see the
/// module doc). Returns the open library, or why it is unusable.
///
/// On failure the library is closed again, so the file can be replaced
/// (on Windows an open DLL cannot be) and checked again.
fn check_library(path: &Path) -> std::result::Result<libloading::Library, String> {
    // SAFETY: opening a library runs its initialisers. This is the file
    // the user or the install layout names as ONNX Runtime, and loading
    // it is exactly what `ort` would do next.
    let library = unsafe { libloading::Library::new(path) }.map_err(|e| e.to_string())?;

    let version = {
        // SAFETY: `GetApiBase` is the signature `ort-sys` declares for
        // this symbol. The `Symbol` borrows `library`, so it cannot
        // outlive it.
        let get_base = unsafe { library.get::<GetApiBase>(b"OrtGetApiBase") }.map_err(|_| {
            "it has no OrtGetApiBase entry point, so it is not an ONNX Runtime library".to_string()
        })?;
        // SAFETY: `OrtGetApiBase` takes no arguments and returns a pointer
        // to a static table, or null.
        let base = unsafe { get_base() };
        if base.is_null() {
            return Err("its OrtGetApiBase returned null".into());
        }
        // SAFETY: `base` is non-null and points at the library's static
        // `OrtApiBase`, whose `GetVersionString` returns a static,
        // NUL-terminated string.
        let version = unsafe { CStr::from_ptr(((*base).GetVersionString)()) }
            .to_string_lossy()
            .into_owned();

        // The same rule `ort` applies: the minor version must be at least
        // the API version it was built for.
        let minor = version
            .split('.')
            .nth(1)
            .and_then(|m| m.parse::<u32>().ok())
            .unwrap_or(0);
        if minor < ort::MINOR_VERSION {
            return Err(format!(
                "it is ONNX Runtime {version}; this build needs 1.{}.0 or newer",
                ort::MINOR_VERSION
            ));
        }
        // SAFETY: as above; `GetApi` returns null for an API version the
        // library does not provide.
        let api = unsafe { ((*base).GetApi)(ort::MINOR_VERSION) };
        if api.is_null() {
            return Err(format!(
                "ONNX Runtime {version} does not provide C API version {}",
                ort::MINOR_VERSION
            ));
        }
        version
    };
    tracing::info!(path = %path.display(), %version, "ONNX Runtime library checked");
    Ok(library)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::mpsc;
    use std::time::Duration;

    // None of these loads a real ONNX Runtime: CI has none, and a load
    // that succeeded would be process-wide state for every other test.

    /// Run `ensure_with` on another thread and fail, rather than hang, if
    /// it never returns. A library that fails to load inside `ort` blocks
    /// the thread forever (see the module doc), so without this a
    /// regression would stall the test run instead of failing it.
    ///
    /// On a timeout it aborts rather than panicking: the stuck thread
    /// holds a lock that `ort`'s exit handler waits on, so the process
    /// could not exit normally afterwards. The message goes straight to
    /// stderr because the harness captures `eprintln!`.
    fn ensure_or_fail(candidates: Vec<PathBuf>) -> Result<PathBuf> {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(ensure_with(&candidates));
        });
        match rx.recv_timeout(Duration::from_secs(30)) {
            Ok(result) => result,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let _ = writeln!(
                    std::io::stderr(),
                    "ensure_with did not return within 30 s. A library that fails to load must \
                     be rejected before `ort` sees it, because `ort` deadlocks on a failed load."
                );
                std::process::abort();
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => panic!("ensure_with panicked"),
        }
    }

    #[test]
    fn library_file_name_matches_platform() {
        let name = library_file_name();
        if cfg!(target_os = "windows") {
            assert_eq!(name, "onnxruntime.dll");
        } else if cfg!(target_os = "macos") {
            assert_eq!(name, "libonnxruntime.dylib");
        } else {
            assert_eq!(name, "libonnxruntime.so");
        }
    }

    #[test]
    fn candidates_prefer_the_environment_then_the_executable_directory() {
        // Built from the temp dir so they are absolute on every OS.
        let base = std::env::temp_dir();
        let exe_dir = base.join("app");
        let cwd = base.join("work");
        let abs = base.join("elsewhere").join("ort.lib");

        let got = candidates_from(Some(abs.as_os_str()), Some(&exe_dir), Some(&cwd));
        assert_eq!(got, vec![abs, exe_dir.join(library_file_name())]);

        // A relative ORT_DYLIB_PATH is tried against the executable's
        // directory first (as `ort` does), then the working directory.
        let got = candidates_from(Some(OsStr::new("lib/ort.so")), Some(&exe_dir), Some(&cwd));
        assert_eq!(
            got,
            vec![
                exe_dir.join("lib/ort.so"),
                cwd.join("lib/ort.so"),
                exe_dir.join(library_file_name()),
            ]
        );

        // Unset and empty mean the same thing: only the executable's
        // directory. There is no bare-name fallback to the system.
        for env in [None, Some(OsStr::new(""))] {
            let got = candidates_from(env, Some(&exe_dir), Some(&cwd));
            assert_eq!(got, vec![exe_dir.join(library_file_name())]);
        }
        assert!(candidates_from(None, None, None).is_empty());
    }

    #[test]
    fn missing_library_reports_every_path_searched() {
        let dir = tempfile::tempdir().expect("tempdir");
        let first = dir.path().join("nope").join(library_file_name());
        let second = dir.path().join(library_file_name());

        let err = ensure_or_fail(vec![first.clone(), second.clone()]).expect_err("no library");
        let Error::MissingRuntime { searched } = &err else {
            panic!("expected MissingRuntime, got {err:?}");
        };
        assert_eq!(searched, &vec![first.clone(), second.clone()]);

        let msg = err.to_string();
        assert!(msg.contains(&first.display().to_string()), "{msg}");
        assert!(msg.contains(&second.display().to_string()), "{msg}");
        assert!(msg.contains("ORT_DYLIB_PATH"), "{msg}");
        assert!(msg.contains("#383"), "{msg}");

        let empty = ensure_or_fail(Vec::new()).expect_err("nothing to look in");
        assert!(matches!(empty, Error::MissingRuntime { .. }));
        assert!(empty.to_string().contains("nowhere"), "{empty}");
    }

    /// A file that is there but is not a library at all. Handed to `ort`,
    /// this blocked the thread forever.
    #[test]
    fn a_file_that_is_not_onnxruntime_is_a_load_error_not_a_hang() {
        let dir = tempfile::tempdir().expect("tempdir");
        let fake = dir.path().join(library_file_name());
        std::fs::write(&fake, b"this is not a shared library").expect("write");

        let err = ensure_or_fail(vec![fake.clone()]).expect_err("must not load");
        match &err {
            Error::RuntimeLoad { path, reason } => {
                assert_eq!(path, &fake);
                assert!(!reason.is_empty());
            }
            other => panic!("expected RuntimeLoad, got {other:?}"),
        }
        assert!(err.to_string().contains(&fake.display().to_string()));

        // `ort` was never given the file, so a second attempt is a real
        // second attempt, not a stale answer or a hang.
        assert!(matches!(
            ensure_or_fail(vec![fake]),
            Err(Error::RuntimeLoad { .. })
        ));
    }

    #[test]
    fn an_explicit_path_that_fails_is_not_skipped_for_the_next_one() {
        let dir = tempfile::tempdir().expect("tempdir");
        let broken = dir.path().join("broken").join(library_file_name());
        std::fs::create_dir_all(broken.parent().unwrap()).unwrap();
        std::fs::write(&broken, b"not a library").unwrap();
        // A second file after it. It is also junk, so if the search did
        // fall through, the error would name this path instead.
        let later = dir.path().join(library_file_name());
        std::fs::write(&later, b"also not a library").unwrap();

        match ensure_or_fail(vec![broken.clone(), later]) {
            Err(Error::RuntimeLoad { path, .. }) => assert_eq!(path, broken),
            other => panic!("expected RuntimeLoad for the first file, got {other:?}"),
        }
    }

    /// A real shared library that loads fine but is not ONNX Runtime: the
    /// check `ort` would make next (`OrtGetApiBase`) is made here instead.
    /// Only where a system library sits at a path known on CI.
    #[cfg(any(all(target_os = "linux", target_arch = "x86_64"), windows))]
    #[test]
    fn a_real_library_that_is_not_onnxruntime_is_a_load_error() {
        let other = if cfg!(windows) {
            PathBuf::from(r"C:\Windows\System32\kernel32.dll")
        } else {
            PathBuf::from("/lib/x86_64-linux-gnu/libm.so.6")
        };
        assert!(
            other.is_file(),
            "{} is missing on this machine",
            other.display()
        );

        match ensure_or_fail(vec![other.clone()]) {
            Err(Error::RuntimeLoad { path, reason }) => {
                assert_eq!(path, other);
                assert!(reason.contains("OrtGetApiBase"), "{reason}");
            }
            got => panic!("expected RuntimeLoad, got {got:?}"),
        }
    }
}
