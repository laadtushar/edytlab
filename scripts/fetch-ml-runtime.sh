#!/usr/bin/env bash
#
# Fetch the pinned ONNX Runtime for one platform (#383).
#
#   scripts/fetch-ml-runtime.sh <platform> <outdir>
#
# <platform> is a key of "platforms" in apps/desktop/src-tauri/ml-runtime.json
# (linux-x64, win-x64 or osx-universal2), which is the only place the version,
# URL and SHA-256 are written down. The script downloads that archive, checks
# its size and SHA-256, and unpacks into <outdir> only the library, renamed to
# the name the app loads (libonnxruntime.so, onnxruntime.dll or
# libonnxruntime.dylib), and the licence notices the manifest lists. Nothing
# else in the archive is written (the Windows one holds a 378 MB .pdb). The
# unpacked library must match the manifest's `library_sha256` too, the digest
# the per-installer check (scripts/installer-ml-runtime.py) looks for.
#
# It exits non-zero, and leaves nothing in <outdir>, on a failed download, a
# size or digest that is not the manifest's, an archive without the library,
# or a library whose digest is not `library_sha256`. On success it prints the
# library's absolute path, and only that, on stdout:
#
#   export ORT_DYLIB_PATH="$(scripts/fetch-ml-runtime.sh linux-x64 ml-runtime/linux-x64)"
#
# A run that finds <outdir> already holding this manifest entry's result does
# nothing, so CI can restore <outdir> from a cache keyed on the manifest's hash
# and still run the script to confirm what it restored. It records the
# archive's digest and the library's own in <outdir>/.fetched, and a changed
# manifest or a library that no longer matches `library_sha256` fetches
# again.
#
# Needs bash, curl, python3 (or python), and sha256sum or shasum. Python reads
# the manifest and does the unpacking, so the same code handles the .tgz and
# the .zip on all three CI runners: `unzip` is not on every Windows shell, and
# `tar` differs between GNU and macOS in how it matches the "./" some
# archives put in front of every name.

set -euo pipefail

die() {
  echo "fetch-ml-runtime: $*" >&2
  exit 1
}

