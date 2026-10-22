# Recording the v0.7.0 promo footage live

The footage in `../footage/` is the released v0.7.0 app driven by real Claude. The stories that drive it are
`apps/desktop/e2e-native/stories/promo.mjs` (with `promo-audio.mjs`) and they run in the native suite, which drives the real
desktop binary over WebDriver (`apps/desktop/e2e-native/`). `make-footage.mjs` in this folder cuts what they leave behind into
the clips. The scripted footage's Playwright recorder is in `../../archive/promo-v0.7.0/2026-10-22-scripted/record/`.

## What you need

Linux with Xvfb, openbox, tauri-driver, WebKitWebDriver, xdotool, ffmpeg and PulseAudio. The app: the release's `.deb`
(`gh api repos/laadtushar/edytlab/releases/tags/v0.7.0`), unpacked with `dpkg-deb -x edytlab_0.7.0_amd64.deb <dir>`; its
binary is `<dir>/usr/bin/edytlab-desktop` and finds its resources (`<dir>/usr/lib/edytlab/`: the ONNX Runtime, templates,
bundled skills) beside it. Unpack it to a short path: Settings prints where it found the runtime.

An audio sink to capture, started once:

```bash
pulseaudio -n --daemonize=yes --exit-idle-time=-1 -L "module-null-sink sink_name=demo" -L module-native-protocol-unix
pactl set-default-sink demo
export PULSE_SERVER=$(pactl info | sed -n 's/^Server String: //p')
```

The model, through the metering proxy (every request is counted in `$OUT/claude-usage.jsonl`, cached input is priced at the
full rate as an upper bound, and it refuses once the budget is spent):

```bash
BUDGET_USD=8 PORT=8788 OUT=/tmp/promo-run node apps/desktop/e2e-native/claude-proxy.mjs &
```

## Recording

From `apps/desktop/e2e-native` (the key is only ever in the environment of this one command; do not echo it):

```bash
APP=/path/to/usr/bin/edytlab-desktop OUT=/tmp/promo-run PROMO=1 RECORD=1 RECORD_AUDIO=demo.monitor \
GDK_SCALE=2 SCREEN=3300x2200x24 ANTHROPIC_E2E_KEY=… DEMO_MODEL=claude-sonnet-5-5 DEMO_EFFORT=low \
  ./run-suite.sh 9-promo-s1          # or s2 … s8b; several substrings run several stories
```

`GDK_SCALE=2` makes the page 1600x1000 CSS pixels at two screen pixels each; `SCREEN` is a display big enough for the
window and its title bar. The story puts the window at the top left, sizes it so the page is exactly 1600x1000, finds where
the page is on the screen (it moves the X pointer and asks the page where it landed) and films exactly that rectangle at
3200x2000. Without `ANTHROPIC_E2E_KEY` the stories run against the scripted model (`fake-llm.mjs`, free), which is how
each shot's screen work was rehearsed first; that is a rehearsal, not footage.

Each story is also a test: it asserts what its clip claims (the tools Claude ran, the gains, the node counts, the readout never
going back, the mix file removed and written again), so a clip whose assertion fails is not used.

Two things about the capture, both measured and handled:

- **ffmpeg loses the first second or two** while it opens the display and the sound, so each story waits before it does
  anything, and the clip is cut from the mark `start`.
- **Sound and picture do not share a clock**, and how far apart they run changes from one recording to the next (0.07 to 1.0 s
  across the runs made). A clip with sound therefore opens on a *beacon*: a white block over the top left of the page and a
  1 kHz tone, sent in the same breath. `make-footage.mjs` finds the block's first frame and the tone's first sample, measures
  the gap (it is in the clip's `.notes.json`, `sync.soundIsAheadBy`) and moves the sound by it. The beacon is before the clip's
  start. The first Play's sound then starts within a frame or two of the app's Pause button appearing, as it should.

## Cutting

```bash
OUT=/tmp/promo-run DEST=marketing/promo-v0.7.0/footage node marketing/promo-v0.7.0/record/make-footage.mjs [clip …]
```

It refuses a recording that ends before its story did (ffmpeg fell behind) or that dropped more than 15% of its frames, and
says to record it again. `REENCODE=1` re-encodes a clip whose `.mp4` already exists.

## If you re-take a clip

The camera boxes and ring rectangles in `../remotion/src/shots.ts` are in CSS pixels of the page and the moments are in
the clip's seconds; check them against the new clip (`remotion/scripts/stills.ts` renders stills of a cut).
