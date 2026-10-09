#!/usr/bin/env bash
# Run the native suite inside one kernel session keyring, the way a
# desktop login does: the app keeps its provider and keys in the kernel
# keyring, and every process of the run (the driver, the app, a restarted
# app) must see the same one for a restart to find its settings.
#
#   OUT=/tmp/edytlab-native ./run-suite.sh [story-id-substring ...]
set -euo pipefail
cd "$(dirname "$0")"
exec keyctl session edytlab-e2e node suite.mjs "$@"
