#!/usr/bin/env python3
"""The ONNX Runtime the release installers carry (#383).

    python3 -I scripts/installer-ml-runtime.py stage-vc-runtime <outdir>
    python3 -I scripts/installer-ml-runtime.py check [--signed] <platform> [--paths-json JSON] [<artifact>...]
    python3 -I scripts/installer-ml-runtime.py describe-dll <file.dll>

The release workflows fetch the pinned library and its notices with
scripts/fetch-ml-runtime.sh into apps/desktop/src-tauri/ml-runtime/<platform>
and build with apps/desktop/src-tauri/tauri.ml-runtime.<os>.conf.json, the
overlay that tells the bundler where each file goes. This script does the two
other steps.

stage-vc-runtime (Windows only) copies the Visual C++ runtime DLLs that
onnxruntime.dll imports (`vc_runtime.files` in ml-runtime.json) from the
build machine's Visual Studio into <outdir>, which the Windows overlay bundles
from apps/desktop/src-tauri/ml-runtime/vc-runtime-x64. It refuses a DLL older
than `vc_runtime.min_version`.

check opens each installer the way a user's machine would see it, without
installing anything, and fails unless:

  - every file the overlay adds is where the overlay says, byte for byte the
    file the build was given: the library (also against the manifest's
    `library_sha256`), the ONNX Runtime LICENSE and ThirdPartyNotices.txt and,
    on Windows, the Visual C++ runtime DLLs and their notice;
  - on Windows, every Visual C++ runtime DLL the installed onnxruntime.dll
    imports sits beside it, at `vc_runtime.min_version` or newer;
  - the app binary from that installer, run with --ml-runtime-status, finds
    that very library, reports the pinned version, and calls it bundled.

<platform> is a key of `platforms` in ml-runtime.json (linux-x64, win-x64,
osx-universal2). Each artifact is an installer the build produced: .deb and
.AppImage, .app (a directory), .app.tar.gz and .dmg, or .msi and the NSIS
-setup.exe. --paths-json takes tauri-action's `artifactPaths` output, a JSON
array of them, and adds the .app.tar.gz that tauri-action packs beside each
.app (the file it uploads). The two kinds each platform ships are required.
--signed is for a
macOS build that was signed: signing rewrites the dylib, so instead of its
bytes the check verifies its signature and that it is the app's team's (the
app's own run then shows the hardened runtime accepts it).

describe-dll prints a DLL's file version and the DLLs it imports.

Python 3.8 or newer, standard library only, run with -I so nothing in the
working directory can shadow a module. The tools each installer needs are the
runner's own: dpkg-deb (deb), the AppImage itself (--appimage-extract),
hdiutil, tar and codesign (macOS), msiexec and 7z (Windows).
"""

import argparse
import hashlib
import json
import os
import plistlib
import re
import shutil
import struct
import subprocess
import sys
import tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
SRC_TAURI = REPO / "apps" / "desktop" / "src-tauri"
MANIFEST = SRC_TAURI / "ml-runtime.json"

OVERLAYS = {
    "linux-x64": "tauri.ml-runtime.linux.conf.json",
    "osx-universal2": "tauri.ml-runtime.macos.conf.json",
    "win-x64": "tauri.ml-runtime.windows.conf.json",
}

# The two installer kinds each platform ships; both must be checked.
REQUIRED_KINDS = {
    "linux-x64": {"deb", "appimage"},
    "osx-universal2": {"app", "dmg"},
    "win-x64": {"msi", "nsis"},
}

# The app's own flag: apps/desktop/src-tauri/src/lib.rs, ML_RUNTIME_STATUS_ARG.
STATUS_ARG = "--ml-runtime-status"
STATUS_TIMEOUT = 120

# Imports that are the Visual C++ runtime, as opposed to Windows itself.
VC_RUNTIME_IMPORT = re.compile(r"^(msvcp|vcruntime|concrt|vccorlib|vcomp)\d.*\.dll$", re.I)


def die(message):
    sys.exit("installer-ml-runtime: " + message)


