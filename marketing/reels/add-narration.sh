#!/usr/bin/env bash
# add-narration.sh <video.mp4> <cues.json> <audio-dir> <out.mp4>
#
# Lays each narration clip <audio-dir>/<cue-id>.(mp3|wav) at its cue's start, ducks the app audio under
# the voice, mixes, loudness-normalises (-14 LUFS integrated, -1 dBTP) and muxes the result onto the
# video stream WITHOUT re-encoding it (-c:v copy). The picture is never touched.
#
# cues.json: {"cues": [{"id": "R01", "start": 0.0, "end": 3.0, "voice_end": 2.4, "silent": false, ...}]}
#   id         clip file name stem
#   start      seconds on the video's timeline where the clip starts (adelay)
#   end        end of the cue; `voice_end` (optional) is where the voice has to stop when the app
#              audio carries music after that point (a "Listen" moment)
#   silent     true = the cue has no narration (no clip is expected)
#   "audio"    optional: use <audio-dir>/<audio>.(mp3|wav) instead of <id>.(mp3|wav)
#   top-level "sfx": [{"file": "sfx/whoosh.mp3", "at": 3.0, "gain_db": -18}]  optional sound effects,
#              paths relative to cues.json; mixed after the ducking, so keep them off music.
#
# Warnings (stderr): a clip longer than its cue window, a clip that leaves under 0.3 s of room, a clip
# that runs past the end of the video, a missing clip for a non-silent cue.
#
# Tunables (environment): DUCK_DB (12)   how far the app audio dips under speech
#                         ATTACK_MS (150) RELEASE_MS (400)   ducking envelope
#                         VOICE_LUFS (-15) level the narration is brought to before mixing
#                         TARGET_LUFS (-14) TARGET_TP (-1)   final loudness, measured on the encoded AAC
#                         AAC_BITRATE (192k)  KEEP_TMP=1 keeps the work directory
set -euo pipefail

if [ "$#" -ne 4 ]; then
  echo "usage: $0 <video.mp4> <cues.json> <audio-dir> <out.mp4>" >&2
  exit 2
fi
VIDEO=$1; CUES=$2; ADIR=$3; OUT=$4
for t in ffmpeg ffprobe python3 awk; do command -v "$t" >/dev/null || { echo "missing tool: $t" >&2; exit 1; }; done
[ -f "$VIDEO" ] || { echo "no such video: $VIDEO" >&2; exit 1; }
[ -f "$CUES" ] || { echo "no such cues file: $CUES" >&2; exit 1; }
[ -d "$ADIR" ] || { echo "no such audio dir: $ADIR" >&2; exit 1; }

DUCK_DB=${DUCK_DB:-12}
ATTACK_MS=${ATTACK_MS:-150}
RELEASE_MS=${RELEASE_MS:-400}
VOICE_LUFS=${VOICE_LUFS:--15}
TARGET_LUFS=${TARGET_LUFS:--14}
TARGET_TP=${TARGET_TP:--1}
AAC_BITRATE=${AAC_BITRATE:-192k}
NICE="nice -n 10"
FF="$NICE ffmpeg -nostdin -hide_banner -y -v error"

TMP=$(mktemp -d "${TMPDIR:-/tmp}/add-narration.XXXXXX")
cleanup() { if [ -z "${KEEP_TMP:-}" ]; then rm -rf "$TMP"; else echo "work dir kept: $TMP" >&2; fi; }
trap cleanup EXIT

# Video length drives everything; the audio is padded or cut to it.
VDUR=$(ffprobe -v error -select_streams v:0 -show_entries stream=duration -of csv=p=0 "$VIDEO")
case "$VDUR" in ""|N/A) VDUR=$(ffprobe -v error -show_entries format=duration -of csv=p=0 "$VIDEO");; esac

# --- 1. read the cues, find the clips, warn, write the voice-track filter graph --------------------
python3 - "$VIDEO" "$CUES" "$ADIR" "$TMP" "$VDUR" <<'PY'
import json, os, subprocess, sys
video, cues_path, adir, tmp, vdur = sys.argv[1], sys.argv[2], sys.argv[3], sys.argv[4], float(sys.argv[5])
doc = json.load(open(cues_path))
cues = doc["cues"] if isinstance(doc, dict) else doc
base = os.path.dirname(os.path.abspath(cues_path))

def dur(path):
    p = subprocess.run(["ffprobe", "-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", path], capture_output=True, text=True)
    return float(p.stdout.strip())

