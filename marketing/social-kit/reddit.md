# Reddit: six posts, one per community

Six posts, written to be different on purpose. Each one talks the way people
in that community talk, says early that you made it, shares one thing, and
ends with a real question. Do not cross-post one text, and do not post two of
these on the same day (`calendar.md` spaces them out). Read
[`../brand-voice.md`](../brand-voice.md) before you edit any of them.

This supersedes the older text in `marketing/launch/reddit.md` for these six
subreddits. It is up to date for v0.9.3 (2026-10-27) and uses all three DJ
demos, and the DJ posts say the demo tracks are synthetic. (That file still
has posts for r/audioengineering, r/podcasting and r/tauri if you want them
later.)

**Links:**
site <https://edytlab.com> ·
DJ recordings <https://edytlab.com/use-cases/dj> ·
source <https://github.com/laadtushar/edytlab> ·
v0.9.3 <https://github.com/laadtushar/edytlab/releases/tag/v0.9.3>

**Videos:** `reel-beatmatched-transition.mp4`, `reel-extended-club-intro.mp4`,
`reel-mini-mix.mp4`, `reel-highlights.mp4`. Reddit video posts generally have
no body field, so if you upload a video, paste the body below as the first
comment. If a sub does not allow video, post the text and link the recordings
page.

## Before every post

1. **Re-read the sidebar the day you post.** No live sidebar could be read
   when this copy was written, so every rules note below is unverified. If a
   sub keeps self-promotion to a weekly or flagged thread, use that thread and
   skip the standalone post. If in doubt, message the moderators first.
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

**Rules note:** Check the sidebar. DJ communities often restrict
self-promotion and may only allow tool posts as discussion with disclosure, or
in a set thread. The post is written as show-and-tell and says it is for prep,
not live; if the rules do not allow it, ask a moderator or skip.

**Title:**

```
The night before a gig is all chores, so I built a free editor you can just tell what blend you want. Three recordings, prep only
```

**Body:**

```
Disclosure: I'm the developer. It's free and open source (MIT), no account.

You know the night before a gig. Finding the tempos, stretching one track to match, counting bars, drawing the fades, checking levels, bouncing a file. None of it is the fun part.

So I made an editor where you type the blend you want. In one of the recordings I ask it to bring a 128 BPM track in under the last 8 bars of a 120 BPM one. It slows the faster track to 120 without changing the pitch, starts it 8 bars before the first one ends, fades one out and the other in, and hands me a WAV. Every step shows up in the chat, so I can read it, undo it, or ask for the same blend with the incoming track starting 4 bars earlier.

The other two recordings build a longer club intro from a track's opening drums, and stitch three tracks into a short mix.

The tracks in them are short loops I generated, and a script types the requests into the real app. I only keep the runs that came out right, so they show the workflow, not how it copes with a messy real library.

It's for prep, not for playing live: it edits files, so there are no decks, and nothing for rekordbox or Serato. Beat detection is an estimate, so you still check a blend by ear.

Your music isn't uploaded. The chat goes to whichever AI you pick, or stays on your machine if you use a local model through Ollama.

Recordings: https://edytlab.com/use-cases/dj
Downloads for macOS, Windows and Linux (the installers are unsigned, so you'll see a warning the first time you open it): https://github.com/laadtushar/edytlab/releases/latest

For people who prep: which chore do you repeat the most, and what would make you not trust a tool like this?
```

---

## 2. r/WeAreTheMusicMakers

**Attach:** `reel-extended-club-intro.mp4`, or text only.

**Suggested flair:** "Feedback" or "Software / tools", whichever the sub offers.

**Rules note:** Check the sidebar. This sub has been wary of self-promotion
and may confine it to designated threads; if so, use the thread and shorten
the body. The post leads with a question, not a download.

**Title:**

```
What boring audio job pulls you out of a session? I built a free editor you talk to and I'm trying to learn which chores matter
```

**Body:**