def load_json(path):
    with open(path, encoding="utf-8") as f:
        return json.load(f)


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def run(argv, **kwargs):
    """subprocess.run with the command echoed, failing on a non-zero exit."""
    print("  $ " + " ".join(str(a) for a in argv), flush=True)
    return subprocess.run([str(a) for a in argv], check=True, **kwargs)


# -- PE files ---------------------------------------------------------------


def _pe_sections(data):
    if data[:2] != b"MZ":
        raise ValueError("not a PE file")
    pe = struct.unpack_from("<I", data, 0x3C)[0]
    if data[pe:pe + 4] != b"PE\0\0":
        raise ValueError("not a PE file")
    count, = struct.unpack_from("<H", data, pe + 6)
    optional_size, = struct.unpack_from("<H", data, pe + 20)
    optional = pe + 24
    magic, = struct.unpack_from("<H", data, optional)
    directories = optional + (112 if magic == 0x20B else 96)
    sections = []
    table = optional + optional_size
    for i in range(count):
        name, vsize, va, raw_size, raw_ptr = struct.unpack_from("<8sIIII", data, table + 40 * i)
        sections.append((name.rstrip(b"\0"), va, max(vsize, raw_size), raw_ptr, raw_size))
    return directories, sections


def _offset(sections, rva):
    for _, va, size, raw_ptr, _ in sections:
        if va <= rva < va + size:
            return rva - va + raw_ptr
    raise ValueError("RVA %#x is in no section" % rva)


def _c_string(data, offset):
    return data[offset:data.index(b"\0", offset)].decode("ascii")


def pe_imports(path):
    """The DLLs a PE file imports, delay-loaded ones included."""
    data = Path(path).read_bytes()
    directories, sections = _pe_sections(data)
    names = []
    rva, _ = struct.unpack_from("<II", data, directories + 8 * 1)
    if rva:
        p = _offset(sections, rva)
        while True:
            name_rva = struct.unpack_from("<5I", data, p)[3]
            if name_rva == 0:
                break
            names.append(_c_string(data, _offset(sections, name_rva)))
            p += 20
    rva, _ = struct.unpack_from("<II", data, directories + 8 * 13)
    if rva:
        p = _offset(sections, rva)
        while True:
            name_rva = struct.unpack_from("<8I", data, p)[1]
            if name_rva == 0:
                break
            names.append(_c_string(data, _offset(sections, name_rva)))
            p += 32
    return names


def pe_file_version(path):
    """The file version from a PE file's VS_FIXEDFILEINFO, as a 4-tuple."""
    data = Path(path).read_bytes()
    _, sections = _pe_sections(data)
    signature = struct.pack("<I", 0xFEEF04BD)
    for name, _, _, raw_ptr, raw_size in sections:
        if name != b".rsrc":
            continue
        at = data.find(signature, raw_ptr, raw_ptr + raw_size)
        if at >= 0:
            ms, ls = struct.unpack_from("<II", data, at + 8)
            return (ms >> 16, ms & 0xFFFF, ls >> 16, ls & 0xFFFF)
    raise ValueError("%s has no version resource" % path)


def version_text(version):
    return ".".join(str(part) for part in version)


def min_version(text):
    return tuple(int(part) for part in text.split("."))


# -- what the build should have put in each installer -----------------------


class Expected:
    """What the overlay and the manifest say an installer for one platform holds."""

    def __init__(self, platform):
        manifest = load_json(MANIFEST)
        if platform not in manifest["platforms"]:
            die("no platform %r in %s" % (platform, MANIFEST))
        entry = manifest["platforms"][platform]
        overlay = load_json(SRC_TAURI / OVERLAYS[platform])
        bundle = overlay["bundle"]
        conf = load_json(SRC_TAURI / "tauri.conf.json")

        self.platform = platform
        self.version = manifest["version"]
        self.library = entry["install_as"]
        self.library_sha256 = entry["library_sha256"]
        self.vc_runtime = entry.get("vc_runtime")
        self.product = conf["productName"]
        self.binary = conf.get("mainBinaryName") or cargo_package_name()
        # Source (as the build was given it) -> path inside the resource dir.
        self.resources = [(SRC_TAURI / src, Path(dst)) for src, dst in bundle.get("resources", {}).items()]
        # Sources that go into Contents/Frameworks under their own names.
        self.frameworks = [SRC_TAURI / src for src in bundle.get("macOS", {}).get("frameworks", [])]

        sources = [src for src, _ in self.resources] + self.frameworks
        missing = [str(src) for src in sources if not src.is_file()]
        if missing:
            die("the files the build bundles are not here (run scripts/fetch-ml-runtime.sh, and on "
                "Windows stage-vc-runtime, first): " + ", ".join(missing))
        staged_library = [src for src in sources if src.name == self.library]
        if len(staged_library) != 1:
            die("%s should bundle %s exactly once" % (OVERLAYS[platform], self.library))
        if sha256(staged_library[0]) != self.library_sha256:
            die("%s is not the library ml-runtime.json pins (library_sha256)" % staged_library[0])


