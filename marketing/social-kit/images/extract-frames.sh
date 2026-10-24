#!/usr/bin/env bash
# Take the app frames for the social images from the site's own videos.
#
#   bash marketing/social-kit/images/extract-frames.sh
#
# One frame of one video at a named second, cropped to part of the app. Nothing
# is drawn on, resized or recoloured. Needs ffmpeg. The frames it writes are
# committed in frames/, so a render does not need it.
#
# Two crops of each frame:
#   tN-chat.png  the chat with the lane beside it, about 1.5 to 1.8 wide for each tall
#                (every canvas but Pinterest)
#   tN-pin.png   a narrower crop of the same frame, about 1.0 to 1.3 (Pinterest)
#
# Each crop's top edge falls in the clear gap above a line of the chat (found by
# scanning the pixels for an empty row, not by eye), so no line is sliced; no
# crop includes the mouse pointer, the video's own speed badge, its title or
# caption, or the status bar with the app's version.
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

# 1 Describe the edit: whats-new-v070 at 11 s (1920x1080 video; the app's panel is x100 y150 1720x835).
# The typed request, then the Fade and Gain calls. Crops start below the ruler.
grab t1-chat $V/whats-new-v070.mp4 11 560 165 1250 780
grab t1-pin  $V/whats-new-v070.mp4 11 900 165 910 760

# 2 A DJ transition in one sentence: dj-beatmatched-transition at 84 s (1280x776 recording).
# After the turn has ended (the pointer has left the chat): Claude's reply on the filter and the bars.
grab t2-chat $D/dj-beatmatched-transition.mp4 84 575 94 705 467
grab t2-pin  $D/dj-beatmatched-transition.mp4 84 800 94 480 467

# 3 Extend a club intro: dj-extended-club-intro at 52 s.
# After the turn: Claude's summary of the 16-bar intro (new length, where the music comes in).
grab t3-chat $D/dj-extended-club-intro.mp4 52 575 112 705 449
grab t3-pin  $D/dj-extended-club-intro.mp4 52 800 112 480 449

# 4 Your audio stays on your computer: dj-mini-mix at 72 s.
# After the turn: Claude's reply on loudness and the limiter, with the third lane beside it.
grab t4-chat $D/dj-mini-mix.mp4 72 575 176 705 406
grab t4-pin  $D/dj-mini-mix.mp4 72 800 176 480 406

# 5 Every edit is a version: whats-new-v070 at 28.5 s (the history graph; the hand pointer is below the crop).
grab t5-chat $V/whats-new-v070.mp4 28.5 400 170 740 490
grab t5-pin  $V/whats-new-v070.mp4 28.5 420 170 640 520

# 6 Free and open source: whats-new-v070 at 44 s (the chat's seven Gain calls beside the lanes; the video's
# "1.7x speed" badge is above the crop).
grab t6-chat $V/whats-new-v070.mp4 44 620 240 1190 720
grab t6-pin  $V/whats-new-v070.mp4 44 900 240 910 740
