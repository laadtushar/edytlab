# LinkedIn: five posts

Five posts with different angles, written in your voice. Post one a few days
apart, not back to back (see `calendar.md`). Each has a video to attach as
native video and a one-line comment to pin right after you publish. Each also
has a square image, and post 3 has a carousel PDF to post in place of its reel;
both are in the new design of the site, under `linkedin/` (see
[`linkedin/README.md`](linkedin/README.md)).

**Videos:** `reel-highlights.mp4`, `reel-beatmatched-transition.mp4`,
`reel-mini-mix.mp4`, `reel-extended-club-intro.mp4`, `reel-teaser.mp4`. The
reels carry their captions on screen, so there is no `.srt` to upload to
LinkedIn's captions option yet (narrated versions with `.srt` captions will
come once narration credits are available).

**Links:**
site <https://edytlab.com> ·
source <https://github.com/laadtushar/edytlab> ·
download <https://github.com/laadtushar/edytlab/releases/latest> (the newest
release, v0.9.2 on 2026-10-26; check before each post that the version a post
names is still the newest)

**Conventions used in every post:** no emoji; 3 to 5 hashtags at the end;
only post 1 has a link in the body, the others carry the link in the pinned
comment, because outbound links in the body are commonly reported to reduce
reach (not verified here). Wherever a download is mentioned, the installers
are described as unsigned.

---

## 1. Launch: what it is (attach `reel-highlights.mp4`)

Angle: the one-paragraph product, the three demos in one breath, the honest
limits. About 250 words.

```
I built an audio editor you talk to. The video is three DJ jobs, each described in plain English and ending in an exported WAV.

A 128 BPM track time-stretched to match a 120 BPM one, started under the last 8 bars of the first, crossfaded with a low-pass on the outgoing track, mastered to about -14 LUFS. An extended club intro, made by copying a track's drums-only first 8 bars. A three-track mini-mix at 124 BPM with overlaps of about 4 bars.

What I like best: the agent flags its own edge cases. Bars that run a beat behind, with the fix. An export peaking right at full scale. A loudness reading that counted the fades.

How it works: the model never hears the audio. It gets text about the session (tempo, key, loudness) and picks from 94 audio tools. The DSP runs on your machine, in Rust. Your files aren't uploaded; only the chat goes to the provider you choose (Anthropic, OpenAI, OpenRouter, Groq, Gemini, or your ChatGPT plan, a preview), or stays local with Ollama.

Honest limits: the demo tracks are short synthetic loops I generated, and the agent in the recordings is Claude. It preps files; it isn't a live DJ tool. No stem separation yet. The installers are unsigned, so macOS and Windows warn on first launch.

Free, open source (MIT), v0.9.2 for macOS, Windows and Linux. DJs: which prep job would you hand off first?

Demos and downloads: https://edytlab.com

#OpenSource #DJing #AudioEngineering #LocalFirst #AIAgents
```

**Pin this comment:**

```
Source (MIT) and installers for macOS, Windows and Linux: https://github.com/laadtushar/edytlab/releases/latest (unsigned, so expect a first-launch warning).
```

**Image:** `linkedin/out/post-1-launch.png` (1200x1200). Use it as the video's
cover if LinkedIn's uploader offers a custom thumbnail, or post it alone. The
chat in it is the app's real chat panel with a scripted example conversation (the
numbers are fixture values, not a model's output), and the image says so.

**Shorter version (under 90 words), same video:**

```
I built an audio editor you talk to. The video: three DJ jobs, each described in plain English and ending in an exported WAV. A beatmatched blend, an extended club intro, a three-track mini-mix. The agent even flags its own edge cases, like bars that run a beat behind.

Free and open source (MIT) for macOS, Windows and Linux. Audio is processed on your machine. It preps files, it doesn't play live, and the installers are unsigned, so expect a first-launch warning.

https://edytlab.com

#OpenSource #DJing #AudioEngineering
```

---

## 2. The DJ workflow, before and after (attach `reel-beatmatched-transition.mp4`)

Angle: a chore list, then the same list done by the agent, with the one moment
that builds trust. About 250 words.

```
Prepping one blend between two tracks is a pile of small chores. Find both tempos. Stretch one to match. Line up the downbeats. Draw the fades. Put a filter on the outgoing track. Check the loudness. Export a file.

Before: that list, by hand, in an editor, every time.

After, in the video: a DJ describes the blend and an AI agent works through the list on the files.

- It reports what it's working with: the 128 BPM track is about 6.7% faster than the 120 BPM one.
- It time-stretches the 128 BPM track to 120, keeping its pitch, and starts it at 16 seconds, under the last 8 bars of the other.
- The outgoing track fades under a 1 kHz low-pass while the incoming one fades in.
- The mix is mastered and exported as a 50-second WAV at -13.9 LUFS, peaking at -1 dBFS.

The moment I trust it most: it tells the DJ that at 16 seconds the incoming track's bars run a beat behind, and offers to start it at 15.5 instead, rather than quietly leaving the blend a little off.

Limits, so nobody has to find them: the tracks are short synthetic loops I generated, so this shows the workflow, not how it copes with a real record. Beat detection is an estimate and can be wrong on a drifting tempo. This is prep for files, not a live tool.

Which step of your own prep would you hand off first?

#DJing #DJTips #AudioEngineering #OpenSource
```

