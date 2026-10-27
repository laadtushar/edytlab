# edytlab launch kit

Copy-paste material for launching edytlab v0.9.3. Everything here is written
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
non-prerelease, which is v0.9.3 today (published 2026-10-27). (The website's own buttons point at the
full releases list on purpose, so they can also reach dev builds.)

## What is in the kit

| File | What it is |
|---|---|
| [`launch/show-hn.md`](launch/show-hn.md) | Show HN title, body and the author's first comment, plus answers to the questions HN will ask |
| [`launch/product-hunt.md`](launch/product-hunt.md) | Tagline, description, maker's first comment, 5 gallery captions, topics |
| [`launch/reddit.md`](launch/reddit.md) | Seven separate posts: r/WeAreTheMusicMakers, r/DJs, r/audioengineering, r/podcasting, r/LocalLLaMA, r/rust, r/tauri |
| [`social/x-thread.md`](social/x-thread.md) | 8-post thread for the v0.9.3 release and the DJ demo |
| [`social/linkedin.md`](social/linkedin.md) | One LinkedIn post, same angle |
| [`social-kit/`](social-kit/README.md) | The 2026-10-10 social set: 5 LinkedIn posts, 5 Instagram reel captions, 6 Reddit posts, X, Bluesky, Mastodon, TikTok, Shorts, YouTube, Show HN and newsletter copy, and a two-week calendar |
| [`social-kit/linkedin/`](social-kit/linkedin/README.md) | The LinkedIn visuals in the website's design: five 1200x1200 images (one per post in `social-kit/linkedin-posts.md`), a seven-page carousel PDF for post 3, the HTML templates, staged screenshots of the real app and the script that renders them |
| [`reels/`](reels/README.md) | Vertical reels (1080×1920) cut from the three DJ demos, plain (no narration), with cue files and the scripts that build them. Re-cut on 2026-10-26 from the new recordings; the earlier reels and the narration made for the old takes are in `archive/` |
| [`demos/narration/`](demos/narration/README.md) | Nothing narrated is current: the voice-over for the old recordings of the DJ demos (narrated mp4s, voice tracks, script, timing and loudness checks) was moved to `archive/demos-narration/2026-10-26-old-recordings/` when the demos were re-recorded. The README there says how to start again |
| [`launch-video/`](launch-video/README.md) | The launch video, cut three ways (LinkedIn 1:1, Reddit 16:9, Instagram 9:16) from the 2026-10-26 recordings, plain (no narration), with `.srt` captions, contact sheets, silent picture-only masters, the script and claim ledger, the research and the code that renders them. It names no version, tool count, model or provider (see "Evergreen media rules" below). The earlier cuts and the narration clips for the old takes are in `archive/` |
| [`promo-v0.7.0/`](promo-v0.7.0/README.md) | The v0.7.0 promo: nine live screen recordings of the released v0.7.0 app at 3200x2000 and 15 fps (`footage/`, with the sound of the two clips that play), the script that cuts them from the native suite's recordings (`record/`; the stories that film them are `apps/desktop/e2e-native/stories/promo.mjs`), the claim ledger (`claims.md`), and the Remotion edit (`remotion/`, outside the pnpm workspace) that cuts them into five finished videos in `out/`: three 9:16 reels (What's new in v0.7.0, Describe the edit, 200 tracks? Ask.), the 16:9 launch video and its 1:1 cut, each with an `.srt` and a contact sheet, mastered to -14 LUFS. The assistant, where there is one, is real Claude (Sonnet 5.5) through the app's own provider and the audio is synthetic, and every app shot says so. The first, scripted version is in `archive/promo-v0.7.0/`. Read its licence note: Remotion's Free License covers an individual's project, not a company of more than three people |
| [`promo-v0.7.0/narration/`](promo-v0.7.0/narration/README.md) | A voice-over for the five finished v0.7.0 promo videos (16:9, its 1:1 twin and the three 9:16 reels): the narrated mp4s with Daniel (all five) and River (the 16:9), the voice tracks, three voice samples, the script traced line by line to `promo-v0.7.0/claims.md`, the lines as ElevenLabs returned them, the timing, loudness and true-peak checks, and the scripts that rebuild any mix. Voice: synthetic, made with ElevenLabs (`eleven_multilingual_v2`), 2026-10-24; the owner confirmed with ElevenLabs that this open-source (MIT) project may use the output |
| [`archive/`](archive/README.md) | Every earlier version of the launch video, the reels, the website demos and the LinkedIn renders, with their captions, contact sheets, cue and timing files, sorted by what it is, then by version, then by kind. Kept for reference, nothing is deleted. Read its licence note: the narrated files use ElevenLabs free-plan audio and are not for posting |
| [`press-kit.md`](press-kit.md) | Boilerplate, fact sheet, asset links |
| [`awesome-lists.md`](awesome-lists.md) | Lists worth a pull request, exact entry text, and which ones are blocked today |