```
Disclosure: I made this. It's free and MIT-licensed, and I'm mostly here to ask a question.

You're deep in a session and the idea is working. Then the loop needs to sit at 124 BPM, or the bounce is too loud, or there's dead air on the vocal take. You stop, do the chore, and the flow is gone.

That's the part I wanted to hand off. It's an editor where you type the job, like "time-stretch this to 124 BPM and keep the pitch", and it does it and shows you each step. Your files stay on your computer.

To be clear about what it isn't: it doesn't make music, and it isn't a DAW. No MIDI, no plugins. It works on audio files.

In one of the recordings on the site, I ask it to copy a track's first 8 bars of drums to make a 16-bar intro, hold the low end back until the drop, and fade the first 4 bars in. The track there is a short loop I generated, so it shows the workflow rather than real-world quality. The one thing I'd warn you about is that big stretches on dense material come out smeared, because the stretching is a phase vocoder I wrote myself.

So, the question: when you stop mid-session for a boring audio job, what is it? Matching a loop's tempo, trimming dead air, getting a bounce to a sensible loudness? Your answers decide what I build next.

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

**Rules note:** Check the sidebar. Production subs often restrict
self-promotion and may keep feedback requests to a dedicated thread. If so,
post the question part there and keep the link out of the main body.

**Title:**

```
How do you make DJ-friendly versions of your own tracks? I've been extending a drum intro to 16 bars by describing it, and I want to know what you do
```

**Body:**

```
Disclosure: I build this tool (free, MIT). The question is the point, so I'll start with it.

You finish a track and want a version a DJ can mix. So you stretch the intro and outro with drums only, to give them something to beat-match over. How do you do that? Copy and paste in the DAW, a separate pass in an audio editor, a dedicated extended-mix bounce?

Here's how it went in one recording of my tool. The track is at 124 BPM. I ask it to copy the drums-only first 8 bars and splice them in after themselves, so the intro runs 16 bars. Then it puts a high-pass filter on the intro so the low end waits for the drop on bar 17, fades the first 4 bars in, and exports a WAV.

It's an audio editor that an AI drives by calling editing tools. It's not a DAW, so no MIDI and no plugins. The demo track is a loop I generated with a drums-only intro written in, so that's a best case. A real arrangement with a pad or a vocal in bar 1 would be messier, and the recording doesn't show that.

Two things I'd like producers' opinions on:

1. Is copying the drum bars the right way to extend, or do you reach for something else, like looping a section or taking a tail from the previous bar?
2. What would you not trust an AI to do to your master?

Recording: https://edytlab.com/use-cases/dj
Free and open source for macOS, Windows and Linux (the installers are unsigned): https://github.com/laadtushar/edytlab/releases/latest
```

---

## 4. r/opensource

**Attach:** text post, optionally `reel-highlights.mp4` as a link or
comment; the point of this post is the project, not the demo.

**Suggested flair:** the sub's project or "promotional" flair, whichever it
requires.

**Rules note:** Check the sidebar. Project posts are usually the point of
this kind of sub, but there may be rules on promotion frequency, on
commercial projects, or on AI-assisted code. The body says up front that it's
mine, that I use Claude Code, and that a hosted option is planned.

**Title:**

```
edytlab: an MIT-licensed desktop audio editor you drive by chatting with an AI (Rust, Tauri 2). I'm looking for contributors and blunt feedback
```

**Body:**

```
Disclosure: it's my project, and I build it with Claude Code. Many commits carry a Co-Authored-By trailer, and the repo has a CLAUDE.md with the rules I work to.

You know the feeling of landing on a repo that says "contributions welcome" and not one word about where to start? I'm trying not to do that, so here is where help would matter most.

edytlab is a desktop audio editor for macOS, Windows and Linux. You say what edit you want in a chat box, and an AI picks from 99 editing tools that run in a Rust engine on your machine. The model never receives your audio, only text about it. The code is here: https://github.com/laadtushar/edytlab

Where I'd love a hand:

- Making more effects run live. Today a gain, a limiter and three filters can sit on the master bus or a bus. Reverb, echo, EQ and the compressor still get baked into a track's audio. That's Rust work in the audio-engine and tools crates (issue 520 has the background).
- A Mac. The macOS build has the microphone permission set up, but nobody has checked it on a signed, notarized build (issue 464).
- A Linux desktop with a locked Secret Service, to see what happens to saved keys (issue 497).
- Stem separation. It isn't built yet (issue 385), and it's the one I'd most like to see land.
- New audio tools or new AI providers. docs/contributing.md walks through both.

