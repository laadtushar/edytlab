# Reddit: six posts, one per community

Six posts written to be different on purpose. Each leads with something that
community can use, says early that you made it, lists the limits, and ends
with a question. Do not cross-post one text, and do not post two of these on
the same day (`calendar.md` spaces them out).

This supersedes the v0.3.0-era text in `marketing/launch/reddit.md` for these
six subreddits: it is updated for v0.7.0 (2026-10-22), includes the other two DJ demos, and
discloses that the demo tracks are synthetic. (That file still has posts for
r/audioengineering, r/podcasting and r/tauri if you want them later.)

**Links:**
site <https://edytlab.com> ·
DJ recordings <https://edytlab.com/use-cases/dj> ·
source <https://github.com/laadtushar/edytlab> ·
v0.7.0 <https://github.com/laadtushar/edytlab/releases/tag/v0.7.0>

**Videos:** `reel-beatmatched-transition.mp4`, `reel-extended-club-intro.mp4`,
`reel-mini-mix.mp4`, `reel-highlights.mp4`. Reddit video posts generally have
no body field, so if you upload a video, paste the body below as the first
comment. If a sub does not allow video, post the text and link the recordings
page.

## Before every post

1. **I could not read any live sidebar from this environment.** The rules
   notes below say what to check and, where I have a general expectation,
   what it is. Treat every one as unverified and re-read the rules the day
   you post. If a sub restricts self-promotion to a weekly or flagged thread,
   use that thread and skip the standalone post. If in doubt, message the
   moderators first.
2. Say you made it in the first sentence (each body below does).
3. Stay for the replies for the first several hours. A drive-by post reads as
   spam.
4. Do not ask for upvotes or stars.
5. If anyone asks whether AI helped build it, say yes: the repo is developed
   with Claude Code.
6. Flair names change and differ per sub. The flairs below are suggestions by
   meaning, not exact labels.

---

## 1. r/DJs

**Attach:** `reel-beatmatched-transition.mp4`, or text with the DJ page link.

**Suggested flair:** the one closest to "software", "tools" or "feedback".

**Rules note:** Check the sidebar. DJ communities commonly restrict
self-promotion and may only allow tool posts as discussion with disclosure, or
in a set thread. The post below is written as show-and-tell with the limits up
front; if the rules do not allow it, ask a moderator or skip.

**Title:**

```
I built a free tool that preps a blend from a plain-English description. Three recordings, including where it flags its own problems
```

**Body:**

```
Disclosure: I'm the developer. It's free and open source (MIT), no account.

The itch: prepping a blend is a pile of small file chores. Find the tempos, stretch one track, line up the downbeats, draw the fades, put a filter on the outgoing track, check loudness, bounce a file. edytlab is a desktop app where you describe those chores and an AI agent runs them as ordinary audio tools on your files. It's for prep, not for playing live.

The recordings show three jobs:

1. Both tracks in F major, at 120 and 128 BPM. The 128 track is time-stretched to 120 (pitch kept) and started under the last 8 bars of the other. The outgoing track fades under a 1 kHz low-pass while the incoming one fades in. Mastered and exported as a 50-second WAV at -13.9 LUFS, peaking at -1 dBFS.
2. An extended intro: the drums-only first 8 bars are copied and spliced in, so the intro runs 16 bars, with a 200 Hz high-pass holding the low end back until the drop.
3. A three-track mix at 124 BPM with overlaps of about 4 bars and crossfades, about -14 LUFS each, limited at -1 dBFS, exported as a 79-second WAV.

What to know before deciding whether it's worth your time:

- The tracks are short synthetic loops (about 32 seconds each) that I generated. The recordings show the workflow, not how it copes with a messy real library.
- Beat and key detection are software estimates. A drifting tempo or a long ambient intro can fool them.
- The agent flags things instead of hiding them: bars that run a beat behind where it was told to start a track (with the fix), an export peaking right at full scale, a loudness reading that counted the fades.
- The recordings were made on a development build just before v0.6.0, and v0.6.0 and later do what they show: the limiter goes on the master bus, so it keeps working when a track changes. v0.5.0 and earlier had no master-bus insert and rendered the tracks into a baked "Master" track instead (issue 460). Only gain, the limiter and three filters run live on the master or a bus; for the compressor in the first demo the agent still mixes the two tracks to a new one.
- No decks, controllers or live mode. It exports WAV, FLAC or MP3 files and has nothing for rekordbox or Serato. No stem separation or acapellas; that isn't built.
- A test script types the requests into the real app when I record, and I only publish runs that passed my checks. The agent isn't deterministic, so your results can differ.
- It needs an AI provider: your own key for Anthropic, OpenAI, OpenRouter, Groq or Gemini, or a local model through Ollama. Your audio files aren't uploaded; the chat text goes to the provider you pick, or stays local with Ollama.

Recordings: https://edytlab.com/use-cases/dj
Downloads for macOS, Windows and Linux (unsigned, so expect a first-launch warning): https://github.com/laadtushar/edytlab/releases/latest

For people who prep: which chore do you repeat the most, and what would make you not trust a tool like this?
```