def cargo_package_name():
    text = (SRC_TAURI / "Cargo.toml").read_text(encoding="utf-8")
    match = re.search(r'^\[package\]\s*$(.*?)^\[', text, re.S | re.M)
    name = match and re.search(r'^name\s*=\s*"([^"]+)"', match.group(1), re.M)
    if not name:
        die("cannot read the package name from %s" % (SRC_TAURI / "Cargo.toml"))
    return name.group(1)


# -- opening each installer -------------------------------------------------


class Installed:
    """One installer, unpacked: the binary to run and where its files are."""

    def __init__(self, artifact, kind, root, argv, resource_dir, frameworks_dir=None, app=None):
        self.artifact = artifact
        # The top of the unpacked tree, which messages name paths from.
        self.root = root
        self.kind = kind
        self.argv = argv
        self.resource_dir = resource_dir
        self.frameworks_dir = frameworks_dir
        self.app = app


def kind_of(path):
    name = path.name
    if name.endswith(".app.tar.gz"):
        return "app.tar.gz"
    if name.endswith(".app") and path.is_dir():
        return "app"
    for suffix, kind in ((".dmg", "dmg"), (".deb", "deb"), (".AppImage", "appimage"), (".msi", "msi"), (".exe", "nsis")):
        if name.endswith(suffix) and path.is_file():
            return kind
    return None


def only(paths, what):
    paths = sorted(paths)
    if len(paths) != 1:
        raise CheckFailed("expected one %s, found %d: %s" % (what, len(paths), ", ".join(map(str, paths)) or "none"))
    return paths[0]


class CheckFailed(Exception):
    pass


def open_linux(artifact, kind, work, expected):
    if kind == "deb":
        root = work / "data"
        run(["dpkg-deb", "-x", artifact, root])
        exe = root / "usr" / "bin" / expected.binary
        argv = [exe]
    else:
        copy = work / artifact.name
        shutil.copy2(artifact, copy)
        copy.chmod(0o755)
        run([copy, "--appimage-extract"], cwd=work, stdout=subprocess.DEVNULL)
        root = work / "squashfs-root"
        exe = root / "usr" / "bin" / expected.binary
        # AppRun is what a user's launch goes through.
        argv = [root / "AppRun"] if (root / "AppRun").exists() else [exe]
    if not exe.is_file():
        raise CheckFailed("no %s in usr/bin" % expected.binary)
    return Installed(artifact, kind, root, argv, root / "usr" / "lib" / expected.product)


def open_app(artifact, kind, app):
    contents = app / "Contents"
    with open(contents / "Info.plist", "rb") as f:
        executable = plistlib.load(f)["CFBundleExecutable"]
    exe = contents / "MacOS" / executable
    if not exe.is_file():
        raise CheckFailed("no Contents/MacOS/%s" % executable)
    return Installed(artifact, kind, app.parent, [exe], contents / "Resources", contents / "Frameworks", app)


def open_macos(artifact, kind, work, expected, cleanups):
    if kind == "app":
        return open_app(artifact, kind, artifact)
    if kind == "app.tar.gz":
        run(["tar", "-xzf", artifact, "-C", work])
        return open_app(artifact, kind, only(work.glob("*.app"), ".app in the archive"))
    mount = work / "mnt"
    mount.mkdir()
    run(["hdiutil", "attach", "-nobrowse", "-readonly", "-noautoopen", "-mountpoint", mount, artifact],
        stdout=subprocess.DEVNULL)
    cleanups.append(["hdiutil", "detach", mount, "-force"])
    return open_app(artifact, kind, only(mount.glob("*.app"), ".app in the disk image"))


