# edytlab launch kit

Copy-paste material for launching edytlab v0.4.0. Everything here is written
to be posted as-is, by the owner, from their own accounts. Nothing in this
folder posts anywhere by itself, and nothing here opens a PR on another repo.

## Links to use

| For | Link |
|---|---|
| Website, demos, docs | <https://edytlab.com> |
| Download (newest versioned release) | <https://github.com/laadtushar/edytlab/releases/latest> |
| Source | <https://github.com/laadtushar/edytlab> |
| Use-case pages | <https://edytlab.com/use-cases/dj>, <https://edytlab.com/use-cases/local-ai-audio-editor> |
| Press kit page | <https://edytlab.com/press> |

Use `releases/latest` for "download" links in posts. It resolves to the newest
non-prerelease, which is v0.4.0 today. (The website's own buttons point at the
full releases list on purpose, so they can also reach dev builds.)

## What is in the kit

| File | What it is |
|---|---|
| [`launch/show-hn.md`](launch/show-hn.md) | Show HN title, body and the author's first comment, plus answers to the questions HN will ask |
| [`launch/product-hunt.md`](launch/product-hunt.md) | Tagline, description, maker's first comment, 5 gallery captions, topics |
| [`launch/reddit.md`](launch/reddit.md) | Seven separate posts: r/WeAreTheMusicMakers, r/DJs, r/audioengineering, r/podcasting, r/LocalLLaMA, r/rust, r/tauri |
| [`social/x-thread.md`](social/x-thread.md) | 8-post thread for v0.4.0 and the DJ demo |
| [`social/linkedin.md`](social/linkedin.md) | One LinkedIn post, same angle |
| [`social-kit/`](social-kit/README.md) | The 2026-10-10 social set: 5 LinkedIn posts, 5 Instagram reel captions, 6 Reddit posts, X, Bluesky, Mastodon, TikTok, Shorts, YouTube, Show HN and newsletter copy, and a two-week calendar |
| [`reels/`](reels/README.md) | Vertical reels (1080×1920) cut from the three DJ demos, plain and narrated, with `.srt` captions, cue files, transcripts and the scripts that build them. Read its licence note before posting a narrated file |
| [`launch-video/`](launch-video/README.md) | The launch video, cut three ways from the real demo recordings: LinkedIn 1:1, Reddit 16:9 and Instagram 9:16, plain and narrated, with `.srt` captions, contact sheets, silent picture-only masters, the narration clips, the script, the research and the code that renders them. Read its licence note before posting a narrated file |
| [`press-kit.md`](press-kit.md) | Boilerplate, fact sheet, asset links |
| [`awesome-lists.md`](awesome-lists.md) | Lists worth a pull request, exact entry text, and which ones are blocked today |

Character limits (HN title 80, Product Hunt tagline 60 and description 260,
X posts 280) were counted by script when the files were written. Re-count if
you edit.

## The accuracy rules (read before you edit anything)

These are the claims that are easy to get wrong. Each has bitten a draft once.

**Never say or imply:**