---

## 2. r/WeAreTheMusicMakers

**Attach:** `reel-extended-club-intro.mp4`, or text only.

**Suggested flair:** "Feedback" or "Software / tools", whichever the sub offers.

**Rules note:** Check the sidebar. This sub has historically been wary of
self-promotion and may confine it to designated threads; if so, use the
thread and shorten the body. The post leads with a question, not a download.

**Title:**

```
What audio chore makes you stop mid-session? I built a free editor that takes plain-English instructions and I'm trying to learn which jobs matter
```

**Body:**

```
Disclosure: I made this. It's free and MIT-licensed, and I'm mostly here to ask a question.

It doesn't generate music and it isn't a DAW: no MIDI, no plugin hosting. It edits audio files. You describe a job in a chat box (for example "time-stretch this to 124 BPM keeping the pitch", or "bring it to -14 LUFS") and an AI agent runs ordinary tools on the file: time-stretch, pitch-shift, fades, EQ, filters, reverb, echo, compression and limiting, loudness, export as WAV, FLAC or MP3. Every edit is a step in a history you can fork, A/B against the previous version, and go back through.

An example from the recordings on the site: a track's drums-only first 8 bars are copied and spliced in to make a 16-bar intro, a 200 Hz high-pass holds the low end back until the drop, and the first 4 bars fade in. The agent also warned that the export peaked right at full scale, which I'd rather it said than not. The track there is a short synthetic loop I generated, so it shows the workflow, not real-world quality.

Honest limits: time-stretch and pitch-shift use a phase vocoder I wrote, so big changes on dense material sound smeared. No stems or vocal isolation (not built). Your audio isn't uploaded; the chat text goes to an AI provider you choose, or stays local with Ollama. The installers are unsigned, so macOS and Windows warn on first launch.

The question: when you stop in the middle of a session to do a boring audio job, what is it? Tempo-matching a loop, trimming dead air, getting a bounce to a sensible loudness? That list decides what I build next.

Site and recordings: https://edytlab.com
Source and downloads: https://github.com/laadtushar/edytlab
```

---

## 3. r/edmproduction

**Attach:** `reel-extended-club-intro.mp4` (this is the only post that leads
with the intro demo; r/WeAreTheMusicMakers uses it as an example, not the
lead).

**Suggested flair:** "Feedback", "Discussion" or "Production talk", whichever
the sub has.

**Rules note:** Check the sidebar. Production subs commonly restrict
self-promotion and may keep feedback requests to a dedicated thread. If so,
post the question part there and keep the link out of the main body.

**Title:**

```
Extending a drum intro to 16 bars and holding the low end back until the drop, by describing it. How do you make DJ-friendly edits of your own tracks?
```

**Body:**

```
Disclosure: I build this tool (free, MIT). The question is the point, so I'll start with it.

When you finish a track and want a version DJs can mix, you probably extend the intro and outro with drums only, so there's something to beat-match over. How do you do it: copy and paste in the DAW, a separate audio-editor pass, a dedicated extended-mix bounce?

Here is how it went in a recording of my tool. The track is at 124 BPM with 1.935-second bars and the first downbeat at 0.476 s (the agent works that out and says so). The drums-only first 8 bars are copied from that downbeat and spliced in after themselves, so the intro runs 16 bars. A 200 Hz high-pass holds the low end back until the drop on bar 17, the first 4 bars fade in, and the result is exported as a WAV. The agent flagged that the export peaks right at full scale.

What it is and isn't: a desktop editor for audio files where an AI agent calls tools (there are 94). It is not a DAW. No MIDI, no plugin hosting, and no stem separation (not built). The demo track is a short synthetic loop I generated with a drums-only intro written in, so this is a best case. A real arrangement with a pad or a vocal in bar 1 would be messier, and the recording doesn't show that. Beat detection is an estimate.

Two things I'd like producers' opinions on:

1. Is copying the drum bars the right way to extend, or do you reach for something else (looping a section, a tail from the previous bar)?
2. What would you not trust an agent to do to your master?

Recording: https://edytlab.com/use-cases/dj
Free and open source for macOS, Windows and Linux (unsigned installers): https://github.com/laadtushar/edytlab/releases/latest
```

