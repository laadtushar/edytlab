#!/usr/bin/env bash
# Builds the finished files for the given compositions (default: all five) into ../out:
#   <id> -> edytlab-v0.7.0-<slug>.mp4 / .srt / -contact.png
# The steps: the sound cues and .srt from src/cuts.ts, the picture by Remotion, the sound and the mastering by
# tools/audio.py, a contact sheet, then tools/verify.py checks every file against the spec.
# Needs: pnpm install --ignore-workspace, node 22, ffmpeg, Python 3.12 or older with Pillow (the contact sheet),
# and a Chromium (REMOTION_BROWSER_EXECUTABLE; see remotion.config.ts).
set -euo pipefail
cd "$(dirname "$0")/.."
IDS=("$@")
[ ${#IDS[@]} -eq 0 ] && IDS=(R1 R2 R3 V1 V1sq)
OUT=../out
mkdir -p "$OUT" work
node scripts/tokens.ts --check >/dev/null
node scripts/cues.ts "$OUT"
node scripts/render.ts "${IDS[@]}" ${NO_BUNDLE:+--no-bundle}
for id in "${IDS[@]}"; do
  slug=$(python3 -c "import json,sys; print(json.load(open('work/cues/$id.json'))['slug'])")
  base="$OUT/edytlab-v0.7.0-$slug"
  echo "== $id -> $base.mp4"
  python3 tools/audio.py "work/cues/$id.json" "work/$id.silent.mp4" "$base.mp4" --work work
  python3 tools/sheet.py "$base.mp4" "$base-contact.png"
done
python3 tools/verify.py "$OUT"