def speech_length(path, total):
    """Clip length without its trailing silence (TTS files end with a little air)."""
    p = subprocess.run(["ffmpeg", "-nostdin", "-hide_banner", "-i", path, "-af", "silencedetect=n=-45dB:d=0.12", "-f", "null", "-"], capture_output=True, text=True)
    last = None
    for line in p.stderr.splitlines():
        if "silence_start:" in line:
            last = float(line.split("silence_start:")[1].split()[0])
    return last if last is not None and last > 0.2 and total - last > 0.1 else total

inputs, filt, labels, placed = [], [], [], 0
warn = lambda m: print(f"WARNING: {m}", file=sys.stderr)
for c in cues:
    cid, start, end = c["id"], float(c["start"]), float(c["end"])
    stem = c.get("audio", cid)
    clip = next((f"{adir}/{stem}.{e}" for e in ("mp3", "wav") if os.path.isfile(f"{adir}/{stem}.{e}")), None)
    if clip is None:
        if not c.get("silent") and c.get("narration") is not False:
            warn(f"{cid}: no {stem}.mp3 or {stem}.wav in {adir}, cue left silent")
        continue
    total = dur(clip)
    eff = speech_length(clip, total)
    window = float(c.get("voice_end", end)) - start
    if eff > window + 1e-3:
        warn(f"{cid}: clip is {eff:.2f}s but its cue window is {window:.2f}s (start {start:.2f} -> {start + window:.2f}); it runs {eff - window:.2f}s into whatever follows")
    elif "max_seconds" in c and c["max_seconds"] and eff > float(c["max_seconds"]) + 1e-3:
        print(f"NOTE: {cid}: clip is {eff:.2f}s, over max_seconds {float(c['max_seconds']):.2f}s (less than 0.3 s of room)", file=sys.stderr)
    if start + eff > vdur + 1e-3:
        warn(f"{cid}: clip ends at {start + eff:.2f}s, after the end of the video ({vdur:.2f}s); it will be cut")
    i = len(inputs) // 2
    inputs += ["-i", clip]
    ms = int(round(start * 1000))
    filt.append(f"[{i}:a]aresample=48000,aformat=sample_fmts=fltp:channel_layouts=stereo,adelay={ms}|{ms}[c{i}]")
    labels.append(f"[c{i}]")
    placed += 1
if not placed:
    print("ERROR: no narration clips found for any cue", file=sys.stderr)
    sys.exit(3)
filt.append("".join(labels) + f"amix=inputs={len(labels)}:normalize=0:duration=longest:dropout_transition=0,apad=whole_dur={vdur:.4f},atrim=end={vdur:.4f}[voice]")
open(f"{tmp}/voice.filter", "w").write(";\n".join(filt))
open(f"{tmp}/voice.args", "w").write("\n".join(inputs[1::2]))
# sound effects
sfx = doc.get("sfx", []) if isinstance(doc, dict) else []
sf, sargs = [], []
for k, s in enumerate(sfx):
    p = s["file"] if os.path.isabs(s["file"]) else os.path.join(base, s["file"])
    if not os.path.isfile(p):
        warn(f"sfx file missing: {p}")
        continue
    ms = int(round(float(s["at"]) * 1000))
    g = float(s.get("gain_db", -18))
    sargs.append(p)
    sf.append(f"[{{IDX{len(sargs)-1}}}:a]aresample=48000,aformat=sample_fmts=fltp:channel_layouts=stereo,volume={g}dB,adelay={ms}|{ms}[s{len(sargs)-1}]")
open(f"{tmp}/sfx.filter", "w").write("\n".join(sf))
open(f"{tmp}/sfx.args", "w").write("\n".join(sargs))
print(f"{placed} narration clip(s) placed, {len(sargs)} sound effect(s)", file=sys.stderr)
PY

# --- 2. voice track (stereo wav, same length as the video) and its level ------------------------------
mapfile -t VIN < "$TMP/voice.args"
VARGS=(); for f in "${VIN[@]}"; do VARGS+=(-i "$f"); done
$FF "${VARGS[@]}" -filter_complex_script "$TMP/voice.filter" -map "[voice]" -c:a pcm_s16le -ar 48000 "$TMP/voice.wav"

# integrated loudness and true peak of a file's audio, as "I TP" (two numbers)
measure() {
  $NICE ffmpeg -nostdin -hide_banner -i "$1" -vn -af ebur128=peak=true -f null - 2>&1 | awk '
    /Summary:/ {s=1}
    s && $1=="I:" {i=$2}
    s && $1=="Peak:" {p=$2}
    END {printf "%s %s\n", i, p}'
}
read -r VI _ < <(measure "$TMP/voice.wav")
VGAIN=$(awk -v t="$VOICE_LUFS" -v i="$VI" 'BEGIN{printf "%.2f", t-i}')

