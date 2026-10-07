#!/usr/bin/env bash
# Start a virtual display, the fake model server and tauri-driver, with a
# fresh HOME so the app starts as a first launch. Prints the HOME used.
#
# Stops only what it started, by PID: `pkill -f` matches any command line
# that merely mentions the name, including the caller's own shell.
set -euo pipefail
OUT=${OUT:-/tmp/edytlab-native}
mkdir -p "$OUT"
# Stop what a previous run started. By exact process name (`-x`), not by
# command line: `keyctl session` wraps tauri-driver, so killing its PID
# leaves the driver alive holding port 4444 and an old session, and
# `pkill -f` would match this shell too.
for name in edytlab-desktop WebKitWebDriver tauri-driver keyctl openbox Xvfb; do
  pkill -x "$name" 2>/dev/null || true
done
pkill -f "fake-llm.mjs" 2>/dev/null || true
# Wait for them to be gone, not just signalled: a new Xvfb started while
# the old one is still shutting down finds display :99 taken, exits, and
# then the old one takes the display with it.
for _ in $(seq 1 40); do
  pgrep -x "Xvfb|openbox|tauri-driver|WebKitWebDriver|edytlab-desktop" >/dev/null 2>&1 || pgrep -x Xvfb >/dev/null 2>&1 || break
  sleep 0.25
done
for name in Xvfb openbox tauri-driver WebKitWebDriver edytlab-desktop keyctl; do
  pkill -9 -x "$name" 2>/dev/null || true
done
sleep 0.5
rm -f /tmp/.X99-lock /tmp/.X11-unix/X99 "$OUT"/*.pid
for _ in $(seq 1 20); do
  (exec 3<>/dev/tcp/127.0.0.1/4444) 2>/dev/null || break
  sleep 0.25
done
export E2E_HOME=${E2E_HOME:-$(mktemp -d /tmp/edytlab-home.XXXX)}
Xvfb :99 -screen 0 1440x900x24 >"$OUT/xvfb.log" 2>&1 &
echo $! >"$OUT/xvfb.pid"
# Wait for the display to accept connections before anything uses it.
for _ in $(seq 1 40); do
  [ -S /tmp/.X11-unix/X99 ] && DISPLAY=:99 xdotool getdisplaygeometry >/dev/null 2>&1 && break
  sleep 0.25
done
DISPLAY=:99 xdotool getdisplaygeometry >/dev/null 2>&1 || { echo "Xvfb did not start" >&2; exit 1; }
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
