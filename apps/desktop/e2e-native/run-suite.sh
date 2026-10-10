#!/usr/bin/env bash
# Run the native suite inside one kernel session keyring, the way a
# desktop login does. The app's primary Linux store is the Secret
# Service, but run-env.sh points the session bus at nothing, so the app
# uses its kernel-keyring fallback (#394), which run-env.sh clears.
# Every process of the run (the driver, the app, a restarted app) must
# see the same session keyring for a restart to find its settings.
#
#   OUT=/tmp/edytlab-native ./run-suite.sh [story-id-substring ...]
set -euo pipefail
cd "$(dirname "$0")"
exec keyctl session edytlab-e2e node suite.mjs "$@"
