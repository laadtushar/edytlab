# Recording harness for the v0.7.0 promo footage

Scripts that drive the **real frontend** (`apps/desktop/src`, built for production, with only the IPC boundary
replaced, exactly as the e2e suite runs it) and record it. Nothing here is part of the app's tests: the app's
`playwright.config.ts` has `testDir: "./e2e"`, so `test:e2e` and CI never see this folder.

```bash
cd apps/desktop
pnpm install --frozen-lockfile
# one shot (the name is matched against the file name), or leave it out for all of them
PROMO_PORT=5311 pnpm exec playwright test -c ../../marketing/promo-v0.7.0/record/playwright.config.ts s1
# type-check the scripts
pnpm exec tsc --noEmit -p ../../marketing/promo-v0.7.0/record
```

`PROMO_PORT` (default 5311) is the preview server's port, so a run never collides with `test:e2e` on 5199.
`PROMO_CRF` (default 16) and `PROMO_PRESET` (default `slow`) set the encode. Output goes to `../footage/`.
A shot needs ffmpeg on the PATH. Frames are written to `.out/` while a shot runs and deleted when it ends (`.out/` is git-ignored).

## What reuses what

The scripts import `apps/desktop/e2e/fixtures.ts` (the `app` fixture: boots the app, records every IPC call, fails
a shot on an uncaught exception, a CSP violation, an unanswered command or an unreleased deferred answer),
`e2e/backend.ts` (the fake backend's answers, each taken from the Rust command's own body) and
`e2e/audio-fixtures.ts`. Nothing is copied from them, so the shots stay in step with the app.

| File | What it is |
|---|---|
| `lib/stage.ts` | The stage: frame-stepped capture, the pointer, typing, scrolling |
| `lib/kit.ts` | The in-page half: stops the page's clock, animations and media, and steps them |
| `lib/human.ts` | The pointer overlay (the capture is of the page, so it has no OS pointer) |
| `lib/studio.ts` | `Studio` (the session the Rust store would hold) and `agentTurn` (the scripted assistant) |
| `lib/music.ts` | The synthetic audio: drums, bass, pad, keys, lead, a formant "vocal", written from arithmetic |
| `lib/media.ts` | Turns the page's play/pause log into a WAV that lines up with the footage |
| `lib/history.ts`, `lib/orchestra.ts`, `lib/skills.ts` | A branching history, a 200-track session, the bundled skills |
| `sN-*.shot.ts` | One shot each |

## How a frame is made, and why

A capture at the device scale of 2 (3200x2000) takes 0.3 to 0.4 s on this machine, and `Page.startScreencast`
cannot give a 2x image (Chromium caps its frames at the page's size in CSS pixels: a 1600x1000 page at scale 2
delivers 1600x1000 frames). So the recording does not run in real time. After the app has booted the page's clock is
stopped, and each frame is made by advancing it by exactly 1/30 s and taking a picture
(`Page.captureScreenshot`, JPEG quality 95, 3200x2000). The page's JavaScript time (Playwright's fake clock), its CSS
and Web animations (paused and seeked each frame) and its media playback position (computed from the page clock; the
element still loads its file for real) all advance together, so the footage is 30 fps by construction, whatever else the
machine is doing. Nothing in the app changes: its own code, styles and IPC calls run as built, and only the clocks it
reads are the stepped ones. The encode is H.264 High, yuv420p, CRF 16, 30 fps, with no audio track.

A page that did not change between frames is not captured again: after a repeated frame the next ones take a cheap
1x look first and a 2x picture only if the page differs.

## What is scripted

The assistant. There is no API key, so `agentTurn` raises the events the real agent loop raises
(`agent://text-delta`, `tool-call`, `node-created`, `tool-call-end`, `done`, in the order `commands.rs` emits them) with
the tool names and arguments of `docs/tools-reference.md`. Everything the user does is real: typing into the composer,
key presses on a slider, clicks. See `../footage/README.md` for the shot-by-shot account and `../claims.md` for what each
shot may be said to show.
