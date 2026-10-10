# edytlab launch video: index

> **Licence.** The narrated files and the narration clips are not covered by the repository's MIT licence. Every `out/launch-*-narrated.mp4` and everything under `out/narration/` was generated with ElevenLabs ([elevenlabs.io](https://elevenlabs.io)) on its free plan. ElevenLabs' terms apply to them: "The free plan does not include a commercial license and cannot be used for any commercial purpose", and a published piece must credit "elevenlabs.io" or "11.ai" in its title ([source](https://elevenlabs.io/docs/help-center/legal/can-i-publish-the-content-i-generate-on-the-platform)). Regenerate the narration on a paid plan before any commercial use: `tools/narrate.py` rebuilds every clip from `out/narration/*/lines_*.json`, and `tools/audio.py` or `tools/assemble.sh` re-mixes them onto the picture-only masters. The plain cuts (`out/launch-linkedin.mp4`, `out/launch-reddit.mp4`, `out/launch-instagram.mp4`) and the masters in `out/picture-only/` contain only the app recordings from `website/public/demos/` and sounds synthesised in code (`tools/sfx.py`), with no ElevenLabs audio, and are covered by the MIT licence.

One launch video, cut three ways, built from the three real demo recordings in `website/public/demos/` (no simulated UI) with code-driven
motion design (kinetic titles, camera zooms onto the agent's chat and the waveform, animated callouts, one brand motion kit).

- `research.md`: the research, every claim with its page, quoted line and status, plus section M on motion design and what was learned from whom.
- `script.md`: the master script (scene by scene), the three platform cuts, and the **claim ledger** (every claim in the video and where it is true).
- `out/`: the videos, captions, contact sheets, picture-only masters and narration clips. `tools/`: the code that renders them. `work/` is a scratch directory the tools create (cue files, the synthesised sounds, QC stills); it is not committed.

## Read this first: ElevenLabs

The `-narrated` files use a voice generated on ElevenLabs' **free plan**. ElevenLabs' terms
(<https://elevenlabs.io/docs/help-center/legal/can-i-publish-the-content-i-generate-on-the-platform>): "The free plan does not include a
commercial license and cannot be used for any commercial purpose." and "you must attribute it to ElevenLabs by including "elevenlabs.io" or
"11.ai" in the title". So:

- Put **elevenlabs.io** (or 11.ai) in the title or first line of any post that uses a `-narrated` file.
- Whether promoting a free, open-source project is a "commercial purpose" is for the owner or ElevenLabs to say; this folder cannot settle it.
  The safe routes: post the un-narrated files (they contain **no** ElevenLabs audio), credit as above, or regenerate the lines on a paid plan
  (`tools/narrate.py gen`, about 700 characters per cut).
- Voice: Roger (`CwhRBWXzGAHq8TQ4Fs17`), model `eleven_v4`, stability 0.5, similarity 0.75, `mp3_44100_128`.

## The files (`out/`)

All H.264 High, yuv420p, 30 fps, with AAC 48 kHz stereo, normalised to -14 LUFS with the true peak under -1 dBTP (measured with ebur128 on the
finished files). Each platform has a version **without** narration (on-screen text carries it, safe to post now) and a `-narrated` version
(same picture, voice added). Each has an `.srt` and one contact sheet (the narrated version has the same picture).

| File | Platform | Frame | Length | Size | Loudness | What it is |
|---|---|---|---|---|---|---|
| `launch-linkedin.mp4` | LinkedIn feed | 1:1, 1080x1080 | 69.0 s | 12.9 MB | -14.1 LUFS / -1.4 dBTP | Master cut: founder hook, the chores, six numbered steps on real footage (the real audio), two more jobs, how it works, honest limits, end card |
| `launch-linkedin-narrated.mp4` | LinkedIn | 1:1 | 69.0 s | 13.9 MB | -14.0 LUFS / -1.5 dBTP | Same, with voice (needs the ElevenLabs credit) |
| `launch-reddit.mp4` | Reddit video post | 16:9, 1920x1080 | 63.4 s | 17.0 MB | -14.1 LUFS / -1.5 dBTP | Understated cut: plain first line, real recording from 4 s, no hook, limits on screen, end card = GitHub address |
| `launch-reddit-narrated.mp4` | Reddit | 16:9 | 63.4 s | 18.0 MB | -14.0 LUFS / -1.3 dBTP | Same, with voice |
| `launch-instagram.mp4` | Instagram Reels | 9:16, 1080x1920 | 31.4 s | 7.8 MB | -14.1 LUFS / -1.5 dBTP | Short cut: hook in frame 3, five steps, real audio, end card; inside Meta's safe zone |
| `launch-instagram-narrated.mp4` | Instagram | 9:16 | 31.4 s | 8.2 MB | -13.9 LUFS / -1.3 dBTP | Same, with voice |
| `launch-{linkedin,reddit,instagram}.srt` | | | | | | The burned-in words as captions (titles, lists, end card) |
| `launch-{linkedin,reddit,instagram}-narrated.srt` | | | | | | The spoken lines, timed to the voice |
| `launch-{linkedin,reddit,instagram}-contact.png` | | | | | | 16-22 frames across the cut (inspected, see "Checks") |
| `picture-only/launch-*-picture-only.mp4` | | | 36.6 MB in all | | no audio | The picture with no sound at all: re-mix it with your own voice or music (below) |
| `narration/{li,rd,ig}/` | | | | | | Every generated clip (two takes per line, more for the tight ones), the line list with timings, `narr_*.json` (the take chosen per line; its `file` paths are relative to `tools/`). The `usage.log` that `narrate.py gen` writes next to them is not committed (the repo ignores `*.log`); its totals are under "ElevenLabs credits" below |

Sound in the un-narrated files: the recording's own audio (only the "Hear it" playback is audible; it is the real playback of the app, in real
time), plus three synthesised UI sounds at low level (a soft whoosh on scene changes, a tick when a callout lands, a chime under the wordmark),
made in code by `tools/sfx.py`: no samples, no licence, no ElevenLabs audio.

## The cuts, in one line each (full scene tables in `script.md`)

- **LinkedIn 69 s:** hook (0-3) -> chores (3-10) -> 1 describe it, 2 the agent calls a tool, 3 it flags what it noticed, 4 the blend, 5 hear it (real audio), 6 master and export (10-45) -> two more jobs (44-52) -> how it works (52-58) -> honest limits (58-64) -> end card (64-69).
- **Reddit 63 s:** plain first line (0-4) -> the same six steps (4-39) -> two more jobs (38-46) -> how it works -> limits -> end card with the GitHub address (58-63).
- **Instagram 31 s:** hook (0-2) -> five steps incl. real audio (2-26) -> end card (26-31).

## Platform choices, and why (details and citations in `research.md`)

- **LinkedIn, 1:1:** LinkedIn lists 1:1, 4:5, 9:16 and 16:9 as supported; most viewers are on phones and muted ("Think like a silent film director: a large
  portion of LinkedIn members will watch your ad with the sound off." / "Consider burning in video subtitles." LinkedIn video ad tips), so the words are burned in; a square fills more
  of a phone feed than a 16:9 strip; the chat panel is near-square, so zooms fill the frame. No LinkedIn source prefers 1:1 over 4:5 for organic video: a judgement call.
- **Reddit, 16:9, understated:** Reddit's own creative study (as summarised by ppc.land) favours things shown "in their natural setting" over
  "a high-budget product shot"; overlay text and captions helped. Reddit's and every subreddit's own rules could not be read from here (all unverified).
- **Instagram, 9:16, 31 s:** "Make sure the first 3 seconds of your reel are engaging, so that people don't move on." (Instagram for Creators FAQ); Meta's published safe zone is "at least 14% of
  the top, 35% of the bottom, and 6% on each side" free of text and logos (an **ad** rule, the strictest published; the cut keeps everything in x 65-1015, y 269-1248).

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

1. **Narration and the ElevenLabs licence** (top of this file): post un-narrated, or narrated with "elevenlabs.io" in the title, or regenerate on a paid plan. I could not listen to any audio here;
   I measured it (loudness, true peak, no line overlaps the real app audio, every line fits its window) but the voice, its pacing and the synthesised sounds have **not been heard by a person**. Listen once before posting.
2. **An AI voice saying "I built ..."**: the LinkedIn narration is in first person but is not the owner's voice. Disclose it, or record the lines yourself (they are in `script.md` and `out/narration/li/lines_li.json`) and re-mix with `tools/audio.py`.
3. **The "I built an audio editor you talk to." hook** (LinkedIn): it is the opening of the owner's own post 1; keep it only if that is true for how the owner wants to present the project.
4. **LinkedIn shape**: 1:1 was chosen. `launch-reddit.mp4` (16:9) also works on LinkedIn desktop. Say if you want a 4:5 or 16:9 LinkedIn file; the engine renders any of them from the same script.
5. **"Demo tracks are short synthetic loops"** is on screen in the limits (and in the Instagram end card). It comes from the owner's own posts (now `marketing/social-kit/`); nothing in the app, the website or the docs states it. Confirm it is true.
6. **Reddit:** pick the community, re-read its rules the day you post (none could be read from here), check it allows video posts and self-promotion, and put the post text in the first comment (video posts have no body). Copy: `marketing/social-kit/reddit.md`.
7. **LinkedIn `.srt`:** the words are already burned in. Upload the `.srt` only for accessibility; LinkedIn then draws captions over the picture and the words appear twice.
8. **Instagram:** the safe zone uses Meta's ad numbers (no organic figure is published); check the in-app preview. The vertical-grid crop (3:4) was not confirmed from Instagram itself; the hook text sits inside it. Pick the cover frame at about 1.5 s.
9. **Agent name:** the recordings show no model name and the video says none. The existing posts say the agent in the recordings is Claude; keep that in the post text if you want it said.
10. **Music:** none is used. Adding trending audio on Instagram is your choice (it "can also impact distribution", but check the licence for a business account).
11. **Sign-off wording:** the end cards say "Free and open source", "installers are unsigned" (Reddit, Instagram) and ask "Which prep job would you hand off first?" (LinkedIn). Change any of it in `tools/launch.py` and re-render.

## Posting copy (existing, already checked against the same facts)

| Platform | File to post | Copy |
|---|---|---|
| LinkedIn | `out/launch-linkedin.mp4` | `marketing/social-kit/linkedin-posts.md`, **post 1** (launch); pin its comment. Swap the attachment from `reel-highlights.mp4` to this file. For the narrated file add "Voice: elevenlabs.io" to the post |
| Reddit | `out/launch-reddit.mp4` | `marketing/social-kit/reddit.md`: **r/DJs** (post 1) or **r/opensource** (post 4); body as the first comment; do not post two on one day (`calendar.md`) |
| Instagram | `out/launch-instagram.mp4` | `marketing/social-kit/instagram.md`, **caption 1** (the lead reel); for the narrated file put "elevenlabs.io" in the caption |

## ElevenLabs credits

- Characters sent for this task: **3,708** in 76 requests (7 lines on Instagram, 12 on LinkedIn, 11 on Reddit, two takes each, a few extra takes for tight lines, and re-written lines when a take ran too long). Voice only; no sound-effect generation (the sounds are synthesised in code so the un-narrated files stay free of ElevenLabs audio).
- Account counter from the API after the last request: **5,617 of 10,000 used** (free tier). The owner confirmed 8,091 remaining at hand-over (counter 1,909), so **4,383 remain**; the counter resets on 2026-11-09 (next reset time from the API).
- The key was read only from the environment inside the commands that used it and handed to curl on stdin; it is in no file, log or argument (searched the working folder for it before it was added to the repo: none; `narrate.py` reads `ELEVENLABS_API_KEY` from the environment and nothing else).

## Checks done

- Contact sheets inspected frame by frame at several sizes; fixes made from them: step titles at one size per canvas, callout regions re-measured on native frames with a 50 px grid (and keyframed where the chat scrolls), chip placement in the side gutters on 16:9, camera moves finished before callouts draw (step 4), the diagram's text auto-fits its panels, title overlaps removed at cuts, SRT cues no longer overlap.
- `tools/verify.py`: every file is H.264/AAC, 30 fps, correct frame size and duration, -13.9 to -14.1 LUFS and -1.3 to -1.5 dBTP; the real app audio sits inside its window (about -10 dB mean there, 20 dB or more quieter in the 3 s before: this check caught an ffmpeg `amix`/`adelay` offset that had put the playback up to about 10 s early in the first mixes, so `tools/audio.py` now sums the sources itself at exact sample offsets); no narration line overlaps the real app audio; each line fits its window (the first Instagram line is played at 1.1x to end before step 1); in the narrated files the voice and the real playback measure within about 2 dB of each other.
- Content: every number or claim on screen is in the claim ledger (`script.md`) with its source; nothing shows or says stem separation or transcription working (they are named only as **not** there); no signed-build claim (the limits and the end cards say unsigned); no user, download or review numbers; no model names.
- Not done: no listening test, no test in the apps themselves (LinkedIn, Reddit and Instagram uploaders), no read of any subreddit's live rules.

## Unverified (from `research.md`, section E)

Reddit's own video limits and every subreddit's current rules; Reddiquette's wording; any first-party evidence that founder-voice framing helps on LinkedIn; Instagram's organic safe-zone numbers and its 3:4 grid crop; Product Hunt's video guidance; Remotion's licence terms (not used); that Instagram's Help Center and Reels page disagree on maximum length (they do).

## Re-render, re-narrate, re-mix (all offline)

Needs Python 3.12 or older (`audio.py` uses `audioop`, removed in 3.13) with Pillow, ffmpeg and ffprobe on the PATH, the Liberation and DejaVu fonts (`/usr/share/fonts/truetype/`), and the three recordings in `website/public/demos/` (set `EDYTLAB_DEMO_DIR` to read them from somewhere else). Run everything from `marketing/launch-video/tools/`. `work/` is created on demand and is git-ignored.

```
cd marketing/launch-video/tools
python3 -I sfx.py ../work/sfx                         # the three synthesised UI sounds (assemble.sh does this when they are missing)
python3 -I launch.py <li|rd|ig> still 5 12 30        # contact sheet of exact frames -> ../work/qc_<key>.png
python3 -I launch.py <li|rd|ig> render out.mp4       # silent picture (about 5-10 min per cut on a shared machine)
python3 -I launch.py <li|rd|ig> cues ../work/cues_<key>.json
python3 -I audio.py mix ../work/cues_<key>.json out.mp4 final.mp4 [--narration narr.json] [--no-sfx]
python3 -I mksrt.py ../work/cues_<key>.json final.srt
./assemble.sh                                        # rebuild the mp4s, plain .srt and contact sheets in out/ (overwrites them; needs out/picture-only/); ./assemble.sh --help
python3 -I verify.py                                 # after assemble.sh: index rows and the narration-vs-real-audio check
# narration (spends ElevenLabs characters; paid plan for commercial use). gen skips takes whose .mp3 exists, so use a fresh folder:
#   set -a; . <env file>; set +a; python3 -I narrate.py gen ../out/narration/li/lines_li.json ../work/narr/li 2
#   python3 -I narrate.py pick ../out/narration/li/lines_li.json ../work/narr/li ../work/narr/narr_li.json
#   python3 -I audio.py mix ../work/cues_li.json ../out/picture-only/launch-linkedin-picture-only.mp4 final.mp4 --narration ../work/narr/narr_li.json
```

To use your own voice: record the lines, write `narr.json` as `[{"t": seconds, "file": "line.wav", "gain_db": 0}, ...]` (times are in `out/narration/*/lines_*.json`) and run `audio.py mix` on a `picture-only` file.
`tools/grid.py` makes a gridded native frame of a demo for placing callouts. `work/sfx/` holds the three synthesised sounds (`tools/sfx.py` regenerates them).
`narrate.py gen` also writes a word-timing `.json` per take next to each `.mp3`; `narrate.py pick` needs them. The committed `out/narration/` folders do not hold them (the committed `narr_*.json` already records the take chosen per line and its `speech_end`), so `pick` works on freshly generated takes, as above.