def open_windows(artifact, kind, work, expected):
    out = work / kind
    if kind == "msi":
        # An administrative install: the files laid out as the MSI would
        # install them, with nothing registered on this machine.
        run(["msiexec", "/a", artifact, "/qn", "TARGETDIR=%s" % out, "/l*v", work / "msiexec.log"])
    else:
        run(["7z", "x", "-y", "-o%s" % out, artifact], stdout=subprocess.DEVNULL)
    exe = only(out.rglob(expected.binary + ".exe"), expected.binary + ".exe in the installer")
    # The install folder is the resource directory, and the exe is in it.
    return Installed(artifact, kind, out, [exe], exe.parent)


# -- the checks -------------------------------------------------------------


def codesign_team(path):
    result = subprocess.run(["codesign", "-dv", "--verbose=2", str(path)], capture_output=True, text=True)
    match = re.search(r"^TeamIdentifier=(.+)$", result.stderr, re.M)
    return match.group(1).strip() if match else None


def check_installed(installed, expected, signed):
    """Every failure for one unpacked installer (empty when it passes)."""
    failures = []

    def fail(message):
        failures.append(message)
        print("    FAIL " + message, flush=True)

    def ok(message):
        print("    ok   " + message, flush=True)

    def shown(path):
        try:
            return str(Path(path).relative_to(installed.root))
        except ValueError:
            return str(path)

    library_path = None
    library_is_framework = False

    for src, dst in expected.resources:
        installed_file = installed.resource_dir / dst
        if src.name == expected.library:
            library_path = installed_file
        if not installed_file.is_file():
            renamed = installed_file.parent / src.name
            if renamed != installed_file and renamed.is_file():
                fail("%s is missing; the installer has it as %s, under the source's name (an MSI does not "
                     "rename a resource)" % (shown(installed_file), shown(renamed)))
            else:
                fail("%s is missing (the overlay puts %s there)" % (shown(installed_file), src.name))
        elif sha256(installed_file) != sha256(src):
            fail("%s is not the file the build was given (%s)" % (shown(installed_file), src))
        else:
            ok(shown(installed_file))

    for src in expected.frameworks:
        if installed.frameworks_dir is None:
            fail("%s has no Frameworks directory" % installed.artifact.name)
            continue
        installed_file = installed.frameworks_dir / src.name
        if src.name == expected.library:
            library_path = installed_file
            library_is_framework = True
        if not installed_file.is_file():
            fail("%s is missing (bundle.macOS.frameworks)" % shown(installed_file))
        elif signed:
            verify = subprocess.run(["codesign", "--verify", "--strict", "--verbose=2", str(installed_file)],
                                    capture_output=True, text=True)
            team, app_team = codesign_team(installed_file), codesign_team(installed.app)
            if verify.returncode != 0:
                fail("%s does not verify: %s" % (shown(installed_file), verify.stderr.strip()))
            elif not team or team == "not set" or team != app_team:
                fail("%s is signed by team %r, the app by %r" % (shown(installed_file), team, app_team))
            else:
                ok("%s (signed, team %s)" % (shown(installed_file), team))
        elif sha256(installed_file) != sha256(src):
            fail("%s is not the file the build was given (%s)" % (shown(installed_file), src))
        else:
            ok(shown(installed_file))

    if library_path is None:
        fail("the overlay does not bundle %s" % expected.library)
        return failures
    # Signing rewrites a framework, and only a framework; see --signed.
    rewritten = signed and library_is_framework
    if library_path.is_file() and not rewritten and sha256(library_path) != expected.library_sha256:
        fail("%s does not have the manifest's library_sha256" % shown(library_path))

    if installed.frameworks_dir is not None and library_path.is_file():
        archs = subprocess.run(["lipo", "-archs", str(library_path)], capture_output=True, text=True)
        if archs.returncode != 0 or not {"x86_64", "arm64"} <= set(archs.stdout.split()):
            fail("%s should be universal (x86_64 and arm64), lipo says %r" % (shown(library_path),
                                                                              archs.stdout.strip()))

    if expected.vc_runtime and library_path.is_file():
        beside = {p.name.lower(): p for p in library_path.parent.iterdir()}
        floor = min_version(expected.vc_runtime["min_version"])
        imported = [name for name in pe_imports(library_path) if VC_RUNTIME_IMPORT.match(name)]
        for name in imported:
            if name.lower() not in beside:
                fail("%s imports %s, which is not beside it" % (library_path.name, name))
        for name in expected.vc_runtime["files"]:
            path = beside.get(name.lower())
            if path is None:
                continue  # reported above, or by the resource check
            version = pe_file_version(path)
            if version[:len(floor)] < floor:
                fail("%s is %s, older than %s" % (name, version_text(version), expected.vc_runtime["min_version"]))
            else:
                ok("%s %s" % (name, version_text(version)))
        ok("%s imports %s from beside it" % (library_path.name, ", ".join(imported) or "nothing"))

    # The app, from this installer's own layout, finds the library and calls
    # it bundled: what Settings would say.
    env = {k: v for k, v in os.environ.items() if k != "ORT_DYLIB_PATH"}
    try:
        result = subprocess.run([str(a) for a in installed.argv] + [STATUS_ARG], env=env, capture_output=True,
                                text=True, timeout=STATUS_TIMEOUT, stdin=subprocess.DEVNULL)
    except subprocess.TimeoutExpired:
        fail("%s %s did not exit in %d s" % (shown(installed.argv[-1]), STATUS_ARG, STATUS_TIMEOUT))
        return failures
    lines = result.stdout.strip().splitlines()
    try:
        status = json.loads(lines[-1]) if lines else None
    except ValueError:
        status = None
    if result.returncode != 0 or not isinstance(status, dict):
        fail("%s %s exited %d; stdout %r; stderr %r" % (shown(installed.argv[-1]), STATUS_ARG, result.returncode,
                                                         result.stdout[-2000:], result.stderr[-2000:]))
        return failures
    print("    the app says: " + json.dumps(status), flush=True)
    if status.get("state") != "found":
        fail("the app did not load the library: %s" % json.dumps(status))
    else:
        if status.get("source") != "bundled":
            fail("the app calls the library %r, not bundled" % status.get("source"))
        if status.get("version") != expected.version:
            fail("the app loaded ONNX Runtime %s, not %s" % (status.get("version"), expected.version))
        loaded = Path(status.get("path", ""))
        if not (loaded.exists() and library_path.exists() and os.path.samefile(loaded, library_path)):
            fail("the app loaded %s, not the installer's %s" % (loaded, shown(library_path)))
        if not failures:
            ok("--ml-runtime-status: %s %s, %s" % (status["version"], status["source"], shown(loaded)))
    return failures