Character limits (HN title 80, Product Hunt tagline 60 and description 260,
X posts 280) were counted by script when the files were written. Re-count if
you edit.

## Evergreen media rules

Every recording, reel, launch video, poster, caption, overlay, title card and narration script must stay true after the next release. A re-cut or re-recording is expensive (live model calls, hours of rendering), so make a piece once and keep it true. **Do not put any of these in media or in the words that go with it:**

- **A version number.** Not on a card, not in a caption, not in a poster, and not in the footage: the demo recordings carry the status bar's version at its right end, so `apps/desktop/e2e-native/make-demo-videos.mjs` draws a box in the bar's own colour over it (`MASK`). A crop that shows the bar must go through that box. Check a frame of every new take.
- **A count that grows**: tools, commands, crates, providers, models, languages. Say nothing, or "deterministic tools" and "any AI provider"; say "dozens" only where a line truly needs a number.
- **A model name or id**, or a provider list. The demos are recorded with a chosen model (`DEMO_MODEL`), and nothing on screen or in a caption names it; where the assistant is in the shot, say "real Claude" at most, as the site's disclosure does.
- **A price, a date, a release channel or signing status** ("unsigned installers", "v0.x is current").
- **A claim about what an analysis found**: a musical key, a beat or downbeat time, a confidence, a loudness reading, a tempo the detector reported. A detector can improve (the DJ demos once said "F major" for every track, #710). Captions and callouts describe what the shot shows ("A real tool call", "Three edits, one request", "It reports what it exported"), never the number. The model's own reply on screen is the take; it is fine that it shows readings, but never repeat or highlight them in a caption, callout, card or script.
- **A parameter the model chose** (a cutoff, a gain) as a fact about the product. It may be on screen in the reply; do not state it in a caption.

**When you write the prompts for a recording**, phrase them so the reply does not recite what changes: ask for the tempo "in a sentence or two about tempo only", give the numbers that belong to the task (a start time, a target) yourself, and keep a step to one request, because a model that is unsure what a previous step did may repeat it (a three-step intro demo once pasted twice). Fixture facts (the tempo a generated track was made at, the file names) are stable and may appear.

**Never use the scripted stand-in model (`fake-llm.mjs`) for a recording captioned as Claude.** Record with the real model through the metering proxy (`apps/desktop/e2e-native/claude-proxy.mjs`, with a `BUDGET_USD` cap), put the key only in the environment of the one command, and grep the repo and the output folder for the key itself and for Anthropic's key prefix before a commit (the pre-existing hits in tests and docs are placeholders).

Where this is applied: `apps/desktop/e2e-native/stories/demos.mjs` (prompts and captions), `make-demo-videos.mjs` (the version box), `website/lib/demos.ts` and `website/lib/videos.ts` (captions and moments), `reels/tools/spec.py` (captions, cards, lines), `launch-video/tools/launch.py` (titles, callouts, tail card) and `social-kit/linkedin/templates`, `social-kit/images/themes.mjs` (no `{{version}}`, no `{{tools}}`). The renderers still know how to fill `{{version}}` and `{{tools}}`; do not use them in a template.

**Known exceptions, not yet applied** (they are text for the owner or UI screenshots, and need a decision): the launch kit's posts and the `social-kit/artifact/launch-hub.mjs` page name the release they were written for, quote the tool count and say the installers are unsigned (true today, and a post is text for the owner, not footage); `social-kit/*.md`, `launch/*.md` and `social/*.md` no longer quote readings, cutoffs or export lengths from the old recordings (the owner's #710 text follow-up), only what the DJ types and the fixture tempos, and `social-kit/artifact/launch-hub.mjs` reads them; `social-kit/linkedin/shots/*.png` are screenshots of the app's Settings and chat and show the UI of the day (retake with `node render.mjs --shots` after a UI change); `docs/brand/` is a before/after record of the 2026 restyle and is kept as it was.

## The accuracy rules (read before you edit anything)

These are the claims that are easy to get wrong. Each has bitten a draft once.

**Never say or imply:**

- Stem separation, vocal isolation or "mashup A's vocals over B's drums". The
  `separate_stems` tool returns an error in every build today
  ([#385](https://github.com/laadtushar/edytlab/issues/385)).
- That transcription is automatic, bundled or free of a download. It works
  from v0.8.0 ([#384](https://github.com/laadtushar/edytlab/issues/384)), but
  only after the user presses Download in Settings → Account for the speech
  model (Whisper base, about 139 MB, from Hugging Face). Nothing downloads by
  itself, and v0.7.0 and earlier cannot transcribe. Say "on your computer"
  and "audio never leaves it", and say that the words reach the AI provider as
  text (not audio), and stay local with Ollama.
- Any accuracy, speed or language-quality figure beyond what was measured: on
  one short synthetic English recording of twelve words, none was wrong and
  every start was within 0.22 s; 7 s of audio took 2.1 to 3.3 s the first time
  on a shared four-core machine. On about 16 minutes of English audiobook
  speech (two LibriSpeech chapters, `scripts/word-timing-eval`), 4% to 7% of
  the words were wrong and the median word start was 0.07 s from a
  forced-alignment reference. That is two kinds of English recording and one
  machine. Do not
  say "accurate", "fast", "human-level" or "supports 99 languages"; the tool
  takes a two-letter language code or detects it, and only English was checked.
- That `cut_words` and `remove_fillers` are tested on real recordings. They
  were run end to end on that one recording. `remove_fillers` reports first and
  changes nothing until asked to apply. `duck_under_speech` and
  `select_region`'s `speech_passage` ("the last thing said") do not need a
  transcript: they key on a voice track's audio (voiced sound, not what was
  said), so say that, not "keyed on the words". The README's older mashup
  example predates this; do not reuse it.
- That the ChatGPT sign-in is tested on real accounts, until the real-account
  list in `docs/specs/2026-10-10-chatgpt-subscription-sign-in.md` §4.10 is
  done, or that it is an OpenAI partnership. Call it a preview. Say "Available
  for ChatGPT Plus and Pro. May not be available on workspace plans; that
  depends on OpenAI and your admin", and do not say Team or Business works or
  does not. Use the words "Continue with ChatGPT". The approved Sign in with
  ChatGPT assets (`website/public/brand/openai/`) may be used as published,
  unaltered (not recoloured or redrawn), and only for the sign-in
  (<https://developers.openai.com/siwc/website>; the owner cleared this with
  OpenAI). No other OpenAI or ChatGPT logo, and no endorsement or partnership
  claim (other marks: <https://openai.com/brand/>, partnercomms@openai.com).
- A user count, download count, star count, testimonial, press quote or
  benchmark. None exists in the repo to cite.
- That the website has no analytics. The desktop app sends no telemetry, but
  the website uses Vercel Web Analytics and Speed Insights (anonymous, no
  cookies) and, once `NEXT_PUBLIC_GA_MEASUREMENT_ID` is set in the site's
  build, Google Analytics 4 behind a consent banner (nothing from Google loads
  before a visitor accepts; it sets cookies), as `website/app/privacy/page.tsx`
  says. Keep the two apart: "the app has no telemetry" is true; "no analytics"
  about the site is not, and "no cookies" holds for the Vercel pair only.
- That builds are signed. They are not
  ([#386](https://github.com/laadtushar/edytlab/issues/386)). Say so wherever a
  download link appears.
- That the editor is for live DJ performance. It edits files. Live decks and
  controllers are on the post-v1 roadmap.

**Safe, and verified against the repo and the GitHub release on 2026-10-22 (v0.7.0; the `measure`, `diarize` and formats rows on 2026-10-27, v0.9.3):**

| Claim | Where it was checked |
|---|---|
| 101 tools | `docs/tools-reference.md` (generated from the tool registry) |
| Six API providers plus your ChatGPT plan: Anthropic, OpenAI, OpenRouter, Groq, Gemini, Ollama, and (from v0.9.0, a preview for Plus and Pro) Continue with ChatGPT | `README.md`, `website/app/docs/getting-started/page.tsx`, `crates/ai/tests/website_provider_claims.rs` |
| Keys and the ChatGPT sign-in token share one OS-keychain entry, so macOS asks for the login password at most once per launch; provider, model and base URL are in `provider-settings.json`, never a secret | `website/app/privacy/page.tsx`, `docs/adr/0009-settings-and-secrets-storage.md`; counted against fake keychain slots, not yet on a real Mac (see below) |
| Ollama needs no key and can keep the chat on your machine | `website/app/privacy/page.tsx`, user guide |
| The ChatGPT plan has no key: its sign-in keeps one refresh token in the keychain and a small registration file; signing out does not disconnect edytlab in ChatGPT (the user does that in ChatGPT settings) | `website/app/privacy/page.tsx` ("Signing in with ChatGPT"), user guide ("Using your ChatGPT plan"); OpenAI's Help Center article 20001542 |
| Audio is never uploaded; only the chat goes to the provider (Settings also asks it for its model list, with your key and no prompt or audio) | `website/app/privacy/page.tsx` |
| The desktop app sends no telemetry. The website uses anonymous Vercel analytics, and Google Analytics 4 only after a visitor accepts the cookie banner, in a build that has a measurement ID | `website/app/privacy/page.tsx`; `<Analytics />`, `<SpeedInsights />` and `<GoogleAnalyticsConsent />` in `website/app/layout.tsx` |
| Branchable session graph, fork, A/B compare, undo | `crates/tools/src/tool/fork_node.rs`, `compare_nodes.rs`, README |
| Plan first is an optional toggle in the chat box | `apps/desktop/src/components/Chat.tsx` |
| Export to WAV, FLAC and MP3 | `crates/tools/src/tool/render_final.rs` |
| Time-stretch and pitch-shift on its own phase vocoder | `crates/audio-time/src/vocoder.rs` |
| BPM, key, beat grid and EBU R128 loudness analysis | `analyze_track` in `docs/tools-reference.md` |
| `measure` (v0.9.3): integrated, momentary and short-term loudness, loudness range, sample and true peak, RMS, noise floor, digital silence and clipping, with an ACX pass or fail (`check: "acx"`) | `measure` in `docs/tools-reference.md`, the v0.9.3 release notes |
| `diarize` (v0.9.3): labels who speaks when on a track or file, on this computer, after the user downloads the speaker model (about 33 MB, two files, from Hugging Face) in Settings → Account; it returns turns and edits nothing, and the assistant can hand them to `split_by_speaker`. Say nothing about accuracy beyond "checked on one 40-second meeting excerpt". Same privacy facts as the speech model (GET requests on the click, no audio, no telemetry, using it makes no request). | `diarize` in `docs/tools-reference.md`, the v0.9.3 release notes, `website/app/privacy/page.tsx` |
| AIFF and Opus (mono and stereo, decoded at 48 kHz) import; M4A/AAC does not, and the app says to convert | the v0.9.3 release notes, `crates/audio-decoder` |
| macOS universal, Windows, Linux builds; MIT; v0.9.3 | GitHub release v0.9.3 (its page lists the six installers), `LICENSE` |

## Known rough edges to be ready for

Readers will find these within an hour. Answer them plainly instead of
getting caught.

- Compaction: v0.5.0 and earlier can delete a user's own recordings when the
  assistant compacts history, once the versions that used a loaded file are
  pruned ([#589](https://github.com/laadtushar/edytlab/issues/589)). v0.6.0
  deletes only audio the project made, inside its own folder
  ([#593](https://github.com/laadtushar/edytlab/pull/593)). Link only
  `releases/latest` or v0.6.1 or later, and tell anyone on an older build to update
  before they ask it to reclaim disk.
- Linux: v0.4.0 and earlier keep API keys in the kernel keyring only until
  reboot. Since v0.5.0 they are stored in the Secret Service, with a warning
  when there is none ([#394](https://github.com/laadtushar/edytlab/issues/394)). Nobody has
  yet checked it on a real desktop across a reboot, or against a locked
  keyring, where an unlock prompt can come up at startup
  ([#497](https://github.com/laadtushar/edytlab/issues/497)). Say "should
  survive a reboot", not "does".
- macOS password prompts: since v0.9.2 keys and the ChatGPT sign-in share one
  keychain entry, so a launch asks at most once ([#734](https://github.com/laadtushar/edytlab/issues/734),
  ADR 0009). That count comes from tests over fake keychain slots; nobody has
  yet checked it on a real Mac. The first launch after updating from v0.9.1 or
  older may ask once for each old entry it moves. Say "at most once per launch",
  not "never".
- Local models: in v0.4.0 a first request was measured at about 15,000 tokens,
  which an 8,192-token context refuses
  ([#395](https://github.com/laadtushar/edytlab/issues/395)). Since v0.5.0
  Ollama is sent a core set of tools plus the ones a message names, with the
  rest listed by name ([#479](https://github.com/laadtushar/edytlab/pull/479)).
  The test suite puts the one-line request at about 8.7 KB, against about
  67 KB with every tool on v0.7.0 (a chat-completions request, 67,130 B
  measured on 2026-10-22 with `cargo test -p ai --test request_size --
  --nocapture`; it was 66 KB on v0.6.0, as `docs/architecture.md`, "Request
  size and small-context models", says). That is
  bytes, not tokens, and nobody has run a local model through a full job on
  it, so do not say "works with local models".
- Master bus: v0.6.0 is the first release whose agent puts an effect on the
  master bus (`add_effect` with scope `master`,
  [#460](https://github.com/laadtushar/edytlab/issues/460), PR #516), on a bus
  (`scope: "bus"`), and auditions one on either (PR #551). v0.5.0 and earlier
  render a baked "Master" track instead. Only gain, the limiter and the
  low-pass, high-pass and notch filters run in a chain; do not promise a shared
  reverb on a bus. The website recordings put their limiter on the master bus,
  as v0.6.0 does; in the beatmatch the agent still mixes the two tracks to a
  new one to compress them, because a compressor cannot go in an effect chain
  yet ([#520](https://github.com/laadtushar/edytlab/issues/520)).
- v0.6.0's own known issues, as they stood on 2026-10-16: a quick key tap on a
  mixer slider can be lost while an earlier change is read back
  ([#596](https://github.com/laadtushar/edytlab/issues/596)), and the A/B
  compare still writes its renders to the system's temporary folder
  ([#575](https://github.com/laadtushar/edytlab/issues/575)). Both are fixed
  since: #575 in v0.6.1 (the renders go in the project's preview cache), and
  #596 in v0.7.0, where a gain or pan slider, mute or solo stays as you set it
  until the session confirms it
  ([#626](https://github.com/laadtushar/edytlab/pull/626)). v0.5.0's (Save
  As leaving the assistant on the original project, and opening a project
  restoring an older head, [#506](https://github.com/laadtushar/edytlab/issues/506)
  and [#507](https://github.com/laadtushar/edytlab/issues/507)) are fixed in
  v0.6.0.
- The label name box and Delete's native confirm were checked on one real Mac
  (v0.9.0 and v0.9.1, macOS 26.5.2, 2026-10-25;
  [#567](https://github.com/laadtushar/edytlab/issues/567), closed, report in
  #727) and passed. Say "checked on one Mac", not "works on every Mac".
- The limiter is a zero-latency sample-peak limiter (instant attack, smooth
  release; it was a hard clip until
  [#441](https://github.com/laadtushar/edytlab/issues/441)), not a look-ahead
  true-peak one. Audio engineers will ask.

## Before the first post

- [ ] Install the v0.9.3 build on at least one machine per OS you will claim
      (not v0.9.0: its first message after Continue with ChatGPT failed with
      "Unsupported content type", which v0.9.2 and later fix).
- [ ] Sign in with a real ChatGPT Plus or Pro plan on v0.9.3 and send a message.
      Until that has worked once, leave every line about the ChatGPT plan out
      of the posts: the fix was checked against a mock of OpenAI's API, not a
      real account.
      Confirm the macOS `xattr` step in the release notes works as written.
- [ ] Open <https://edytlab.com> on a phone and play the demo.
- [ ] GitHub repo topics. The repo currently lists `audacity`, `audacity-pro`
      and `audacity-setup`, which will look odd to a visitor. Suggested set:
      `audio-editor`, `ai-agent`, `llm`, `ollama`, `tauri`, `rust`, `dsp`,
      `dj`, `podcast`, `local-first`. (Settings, About, Topics. It needs repo
      admin, so it is not part of this PR.)
- [ ] Set the repo social preview image (Settings, General). The demo poster
      `website/public/demos/dj-beatmatched-transition.jpg` is the clearest
      still in the repo. `docs/brand/` also holds before and after screenshots
      of the app's colours (#515, 1440x900, taken with the IPC faked, so they
      show fake data; the `after-*` set was retaken on 2026-10-26 for v0.9.2 (the status bar says V0.9.2), so it shows the ChatGPT logo on the provider button and the message box at the pane's full width); the `before-*` set is the look v0.5.0 and earlier
      shipped, and the `after-*` set is the violet look the app has had since v0.6.0.
- [ ] Capture the Product Hunt screenshots listed in
      [`launch/product-hunt.md`](launch/product-hunt.md) from the v0.9.3 build.
      The only UI screenshots in the repo are the `docs/brand/` pair above and the staged ones in `social-kit/linkedin/shots/` (fake data, retaken by `social-kit/linkedin/render.mjs --shots`).
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
| 2 | **r/LocalLLaMA** | Wants the Ollama angle and honest numbers. Bring the #395 data and the request sizes from #479, and say that no local model has run a full job. |
| 3 | **r/DJs** | Lead with the demo video. |
| 4 | **r/WeAreTheMusicMakers** | Read the sidebar the same day; self-promotion is often limited to a set thread. |
| 5 | **r/podcasting** | Say plainly what transcription was checked on (a short synthetic English recording and 16 minutes of English audiobook speech), that it needs a one-time model download, and that there is no speaker splitting by voice. |
| 6 | **r/audioengineering** | Expect scrutiny of the DSP. Post only if the current rules allow it. |
| 7 to 14 | **Product Hunt** ([`launch/product-hunt.md`](launch/product-hunt.md)) | Needs the screenshots and a launch-day calendar. Schedule it for 00:01 PT. Later is fine; it works better with a few real users to show up. |
| After the star count passes the bar | **Awesome lists** ([`awesome-lists.md`](awesome-lists.md)) | `awesome-rust` requires more than 50 stars or 2,000 downloads. The repo had 4 on 2026-10-10 and 6 on 2026-10-22. |

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
  telemetry. Google Analytics 4 is wired in behind a consent banner (#684) and
  is off until `NEXT_PUBLIC_GA_MEASUREMENT_ID` is set; with it set, the privacy
  page names it. The other built-in signals are GitHub release download counts,
  stars and issues. Do not add any further tracking for the launch; the
  privacy page lists the analytics the site runs, and a launch post must not
  name one it does not.
