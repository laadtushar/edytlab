#!/bin/bash
# assemble.sh  -> rebuilds the finished files in out/ (the six mp4s, the plain .srt files and the contact sheets) from
#   out/picture-only/*.mp4   the silent picture of each cut (kept in the repo; `launch.py <key> render` makes them),
#   work/                    scratch: cue files and the three synthesised UI sounds, regenerated here (work/ is not committed),
#   out/narration/<key>/     the narration takes and narr_<key>.json (kept in the repo; `narrate.py gen` and `pick` make them).
# It OVERWRITES the files in out/. Run it from anywhere; it needs website/public/demos/ (or EDYTLAB_DEMO_DIR), ffmpeg and Pillow.
# The narrated .srt files are copied from out/narration/<key>/narr_<key>.srt when `narrate.py pick` has written one; otherwise the committed ones stay.
# Narration is never regenerated here (that is `narrate.py gen`, which spends ElevenLabs characters).
case "$1" in -h|--help) sed -n '2,8p' "$0"; exit 0;; esac
set -e
cd "$(dirname "$0")"
OUT=../out; WORK=../work
mkdir -p $OUT $WORK/sfx
if [ ! -f $WORK/sfx/whoosh.wav ] || [ ! -f $WORK/sfx/tick.wav ] || [ ! -f $WORK/sfx/chime.wav ]; then python3 -I sfx.py $WORK/sfx; fi
declare -A NAME=( [li]=linkedin [rd]=reddit [ig]=instagram )
declare -A SHEET=( [li]="5 360 1.0 2.8 5.5 9.2 11.5 15.5 17.5 20.5 24.5 28 33 40 46 50 54 57 60 62.5 65 67.5" [rd]="4 480 1.5 3.5 6.5 10.5 14 19 22 27 34 40 43 47 50 54 58 61" [ig]="8 240 0.5 2.0 4 7 10 13 16 18 20 23 25 27 29 31" )
for k in li rd ig; do
  n=${NAME[$k]}
  nice -n 10 python3 -I launch.py $k cues $WORK/cues_$k.json > /dev/null
  nice -n 10 python3 -I audio.py mix $WORK/cues_$k.json ../out/picture-only/launch-$n-picture-only.mp4 $OUT/launch-$n.mp4 | tail -1
  nice -n 10 python3 -I audio.py mix $WORK/cues_$k.json ../out/picture-only/launch-$n-picture-only.mp4 $OUT/launch-$n-narrated.mp4 --narration $OUT/narration/$k/narr_$k.json | tail -1
  python3 -I mksrt.py $WORK/cues_$k.json $OUT/launch-$n.srt
  if [ -f $OUT/narration/$k/narr_$k.srt ]; then cp $OUT/narration/$k/narr_$k.srt $OUT/launch-$n-narrated.srt; fi
  nice -n 10 python3 -I sheet.py $OUT/launch-$n.mp4 $OUT/launch-$n-contact.png ${SHEET[$k]}
done
echo assembled