# --- 3. duck the app audio under the voice, mix ---------------------------------------------------------
# sidechaincompress keyed on the voice. Its ratio is fixed at 20:1 (so the dip is about 0.95 x the
# level above the threshold) and the real depth depends on how the speech sits, so the threshold is
# CALIBRATED: a pink-noise bed is ducked with the same filter, the dip is measured inside the voiced
# stretches, and the threshold is moved until the dip is DUCK_DB (a few passes, a second each).
THR=$(awk -v v="$VOICE_LUFS" -v d="$DUCK_DB" 'BEGIN{db=v-4.0-d/0.95; printf "%.5f", 10^(db/20)}')
THR=$(python3 - "$TMP" "$VDUR" "$VGAIN" "$THR" "$DUCK_DB" "$ATTACK_MS" "$RELEASE_MS" <<'PY'
import math, os, struct, subprocess, sys
tmp, vdur, vgain, thr, duck, att, rel = sys.argv[1], float(sys.argv[2]), sys.argv[3], float(sys.argv[4]), float(sys.argv[5]), sys.argv[6], sys.argv[7]
RATE, HOP = 8000, 0.05

def pcm(args):
    p = subprocess.run(["nice", "-n", "10", "ffmpeg", "-nostdin", "-v", "error"] + args + ["-ac", "1", "-ar", str(RATE), "-f", "s16le", "-"], capture_output=True)
    n = len(p.stdout) // 2
    return struct.unpack(f"<{n}h", p.stdout)

def frames(s):
    h = int(RATE * HOP)
    out = []
    for i in range(0, len(s) - h + 1, h):
        e = sum(x * x for x in s[i:i + h]) / h
        out.append(10 * math.log10(max(e, 1.0) / 32768.0 ** 2))
    return out

voice = frames(pcm(["-i", f"{tmp}/voice.wav", "-af", f"volume={vgain}dB"]))
# voiced stretches: frames above -50 dB, gaps under 0.3 s merged; measure 0.35 s in, to 0.05 s before the end
act = [v > -50 for v in voice]
gap = int(0.3 / HOP)
for i in range(len(act)):
    if not act[i] and any(act[max(0, i - gap):i]) and any(act[i:i + gap]):
        act[i] = True
meas = []
i = 0
while i < len(act):
    if act[i]:
        j = i
        while j < len(act) and act[j]:
            j += 1
        meas += list(range(i + int(0.35 / HOP), max(i + int(0.35 / HOP), j - 1)))
        i = j
    else:
        i += 1
if len(meas) < 4:
    print(f"{thr:.5f}")
    sys.exit(0)
best = thr
for _ in range(4):
    flt = (f"[0:a]aformat=sample_fmts=fltp:channel_layouts=stereo[bed];[1:a]volume={vgain}dB[k];"
           f"[bed][k]sidechaincompress=threshold={thr:.5f}:ratio=20:attack={att}:release={rel}:makeup=1:knee=1.5:link=average:detection=rms[d]")
    args = ["-f", "lavfi", "-i", f"anoisesrc=color=pink:amplitude=0.3:sample_rate=48000:duration={vdur}", "-i", f"{tmp}/voice.wav",
            "-filter_complex", flt, "-map", "[d]"]
    bed = frames(pcm(["-f", "lavfi", "-i", f"anoisesrc=color=pink:amplitude=0.3:sample_rate=48000:duration={vdur}",
                      "-af", "aformat=sample_fmts=fltp:channel_layouts=stereo"]))
    duck_f = frames(pcm(args))
    n = min(len(bed), len(duck_f))
    pts = [k for k in meas if k < n]
    a = sum(bed[k] - duck_f[k] for k in pts) / len(pts)      # dB of dip, positive
    best = thr
    if abs(a - duck) <= 0.6:
        break
    thr = min(1.0, max(0.001, thr * 10 ** ((a - duck) / 0.95 / 20)))
print(f"{best:.5f}")
PY
)
SFX_FILTER=""; SFX_LABELS=""; SARGS=()
if [ -s "$TMP/sfx.args" ]; then
  mapfile -t SIN < "$TMP/sfx.args"
  for f in "${SIN[@]}"; do SARGS+=(-i "$f"); done
  # inputs: 0 = video, 1 = voice.wav, 2.. = sfx
  SFX_FILTER=$(awk 'BEGIN{ORS=";"} {print}' "$TMP/sfx.filter")
  for k in "${!SIN[@]}"; do
    SFX_FILTER=${SFX_FILTER//\{IDX$k\}/$((k+2))}
    SFX_LABELS="$SFX_LABELS[s$k]"
  done
fi
NMIX=$((2 + ${#SARGS[@]}/2))
cat > "$TMP/mix.filter" <<EOF
[0:a]aresample=48000:async=1:first_pts=0,aformat=sample_fmts=fltp:channel_layouts=stereo,apad=whole_dur=$VDUR,atrim=end=$VDUR[app];
[1:a]volume=${VGAIN}dB,asplit=2[vkey][vmix];
[app][vkey]sidechaincompress=threshold=$THR:ratio=20:attack=$ATTACK_MS:release=$RELEASE_MS:makeup=1:knee=1.5:link=average:detection=rms,asplit=2[ducked][dkcopy];
${SFX_FILTER}
[ducked][vmix]${SFX_LABELS}amix=inputs=$NMIX:normalize=0:duration=first:dropout_transition=0[mix]
EOF
$FF -i "$VIDEO" -i "$TMP/voice.wav" "${SARGS[@]}" -filter_complex_script "$TMP/mix.filter" -map "[mix]" -c:a pcm_s16le -ar 48000 "$TMP/mix.wav" \
    -map "[dkcopy]" -c:a pcm_s16le -ar 48000 "$TMP/ducked.wav"

# --- 4. loudness: gain, lookahead limiter, AAC; measure the AAC itself and iterate ----------------------
read -r MI MP < <(measure "$TMP/mix.wav")
GAIN=$(awk -v t="$TARGET_LUFS" -v i="$MI" 'BEGIN{printf "%.3f", t-i}')
CEIL=$(awk -v tp="$TARGET_TP" 'BEGIN{printf "%.2f", tp-0.6}')
for n in 1 2 3 4 5 6 7 8; do
  LIM=$(awk -v c="$CEIL" 'BEGIN{printf "%.4f", 10^(c/20)}')
  $FF -i "$TMP/mix.wav" -af "volume=${GAIN}dB,alimiter=limit=$LIM:attack=4:release=60:level=0,aresample=48000" \
      -c:a aac -b:a "$AAC_BITRATE" -ar 48000 -ac 2 "$TMP/mix.m4a"
  read -r OI OP < <(measure "$TMP/mix.m4a")
  DONE=$(awk -v i="$OI" -v p="$OP" -v t="$TARGET_LUFS" -v tp="$TARGET_TP" 'BEGIN{d=i-t; if(d<0)d=-d; print (d<=0.25 && p<=tp-0.05) ? 1 : 0}')
  [ "$DONE" = 1 ] && break
  GAIN=$(awk -v g="$GAIN" -v i="$OI" -v t="$TARGET_LUFS" 'BEGIN{d=i-t; if(d<0)d=-d; if(d<=0.25){printf "%.3f", g} else {printf "%.3f", g+(t-i)*0.9}}')
  CEIL=$(awk -v c="$CEIL" -v p="$OP" -v tp="$TARGET_TP" 'BEGIN{if(p>tp-0.05){printf "%.2f", c-(p-tp)-0.15} else {printf "%.2f", c}}')
done

# --- 5. mux: the video stream is copied, never re-encoded -------------------------------------------------
mkdir -p "$(dirname "$OUT")"
$FF -i "$VIDEO" -i "$TMP/mix.m4a" -map 0:v:0 -map 1:a:0 -c:v copy -c:a copy -movflags +faststart "$OUT"

# --- 6. report ------------------------------------------------------------------------------------------------
read -r FI FP < <(measure "$OUT")
VD=$(ffprobe -v error -select_streams v:0 -show_entries stream=duration -of csv=p=0 "$OUT")
AD=$(ffprobe -v error -select_streams a:0 -show_entries stream=duration -of csv=p=0 "$OUT")
printf 'wrote %s\n  video %.2fs, audio %.2fs, loudness %s LUFS integrated, true peak %s dBTP (targets %s / %s)\n  voice %s LUFS -> %s dB gain; ducking about %s dB (attack %s ms, release %s ms)\n' \
  "$OUT" "$VD" "$AD" "$FI" "$FP" "$TARGET_LUFS" "$TARGET_TP" "$VI" "$VGAIN" "$DUCK_DB" "$ATTACK_MS" "$RELEASE_MS"
