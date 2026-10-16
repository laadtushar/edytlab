//! The ONNX Runtime library gate.
//!
//! `ort` is built with `load-dynamic`: the ONNX Runtime library is
//! `dlopen`ed the first time any `ort` API is used. Left to itself, `ort`
//! (2.0.0-rc.13) does that badly in three ways:
//!
//! - **A failed implicit load panics.** `Session::builder`, `ort::api()`
//!   and the rest load the library on first use, and a failure there is
//!   `.expect("Failed to load ONNX Runtime dylib")` inside a one-time
//!   initialiser, not a `Result`. The release profile is `panic = "abort"`
//!   and a tool runs while the agent loop holds the dispatcher, store and
//!   engine mutexes, so a missing library would end the app in the middle
//!   of a turn.
//! - **It searches.** With no `ORT_DYLIB_PATH` it hands the loader the
//!   bare name (`libonnxruntime.so`, `libonnxruntime.dylib`,
//!   `onnxruntime.dll`), and a relative `ORT_DYLIB_PATH` that names no file
//!   next to the executable is handed over as written. Either way the
//!   working directory or the system loader's own search order decides
//!   which file loads (see "Where it looks" for why that is not wanted).
//! - **Some bad libraries panic even through `ort::init_from`.** It
//!   returns a `LoadDynamicError` for a file that cannot be opened, has no
//!   `OrtGetApiBase`, or is older than 1.[`ort::MINOR_VERSION`]. But an
//!   `OrtGetApiBase` that returns null is an `assert!`, and a library that
//!   has no C API of that version panics on the first use after it.
//!
//! `ort::init_from` was worse in 2.0.0-rc.12: it turned a load failure
//! into an `ort::Error`, building that error called `ort::api()`, and
//! `ort::api()` re-entered the library initialisation still running on the
//! same thread, which then blocked forever. That was a hang, not a panic,
//! through `init_from` and the implicit load alike. rc.13 fixed it
//! ("Don't deadlock when load-dynamic fails",
//! <https://github.com/pykeio/ort/releases/tag/v2.0.0-rc.13>), and this
//! module is still needed for the three points above.
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
//!    as is; a relative one is resolved against the executable's
//!    directory and nowhere else.
//! 2. [`library_file_name`] in the **bundle directory**, if the app
//!    registered one with [`set_bundle_dir`]: where the installer puts the
//!    library. [`bundle_dir_for`] says where that is for each platform
//!    (macOS: `Contents/Frameworks`, found from the executable at
//!    `Contents/MacOS`; Linux: Tauri's resource directory, which is
//!    `/usr/lib/edytlab` for a deb and `<mount>/usr/lib/edytlab` for an
//!    AppImage; Windows: none, because the DLL sits next to the exe).
//! 3. [`library_file_name`] next to the executable.
//!
//! The **working directory is never searched**: it is wherever the app
//! happened to be started from, and often somewhere other users or other
//! programs can write, so a library found there could be planted
//! (CWE-427).
//!
//! There is deliberately **no bare-name or system-wide search** either.
//! Asking the loader for plain `onnxruntime.dll` can pick up an older copy
//! from `System32`, and any system library may be the wrong version for
//! the `ort` we are built against. A library is loaded by absolute path,
//! or not at all: [`ensure_with`] refuses a candidate that is not
//! absolute, because for one the file that is checked (looked up from the
//! working directory) and the file that loads (found by the loader's own
//! search) can be two different files. For the same reason
//! [`set_bundle_dir`] takes only an absolute directory.
//!
//! A path that exists but fails the checks ends the search with
//! [`Error::RuntimeLoad`]. Quietly falling through to the next candidate
//! would run something other than what the user pointed at.
//!
//! ## The macOS minimum
//!
//! The runtime the installers ship (1.23.2) needs macOS 13.4. dyld only
//! reports a too-new minimum lazily, as an addendum to a symbol error, so
//! that answer is late and says nothing about what to do. So on macOS,
//! once a library has been found and before it is opened, the running
//! macOS is read (`kern.osproductversion`) and compared with `min_macos`
//! from `apps/desktop/src-tauri/ml-runtime.json`, included at build time so
//! the pin has one home. An older system is [`Error::RuntimeUnsupported`]
//! and the file is never opened. A version that cannot be read or parsed
//! does not refuse: the guard exists to give a clear answer, and dyld still
//! decides. So does `10.16`, which is not a version but what the sysctl
//! answers a process that has `SYSTEM_VERSION_COMPAT=1` set: a macOS 14
//! started from a Terminal that exports it would otherwise be told it is
//! too old.
//!
//! ## The library's own dependencies
//!
//! Choosing the file is not enough on Windows, where the library's imports
//! (`msvcp140.dll`, `vcruntime140.dll`, `vcruntime140_1.dll`) are found by
//! a search of their own. `LoadLibraryExW(path, 0)` searches the default
//! order for them, which includes the working directory and `PATH`, so a
//! `vcruntime140_1.dll` planted in a writable launch directory, or an old
//! copy somewhere on `PATH`, would be loaded into the process (CWE-427
//! again, one level down). The library is therefore opened with
//! `LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32`: its
//! dependencies come from the directory it sits in (where the installer
//! puts the VC++ runtime DLLs beside it) and from `System32`, and from
//! nowhere else, so the working directory and `PATH` are not searched for
//! them either. Elsewhere the platform loader resolves dependencies as it
//! does for any library, relative to the file we name.

use std::ffi::{CStr, OsStr};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use crate::{Error, Result};

/// The library `ensure` found and loaded, and the version it reported,
/// once it has.
static LOADED: OnceLock<Loaded> = OnceLock::new();

/// Where the installer puts the library, registered once at startup by
/// [`set_bundle_dir`].
static BUNDLE_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Our own handle on the loaded library, kept for the life of the process
/// so the library is never unloaded under `ort`.
static PINNED: OnceLock<libloading::Library> = OnceLock::new();

/// Serialises the load, so two threads racing on a cold start do not
/// both open the library and call `init_from`.
static LOAD_LOCK: Mutex<()> = Mutex::new(());