Five open issues are labelled "good first issue" (#846 to #850: a wah-wah effect, a curve option for fade, a mute-all-tracks tool, and tests for mute, solo and pan). Each one says where in the code to start.

Things this crowd will ask:

- Cost: the app is free. With a hosted AI you pay that provider under your own key, a local model through Ollama costs nothing, and you can use a ChatGPT Plus or Pro plan in place of a key (a preview). A hosted option is on the roadmap with pricing undecided. The desktop app and the engine are MIT today.
- Telemetry: none in the desktop app.
- Signing: the installers are unsigned, so macOS and Windows warn on first launch (issue 386).

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
models and has had rules about self-promotion and disclosure. The post says
plainly that it is not a local-model-first app yet and that I have no
local-model results, which is why it is written as a request for them. If the
sub requires an open-source or local-first project, this qualifies on the
first and only partly on the second.

**Title:**

```
I built an open-source audio editor where the model calls 99 tools, with an Ollama provider. I haven't tested local models properly, so I'm asking for your results
```

**Body:**

```
Disclosure: my project (MIT). I'm posting here because of the Ollama provider, and because I need data I don't have.

Anyone who has pointed a small local model at a tool-calling app knows how it goes: the tool schemas alone eat the context window. I hit it with my own app. edytlab is a desktop audio editor, and the model chooses and orders calls to 99 audio tools (EQ, fades, time-stretch, loudness, export) that run in a Rust engine on your machine. The model never receives audio, only text: the session state, measurements like tempo and loudness, and tool results.

At first a one-line request, "Make track 0 louder by 6 dB.", produced a first request of about 15,000 tokens. I pointed the Ollama provider at llama.cpp's llama-server running Qwen2.5-3B with an 8,192-token context, and it refused the request outright. The numbers are in issue 395.

So now Ollama gets a smaller list. It's sent a core set of 15 tools plus the ones your message names, with shortened descriptions, and the names of the rest go in one line of the system prompt so the model knows they exist. The repo's request-size test puts that same request at about 8.7 KB of text, against about 67 KB with every tool. That's bytes, not tokens, and I haven't re-run Qwen2.5-3B on the new request. If a server refuses a request as too long, the app now says so and what to change.

There's no key for Ollama. It uses the OpenAI-compatible endpoint (default http://localhost:11434/v1), and Test in Settings sends a small request to check that the model can call tools.

What I haven't done is run a local model through a full job. The demo recordings were made with Claude. I don't know which local models can drive this tool list without losing the thread, and I'm not going to guess. I also don't know how often the trimming hides a tool the model needed: a request described by effect ("make it sound like a cathedral") doesn't carry the reverb schema.

What I'd like opinions on: is a core set plus a names line the right shape, or would you rather the model call a search tool to fetch a schema when it needs one?

If you try it, tell me the model, the quantisation, the context size, whether it kept to the schemas, and how it failed. Failure reports are as useful as successes.

Source (issue 395 has the old numbers): https://github.com/laadtushar/edytlab
Downloads for macOS, Windows and Linux (unsigned, so expect a first-launch warning): https://github.com/laadtushar/edytlab/releases/latest
```

---

## 6. r/rust

**Attach:** none needed. A text post. Link the recordings at the end only.

**Suggested flair:** the sub's project flair.

**Rules note:** Check the sidebar. r/rust has a project flair and moderators
have pushed back on low-effort or AI-generated project posts, so check
whether there is a rule about AI-assisted projects or a showcase thread, and
follow it. The post says in the first line that Claude Code was used and puts
the engineering first, which is the best defence.

**Title:**

```
I dropped Rubber Band for a hand-written phase vocoder in a Tauri 2 audio editor. Notes on content-addressed undo and lock discipline, and where I want review
```

**Body:**

```
Disclosure: my project (MIT), developed with Claude Code; the commit trailers and CLAUDE.md say so. I'm posting engineering notes, and I'd like specific review.

I planned to use Rubber Band for time-stretching. It needs librubberband-dev on Linux, vcpkg on Windows and Homebrew on macOS, and a native dependency like that breaks every CI target at once. So I dropped it and wrote a phase vocoder on realfft instead, with onset-triggered phase resets to protect transients. The cost is written at the top of crates/audio-time/src/vocoder.rs: dense mixes smear, and the further the factor is from 1.0 the worse it gets.

That's one piece of a desktop audio editor where an LLM agent calls 99 deterministic tools, a Rust workspace of 17 crates behind a Tauri 2 and React shell. A few other parts that might interest this sub:

audio-dsp has no dependencies, on purpose. It's functions over &mut [f32] plus a sample rate and channel count, with no session types and no IO. It sits below both the tools crate and the renderer, so a dependency there would be a dependency everywhere. It exists because the effect algorithms used to be pub(crate) inside tools and the renderer needed them too, which meant a dependency cycle or a second copy.

Session state is a content-addressed DAG: a node's id is the blake3 of its state. Two routes to the same state are one node, and it keeps the parent from its first visit. That bit me. Undo that followed node.parent went to the wrong place after mute then unmute (#398). The fix isn't in the graph. The frontend keeps the path the user actually took, as one reducer for head, undo trail and redo list (headTrail.ts), and that path is now saved with the project so undo survives a restart. I'm not sure it's the right model, and I'd like to hear how you'd do it.

Lock discipline was the other lesson. A long tool held the store lock while synchronous Tauri commands waited on it from the main thread, and the window froze (#421). Those commands now run off the main thread. A tool that opts in runs on a staged store handle with no lock held and commits only if the head hasn't moved. If it has, the tool runs again on the new head, and a test pins which tools opt in.

Where I'd like review: the vocoder's transient handling, and the lock discipline in the agent loop. Stem separation is still a stub (#385).

Source: https://github.com/laadtushar/edytlab
Demo recordings (DJ jobs, screen-captured from the real app): https://edytlab.com/#demos
```

---

## Notes (not part of the posts)

Facts to re-check on the day:

- Issue numbers and open/closed state. On 2026-10-25: closed are 460 (PR 516),
  453 (PR 521) and 520 (all in v0.6.0), 421, 395 and 394 (all in v0.5.0), and
  383 and 384 (the ONNX Runtime and model delivery, and Whisper transcription:
  the installers carry the runtime from v0.7.0, #615, and the model download
  and transcription come with v0.8.0); open are 464, 497, 389, 386 and 385
  (stem separation), and 474 (Sign in with ChatGPT, whose preview shipped in
  v0.9.0). The r/opensource post now names only 385 for the ML pieces that are
  still missing. Re-checked on 2026-10-27: 464, 497, 389, 386, 385 and 474 are
  still open; 567 (the Mac checks, passed on v0.9.0 and v0.9.1) is closed.
- The good first issue sentence names #846 to #850, which carried the label and
  were open when checked on 2026-10-27. Re-check that they are still open (and
  still labelled) before posting r/opensource; edit the sentence if one has been taken.
- r/LocalLLaMA numbers: 15,157 tokens (posted as "about 15,000") and the 8,192
  context are v0.4.0's, from issue 395. The 8.7 KB / 67 KB figures are from
  `crates/ai/tests/request_size.rs` (`cargo test -p ai --test request_size --
  --nocapture`); they are bytes. Measured on v0.7.0 on 2026-10-22: Ollama's
  slim request 8,748 B (2,343 B of system prompt, 6,252 B of tools), a
  chat-completions request with all 95 tools 67,130 B, Anthropic's 64,488 B.
  The full figure was 58 KB on v0.5.0 and 66 KB on v0.6.0 (the table in
  `docs/architecture.md` is v0.6.0's). Re-run the test on the day; if the
  numbers have moved, change them in the post.
- The "Master" workaround is from issue 460. PR 516 closed it, and v0.6.0 is
  the first release with the master-bus tool; v0.5.0 and earlier still render a
  baked "Master" track. The posts no longer mention it, but if someone asks, the
  recordings were made after PR 516.
- "A script types the requests" and "I only keep the runs that came out right"
  (r/DJs): `apps/desktop/e2e-native/stories/demos.mjs` and
  `make-demo-videos.mjs`. The site captions do not say this, so it appears
  only in r/DJs here. Add it to others if asked.
- The posts leave out the lists the older text carried (every AI provider, the
  file formats, the loudness and limiter figures). If someone asks, the answers
  are on the site and in `marketing/README.md`, "The accuracy rules".
