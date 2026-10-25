# Product Hunt

**Website field:** <https://edytlab.com>
**Download link (use in the comment and in "Get it"):** <https://github.com/laadtushar/edytlab/releases/latest>
**Source link:** <https://github.com/laadtushar/edytlab>

Product Hunt's form fields and topic names change; the limits below are the
ones that were asked for (tagline 60, description 260). Check the live form
before you paste.

## Name

edytlab

## Tagline (60 characters or fewer)

```
Edit audio by chatting with an AI agent
```

Alternates:

- `Describe an audio edit. An agent does it, locally.`
- `Open-source audio editor you control by chatting`

## Description (260 characters or fewer)

```
A free, open-source desktop audio editor. Describe an edit in chat and an AI agent runs 94 audio tools. The DSP runs locally in Rust, so audio is never uploaded. Bring a key (Anthropic, OpenAI, OpenRouter, Groq, Gemini), a ChatGPT plan (preview) or Ollama.
```

## Maker's first comment

```
Hi Product Hunt. I'm the maker of edytlab.

edytlab is a desktop audio editor (macOS, Windows, Linux, MIT) where you edit by describing what you want. "Time-stretch track 2 to match track 1's tempo, crossfade the last 8 bars, limit at -1 dB, bring it to -14 LUFS, export a WAV." An AI agent turns that into calls to 94 audio tools. The tools do the signal processing; the model only picks and orders them, and never touches the samples.

What I care about:

- Your audio stays on your machine. Decode, DSP, mixing and export run locally in Rust. Only the chat goes to the AI provider you choose. With Ollama there's no key and nothing leaves your computer.
- You stay in control. An optional "plan first" mode shows the agent's steps for your approval, and every edit is a node in a branchable graph, so you can fork, A/B compare and go back.
- It's not locked to one model: Anthropic, OpenAI, OpenRouter, Groq, Gemini, Ollama or, in a preview, your ChatGPT plan, switchable from Settings.

The clearest example is the demo on the site: a DJ asks for two tracks' tempos, the incoming track is time-stretched to match, crossfaded in 8 bars before the first one ends, and the mix is exported as a WAV at -14 LUFS. (It's a 2:45 recording with sound; the waits for the agent are sped up. The agent also flags a spot where the bars land a beat apart.)

What it doesn't do yet, so you don't have to find out: no stem separation (the tool is registered but not implemented). Transcription, and so editing by text, works from v0.8.0 once you download the speech model (about 139 MB) in Settings; it runs on your computer, and the words, not the audio, go to your AI provider. The builds are unsigned, so macOS and Windows will warn on first launch; the release notes have the two-line fix. It's an offline editor, not a live DJ tool.

Download: https://github.com/laadtushar/edytlab/releases/latest
Demo and docs: https://edytlab.com

I'd love to know which edit you'd ask for first.
```

## Gallery (5 images or clips)

The repo has no screenshots made for this listing. The demo poster is the one
still made for the site, and `docs/brand/` has before and after screenshots of
the app's colours (#515, taken with the IPC faked, so they show fake data):
the `before-*` set is the look v0.5.0 and earlier shipped, and the `after-*`
set is the violet look the app has had since v0.6.0. Capture the other four
from the running v0.9.0 app at 1280x776 (the same frame the demo uses) and keep the
filenames out of the repo unless you want them in `website/public`.

| # | Show | Caption |
|---|---|---|
| 1 | The demo video, or its poster `website/public/demos/dj-beatmatched-transition.jpg` (<https://edytlab.com/demos/dj-beatmatched-transition.mp4>) | Ask in plain English; the agent beatmatches, crossfades, masters and exports a WAV. |
| 2 | Chat panel after an edit, with tool badges visible beside the waveform | Every edit is a real tool call you can see, not a black box. |
| 3 | The plan card with Plan first switched on | Plan first: review the agent's steps and approve them before anything runs. |
| 4 | The history Graph view with a forked branch | Fork a take, A/B it against the original, go back whenever you like. |
| 5 | Settings with the provider list open, Ollama selected | Six API providers plus your ChatGPT plan (a preview), including Ollama for a model that stays on your machine. |

## Topics

Choose from what Product Hunt offers; these are the closest matches:

- Audio
- Music
- Open Source
- Artificial Intelligence
- Developer Tools (only if "Desktop Apps" or a similar topic is not available)

## Launch-day notes

- Schedule for 00:01 PT; you cannot ask for upvotes in the post, comments or
  DMs, and neither does this kit.
- Be present for the first several hours. Answer every comment with specifics.
- Pin the unsigned-build note in your first reply to anyone who reports an
  install warning.
- Do not put a number of users, downloads or stars anywhere in the listing.
