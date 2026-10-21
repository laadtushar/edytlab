# Archive: every earlier cut of the launch video, the reels and the demos

> **Licence: do not post the narrated files.** Every `*-narrated.mp4` here carries audio generated with ElevenLabs ([elevenlabs.io](https://elevenlabs.io)) on its free plan: the voice, and in the narrated reels also the whoosh, hit and ding effects (`marketing/reels/sfx/`). ElevenLabs' terms: "The free plan does not include a commercial license and cannot be used for any commercial purpose", and a published piece must credit "elevenlabs.io" or "11.ai" in its title ([source](https://elevenlabs.io/docs/help-center/legal/can-i-publish-the-content-i-generate-on-the-platform)). The files without `-narrated` have no ElevenLabs audio (the PRs that added them say so) and are covered by the repository's MIT licence.

> **They show an interface that no longer exists.** Everything dated 2026-10-09 and 2026-10-10 was recorded in the app's old amber look, on v0.4.0 or the builds just before it, and the launch cuts end on a `v0.4.0` card. The current versions, in the app's violet look, are in `website/public/demos/`, `marketing/reels/` and `marketing/launch-video/out/`. Post those.

Each folder is named after the day the files landed on `main` and the PR that landed them. Every file except the two working renders is the exact file that PR committed (the same git blob, so restoring them added nothing to the repository's history). Later PRs replaced them in place, or deleted them.

| Folder | What it holds | Landed in | Replaced by |
|---|---|---|---|
| `2026-10-09-pr417/demos/` | The first beatmatch-and-blend DJ demo, 1280x776, 2:58 | #417 (demo videos section) | #459 |
| `2026-10-09-pr432/demos/` | The first extended-intro DJ demo, with sound, 1280x776, 2:26 | #432 | #459 |
| `2026-10-09-pr459/demos/` | The three DJ demos recorded on v0.4.0, with sound, 1280x776: beatmatched transition 3:08, extended club intro 1:56, mini mix 2:18 | #459 | #581 (re-filmed in the new look) |
| `2026-10-10-pr485/reels/` | The first reels, 1080x1920, plain and narrated: highlights (1:02, plus a Spanish narration), teaser (0:14), beatmatched transition (0:55), extended club intro (0:40), mini mix (0:44), and the three full demos narrated (`demo-dj-*-narrated.mp4`, 1280x776) | #485 | #590 (plain reels re-cut; narrated ones deleted) |
| `2026-10-10-pr493/launch-video/` | The first launch video, plain and narrated: LinkedIn 1:1 (1:09), Reddit 16:9 (1:03), Instagram 9:16 (0:31), and the three silent picture-only masters | #493 | #598 (re-cut; narrated ones deleted) |
| `2026-10-16-working-renders/launch-video/` | An earlier draft of today's Instagram cut, with and without sound, 1080x1920, 0:31.5. It is the re-filmed footage in the new look, nearly frame for frame the committed cut. It was rendered while #598 was made and never committed before | never landed | the committed `launch-instagram.mp4` |

The `.srt` captions, contact sheets and narration timings of these cuts are not restored; they are in git history at the commits above.