---

## 4. r/opensource

**Attach:** text post, optionally `reel-highlights.mp4` as a link or
comment; the point of this post is the project, not the demo.

**Suggested flair:** the sub's project or "promotional" flair, whichever it
requires.

**Rules note:** Check the sidebar. Project posts are usually the point of
this kind of sub, but there may be rules on promotion frequency, on
commercial projects, or on AI-assisted code. The body discloses all three up
front.

**Title:**

```
edytlab: an MIT-licensed desktop audio editor driven by an AI agent (Rust, Tauri 2). Looking for contributors and blunt feedback
```

**Body:**

```
Disclosure: it's my project.

edytlab is a desktop audio editor for macOS, Windows and Linux. You describe an edit in a chat box and an AI agent calls deterministic audio tools (94 of them) that run locally in a Rust engine. The model never receives the audio, only text about it. It's MIT-licensed: https://github.com/laadtushar/edytlab. v0.7.0 is the current release.

Where I'd like help:

- Effects on a chain (issue 520, closed, has the background). Since v0.6.0 the agent can put an effect on the master bus and on a bus, but only five kinds run in a chain: gain, the limiter and three filters. Still missing: streaming versions of the other eleven (reverb, echo, EQ, compressor and others), which today can only be baked into a track. Rust, in the audio-engine and tools crates.
- macOS microphone (issue 464). Since v0.5.0 the build has the entitlement and the usage text, but it hasn't been checked on a signed, notarized macOS build. That needs a Mac.
- A real pass on Linux with a locked Secret Service (issue 497). The code is tested against fakes only.
- Shipping the ML pieces (issues 383 to 385). Stem separation (Demucs) and transcription (Whisper) are wired in as tools, but their inference is not shipped. The ONNX Runtime ships inside the installers now; model delivery and the inference itself are what's left.
- New audio tools and new LLM providers. docs/contributing.md walks through both. Six providers today: Anthropic, OpenAI, OpenRouter, Groq, Gemini, and Ollama for local models.

No open issue carries the "good first issue" label right now. If you want a starter task, say so and I'll write one up.

Things an open-source crowd will ask about:

- AI: I develop it with Claude Code. Many commits carry a Co-Authored-By trailer, and the repo has a CLAUDE.md with the working rules.
- Cost: the app is free. With a hosted provider you pay that provider per token under your own key; Ollama is free. A hosted option that bundles AI inference is on the roadmap with pricing undecided, and issue 389 evaluates a browser-based version. The desktop app and audio engine are MIT today.
- Telemetry: none in the desktop app.
- Signing: the installers are unsigned (certificates are not provisioned yet, issue 386), so macOS and Windows warn on first launch.

What would make you trust it, or not?
```

---

## 5. r/LocalLLaMA

**Attach:** none needed. A text post; link the repo and issue 395 (closed by
the v0.5.0 change, with the old numbers). If you attach a video, make clear it
was recorded with a hosted model.

**Suggested flair:** the sub's "Discussion" or "Resources" style flair
(whichever fits a project asking for data).

**Rules note:** Check the sidebar. This sub expects posts to be about local
models and has had rules about self-promotion and disclosure. The post is
honest that it is not a local-model-first app yet and that the author has no
local-model success data, even after the smaller tool list; that is also why it
is written as a request for results. If the sub requires an open-source or
local-first project, this qualifies on the first and only partly on the second.

**Title:**

```
Open-source desktop app where the model calls 94 audio tools, with an Ollama provider. What I've measured with local models (not much) and what I haven't
```

**Body:**

