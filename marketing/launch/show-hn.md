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
3. `Show HN: edytlab – desktop audio editor where an LLM calls 93 DSP tools` (71)

Recommended: **3**. It names the architecture (an LLM calling deterministic
tools), which is the part HN will want to argue about.

## First comment

Post this as a comment on your own submission as soon as it is up.

---

Hi HN. edytlab is a desktop audio editor (macOS, Windows, Linux, MIT) where you
edit by describing what you want. Load some audio and type something like
"time-stretch the second track to match the first one's tempo, start it 8 bars
before the first one ends, crossfade, limit at -1 dB, bring it to -14 LUFS and
export a WAV". An LLM agent turns that into calls to 93 audio tools. The tools
do the signal processing; the model only chooses and sequences them. It never
sees samples.

There is a screen recording of that exact request on the site (3:08, with
sound): https://edytlab.com/#demos. In it the agent also points out a
spot where the bars land a beat apart, which I left in on purpose.

**Why local-first.** Decode, DSP, mixing and export all run in-process in Rust.
Your audio is never uploaded. The only thing that leaves the machine is the
chat (your prompts, the agent's context and tool results) going to whichever
provider you pick: Anthropic, OpenAI, OpenRouter, Groq, Gemini, or Ollama. With
Ollama there is no key and the chat stays on your machine too. The app has no
telemetry and the website has no analytics.

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

- No stem separation and no transcription. Both are registered as tools and both
  return an error today; the ONNX inference and model delivery are not built
  (#383, #384, #385). That also means transcript-based editing (cut words,
  remove fillers) does not work end to end. Nothing in the app or the docs
  should suggest otherwise; if you find a place that does, that is a bug.
- Builds are unsigned. macOS needs one `xattr` command after you drag the app
  to Applications and Windows shows a SmartScreen warning. The release notes
  have the steps.
- It is an offline editor. It is not a live DJ tool or a DAW: no MIDI, no VST
  hosting.
- Rough edges I know about: on Linux, v0.4.0 and earlier keep your API key
  only until reboot (#394; the next release uses the Secret Service and warns
  when there is none); the limiter has instant attack and no look-ahead, and is not a
  true-peak one; with a local model the first request is large (about 15k
  tokens in my one measurement, #395), so you need a generous context window.

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

Download (unsigned v0.3.0): https://github.com/laadtushar/edytlab/releases/latest
Source: https://github.com/laadtushar/edytlab

---

## Questions to have an answer ready for

| Likely question | Answer, with the evidence |
|---|---|
| "Why not just script ffmpeg/sox?" | You can, and for a fixed pipeline you should. This is for edits you would otherwise do by ear and by hand, with an audible A/B at each step and a graph of what you tried. |
| "Does the LLM hear the audio?" | No. It gets the session state and tool results such as BPM, key, beat grid and loudness from `analyze_track`, and chooses edits from that. |
| "What does it cost?" | The app is free (MIT). Hosted providers charge you per token under your own key; Ollama is free. The docs give a rough range, but do not quote it here as a promise. |
| "Is the audio really never uploaded?" | Yes: there is no server in the loop. The privacy page lists the network traffic: chat requests to your chosen provider and nothing else. You can check it in your OS network tools. |
| "Why Tauri and not Electron?" | The back end is Rust, so the UI process and the DSP crates share one workspace. Do not claim size or speed numbers against Electron; none were measured. |
| "Is there a web version?" | No. A browser SaaS is evaluated in #389 and undecided. |
| "93 tools is a lot for a model" | It is. That is #395. The Capabilities menu and agent profiles can narrow the tool list, and plan-first keeps a human in the loop. |