[[ $# -eq 2 ]] || die "usage: $0 <platform> <outdir>   (platform: linux-x64 | win-x64 | osx-universal2)"
platform=$1
outdir=$2

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
manifest="$here/../apps/desktop/src-tauri/ml-runtime.json"
[[ -f $manifest ]] || die "no manifest at $manifest"

py=
for candidate in python3 python; do
  if command -v "$candidate" >/dev/null 2>&1 && "$candidate" -I -c 'import sys' >/dev/null 2>&1; then
    py=$candidate
    break
  fi
done
[[ -n $py ]] || die "python3 is needed to read $manifest"

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d' ' -f1
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | cut -d' ' -f1
  else
    die "neither sha256sum nor shasum is installed"
  fi
}

# `python -I` puts neither the script's directory nor the working directory
# on sys.path, so nothing in either can shadow a standard module.
IFS= read -r -d '' PYSRC <<'PY' || true
import json, os, posixpath, shutil, sys, tarfile, zipfile


def die(message):
    sys.exit("fetch-ml-runtime: " + message)


def norm(name):
    # Some archives name every entry "./dir/file", others "dir/file".
    return posixpath.normpath(name.replace("\\", "/"))


def bare(name):
    if not name or name in (".", "..") or "/" in name or "\\" in name:
        die("%r is not a bare file name" % (name,))
    return name


def entry(manifest, platform):
    with open(manifest, encoding="utf-8") as f:
        data = json.load(f)
    platforms = data["platforms"]
    if platform not in platforms:
        die("no platform %r in the manifest; it has: %s" % (platform, ", ".join(sorted(platforms))))
    return data, platforms[platform]


def wanted(data, p):
    # The library, then each notice from the archive's top directory.
    member = norm(p["member"])
    root = member.split("/")[0]
    names = {member: bare(p["install_as"])}
    for notice in data["notices"]:
        names[root + "/" + bare(notice)] = notice
    return names


def extract(manifest, platform, archive, stage):
    data, p = entry(manifest, platform)
    names = wanted(data, p)
    library = norm(p["member"])
    found = set()

    def save(name, reader):
        dest = os.path.join(stage, names[name])
        with open(dest, "wb") as out:
            shutil.copyfileobj(reader, out)
        os.chmod(dest, 0o755 if name == library else 0o644)
        found.add(name)

    if archive.endswith(".zip"):
        with zipfile.ZipFile(archive) as z:
            for info in z.infolist():
                name = norm(info.filename)
                if name in names and name not in found and not info.is_dir():
                    with z.open(info) as reader:
                        save(name, reader)
    else:
        with tarfile.open(archive, "r:gz") as t:
            for info in t:
                name = norm(info.name)
                if name in names and name not in found:
                    if not info.isfile():
                        die("%s is not a regular file in the archive" % name)
                    reader = t.extractfile(info)
                    save(name, reader)
                    reader.close()
                if len(found) == len(names):
                    break  # the rest of the archive is not needed

    missing = sorted(set(names) - found)
    if missing:
        die("the archive does not contain: " + ", ".join(missing))


command = sys.argv[1]
if command == "fields":
    _, p = entry(sys.argv[2], sys.argv[3])
    print("\t".join([p["url"], p["sha256"], str(p["size"]), bare(p["install_as"]), p["library_sha256"]]))
elif command == "notices":
    data, _ = entry(sys.argv[2], sys.argv[3])
    print("\n".join(bare(n) for n in data["notices"]))
elif command == "extract":
    extract(*sys.argv[2:6])
else:
    die("unknown command " + command)
PY

# The `tr` drops the carriage return Python writes on Windows.
fields=$("$py" -I -c "$PYSRC" fields "$manifest" "$platform" | tr -d '\r')
IFS=$'\t' read -r url sha256 size install_as library_sha256 <<<"$fields"
notices=$("$py" -I -c "$PYSRC" notices "$manifest" "$platform" | tr -d '\r')

mkdir -p "$outdir"
outdir=$(cd "$outdir" && pwd)
library="$outdir/$install_as"
stamp="$outdir/.fetched"

complete() {
  local name
  [[ -f $library && -f $stamp ]] || return 1
  while IFS= read -r name; do
    [[ -f $outdir/$name ]] || return 1
  done <<<"$notices"
  [[ $(cat "$stamp") == "$sha256 $library_sha256" && $(sha256_of "$library") == "$library_sha256" ]]
}

if complete; then
  echo "fetch-ml-runtime: $platform is already in $outdir (archive sha256 $sha256)" >&2
  echo "$library"
  exit 0
fi

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
archive="$work/${url##*/}"

echo "fetch-ml-runtime: downloading $url" >&2
curl -fsSL --retry 3 -o "$archive" "$url" || die "download failed: $url"

actual_size=$(wc -c <"$archive" | tr -d '[:space:]')
[[ $actual_size == "$size" ]] ||
  die "$url is $actual_size bytes, but the manifest says $size. Nothing was unpacked."
actual_sha=$(sha256_of "$archive")
[[ $actual_sha == "$sha256" ]] ||
  die "sha256 mismatch for $url: got $actual_sha, the manifest says $sha256. Nothing was unpacked."

stage="$work/stage"
mkdir "$stage"
"$py" -I -c "$PYSRC" extract "$manifest" "$platform" "$archive" "$stage"
actual_library_sha=$(sha256_of "$stage/$install_as")
[[ $actual_library_sha == "$library_sha256" ]] ||
  die "sha256 mismatch for $install_as unpacked from $url: got $actual_library_sha, the manifest says $library_sha256. Nothing was unpacked."

# Only a verified archive's files reach <outdir>, and the stamp goes last.
rm -f "$stamp"
for file in "$stage"/*; do
  mv -f "$file" "$outdir/"
done
echo "$sha256 $library_sha256" >"$stamp"

echo "fetch-ml-runtime: installed $install_as ($(wc -c <"$library" | tr -d '[:space:]') bytes) and $(echo "$notices" | tr '\n' ' ')into $outdir" >&2
echo "$library"
