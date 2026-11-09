#!/usr/bin/env python3
r"""Put the Tauri bundler's build tools where it looks for them, before it runs.

    python3 -I scripts/fetch-bundler-tools.py <downloads-dir>

`tauri build` downloads its packaging tools from GitHub the first time it needs
them (WiX and NSIS on Windows, AppRun and linuxdeploy on Linux) and does not
retry: one 504 from GitHub ends the bundle step and so the release leg
(dev.677 on Windows, dev.688 on Linux and Windows). It skips a download when the
file is already in its tool folder, `<cache dir>/tauri` (`~/.cache/tauri` on
Linux, `%LOCALAPPDATA%\tauri` on Windows; `bundle.useLocalToolsDir` is unset).
This script downloads the same files from the same URLs with retries, checks
each against a SHA-256 or SHA-1 written below, and lays them out the way the
bundler would, so it finds them and downloads nothing.

<downloads-dir> holds the verified downloads (not the unpacked tools) so a CI
cache can keep it. A file in it is checked again on every run, and one that
does not match is downloaded again, so a damaged cache costs a download, not a
wrong tool. macOS needs nothing: its app and dmg bundlers download nothing.

Where the bundler checks a file's hash, the same hash is written here
(WiX: SHA-256, NSIS and its plugin: SHA-1). The bundler checks neither AppRun
nor linuxdeploy; those two are pinned here to the SHA-256 of the release assets
as downloaded on 2026-11-09, which is stricter than the bundler. The AppImage
plugin is fetched from a moving `continuous` release the bundler does not check
either, so it cannot be pinned: it is fetched with retries and never cached,
and if it cannot be had the bundler tries itself, as it always did.

A file that cannot be downloaded is a warning, not a failure: the bundler then
makes its own attempt, so this step can only remove failures. A file whose hash
does not match is a failure, always.

The pins belong to one bundler release. `crates/tools/tests/release_bundler_tools.rs`
fails when `@tauri-apps/cli` moves off TAURI_CLI_VERSION, so a bump comes here
to re-read the bundler's `tauri-bundler-<version>/src/bundle/` constants
(`windows/msi/mod.rs`, `windows/nsis/mod.rs`, `linux/appimage/linuxdeploy.rs`).

Standard library only. Exit status: 0 done (or nothing to do), 1 integrity
failure or a tool folder it cannot find.
"""

import hashlib
import os
import shutil
import sys
import tempfile
import time
import urllib.error
import urllib.request
import zipfile

# `@tauri-apps/cli` in apps/desktop/package.json and pnpm-lock.yaml. tauri-cli
# 2.12.4 depends on `tauri-bundler = "=2.10.4"`.
TAURI_CLI_VERSION = "2.12.4"
TAURI_BUNDLER_VERSION = "2.10.4"

# name, url, (algorithm, hex digest)
LINUX_APPRUN = (
    "AppRun-x86_64",
    (
        "https://github.com/tauri-apps/binary-releases/releases/download/"
        "apprun-old/AppRun-x86_64"
    ),
    ("sha256", "f30140a43a0a59e46db21bdefdf749b9e9f2c6946e92afabbacf98b8ae73fb4f"),
)
LINUX_LINUXDEPLOY = (
    "linuxdeploy-07333c6-x86_64.AppImage",
    (
        "https://github.com/tauri-apps/binary-releases/releases/download/"
        "linuxdeploy-07333c6/linuxdeploy-x86_64.AppImage"
    ),
    ("sha256", "36a2d7e274d12e1050d0e9ecfe11d339ed54720b2bec464c286d53f8b07f5c62"),
)
# Moving target: no digest, never cached.
LINUX_APPIMAGE_PLUGIN = (
    "linuxdeploy-plugin-appimage.AppImage",
    (
        "https://github.com/linuxdeploy/linuxdeploy-plugin-appimage/releases/"
        "download/continuous/linuxdeploy-plugin-appimage-x86_64.AppImage"
    ),
)
WIX = (
    "wix314-binaries.zip",
    (
        "https://github.com/wixtoolset/wix3/releases/download/"
        "wix3141rtm/wix314-binaries.zip"
    ),
    ("sha256", "6ac824e1642d6f7277d0ed7ea09411a508f6116ba6fae0aa5f2c7daa2ff43d31"),
)
NSIS = (
    "nsis-3.13.zip",
    (
        "https://github.com/tauri-apps/binary-releases/releases/download/"
        "nsis-3.13/nsis-3.13.zip"
    ),
    ("sha1", "db14f8af2ea346b786c6ae19343f61aaf5e09b8e"),
)
NSIS_TAURI_UTILS = (
    "nsis_tauri_utils.dll",
    (
        "https://github.com/tauri-apps/nsis-tauri-utils/releases/download/"
        "nsis_tauri_utils-v0.5.3/nsis_tauri_utils.dll"
    ),
    ("sha1", "75197fee3c6a814fe035788d1c34ead39349b860"),
)