/// The C signature of `OrtGetApiBase`, as `ort-sys` declares it.
type GetApiBase = unsafe extern "system" fn() -> *const ort::sys::OrtApiBase;

/// `apps/desktop/src-tauri/ml-runtime.json`, the one place the pinned
/// runtime is written down. Included, not read at run time: the macOS
/// minimum below has to travel with the binary it guards.
const MANIFEST: &str = include_str!("../../../apps/desktop/src-tauri/ml-runtime.json");

/// A library that passed [`check_library`] and is held open.
#[derive(Debug)]
struct Loaded {
    path: PathBuf,
    /// What its `GetVersionString` said, such as `1.23.2`.
    version: String,
}

/// Where a candidate path came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// `ORT_DYLIB_PATH`.
    Environment,
    /// The directory registered with [`set_bundle_dir`].
    Bundle,
    /// Next to the executable.
    Executable,
}

/// The library `ensure` loaded: [`info`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeInfo {
    /// The file that was loaded. Always absolute.
    pub path: PathBuf,
    /// The runtime's own version string, such as `1.23.2`.
    pub version: String,
    /// Which of the places searched it was found in.
    pub origin: Origin,
}

impl RuntimeInfo {
    /// Whether the installer put this library here, as opposed to someone
    /// pointing the app at one (`ORT_DYLIB_PATH`) or leaving one next to a
    /// development build. The bundle directory is where macOS and Linux
    /// installs carry it; the Windows installer puts the DLL next to the
    /// exe, where there is no bundle directory to register, so there a
    /// library beside the executable is the installer's.
    pub fn bundled(&self) -> bool {
        match self.origin {
            Origin::Bundle => true,
            Origin::Executable => cfg!(windows),
            Origin::Environment => false,
        }
    }
}

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

/// Register the directory the installer puts the library in, for
/// [`ensure`] to search after `ORT_DYLIB_PATH` and before the executable's
/// own directory. The app calls this once, at startup; see
/// [`bundle_dir_for`] for what to pass.
///
/// Returns whether this call set it. It does not when `dir` is not an
/// absolute path (every candidate must be, see [`ensure_with`]) or when a
/// directory was registered already: the first one stands, so nothing that
/// runs later can redirect the search.
pub fn set_bundle_dir(dir: PathBuf) -> bool {
    dir.is_absolute() && BUNDLE_DIR.set(dir).is_ok()
}

/// The directory registered with [`set_bundle_dir`], if any.
pub fn bundle_dir() -> Option<&'static Path> {
    BUNDLE_DIR.get().map(PathBuf::as_path)
}

/// Where the installer for `os` (a value of [`std::env::consts::OS`]) puts
/// the library, given the running executable and the directory Tauri calls
/// its resource directory. `None` when there is no such directory to add.
///
/// - `"macos"`: `Contents/Frameworks`, the sibling of `Contents/MacOS` that
///   the executable runs from (`bundle.macOS.frameworks` puts the dylib
///   there and Tauri signs it with the app). An executable that is not at
///   `<name>.app/Contents/MacOS/<exe>`, such as a `cargo run` build or the
///   CLI, has no such directory, and none is guessed.
/// - `"linux"`: the resource directory, passed in because it is Tauri's
///   to work out (`tauri_utils::platform::resource_dir`): `../lib/<name>`
///   from the executable for a deb or an AppImage, which is
///   `/usr/lib/edytlab` or `<mount>/usr/lib/edytlab`, and the executable's
///   own directory in a `target/` build.
/// - anything else, Windows included: none. The Windows installer puts the
///   DLL next to the exe, which is always searched.
pub fn bundle_dir_for(
    os: &str,
    exe: Option<&Path>,
    resource_dir: Option<&Path>,
) -> Option<PathBuf> {
    match os {
        "macos" => macos_frameworks_dir(exe?),
        "linux" => resource_dir
            .filter(|dir| dir.is_absolute())
            .map(Path::to_path_buf),
        _ => None,
    }
}

/// `<name>.app/Contents/Frameworks` for the executable
/// `<name>.app/Contents/MacOS/<exe>`, or `None` for any other place.
fn macos_frameworks_dir(exe: &Path) -> Option<PathBuf> {
    let macos = exe.parent()?;
    if macos.file_name()? != "MacOS" {
        return None;
    }
    let contents = macos.parent()?;
    if contents.file_name()? != "Contents" || !contents.is_absolute() {
        return None;
    }
    Some(contents.join("Frameworks"))
}

/// Every place [`ensure`] will look, in order. See the module doc.
pub fn candidate_paths() -> Vec<PathBuf> {
    tagged_candidate_paths()
        .into_iter()
        .map(|(path, _)| path)
        .collect()
}

fn tagged_candidate_paths() -> Vec<(PathBuf, Origin)> {
    let exe = std::env::current_exe().ok();
    tagged_candidates(
        std::env::var_os("ORT_DYLIB_PATH").as_deref(),
        bundle_dir(),
        exe.as_deref().and_then(Path::parent),
    )
}

/// [`candidate_paths`] with its inputs passed in, each path with the place
/// it came from, so the order can be tested without touching the process
/// environment. There is no working directory among them on purpose; see
/// the module doc.
fn tagged_candidates(
    env_path: Option<&OsStr>,
    bundle_dir: Option<&Path>,
    exe_dir: Option<&Path>,
) -> Vec<(PathBuf, Origin)> {
    let mut out: Vec<(PathBuf, Origin)> = Vec::new();
    let mut push = |p: PathBuf, origin: Origin| {
        if !out.iter().any(|(seen, _)| *seen == p) {
            out.push((p, origin));
        }
    };

    if let Some(raw) = env_path.filter(|v| !v.is_empty()) {
        let raw = Path::new(raw);
        if raw.is_absolute() {
            push(raw.to_path_buf(), Origin::Environment);
        } else if let Some(dir) = exe_dir {
            push(dir.join(raw), Origin::Environment);
        }
    }
    // A bundle directory that is the executable's own (Tauri's resource
    // directory in a `target/` build) is not a bundle: it stays the one
    // candidate "next to the executable".
    if let Some(dir) = bundle_dir.filter(|dir| Some(*dir) != exe_dir) {
        push(dir.join(library_file_name()), Origin::Bundle);
    }
    if let Some(dir) = exe_dir {
        push(dir.join(library_file_name()), Origin::Executable);
    }
    out
}

