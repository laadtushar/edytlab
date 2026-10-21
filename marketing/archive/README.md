# Archive: every earlier version of the launch video, the reels and the demos

Nothing made for edytlab's marketing is thrown away. When a video, its captions, contact sheet, cue file or narration timing is replaced or removed, the earlier file is kept here, sorted by what it is, then by version, then by kind.

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
└── demos/
    ├── 2026-10-09-first-takes/       the first two website demos, video and poster (#417, #432)
    └── 2026-10-09-v0.4.0/            the three DJ demos recorded on v0.4.0, video and poster (#459), replaced by #581
```

## Where each file came from

Every file except the two drafts is the exact file a PR committed to `main` (the same git blob), so keeping them here adds nothing to the repository's history.

| Folder | Landed in | Replaced or removed by | Lengths and frames |
|---|---|---|---|
| `launch-video/2026-10-10-v0.4.0-amber/` | #493 (2026-10-10) | #598 (2026-10-16) | LinkedIn 1:1 1080x1080 1:09; Reddit 16:9 1920x1080 1:03; Instagram 9:16 1080x1920 0:31 |
| `launch-video/2026-10-16-instagram-draft/` | never committed | the committed `launch-instagram.mp4` | 9:16, 0:31.5. The re-filmed footage in the new look, nearly frame for frame the committed cut; rendered while #598 was made |
| `reels/2026-10-10-v0.4.0-amber/` | #485 (2026-10-10); `tools/map-captions.mjs` is the last version, from #537 (2026-10-11), and `map-captions.pr485.mjs` the first | #590 (2026-10-13) | Reels 1080x1920: highlights 1:02, teaser 0:14, beatmatched transition 0:55, extended club intro 0:40, mini mix 0:44. Full demos 1280x776: 3:08, 1:56, 2:18 |
| `demos/2026-10-09-first-takes/` | #417 (beatmatched transition, 2:58), #432 (extended club intro, 2:26) | #459 (same day) | 1280x776 |
| `demos/2026-10-09-v0.4.0/` | #459 (2026-10-09) | #581 (2026-10-13, re-filmed in the new look) | 1280x776: beatmatched transition 3:08, extended club intro 1:56, mini mix 2:18 |

Earlier versions of the documents and scripts in `marketing/` (the READMEs, `script.md`, the Python tools) were edited in place rather than replaced, so they are not copied here; `git log -p -- marketing/` shows every version.

## Adding to the archive

When a PR replaces or removes a video, caption, contact sheet, cue file or timing file in `marketing/` or `website/public/demos/`, move the old one here in the same PR: `<what>/<date it landed>-<version or look>/<kind>/`, and add its row above.