- Stem separation, vocal isolation or "mashup A's vocals over B's drums". The
  `separate_stems` tool returns an error in every build today
  ([#385](https://github.com/laadtushar/edytlab/issues/385)).
- Transcription, transcript-based editing, "remove filler words", "edit by
  deleting text". `transcribe` is a stub
  ([#384](https://github.com/laadtushar/edytlab/issues/384)), and `cut_words`,
  `remove_fillers` and `duck_under_speech` need a transcript. They are built
  and untestable end to end until it exists. The README's older mashup example
  predates this; do not reuse it.
- A user count, download count, star count, testimonial, press quote or
  benchmark. None exists in the repo to cite.
- That the website has no analytics. The desktop app sends no telemetry, but
  the website uses Vercel Web Analytics and Speed Insights (anonymous, no
  cookies), as `website/app/privacy/page.tsx` says. Keep the two apart:
  "the app has no telemetry" is true; "no analytics" about the site is not.
- That builds are signed. They are not
  ([#386](https://github.com/laadtushar/edytlab/issues/386)). Say so wherever a
  download link appears.
- That the editor is for live DJ performance. It edits files. Live decks and
  controllers are on the post-v1 roadmap.

**Safe, and verified against the repo on 2026-10-10:**

| Claim | Where it was checked |
|---|---|
| 93 tools | `docs/tools-reference.md` (generated from the tool registry) |
| Six providers: Anthropic, OpenAI, OpenRouter, Groq, Gemini, Ollama | `README.md`, `website/app/docs/getting-started/page.tsx` |
| Ollama needs no key and can keep the chat on your machine | `website/app/privacy/page.tsx`, user guide |
| Audio is never uploaded; only the chat goes to the provider | `website/app/privacy/page.tsx` |
| The desktop app sends no telemetry. The website uses anonymous Vercel analytics | `website/app/privacy/page.tsx`; `<Analytics />` and `<SpeedInsights />` in `website/app/layout.tsx` |
| Branchable session graph, fork, A/B compare, undo | `crates/tools/src/tool/fork_node.rs`, `compare_nodes.rs`, README |
| Plan first is an optional toggle in the chat box | `apps/desktop/src/components/Chat.tsx` |
| Export to WAV, FLAC and MP3 | `crates/tools/src/tool/render_final.rs` |
| Time-stretch and pitch-shift on its own phase vocoder | `crates/audio-time/src/vocoder.rs` |
| BPM, key, beat grid and EBU R128 loudness analysis | `analyze_track` in `docs/tools-reference.md` |
| macOS universal, Windows, Linux builds; MIT; v0.4.0 | GitHub release v0.4.0, `LICENSE` |

## Known rough edges to be ready for

Readers will find these within an hour. Answer them plainly instead of
getting caught.

- Linux: v0.4.0 and earlier keep API keys in the kernel keyring only until
  reboot. The next release stores them in the Secret Service and warns when
  there is none
  ([#394](https://github.com/laadtushar/edytlab/issues/394)).
- Local models: a first request was measured at about 15,000 tokens, which an
  8,192-token context refuses
  ([#395](https://github.com/laadtushar/edytlab/issues/395)).
- The limiter is a zero-latency sample-peak limiter (instant attack, smooth
  release; it was a hard clip until
  [#441](https://github.com/laadtushar/edytlab/issues/441)), not a look-ahead
  true-peak one. Audio engineers will ask.

## Before the first post

- [ ] Install the v0.4.0 build on at least one machine per OS you will claim.
      Confirm the macOS `xattr` step in the release notes works as written.
- [ ] Open <https://edytlab.com> on a phone and play the demo.
- [ ] GitHub repo topics. The repo currently lists `audacity`, `audacity-pro`
      and `audacity-setup`, which will look odd to a visitor. Suggested set:
      `audio-editor`, `ai-agent`, `llm`, `ollama`, `tauri`, `rust`, `dsp`,
      `dj`, `podcast`, `local-first`. (Settings, About, Topics. It needs repo
      admin, so it is not part of this PR.)
- [ ] Set the repo social preview image (Settings, General). The demo poster
      `website/public/demos/dj-beatmatched-transition.jpg` is the only still in
      the repo; a cropped app screenshot would be better.
- [ ] Capture the Product Hunt screenshots listed in
      [`launch/product-hunt.md`](launch/product-hunt.md). No UI screenshots are
      committed to the repo.
- [ ] Decide how you will answer "was this written by AI?". The commit history
      shows `Co-Authored-By: Claude` trailers on many commits, and the repo has a
      `CLAUDE.md`. The drafts disclose it once, briefly. Keep that line.

## Posting order

Spread it out. Each post needs you present for the first few hours to answer
comments, and one thread cannot be answered well while three others are open.

| Day | Post | Why here |
|---|---|---|
| 0 | **Show HN** ([`launch/show-hn.md`](launch/show-hn.md)) | Submit the site URL, then post the first comment within minutes. Stay on for 4 to 6 hours. Do not ask anyone to upvote; HN treats that as a violation. |
| 0 | **X thread** and **LinkedIn** | Same day, after the HN comment is up. Use the HN link only if the post is doing well. |
| 1 | **r/rust** and **r/tauri** | Technical audiences; the easiest to be useful to. Fix anything HN found first. |
| 2 | **r/LocalLLaMA** | Wants the Ollama angle and honest numbers. Bring the #395 data. |
| 3 | **r/DJs** | Lead with the demo video. |
| 4 | **r/WeAreTheMusicMakers** | Read the sidebar the same day; self-promotion is often limited to a set thread. |
| 5 | **r/podcasting** | Say plainly that there is no transcript editing. |
| 6 | **r/audioengineering** | Expect scrutiny of the DSP. Post only if the current rules allow it. |
| 7 to 14 | **Product Hunt** ([`launch/product-hunt.md`](launch/product-hunt.md)) | Needs the screenshots and a launch-day calendar. Schedule it for 00:01 PT. Later is fine; it works better with a few real users to show up. |
| After the star count passes the bar | **Awesome lists** ([`awesome-lists.md`](awesome-lists.md)) | `awesome-rust` requires more than 50 stars or 2,000 downloads. On 2026-10-10 the repo has 4. |

The days are a suggestion. Do not post the same day to several subreddits, and
do not paste the same text twice: each post in `reddit.md` is written for its
own community.

## Rules of engagement

- Say you built it, in the first line, everywhere.
- Subreddit rules change and differ in how they treat self-promotion. They were
  not checked from this environment. Re-read each sidebar the day you post. If
  a sub restricts promotion to a weekly thread, use that thread and skip the
  standalone post.
- Answer criticism with the issue number if one exists, and open an issue if
  one does not.
- No one is asked to star, upvote or share.
- Measurement: the website runs Vercel Web Analytics and Speed Insights
  (anonymous page views and page performance, no cookies), and the owner sees
  them as aggregates in the Vercel dashboard. The desktop app sends no
  telemetry. The other built-in signals are GitHub release download counts,
  stars and issues. Do not add any further tracking for the launch; the
  privacy page says Vercel's is the only analytics the site runs.