**Pin this comment:**

```
The DJ walkthrough, with all three recordings and the full list of what it can't do: https://edytlab.com/use-cases/dj
```

**Image:** `linkedin/out/post-2-before-after.png` (1200x1200), same use, and the
same scripted example conversation, captioned as one.

---

## 3. The build story (attach `reel-mini-mix.mp4`)

Angle: the design decision everything hangs off (the model never touches a
sample), then three engineering choices, one of them a bug story. About 250
words.

```
The audio editor I've been building has an AI agent in it, and the model never touches a sample. Everything else follows from that.

The agent chooses and orders 94 audio tools: fades, EQ, filters, time-stretch, loudness, export. The tools are plain Rust functions, with no model in the audio path. The video shows it sequencing a three-track mix with them.

What I'd tell someone starting this:

- The stack is a Tauri 2 shell, a React front end and a Rust workspace behind it. The effects crate has zero dependencies, on purpose.
- I wanted Rubber Band for time-stretch and dropped it. A native dependency needs a different package on each of three CI targets and breaks all three builds at once. So the phase vocoder is mine, and the cost is that dense mixes smear at large changes.
- Every edit is a node in a history graph, with fork and A/B compare. Nodes are named after their content, so mute then unmute lands back on an old node, and undo used to jump to that node's first parent. v0.4.0 fixes it: undo and redo follow the path you actually took.
- Plan first is an optional mode where nothing changes until you approve the agent's steps.

I build it with Claude Code, and the repo has a CLAUDE.md with the working rules.

Source is MIT, if you want to read how any of it works. What would you want to review first?

#Rust #Tauri #OpenSource #AIAgents #AudioEngineering
```

**Pin this comment:**

```
Every tool, with its parameters, generated from the tool registry: https://github.com/laadtushar/edytlab/blob/main/docs/tools-reference.md
```

**Image and carousel:** `linkedin/out/post-3-model-never-touches-a-sample.png`
(1200x1200), and `linkedin/out/post-3-carousel.pdf`, seven 1080x1350 pages
that walk through the same four points with screenshots of the app (the real
interface; the conversation, plan and history in them are scripted examples, and
each page says so). LinkedIn
takes a PDF as a document post, so post it instead of the reel. If you do,
delete the sentence in the post's second paragraph that says what the video
shows: there is no video in that version.

---

## 4. Privacy and local-first, with the Ollama option (attach `reel-extended-club-intro.mp4`)

Angle: answer the first question people ask about AI audio tools, precisely,
including the part that does not work well yet. About 330 words.

```
"Does my audio get uploaded?" is the first question about any AI audio tool, so here is the exact answer for edytlab.

Your audio files: no. Decoding, editing, mixing and export all run on your computer, in a Rust engine. The extended intro in the video was built that way.

What does leave: the chat. Your messages, plus the text the agent works from (a track's tempo, key and loudness, and the results of its tool calls), go to the AI provider you picked: Anthropic, OpenAI, OpenRouter, Groq or Gemini, using your own API key, or OpenAI, if you sign in with your ChatGPT plan instead (a preview, no key). The model never receives the audio. When you open Settings, the app also asks that provider for its list of models, with your key or sign-in and no prompt or audio.

With Ollama there's no key, and the chat stays on your machine too.

Two honest caveats about that last part:

- Local models have small context windows. In v0.4.0 one measured first request was about 15,000 tokens, and a model with an 8,192-token context refused it. Since v0.5.0 Ollama gets a core set of tools plus the ones your message names, about 8.7 KB of request against about 67 KB with every tool in the test suite (bytes, not tokens), but I haven't re-run that model on it.
- The recordings were made with Claude as the agent. I haven't confirmed which local models can finish a job like these.

The desktop app sends no telemetry. API keys live in your operating system's credential store. On Linux, v0.4.0 and earlier kept them in the kernel keyring only until you reboot; since v0.5.0 they go in the Secret Service (GNOME Keyring, KWallet or KeePassXC), with a warning when none is running. I haven't checked that across a real reboot yet.

The numbers are in the repo's issues and tests. If you run local models, I'd like your results.

#LocalFirst #Privacy #Ollama #OpenSource #AudioEngineering
```

**Pin this comment:**

```
What stays on your machine, what leaves, and how to run it with Ollama: https://edytlab.com/use-cases/local-ai-audio-editor
```

