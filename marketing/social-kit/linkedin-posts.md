# LinkedIn: five posts

Five posts with different angles, written in your voice. Post one a few days
apart, not back to back (see `calendar.md`). Each has a video to attach as
native video and a one-line comment to pin right after you publish.

**Videos:** `reel-highlights.mp4`, `reel-beatmatched-transition.mp4`,
`reel-mini-mix.mp4`, `reel-extended-club-intro.mp4`, `reel-teaser.mp4`. If
LinkedIn's uploader offers a captions option, upload the matching `.srt`
(for example `reel-highlights.srt`); otherwise use the `-narrated` version.

**Links:**
site <https://edytlab.com> ·
source <https://github.com/laadtushar/edytlab> ·
v0.4.0 <https://github.com/laadtushar/edytlab/releases/tag/v0.4.0>

**Conventions used in every post:** no emoji; 3 to 5 hashtags at the end;
only post 1 has a link in the body, the others carry the link in the pinned
comment, because outbound links in the body are commonly reported to reduce
reach (not verified here). Wherever a download is mentioned, the installers
are described as unsigned.

---

## 1. Launch: what it is (attach `reel-highlights.mp4`)

Angle: the one-paragraph product, the three demos in one breath, the honest
limits. About 230 words.

```
I built an audio editor you talk to. The video is three DJ jobs, each described in plain English and ending in an exported WAV.

A 128 BPM track time-stretched to match a 120 BPM one, started under the last 8 bars of the first, crossfaded with a low-pass on the outgoing track, mastered to -14 LUFS. An extended club intro, made by copying a track's drums-only first 8 bars. A three-track mini-mix at 124 BPM with 4-bar overlaps.

What I like best: the agent flags its own edge cases. A downbeat half a second into a file. A last entry half a bar off the beat grid. An export peaking at 0 dBFS with no headroom.

How it works: the model never hears the audio. It gets text about the session (tempo, key, loudness) and picks from 93 audio tools. The DSP runs on your machine, in Rust. Your files aren't uploaded; only the chat goes to the provider you choose (Anthropic, OpenAI, OpenRouter, Groq, Gemini), or stays local with Ollama.

Honest limits: the demo tracks are short synthetic loops I generated, and the agent in the recordings is Claude. It preps files; it isn't a live DJ tool. No stem separation yet. The installers are unsigned, so macOS and Windows warn on first launch.

Free, open source (MIT), v0.4.0 for macOS, Windows and Linux. DJs: which prep job would you hand off first?

Demos and downloads: https://edytlab.com

#OpenSource #DJing #AudioEngineering #LocalFirst #AIAgents
```

**Pin this comment:**

```
Source (MIT) and installers for macOS, Windows and Linux: https://github.com/laadtushar/edytlab/releases/tag/v0.4.0 (unsigned, so expect a first-launch warning).
```

**Shorter version (under 90 words), same video:**

```
I built an audio editor you talk to. The video: three DJ jobs, each described in plain English and ending in an exported WAV. A beatmatched blend, an extended club intro, a three-track mini-mix. The agent even flags its own edge cases, like a downbeat half a second into a file.

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

- It reports what it's working with: both tracks in F major, at 120 and 128 BPM.
- It time-stretches the 128 BPM track to 120, keeping its pitch, and starts it at 16 seconds, under the last 8 bars of the other.
- The outgoing track fades under a 2 kHz low-pass while the incoming one fades in.
- The mix is mastered for streaming and exported as a 50-second WAV at -14 LUFS, peaking at -1 dBFS.

The moment I trust it most: it tells the DJ the incoming track's first downbeat sits half a second into the file, and offers to nudge it, instead of quietly leaving the blend a little off.

Limits, so nobody has to find them: the tracks are short synthetic loops I generated, so this shows the workflow, not how it copes with a real record. Beat detection is an estimate and can be wrong on a drifting tempo. This is prep for files, not a live tool.

Which step of your own prep would you hand off first?

#DJing #DJTips #AudioEngineering #OpenSource
```

**Pin this comment:**

```
The DJ walkthrough, with all three recordings and the full list of what it can't do: https://edytlab.com/use-cases/dj
```

---

## 3. The build story (attach `reel-mini-mix.mp4`)

Angle: the design decision everything hangs off (the model never touches a
sample), then three engineering choices, one of them a bug story. About 250
words.

```
The audio editor I've been building has an AI agent in it, and the model never touches a sample. Everything else follows from that.

The agent chooses and orders 93 audio tools: fades, EQ, filters, time-stretch, loudness, export. The tools are deterministic Rust. The video shows it sequencing a three-track mix with them.

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

---

## 4. Privacy and local-first, with the Ollama option (attach `reel-extended-club-intro.mp4`)

Angle: answer the first question people ask about AI audio tools, precisely,
including the part that does not work well yet. About 250 words.

```
"Does my audio get uploaded?" is the first question about any AI audio tool, so here is the exact answer for edytlab.