```
Disclosure: my project (MIT). I'm posting here because of the Ollama provider, and because I need data I don't have.

edytlab is a desktop audio editor. You describe an edit and the model chooses and orders calls to 94 deterministic audio tools (EQ, fades, time-stretch, loudness, export). The tools run in a Rust engine on your machine. The model never receives audio, only text: the session state, measurements such as tempo and loudness, and tool results.

What exists for local models:

- An Ollama provider. No key. It talks to the OpenAI-compatible endpoint (default http://localhost:11434/v1). In Settings, Test sends a minimal request to check that the model can call tools.
- A smaller tool list (since v0.5.0). Ollama is sent a core set of 15 tools plus the ones your message names (by name, a distinctive word, or a phrase like "remove the reverb"), with shortened descriptions. The names of the rest go in one line of the system prompt, so the model knows they exist and can call them. Hosted providers still get every tool.
- If a server refuses a request as too long, the app now says so and what to change. llama.cpp refuses; Ollama trims an over-long prompt silently, and the app shows nothing.
- "Plan first" (the agent shows its steps and nothing runs until you approve) was being sent in the wrong format to Ollama, Groq and Gemini until v0.3.0. That's fixed. I haven't verified it end to end with a real local model.

What I have measured:

- Before the change (v0.4.0), a one-line request ("Make track 0 louder by 6 dB.") produced a 15,157-token first request. I pointed the Ollama provider at llama.cpp's llama-server running Qwen2.5-3B with an 8,192-token context, and it refused the request outright. The numbers are in issue 395.
- After it, the repo's request-size test (`crates/ai/tests/request_size.rs`, against a mock server) puts that same request at about 8.7 KB of text, against about 67 KB with every tool on v0.7.0: roughly 2.3 KB of system prompt and 6.3 KB of tool schemas. That is bytes, not tokens. I did not run a tokenizer, and I have not re-run llama-server with Qwen2.5-3B on the new request.

What I haven't done:

- I have not run a local model through a full job. The demo recordings on the site were made with Claude as the agent, not a local model. I don't know which local models can drive this tool list without losing the thread, and I'm not going to guess.
- I haven't compared quantisations or context settings.
- I don't know how often the filtering hides a tool the model needed. A tool is sent in full only when the message names it or it is in the core, so a request described by effect ("make it sound like a cathedral") does not carry the reverb schema.

What I'd like opinions on: the selection is a heuristic (words and phrases in the message, the assistant's last message, tools used earlier). Is a core set plus a names line the right shape, or would you rather have a search tool the model calls to fetch a schema? A capabilities blacklist and agent profiles can also narrow the list by hand; I haven't tried that with local models either.

If you try it: which model, quantisation and context size, whether it kept to the tool schemas, and how it failed. Failure reports are as useful as successes.

Source (issue 395 has the v0.4.0 numbers): https://github.com/laadtushar/edytlab
Downloads for macOS, Windows and Linux (unsigned, so expect a first-launch warning): https://github.com/laadtushar/edytlab/releases/latest
```

---

## 6. r/rust

**Attach:** none needed. A text post. Link the recordings at the end only.

**Suggested flair:** the sub's project flair.

**Rules note:** Check the sidebar. r/rust has a project flair and moderators
have pushed back on low-effort or AI-generated project posts, so check
whether there is a rule about AI-assisted projects or a showcase thread, and
follow it. The post discloses the Claude Code use in the first line and gives
the technical substance first, which is the best defence.

**Title:**

```
edytlab: a Tauri 2 audio editor with a zero-dependency DSP crate and a hand-written phase vocoder. Notes on dropping Rubber Band, content-addressed undo and lock discipline
```

**Body:**