/// Make sure an ONNX Runtime library is loaded, loading it if needed.
///
/// Returns the path it loaded. Cheap after the first success: that is a
/// lock-free read. After a failure nothing is remembered, so a later
/// call retries — the user may have put the library in place since.
///
/// **Every entry point into `ort` must call this first.** `Session::builder`,
/// `ort::init`, `ort::api()` and the rest all load the library on first
/// use, panic when they cannot (a release build aborts) and search the
/// system for a bare name, so a caller that skips this loses the
/// protection this module exists to give.
pub fn ensure() -> Result<PathBuf> {
    ensure_with(&candidate_paths())
}

/// [`ensure`] over an explicit candidate list: the first one that is a
/// file is checked and loaded, or [`Error::MissingRuntime`] if none is.
///
/// Every candidate must be an absolute path. A relative one is refused with
/// [`Error::RuntimeLoad`] before anything is looked up, whether or not a
/// library has loaded already: see the module doc for why.
///
/// On macOS, a file that is found is not opened on a system older than the
/// runtime's minimum: [`Error::RuntimeUnsupported`], see the module doc.
///
/// Once any library has loaded, this returns that path whatever
/// `candidates` says: `ort` holds one library per process.
pub fn ensure_with(candidates: &[PathBuf]) -> Result<PathBuf> {
    load(candidates, check_os).map(|loaded| loaded.path.clone())
}

/// What [`ensure`] loaded, loading it first if need be: the file, the
/// version the runtime reports, and which place it was found in. For a
/// status line; nothing needs it to build a session.
///
/// It is [`ensure`] with the answer kept, so it has the same effects and
/// the same errors: the library is opened by absolute path, checked, and
/// handed to `ort` once, and a missing, unsupported or unloadable one is an
/// [`Error`], never a panic and never a hang. It blocks on that load, so a
/// caller on an async runtime runs it on a blocking thread.
pub fn info() -> Result<RuntimeInfo> {
    info_with(&tagged_candidate_paths(), check_os)
}

fn info_with(
    tagged: &[(PathBuf, Origin)],
    os_check: impl FnOnce() -> Result<()>,
) -> Result<RuntimeInfo> {
    let paths: Vec<PathBuf> = tagged.iter().map(|(path, _)| path.clone()).collect();
    let loaded = load(&paths, os_check)?;
    // A library that loaded earlier from a place the list no longer has
    // (the environment changed since) was pointed at by someone.
    let origin = tagged
        .iter()
        .find(|(path, _)| *path == loaded.path)
        .map_or(Origin::Environment, |(_, origin)| *origin);
    Ok(RuntimeInfo {
        path: loaded.path.clone(),
        version: loaded.version.clone(),
        origin,
    })
}

/// [`ensure_with`] with the operating-system check passed in, so the order
/// (check the OS, then open the file) is testable on any OS.
fn load(candidates: &[PathBuf], os_check: impl FnOnce() -> Result<()>) -> Result<&'static Loaded> {
    if let Some(relative) = candidates.iter().find(|p| !p.is_absolute()) {
        return Err(Error::RuntimeLoad {
            path: relative.clone(),
            reason: "not an absolute path, so the system loader could open a different file \
                     than the one that was checked"
                .into(),
        });
    }
    if let Some(loaded) = LOADED.get() {
        return Ok(loaded);
    }
    let _guard = LOAD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(loaded) = LOADED.get() {
        return Ok(loaded);
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

    // Before the file is opened. See "The macOS minimum" in the module doc.
    os_check()?;

    // Only this one is tried: it is the file the user's configuration
    // (or the install layout) points at, and a failure is theirs to hear.
    let (library, version) = check_library(path).map_err(|reason| Error::RuntimeLoad {
        path: path.clone(),
        reason,
    })?;

    // The invariant: `library` stays open across `init_from`. While we
    // hold it, `ort`'s own open of `path` is answered with this very
    // library, the one that just passed every check `ort` makes, instead
    // of reading the file a second time. Close it first (move it, drop
    // it, or reorder these lines) and `ort` loads whatever is at `path`
    // by then: a file swapped in between would get only `ort`'s own,
    // thinner checks, and some failures there are panics (see the module
    // doc).
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
    let _ = LOADED.set(Loaded {
        path: path.clone(),
        version,
    });
    Ok(LOADED.get().expect("set just above, under the load lock"))
}

// -- the macOS minimum ------------------------------------------------------

/// The oldest macOS the pinned runtime runs on, as `ml-runtime.json` says
/// (`min_macos`). Empty if the manifest has none, which `parse_macos_version`
/// does not accept and a test fails on.
pub fn min_macos() -> &'static str {
    static MIN: OnceLock<String> = OnceLock::new();
    MIN.get_or_init(|| {
        serde_json::from_str::<serde_json::Value>(MANIFEST)
            .ok()
            .and_then(|m| m.get("min_macos")?.as_str().map(str::to_owned))
            .unwrap_or_default()
    })
}

/// The operating-system half of [`load`]: refuse a macOS older than
/// [`min_macos`]. Nothing to check anywhere else.
fn check_os() -> Result<()> {
    if cfg!(target_os = "macos") {
        refuse_old_macos(running_macos_version().as_deref(), min_macos())
    } else {
        Ok(())
    }
}

/// What `kern.osproductversion` answers to a process that has
/// system-version compatibility on (`SYSTEM_VERSION_COMPAT=1`, an old pip
/// workaround people export in their shell): XNU's `sysctl_osproductversion`
/// returns `osproductversioncompat`, `SystemVersionCompat.plist`'s
/// ProductVersion, instead of the real version. No macOS ever had this
/// number, so it says nothing about the system the app is running on.
/// (XNU `bsd/kern/kern_sysctl.c`; OpenJDK JDK-8269850.)
const MACOS_COMPAT_VERSION: (u32, u32) = (10, 16);

