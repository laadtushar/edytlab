# X thread: v0.3.0 and the DJ demo

**Links:** site <https://edytlab.com> · download
<https://github.com/laadtushar/edytlab/releases/latest>

Attach the demo to post 1. Upload
`website/public/demos/dj-beatmatched-transition.mp4` (2.4 MB, 3:08) directly
as native video, rather than linking to it. Put links in post 8, not post 1.

Each post is 280 characters or fewer (counted by script). Replace nothing in
brackets until the extended-intro demo (PR #432) is on the site.

---

**1/8**

```
I built a desktop audio editor you drive by chatting.

Here a DJ asks for two tracks' tempos, the incoming one is time-stretched to match, crossfaded in 8 bars before the first ends, and the mix is exported as a WAV at -14 LUFS.

(3:08, with sound. Free, open source.)
```

**2/8**

```
How it works: an AI agent turns your request into calls to 93 audio tools. The tools do the DSP, in Rust, on your machine. The model only picks and orders them. It never touches the samples.
```

**3/8**

```
Your audio is never uploaded. Only the chat goes to the AI provider you choose.

Anthropic, OpenAI, OpenRouter, Groq, Gemini, or Ollama. With Ollama there's no key and the chat stays on your computer too.
```

**4/8**

```
Every edit is a node in a graph. Fork a take, A/B it against the original, go back.

There's also an optional "plan first" mode: the agent shows its steps and you approve before anything runs.
```

**5/8**

```
For the DJ job: it analyses BPM, key, beat grid and loudness, then time-stretches or pitch-shifts with a phase vocoder written for the project, in pure Rust.

It's for prep, not live use. And in the demo, the agent itself flags a spot where the bars land a beat apart.
```

**6/8**

```
v0.3.0 is out. Highlights: playback works on Linux and macOS again, the assistant knows the session it's editing ("fade out the last two seconds" now fades), and the window no longer freezes during long edits.
```

**7/8**

```
What it can't do yet, so you don't have to find out: no stem separation, no transcription (so no editing by text). Builds are unsigned, so macOS and Windows warn on first launch. The release notes have the fix.
```

**8/8**

```
Download for macOS (universal), Windows or Linux: https://github.com/laadtushar/edytlab/releases/latest

Demo and docs: https://edytlab.com

MIT-licensed. Tell me which edit you'd ask for first. [EXTENDED-INTRO DEMO LINK, when #432 is live]
```

---

## Notes

- If you pin one post, pin post 1.
- Reply to questions with the issue number when one exists.
- Post 6 repeats the v0.3.0 release notes. Check them against
  <https://github.com/laadtushar/edytlab/releases/latest> before posting; if
  there is a newer release, rewrite that post.
