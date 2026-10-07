#!/usr/bin/env bash
# Start a virtual display, the fake model server and tauri-driver, with a
# fresh HOME so the app starts as a first launch. Prints the HOME used.
#
# Stops only what it started, by PID: `pkill -f` matches any command line
# that merely mentions the name, including the caller's own shell.
set -euo pipefail
OUT=${OUT:-/tmp/edytlab-native}
mkdir -p "$OUT"
for f in "$OUT"/*.pid; do
  [ -e "$f" ] && kill "$(cat "$f")" 2>/dev/null || true
  rm -f "$f"
done
export E2E_HOME=${E2E_HOME:-$(mktemp -d /tmp/edytlab-home.XXXX)}
Xvfb :99 -screen 0 1440x900x24 >"$OUT/xvfb.log" 2>&1 &
echo $! >"$OUT/xvfb.pid"
sleep 1
# A window manager, so native dialogs (the GTK file chooser) get focus the
# way they do on a desktop.
DISPLAY=:99 openbox >"$OUT/openbox.log" 2>&1 &
echo $! >"$OUT/openbox.pid"
sleep 1
node "$(dirname "$0")/fake-llm.mjs" >"$OUT/fake-llm.log" 2>&1 &
echo $! >"$OUT/fake-llm.pid"
# A named session keyring per story: the app keeps its provider, keys and
# base URLs in the kernel keyring, which a fresh HOME does not reset. A
# restart within a story passes the same E2E_KEYRING to see them again.
E2E_KEYRING=${E2E_KEYRING:-e2e-$RANDOM$RANDOM}
HOME="$E2E_HOME" DISPLAY=:99 XDG_DATA_HOME="$E2E_HOME/.local/share" XDG_CONFIG_HOME="$E2E_HOME/.config" \
  WEBKIT_DISABLE_COMPOSITING_MODE=1 keyctl session "$E2E_KEYRING" tauri-driver >"$OUT/tauri-driver.log" 2>&1 &
echo $! >"$OUT/tauri-driver.pid"
sleep 2
echo "$E2E_HOME $E2E_KEYRING"