**Image:** `linkedin/out/post-4-privacy.png` (1200x1200), same use.

---

## 5. Open source: help wanted (attach `reel-teaser.mp4`)

Angle: specific, honest invitations, including the big unshipped piece. About
300 words.

```
edytlab is MIT-licensed and I'd like help with it. Here is where, specifically.

It's a desktop audio editor in Tauri 2, Rust and React, where an AI agent calls 94 audio tools. v0.9.2 is out for macOS, Windows and Linux (unsigned installers).

Places a contributor could make a real difference:

- Effects on a chain. Since v0.6.0 the agent can put an effect on the master bus and on a bus (pull requests 516 and 551), but only five kinds run in a chain: gain, the limiter and three filters. Still missing: streaming versions of the other eleven (reverb, echo, EQ, compressor and others), which today can only be baked into a track. A stored effect the renderer can't run is left out of the render with a warning; issue 520, now closed, has the details, in the audio-engine and tools crates.
- macOS microphone access. Since v0.5.0 the build has the entitlement and the usage text, but nobody has recorded on a signed, notarized macOS build to confirm it. That check is issue 464, and it needs a Mac.
- A real pass on Linux with a locked keyring. The Secret Service code is tested against fakes only. Issue 497.
- New audio tools and new LLM providers. Each is one registered implementation, and docs/contributing.md walks through both.
- The big one: stem separation is not shipped. The Demucs tool is wired in and the installers carry the ONNX Runtime, but nothing runs a stem model (issue 385). Transcription does run, from v0.8.0, on a Whisper model you download once; I've checked it on one short synthetic English recording and about 16 minutes of English audiobook speech (4% to 7% of the words wrong). If you've shipped model inference inside a Tauri app, I'd like to talk.

The repo has a "good first issue" label, but nothing open carries it yet. If you want a starter task, reply here and I'll write one up.

Bug reports with your OS, version and provider are welcome too.

#OpenSource #Rust #Tauri #Contributing
```

**Pin this comment:**

```
Contributing guide: https://github.com/laadtushar/edytlab/blob/main/docs/contributing.md · open issues: https://github.com/laadtushar/edytlab/issues
```

**Image:** `linkedin/out/post-5-help-wanted.png` (1200x1200), same use.

---

## Notes (not part of the posts)

Sources for the claims:

- Demo numbers and the flagged edge cases: captions in
  `website/lib/demos.ts`.
- "Synthetic loops I generated": `apps/desktop/e2e-native/fixtures.mjs` writes
  the three tracks (32 seconds each) from code. The DJ page and README say
  "tracks", so this is a disclosure the site does not currently make.
- 94 tools: `docs/tools-reference.md`. Providers and Ollama:
  `README.md`. Local-first table: `website/app/use-cases/local-ai-audio-editor/page.tsx`.
  The 15,000-token / 8,192-context numbers are v0.4.0's, from issue 395; what
  has been sent since v0.5.0 is in `docs/architecture.md` ("Request size and
  small-context models", whose table is v0.6.0's) and
  `crates/ai/tests/request_size.rs`; the 8.7 KB and 67 KB here are v0.7.0's,
  measured on 2026-10-22 with `cargo test -p ai --test request_size --
  --nocapture`.
- Undo story: v0.4.0 changelog, `apps/desktop/src/lib/headTrail.ts`.
- Zero-dependency effects crate and Rubber Band decision:
  `crates/audio-dsp/Cargo.toml`, `crates/audio-time/src/vocoder.rs`.
- Master bus: issue 460, closed by PR 516, which v0.6.0 is the first release to ship (v0.5.0 and earlier render a baked "Master" track instead); the recordings were made after it and put the limiter on the master bus, as the mini-mix caption in `demos.ts` says.
- Linux keyring: issue 394. v0.4.0 and earlier use the kernel keyring; v0.5.0 and later use the Secret Service (`crates/ai/src/keychain.rs`, `docs/architecture.md`).

Decisions for you:

- Post 3 says "I build it with Claude Code". It is true (commit trailers,
  `CLAUDE.md`) and `marketing/README.md` recommends disclosing once. Delete
  the line if you prefer, but decide before someone asks.
- Post 5 promises to write up a starter task on request, and says no open
  issue carries the `good first issue` label (true when checked on 2026-10-22:
  15 open issues, none labelled). If you label two issues first, replace that paragraph with
  their numbers.
- Post 1 says the agent in the recordings is Claude. That is what the demo
  captions say. No model version is named anywhere.
- A test script types the requests into the real app when the demos are
  recorded (`apps/desktop/e2e-native/stories/demos.mjs`); the replies and
  edits are the live agent's, and only runs that passed the script's checks
  become videos. The posts do not say "I typed", for that reason. Say it
  openly if asked.
