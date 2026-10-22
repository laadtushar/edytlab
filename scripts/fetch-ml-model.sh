#!/usr/bin/env bash
#
# Fetch a pinned speech model for the opt-in ML tests (#383, #384).
#
#   scripts/fetch-ml-model.sh <model-id> <outdir>
#
# <model-id> is the "id" of an entry in crates/ml-pipeline/models.json
# (whisper-base), which is the only place the repository, the commit, each
# file's URL, size and SHA-256 are written down. The script downloads every
# file of the entry into <outdir>/<path> (the same layout the app's model
# folder has), checks each one's size and SHA-256 before it is given its
# final name, and prints <outdir>'s absolute path, and only that, on stdout:
#
#   export WHISPER_MODEL_DIR="$(scripts/fetch-ml-model.sh whisper-base models/whisper-base)"
#
# This is a developer and CI script, run by hand or by .github/workflows/ml.yml.
# The app never runs it, and nothing the app ships downloads through it: the
# app downloads a model only when the user presses Download (#383,
# crates/ml-pipeline/tests/only_the_user_downloads.rs).
#
# It exits non-zero on a failed download or a size or digest that is not the
# manifest's, and a file that fails is deleted, never left under its name. A
# run that finds every file already there with the right size and digest
# downloads nothing, so CI can restore <outdir> from a cache keyed on the
# manifest's hash and still run the script to confirm what it restored.
#
# Needs bash, curl, python3 (or python), and sha256sum or shasum. Python reads
# the manifest, as in fetch-ml-runtime.sh.

set -euo pipefail

die() {
  echo "fetch-ml-model: $*" >&2
  exit 1
}

[[ $# -eq 2 ]] || die "usage: $0 <model-id> <outdir>   (model-id: whisper-base)"
model=$1
outdir=$2

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
manifest="$here/../crates/ml-pipeline/models.json"
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

# One line per file: path, url, size, sha256, tab-separated. A path that is
# absolute or climbs out of <outdir> is refused, as the app's own store does.
IFS= read -r -d '' PYSRC <<'PY' || true
import json, posixpath, sys

manifest, model = sys.argv[1], sys.argv[2]
with open(manifest, encoding="utf-8") as f:
    data = json.load(f)
entries = [m for m in data["models"] if m["id"] == model]
if not entries:
    sys.exit("fetch-ml-model: no model %r in the manifest; it has: %s"
             % (model, ", ".join(m["id"] for m in data["models"])))
for file in entries[0]["files"]:
    path = file["path"]
    norm = posixpath.normpath(path)
    if path.startswith("/") or "\\" in path or norm != path or norm.split("/")[0] == "..":
        sys.exit("fetch-ml-model: %r is not a relative path inside the model folder" % (path,))
    print("\t".join([path, file["url"], str(file["size"]), file["sha256"]]))
PY

files=$("$py" -I -c "$PYSRC" "$manifest" "$model" | tr -d '\r')
[[ -n $files ]] || die "the manifest lists no files for $model"

mkdir -p "$outdir"
outdir=$(cd "$outdir" && pwd)

# Whether $1 is on disk with the pinned size and digest.
verified() {
  local file=$1 size=$2 sha256=$3
  [[ -f $file ]] || return 1
  [[ $(wc -c <"$file" | tr -d '[:space:]') == "$size" ]] || return 1
  [[ $(sha256_of "$file") == "$sha256" ]]
}

fetched=0
while IFS=$'\t' read -r path url size sha256; do
  dest="$outdir/$path"
  if verified "$dest" "$size" "$sha256"; then
    echo "fetch-ml-model: $path is already here (sha256 $sha256)" >&2
    continue
  fi
  mkdir -p "$(dirname "$dest")"
  part="$dest.part"
  rm -f "$dest" "$part"
  echo "fetch-ml-model: downloading $url" >&2
  curl -fsSL --retry 3 -o "$part" "$url" || {
    rm -f "$part"
    die "download failed: $url"
  }
  actual_size=$(wc -c <"$part" | tr -d '[:space:]')
  if [[ $actual_size != "$size" ]]; then
    rm -f "$part"
    die "$url is $actual_size bytes, but the manifest says $size. Nothing was kept."
  fi
  actual_sha=$(sha256_of "$part")
  if [[ $actual_sha != "$sha256" ]]; then
    rm -f "$part"
    die "sha256 mismatch for $url: got $actual_sha, the manifest says $sha256. Nothing was kept."
  fi
  mv -f "$part" "$dest"
  fetched=$((fetched + 1))
done <<<"$files"

echo "fetch-ml-model: $model is in $outdir ($fetched file(s) downloaded)" >&2
echo "$outdir"
