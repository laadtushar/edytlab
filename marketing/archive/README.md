# Archive: every earlier version of the launch video, the reels, the demos, the v0.7.0 promo, the LinkedIn renders and the brand screenshots

Nothing made for edytlab's marketing is thrown away. When a video, its captions, contact sheet, cue file, narration timing or a rendered image or PDF is replaced or removed, the earlier file is kept here, sorted by what it is, then by version, then by kind.

> **Licence: do not post the narrated files.** Every file with `narrated` in its name carries audio generated with ElevenLabs ([elevenlabs.io](https://elevenlabs.io)) on its free plan: the voice, and in the narrated reels also the whoosh, hit and ding effects (`marketing/reels/sfx/`). ElevenLabs' terms: "The free plan does not include a commercial license and cannot be used for any commercial purpose", and a published piece must credit "elevenlabs.io" or "11.ai" in its title ([source](https://elevenlabs.io/docs/help-center/legal/can-i-publish-the-content-i-generate-on-the-platform)). The files without `narrated` have no ElevenLabs audio (the PRs that added them say so) and are covered by the repository's MIT licence.

> **They show an interface that no longer exists.** Everything dated 2026-10-09 and 2026-10-10 was recorded in the app's old amber look, on v0.4.0 or the builds just before it, and the launch cuts end on a `v0.4.0` card. The versions to post, in the app's violet look, are in `website/public/demos/`, `marketing/reels/` and `marketing/launch-video/out/`.

## Layout

```
archive/
├── launch-video/
│   ├── 2026-10-10-v0.4.0-amber/      first launch video (#493), replaced by #598
│   │   ├── plain/                    launch-{linkedin,reddit,instagram}.mp4 and their .srt
│   │   ├── narrated/                 the same three cuts with the ElevenLabs voice, and their .srt
│   │   ├── picture-only/             the three silent masters
│   │   └── contact-sheets/           one frame grid per plain cut
│   ├── 2026-10-24-limits-slide/      the LinkedIn and Reddit cuts that end with the "Honest limits" slide, replaced when the owner removed it
│   │   ├── plain/                    launch-{linkedin,reddit}.mp4
│   │   ├── captions/                 their .srt
│   │   ├── picture-only/             the two silent masters
│   │   └── contact-sheets/           one frame grid per cut
│   ├── 2026-10-16-violet/            the LinkedIn and Reddit cuts that said "93 tools" (#598), replaced for v0.7.0
│   │   ├── plain/                    launch-{linkedin,reddit}.mp4
│   │   ├── captions/                 their .srt
│   │   ├── picture-only/             the two silent masters
│   │   └── contact-sheets/           one frame grid per cut
│   └── 2026-10-16-instagram-draft/   a draft of today's Instagram cut, with and without sound
├── reels/
│   └── 2026-10-10-v0.4.0-amber/      first reels (#485), replaced by #590
│       ├── plain/                    reel-{highlights,teaser,beatmatched-transition,extended-club-intro,mini-mix}.mp4
│       ├── narrated/                 the same reels narrated, a Spanish highlights reel, and their .srt
│       ├── full-demos-narrated/      the three full demos narrated (1280x776), and their .srt
│       ├── contact-sheets/           one frame grid per reel, plain and narrated
│       ├── cues/                     the caption cue files the reels were cut from
│       ├── narration-timing/         when each narration line plays, per narrated video
│       └── tools/                    the caption-mapping script and its mapping, removed in #590
├── demos/
│   ├── 2026-10-09-first-takes/       the first two website demos, video and poster (#417, #432)
│   └── 2026-10-09-v0.4.0/            the three DJ demos recorded on v0.4.0, video and poster (#459), replaced by #581
├── demos-narration/
│   └── 2026-10-24-f-major-line/      the beatmatch demo's narrated mixes and the BT04 line that said both tracks were in F major (#686)
│       ├── narrated/                 daniel/ and river/: dj-beatmatched-transition-narrated-*.mp4
│       ├── voice-tracks/             narration-dj-beatmatched-transition-daniel.wav
│       ├── lines/                    daniel/ and river/: bt04.mp3
│       └── checks/                   metrics-dj-beatmatched-transition-{daniel,river}.json
├── linkedin/
│   ├── 2026-10-11-v0.5.0/            the LinkedIn renders that carried "v0.5.0" and "93 tools" (#578), replaced for v0.7.0
│   │   └── out/                      post-1-launch.png, post-3-model-never-touches-a-sample.png, post-3-carousel.pdf
│   ├── 2026-10-22-v0.7.0-six-providers/   the renders and the Settings screenshot that said "6 providers" or listed six, replaced for v0.9.0
│   │   ├── out/                      post-1-launch.png, post-3-carousel.pdf, post-4-privacy.png
│   │   └── shots/                    ollama.png
│   ├── 2026-10-25-v0.9.0-vector-carousel/   the carousel PDF printed from HTML, whose gradient headlines some viewers did not paint (#643)
│   │   └── out/                      post-3-carousel.pdf
│   ├── 2026-10-25-v0.9.0/            the renders that said "v0.9.0", replaced for v0.9.1
│   │   └── out/                      post-1-launch.png, post-3-carousel.pdf
│   ├── 2026-10-26-v0.9.1-chatgpt-logo/   the Settings screenshot and post 4 render drawn before the ChatGPT logo (#749), replaced for v0.9.1
│   │   ├── out/                      post-4-privacy.png
│   │   └── shots/                    ollama.png
│   └── 2026-10-26-v0.9.1/            the renders that said "v0.9.1", replaced for v0.9.2
│       └── out/                      post-1-launch.png, post-3-carousel.pdf
├── docs-brand/
│   └── 2026-10-11-violet-six-providers/   the Settings screenshot of the brand pair (#515), replaced for v0.9.0
│                                     after-06-settings-dialog.png
└── promo-v0.7.0/
    ├── 2026-10-24-issue-badges/      the live promo's 16:9, 1:1 and What's new renders with issue-number chips, and the narrated mixes made on them
    │   ├── out/                      the three videos, their .srt captions and their contact sheets
    │   └── narration/                daniel/ (16:9, 1:1, What's new) and river/ (16:9): the narrated mixes
    └── 2026-10-22-scripted/          the v0.7.0 promo made with a scripted assistant (#644, #649), replaced by the live one
        ├── footage/                  the nine clips (s1 to s8b), their two .wav files and the .marks.json / .notes.json beside each
        ├── out/                      the five videos, their .srt captions and their contact sheets
        └── record/                   the Playwright scripts that recorded the footage against the fake backend (they import
                                      paths that no longer exist from here; kept as they were)
```

## Where each file came from

Every file except the two drafts is the exact file a PR committed to `main` (the same git blob), so keeping them here adds nothing to the repository's history.

| Folder | Landed in | Replaced or removed by | Lengths and frames |
|---|---|---|---|
| `launch-video/2026-10-10-v0.4.0-amber/` | #493 (2026-10-10) | #598 (2026-10-16) | LinkedIn 1:1 1080x1080 1:09; Reddit 16:9 1920x1080 1:03; Instagram 9:16 1080x1920 0:31 |
| `launch-video/2026-10-16-violet/` | #598 (2026-10-16) | the tool-count re-render for v0.7.0 (2026-10-22, `launch.py` says 94, `docs/tools-reference.md`) | LinkedIn 1:1 1080x1080 1:09 and Reddit 16:9 1920x1080 1:03, each as the plain cut, its `.srt`, its contact sheet and its silent master. They are the current violet look, kept only for the number: the how-it-works card and the `.srt` said "93 tools", the new files say 94 and are the same pictures and the same sound otherwise. The Instagram cut shows no count, so it was not replaced |
| `launch-video/2026-10-24-limits-slide/` | the tool-count re-render for v0.7.0 (2026-10-22; the files it replaced are in `launch-video/2026-10-16-violet/`) | the "Honest limits" slide's removal (2026-10-24: the owner removed the slide, which had also gone stale, since transcription shipped in v0.8.0) | LinkedIn 1:1 1080x1080 1:09 and Reddit 16:9 1920x1080 1:03, each as the plain cut, its `.srt`, its contact sheet and its silent master. They show 94 tools and the four-row "Honest limits" slide (5.3 s) between the how-it-works card and the end card; the new files are the same pictures and sound without the slide (64.0 s and 58.4 s). The Instagram cut has no such slide, so it was not replaced |
| `launch-video/2026-10-16-instagram-draft/` | never committed | the committed `launch-instagram.mp4` | 9:16, 0:31.5. The re-filmed footage in the new look, nearly frame for frame the committed cut; rendered while #598 was made |
| `reels/2026-10-10-v0.4.0-amber/` | #485 (2026-10-10); `tools/map-captions.mjs` is the last version, from #537 (2026-10-11), and `map-captions.pr485.mjs` the first | #590 (2026-10-13) | Reels 1080x1920: highlights 1:02, teaser 0:14, beatmatched transition 0:55, extended club intro 0:40, mini mix 0:44. Full demos 1280x776: 3:08, 1:56, 2:18 |
| `demos/2026-10-09-first-takes/` | #417 (beatmatched transition, 2:58), #432 (extended club intro, 2:26) | #459 (same day) | 1280x776 |
| `demos/2026-10-09-v0.4.0/` | #459 (2026-10-09) | #581 (2026-10-13, re-filmed in the new look) | 1280x776: beatmatched transition 3:08, extended club intro 1:56, mini mix 2:18 |
| `demos-narration/2026-10-24-f-major-line/` | #686 (2026-10-24) | #710 (2026-10-26): the demo's recording shows, and BT04 said aloud, that both tracks are in F major, which is not the key of the tracks (the current detector reads A major and B major). They come back, voiced again, when the demo is re-recorded | `narrated/`: the 1280x776 narrated demo with Daniel and with River (165.3 s each; the picture is the demo's own, so it shows the old reply on screen); `voice-tracks/`: Daniel's voice alone; `lines/`: the two clips of BT04 as ElevenLabs returned them; `checks/`: the loudness and true-peak measurements of the two mixes. The voice and its licence are those described in `demos/narration/README.md` |
| `linkedin/2026-10-11-v0.5.0/out/` | #578 (2026-10-11) | the v0.7.0 staleness sweep (2026-10-22), re-rendered by `social-kit/linkedin/render.mjs` | 1200x1200 PNGs (post 1 and the post 3 square) and the seven-page 1080x1350 carousel PDF. They said "v0.5.0" (post 1 and the carousel's last page) and "93 tools" (all three); the new renders say v0.7.0 and 94 tools. The other three squares (posts 2, 4 and 5) carry neither number, so they were not replaced |

| `promo-v0.7.0/2026-10-22-scripted/` | #644 (footage and edit) and #649 (2026-10-22) | the live promo (2026-10-22): the same five videos re-cut from the released v0.7.0 app driven by real Claude, with the note "real Claude (Sonnet 5.5)" or "recorded live" in place of "scripted assistant" | `out/`: whats-new 9:16 0:31 (8.9 MB), describe-the-edit 9:16 0:21, 200-tracks 9:16 0:21, 16:9 0:58, 1:1 0:58, each with its `.srt` and a 20-frame contact sheet. `footage/`: nine clips, 3200x2000 at 30 fps, 38 MB. Every frame of app UI in them was the real v0.7.0 frontend in Chromium with the Rust backend replaced by a scripted one, and the assistant's replies and tool calls were scripted; the audio was synthetic |
| `promo-v0.7.0/2026-10-24-issue-badges/` | #665 (the live promo) and #685 (the narrated mixes), 2026-10-22 and 2026-10-24 | the issue-number badges' removal (2026-10-24: issue-number badges removed, since viewers did not know what `#596`, `#602`, `#618`, `#574` and `#607` meant) | `out/`: the 16:9 (1920x1080, 1:16, 16.2 MB) and 1:1 (1080x1080, 1:16, 11.1 MB) cuts, each with a corner chip carrying a number on five shots, and What's new 9:16 (1080x1920, 0:39, 10.7 MB, "(#596)" in its faders line), with their `.srt` and contact sheets. `narration/`: Daniel's mixes of those three and River's 16:9 mix. The new files are the same pictures and the same sound (the audio track is bit-identical, so the voice tracks and the loudness and timing checks in `narration/checks/` are unchanged) without the numbers. Describe the edit and 200 tracks had none and rendered byte-for-byte the same, so they were not replaced. These narrated files carry ElevenLabs audio: see the licence note at the top and `promo-v0.7.0/narration/README.md` |
| `linkedin/2026-10-22-v0.7.0-six-providers/` | `out/post-1-launch.png` and `out/post-3-carousel.pdf`: the v0.7.0 sweep (2026-10-22), which replaced the files in `linkedin/2026-10-11-v0.5.0/`. `out/post-4-privacy.png` and `shots/ollama.png`: #578 (2026-10-11), unchanged since | the v0.9.0 staleness sweep (2026-10-25), re-rendered by `social-kit/linkedin/render.mjs` (and `--shots` for `ollama.png`) | 1200x1200 PNGs (post 1, post 4) and the seven-page 1080x1350 carousel PDF, and the 1440x663 Settings crop (the new one is 1440x795). The stat said "6 providers" (post 1 and the carousel's last page), post 4's alt text said "six providers", and the Settings crop shows six provider buttons, where v0.9.0 has a seventh, ChatGPT plan (#703). The new files say "6 API providers" and "or use your ChatGPT plan" (the wording of #704), v0.9.0 where they name a version, and show the seven buttons. Post 1 and the carousel also said v0.7.0 |
| `linkedin/2026-10-25-v0.9.0-vector-carousel/` | `out/post-3-carousel.pdf`: the v0.9.0 staleness sweep (#709, 2026-10-25) | the same seven pages, built from page images by `social-kit/linkedin/render.mjs` (#643) | Seven 1080x1350 pages printed from the HTML, with selectable text, 5,156 KB. The gradient headline halves were SVG text with a gradient fill, which PDFium left out and poppler drew as solid magenta boxes. The new PDF is one 2x JPEG per page, 2,691 KB, and draws the same in both |
| `linkedin/2026-10-25-v0.9.0/` | `out/post-1-launch.png`: the v0.9.0 staleness sweep (#709, 2026-10-25). `out/post-3-carousel.pdf`: #725 (2026-10-25), the image-page build that replaced `linkedin/2026-10-25-v0.9.0-vector-carousel/` | the v0.9.1 staleness sweep (2026-10-25), re-rendered by `social-kit/linkedin/render.mjs` (no `--shots`: the app's UI did not change) | 1200x1200 PNG (post 1, 576 KB) and the seven-page 1080x1350 carousel PDF (2691 KB). Post 1's pill said "v0.9.0 · open source" and the carousel's last page "v0.9.0 for macOS, Windows and Linux"; the new files say v0.9.1 and are the same pictures otherwise, with the same file sizes to the kilobyte. The other four renders were not replaced: posts 3 (the square) and 4 render byte for byte the same, and posts 2 and 5 differ only in a few pixels of text rasterisation (the committed files came from a Windows render), so they stayed as committed |
| `linkedin/2026-10-26-v0.9.1-chatgpt-logo/` | `out/post-4-privacy.png` and `shots/ollama.png`: the v0.9.0 staleness sweep (#709, 2026-10-25), which had added the ChatGPT plan button | the UI-screenshot sweep of 2026-10-26 (the ChatGPT logo that #749 put on the button), re-rendered by `social-kit/linkedin/render.mjs` after retaking `shots/ollama.png` with `--shots` | 1200x1200 PNG (post 4) and the 1440x795 Settings crop. The crop and the square drew the ChatGPT plan button without OpenAI's logo; the new files draw it with the logo and are the same picture otherwise (the crop differs from the old one only inside that button, checked pixel by pixel). The other four `shots/` files retook as the same picture (anti-aliasing only; none of them shows the message box) and the other four renders and the carousel were not replaced |
| `linkedin/2026-10-26-v0.9.1/` | `out/post-1-launch.png` and `out/post-3-carousel.pdf`: the v0.9.1 staleness sweep (2026-10-25), which replaced the files in `linkedin/2026-10-25-v0.9.0/` | the v0.9.2 release sweep (2026-10-26), re-rendered by `social-kit/linkedin/render.mjs` with the version at 0.9.2 in the working tree only (the release bump was still in flight) | 1200x1200 PNG (post 1, 576 KB) and the seven-page 1080x1350 carousel PDF (2691 KB). Post 1's pill said "v0.9.1 · open source" and the carousel's last page "v0.9.1 for macOS, Windows and Linux"; the new files say v0.9.2 and are the same pictures otherwise, with the same file sizes to the kilobyte |
| `docs-brand/2026-10-11-violet-six-providers/` | #515 (2026-10-11) | the v0.9.0 staleness sweep (2026-10-25), retaken from the production frontend with the IPC faked (1440x900 window, scaled to 1080x675 as the others are) | 1080x675. The Settings dialog of the "after" set, with six provider buttons and no ONNX Runtime line; the new file has the seventh button (ChatGPT plan) and the Account tab's ONNX Runtime line (answered "bundled" by the fake backend). The other six `after-*` files and the `before-*` set were not touched |

Earlier versions of the documents and scripts in `marketing/` (the READMEs, `script.md`, the Python tools) were edited in place rather than replaced, so they are not copied here; `git log -p -- marketing/` shows every version.

## Adding to the archive

When a PR replaces or removes a video, caption, contact sheet, cue file, timing file, or a rendered image or PDF in `marketing/` or `website/public/demos/`, move the old one here in the same PR: `<what>/<date it landed>-<version or look>/<kind>/`, and add its row above.
