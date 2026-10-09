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
# The scripted model is a node process; find it by its own command line
# rather than `pkill -f`, which would match any shell that merely mentions
# the name.
for pid in $(pgrep -x node); do
  case "$(tr '\0' ' ' </proc/"$pid"/cmdline 2>/dev/null)" in
    *fake-llm.mjs*) kill "$pid" 2>/dev/null || true ;;
  esac
done
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
# The scripted model, unless a real one already answers on :11434
# (LLM=real: llama-server with a local model, started separately).
if [ "${LLM:-fake}" != real ]; then
  node "$(dirname "$0")/fake-llm.mjs" >"$OUT/fake-llm.log" 2>&1 &
  echo $! >"$OUT/fake-llm.pid"
  # Something else already on :11434 (a real llama-server left running)
  # would answer in the fake's place and every scripted story would run
  # against it. The fake is up when its request log answers.
  for _ in $(seq 1 40); do
    curl -sf http://127.0.0.1:11434/__requests >/dev/null 2>&1 && break
    sleep 0.25
  done
  curl -sf http://127.0.0.1:11434/__requests >/dev/null 2>&1 || {
    echo "the fake model is not what answers on :11434 - stop the other server (or run with LLM=real)" >&2
    exit 1
  }
fi
# The app keeps its provider, keys and base URLs through the `keyring`
# crate, which on Linux is the kernel keyring: an entry lives in the
# *session* keyring and is linked into the user's *persistent* one, so a
# run is isolated by neither a fresh HOME nor a fresh process. A first
# launch clears both; a restart within a story passes KEEP_KEYRING=1 to
# find its settings again.
#
# Both need a session keyring that every process of the run shares, which
# a desktop login provides (pam_keyinit) and a bare container does not:
# without one each lookup starts from nothing and a restart "forgets"
# everything. `run-suite.sh` supplies it; say so when it is missing.
if ! keyctl rdescribe @s >/dev/null 2>&1; then
  echo "run-env.sh: no session keyring; start the suite through run-suite.sh" >&2
fi
if [ "${KEEP_KEYRING:-0}" != 1 ]; then
  # The persistent keyring must be linked into the session keyring to be
  # possessed: clearing it through @u is "Permission denied".
  keyctl clear "$(keyctl get_persistent @s)" >/dev/null || echo "run-env.sh: could not clear the persistent keyring" >&2
  keyctl clear @s >/dev/null || echo "run-env.sh: could not clear the session keyring" >&2
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
