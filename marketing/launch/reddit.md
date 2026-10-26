# Reddit posts

Seven posts, one per community. They are written to be different on purpose:
each leads with something that community can use, and mentions the project
second. Do not cross-post one text.

**Links to use in every post:**
Website <https://edytlab.com> ·
Download <https://github.com/laadtushar/edytlab/releases/latest> ·
Source <https://github.com/laadtushar/edytlab>

**Before each post:**

1. Re-read that sub's sidebar and rules on the day. Their treatment of
   self-promotion was not checked from this environment and changes without
   notice. If the rules say promotion goes in a weekly or flaired thread, post
   there.
2. Say you made it, in the first sentence.
3. Put the link where the rules allow (post body or first comment). If links
   are banned in posts, drop it in a comment only when asked.
4. Do not mention stem separation as a feature anywhere. It does not work.
   Transcription works from v0.8.0 after the user downloads the speech model;
   say that, and say what it was checked on (see `marketing/README.md`).
5. Stay for the replies. A drive-by post reads as spam.
6. If the sub asks whether AI helped build the project, say yes. It did: the
   repo is developed with Claude Code.

---

## 1. r/WeAreTheMusicMakers

**Audience:** home producers and songwriters. Wary of "AI music" and of
promotion. edytlab does not generate music, so lead with that.

**Flair:** whichever fits "Software/Tools" or "Feedback"; if the sub confines
self-promotion to a set thread, use that thread.

**Title:**
`I made a free, open-source audio editor where you describe the edit instead of hunting through menus (feedback wanted)`

**Body:**

> Disclosure: I made this. It's free and MIT-licensed, no account, no
> subscription.
>
> It doesn't generate music, and it isn't a DAW. There's no MIDI and no plugin
> hosting. It's for the audio-editing chores around a track: matching two
> loops' tempo, nudging a sample into the key of your song, trimming the dead
> air off a recording, getting a bounce to a sensible loudness, adding a fade.
>
> You load files and type what you want, for example "time-stretch the loop to
> 124 BPM, pitch it down two semitones, add a short reverb, fade the last two
> seconds". An AI agent then runs ordinary audio tools (stretch, pitch-shift,
> reverb, fade) on your files. You can fork a take, A/B it against the original
> and go back. The processing happens on your computer; your audio isn't
> uploaded. The only thing sent out is the chat text, to whichever AI provider
> you choose (or none, if you run Ollama locally).
>
> Honest limits: the time-stretch and pitch-shift are a phase vocoder I wrote,
> so big changes on dense material sound smeared. It can't split a song into
> stems or vocals; that feature isn't built. The installers are unsigned, so
> macOS and Windows warn you on first launch.
>
> What I want to know: when you stop in the middle of a session to do a boring
> audio job, what is it? That list is my roadmap.
>
> Site and demo: https://edytlab.com
> Download: https://github.com/laadtushar/edytlab/releases/latest

---

## 2. r/DJs

**Audience:** bedroom and working DJs. Practical, allergic to hype. Make sure
nobody thinks this runs live.

**Title:**
`Free tool I built for prepping blends: ask for the tempos, it time-stretches the incoming track, crossfades, masters and exports (demo inside)`

**Body:**

