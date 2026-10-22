# LinkedIn: v0.7.0 and the DJ demo

**Links:** site <https://edytlab.com> · download
<https://github.com/laadtushar/edytlab/releases/latest>

Upload `website/public/demos/dj-beatmatched-transition.mp4` as native video.
LinkedIn de-prioritises external links in the body, so the links go in the first
comment, which you post right after publishing.

## Post

```
I've released edytlab 0.7.0, a free, open-source desktop audio editor you drive by describing what you want.

The demo is a DJ transition. The request: get both tracks' tempos, time-stretch the incoming track to match, start it 8 bars before the outgoing one ends, crossfade, put a low-pass on the outgoing track, compress and limit the mix, bring it to -14 LUFS, export a WAV. An AI agent turns that into calls to audio tools, step by step. (The recording is 2:45, with sound; the waits for the agent are sped up. In it the agent also points out a spot where the bars land a beat apart.)

Three design choices I'd like feedback on:

1. The model never touches the audio. It chooses and orders 94 tools; the DSP runs locally in Rust, with no model in the audio path. Your audio is not uploaded. Only the chat goes to the AI provider you pick: Anthropic, OpenAI, OpenRouter, Groq, Gemini, or Ollama, which keeps the chat on your machine too.

2. Every edit is a node in a branching history. You can fork a take, A/B it against the original, and go back. An optional "plan first" mode shows the agent's steps for your approval before it runs them.

3. The time-stretch and pitch-shift are a phase vocoder written for the project, in pure Rust, rather than a bundled C++ library. That keeps the build simple on macOS, Windows and Linux. The cost is that dense material can sound smeared at large changes.

What it can't do yet: stem separation and transcription are not built, so there's no editing by text. The builds are unsigned, so macOS and Windows warn on first launch. It's an editor for prepared files, not a live performance tool.

If you work with audio, which edit would you ask for first?

#opensource #audio #rust #localfirst #AI
```

## First comment

```
Download (macOS universal, Windows, Linux; the installers are unsigned, so macOS and Windows warn on first launch): https://github.com/laadtushar/edytlab/releases/latest
Demo, docs and the DJ walkthrough: https://edytlab.com
Source (MIT): https://github.com/laadtushar/edytlab
```

## Image

`../social-kit/linkedin/out/post-1-launch.png` (1200x1200): the app's chat panel
with a scripted example conversation (the image says so) and the three numbers
from the site. Use it as the video's cover if the uploader
offers a custom thumbnail, or post it on its own if you skip the video. How it
is made, and what each number is read from, is in
[`../social-kit/linkedin/README.md`](../social-kit/linkedin/README.md).

## Optional second post

Same format, built around the extended-intro demo with sound.
`https://edytlab.com/demos/dj-extended-club-intro.mp4`. Describe only what the recording shows; read the
caption in `website/lib/demos.ts` and quote it, rather than writing from
memory.
