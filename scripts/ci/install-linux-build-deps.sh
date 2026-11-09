#!/usr/bin/env bash
# Installs the system packages a Linux Tauri build needs (GTK, WebKit,
# AppIndicator and friends) on a GitHub-hosted Ubuntu runner.
#
#   bash scripts/ci/install-linux-build-deps.sh [--recommends] [extra-package ...]
#
# Run by the "Install Linux build deps" step of ci.yml, coverage.yml,
# release-dev.yml and release-signed.yml. The release builds pass
# `--recommends libfuse2 patchelf` (the AppImage bundle). Each step's
# `timeout-minutes` has to hold this script's worst case, and
# crates/tools/tests/apt_retry_budget.rs checks that against the numbers here.
#
# Why it is shaped like this:
#
# - The runner image points apt at azure.archive.ubuntu.com, which stalled
#   mid-index-fetch on four runs (#660, #661, #669): `Ign:` on every azure
#   line, then silence. The canonical archive answered in the same logs, so
#   the mirror is rewritten away. Plain http to archive.ubuntu.com then
#   stalled too (2026-11-09: each .deb hung 35-40 s before an `Ign:`, and
#   #979's install ran out of its time, exit 124) while https answered at
#   once, so the rewrite also moves it to https. apt verifies every index and
#   .deb against the archive's signing keys whatever the transport.
#
# - A step's `timeout-minutes` kills the whole step, so a retry needs its
#   timeout on the command (the first attempt at this bounded the step with
#   `timeout-minutes: 6`, and its retry never ran). `Acquire::*::Timeout`
#   keeps one unresponsive host from eating an attempt on its own.
#
# - Only the network is put under a timeout. `apt-get install` under
#   `timeout 300` was killed mid-unpack when a slow download had used the
#   time up, which left dpkg's journal pending, and the retry then stopped
#   at once: "E: dpkg was interrupted, you must manually run 'sudo dpkg
#   --configure -a'", exit 100 (#957's Linux leg at 332c6a5). So the attempts
#   only download (`--download-only`: "package files are only retrieved, not
#   unpacked or installed"), and the install afterwards runs from those
#   archives with `--no-download` ("Disables downloading of packages"); both
#   from apt-get(8), https://manpages.ubuntu.com/manpages/jammy/man8/apt-get.8.html.
#   The install has no network to wait for. It took 8-21 s on a runner (12
#   successful ci.yml and release-dev.yml runs, 2026-11-09), so its
#   `timeout 120` is reached only by a hang, and then fails the step in two
#   minutes rather than at the step's cap. Without `--ignore-missing`, an
#   archive that is somehow missing fails the install instead of quietly
#   leaving the package out.
set -euo pipefail

recommends=(--no-install-recommends)
if [[ "${1:-}" == "--recommends" ]]; then
  recommends=()
  shift
fi
packages=(
  libwebkit2gtk-4.1-dev
  libgtk-3-dev
  libayatana-appindicator3-dev
  librsvg2-dev
  libssl-dev
  libasound2-dev
  libdbus-1-dev
  pkg-config
  "$@"
)

# The runner reads its mirrors from /etc/apt/apt-mirrors.txt (sources say
# `mirror+file:/etc/apt/apt-mirrors.txt`), and newer images keep the sources
# in deb822 `*.sources` files, so the rewrite covers all three: rewriting
# only `*.list` left apt on the Azure mirror (#660, #661, #669).
sudo sed -i 's#https\?://\(azure\.\)\?archive\.ubuntu\.com/ubuntu#https://archive.ubuntu.com/ubuntu#g' \
  /etc/apt/apt-mirrors.txt /etc/apt/sources.list \
  /etc/apt/sources.list.d/*.list /etc/apt/sources.list.d/*.sources 2>/dev/null || true
grep -h -r azure /etc/apt/apt-mirrors.txt /etc/apt/sources.list /etc/apt/sources.list.d/ 2>/dev/null \
  && echo "::warning::apt still names an Azure mirror" || true

# One attempt: the index, then every archive the install needs. A failed or
# timed-out `update` does not end the attempt (the download still runs, with
# whatever lists there are), and it is said so here rather than left to
# `errexit`, which is suspended for the first attempt (left of `||`) but not
# for the retry. So one attempt can take 150 + 300 s, and the download's
# result is the attempt's.
fetch() {
  sudo timeout 150 apt-get \
    -o Acquire::Retries=3 \
    -o Acquire::http::Timeout=15 \
    -o Acquire::https::Timeout=15 \
    update || echo "::warning::apt-get update failed (exit $?); downloading with the lists there are"
  sudo timeout 300 apt-get \
    -o Acquire::Retries=3 \
    -o Acquire::http::Timeout=15 \
    -o Acquire::https::Timeout=15 \
    install -y --download-only "${recommends[@]}" "${packages[@]}"
}
fetch || {
  echo "::warning::apt download failed once — retrying on a fresh connection"
  sleep 10
  fetch
}

# Local only: unpack and configure what was downloaded. Noninteractive, so
# no debconf question can sit waiting for input.
sudo DEBIAN_FRONTEND=noninteractive timeout 120 apt-get install -y --no-download \
  "${recommends[@]}" "${packages[@]}"