def check(platform, artifacts, signed):
    expected = Expected(platform)
    seen = set()
    results = []
    for artifact in artifacts:
        artifact = Path(artifact).resolve()
        kind = kind_of(artifact)
        if kind is None:
            die("%s is not an installer this check knows (.deb, .AppImage, .app, .app.tar.gz, .dmg, .msi, .exe)"
                % artifact)
        seen.add(kind)
        print("\n== %s (%s)" % (artifact.name, kind), flush=True)
        work = Path(tempfile.mkdtemp(prefix="ml-runtime-check-", dir=os.environ.get("RUNNER_TEMP")))
        cleanups = []
        try:
            if platform == "linux-x64":
                installed = open_linux(artifact, kind, work, expected)
            elif platform == "osx-universal2":
                installed = open_macos(artifact, kind, work, expected, cleanups)
            else:
                installed = open_windows(artifact, kind, work, expected)
            failures = check_installed(installed, expected, signed)
        except (CheckFailed, subprocess.CalledProcessError, OSError, ValueError) as e:
            failures = ["could not open it: %s" % e]
            print("    FAIL " + failures[0], flush=True)
        finally:
            for argv in cleanups:
                subprocess.run([str(a) for a in argv], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            shutil.rmtree(work, ignore_errors=True)
        results.append((artifact.name, failures))

    missing = REQUIRED_KINDS[platform] - seen
    print("\n== summary for %s (ONNX Runtime %s)" % (platform, expected.version))
    failed = False
    for name, failures in results:
        print("  %s %s" % ("FAIL" if failures else "ok  ", name))
        for failure in failures:
            print("::error title=ONNX Runtime in %s::%s" % (name, failure))
            failed = True
    for kind in sorted(missing):
        print("::error title=ONNX Runtime in the installers::no %s among the artifacts checked" % kind)
        failed = True
    if failed:
        sys.exit(1)


# -- the Visual C++ runtime -------------------------------------------------


def visual_studio_crt_dir():
    root = Path(os.environ.get("ProgramFiles(x86)", r"C:\Program Files (x86)"))
    vswhere = root / "Microsoft Visual Studio" / "Installer" / "vswhere.exe"
    if not vswhere.is_file():
        die("no vswhere.exe at %s: is Visual Studio with the C++ tools installed?" % vswhere)
    found = subprocess.run([str(vswhere), "-latest", "-products", "*", "-requires",
                            "Microsoft.VisualStudio.Component.VC.Tools.x86.x64", "-property", "installationPath"],
                           capture_output=True, text=True, check=True).stdout.strip().splitlines()
    if not found:
        die("vswhere found no Visual Studio with the C++ x64 tools")
    vc = Path(found[0]) / "VC"
    default = vc / "Auxiliary" / "Build" / "Microsoft.VCRedistVersion.default.txt"
    if default.is_file():
        version = default.read_text(encoding="utf-8").strip()
    else:
        versions = [p.name for p in (vc / "Redist" / "MSVC").iterdir() if re.match(r"^\d+(\.\d+)+$", p.name)]
        if not versions:
            die("no redistributable under %s" % (vc / "Redist" / "MSVC"))
        version = max(versions, key=min_version)
    return only((vc / "Redist" / "MSVC" / version / "x64").glob("Microsoft.VC*.CRT"), "x64 CRT folder")


def stage_vc_runtime(outdir):
    manifest = load_json(MANIFEST)
    vc = manifest["platforms"]["win-x64"]["vc_runtime"]
    floor = min_version(vc["min_version"])
    crt = visual_studio_crt_dir()
    print("installer-ml-runtime: the Visual C++ runtime from %s" % crt)
    outdir = Path(outdir)
    outdir.mkdir(parents=True, exist_ok=True)
    for name in vc["files"]:
        src = crt / name
        if not src.is_file():
            die("%s has no %s" % (crt, name))
        version = pe_file_version(src)
        if version[:len(floor)] < floor:
            die("%s is %s; ONNX Runtime needs %s or newer" % (src, version_text(version), vc["min_version"]))
        shutil.copy2(src, outdir / name)
        print("  %s %s sha256 %s" % (name, version_text(version), sha256(outdir / name)))


def describe_dll(path):
    print("file version: %s" % version_text(pe_file_version(path)))
    for name in pe_imports(path):
        print("imports: %s" % name)


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    sub = parser.add_subparsers(dest="command", required=True)
    p = sub.add_parser("stage-vc-runtime")
    p.add_argument("outdir")
    p = sub.add_parser("check")
    p.add_argument("--signed", action="store_true")
    p.add_argument("platform", choices=sorted(OVERLAYS))
    p.add_argument("--paths-json", help="tauri-action's artifactPaths output")
    p.add_argument("artifacts", nargs="*")
    p = sub.add_parser("describe-dll")
    p.add_argument("path")
    args = parser.parse_args()
    if args.command == "stage-vc-runtime":
        stage_vc_runtime(args.outdir)
    elif args.command == "check":
        artifacts = list(args.artifacts)
        if args.paths_json:
            for path in json.loads(args.paths_json):
                artifacts.append(path)
                packed = path + ".tar.gz"
                if path.endswith(".app") and Path(packed).is_file():
                    artifacts.append(packed)
        if not artifacts:
            die("no installers to check")
        check(args.platform, artifacts, args.signed)
    else:
        describe_dll(args.path)


if __name__ == "__main__":
    main()