/// [`Error::RuntimeUnsupported`] if `running` is a macOS older than
/// `needs`. A version that is unknown (`None`), that is the compatibility
/// stand-in ([`MACOS_COMPAT_VERSION`]), or that neither side can be parsed
/// from does not refuse, and says so in the log: the check gives a clear
/// answer, it is not a gate, and dyld still decides.
fn refuse_old_macos(running: Option<&str>, needs: &str) -> Result<()> {
    let Some(running) = running else {
        tracing::warn!("could not read the macOS version; not checking the ONNX Runtime minimum");
        return Ok(());
    };
    if parse_macos_version(running) == Some(MACOS_COMPAT_VERSION) {
        tracing::warn!(
            running,
            "macOS reports its system-version compatibility number (SYSTEM_VERSION_COMPAT is \
             on for this process), not its version; not checking the ONNX Runtime minimum"
        );
        return Ok(());
    }
    match macos_meets(running, needs) {
        Some(false) => Err(Error::RuntimeUnsupported {
            needs: needs.trim().to_string(),
            running: running.trim().to_string(),
        }),
        Some(true) => Ok(()),
        None => {
            tracing::warn!(
                running,
                needs,
                "could not compare macOS versions; not checking the ONNX Runtime minimum"
            );
            Ok(())
        }
    }
}

/// Whether macOS `running` is at least `needs`, comparing major and then
/// minor as numbers (`13.10` is newer than `13.4`); a patch level is
/// ignored, since no minimum names one. `None` if either does not parse.
fn macos_meets(running: &str, needs: &str) -> Option<bool> {
    Some(parse_macos_version(running)? >= parse_macos_version(needs)?)
}

/// `(major, minor)` of `13.4`, `13.4.1` or `14` (which is `14.0`). Plain
/// decimal digits only: no sign, no suffix, no empty part.
fn parse_macos_version(text: &str) -> Option<(u32, u32)> {
    fn number(part: &str) -> Option<u32> {
        if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        part.parse().ok()
    }
    let mut parts = text.trim().split('.');
    let major = number(parts.next()?)?;
    let minor = match parts.next() {
        Some(part) => number(part)?,
        None => 0,
    };
    // A patch level, and anything after it, must still be a number.
    for part in parts {
        number(part)?;
    }
    Some((major, minor))
}