ATTEMPTS = 6
# Seconds to wait before attempt 2, 3, ...: a GitHub 504 usually clears within
# a minute or two.
DELAYS = (5, 15, 30, 45, 60)
TIMEOUT = 120


class IntegrityError(Exception):
    """A download or archive is not the one the pins describe."""


class ShortBodyError(OSError):
    """The body ended before Content-Length; an OSError, so it is retried."""


def log(message):
    """Print a progress line."""
    print(f"fetch-bundler-tools: {message}", flush=True)


def warn(message):
    """Print a GitHub Actions warning annotation."""
    print(f"::warning::fetch-bundler-tools: {message}", flush=True)


def digest_of(path, algorithm):
    """Return the hex digest of the file at `path`."""
    h = hashlib.new(algorithm)
    # skipcq: PTC-W6004 - paths are built here under our own cache and tool folders
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def retryable(error):
    """Whether another attempt could fix `error`."""
    if isinstance(error, urllib.error.HTTPError):
        return error.code >= 500 or error.code in (408, 425, 429)
    # URLError (DNS, refused, reset), timeouts, and a body cut short.
    return isinstance(error, (urllib.error.URLError, OSError, EOFError)) or (
        error.__class__.__name__ == "IncompleteRead"
    )


def download(url, dest, sleep=time.sleep):
    """Download `url` to `dest`, retrying what a later attempt can fix.

    Returns None on success, or the last error. `dest` is written whole or not
    at all.
    """
    if not url.startswith("https://"):
        raise ValueError(f"refusing a non-https URL: {url}")
    last = None
    for attempt in range(1, ATTEMPTS + 1):
        if attempt > 1:
            delay = DELAYS[min(attempt - 2, len(DELAYS) - 1)]
            log(f"retrying in {delay} s (attempt {attempt} of {ATTEMPTS}): {last}")
            sleep(delay)
        part = dest + ".part"
        try:
            request = urllib.request.Request(
                url, headers={"User-Agent": "edytlab-fetch-bundler-tools"}
            )
            # Safe: every URL is a module constant, and https:// is checked above.
            # skipcq: BAN-B310
            with urllib.request.urlopen(request, timeout=TIMEOUT) as response:
                with open(part, "wb") as out:
                    shutil.copyfileobj(response, out)
                    written = out.tell()
                # http.client does not raise when a body ends before its
                # Content-Length, so a cut-off download is caught here.
                expected = response.headers.get("Content-Length")
                if expected is not None and written != int(expected):
                    raise ShortBodyError(
                        f"got {written} of {expected} bytes from {url}"
                    )
            os.replace(part, dest)
            return None
        except Exception as error:  # noqa: BLE001 - classified just below
            last = error
            if os.path.exists(part):
                os.remove(part)
            if not retryable(error):
                return error
    return last


def verified(cache, entry, sleep=time.sleep):
    """The path of `entry` in `cache`, downloaded if needed and checked.

    None when it could not be downloaded. Raises IntegrityError on a download
    whose digest is not the pinned one.
    """
    name, url, (algorithm, expected) = entry
    path = os.path.join(cache, name)
    if os.path.isfile(path):
        if digest_of(path, algorithm) == expected:
            log(f"{name}: in the download cache, {algorithm} matches")
            return path
        log(
            f"{name}: in the download cache but its {algorithm} is not the "
            "pinned one; downloading again"
        )
        os.remove(path)
    log(f"downloading {url}")
    error = download(url, path, sleep)
    if error is not None:
        warn(f"{url} could not be downloaded ({error}); the bundler will try itself")
        return None
    actual = digest_of(path, algorithm)
    if actual != expected:
        os.remove(path)
        raise IntegrityError(
            f"{name} {algorithm} mismatch for {url}: got {actual}, "
            f"pinned {expected}. Nothing was installed from it."
        )
    log(f"{name}: {algorithm} verified")
    return path


