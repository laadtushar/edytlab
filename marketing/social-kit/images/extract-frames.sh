#!/usr/bin/env bash
# Take the app frames for the social images from the site's own videos.
#
#   bash marketing/social-kit/images/extract-frames.sh
#
# One frame of one video at a named second, cropped to the app (the video's
# own title, the burned-in step caption, the status bar and its footer note are
# left out; nothing is drawn on, resized or recoloured). Needs ffmpeg.
# The frames it writes are committed in frames/, so a render does not need it.
set -euo pipefail
cd "$(dirname "$0")/../../.."
OUT=marketing/social-kit/images/frames
mkdir -p "$OUT"

# grab <name> <video> <seconds> <x> <y> <w> <h>
grab() {
  ffmpeg -v error -y -ss "$3" -i "$2" -frames:v 1 -vf "crop=$6:$7:$4:$5" "$OUT/$1.png"
}

V=website/public/videos
D=website/public/demos

# 1 Describe the edit: whats-new-v070 at 11 s (the request, the Fade and Gain calls). 1920x1080 video, the app's panel is x96 y150 1728x836, cropped 4 px inside its edge.
grab t1-wide $V/whats-new-v070.mp4 11 100 151 1716 829
grab t1-mid  $V/whats-new-v070.mp4 11 560 151 1256 829
grab t1-tall $V/whats-new-v070.mp4 11 900 178 916 540

# 2 A DJ transition in one sentence: dj-beatmatched-transition at 76 s (1280x776 recording; every crop stops at y 612, above the chat box, so the burned-in step caption and the status bar with its version are out).
grab t2-wide $D/dj-beatmatched-transition.mp4 76 0 0 1280 612
grab t2-mid $D/dj-beatmatched-transition.mp4 76 505 96 775 516
grab t2-tall $D/dj-beatmatched-transition.mp4 76 480 96 800 516

# 3 Extend a club intro: dj-extended-club-intro at 47 s (the request to repeat the first 8 bars, and the Copy and Paste calls).
grab t3-wide $D/dj-extended-club-intro.mp4 47 0 0 1280 612
grab t3-mid $D/dj-extended-club-intro.mp4 47 505 96 775 516
grab t3-tall $D/dj-extended-club-intro.mp4 47 660 96 620 516

# 4 Your audio stays on your computer: dj-mini-mix at 40 s (three tracks on the left, the request and its four Fade calls in the chat on the right).
grab t4-wide $D/dj-mini-mix.mp4 40 0 0 1280 612
grab t4-mid $D/dj-mini-mix.mp4 40 505 96 775 516
grab t4-tall $D/dj-mini-mix.mp4 40 540 96 740 516

# 5 Every edit is a version: whats-new-v070 at 28.5 s (the history graph, three branches).
grab t5-wide $V/whats-new-v070.mp4 28.5 470 180 580 380
grab t5-mid  $V/whats-new-v070.mp4 28.5 470 180 580 380
grab t5-tall $V/whats-new-v070.mp4 28.5 470 180 580 500

# 6 Free and open source: whats-new-v070 at 46.5 s (two lanes of a big session, the gain read-outs; the panel is x96 y150 1728x836, cropped 4 px inside its edge).
grab t6-wide $V/whats-new-v070.mp4 46.5 100 151 1716 793
grab t6-mid  $V/whats-new-v070.mp4 46.5 100 151 1250 793
grab t6-tall $V/whats-new-v070.mp4 46.5 100 151 1000 793
