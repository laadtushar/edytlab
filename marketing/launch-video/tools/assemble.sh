#!/bin/bash
# assemble.sh  -> rebuilds the finished files in out/ (the three plain mp4s, their .srt files and the contact sheets) from
#   out/picture-only/*.mp4   the silent picture of each cut (kept in the repo; `launch.py <key> render` makes them),
#   work/                    scratch: cue files and the three synthesised UI sounds, regenerated here (work/ is not committed).
# It OVERWRITES the files in out/. Run it from anywhere; it needs website/public/demos/ (or EDYTLAB_DEMO_DIR), ffmpeg and Pillow.
# There are no narrated cuts: the voice lines in out/narration/ were written for the earlier takes (see README.md). `audio.py mix
# ... --narration` still mixes a voice onto a picture-only master when there is a narr.json timed to these cuts.
case "$1" in -h|--help) sed -n '2,7p' "$0"; exit 0;; esac
set -e
cd "$(dirname "$0")"
OUT=../out; WORK=../work
mkdir -p $OUT $WORK/sfx
if [ ! -f $WORK/sfx/whoosh.wav ] || [ ! -f $WORK/sfx/tick.wav ] || [ ! -f $WORK/sfx/chime.wav ]; then python3 -I sfx.py $WORK/sfx; fi
declare -A NAME=( [li]=linkedin [rd]=reddit [ig]=instagram )
declare -A SHEET=( [li]="5 360 1.0 2.8 5.5 9.2 12.5 16.5 20.5 24.5 27 29 32 35 38 41.5 44.5 46.5 49.5 53 57 60 62.5 65 67.5" [rd]="4 480 1.5 3.5 7 10.5 14 18.5 21 24.5 28 32 34.5 38.5 41 44 47 50 54 58.5 61" [ig]="8 240 0.5 2.0 4 7.2 9.5 11.5 14 16.5 19 21.5 24 26.5 28.5 30.5" )
for k in li rd ig; do
  n=${NAME[$k]}
  nice -n 10 python3 -I launch.py $k cues $WORK/cues_$k.json > /dev/null
  nice -n 10 python3 -I audio.py mix $WORK/cues_$k.json ../out/picture-only/launch-$n-picture-only.mp4 $OUT/launch-$n.mp4 | tail -1
  python3 -I mksrt.py $WORK/cues_$k.json $OUT/launch-$n.srt
  nice -n 10 python3 -I sheet.py $OUT/launch-$n.mp4 $OUT/launch-$n-contact.png ${SHEET[$k]}
done
echo assembled
