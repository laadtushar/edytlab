# Show HN

**Link to submit:** <https://edytlab.com>
**Download link for the comment:** <https://github.com/laadtushar/edytlab/releases/latest>
**Source link for the comment:** <https://github.com/laadtushar/edytlab>

HN rules to keep in mind: the title carries the whole pitch, there is no
marketing language, the thing must be usable without signing up (it is: a
download, plus a key you already have, a ChatGPT plan or Ollama), and nobody is
asked to upvote.

This is the technical channel, so the first comment may say how it works, what
was hard and what is missing. Keep it in plain sentences, in your own voice
(`../brand-voice.md`).

## Title (80 characters or fewer)

Pick one. Counts include the `Show HN: ` prefix.

1. `Show HN: edytlab – edit audio by chatting with an agent, DSP runs locally` (73)
2. `Show HN: edytlab – an open-source audio editor you drive in plain English` (73)
3. `Show HN: edytlab – desktop audio editor where an LLM calls 99 DSP tools` (71)

Recommended: **3**. It names the architecture (an LLM calling deterministic
tools), which is the part HN will want to argue about.

## First comment

Post this as a comment on your own submission as soon as it is up.

---

Hi HN. Some edits take one sentence to describe and a lot of clicking to do:
"bring the second track in under the last 8 bars of the first, match the tempo,
crossfade, and export a WAV." I built edytlab so I can type that and watch it
happen.

It's a desktop audio editor for macOS, Windows and Linux, MIT licensed. An LLM
reads a summary of your session, plus what its analysis tools report, like
tempo, key and loudness, and calls the editing tools it needs, 99 of them. The tools do the signal processing, in
Rust, on your computer. The model only chooses them and puts them in order. It
never sees a sample.

There's a screen recording of that job, with sound, at
https://edytlab.com/#demos. It's asked for in six short requests, and the waits
are sped up. The agent's replies in it are the model's own words, not a script.

**Where your audio goes.** Nowhere. Decoding, DSP, mixing and export all run
in-process. What does leave your machine is the chat: your prompts, the session
summary and the tool results go to the provider you pick, which can be
Anthropic, OpenAI, OpenRouter, Groq, Gemini or Ollama. There's also a preview
that uses your ChatGPT plan through Sign in with ChatGPT, so no key is needed.
Settings asks your provider for its model list too, with your key and no
prompt. With Ollama there's no key, and the chat stays on your computer as well.
The app has no telemetry. The website is separate and uses Vercel's anonymous
analytics, which the privacy page describes.

**How it's built.** A Tauri 2 shell and a React front end sit on a Rust
workspace. The effects live in an in-house `audio-dsp` crate with no
dependencies, with `symphonia` for decoding, `rubato` for resampling and
`realfft` for the FFT.

The trade-off to know about is time-stretch and pitch-shift. I wanted Rubber
Band, then dropped it: it needs a different native package on each of the three CI
targets, and one native dependency can break every build at once. So I wrote a
phase vocoder instead, and the trade is written up at the top of
`crates/audio-time/src/vocoder.rs`. It's cleaner on sustained material than on
dense mixes, and the further the factor is from 1.0, the more it smears.

Every state of your session is a node in a content-addressed graph, so instead
of one undo stack you can fork, name, compare two versions by ear and go back.
There's also an optional "plan first" mode, where the agent shows its steps and
you approve them before anything runs. Export is WAV, FLAC or MP3.

**What's missing.**

- Stem separation doesn't work yet. The tool is registered and returns an
  error. The installers carry the ONNX Runtime, but there's no stem model in the
  app and the inference isn't built. If you find a place in the app or the docs
  that suggests otherwise, that's a bug.
- Transcription works from v0.8.0, after a one-time download of the Whisper
  base model, about 139 MB, which you start yourself in Settings. Nothing
  downloads on its own. It runs on your CPU. I've only checked it on English:
  a short synthetic recording, and about 16 minutes of audiobook speech where 4%
  to 7% of the words came out wrong. So read the words before you cut on them.
  The audio stays on your computer. The words reach your AI provider as a tool
  result, and stay local with Ollama.
- The installers are unsigned. macOS needs one `xattr` command after you drag
  the app to Applications, and Windows shows a SmartScreen warning. The release
  notes have the steps.
- It's an offline editor. It isn't a live DJ tool or a DAW: no MIDI, no VST
  hosting.
- Only five effects can sit live on a track, a bus or the master bus: gain, the
  limiter and three filters. Reverb, EQ or a compressor gets baked into the
  track's audio instead. The limiter has an instant attack and no look-ahead,
  and it isn't a true-peak limiter.
- I haven't run a local model through a full job yet. Since v0.5.0, Ollama gets
  a smaller set of tools to keep the request small.
- On Linux your API key goes in the Secret Service, with a warning when there
  isn't one. I haven't yet checked that across a real reboot or with a locked
  keyring.
- If you tried an older build: two bugs there could lose audio, one in v0.5.0
  and earlier and one in v0.6.0. Both are fixed in v0.6.1, so please use that or
  later.

**Transparency.** I develop this with Claude Code as a collaborator. Many
commits carry a `Co-Authored-By: Claude` trailer, and the repo has a
`CLAUDE.md` with the working rules.

**What I'd like to hear.**

1. If you edit audio for a living or for fun: which edit would you ask for that
   isn't in the tool list (`docs/tools-reference.md`)?
2. If you run local models: which ones can drive a tool list this size without
   losing the thread?
3. If you do DSP: the vocoder and the limiter are the two places I expect the
   most justified criticism.

Download (unsigned v0.9.3): https://github.com/laadtushar/edytlab/releases/latest
Source: https://github.com/laadtushar/edytlab

---

## Questions to have an answer ready for

| Likely question | Answer, with the evidence |
|---|---|
| "Why not just script ffmpeg/sox?" | You can, and for a fixed pipeline you should. This is for the edits you'd otherwise do by ear and by hand, where you want to hear an A/B at each step and keep a graph of what you tried. |
| "Does the LLM hear the audio?" | No. It gets the session state and tool results such as BPM, key, beat grid and loudness from `analyze_track`, and chooses its edits from those. |
| "What does it cost?" | The app is free and MIT licensed. Hosted providers charge you per token under your own key, Ollama is free, and with a ChatGPT Plus or Pro plan (a preview, v0.9.0 and later) the usage comes out of the plan. I can't promise a number, because it depends on the model and the job. |
| "Is the audio really never uploaded?" | Yes. There's no server in the loop. The privacy page lists the network traffic: chat requests to your chosen provider, plus the request Settings makes for its model list, which carries your key or sign-in and no prompt or audio. You can watch it yourself in your OS network tools. |
| "Why Tauri and not Electron?" | The back end is Rust, so the UI process and the DSP crates share one workspace. I haven't measured size or speed against Electron, so I won't claim any. |
| "Is there a web version?" | No. I've looked at a browser version (#389) and haven't decided. |
| "99 tools is a lot for a model" | It is. For Ollama the app sends a core set of about 15 tools plus the ones your message names, and lists the rest by name (#395, #479; since v0.5.0). Hosted providers still get all of them, because Anthropic's prompt cache needs a fixed list. The Capabilities menu and agent profiles can narrow the list, and plan first keeps a person in the loop. |