Your audio files: no. Decoding, editing, mixing and export all run on your computer, in a Rust engine. The extended intro in the video was built that way.

What does leave: the chat. Your messages, plus the text the agent works from (a track's tempo, key and loudness, and the results of its tool calls), go to the AI provider you picked: Anthropic, OpenAI, OpenRouter, Groq or Gemini, using your own API key. The model never receives the audio.

With Ollama there's no key, and the chat stays on your machine too.

Two honest caveats about that last part:

- Every request carries the full tool list. One measured first request was about 15,000 tokens, and a local model with an 8,192-token context refused it. You need solid tool calling and a large context.
- The recordings were made with Claude as the agent. I haven't confirmed which local models can finish a job like these.

The desktop app sends no telemetry. API keys live in your operating system's credential store; on Linux that is the kernel keyring, which does not survive a reboot yet.

Both caveats are open issues in the repo, with numbers. If you run local models, I'd like your results.

#LocalFirst #Privacy #Ollama #OpenSource #AudioEngineering
```

**Pin this comment:**

```
What stays on your machine, what leaves, and how to run it with Ollama: https://edytlab.com/use-cases/local-ai-audio-editor
```

---

## 5. Open source: help wanted (attach `reel-teaser.mp4`)

Angle: specific, honest invitations, including the big unshipped piece. About
250 words.

```
edytlab is MIT-licensed and I'd like help with it. Here is where, specifically.

It's a desktop audio editor in Tauri 2, Rust and React, where an AI agent calls 93 deterministic audio tools. v0.4.0 is out for macOS, Windows and Linux (unsigned installers).

Places a contributor could make a real difference:

- Master bus effects. The session model and the renderer already support a master effect chain, but no tool targets it. So the agent works around it by rendering a baked "Master" track and muting the originals, which the beatmatch and mini-mix recordings both show. Issue 460 has the fix outlined, in the tools crate.
- macOS microphone access. Issue 464 is small, but it needs checking on a hardened macOS build.
- Follow-ups to Plan first: issue 438.
- New audio tools and new LLM providers. Each is one registered implementation, and docs/contributing.md walks through both.
- The big one: stem separation and transcription are not shipped. The Demucs and Whisper tools are wired in, but their inference is not (issues 383 to 385). If you've shipped ONNX Runtime inside a Tauri app, I'd like to talk.

The repo has a "good first issue" label, but nothing open carries it yet. If you want a starter task, reply here and I'll write one up.

Bug reports with your OS, version and provider are welcome too.

#OpenSource #Rust #Tauri #Contributing
```

**Pin this comment:**

```
Contributing guide: https://github.com/laadtushar/edytlab/blob/main/docs/contributing.md · open issues: https://github.com/laadtushar/edytlab/issues
```

---

## Notes (not part of the posts)

Sources for the claims:

- Demo numbers and the three flagged edge cases: captions in
  `website/lib/demos.ts`.
- "Synthetic loops I generated": `apps/desktop/e2e-native/fixtures.mjs` writes
  the three tracks (32 seconds each) from code. The DJ page and README say
  "tracks", so this is a disclosure the site does not currently make.
- 93 tools: `docs/tools-reference.md`. Providers and Ollama:
  `README.md`. Local-first table and the 15,000-token / 8,192-context numbers:
  `website/app/use-cases/local-ai-audio-editor/page.tsx` and issue 395.
- Undo story: v0.4.0 changelog, `apps/desktop/src/lib/headTrail.ts`.
- Zero-dependency effects crate and Rubber Band decision:
  `crates/audio-dsp/Cargo.toml`, `crates/audio-time/src/vocoder.rs`.
- Master-bus workaround: issue 460, and the mini-mix caption in `demos.ts`.
- Linux keyring: issue 394 (open).

Decisions for you:

- Post 3 says "I build it with Claude Code". It is true (commit trailers,
  `CLAUDE.md`) and `marketing/README.md` recommends disclosing once. Delete
  the line if you prefer, but decide before someone asks.
- Post 5 promises to write up a starter task on request, and says no open
  issue carries the `good first issue` label (true when checked: 17 open issues,
  none labelled). If you label two issues first, replace that paragraph with
  their numbers.
- Post 1 says the agent in the recordings is Claude. That is what the demo
  captions say. No model version is named anywhere.
- A test script types the requests into the real app when the demos are
  recorded (`apps/desktop/e2e-native/stories/demos.mjs`); the replies and
  edits are the live agent's, and only runs that passed the script's checks
  become videos. The posts do not say "I typed", for that reason. Say it
  openly if asked.
