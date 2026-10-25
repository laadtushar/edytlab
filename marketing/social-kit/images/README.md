# Social images

Six themes, each at seven canvas sizes: 42 PNGs in [`out/`](out/), for the
platforms below, and one caption per platform for each theme in
[`captions.md`](captions.md). The look is the website's, through the LinkedIn
kit's pipeline (`../linkedin/tokens.mjs`, `base.css`, `lint.mjs`): violet on
zinc, Inter, the gradient headline, the aurora wash, the cards.

**Every app frame is a frame of one of the site's own videos** (or of the v0.7.0
video, which the site served until v0.9.1): the real app,
and where the assistant is in the shot, real Claude. Nothing is scripted or
staged. The LinkedIn kit's `post-1` and `post-2` use a scripted example
conversation, so none of their screenshots is used here. Where a frame is shown
the image carries the videos' own disclosure as a small label: "Real app, real
Claude" (the assistant is in the shot) or "Real app, recorded live" (it is not).

## The themes

| # | Headline | Frame from |
|---|---|---|
| 1 | Describe the edit. | the typed request and the two edits it made (Fade, Gain) |
| 2 | A DJ transition in one sentence. | the blend request: Fade, Low-pass filter, Fade |
| 3 | Extend a club intro. | the request to repeat the first 8 bars: Copy, Paste |
| 4 | Your audio stays on your computer. | a three-track session with the chat beside it |
| 5 | Every edit is a version. | the history graph, with its branches |
| 6 | Free and open source. | the chat's list of Gain edits beside the lanes of a large session (MIT, 94 tools, macOS · Windows · Linux, the repo) |

## Sizes and platforms

`out/<theme>__<canvas>.png`, for example `out/2-dj-transition-in-one-sentence__x-1600x900.png`.

| Canvas id | Size (px) | Platforms |
|---|---|---|
| `square-1080x1080` | 1080 x 1080 | Instagram, Facebook, Threads (square) |
| `portrait-1080x1350` | 1080 x 1350 | Instagram, Threads (portrait) |
| `story-1080x1920` | 1080 x 1920 | Stories, Reels, TikTok cover (no text in the top 260 px or the bottom 350 px) |
| `x-1600x900` | 1600 x 900 | X, Bluesky |
| `linkedin-1200x627` | 1200 x 627 | LinkedIn, Facebook link |
| `youtube-1280x720` | 1280 x 720 | YouTube thumbnail |
| `pinterest-1000x1500` | 1000 x 1500 | Pinterest |

Each size has its own layout: a stack (brand, headline, line, frame, footer)
on the square and portrait canvases, a text column beside the frame on the
wide ones, and a poster layout on the two tall ones (Story and Pinterest: the
headline is fitted to the width, the frame takes the middle, and on the Story
the label and URL sit just above the 350 px the platforms cover). The frame is
cropped per canvas (`frames/tN-chat.png` for every canvas but Pinterest, which
uses the narrower `frames/tN-pin.png`); nothing is resized, recoloured or drawn
on, except theme 4's two small tags. Largest PNG: about 450 KB (no `pngquant`
on the machine; the aurora is rendered without film grain, as in the LinkedIn
kit, which keeps the files small).

## Which video and time each frame came from

Videos are in `website/public/demos/` and `marketing/promo-v0.7.0/out/`. Crops are
in pixels of the video; [`extract-frames.sh`](extract-frames.sh) holds every
number. Each crop starts in a clear gap above a line of the chat (found by
scanning the pixels), so no message is cut through, and none includes the mouse
pointer, the video's own speed badge, title or caption, or the status bar with
the app's version.

