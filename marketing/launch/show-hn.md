# Show HN

**Link to submit:** <https://edytlab.com>
**Download link for the comment:** <https://github.com/laadtushar/edytlab/releases/latest>
**Source link for the comment:** <https://github.com/laadtushar/edytlab>

HN rules to keep in mind: the title carries the whole pitch, there is no
marketing language, the thing must be usable without signing up (it is: a
download, plus a key you already have or Ollama), and nobody is asked to
upvote.

## Title (80 characters or fewer)

Pick one. Counts include the `Show HN: ` prefix.

1. `Show HN: edytlab – edit audio by chatting with an agent, DSP runs locally` (73)
2. `Show HN: edytlab – an open-source audio editor you drive in plain English` (73)
3. `Show HN: edytlab – desktop audio editor where an LLM calls 94 DSP tools` (71)

Recommended: **3**. It names the architecture (an LLM calling deterministic
tools), which is the part HN will want to argue about.

## First comment

Post this as a comment on your own submission as soon as it is up.

---

Hi HN. edytlab is a desktop audio editor (macOS, Windows, Linux, MIT) where you
edit by describing what you want. Load some audio and type something like
"time-stretch the second track to match the first one's tempo, start it 8 bars
before the first one ends, crossfade, limit at -1 dB, bring it to -14 LUFS and
export a WAV". An LLM agent turns that into calls to 94 audio tools. The tools
do the signal processing; the model only chooses and sequences them. It never
sees samples.

There is a screen recording of that exact request on the site (2:45, with
sound): https://edytlab.com/#demos. In it the agent also points out a
spot where the bars land a beat apart, which I left in on purpose.

**Why local-first.** Decode, DSP, mixing and export all run in-process in Rust.
Your audio is never uploaded. The only things that leave the machine are the
chat (your prompts, the agent's context and tool results) going to whichever
provider you pick: Anthropic, OpenAI, OpenRouter, Groq, Gemini, or Ollama; and
the request Settings makes to that provider for its model list, which carries
your key and no prompt or audio. With
Ollama there is no key and the chat stays on your machine too. The app has no
telemetry. The website, which is separate, uses Vercel's anonymous analytics
(page views and page performance, no cookies); the privacy page says so.

**How it is built.**

- Tauri 2 shell, React front end, Rust workspace behind it.
- The effects live in an in-house `audio-dsp` crate with zero dependencies.
  Decoding is `symphonia`, resampling `rubato`, FFT `realfft`.
- Time-stretch and pitch-shift run on a phase vocoder written for this project.
  I wanted Rubber Band and dropped it, because it needs a different native
  package on each of the three CI targets and a native dependency breaks every
  build at once. The trade is documented at the top of
  `crates/audio-time/src/vocoder.rs`. It is cleaner on sustained material than
  on dense mixes, and the further the factor is from 1.0 the more it smears.
- Every state is a node in a content-addressed graph. You can fork, name,
  compare (A/B) and revert, instead of living with a linear undo stack.
- There is an optional "plan first" mode where the agent shows its steps and
  you approve before anything runs.
- Export is WAV, FLAC or MP3.

**What does not work yet.** I would rather say it here than have you find it:

- No stem separation. The tool is registered and returns an error today; the
  installers carry the ONNX Runtime, but no stem model exists in the app and the
  inference is not built (#385). Nothing in the app or the docs should suggest
  otherwise; if you find a place that does, that is a bug.
- Transcription works from v0.8.0 (#383, #384), with limits. It needs a
  one-time download of the Whisper base model (about 139 MB) that you start in
  Settings; nothing downloads by itself. It runs on your CPU. I have measured it
  on one short synthetic English recording (twelve words, none wrong, every
  start within 0.23 s), not on real voices, other languages or noisy audio, so
  read the words before you cut on them. The audio stays on your computer; the
  words reach your AI provider as a tool result (bounded to 6,000 bytes a call),
  and stay local with Ollama.
- Builds are unsigned. macOS needs one `xattr` command after you drag the app
  to Applications and Windows shows a SmartScreen warning. The release notes
  have the steps.
- It is an offline editor. It is not a live DJ tool or a DAW: no MIDI, no VST
  hosting.
- Rough edges I know about: on Linux, since v0.5.0 your API key goes in the
  Secret Service, with a warning when there is none (#394; v0.4.0 and earlier
  kept it only until reboot), but I have not yet checked it across a real
  reboot or against a locked keyring (#497); only five effects (gain, the
  limiter and three filters) can sit live on a track, a bus or the master bus,
  so reverb, EQ or a compressor is baked into the track's audio instead; the
  limiter has instant attack and no look-ahead, and is not a true-peak one;
  with a local model, v0.4.0's first request was about 15k tokens in my one
  measurement (#395), and since v0.5.0 Ollama gets a smaller tool set, but I
  have not run a local model through a full job. If you tried an earlier
  build: v0.5.0 and older could delete your own recordings when the assistant
  compacted history (#589), and v0.6.0 could lose a Save As copy's audio once
  the original was deleted (#612), so please use v0.6.1 or later.

**Transparency.** This project is developed with Claude Code as a collaborator.
Many commits carry a `Co-Authored-By: Claude` trailer, and the repo has a
`CLAUDE.md` with the working rules.

**What I would like feedback on.**

1. If you edit audio for a living or for fun: which edit would you ask for that
   is not in the tool list (`docs/tools-reference.md`)?
2. If you run local models: which ones can drive a tool list this size without
   losing the thread?
3. If you do DSP: the vocoder and the limiter are the two places I expect the
   most justified criticism.

Download (unsigned v0.7.0): https://github.com/laadtushar/edytlab/releases/latest
Source: https://github.com/laadtushar/edytlab

---

## Questions to have an answer ready for

| Likely question | Answer, with the evidence |
|---|---|
| "Why not just script ffmpeg/sox?" | You can, and for a fixed pipeline you should. This is for edits you would otherwise do by ear and by hand, with an audible A/B at each step and a graph of what you tried. |
| "Does the LLM hear the audio?" | No. It gets the session state and tool results such as BPM, key, beat grid and loudness from `analyze_track`, and chooses edits from that. |
| "What does it cost?" | The app is free (MIT). Hosted providers charge you per token under your own key; Ollama is free. The docs give a rough range, but do not quote it here as a promise. |
| "Is the audio really never uploaded?" | Yes: there is no server in the loop. The privacy page lists the network traffic: chat requests to your chosen provider, plus the request Settings makes to that provider for its model list (it carries your key, no prompt or audio). You can check it in your OS network tools. |
| "Why Tauri and not Electron?" | The back end is Rust, so the UI process and the DSP crates share one workspace. Do not claim size or speed numbers against Electron; none were measured. |
| "Is there a web version?" | No. A browser SaaS is evaluated in #389 and undecided. |
| "94 tools is a lot for a model" | It is. For Ollama, the app sends a core set of about 15 tools plus the ones your message names, and lists the rest by name (#395, #479; since v0.5.0). Hosted providers still get all of them: Anthropic's prompt cache needs a fixed list. The Capabilities menu and agent profiles can narrow the list, and plan-first keeps a human in the loop. |
