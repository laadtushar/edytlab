# edytlab v0.7.0 promo: the Remotion edit

Five finished videos for the v0.7.0 release, cut with [Remotion](https://www.remotion.dev) from the screen recordings in
[`footage/`](footage/README.md) (phase 1, #644). Everything is in this folder: the footage and the scripts that recorded it
(`footage/`, `record/`), the claim ledger ([`claims.md`](claims.md)), the Remotion project that edits it
([`remotion/`](remotion/)) and the finished files ([`out/`](out/)).

## Honesty, in one paragraph

Every frame of app UI is the real v0.7.0 frontend (commit `23bbfe2e`), recorded in Chromium with only the IPC boundary to the
Rust backend replaced. **The assistant in the footage is scripted**: there was no API key, so the replies and the tool calls
are raised by the recording (`record/lib/studio.ts`), using the real tool names and arguments. **The audio is synthetic**,
written from arithmetic, and the footage never says it is anyone's music. Remotion draws only titles, chips, callout rings,
labels, a "speed" badge, a progress line and the end card; it draws no app UI. **Every shot of app footage carries the note
"v0.7.0 · real app · scripted assistant".** The videos do not say that stem separation or transcription works (they do not:
no model ships and no inference runs), they say the installers are unsigned, and the one line about the ONNX Runtime says
"groundwork only: no model yet". Every on-screen claim has a row in [`claims.md`](claims.md) naming a PR, a doc or a test.

## The files (`out/`)

All H.264 High, yuv420p, 30 fps, AAC 48 kHz stereo, mastered to -14 LUFS integrated with a true peak under -1 dBTP, **measured
on the finished AAC files** with ebur128 (`remotion/tools/verify.py` re-measures them and fails if any number is off). Each
video has an `.srt` of its on-screen words (titles, labels, chips, the end card and the note, split wherever the set of
words changes) and a 20-frame contact sheet.

| File in `out/` | Platform | Frame | Length | Size | Loudness (final AAC) |
|---|---|---|---|---|---|
| `edytlab-v0.7.0-whats-new-9x16.mp4` | Instagram Reels, TikTok, YouTube Shorts | 1080x1920 (9:16) | 30.6 s | 8.9 MB | -14.0 LUFS / -1.4 dBTP |
| `edytlab-v0.7.0-describe-the-edit-9x16.mp4` | Instagram Reels, TikTok, YouTube Shorts | 1080x1920 (9:16) | 21.0 s | 6.2 MB | -13.9 LUFS / -1.4 dBTP |
| `edytlab-v0.7.0-200-tracks-9x16.mp4` | Instagram Reels, TikTok, YouTube Shorts | 1080x1920 (9:16) | 20.7 s | 6.4 MB | -13.9 LUFS / -1.2 dBTP |
| `edytlab-v0.7.0-16x9.mp4` | YouTube, X, Reddit | 1920x1080 (16:9) | 57.8 s | 14.8 MB | -14.1 LUFS / -1.2 dBTP |
| `edytlab-v0.7.0-1x1.mp4` | LinkedIn | 1080x1080 (1:1) | 57.8 s | 9.7 MB | -14.1 LUFS / -1.2 dBTP |

The five videos total 45.9 MB; with the `.srt` files and contact sheets, `out/` is 52.0 MB. Beside each video: `<name>.srt` (the on-screen words) and `<name>-contact.png` (20 frames).

| Composition | What it is |
|---|---|
| `whats-new-9x16` (R1) | "What's new in v0.7.0": hook in the first 2 s, then five changes in the order faders (#596), history graph (#602), 200 tracks (#618), the mix (#607), skills (#574), then the end card. The S7 playback is the app's own (synthetic) sound |
| `describe-the-edit-9x16` (R2) | "Describe the edit", from S1: type it, the assistant calls `fade` and `set_track_gain`, the timeline changes, press play. The sound is S1's own |
| `200-tracks-9x16` (R3) | "200 tracks? Ask.", from S4: the request, `list_session` paging, twelve `set_track_gain` calls, the strings at -4.0 dB. No app audio exists for S4, so the sound is UI sounds only (a tick for each chip, rising for the twelve) |
| `16x9` (V1) | "edytlab v0.7.0" for YouTube, X and Reddit: S8a b-roll hook, S1, S2, S8b + S3, S4, S5 ("The ONNX Runtime now ships in every installer", chip "groundwork only: no model yet"), S6, S7, end card |
| `1x1` (V1sq) | The same cut and timeline as V1 in 1080x1080 (LinkedIn): the titles break onto two lines, the camera frames each shot for the squarer card |

9:16 layout: **every word the edit adds, and the whole footage card with the app's own words in it, lies inside Meta's published
ad safe zone**: none in the top 14%, the bottom 35% or 6% at each side (x 65 to 1015, y 269 to 1248 of 1080x1920). That is
asserted in code, not by eye: see "How it is checked". The bottom third carries only the aurora and a thin progress line, which
have no words.

## Licence: Remotion's Free License covers this

**Read before using, and before upgrading Remotion.** Remotion is not MIT. It is "source-available software, but it is not
open source software" ([FAQ](https://www.remotion.dev/docs/license/faq)), under its own licence.
<https://www.remotion.dev/license> redirects to
[`LICENSE.md`](https://github.com/remotion-dev/remotion/blob/main/LICENSE.md) in the Remotion repository (the docs page,
<https://www.remotion.dev/docs/license>, links to the same text and the [FAQ](https://www.remotion.dev/docs/license/faq)). Read on
2026-10-22. What it says:

> Individuals and small companies are allowed to use Remotion to create videos for free (even commercial), while a company
> license is required for for-profit organizations of a certain size.

> You are eligible to use Remotion for free if you are: an individual; a for-profit organization with up to 3 employees; a
> non-profit or not-for-profit organization; evaluating whether Remotion is a good fit, and are not yet using it in a
> commercial way.

> Permission is hereby granted, free of charge, to any person eligible for the "Free License", to use the software
> non-commercially or commercially for the purpose of creating videos and images and to modify the software to their own
> liking [...]

> It is not allowed to copy or modify Remotion code for the purpose of selling, renting, licensing, relicensing, or
> sublicensing your own derivate of Remotion.

The FAQ adds that the Free License is for "an individual, whether for personal or commercial use", and on telemetry: "For
server-side rendering, there is no automatic telemetry" (this project renders with the CLI and `@remotion/renderer`, which
is server-side; the telemetry the FAQ describes for client-side rendering applies to the Studio and Player, which the
renders do not use).

**Finding.** edytlab is an individual's MIT open-source project: the repository `laadtushar/edytlab` belongs to a personal
GitHub account (owner type `User`, not an organisation), and the promo is made by that individual to promote the release. That
is the Free License's first category, and the use (rendering promotional videos) is the use it grants. **Why it matters in
practice:**

- This folder holds our own code and the videos it renders, which are covered by the repository's MIT licence. It does **not**
  contain Remotion's code: `remotion/node_modules` is not committed, and `package.json` pins the packages from npm, so each
  person who renders installs Remotion under its own licence.
- If edytlab ever becomes a for-profit organisation of more than three people, the Free License stops covering it and a Company
  License is needed (<https://www.remotion.pro/license>). Whoever re-renders should be an individual, or have that licence.
- The LICENSE.md says "In Remotion 5.0, the license will slightly change", so the versions are pinned (`4.0.534`, released
  2026-10-07) and the licence should be read again before any upgrade.
- This is a reading of the published text by the person who made the edit, not legal advice. The owner should confirm that the
  GitHub account is theirs as an individual.

Other licences: Inter comes from `@fontsource/inter` (SIL Open Font License 1.1, bundled, no network at render time); the sound
effects are made by `marketing/launch-video/tools/sfx.py` (code, no samples); the footage and the audio in it are synthetic and
ours. There is no ElevenLabs audio and no stock music anywhere in this folder.

## Re-rendering

```bash
cd marketing/promo-v0.7.0/remotion
pnpm install --ignore-workspace        # standalone, like website/: not part of the pnpm workspace
tools/build.sh                         # all five; or: tools/build.sh R1 V1   (ids: R1 R2 R3 V1 V1sq)
```

You need Node 22, ffmpeg, Python 3.12 or older (the mixer uses `audioop`) with Pillow (the contact sheets), and a Chromium.
Remotion v4 wants its headless shell and downloads one on first use; where that download is blocked, set
`REMOTION_BROWSER_EXECUTABLE` to a `chrome-headless-shell` (Playwright's `chromium_headless_shell-*/chrome-linux/headless_shell`
works; `remotion.config.ts` looks for `/opt/pw-browsers/...` by default). The pinned versions are `remotion`, `@remotion/cli` and
`@remotion/renderer` 4.0.534 (published 2026-10-07), `@fontsource/inter` 5.2.8 and React 18.3.1 (`pnpm-lock.yaml` fixes the rest).
Rendering is about 5,400 frames at roughly 0.4 to 0.6 s a frame on four cores: **about 50 minutes for all five in one process
(`tools/build.sh`), about 30 with two `node scripts/render.ts` processes side by side** (R1 R3 R2, and V1 V1sq; render, then
`python3 tools/audio.py` each). `NO_BUNDLE=1 tools/build.sh R2` reuses an existing `work/bundle`. The output folder is `../out`.
`work/` (bundle, silent renders, mixes) and `node_modules` are git-ignored: delete them when done.

Two things that bit while making these, both machine limits rather than bugs in the edit: the Remotion compositor keeps decoded
3200x2000 frames and, left to itself, takes half the machine's memory, so two renders at once were OOM-killed (`scripts/render.ts`
now caps it at 1 GB; `REMOTION_VIDEO_CACHE_BYTES` changes that); and Chromium refused to load footage frames ("disk space is low")
with about 4.9 GB of disk free and three render tabs open, so keep a few GB free.

Single steps: `pnpm tokens:check` (the colours are generated from `website/app/globals.css`), `pnpm typecheck`,
`node scripts/cues.ts` (the sound cues and the `.srt` files), `node scripts/render.ts R1` (a silent near-lossless render into
`work/`), `python3 tools/audio.py work/cues/R1.json work/R1.silent.mp4 out.mp4` (sound, mastering, the final encode),
`node scripts/stills.ts R1 /some/dir 3 8 14` (one PNG per time, for looking), `pnpm studio` (the Remotion Studio).

## How it is built (`remotion/`)

| Piece | Where |
|---|---|
| The cuts: which stretch of which clip, at what speed, with which words. Pure data, read by the renderer **and** by the sound and `.srt` export | `src/cuts.ts`, `src/timeline.ts` |
| The direction of each clip: where the camera looks and what is circled and when, in the clip's own time, so any stretch can be cut with it | `src/shots.ts` |
| One brand module: the website's colours (generated from `website/app/globals.css` by `scripts/tokens.ts`, which also fails if a text colour is under 4.5:1 contrast), Inter, the words the edit repeats | `src/brand.ts`, `src/tokens.generated.ts` |
| The look of `marketing/reels/tools/brand.py`: the site's two-wash aurora behind the page, the film grain, the mark from `website/public/logo.svg`, the gradient headline | `components/Background.tsx`, `Logo.tsx`, `KineticTitle.tsx` |
| Components: kinetic title, chip, callout ring with label, the honesty note, the virtual camera over a clip, the end card | `src/components/` |
| The virtual camera: a focus box in the app window's CSS pixels, fitted to each layout's card (zoom capped at 3 card pixels per CSS pixel, the file has 2), eased between keyframes | `src/camera.ts` |
| Layouts and safe areas for 9:16, 16:9 and 1:1 | `src/layout.ts` |
| Sound: the app's own audio at its marked moments, and UI sounds from `marketing/launch-video/tools/sfx.py` (whoosh, tick, chime); `loudnorm` in two passes, the final AAC measured | `scripts/cues.ts`, `tools/audio.py` |
| Contact sheets and the output checks | `tools/sheet.py`, `tools/verify.py` |

## How it is checked

- **Safe zone, in code.** Each block of words the edit adds (titles, sub lines, labels, chips, the note, the end card's lines) is
  measured by the browser that draws the frame (`components/SafeCheck.tsx`) and must lie inside the layout's safe area; a line
  that overflows its box fails too. A violation throws and the render stops. `layout.ts` also asserts at import that every static
  box, the footage card included, lies inside its safe area and that the 9:16 area is Meta's numbers.
- **Sound.** `scripts/cues.ts` fails if a stretch that does not play a clip's own sound has any in it (a speed-up of S1 or S7
  would otherwise change the pitch). `tools/audio.py` measures the finished AAC, not its source, and lowers the true-peak target
  and re-encodes until the file itself is under -1 dBTP.
- **Claims.** [`claims.md`](claims.md) has a row for every word the edit puts on screen. No row says stem separation or
  transcription works.
- **Looking.** Stills from every output were extracted and looked at (legibility at phone size, nothing clipped, safe zones, the
  note on every app shot, spelling); what was found and fixed is in the pull request.

## Not done, and known limits

- The assistant is scripted and the audio synthetic (above). Nothing here shows a model doing a job.
- The edit does not say how long a real assistant takes; waits for it are not in the footage, and the one speed-up (R1's
  `list_session` stretch, V1's) is labelled with a "N× speed" badge.
- The renders are not signed off by a person watching and listening: the checks are the measurements above and stills.
- If the footage is re-taken (`record/README.md`), the camera boxes and ring rectangles in `src/shots.ts` are in the app
  window's CSS pixels and the moments are in the clip's seconds; re-check them against the new clip.