```
Disclosure: my project (MIT), developed with Claude Code; the commit trailers and CLAUDE.md say so. I'm posting engineering notes, and I'd like specific review.

It's a desktop audio editor where an LLM agent calls 94 deterministic tools: a Rust workspace of 16 crates behind a Tauri 2 + React shell. The parts that might interest this sub:

- audio-dsp has no dependencies, on purpose: functions over &mut [f32] plus a sample rate and channel count, no session types, no IO. It sits below both the tools crate and the renderer, so a dependency there is a dependency everywhere. It exists because the effect algorithms used to be pub(crate) inside tools and the renderer needed them too, which meant a dependency cycle or a second copy.
- Time-stretch and pitch-shift are a phase vocoder on realfft, with onset-triggered phase resets (spectral-flux onset detection, causal thresholding) to protect transients. I planned on Rubber Band and dropped it: it needs librubberband-dev on Linux, vcpkg on Windows and Homebrew on macOS, and a native dependency like that breaks every CI target at once. The limits are written at the top of crates/audio-time/src/vocoder.rs: dense mixes smear, and the further the factor is from 1.0 the worse.
- Session state is a content-addressed DAG: a node's id is the blake3 of its state. Two routes to the same state are one node, and that node keeps the parent from its first visit. Undo that followed node.parent broke on mute-then-unmute (#398). The fix in v0.4.0 is not in the graph: the frontend keeps the path the user took, as one reducer for head, undo trail and redo list (headTrail.ts). Since v0.6.0 that path is saved with the project (a small trail.json beside the graph, #453), so undo survives a restart; before, it fell back to the parent. I'm not sure that's the right model, and I'd like to hear how you'd do it.
- Tools are registered in a dispatcher with JSON-schema validation, and docs/tools-reference.md is generated from the registry by a test, so the doc can't disagree with what the agent can call.
- Lock discipline: a long tool held the store lock while synchronous Tauri commands waited for it on the main thread, which froze the window (#421). Moving those commands off the main thread shipped in v0.3.0 and v0.5.0. Also in v0.5.0, a tool that opts in runs on a staged store handle with no lock held and commits only if the head hasn't moved; if it has, the tool runs again on the new head, and a list in a test pins which tools opt in. The convention for the rest is to take and drop the store lock before touching the engine lock.
- LLM providers sit behind an LlmProvider trait; request serialisation and SSE parsing are per provider.
- Tests: cargo test, vitest, Playwright against a production build of the frontend with only the IPC boundary replaced (its fake backend answers from the Rust source), and a native suite that drives the real binary with WebDriver. That native suite is also how the demo videos are recorded.

Where I'd like review: the vocoder's transient handling, and the lock discipline in the agent loop.

What isn't built: ONNX stem separation and transcription are stubs. The installers carry the ONNX Runtime now, but nothing runs a model on it (#383 to #385). A checksummed, resumable model downloader exists, but nothing calls it.

Source: https://github.com/laadtushar/edytlab
Demo recordings (DJ jobs, screen-captured from the real app): https://edytlab.com/#demos
```

---

## Notes (not part of the posts)

Facts to re-check on the day:

- Issue numbers and open/closed state. On 2026-10-22: closed are 460 (PR 516),
  453 (PR 521) and 520 (all in v0.6.0), and 421, 395 and 394 (all in v0.5.0);
  open are 464, 497, 389, 386, 384 and 385, and 383 (the ONNX Runtime and
  model delivery): the installers carry the runtime from v0.7.0 (#615), but no
  model is downloaded and no inference runs, so keep saying "383 to 385". They
  were all open when this was written (2026-10-10).
- "No open issue carries the good first issue label": true when checked on
  2026-10-22 (15 open issues, none labelled). Label two issues before posting r/opensource
  and replace that sentence with their numbers.
- r/LocalLLaMA numbers: 15,157 tokens and the 8,192 context are v0.4.0's, from
  issue 395. The 8.7 KB / 67 KB figures and the 2.3 KB / 6.3 KB split are from
  `crates/ai/tests/request_size.rs` (`cargo test -p ai --test request_size --
  --nocapture`); they are bytes. Measured on v0.7.0 on 2026-10-22: Ollama's
  slim request 8,748 B (2,343 B of system prompt, 6,252 B of tools), a
  chat-completions request with all 94 tools 67,130 B, Anthropic's 64,488 B.
  The full figure was 58 KB on v0.5.0 and 66 KB on v0.6.0 (the table in
  `docs/architecture.md` is v0.6.0's).
- The "Master" workaround is from issue 460. PR 516 closed it, and v0.6.0 is
  the first release with the master-bus tool; v0.5.0 and earlier still render a
  baked "Master" track. The recordings were made after PR 516: the mini-mix
  caption in `website/lib/demos.ts` puts its limiter on the master, and the
  beatmatch mixes to a new track only to compress it.
- "A test script types the requests" and "only runs that passed are
  published": `apps/desktop/e2e-native/stories/demos.mjs` and
  `make-demo-videos.mjs`. The site captions do not say this, so it appears
  only in r/DJs here. Add it to others if asked.
