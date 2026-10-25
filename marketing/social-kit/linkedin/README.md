# LinkedIn visuals

The images and the carousel for the five LinkedIn posts in
[`../linkedin-posts.md`](../linkedin-posts.md), in the website's design: the
violet-on-zinc palette, Inter, the gradient headline, the aurora wash, the cards,
the stats strip. Nothing here is a photo or a stock asset. The app screenshots
are taken from the real frontend (see [Screenshots](#screenshots)).

## What is in `out/`

These are the files to upload. Post the image **or** the reel; LinkedIn takes one
kind of media per post.

Post 1, the post 3 square and the carousel were re-rendered on 2026-10-22 for
v0.7.0 (they said v0.5.0 and 93 tools); the earlier files are kept in
[`../../archive/linkedin/2026-10-11-v0.5.0/out/`](../../archive/README.md). The other three squares
carry neither the version nor the tool count, and a re-render drew the same
picture, so they are the 2026-10-11 files.

Post 1, post 4 and the carousel were re-rendered on 2026-10-25 for v0.9.0: the
stat said "6 providers", and the Settings screenshot in post 4 listed six
buttons, where v0.9.0 has a seventh, **ChatGPT plan** (#703). The stat now says
"6 API providers" with "or use your ChatGPT plan" in its label, the wording of the
site's stats strip (#704). Post 1's pill and the carousel's last page say v0.9.0.
The files they replace are in
[`../../archive/linkedin/2026-10-22-v0.7.0-six-providers/`](../../archive/README.md).
Posts 2, 3 (the square) and 5 carry no provider count and were not re-rendered.

| File | Size | Goes with | What it shows |
|---|---|---|---|
| `out/post-1-launch.png` | 1200x1200, 576 KB | Post 1, launch | "An audio editor you talk to": a "v0.9.0 · open source" pill, the app's chat panel with a scripted example conversation, and the site's three numbers (0 bytes of audio uploaded, 6 API providers, 94 tools) |
| `out/post-2-before-after.png` | 1200x1200, 396 KB | Post 2, DJ workflow | The seven chores of one blend, beside the same scripted example conversation in the app's chat panel |
| `out/post-3-model-never-touches-a-sample.png` | 1200x1200, 203 KB | Post 3, build story | The model reads text and answers with tool calls; the 94 tools and the audio are on your machine |
| `out/post-4-privacy.png` | 1200x1200, 224 KB | Post 4, privacy | "Does my audio get uploaded? No." What stays, what leaves, and Settings with Ollama selected (the six API providers and the ChatGPT plan button) |
| `out/post-5-help-wanted.png` | 1200x1200, 249 KB | Post 5, open source | Four places a contributor would make a difference, with issue numbers |
| `out/post-3-carousel.pdf` | 7 pages of 1080x1350, each an image (2x, JPEG), 2691 KB | Post 3, **instead of the reel** | The build story as a document: cover, the pipeline (94 tools), a tool call, the history graph, Plan first, why the time-stretch is in Rust, the source (0 bytes, 6 API providers, 94 tools, and "v0.9.0 for macOS, Windows and Linux. The installers are unsigned") |

A square can also be the cover of the post's reel, if LinkedIn's uploader offers a
custom thumbnail (not checked from here). The carousel is a native document post:
upload the PDF, and delete from post 3 the sentence that says what the video shows.
`../linkedin-posts.md` says the same under each post.

The carousel suits post 3 because that post is a list of design decisions, each
of which has a screen in the app. It is also the one post whose copy is mostly
about the app rather than about the demo recordings.

## Re-rendering

```
pnpm install --frozen-lockfile                          # once, at the repo root
node marketing/social-kit/linkedin/render.mjs           # squares and carousel, from the committed screenshots
node marketing/social-kit/linkedin/render.mjs --shots   # retake the app screenshots first, then render
node marketing/social-kit/linkedin/render.mjs --only post-4-privacy   # one canvas (or post-3-carousel)
```

It needs Node 22, the repo's Playwright (a dev dependency of `apps/desktop`) and
the Chromium that Playwright installs (`pnpm --filter @edytlab/desktop exec playwright install chromium`).
It needs no network and no API key. Rendering takes about 20 seconds; `--shots`
adds a production build of the frontend and about a minute of driving it.

Every canvas is checked by [`lint.mjs`](lint.mjs) before it is written, and the
render stops with the list if any check fails:

- text inside the 60 px safe area (the templates use 72 px margins);
- no text clipped by its frame or by the canvas edge, and no screenshot cut off
  by its frame (a deliberate crop is marked `data-crops`);
- text no smaller than 24 px on a square and 28 px on a carousel page (a feed
  shows a square at under half size and a page at about a third);
- contrast of at least 4.5:1 for every piece of text, measured from the rendered
  pixels (so the aurora behind it counts), with the gradient headline measured at
  both of its colours;
- Inter and the mono face actually loaded, and the carousel has 6 to 8 pages.

It does not judge whether a slide is any good. Look at the output after a change.

## Where the look comes from

[`tokens.mjs`](tokens.mjs) reads the website at render time, so nothing is retyped:

| Taken from | What |
|---|---|
| `website/app/globals.css` | the `:root` and `.dark` token blocks verbatim (colours, `--radius`), and the values the site writes inside rules: `.gradient-text`, `.glow`, `.surface`, `.aurora` |
| `website/components/motion/logo.tsx`, `website/public/logo.svg` | the logo's viewBox and shapes |
| `fonts/inter-latin-variable.woff2` | the Inter the live site serves (`next/font`, `website/app/layout.tsx`); the repo has no copy of its own, so this is a copy of that file, with its licence in `fonts/Inter-OFL.txt` (SIL OFL 1.1) |
| `@fontsource-variable/geist-mono` (a dependency of `apps/desktop`) | the mono face, the app's own; the site's mono is a system stack that differs per machine, which would make every render different |
| `apps/desktop/src-tauri/tauri.conf.json`, `docs/tools-reference.md` | the version and the tool count on the images |

What the site keeps in Tailwind classes (type scale, tracking, the `rounded-xl`
card, the panel header, the stats strip) is in [`base.css`](base.css), each rule
with a comment naming the component it was read from. The site's px are doubled
for a social canvas. The site's film grain is left out: it turns each PNG into
over a megabyte, and the aurora does not band visibly without it.

## Screenshots

[`shots.spec.ts`](shots.spec.ts), run by [`shots.config.mjs`](shots.config.mjs),
takes the screenshots in `shots/` from the real frontend in Chromium, the way the
CI specs in `apps/desktop/e2e/` do it: a production build served by `vite preview`,
the shipped Content Security Policy, and only the IPC boundary faked, by
`apps/desktop/e2e/backend.ts`. The spec borrows `test` and the answers from there,
so a command the app calls that nothing answers fails the run.

- **What is real:** every pixel of the interface, in the app's own markup and
  stylesheet, in the violet look that came with #515 and #528.
- **What is staged:** the data. The session is two synthetic 16-bar loops at
  120 BPM that [`loops.ts`](loops.ts) writes from arithmetic (nothing is
  committed as audio, and nothing has a licence), the chat is a scripted example
  conversation (a request, four tool calls and a reply written for the slide:
  "126 BPM" and "16.0 s" are fixture values, not a model's output), the plan is a
  scripted example plan, and the graph is six nodes. Each slide says so under the
  screenshot: "The real app's chat, with a scripted example conversation", "...
  plan card, with a scripted example plan", "... history graph, with example data",
  "... timeline, example data". None of it is a recording of a real run and none of
  it is a result of one.
- **The window and the crops:** 1280 wide as Tauri opens it, 840 tall, at 2x or 3x
  pixels depending on how large the crop is shown. Each crop is cut to what the
  slide needs and is shown at 1.5x to 2.9x its size on screen, so the interface
  text is about 25 to 40 px on the canvas.
- **Not part of CI.** `apps/desktop/playwright.config.ts` has `testDir: ./e2e`, so it
  never sees this folder, and no spec under `apps/desktop/e2e` changed. The config
  uses port 5391 (`SHOTS_PORT` moves it); 5199 is the CI config's.

Retaken on 2026-10-25 against the v0.9.0 frontend (`render.mjs --shots`; the Settings
provider picker gained the ChatGPT plan button, #703): `ollama.png` is the new file, one
row taller, and post 4's template was tightened to fit it (a smaller "No.", tighter
margins, the screenshot at 97% width). The other four were retaken too and look the same
(anti-aliasing only, compared side by side for the chat and the plan card), so they are
the earlier files. The earlier `ollama.png` is in
[`../../archive/linkedin/2026-10-22-v0.7.0-six-providers/shots/`](../../archive/README.md).

Retaken on 2026-10-22 against the v0.7.0 frontend (`render.mjs --shots`, after
the lane controls, the History graph and the mix cache changed): the interface
they draw is the same, and the pixels differ only by anti-aliasing between
Chromium builds (the plan card wraps one word differently), so the files from
2026-10-11 are kept.

| File | What it is |
|---|---|
| `shots/chat.png` | the assistant after one request: four tool calls and the reply (a scripted example conversation) |
| `shots/timeline.png` | the two lanes, with the crossfade drawn as volume envelopes (example data) |
| `shots/graph.png` | the history graph at a fork: tempo match, then crossfade with a low-pass, or a hard cut (example data) |
| `shots/plan.png` | Plan first's card, held for approval (a scripted example plan) |
| `shots/ollama.png` | Settings with Ollama selected (six API providers and the ChatGPT plan button), and the dialog's own note that it needs no key |

## What the slides claim, and where each claim comes from

Under the rules in [`../../README.md`](../../README.md). No slide says anything
about stem separation, transcription or text editing, and none carries a user
count, a quote or a benchmark.

| Claim on a slide | Source |
|---|---|
| 94 tools; fade / EQ / time-stretch / loudness / export are among them; they are Rust functions with no model in the audio path. The slides do not call the 94 "deterministic": two of them (`separate_stems`, `transcribe`) only return an error today, and tools an MCP server adds are not part of the 94 | `docs/tools-reference.md` (read at render time), `crates/tools/src/tool/` |
| 6 API providers, one local, plus the ChatGPT plan (v0.9.0, a preview); 0 bytes of audio uploaded | `website/components/landing/stats-strip.tsx`, `website/app/privacy/page.tsx` |
| The model reads text (tempo, key, loudness), never receives the audio; the DSP runs in Rust on your machine | marketing `README.md` accuracy table; `docs/architecture.md` |
| Only the chat leaves; Ollama needs no key and runs on your machine | `website/app/privacy/page.tsx`; the dialog's own text in `shots/ollama.png` |
| Plan first: no edit runs without approval | `docs/architecture.md`, "Plan first" |
| Every edit is a node; fork, A/B, go back; undo follows the path you took | `crates/tools/src/tool/fork_node.rs`, `compare_nodes.rs`, `apps/desktop/src/lib/headTrail.ts` |
| Rubber Band was dropped for a pure-Rust phase vocoder; the cost is smearing on dense material at large changes | `crates/audio-time/src/vocoder.rs`, `README.md` |
| Eleven effects cannot yet run on a master or bus chain | `NOT_YET_STREAMING` in `crates/audio-engine/src/effect_chain.rs` |
| #464 (macOS microphone), #497 (Linux keyring), #383 to #385 (ML inference) | GitHub, checked 2026-10-22: #464, #497, #383, #384 and #385 are all open. The installers carry the ONNX Runtime from v0.7.0 (#615), but no model is downloaded and no inference runs, so "Not shipped yet: ONNX Runtime inference inside a Tauri app" on the post 5 square is still true |
| v0.9.0 for macOS, Windows and Linux; MIT; the installers are unsigned | `tauri.conf.json` (`render.mjs` reads the version from it), `LICENSE`, the v0.9.0 release notes. The last carousel page, the only canvas that mentions a download, says so; post 1's pill names the version. Both were re-rendered on 2026-10-25 and say v0.9.0. The version and the tool count are read at render time, so **run `node render.mjs` after the next release or the next new tool, before posting either** |

## Files

| File | What it is |
|---|---|
| `render.mjs` | renders `templates/` to `out/`; `--shots` retakes the screenshots first |
| `lint.mjs` | the brand check |
| `tokens.mjs`, `base.css` | the site's tokens, read from the site; the rest of the look |
| `templates/` | one HTML file per canvas; `post-3-carousel.html` holds the seven pages |
| `shots.spec.ts`, `shots.config.mjs`, `loops.ts` | the screenshots, and the audio they load |
| `fonts/` | Inter, as the site serves it, and its licence |
| `package.json` | marks the folder as ES modules for Node and Playwright; not a workspace package |
| `.build/` | composed pages, rewritten on every render, not committed |

## Notes for whoever edits these

- Change the copy in a template, then re-render. A number that exists elsewhere
  in the repo (`{{version}}`, `{{tools}}`) is filled in at render time; do not type it.
- A UI change makes `shots/` stale. Retake it with `--shots`, look at each image,
  and re-render. This is on the staleness sweep's list for UI changes
  (screenshots and recordings that show the app).
- The carousel PDF is built from images of its pages (`carouselPdf` in
  `render.mjs`, #643): each page is screenshotted at 2x and stored as a JPEG, one
  image per page. Printed from the HTML, the gradient headline halves
  (`background-clip: text`, and later an SVG `<text>` with a gradient fill) did
  not paint in every viewer: PDFium left them out and poppler drew a solid
  magenta box. The image PDF was checked on 2026-10-25 in PDFium (pypdfium2
  5.14) and poppler 24.02 (`pdftoppm`), and every page, headline included,
  draws as the canvas does. The cost: the PDF's text is not selectable or
  searchable, which LinkedIn's document viewer does not offer anyway. It was not
  checked in Acrobat or LinkedIn's uploader preview; look at the preview before
  posting.
- LinkedIn's own limits for images and documents were not checked from here. The
  files are well inside the sizes it is usually reported to accept (an image under
  a few MB, a document well under 100 MB), but check the uploader on the day.