def tools_dir():
    """Where the bundler keeps its tools: the `dirs` crate's cache_dir() + tauri."""
    if sys.platform == "win32":
        base = os.environ.get("LOCALAPPDATA")
    else:
        base = os.environ.get("XDG_CACHE_HOME")
        if not base or not os.path.isabs(base):
            home = os.environ.get("HOME")
            base = os.path.join(home, ".cache") if home else None
    if not base:
        raise SystemExit(
            "fetch-bundler-tools: cannot tell where the bundler keeps its tools"
        )
    return os.path.join(base, "tauri")


# The bundler's write_and_make_executable (bundle/linux/appimage/mod.rs) sets 0o770;
# the runner user both writes and runs these files, so owner-only is enough.
EXECUTABLE = 0o700


def install_file(source, dest, executable):
    """Copy `source` to `dest` unless the bundler already has one there."""
    if os.path.exists(dest):
        log(f"{dest} is already there")
        return
    os.makedirs(os.path.dirname(dest), exist_ok=True)
    tmp = dest + ".part"
    shutil.copyfile(source, tmp)
    if executable:
        os.chmod(tmp, EXECUTABLE)
    os.replace(tmp, dest)
    log(f"installed {dest}")


def unpack_zip(archive, dest, root=None):
    """Unpack `archive` to a new folder `dest`, built aside and renamed into place.

    With `root`, the archive's single top folder of that name becomes `dest`
    (nsis-3.13/ becomes NSIS/), the way the bundler does it.
    """
    parent = os.path.dirname(dest)
    os.makedirs(parent, exist_ok=True)
    stage = tempfile.mkdtemp(prefix=".stage-", dir=parent)
    try:
        with zipfile.ZipFile(archive) as z:
            z.extractall(stage)  # drops absolute and ".." parts of member names
        source = os.path.join(stage, root) if root else stage
        if not os.path.isdir(source):
            raise IntegrityError(f"{archive} has no {root}/ folder")
        os.replace(source, dest)
    finally:
        shutil.rmtree(stage, ignore_errors=True)
    log(f"unpacked {os.path.basename(archive)} into {dest}")


def linux(cache, tools):
    """Install AppRun, linuxdeploy and the AppImage plugin."""
    for entry in (LINUX_APPRUN, LINUX_LINUXDEPLOY):
        path = verified(cache, entry)
        if path:
            install_file(path, os.path.join(tools, entry[0]), executable=True)

    plugin = os.path.join(tools, LINUX_APPIMAGE_PLUGIN[0])
    if not os.path.exists(plugin):
        os.makedirs(tools, exist_ok=True)
        log(f"downloading {LINUX_APPIMAGE_PLUGIN[1]}")
        error = download(LINUX_APPIMAGE_PLUGIN[1], plugin)
        if error is None:
            os.chmod(plugin, EXECUTABLE)
        else:
            warn(
                f"the AppImage plugin could not be downloaded ({error}); "
                "linuxdeploy uses its built-in one"
            )


def windows(cache, tools):
    """Install WiX and NSIS (with its Tauri plugin)."""
    wix = verified(cache, WIX)
    wix_dir = os.path.join(tools, "WixTools314")
    if wix and not os.path.exists(wix_dir):
        unpack_zip(wix, wix_dir)

    nsis = verified(cache, NSIS)
    utils = verified(cache, NSIS_TAURI_UTILS)
    nsis_dir = os.path.join(tools, "NSIS")
    if nsis and not os.path.exists(nsis_dir):
        unpack_zip(nsis, nsis_dir, root="nsis-3.13")
    # Separate from the unpack: an NSIS folder the bundler or an earlier run
    # left without the plugin still gets it (install_file skips an existing one).
    if utils and os.path.isdir(nsis_dir):
        plugin_dir = os.path.join(nsis_dir, "Plugins", "x86-unicode", "additional")
        install_file(
            utils, os.path.join(plugin_dir, "nsis_tauri_utils.dll"), executable=False
        )


def main(argv):
    """Entry point; returns the process exit status."""
    if len(argv) != 2:
        raise SystemExit(f"usage: {argv[0]} <downloads-dir>")
    cache = os.path.abspath(argv[1])
    if sys.platform not in ("win32", "linux"):
        log(f"nothing to fetch on {sys.platform}: the bundlers there download no tools")
        return 0
    os.makedirs(cache, exist_ok=True)
    tools = tools_dir()
    log(f"bundler {TAURI_BUNDLER_VERSION} tools go in {tools}")
    try:
        (windows if sys.platform == "win32" else linux)(cache, tools)
    except IntegrityError as error:
        print(f"fetch-bundler-tools: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
