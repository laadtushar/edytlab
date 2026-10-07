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
# command line: `pkill -f` would match this shell too.
for name in edytlab-desktop WebKitWebDriver tauri-driver openbox Xvfb; do
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
for name in Xvfb openbox tauri-driver WebKitWebDriver edytlab-desktop; do
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
# The app keeps its provider, keys and base URLs through the `keyring`
# crate, which on Linux uses the user's *persistent* kernel keyring: it
# outlives sessions, so neither a fresh HOME nor a session keyring
# isolates a run. Clear it for a first launch; a restart within a story
# passes KEEP_KEYRING=1 to find its settings again.
if [ "${KEEP_KEYRING:-0}" != 1 ]; then
  keyctl clear "$(keyctl get_persistent @u)" >/dev/null 2>&1 || true
fi
# tauri-driver cannot bind :4444 while the previous run's socket is still
# in TIME_WAIT, and exits when it cannot. Start it until it is answering.
#
# It also drives WebKitWebDriver on a second port (4445 by default); one
# still winding down from the last session made the *next* session's
# creation crash the driver — every other story failed with "fetch
# failed" — so each start takes a fresh native port.
started=0
for attempt in $(seq 1 30); do
  HOME="$E2E_HOME" DISPLAY=:99 XDG_DATA_HOME="$E2E_HOME/.local/share" XDG_CONFIG_HOME="$E2E_HOME/.config" \
    WEBKIT_DISABLE_COMPOSITING_MODE=1 tauri-driver --native-port "$((4500 + RANDOM % 4000))" >"$OUT/tauri-driver.log" 2>&1 &
  echo $! >"$OUT/tauri-driver.pid"
  for _ in $(seq 1 12); do
    sleep 0.25
    kill -0 "$(cat "$OUT/tauri-driver.pid")" 2>/dev/null || break
    if curl -fsS -o /dev/null http://127.0.0.1:4444/status 2>/dev/null; then started=1; break 2; fi
  done
  sleep 1
done
[ "$started" = 1 ] || { echo "tauri-driver did not start" >&2; tail -5 "$OUT/tauri-driver.log" >&2; exit 1; }
echo "$E2E_HOME"
