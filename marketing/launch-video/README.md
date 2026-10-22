# edytlab launch video: index

> **Licence.** Everything under `out/narration/` was generated with ElevenLabs ([elevenlabs.io](https://elevenlabs.io)) on its free plan, and ElevenLabs' terms apply to it: "The free plan does not include a commercial license and cannot be used for any commercial purpose", and a published piece must credit "elevenlabs.io" or "11.ai" in its title ([source](https://elevenlabs.io/docs/help-center/legal/can-i-publish-the-content-i-generate-on-the-platform)). Those clips belong to the earlier narrated cuts, which are kept only in [`../archive/`](../archive/README.md) (below); `tools/narrate.py` rebuilds clips from `out/narration/*/lines_*.json` on a paid plan. The cuts that are published here (`out/launch-linkedin.mp4`, `out/launch-reddit.mp4`, `out/launch-instagram.mp4`) and the masters in `out/picture-only/` contain only the app recordings from `website/public/demos/` and sounds synthesised in code (`tools/sfx.py`), with no ElevenLabs audio, and are covered by the MIT licence.

> **Status, 2026-10-16.** Re-cut from the re-filmed demos (#581: the app's violet look, recorded on a development build between v0.5.0 and v0.6.0), in the website's look (`marketing/reels/tools/brand.py`, shared with the reels). Every source time was re-measured on the new videos (`tools/launch.py`; the caption and sound windows are the reels', `marketing/reels/tools/spec.py`). **The end cards name no version.** The footage matches v0.6.0, published the same day: it has the release's violet look, and the limiter it puts on the master bus is a v0.6.0 feature. Each shot still carries the note "Development build", which is true of the recording and harmless; the cuts were not re-rendered for the release (the 2026-10-22 re-render, below, changed the tool count and nothing else). The earlier cuts (amber UI, end card `v0.4.0`) and their narrated versions are kept for reference, with their `.srt` files and contact sheets, in [`marketing/archive/launch-video/2026-10-10-v0.4.0-amber/`](../archive/README.md), not here. **There are no narrated cuts for now:** the old ones showed an interface that no longer exists, and no ElevenLabs key is available to voice new ones. `out/narration/` (the clips, the line lists and the chosen takes) and the lines in `script.md` stay as the source for re-narrating, and **were written for the earlier takes**: the re-filmed agent says different things at different times, so check every line against the new footage and re-time it before use (`narr_*.json` and `lines_*.json` hold the old cuts' times).
>
> **Update, 2026-10-22 (v0.7.0 is current): the LinkedIn and Reddit cuts were re-rendered for 94 tools.** They said "93 tools" in the how-it-works card (the step title and the diagram) and in their `.srt` files: the count when they were cut. It is 94 now (`list_session`, #634, in v0.7.0; `docs/tools-reference.md`: "Every one of the 94 tools the AI agent can call"), so `tools/launch.py` (`common_tail`) now says 94 and those two cuts, their picture-only masters, `.srt` files and contact sheets were rebuilt from it. **The Instagram cut shows no count, so it and its files were left as they were, byte for byte.** The "93 tools" files they replace are kept, unchanged, in [`marketing/archive/launch-video/2026-10-16-violet/`](../archive/README.md). Nothing else changed: the lengths are the same (69.3 s, 63.7 s); every frame outside the how-it-works card is the same picture (bit for bit on the title, list and end-card stretches, and 44 dB PSNR or better against the old file on the footage, which is the video encoder's rounding); each soundtrack decodes to the same samples as before (same MD5 of the decoded audio), so the loudness numbers below stand, and were measured again on the new files; the `.srt` files differ from the old ones in one line each. No version is named, "no stem separation or transcription yet" is still true (v0.7.0's installers carry the ONNX Runtime, but no model ships and nothing runs one), and the installers are still unsigned. **Re-narrating:** the earlier narration clip for this card (`out/narration/li/L10_t*.mp3`, with `lines_li.json` and `narr_li.json`) says "Ninety-three" and is left as recorded; `script.md`'s line says ninety-four, so record that one again.

One launch video, cut three ways, built from the three real demo recordings in `website/public/demos/` (no simulated UI) with code-driven
motion design (kinetic titles, camera zooms onto the agent's chat and the waveform, animated callouts, one brand motion kit).

- `research.md`: the research, every claim with its page, quoted line and status, plus section M on motion design and what was learned from whom.
- `script.md`: the master script (scene by scene), the three platform cuts, and the **claim ledger** (every claim in the video and where it is true).
- `out/`: the videos, captions, contact sheets, picture-only masters and narration clips. `tools/`: the code that renders them. `work/` is a scratch directory the tools create (cue files, the synthesised sounds, QC stills); it is not committed.

## Read this first: ElevenLabs (only when re-narrating)

The published cuts have no voice. The clips in `out/narration/` use a voice generated on ElevenLabs' **free plan**. ElevenLabs' terms
(<https://elevenlabs.io/docs/help-center/legal/can-i-publish-the-content-i-generate-on-the-platform>): "The free plan does not include a
commercial license and cannot be used for any commercial purpose." and "you must attribute it to ElevenLabs by including "elevenlabs.io" or
"11.ai" in the title". So, when a narrated cut comes back:

- Put **elevenlabs.io** (or 11.ai) in the title or first line of any post that uses a file with that voice.
- Whether promoting a free, open-source project is a "commercial purpose" is for the owner or ElevenLabs to say; this folder cannot settle it.
  The safe routes: post the plain files (they contain **no** ElevenLabs audio), credit as above, or regenerate the lines on a paid plan
  (`tools/narrate.py gen`, about 700 characters per cut).
- Voice: Roger (`CwhRBWXzGAHq8TQ4Fs17`), model `eleven_v4`, stability 0.5, similarity 0.75, `mp3_44100_128`.

## The files (`out/`)

All H.264 High, yuv420p, 30 fps, with AAC 48 kHz stereo, normalised to -14 LUFS (measured -14.1 to -14.2) with the true peak under -1 dBTP (measured on the finished AAC files with ebur128: `tools/audio.py` re-encodes until the file itself, not just its source, is under -1 dBTP). There are **no narrated cuts** (see the status note at the top): the on-screen text carries the story, so the plain files are safe to post as they are. Each cut has an `.srt` and one contact sheet.

| File | Platform | Frame | Length | Size | Loudness | What it is |
|---|---|---|---|---|---|---|
| `launch-linkedin.mp4` | LinkedIn feed | 1:1, 1080x1080 | 69.3 s | 9.7 MB | -14.2 LUFS / -1.2 dBTP | Master cut: founder hook, the chores, six numbered steps on real footage (the real audio), two more jobs, how it works, honest limits, end card |
| `launch-reddit.mp4` | Reddit video post | 16:9, 1920x1080 | 63.7 s | 12.4 MB | -14.2 LUFS / -1.7 dBTP | Understated cut: plain first line, real recording from 4 s, no hook, limits on screen, end card = GitHub address |
| `launch-instagram.mp4` | Instagram Reels | 9:16, 1080x1920 | 31.5 s | 5.4 MB | -14.1 LUFS / -1.5 dBTP | Short cut: hook in frame 3, five steps, real audio, end card; every word inside Meta's safe zone |
| `launch-{linkedin,reddit,instagram}.srt` | | | | | | The burned-in words as captions (titles, lists, end card) |
| `launch-{linkedin,reddit,instagram}-contact.png` | | | | | | 16-25 frames across the cut (inspected, see "Checks") |
| `picture-only/launch-*-picture-only.mp4` | | | 26.4 MB in all | | no audio | The picture with no sound at all: re-mix it with your own voice or music (below) |
| `narration/{li,rd,ig}/` | | | | | | Every clip generated for the earlier cuts (two takes per line, more for the tight ones), the line list with timings, `narr_*.json` (the take chosen per line; its `file` paths are relative to `tools/`). **For the earlier takes**: the lines and their times need re-checking before use (top of this file). The `usage.log` that `narrate.py gen` writes next to them is not committed (the repo ignores `*.log`); its totals are under "ElevenLabs credits" below |

Sound in the cuts: the recording's own audio (only the "Hear it" playback is audible; it is the real playback of the app, in real
time, and the shot starts 0.26 s before the sound does and ends inside it), plus three synthesised UI sounds at low level (a soft whoosh on scene changes, a tick when a callout lands, a chime under the wordmark),
made in code by `tools/sfx.py`: no samples, no licence, no ElevenLabs audio. The earlier cuts, `launch-*-narrated.mp4` among them, are in [`marketing/archive/`](../archive/README.md), for reference only.

## The cuts, in one line each (full scene tables in `script.md`)

- **LinkedIn 69 s:** hook (0-3.5) -> chores (3.5-10.4) -> 1 describe it, 2 the agent calls a tool, 3 it flags what it noticed, 4 the blend, 5 hear it (real audio), 6 master and export (9.9-44.9) -> two more jobs (44.6-52.3) -> how it works (52.2-58.9) -> honest limits (58.8-64.2) -> end card (64.2-69.3).
- **Reddit 64 s:** plain first line (0-4.4) -> the same six steps (4.0-39.0) -> two more jobs (38.7-46.4) -> how it works -> limits -> end card with the GitHub address (58.3-63.7).
- **Instagram 31.5 s:** hook (0-2.5) -> five steps incl. real audio (2.2-26.3) -> end card (26.1-31.5).

## Platform choices, and why (details and citations in `research.md`)

- **LinkedIn, 1:1:** LinkedIn lists 1:1, 4:5, 9:16 and 16:9 as supported; most viewers are on phones and muted ("Think like a silent film director: a large
  portion of LinkedIn members will watch your ad with the sound off." / "Consider burning in video subtitles." LinkedIn video ad tips), so the words are burned in; a square fills more
  of a phone feed than a 16:9 strip; the chat panel is near-square, so zooms fill the frame. No LinkedIn source prefers 1:1 over 4:5 for organic video: a judgement call.
- **Reddit, 16:9, understated:** Reddit's own creative study (as summarised by ppc.land) favours things shown "in their natural setting" over
  "a high-budget product shot"; overlay text and captions helped. Reddit's and every subreddit's own rules could not be read from here (all unverified).
- **Instagram, 9:16, 31.5 s:** "Make sure the first 3 seconds of your reel are engaging, so that people don't move on." (Instagram for Creators FAQ); Meta's published safe zone is "at least 14% of
  the top, 35% of the bottom, and 6% on each side" free of text and logos (an **ad** rule, the strictest published). Every word the cut adds sits in x 120-960 and y 269-1248: Meta's top and bottom, and the reels' 120 px at each side (the right edge is where Reels, Shorts and TikTok put their buttons). `engine.py` asserts it for every title, chip, list, note and card line when the cut is built; the recording and the progress line carry no words and may use the rest.

## Motion design: what was learned from whom (full table with quoted lines: `research.md`, section M)

| Technique in the cut | Learned from |
|---|---|
| Enter on emphasized decelerate `cubic-bezier(0.05, 0.7, 0.1, 1)`, leave on emphasized accelerate `(0.3, 0, 0.8, 0.15)`, standard `(0.2, 0, 0, 1)`; duration grows with distance | Material Components motion docs <https://github.com/material-components/material-components-android/blob/master/docs/theming/Motion.md> |
| Camera moves on easeInOutCubic ("By default, property tweens use easeInOutCubic as the timing function."); springs follow Hooke's law | Motion Canvas <https://motioncanvas.io/docs/tweening/> |
| Damped spring (zeta 0.78) for chips: "To disable the default bounce, increase the damping parameter." | Remotion <https://www.remotion.dev/docs/spring> |
| Zoom to where the action is ("focuses on the areas where clicks occurred") | Screen Studio <https://screen.studio/guide/auto-zoom> |
| Zoom and pan "to draw attention to key actions"; callouts, arrows and one theme "for a consistent look, style, or brand across videos" | TechSmith Camtasia <https://www.techsmith.com/learn/tutorials/camtasia/animations/>, <https://www.techsmith.com/learn/tutorials/camtasia/annotations/> |
| Anticipation (the camera arrives before the ring draws) and staging (one emphasised thing per shot) | Twelve basic principles of animation <https://en.wikipedia.org/wiki/Twelve_basic_principles_of_animation> |
| Words on screen: two lines at most, at most 20 characters per second | Netflix timed-text style guide <https://partnerhelp.netflixstudios.com/hc/en-us/articles/217350977-English-Timed-Text-Style-Guide> |
| No flashing, readable contrast | WCAG 2.2 criteria 2.3.1 and 1.4.3 <https://www.w3.org/WAI/WCAG22/Understanding/three-flashes-or-below-threshold.html>, <https://www.w3.org/WAI/WCAG22/Understanding/contrast-minimum.html> |
| Kinetic type: few animated things at once, test muted (vendor blogs, low weight) | <https://hera.video/blog/kinetic-typography-video-generator-guide>, <https://www.nemovideo.com/blog/kinetic-typography-product-ads> |

**Tooling:** Remotion and Motion Canvas need a headless Chromium that is not installed here (hundreds of MB on a 6 GB disk), and Remotion's licence terms
for for-profit use could not be confirmed, so the same ideas run offline in `tools/engine.py` (Python + Pillow + ffmpeg, nothing downloaded): a frame is a pure function of time,
the camera interpolates a crop of the recording and the frame it sits in together, callout regions are in recording pixels so they follow the camera (and are
keyframed where the recorded chat scrolls).

## Decisions the owner must make before posting

1. **Listen once.** The three UI sounds and the mix levels have **not been heard by a person**: I measured (loudness, true peak, the real playback sits inside its window and is quiet in the 3 s before: `tools/verify.py`) but could not listen. The plain cuts have no voice, so there is no AI-voice or ElevenLabs question to settle before posting them.
2. **Narration, later.** If you want a voice: re-check `script.md`'s lines against the new footage (they were written for the earlier takes), time them to the new cuts, regenerate on a paid plan or accept the free-plan credit (top of this file), and mix with `tools/audio.py`. A first-person line on LinkedIn ("I built ...") in an AI voice should be disclosed, or recorded by you.
3. **The "I built an audio editor you talk to." hook** (LinkedIn): it is the opening of the owner's own post 1; keep it only if that is true for how the owner wants to present the project.
4. **LinkedIn shape**: 1:1 was chosen. `launch-reddit.mp4` (16:9) also works on LinkedIn desktop. Say if you want a 4:5 or 16:9 LinkedIn file; the engine renders any of them from the same script.
5. **"Demo tracks are short synthetic loops"** is on screen in the limits (and in the Instagram end card). It comes from the owner's own posts (now `marketing/social-kit/`); nothing in the app, the website or the docs states it. Confirm it is true.
6. **Reddit:** pick the community, re-read its rules the day you post (none could be read from here), check it allows video posts and self-promotion, and put the post text in the first comment (video posts have no body). Copy: `marketing/social-kit/reddit.md`.
7. **LinkedIn `.srt`:** the words are already burned in. Upload the `.srt` only for accessibility; LinkedIn then draws captions over the picture and the words appear twice.
8. **Instagram:** the safe zone uses Meta's ad numbers (no organic figure is published); check the in-app preview. The vertical-grid crop (3:4) was not confirmed from Instagram itself; the hook text sits inside it. Pick the cover frame at about 1.5 s.
9. **Agent name:** the recordings show no model name and the video says none. The existing posts say the agent in the recordings is Claude; keep that in the post text if you want it said.
10. **Music:** none is used. Adding trending audio on Instagram is your choice (it "can also impact distribution", but check the licence for a business account).
11. **Version and release:** the cuts name no version. The footage was recorded on a development build before v0.6.0 and matches v0.6.0: the same violet look, and the master-bus limiter in step 6 is in the release. Post them as what v0.6.0 and later (v0.7.0 is the current release) do, with the demo copy in `marketing/social-kit/more.md`, and the tool count on screen is 94, as in v0.7.0 (re-rendered on 2026-10-22: see the update note at the top). The "Development build" note under each shot is a known, harmless mismatch with the release (it is true of the recording); the 2026-10-22 re-render left it on purpose, since it changed the count and nothing else. If you want the version on the end card, add it in `tools/launch.py` (`end_card`) and re-render.
12. **Sign-off wording:** the end cards say "Free, open source", "installers are unsigned" (Reddit, Instagram) and ask "Which prep job would you hand off first?" (LinkedIn). Change any of it in `tools/launch.py` and re-render.

## Posting copy (existing, already checked against the same facts)

| Platform | File to post | Copy |
|---|---|---|
| LinkedIn | `out/launch-linkedin.mp4` | `marketing/social-kit/linkedin-posts.md`, **post 1** (launch); pin its comment. Swap the attachment from `reel-highlights.mp4` to this file |
| Reddit | `out/launch-reddit.mp4` | `marketing/social-kit/reddit.md`: **r/DJs** (post 1) or **r/opensource** (post 4); body as the first comment; do not post two on one day (`calendar.md`) |
| Instagram | `out/launch-instagram.mp4` | `marketing/social-kit/instagram.md`, **caption 1** (the lead reel) |

## ElevenLabs credits

- Characters sent for the earlier clips (the ones in `out/narration/`): **3,708** in 76 requests (7 lines on Instagram, 12 on LinkedIn, 11 on Reddit, two takes each, a few extra takes for tight lines, and re-written lines when a take ran too long). Voice only; no sound-effect generation (the sounds are synthesised in code so the un-narrated files stay free of ElevenLabs audio).
- Account counter from the API after the last request: **5,617 of 10,000 used** (free tier). The owner confirmed 8,091 remaining at hand-over (counter 1,909), so **4,383 remain**; the counter resets on 2026-11-09 (next reset time from the API).
- The key was read only from the environment inside the commands that used it and handed to curl on stdin; it is in no file, log or argument (searched the working folder for it before it was added to the repo: none; `narrate.py` reads `ELEVENLABS_API_KEY` from the environment and nothing else).

## Checks done

- Contact sheets of exact frames inspected frame by frame (`launch.py <key> still ...`, and again on the finished files). Fixes made from them: every crop stays above the recording's own caption card (y 684), callout regions re-measured on native frames with a 50 px grid and keyframed where the chat scrolls (the tool chip, the request bubble, the three chips), each shot cut inside the stretch where the chat holds still, the playhead ring follows the measured playhead (15.6 px/s), the ring is shortened on the square and tall cuts so its chip does not land on the title or the note, no two callouts overlap.
- Source times: each shot is asserted by `launch.py` to lie inside the burned-in caption it is cut from (caption starts re-detected with `marketing/reels/tools/detect_captions.py` and matching the reels' `spec.py`), a silent shot never overlaps the app's sound, and the Listen shot starts 0.26 s before the sound (86.07 s against the sound's 86.33 s) and ends inside it (95.27 s; the sound runs to 102.90 s). Windows re-checked with `silencedetect -55 dB, d=0.5`.
- Brand and contrast: `brand.py` fails the import under 4.5:1 for any of its text colours; `engine.py` also checks every title, list, note, diagram label, lockup and end-card line against the brightest pixel of the background behind it and refuses to build under 4.5:1 (the gradient's darker end counts for stressed words), and asserts that each of them lies inside its layout's safe area (the tall one: x 120-960, y 269-1248). The reels' pictures are byte-identical to before the shared drawing moved into `brand.py` (hash-compared: background, card background and the mark at 84 and 150 px).
- `tools/verify.py`: every file is H.264/AAC, 30 fps, correct frame size and duration, -14.1 to -14.2 LUFS and -1.2 to -1.7 dBTP (the first LinkedIn mix came out of the AAC encoder at -0.4 dBTP, so `audio.py` now measures the file and re-encodes under -1); the real app audio sits inside its window (-11 to -12 dB mean there, 25 dB or more quieter in the 3 s before).
- Re-render for 94 tools, 2026-10-22: the two pictures were rebuilt with `launch.py <li|rd> render`, mixed with `audio.py mix`, and given their `.srt` and contact sheet by `mksrt.py` and `sheet.py`: the lines of `assemble.sh` run for those two keys only (`assemble.sh` itself rebuilds all three cuts, and the Instagram cut had nothing to change). The old and new pictures were compared frame by frame (ffmpeg `psnr`): the frames outside the how-it-works card are bit-identical or at 44 dB or better, the card's frames sit at 32 dB (two glyphs changed), nothing moved in time. Each soundtrack decodes to the same MD5 as the file it replaces. Loudness, measured on the finished AAC files with `ebur128=peak=true` as `audio.py` does: LinkedIn -14.2 LUFS / -1.2 dBTP, Reddit -14.2 LUFS / -1.7 dBTP (the table's numbers). `verify.py`: the real-audio windows are OK. The Instagram plan builds, so `engine.py`'s `assert_safe` (Meta's safe zone, x 120-960, y 269-1248) still passes for every title, chip, list, note and card line; its files are the committed ones. Eight stills of each cut were looked at, the 94 frame at full size.
- Content: every number or claim on screen is in the claim ledger (`script.md`) with its source on the new footage (the tool count was 93 when the cuts were made on 2026-10-16, and is 94 in the LinkedIn and Reddit cuts re-rendered on 2026-10-22: see the update note at the top); nothing shows or says stem separation or transcription working (they are named only as **not** there); no signed-build claim (the limits and the end cards say unsigned); no user, download or review numbers; no model names; **no version number**.
- Not done: no listening test, no test in the apps themselves (LinkedIn, Reddit and Instagram uploaders), no read of any subreddit's live rules, no narration.

## Unverified (from `research.md`, section E)

Reddit's own video limits and every subreddit's current rules; Reddiquette's wording; any first-party evidence that founder-voice framing helps on LinkedIn; Instagram's organic safe-zone numbers and its 3:4 grid crop; Product Hunt's video guidance; Remotion's licence terms (not used); that Instagram's Help Center and Reels page disagree on maximum length (they do).

## Re-render, re-narrate, re-mix (all offline)

Needs Python 3.12 or older (`audio.py` uses `audioop`, removed in 3.13) with Pillow (with its raqm support, which the wheels have, so Inter is kerned), ffmpeg and ffprobe on the PATH, Inter (Debian/Ubuntu's `fonts-inter`, in `/usr/share/fonts/opentype/inter`; `EDYTLAB_REEL_FONT_DIR` points at another copy, as for the reels), and the three recordings in `website/public/demos/` (set `EDYTLAB_DEMO_DIR` to read them from somewhere else). The look comes from `marketing/reels/tools/brand.py`, which reads `website/app/globals.css` and `website/public/logo.svg`; the caption and sound windows come from `marketing/reels/tools/spec.py`. Run everything from `marketing/launch-video/tools/`. `work/` is created on demand and is git-ignored.

```
cd marketing/launch-video/tools
mkdir -p ../work/sfx && python3 -I sfx.py ../work/sfx   # the three synthesised UI sounds (assemble.sh does this when they are missing; sfx.py needs the folder to exist)
python3 -I launch.py <li|rd|ig> still 5 12 30        # contact sheet of exact frames -> ../work/qc_<key>.png
python3 -I launch.py <li|rd|ig> render out.mp4       # silent picture (about 5-15 min per cut on a shared machine)
python3 -I launch.py <li|rd|ig> cues ../work/cues_<key>.json
python3 -I audio.py mix ../work/cues_<key>.json out.mp4 final.mp4 [--no-sfx]
python3 -I mksrt.py ../work/cues_<key>.json final.srt
./assemble.sh                                        # rebuild the mp4s, .srt files and contact sheets in out/ (overwrites them; needs out/picture-only/); ./assemble.sh --help
# assemble.sh does not render the pictures: `launch.py <key> render` writes each one first. To change one or two cuts only, run its per-cut lines for those keys,
# and move the files they replace into marketing/archive/ (README there) before overwriting them.
python3 -I verify.py                                 # after assemble.sh: the index rows and the real-audio-window check
```

To re-cut after the demos are re-filmed again: `marketing/reels/tools/detect_captions.py` and `silencedetect` for the caption and sound windows (put them in the reels' `spec.py`), `tools/grid.py` (a gridded native frame of a demo, in source pixels) to place the regions in `launch.py`'s `REG` and each callout, `launch.py <key> still ...` to check, then `render`, `assemble.sh`, `verify.py`. `Plan.clip` refuses a shot outside its caption's window, a silent shot that would let the app's sound in, and a Listen shot that does not start on the sound.

### Re-narrating (not done: no ElevenLabs key)

```
# the lines in ../out/narration/*/lines_*.json and script.md were written for the earlier takes: check them against the new footage, re-time them to
# `launch.py <key> cues` (its "scenes" give each section's start and end), and write the new lines_*.json first. gen skips takes whose .mp3 exists, so use a fresh folder:
#   set -a; . <env file>; set +a; python3 -I narrate.py gen ../out/narration/li/lines_li.json ../work/narr/li 2
#   python3 -I narrate.py pick ../out/narration/li/lines_li.json ../work/narr/li ../work/narr/narr_li.json
#   python3 -I audio.py mix ../work/cues_li.json ../out/picture-only/launch-linkedin-picture-only.mp4 final.mp4 --narration ../work/narr/narr_li.json
```

To use your own voice: record the lines, write `narr.json` as `[{"t": seconds, "file": "line.wav", "gain_db": 0}, ...]` and run `audio.py mix` on a `picture-only` file. The committed `narr_*.json` and `lines_*.json` carry the earlier cuts' times, so they are the format to copy, not a timing to reuse.
`narrate.py gen` also writes a word-timing `.json` per take next to each `.mp3`; `narrate.py pick` needs them. The committed `out/narration/` folders do not hold them (the committed `narr_*.json` already records the take chosen per line and its `speech_end`), so `pick` works on freshly generated takes, as above.
