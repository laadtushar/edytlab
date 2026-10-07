#!/usr/bin/env bash
# Start a virtual display, the fake model server and tauri-driver, with a
# fresh HOME so the app starts as a first launch. Prints the HOME used.
set -euo pipefail
OUT=${OUT:-/tmp/edytlab-native}
mkdir -p "$OUT"
export E2E_HOME=${E2E_HOME:-$(mktemp -d /tmp/edytlab-home.XXXX)}
pkill -f "Xvfb :99" 2>/dev/null || true
pkill -f tauri-driver 2>/dev/null || true
pkill -f WebKitWebDriver 2>/dev/null || true
pkill -f fake-llm.mjs 2>/dev/null || true
Xvfb :99 -screen 0 1440x900x24 >"$OUT/xvfb.log" 2>&1 &
sleep 1
node "$(dirname "$0")/fake-llm.mjs" >"$OUT/fake-llm.log" 2>&1 &
HOME="$E2E_HOME" DISPLAY=:99 XDG_DATA_HOME="$E2E_HOME/.local/share" XDG_CONFIG_HOME="$E2E_HOME/.config" \
  WEBKIT_DISABLE_COMPOSITING_MODE=1 tauri-driver >"$OUT/tauri-driver.log" 2>&1 &
sleep 2
echo "$E2E_HOME"