| Theme | Video | At | Why that moment |
|---|---|---|---|
| 1 | `promo-v0.7.0/out/edytlab-v0.7.0-16x9.mp4` (16:9, 1920x1080; the site served it as `videos/whats-new-v070.mp4` until v0.9.1) | 11 s | the request typed into the chat, then the Fade and Gain edits; its own "Describe the edit" moment, 6.8 s to 13 s |
| 2 | `demos/dj-beatmatched-transition.mp4` (1280x776) | 84 s | Claude's reply after the blend (the filter, the bars still a beat apart); the "Blend" step starts at 59.6 s and the pointer has left the chat |
| 3 | `demos/dj-extended-club-intro.mp4` | 52 s | Claude's summary of the 16-bar intro (new length, where the music comes in); the "Extend the intro" step starts at 35.4 s |
| 4 | `demos/dj-mini-mix.mp4` | 72 s | Claude's reply on loudness and the limiter, with a lane beside it ("Match loudness, limit the master" starts at 46.7 s) |
| 5 | `promo-v0.7.0/out/edytlab-v0.7.0-16x9.mp4` | 28.5 s | the history graph, in the graph moment that starts at 25.967 s |
| 6 | `promo-v0.7.0/out/edytlab-v0.7.0-16x9.mp4` | 44 s | the chat's run of Gain edits beside the lanes, in the 200-track moment (36.6 s to 48.3 s); the chat and the lanes are both in the frame, so "Real app, real Claude" is true of it |

## What the images claim

Taken from `README.md`, `marketing/press-kit.md`, `website/app/privacy/page.tsx`
and `CLAUDE.md`.

| Claim on an image | Source |
|---|---|
| Free and open source, MIT | `README.md` license badge, `LICENSE` |
| macOS, Windows and Linux; the installers are unsigned | `README.md` ("The installers are unsigned", #386) |
| Audio is edited on your computer; only the chat goes to the provider you choose | `website/app/privacy/page.tsx` ("Network traffic": chat requests carry prompt text, agent context and tool results, never raw audio) |
| Ollama runs fully local, with no key | `website/app/privacy/page.tsx` ("Ollama for a local model, which needs no key") |
| 94 tools | read at render time from `docs/tools-reference.md` (`toolCount()` in `../linkedin/tokens.mjs`), the same source as the site's tool count |
| Every edit is a node in a history graph; go back to any version | `README.md`, `docs/architecture.md`; the frame is the real History graph |
| The DJ steps (beatmatch, crossfade with a low-pass filter on the outgoing track; 8 bars become 16, a high-pass holds the low end until the drop) | the demos' captions in `website/lib/demos.ts` and the videos' own moments in `website/lib/videos.ts` |
| "Real app, real Claude" / "Real app, recorded live" | the videos' own disclosures (`disclosure` in `website/lib/videos.ts`; `marketing/promo-v0.7.0/claims.md`) |

Said nowhere: a version number, a user count, a testimonial, a benchmark, a
superlative, another product's name, or anything that has not shipped.

## Re-rendering

```
node marketing/social-kit/images/render.mjs                    # all 42
node marketing/social-kit/images/render.mjs --only 4           # one theme (its number)
node marketing/social-kit/images/render.mjs --only story       # one canvas (id prefix)
node marketing/social-kit/images/render.mjs --sheets <dir>     # also one contact sheet per theme
bash marketing/social-kit/images/extract-frames.sh             # retake the frames (needs ffmpeg)
```

It needs Node 22, Playwright with its Chromium (the repo's, `pnpm install` at
the root, or any `playwright` Node can resolve), the `@fontsource-variable/geist-mono`
package that `apps/desktop` depends on, and no network. The frames are
committed, so a render does not need ffmpeg. Copy lives in
[`themes.mjs`](themes.mjs) (headlines, lines, labels) and the canvases in the
same file; layouts are in [`images.css`](images.css) and
[`templates/image.html`](templates/image.html). The tool count is read at
render time, so re-render after a tool is added.

Every canvas is checked by the LinkedIn kit's `lint.mjs` before it is written,
and the render stops with the list if anything fails: text inside the 60 px
safe area, no text clipped, text no smaller than 24 px, contrast of at least
4.5:1 measured from the rendered pixels. The Story canvas also fails if any
text is in the top 260 px or the bottom 350 px. The lint does not judge
whether an image is any good: look at the output after a change.

## Files

| Path | What it is |
|---|---|
| `out/` | the 42 PNGs |
| `captions.md` | one caption per platform for each theme |
| `render.mjs` | renders `themes.mjs` x canvases through `templates/image.html` |
| `themes.mjs`, `images.css`, `templates/image.html` | copy, canvases and layouts |
| `frames/`, `extract-frames.sh` | the 12 cropped video frames and the script that takes them |
| `LAUNCH.md` | how to use the set for a launch: which canvas where, the order, times, links |
| `.build/` | composed pages, rewritten on every render, not committed |