/// The running macOS, from `kern.osproductversion` (`13.4.1`). The same
/// string `sw_vers -productVersion` prints, read through `sysctlbyname`
/// so that nothing is spawned. `None` if the call fails.
#[cfg(target_os = "macos")]
fn running_macos_version() -> Option<String> {
    let name = std::ffi::CString::new("kern.osproductversion").ok()?;
    let mut len: libc::size_t = 0;
    // SAFETY: with a null buffer `sysctlbyname` only writes the size it
    // needs into `len`; `name` is a valid NUL-terminated string.
    let rc = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            std::ptr::null_mut(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 || len == 0 {
        return None;
    }
    let mut buf = vec![0u8; len];
    // SAFETY: `buf` is `len` bytes, which `sysctlbyname` is told, and may
    // shorten `len` to what it wrote; `name` is as above.
    let rc = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            buf.as_mut_ptr().cast(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 {
        return None;
    }
    buf.truncate(len);
    // The value is a NUL-terminated C string.
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    String::from_utf8(buf[..end].to_vec()).ok()
}

#[cfg(not(target_os = "macos"))]
fn running_macos_version() -> Option<String> {
    None
}

/// Open the library at `path`, by absolute path.
///
/// On Windows its dependencies are searched for in the directory it sits in
/// and in `System32` only (see "The library's own dependencies" in the
/// module doc), never in the working directory or on `PATH`. The handle is
/// kept by [`PINNED`], so `ort::init_from`'s own `Library::new(path)` is
/// answered with this already loaded module and searches for nothing.
#[cfg(windows)]
fn open_library(path: &Path) -> std::result::Result<libloading::Library, String> {
    use libloading::os::windows::{
        Library as WindowsLibrary, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, LOAD_LIBRARY_SEARCH_SYSTEM32,
    };
    // SAFETY: opening a library runs its initialisers. This is the file
    // the user or the install layout names as ONNX Runtime, and loading
    // it is exactly what `ort` would do next.
    unsafe {
        WindowsLibrary::load_with_flags(
            path,
            LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32,
        )
    }
    .map(libloading::Library::from)
    .map_err(|e| e.to_string())
}

/// Open the library at `path`, by absolute path.
#[cfg(not(windows))]
fn open_library(path: &Path) -> std::result::Result<libloading::Library, String> {
    // SAFETY: opening a library runs its initialisers. This is the file
    // the user or the install layout names as ONNX Runtime, and loading
    // it is exactly what `ort` would do next.
    unsafe { libloading::Library::new(path) }.map_err(|e| e.to_string())
}

/// Open `path` and make the checks `ort` makes when it loads a library,
/// without going through `ort` (which cannot report a failure; see the
/// module doc). Returns the open library and the version it reports, or why
/// it is unusable.
///
/// On failure the library is closed again, so the file can be replaced
/// (on Windows an open DLL cannot be) and checked again.
fn check_library(path: &Path) -> std::result::Result<(libloading::Library, String), String> {
    let library = open_library(path)?;

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
        // `OrtApiBase`; its `GetVersionString` takes no arguments.
        let version_ptr = unsafe { ((*base).GetVersionString)() };
        let version = version_from_ptr(version_ptr)
            .ok_or_else(|| "its GetVersionString returned null".to_string())?;

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
    Ok((library, version))
}

/// The text of the C string a library returned, or `None` if it returned
/// a null pointer (which `CStr::from_ptr` would turn into undefined
/// behaviour).
fn version_from_ptr(ptr: *const std::ffi::c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    // SAFETY: `ptr` is non-null, and the caller passes what
    // `GetVersionString` returned: a static, NUL-terminated string.
    Some(
        unsafe { CStr::from_ptr(ptr) }
            .to_string_lossy()
            .into_owned(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::mpsc;
    use std::time::Duration;

    // None of these loads a real ONNX Runtime: CI has none, and a load
    // that succeeded would be process-wide state for every other test.

    /// [`tagged_candidates`] without the tags.
    fn candidates_from(
        env_path: Option<&OsStr>,
        bundle_dir: Option<&Path>,
        exe_dir: Option<&Path>,
    ) -> Vec<PathBuf> {
        tagged_candidates(env_path, bundle_dir, exe_dir)
            .into_iter()
            .map(|(path, _)| path)
            .collect()
    }

    /// Run `ensure_with` on another thread and fail, rather than hang, if
    /// it never returns. A library that failed to load inside `ort`
    /// 2.0.0-rc.12 blocked the thread forever (see the module doc), and
    /// rc.13 fixed that, but a regression there, or any other stall in the
    /// load, would still stall the test run instead of failing it without
    /// this.
    ///
    /// On a timeout it aborts rather than panicking: the stuck thread
    /// holds a lock that `ort`'s exit handler waits on, so the process
    /// could not exit normally afterwards. The message goes straight to
    /// stderr because the harness captures `eprintln!`.
    fn run_or_abort(work: impl FnOnce() -> Result<PathBuf> + Send + 'static) -> Result<PathBuf> {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(work());
        });
        match rx.recv_timeout(Duration::from_secs(30)) {
            Ok(result) => result,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let _ = writeln!(
                    std::io::stderr(),
                    "ensure_with did not return within 30 s. A library that fails to load must \
                     be rejected with an error (`ort` 2.0.0-rc.12 deadlocked on a failed load; \
                     rc.13 does not, so suspect a regression there)."
                );
                std::process::abort();
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => panic!("ensure_with panicked"),
        }
    }

    fn ensure_or_fail(candidates: Vec<PathBuf>) -> Result<PathBuf> {
        run_or_abort(move || ensure_with(&candidates))
    }

    /// [`ensure_or_fail`] with the operating-system check replaced by
    /// `os_check`, so the macOS refusal can be exercised on any OS.
    fn load_or_fail(
        candidates: Vec<PathBuf>,
        os_check: impl FnOnce() -> Result<()> + Send + 'static,
    ) -> Result<PathBuf> {
        run_or_abort(move || load(&candidates, os_check).map(|loaded| loaded.path.clone()))
    }

    /// `ort` refuses a library older than the API level it was built for
    /// (`ort::MINOR_VERSION`, set by its `api-NN` feature), so that level
    /// is the oldest runtime the app can ever load. It is pinned to 23
    /// because the runtime the installers ship is 1.23.2, and 1.23.2 is
    /// the newest release with a macOS build for Intel Macs.
    #[test]
    fn ort_is_built_for_the_runtime_the_installers_pin() {
        assert_eq!(
            ort::MINOR_VERSION,
            23,
            "`ort` is built for ONNX Runtime API level {} (1.{}.x or newer), but the runtime \
             the app pins is 1.23.2. 1.23.2 is the last release with a macOS build for Intel \
             Macs (1.24 and later are arm64 only), and `ort` rejects a library older than its \
             API level, so raising the level strands them. Keep the `api-23` feature on `ort` \
             in the workspace Cargo.toml, and see #383 before changing the pin.",
            ort::MINOR_VERSION,
            ort::MINOR_VERSION,
        );
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
    fn candidates_prefer_the_environment_then_the_bundle_then_the_executable_directory() {
        // Built from the temp dir so they are absolute on every OS.
        let base = std::env::temp_dir();
        let exe_dir = base.join("app");
        let bundle = base.join("bundle");
        let abs = base.join("elsewhere").join("ort.lib");

        let got = candidates_from(Some(abs.as_os_str()), Some(&bundle), Some(&exe_dir));
        assert_eq!(
            got,
            vec![
                abs,
                bundle.join(library_file_name()),
                exe_dir.join(library_file_name())
            ]
        );

        // A relative ORT_DYLIB_PATH is resolved against the executable's
        // directory.
        let got = candidates_from(
            Some(OsStr::new("lib/ort.so")),
            Some(&bundle),
            Some(&exe_dir),
        );
        assert_eq!(
            got,
            vec![
                exe_dir.join("lib/ort.so"),
                bundle.join(library_file_name()),
                exe_dir.join(library_file_name())
            ]
        );

        // Unset and empty mean the same thing: the bundle, then the
        // executable's directory. There is no bare-name fallback to the
        // system.
        for env in [None, Some(OsStr::new(""))] {
            let got = candidates_from(env, Some(&bundle), Some(&exe_dir));
            assert_eq!(
                got,
                vec![
                    bundle.join(library_file_name()),
                    exe_dir.join(library_file_name())
                ]
            );
        }

        // No bundle registered (a Windows install, a `cargo run`): what
        // the search was before the bundle existed.
        let got = candidates_from(None, None, Some(&exe_dir));
        assert_eq!(got, vec![exe_dir.join(library_file_name())]);
        assert!(candidates_from(None, None, None).is_empty());

        // The bundle is still searched when the executable's directory is
        // unknown.
        assert_eq!(
            candidates_from(None, Some(&bundle), None),
            vec![bundle.join(library_file_name())]
        );
    }

    /// A bundle directory that is the executable's own (a `cargo run`,
    /// where Tauri's resource directory is `target/debug`) is one
    /// candidate, and it stays "next to the executable", not "bundled".
    #[test]
    fn a_bundle_directory_equal_to_the_executable_directory_is_one_candidate() {
        let exe_dir = std::env::temp_dir().join("app");
        let tagged = tagged_candidates(None, Some(&exe_dir), Some(&exe_dir));
        assert_eq!(
            tagged,
            vec![(exe_dir.join(library_file_name()), Origin::Executable)]
        );
    }

    #[test]
    fn each_candidate_is_tagged_with_where_it_came_from() {
        let base = std::env::temp_dir();
        let exe_dir = base.join("app");
        let bundle = base.join("bundle");
        let abs = base.join("elsewhere").join("ort.lib");

        let tagged = tagged_candidates(Some(abs.as_os_str()), Some(&bundle), Some(&exe_dir));
        assert_eq!(
            tagged,
            vec![
                (abs, Origin::Environment),
                (bundle.join(library_file_name()), Origin::Bundle),
                (exe_dir.join(library_file_name()), Origin::Executable),
            ]
        );
    }

    #[test]
    fn only_the_bundle_counts_as_bundled_and_on_windows_the_executable_directory_too() {
        let info = |origin| RuntimeInfo {
            path: PathBuf::from("x"),
            version: "1.23.2".into(),
            origin,
        };
        assert!(info(Origin::Bundle).bundled());
        assert!(!info(Origin::Environment).bundled());
        // The Windows installer puts the DLL beside the exe, which is the
        // bundle there. Anywhere else a library next to the executable
        // was put there by hand or by a build.
        assert_eq!(info(Origin::Executable).bundled(), cfg!(windows));
    }

    /// The installer's layout, per OS, from a fake executable path: where
    /// the app tells `ensure` to look first (`set_bundle_dir`).
    ///
    /// The fake paths hang off the temp directory so that they are absolute
    /// on every OS (`/Applications/...` is not on Windows, where the layout
    /// is the same function over the same path shape).
    #[test]
    fn the_bundle_directory_follows_each_installers_layout() {
        let base = std::env::temp_dir();

        // macOS: `/Applications/edytlab.app/Contents/MacOS/<exe>`, and the
        // library in the sibling `Contents/Frameworks`
        // (`bundle.macOS.frameworks`).
        let app = base.join("Applications").join("edytlab.app");
        let exe = app.join("Contents").join("MacOS").join("edytlab");
        let frameworks = app.join("Contents").join("Frameworks");
        assert_eq!(
            bundle_dir_for("macos", Some(&exe), None),
            Some(frameworks.clone())
        );
        // The resource directory is not what macOS uses, even if given.
        let resources = app.join("Contents").join("Resources");
        assert_eq!(
            bundle_dir_for("macos", Some(&exe), Some(&resources)),
            Some(frameworks)
        );

        // Linux: Tauri's resource directory, which for a deb is
        // `/usr/lib/edytlab` and for an AppImage `<mount>/usr/lib/edytlab`.
        // The executable does not say.
        let deb_exe = base.join("usr").join("bin").join("edytlab");
        let deb_res = base.join("usr").join("lib").join("edytlab");
        assert_eq!(
            bundle_dir_for("linux", Some(&deb_exe), Some(&deb_res)),
            Some(deb_res.clone())
        );
        let mount = base.join(".mount_edytlaAbc123");
        let appimage_exe = mount.join("usr").join("bin").join("edytlab");
        let appimage_res = mount.join("usr").join("lib").join("edytlab");
        assert_eq!(
            bundle_dir_for("linux", Some(&appimage_exe), Some(&appimage_res)),
            Some(appimage_res)
        );
        assert_eq!(bundle_dir_for("linux", Some(&deb_exe), None), None);

        // Windows: the DLL sits next to the exe, which is searched anyway,
        // so there is nothing to add (and so no duplicate candidate).
        assert_eq!(
            bundle_dir_for(
                "windows",
                Some(Path::new(r"C:\Program Files\edytlab\edytlab.exe")),
                Some(Path::new(r"C:\Program Files\edytlab"))
            ),
            None
        );
    }

    /// A macOS executable that is not inside an app bundle (`cargo run`,
    /// the CLI) has no `Frameworks` sibling to look in. Guessing one from
    /// any directory's parent would search a directory nobody installed.
    #[test]
    fn a_macos_executable_outside_an_app_bundle_has_no_bundle_directory() {
        let base = std::env::temp_dir();
        let app = base.join("Applications").join("edytlab.app");
        let not_in_a_bundle = [
            base.join("edytlab")
                .join("target")
                .join("debug")
                .join("edytlab"),
            base.join("usr").join("local").join("bin").join("edytlab"),
            // Inside the bundle, but not where the executable lives.
            app.join("Contents").join("Resources").join("edytlab"),
            app.join("MacOS").join("edytlab"),
            // `MacOS` is right but `Contents` is not.
            base.join("elsewhere").join("MacOS").join("edytlab"),
            base.join("MacOS").join("edytlab"),
            PathBuf::from("edytlab"),
        ];
        for exe in &not_in_a_bundle {
            assert_eq!(
                bundle_dir_for("macos", Some(exe), None),
                None,
                "{}",
                exe.display()
            );
        }
        assert_eq!(bundle_dir_for("macos", None, None), None);
    }

    /// A relative directory would make every `ensure` fail (a relative
    /// candidate is refused), so it is never registered as the bundle.
    #[test]
    fn a_relative_resource_directory_is_not_a_bundle_directory() {
        assert_eq!(
            bundle_dir_for("linux", None, Some(Path::new("lib/edytlab"))),
            None
        );
    }

    /// The same candidate, found through `ensure_with`'s ordering rules,
    /// with the OS check held out of it: a library in the bundle is the
    /// one that is tried, before the one next to the executable.
    #[test]
    fn the_bundle_is_tried_before_the_executable_directory() {
        let dir = tempfile::tempdir().expect("tempdir");
        let exe_dir = dir.path().join("MacOS");
        let bundle = dir.path().join("Frameworks");
        for d in [&exe_dir, &bundle] {
            std::fs::create_dir_all(d).unwrap();
            std::fs::write(d.join(library_file_name()), b"not a library").unwrap();
        }
        let candidates = candidates_from(None, Some(&bundle), Some(&exe_dir));
        match load_or_fail(candidates, || Ok(())) {
            Err(Error::RuntimeLoad { path, .. }) => {
                assert_eq!(path, bundle.join(library_file_name()))
            }
            other => panic!("expected RuntimeLoad for the bundle's copy, got {other:?}"),
        }
    }

    /// B3. `ORT_DYLIB_PATH=lib/ort.so` used to be tried against the working
    /// directory too, so whoever could write to the directory the app was
    /// started from could get a library of their choosing loaded (CWE-427).
    /// The working directory is no longer an input to the search at all.
    #[test]
    fn a_relative_env_path_is_resolved_against_the_executable_directory_only() {
        let exe_dir = std::env::temp_dir().join("app");

        for raw in ["lib/ort.so", "ort.so", "./ort.so", "../ort.so"] {
            let got = candidates_from(Some(OsStr::new(raw)), None, Some(&exe_dir));
            assert_eq!(
                got,
                vec![exe_dir.join(raw), exe_dir.join(library_file_name())],
                "{raw}"
            );
        }

        // With no executable directory there is nothing to resolve it
        // against. It is dropped, not tried against wherever the process
        // happens to be running.
        assert!(candidates_from(Some(OsStr::new("lib/ort.so")), None, None).is_empty());
    }

    /// B2. A relative candidate is looked for with `is_file` in the working
    /// directory but opened by the system loader's own search (on Windows
    /// that includes System32), so the file that is checked and the file
    /// that loads can differ. It must never get that far.
    #[test]
    fn a_relative_candidate_is_refused_not_looked_up() {
        // `cargo test` runs a package's tests with the package directory as
        // the working directory, so this file really is there to be found.
        let there = PathBuf::from("Cargo.toml");
        assert!(there.is_file(), "tests run from the package directory");

        for relative in [
            there,
            PathBuf::from("no/such/dir/libonnxruntime.so"),
            PathBuf::from(library_file_name()),
        ] {
            match ensure_or_fail(vec![relative.clone()]) {
                Err(Error::RuntimeLoad { path, reason }) => {
                    assert_eq!(path, relative);
                    assert!(reason.contains("not an absolute path"), "{reason}");
                }
                other => panic!(
                    "{}: expected RuntimeLoad, got {other:?}",
                    relative.display()
                ),
            }
        }

        // Not skipped because an absolute candidate came first and was
        // simply missing.
        let dir = tempfile::tempdir().expect("tempdir");
        let missing = dir.path().join("nope").join(library_file_name());
        let relative = PathBuf::from("lib").join(library_file_name());
        match ensure_or_fail(vec![missing, relative.clone()]) {
            Err(Error::RuntimeLoad { path, reason }) => {
                assert_eq!(path, relative);
                assert!(reason.contains("not an absolute path"), "{reason}");
            }
            other => panic!("expected RuntimeLoad, got {other:?}"),
        }
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
        assert!(msg.contains("#383"), "{msg}");
        // It reports the fact. A tool passes this text to the agent, which
        // must not read a remedy into it.
        assert!(!msg.contains("ORT_DYLIB_PATH"), "{msg}");

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

    /// A shared library that is certainly on this machine, loads fine, and
    /// is not ONNX Runtime.
    ///
    /// On Linux, one this very process has mapped, read from
    /// `/proc/self/maps`, so where it sits does not matter: Debian keeps
    /// libm in `/lib/x86_64-linux-gnu`, Fedora in `/lib64`, Arch in
    /// `/usr/lib`. On Windows, `kernel32.dll` in the system directory.
    /// `None` where neither works, such as a static musl build, which maps
    /// no shared library at all.
    #[cfg(any(target_os = "linux", windows))]
    fn some_other_shared_library() -> Option<PathBuf> {
        #[cfg(windows)]
        {
            let root = std::env::var_os("SystemRoot")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
            Some(root.join("System32").join("kernel32.dll"))
        }
        #[cfg(not(windows))]
        {
            let maps = std::fs::read_to_string("/proc/self/maps").ok()?;
            maps.lines()
                // The path is the last column and starts at the first
                // slash. Anonymous mappings and `[stack]` have none.
                .filter_map(|line| line.find('/').map(|i| PathBuf::from(&line[i..])))
                .find(|path| {
                    let name = path.file_name().and_then(OsStr::to_str).unwrap_or("");
                    name.contains(".so") && !name.contains("onnxruntime") && path.is_file()
                })
        }
    }

    /// A real shared library that loads fine but is not ONNX Runtime: the
    /// check `ort` would make next (`OrtGetApiBase`) is made here instead.
    #[cfg(any(target_os = "linux", windows))]
    #[test]
    fn a_real_library_that_is_not_onnxruntime_is_a_load_error() {
        let Some(other) = some_other_shared_library() else {
            // Written to stderr directly: the harness hides `eprintln!`
            // from a test that passes.
            let _ = writeln!(
                std::io::stderr(),
                "skipping a_real_library_that_is_not_onnxruntime_is_a_load_error: no shared \
                 library to load was found on this machine"
            );
            return;
        };
        assert!(
            other.is_absolute() && other.is_file(),
            "{}",
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

    /// `CStr::from_ptr(null)` is undefined behaviour, and a library we know
    /// nothing about is what answers `GetVersionString`.
    #[test]
    fn a_null_version_string_is_not_read() {
        assert_eq!(version_from_ptr(std::ptr::null()), None);

        let version = std::ffi::CString::new("1.24.1").unwrap();
        assert_eq!(
            version_from_ptr(version.as_ptr()).as_deref(),
            Some("1.24.1")
        );
    }

    // -- the macOS minimum (#383) ------------------------------------------

    /// dyld reports a library whose minimum OS is newer than the running
    /// one only lazily, as an addendum to a symbol error, so the refusal is
    /// ours. These are the comparisons it makes, against the manifest's
    /// 13.4.
    #[test]
    fn the_macos_minimum_is_compared_by_number_not_by_text() {
        let needs = "13.4";
        for (running, ok) in [
            ("13.3", false),
            ("13.3.1", false),
            ("13.3.9", false),
            ("13.4", true),
            ("13.4.0", true),
            ("13.4.1", true),
            // 10 > 4 as numbers; "13.10" < "13.4" as text.
            ("13.10", true),
            ("14.0", true),
            ("14", true),
            ("15.7.2", true),
            ("26.0", true),
            ("12.7.6", false),
            ("11.0", false),
            ("10.15", false),
            ("10.15.7", false),
            ("9", false),
        ] {
            assert_eq!(
                macos_meets(running, needs),
                Some(ok),
                "macOS {running} against a minimum of {needs}"
            );
        }
    }

    #[test]
    fn a_malformed_macos_version_is_not_compared() {
        for bad in [
            "",
            " ",
            "abc",
            "13.x",
            "13..4",
            "13.4.",
            ".4",
            "-13.4",
            "+13.4",
            "13.-4",
            "13,4",
            "13.4-beta",
            "13.4 (a)",
            "v13.4",
            "13.4.x",
            "99999999999.0",
        ] {
            assert_eq!(macos_meets(bad, "13.4"), None, "running {bad:?}");
            assert_eq!(macos_meets("14.0", bad), None, "needs {bad:?}");
        }
        // Whitespace around a good version is what `sysctl` hands back.
        assert_eq!(macos_meets("13.4\n", "13.4"), Some(true));
        assert_eq!(macos_meets(" 13.3 ", "13.4"), Some(false));
    }

    #[test]
    fn an_old_macos_is_refused_and_says_both_versions() {
        match refuse_old_macos(Some("12.6.1"), "13.4") {
            Err(Error::RuntimeUnsupported { needs, running }) => {
                assert_eq!(needs, "13.4");
                assert_eq!(running, "12.6.1");
            }
            other => panic!("expected RuntimeUnsupported, got {other:?}"),
        }
        let msg = refuse_old_macos(Some("13.3"), "13.4")
            .unwrap_err()
            .to_string();
        assert!(msg.contains("13.4") && msg.contains("13.3"), "{msg}");
        assert!(msg.contains("macOS"), "{msg}");
        // It reports the fact. A tool passes this text to the agent, which
        // must not read a remedy into it.
        assert!(!msg.contains("ORT_DYLIB_PATH"), "{msg}");

        assert!(refuse_old_macos(Some("13.4"), "13.4").is_ok());
        assert!(refuse_old_macos(Some("14.2"), "13.4").is_ok());
    }

    /// The guard is there to give a clear answer, not to be a gate that a
    /// machine we cannot read could lock the feature behind: if the running
    /// version is unknown or unreadable, dyld still decides.
    #[test]
    fn a_macos_version_that_cannot_be_read_does_not_refuse() {
        assert!(refuse_old_macos(None, "13.4").is_ok());
        assert!(refuse_old_macos(Some("garbage"), "13.4").is_ok());
        assert!(refuse_old_macos(Some("12.0"), "garbage").is_ok());
    }

    /// With system-version compatibility on for the process, XNU's
    /// `sysctl_osproductversion` answers `kern.osproductversion` with
    /// `osproductversioncompat` (`SystemVersionCompat.plist`'s
    /// ProductVersion), which is `10.16`. A process gets that from
    /// `SYSTEM_VERSION_COMPAT=1`, an old pip workaround people export in
    /// their shell, so a macOS 14 launched from such a Terminal reports
    /// itself as 10.16. No macOS ever had that number: it is a stand-in, not
    /// a version, so it must not be read as "older than 13.4" (the app used
    /// to refuse the library with "this Mac is running macOS 10.16").
    /// Sources: XNU `bsd/kern/kern_sysctl.c` (`sysctl_osproductversion`),
    /// OpenJDK JDK-8269850.
    #[test]
    fn the_compatibility_version_is_not_a_real_macos_and_does_not_refuse() {
        for compat in ["10.16", "10.16.0", " 10.16\n"] {
            assert!(
                refuse_old_macos(Some(compat), "13.4").is_ok(),
                "{compat:?} is what macOS reports under SYSTEM_VERSION_COMPAT"
            );
        }
        // Its neighbours are real versions and still refuse.
        for old in ["10.15", "10.15.7", "10.17", "11.0", "12.7.6"] {
            assert!(
                matches!(
                    refuse_old_macos(Some(old), "13.4"),
                    Err(Error::RuntimeUnsupported { .. })
                ),
                "macOS {old} is older than 13.4"
            );
        }
    }

    /// The minimum is read from `ml-runtime.json`, the one place the pin
    /// is written down, at build time. A manifest edit that breaks it
    /// would otherwise turn the guard into a no-op without a sound.
    #[test]
    fn the_manifests_minimum_macos_parses() {
        let needs = min_macos();
        assert!(
            parse_macos_version(needs).is_some(),
            "ml-runtime.json `min_macos` must be MAJOR.MINOR, got {needs:?}"
        );
        // 1.23.2 is the last release with a macOS build for Intel Macs,
        // and its minimum is 13.4 on both slices (#383).
        assert_eq!(needs, "13.4");
        // And the manifest's own text is what is compared against.
        let manifest: serde_json::Value = serde_json::from_str(MANIFEST).expect("manifest JSON");
        assert_eq!(manifest["min_macos"].as_str(), Some(needs));
    }

    /// The refusal comes before the library is opened, not after: a file
    /// that is not a library would otherwise be a `RuntimeLoad`, and on a
    /// Mac that is too old the real library would be opened and fail
    /// inside dyld with a message about a missing symbol.
    #[test]
    fn the_os_is_checked_before_the_library_is_opened() {
        let dir = tempfile::tempdir().expect("tempdir");
        let fake = dir.path().join(library_file_name());
        std::fs::write(&fake, b"this is not a shared library").expect("write");

        let refused = || {
            Err(Error::RuntimeUnsupported {
                needs: "13.4".into(),
                running: "12.6".into(),
            })
        };
        match load_or_fail(vec![fake.clone()], refused) {
            Err(Error::RuntimeUnsupported { needs, running }) => {
                assert_eq!((needs.as_str(), running.as_str()), ("13.4", "12.6"));
            }
            other => panic!("expected RuntimeUnsupported before any load, got {other:?}"),
        }
        // Nothing was remembered: the same file, on an OS that passes, is
        // opened (and, being junk, is a load error).
        assert!(matches!(
            load_or_fail(vec![fake], || Ok(())),
            Err(Error::RuntimeLoad { .. })
        ));
    }

    /// A Mac that is too old and has no library gets "not found", not a
    /// complaint about its OS: there is nothing to refuse to load.
    #[test]
    fn with_no_library_a_too_old_os_is_not_the_answer() {
        let dir = tempfile::tempdir().expect("tempdir");
        let missing = dir.path().join(library_file_name());
        let refused = || {
            Err(Error::RuntimeUnsupported {
                needs: "13.4".into(),
                running: "12.6".into(),
            })
        };
        assert!(matches!(
            load_or_fail(vec![missing], refused),
            Err(Error::MissingRuntime { .. })
        ));
    }
}
