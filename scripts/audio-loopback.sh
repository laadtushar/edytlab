#!/usr/bin/env bash
# Run the recorder's loopback tests against a PRIVATE PulseAudio daemon.
#
#   scripts/audio-loopback.sh [extra args for the test binary]
#
# No sound card is needed. A daemon of our own (nothing else on the machine
# is touched: it has its own socket, state and runtime dir under a fresh
# temporary directory) gets one null sink, `loop`. cpal's ALSA host reaches
# it through ALSA's `pulse` plugin, which reads PULSE_SERVER / PULSE_SINK /
# PULSE_SOURCE, so the program plays into `loop` and records `loop.monitor`:
# a real device path with real buffering, which is what the latency model
# has to hold up against. It does not cover real converters or the air
# between a speaker and a microphone; a calibration on the user's machine
# measures those.
#
# The daemon is started in this script, its PID is kept, and it is stopped
# by that PID when the script exits however it exits. Never `pkill -f` it,
# and never point this at a shared sink (the demo recordings use one).
set -euo pipefail

command -v pulseaudio >/dev/null || { echo "pulseaudio is not installed" >&2; exit 2; }

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
dir="$(mktemp -d "${TMPDIR:-/tmp}/edytlab-loopback.XXXXXX")"
pid=""

cleanup() {
  if [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null; then
    kill "$pid" 2>/dev/null || true
    # A bounded wait for it to go; the PID is ours, nothing else is signalled.
    for _ in $(seq 1 50); do kill -0 "$pid" 2>/dev/null || break; sleep 0.1; done
    kill -9 "$pid" 2>/dev/null || true
  fi
  rm -rf "$dir"
}
trap cleanup EXIT INT TERM

export PULSE_RUNTIME_PATH="$dir" PULSE_STATE_PATH="$dir" PULSE_CONFIG_PATH="$dir" XDG_RUNTIME_DIR="$dir"
sock="$dir/native"

pulseaudio -n --daemonize=no --use-pid-file=no --exit-idle-time=-1 --log-target=stderr \
  --load="module-native-protocol-unix socket=$sock auth-anonymous=1" \
  --load="module-null-sink sink_name=loop rate=48000 channels=2" \
  2>"$dir/pulseaudio.log" &
pid=$!

ready=""
for _ in $(seq 1 100); do
  if [ -S "$sock" ] && PULSE_SERVER="unix:$sock" pactl info >/dev/null 2>&1; then ready=1; break; fi
  kill -0 "$pid" 2>/dev/null || break
  sleep 0.1
done
if [ -z "$ready" ]; then
  echo "the private PulseAudio daemon did not start:" >&2
  cat "$dir/pulseaudio.log" >&2
  exit 3
fi

echo "private PulseAudio pid $pid, socket $sock"
cd "$root"
EDYTLAB_AUDIO_LOOPBACK=1 \
PULSE_SERVER="unix:$sock" PULSE_SINK=loop PULSE_SOURCE=loop.monitor \
  cargo test -p recorder --test loopback -- --ignored --nocapture --test-threads=1 "$@"