> Disclosure: I'm the developer. It's free and open source.
>
> This is for prep, not for playing live. You load two tracks and describe the
> transition. In the recording on the site (2:45, with sound), the
> request is: get both tracks' tempos, time-stretch the incoming track to match,
> start it 8 bars before the outgoing track ends, crossfade, put a low-pass on
> the outgoing track over the overlap, compress and limit the mix, bring it to
> -14 LUFS and export a WAV.
>
> Under the hood it's a tempo/key/beat-grid analysis, a time-stretch that keeps
> pitch, and ordinary mixing tools. Everything is a step in a history graph, so
> you can try the blend with the incoming track starting 4 bars earlier, A/B it
> against the first take and keep whichever you like.
>
> Things it will not do for you: it doesn't split tracks into stems or
> acapellas (not built), it doesn't talk to rekordbox or Serato, and there's no
> live mode. Beat detection is software, so on a track with a drifting tempo or
> a long ambient intro it can be off. The recording shows exactly that: the
> assistant itself points out that at the 16 s start the bars land a beat
> apart, and I left it in.
>
> Demo: https://edytlab.com/#demos
> Download (unsigned, so there's a first-launch warning on Mac and Windows):
> https://github.com/laadtushar/edytlab/releases/latest
>
> If you prep blends, what's the part of the process you'd most like to skip?

---

## 3. r/audioengineering

**Audience:** working engineers; hostile to shortcuts and to promotion. Many
posts here are questions. If self-promotion is not permitted, **do not post
this**; instead, answer relevant threads honestly and mention the project only
if asked. The post below is written as a discussion with disclosure, so it
holds up only if the rules allow it.

**Title:**
`I built an editor where an LLM only chooses and orders deterministic DSP tools and never touches samples. Where would you not trust it?`

**Body:**

> Disclosure: this is my own open-source project (MIT), linked at the bottom.
> I'm after criticism of the approach, not downloads.
>
> The design: the model gets a text description of the session (tracks, levels,
> effects, and measurements like BPM, key, beat grid and EBU R128 loudness) and
> a list of 94 tools (gain, EQ, compressor, limiter, filters, fades, time
> stretch, pitch shift, normalise to a LUFS target, and so on). It picks and
> orders tools. The audio is processed by the tools, in Rust, deterministically,
> so the same tool call on the same file gives the same bytes. (The model's
> choice of calls is not deterministic; the tools' output is.) The model never
> hears or sees samples.
>
> What I know is weak:
>
> - The limiter turns the gain down and releases over about 80 ms rather than
>   clipping, but it has no look-ahead (the gain steps in on the first sample
>   over the ceiling) and it isn't a true-peak limiter. Loudness normalisation
>   to a LUFS target is capped so the true peak stays under -1 dBFS rather than
>   clipping.
> - Time-stretch and pitch-shift are a phase vocoder with onset-triggered
>   phase resets. Fine on sustained material, phasey on dense mixes and at
>   factors far from 1.0.
> - Noise reduction is plain spectral subtraction with a noise profile from the
>   start of the clip.
> - The model chooses parameters from a description, not from listening. That's
>   why there's an optional plan-first mode and an audible A/B at each step.
>
> Questions for you: which of these would make you refuse to use it on paid
> work? Which measurements should it hand the model that it doesn't? What would
> you want logged so an edit is reproducible?
>
> Source: https://github.com/laadtushar/edytlab (tool list in
> docs/tools-reference.md)

---

## 4. r/podcasting

**Audience:** hosts and editors; many have been burned by "AI podcast
editors". **Be explicit about what transcription was checked on**, because
that is what the category promises.

**Title:**
`Free, open-source audio editor for the boring cleanup passes (noise, levelling, silence, loudness), with local transcription you download once`

**Body:**

> Disclosure: I made this. Free, MIT, runs on your computer.
>
> What it's for is the mechanical cleanup around an episode: cut long silences,
> reduce steady background noise, tame harsh "s" sounds, level out loud and
> quiet passages, high-pass the rumble, and bring the export to a loudness
> target (-16 LUFS for Apple Podcasts, for example). You describe the passes in
> a chat box and an assistant runs them as ordinary audio tools, one step at a
> time, with an audible A/B against the previous version so you can back out.
>
> Since v0.8.0 it can also transcribe, on your computer, with the Whisper base
> model (about 139 MB). You press Download once in Settings; nothing downloads
> by itself. Then you can ask for the words with times, cut a passage by its
> words, or have the "um"s found (it reports first and changes nothing until you
> say so). Audio never leaves your computer, but the words go to your AI
> provider as text, unless you use Ollama. What I can't tell you: I have only
> checked it on one short synthetic English recording (twelve words, none wrong)
> and about 16 minutes of English audiobook speech (4% to 7% of the words wrong),
> not on real episodes, other accents or noisy rooms. It doesn't split a
> recording into one track per speaker automatically. There's no cloud, no
> hosting, no publishing.
>
> Your recordings aren't uploaded. The only data that leaves is the chat text,
> to the AI provider you pick (or nothing at all, with a local model).
>
> Noise reduction here is basic spectral subtraction. If a recording is very
> noisy, a specialised denoiser will beat it. I'd like to hear from people who
> have a standard cleanup chain: what's in yours, in what order?
>
> https://edytlab.com
> Download (unsigned, so expect a first-launch warning):
> https://github.com/laadtushar/edytlab/releases/latest

---

## 5. r/LocalLLaMA

**Audience:** people running models locally; care about context size, tool
calling, and quantisation. They want real numbers, not slogans. Do not claim
any model works unless you have tried it.

**Title:**
`Open-source desktop app where a local model (Ollama) edits audio by calling 94 tools. Looking for which tool-calling models can handle it`

**Body:**

> Disclosure: my project, MIT-licensed.
>
> edytlab is a desktop audio editor driven by an LLM agent. The model chooses
> and orders calls to 94 deterministic audio tools (EQ, compression, time
> stretch, fades, loudness, export); the DSP is in Rust and runs on your
> machine. Ollama is one of six API providers (the others are Anthropic, OpenAI,
> OpenRouter, Groq and Gemini), and there is a preview that uses a ChatGPT
> Plus or Pro plan instead of a key. For Ollama there's no key: point it at your
> daemon, pick a model, and the chat stays local too. The Settings "Test" button
> sends a minimal request and checks the model can call tools.
>
> What I've actually measured, which is not much:
>
> - In v0.4.0 a one-line request produced a first request of about 15,000
>   tokens (system prompt plus every tool schema).
> - With a llama.cpp `llama-server` and Qwen2.5-3B at an 8,192 context, that
>   request was refused outright. The numbers are in
>   https://github.com/laadtushar/edytlab/issues/395.
> - On CPU, prompt processing at ~100 tokens/s means minutes to the first reply
>   for a request that size.
>
> What changed in v0.5.0: Ollama is sent a core set of 15 tools plus the ones
> your message names, with shortened descriptions, and the names of the rest go
> in one line of the system prompt so the model knows they exist. Hosted
> providers still get every tool. The test that measures it
> (`crates/ai/tests/request_size.rs`, against a mock server) puts the one-line
> request at about 8.7 KB of text, against about 67 KB with every tool (on
> v0.7.0; it was 66 KB in v0.6.0 and 58 KB in v0.5.0, and the full list grows with the tools). That is
> bytes, not tokens, I have not run a tokenizer, and I have not re-run
> llama-server with Qwen2.5-3B on the new request. If the server still refuses
> a request as too long, the app now says so and what to change (llama.cpp does
> refuse; Ollama trims silently, and the app shows nothing).
>
> I haven't benchmarked models against the tool list, and I'd rather ask than
> guess. If you try it: which model and quantisation, what context setting, and
> did it keep to the tool schemas? A tool is sent in full only when the message
> names it or it is in the core, so a request described by effect ("make it
> sound like a cathedral") may not carry the reverb schema; I don't know how
> often that matters. Reports of failures are as useful as successes.
>
> Optional "plan first" mode makes the agent show its steps for your approval
> before any edit runs, which helps with smaller models that make odd choices.
>
> Source: https://github.com/laadtushar/edytlab
> Download (unsigned): https://github.com/laadtushar/edytlab/releases/latest
> Site: https://edytlab.com

---

## 6. r/rust

**Audience:** Rust programmers. Value-first means the engineering. Check
whether the sub has a project-showcase thread or specific rules on AI-assisted
code, and follow them. Answer the "was this AI-generated" question with the
truth.

**Title:**
`edytlab: a Tauri 2 audio editor with a dependency-free DSP crate and a hand-written phase vocoder on realfft`

**Body:**

> Disclosure: my project, MIT. The repo is developed with Claude Code; the
> commit trailers say so.
>
> The Rust side of a desktop audio editor that an LLM agent drives through 94
> tools:
>
> - `audio-dsp` has deliberately zero dependencies: functions over `&mut [f32]`
>   plus a sample rate and channel count. It sits under both the tools and the
>   renderer, so a dependency there would be a dependency everywhere.
> - Decode is `symphonia`, resampling `rubato`, FFT `realfft`, WAV `hound`,
>   FLAC `flac-codec`, MP3 `rusty_mp3`, mic capture `cpal`.
> - Time-stretch and pitch-shift are a phase vocoder written for the project,
>   with peak-based phase locking and onset-triggered phase resets. I wanted
>   Rubber Band and dropped it: it needs `librubberband-dev` on Linux, vcpkg on
>   Windows and Homebrew on macOS, and a native dependency like that breaks all
>   three CI targets at once. The module docs in
>   `crates/audio-time/src/vocoder.rs` write down what the trade costs.
> - Session state is a content-addressed DAG (node id = blake3 of the state),
>   so two routes to the same state are one node. That had a sharp edge for
>   undo (issue #398: a revisited state keeps its first parent, so undo went to
>   the wrong node). v0.4.0 fixes it by making undo and redo follow the path
>   the user took (#401), and I'd still like to hear how you'd model it.
> - Tools are registered in a dispatcher with JSON-schema validation, and
>   `docs/tools-reference.md` is generated from the registry by a test, so the
>   docs can't disagree with what the agent can call.
> - LLM providers sit behind an `LlmProvider` trait; request serialisation and
>   SSE parsing are per provider.
>
> Two things I'd particularly like reviewed: the vocoder's transient handling,
> and the lock discipline in the agent loop (long tools used to hold the store
> lock and freeze the window, #421; since v0.5.0 a tool that opts in runs
> without the lock and commits only if the head hasn't moved, and re-runs
> otherwise).
>
> What is not built: ONNX stem separation is a stub. Transcription runs Whisper base on the same runtime, from a model the user downloads.
>
> Source: https://github.com/laadtushar/edytlab
> Site and demo: https://edytlab.com

---

## 7. r/tauri

**Audience:** Tauri developers. Show what you learned shipping a real app on
all three platforms. Only claim things you hit.

**Title:**
`Shipped a Tauri 2 audio editor on macOS, Windows and Linux. Notes on asset scope, main-thread commands and Linux keyrings`

**Body:**

> Disclosure: it's my open-source project (MIT), edytlab, a desktop audio
> editor driven by an LLM agent. The repo is developed with Claude Code.
>
> A few things from getting it onto three platforms that might save someone
> time:
>
> - **Asset protocol scope.** In v0.2.0 I narrowed the asset scope. The rendered
>   mix and previews lived in a hidden folder, so playback returned 403 on
>   Linux and macOS; Windows wasn't affected. It was invisible until a native
>   run on Linux. Fixed in v0.3.0.
> - **Sync commands run on the main thread.** A few synchronous commands took
>   the same store lock a long-running tool held, so the whole window froze
>   during a long edit (#421). v0.3.0 moved those commands off the main thread
>   (#423), and v0.5.0 moved the rest (#489) and lets long tools run without
>   holding the lock at all (#487). The lesson: don't take a lock a background
>   worker might hold inside a sync command.
> - **Keyring on Linux.** `keyring`'s `linux-native` backend is the kernel
>   keyring: in-memory, gone after a reboot. That is what v0.4.0 and earlier
>   use (#394); since v0.5.0 the app stores credentials in the Secret Service
>   and warns when there is none. I haven't checked that across a real reboot yet. If you ship
>   credentials on Linux, read the backend's docs before you rely on it.
> - **Testing.** Playwright runs the production front end in Chromium with only
>   the IPC boundary replaced, and the fake backend answers from the Rust
>   source. A separate native suite drives the real binary with WebDriver for
>   what jsdom and Chromium cannot see (WebKitGTK layout, for example).
> - **Release.** One matrix produces a universal macOS dmg, Windows msi and
>   NSIS exe, Linux deb and AppImage. They're unsigned, so there's a README
>   section on the macOS quarantine flag.
>
> Happy to go into any of these. If you've solved the signing-on-a-budget
> problem for an open-source Tauri app, I'd like to hear how.
>
> https://github.com/laadtushar/edytlab · https://edytlab.com
