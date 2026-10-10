# Everything else: X, Bluesky, Mastodon, TikTok, YouTube, Show HN, newsletter

All in your voice. Same accuracy rules as the other files: no stem separation
or transcription claims, installers described as unsigned wherever a download
appears, the demo tracks are synthetic loops, no invented numbers.

**Links:**
site <https://edytlab.com> ·
source <https://github.com/laadtushar/edytlab> ·
v0.4.0 <https://github.com/laadtushar/edytlab/releases/tag/v0.4.0> ·
DJ recordings <https://edytlab.com/use-cases/dj>

**Videos:** `reel-highlights.mp4`, `reel-beatmatched-transition.mp4`,
`reel-extended-club-intro.mp4`, `reel-mini-mix.mp4`, `reel-teaser.mp4`; each
also as `-narrated` with a matching `.srt`. The full recordings are
`website/public/demos/dj-beatmatched-transition.mp4` (3:08),
`dj-extended-club-intro.mp4` (1:56) and `dj-mini-mix.mp4` (2:18), also at
`https://edytlab.com/demos/<name>.mp4`.

---

## 1. X thread (7 posts) and single post

Attach `reel-highlights.mp4` to post 1 as native video. Every post is 280
characters or fewer (counted by script; X counts any link as 23).

**1/7** (attach `reel-highlights.mp4`)

```
I built a desktop audio editor you talk to. The video: three DJ jobs, each described in plain English and ending in an exported WAV.

Free, open source, macOS / Windows / Linux.
```

**2/7**

```
Job 1: a 128 BPM track is time-stretched to 120 (pitch kept), started under the last 8 bars of the other, crossfaded with a 2 kHz low-pass on the outgoing one, mastered to -14 LUFS and exported as a WAV.
```

**3/7**

```
Job 2: copy a track's drums-only first 8 bars, splice them in, and the intro runs 16 bars. A 150 Hz high-pass holds the low end back until the drop.
```

**4/7**

```
Job 3: three tracks at 124 BPM, 4-bar overlaps and crossfades, about -14 LUFS each, a limiter at -1 dBFS, exported as an 80-second WAV.
```

**5/7**

```
The part I like best: the agent flags its own edge cases. A downbeat half a second into a file. A last entry half a bar off the beat grid. An export peaking at 0 dBFS with no headroom.
```

**6/7**

```
How it works: the model never hears the audio. It gets text (tempo, key, loudness) and picks from 93 deterministic tools. The DSP runs on your machine, in Rust. The chat goes to the provider you pick, or stays local with Ollama.
```

**7/7**

```
Limits: the demo tracks are short synthetic loops I generated. It preps files, not live sets. No stem separation. Installers are unsigned, so macOS and Windows warn on first launch.

https://edytlab.com
https://github.com/laadtushar/edytlab
```

**Single post** (attach `reel-teaser.mp4` or `reel-highlights.mp4`):

```
I built a free, open-source audio editor you talk to. In the video, a DJ describes a blend and an AI agent time-stretches, crossfades, masters to -14 LUFS and exports a WAV. Audio is processed on your machine. It preps files, not live sets.

https://edytlab.com
```

---

## 2. Bluesky (300 characters or fewer)

Attach `reel-highlights.mp4`.

```
I built an audio editor you talk to. The video is three DJ jobs described in plain English: a beatmatched blend, a 16-bar extended intro, a three-track mini-mix. Free, open source (MIT), audio processed on your machine. It preps files, not live sets. https://edytlab.com
```

---

## 3. Mastodon (500 characters or fewer)

Attach `reel-highlights.mp4`, and add alt text (use the one in `instagram.md`).

```
I built a free, open-source desktop audio editor you talk to (macOS, Windows, Linux; MIT).

The video shows three DJ jobs described in plain English: a 128 BPM track beatmatched to 120, an 8-bar drum intro extended to 16, and a three-track mini-mix. An AI agent calls 93 audio tools that run on your machine; the chat goes to the provider you choose, or stays local with Ollama.

Demo tracks are synthetic loops. Unsigned installers. https://edytlab.com

#OpenSource #Rust #AudioEngineering #DJ
```

---

## 4. TikTok and YouTube Shorts

TikTok hashtags are kept to five. If the app offers a toggle to label
AI-generated or AI-edited content, read its wording and decide: the video is
a screen recording of an AI agent editing synthetic demo tracks, and I could
not check from here whether either platform would want it flagged.

### `reel-highlights.mp4`

TikTok:

```
Three DJ jobs, described in plain English: a beatmatched blend, an extended intro, a three-track mini-mix. Free and open source. Link in bio. #dj #djtips #beatmatching #musicproduction #opensource
```

YouTube Shorts title (under 100 characters):

```
Three DJ jobs by chatting with an AI agent (free, open source)
```

YouTube Shorts description:

```
Three DJ jobs described in plain English: a beatmatched blend, an extended club intro and a three-track mini-mix. edytlab is a free, open-source desktop audio editor for macOS, Windows and Linux. The audio is processed on your computer; the chat goes to the AI provider you choose, or stays local with Ollama. Demo tracks are synthetic loops. It preps files; it isn't a live DJ tool.

Demos and downloads (installers are unsigned): https://edytlab.com
Source: https://github.com/laadtushar/edytlab

#Shorts #DJ #BeatMatching #OpenSource
```

### `reel-beatmatched-transition.mp4`

TikTok:

```
A 128 BPM track time-stretched to 120, blended under the end of another, mastered to -14 LUFS and exported. The agent also flags a half-second downbeat offset. Link in bio. #beatmatching #djtransitions #djing #audioengineering #opensource
```

Shorts title:

```
AI agent beatmatches 120 and 128 BPM tracks and exports a WAV
```

Shorts description:

```
A DJ asks what they are working with: both tracks are in F major, at 120 and 128 BPM. The 128 BPM track is time-stretched to 120 and started under the last 8 bars of the other, which fades out under a 2 kHz low-pass while the new one fades in. The mix is mastered and exported as a 50-second WAV at -14 LUFS. Demo tracks are synthetic loops.

edytlab is free and open source (MIT): https://edytlab.com
Installers are unsigned, so macOS and Windows warn on first launch.

#Shorts #DJ #BeatMatching
```

### `reel-extended-club-intro.mp4`

TikTok:

```
Need a longer intro to mix over? The drums-only first 8 bars are copied and spliced in, so it runs 16 bars, with a 150 Hz high-pass until the drop. Link in bio. #djedits #extendedmix #djtips #musicproduction #opensource
```

Shorts title:

```
Extend a drum intro from 8 to 16 bars by describing it
```

Shorts description:

```
The agent reports the tempo and bar length (124 BPM, a 1.935-second bar, first downbeat at 0.476 s), copies the drums-only first 8 bars and splices them in so the intro runs 16 bars. A 150 Hz high-pass holds the low end back until the drop on bar 17, and it warns that the export peaks at 0 dBFS with no headroom. The demo track is a synthetic loop.

edytlab is free and open source (MIT): https://edytlab.com
Installers are unsigned, so macOS and Windows warn on first launch.

#Shorts #DJ #MusicProduction
```

### `reel-mini-mix.mp4`

TikTok:

```
Three tracks at 124 BPM, sequenced with 4-bar overlaps and crossfades, limited at -1 dBFS, exported as an 80-second WAV. The agent flags a half-bar offset. Link in bio. #djmix #minimix #djing #audioengineering #opensource
```

Shorts title:

```
A three-track DJ warm-up mix, built by chatting with an AI agent
```

Shorts description:

```
Two tracks are time-stretched to 124 BPM keeping their pitch, then all three are sequenced so each comes in 4 bars before the previous one ends, with a crossfade over each overlap. Each is brought to about -14 LUFS and the mix gets a limiter at -1 dBFS before it is exported as an 80-second WAV. The agent flags that the last entry lands half a bar off the beat grid, because one track is 16 and a half bars long. Demo tracks are synthetic loops.

edytlab is free and open source (MIT): https://edytlab.com
Installers are unsigned, so macOS and Windows warn on first launch.

#Shorts #DJ #DJMix
```

### `reel-teaser.mp4` (12 to 15 seconds)

TikTok:

```
Describe the blend. Get the file. A free, open-source audio editor you talk to. Link in bio. #dj #beatmatching #musicproduction #aiaudio #opensource
```

Shorts title:

```
Describe the blend, get the file: a free open-source audio editor
```

Shorts description:

```
edytlab is a free, open-source desktop audio editor where an AI agent calls audio tools that run on your computer. Full demos: https://edytlab.com (installers are unsigned).

#Shorts #DJ #OpenSource
```

---

## 5. YouTube long-form description (the three full recordings)

Title options:

1. `edytlab v0.4.0: three DJ jobs with an AI audio agent (beatmatch, extended intro, mini-mix)`
2. `I built an audio editor you talk to: three full DJ demos, with sound`

**Chapters.** The three recordings are 3:08, 1:56 and 2:18. If you upload them
back to back with no title cards, the chapters are 0:00, 3:08 and 5:04 and the
total is 7:22. That is arithmetic on the durations in `website/lib/demos.ts`,
not a measurement of the finished file: **check them against the actual
upload and change the numbers if you added intros or cards.** Per-step chapters
inside each demo are left as TODO because the step times are not in the repo.

```
edytlab is a free, open-source desktop audio editor where you describe an edit and an AI agent runs audio tools to do it. These are three full screen recordings of the app on the v0.4.0 build, recorded with sound. Waits for the agent are sped up; every playback is in real time. The agent in the recordings is Claude.

Read this before you watch:
- The three tracks are short synthetic loops I generated for the demos, not real records. These show the workflow, not how it copes with a messy library.
- A test script types the requests into the real app when I record. The agent's replies and edits are live. I only publish runs that passed my checks.
- It's for prepping audio files, not for playing live. There are no decks or controllers.
- No stem separation and no transcription: those tools exist but their inference isn't shipped.

Chapters
0:00 Beatmatch and blend two tracks
3:08 Extend an intro for mixing
5:04 A three-track mini-mix

What happens in each
1. Both tracks are in F major, at 120 and 128 BPM. The 128 BPM track is time-stretched to 120 and started under the last 8 bars of the other. The agent points out that its first downbeat sits half a second into the file and offers to nudge it. The first track fades out under a 2 kHz low-pass while the second fades in, then the mix is mastered for streaming and exported as a 50-second WAV at -14 LUFS, peaking at -1 dBFS. (TODO: step timestamps)
2. The agent reports the tempo and bar length (124 BPM, a 1.935-second bar, first downbeat at 0.476 s) without changing anything. It copies the drums-only first 8 bars and splices them in, so the intro runs 16 bars. A 150 Hz high-pass holds the low end back until the drop on bar 17, the first 4 bars fade in, and the track is exported as a WAV. The agent flags that it peaks at 0 dBFS, with no headroom. (TODO: step timestamps)
3. A warm-up mix at 124 BPM. Two tracks are time-stretched to 124, keeping their pitch. The agent analyses all three and sequences them so each comes in 4 bars before the one before it ends, with a crossfade over each overlap, and flags that the last entry lands half a bar off its beat grid because one track is 16 and a half bars long. Each track is brought to about -14 LUFS, the three are rendered into a Master track with a limiter at -1 dBFS, and the mix is exported as an 80-second WAV. (TODO: step timestamps)

How it works
The model never hears the audio. It chooses and orders calls to 93 deterministic audio tools; the signal processing runs on your computer, in Rust. Your audio files aren't uploaded. The chat goes to the AI provider you pick (Anthropic, OpenAI, OpenRouter, Groq, Gemini), or stays local with Ollama.

Get it
Free and open source (MIT) for macOS, Windows and Linux. The installers are unsigned, so macOS and Windows warn on first launch; the release notes have the steps.
Website and docs: https://edytlab.com
Download v0.4.0: https://github.com/laadtushar/edytlab/releases/tag/v0.4.0
Source: https://github.com/laadtushar/edytlab
The DJ walkthrough, with the limits: https://edytlab.com/use-cases/dj

#DJ #BeatMatching #OpenSource #AudioEditing
```

Tags field (comma separated):

```
edytlab, AI audio editor, beatmatching, DJ prep, extended intro, time stretch, open source audio editor, Rust, Tauri, local-first, Ollama
```

---

## 6. Show HN (refreshed for v0.4.0)

Submit <https://edytlab.com>. Post the first comment as soon as it is up and
stay for several hours. Do not ask anyone to upvote.

**Title options** (counts include the `Show HN: ` prefix; all 80 or under):

1. `Show HN: edytlab – desktop audio editor where an LLM calls 93 DSP tools` (71)
2. `Show HN: edytlab – edit audio by chatting; the DSP runs locally in Rust` (71)

Recommended: 1.

**First comment:**

```
Hi HN. edytlab is a desktop audio editor (macOS, Windows, Linux, MIT) where you edit by describing what you want. Load some audio and ask for something like "time-stretch the second track to match the first one's tempo, start it 8 bars before the first ends, crossfade, limit at -1 dB, bring it to -14 LUFS and export a WAV". An LLM agent turns that into calls to 93 audio tools. The tools do the signal processing; the model only chooses and sequences them, and it never receives samples. It gets text: the session state and measurements such as tempo, key, beat grid and loudness.

There are three screen recordings of DJ jobs on the site (3:08, 1:56 and 2:18, with sound): https://edytlab.com/#demos. Some things about them that the captions don't say:

- The three tracks are short synthetic loops (about 32 seconds each) generated by a script, not real records.
- A test script types the requests into the real app. The agent's replies and edits are live, with Claude as the model, and I only publish runs that passed my checks. The model isn't deterministic, so yours will differ.
- The agent flags its own edge cases in them: a first downbeat half a second into a file, a last entry half a bar off the beat grid, an export peaking at 0 dBFS. I left those in on purpose.

Where things run. Decode, DSP, mixing and export run in-process in Rust. Audio files are never uploaded. The chat (your prompts, the context and the tool results) goes to the provider you pick: Anthropic, OpenAI, OpenRouter, Groq, Gemini, or Ollama, which needs no key and keeps the chat on your machine. The desktop app has no telemetry.

How it's built:

- Tauri 2 shell, React front end, a Rust workspace of 16 crates.
- Effects live in an in-house audio-dsp crate with no dependencies. Decode is symphonia, resampling rubato, FFT realfft.
- Time-stretch and pitch-shift are a phase vocoder written for this project. I wanted Rubber Band and dropped it: it needs a different native package on each of the three CI targets, and a native dependency like that breaks every build at once. The trade is documented at the top of crates/audio-time/src/vocoder.rs. It's cleaner on sustained material than on dense mixes, and the further the factor is from 1.0 the more it smears.
- Session state is a content-addressed graph (node id is a blake3 of the state): fork, name, compare (A/B), revert. That was a trap for undo: a revisited state keeps its first parent, so undo went to the wrong place after mute then unmute. v0.4.0 keeps the path the user took in the frontend (#398).
- An optional "plan first" mode: the first step that would change the session is held for your approval.

What doesn't work, so you don't have to find it:

- No stem separation and no transcription. Both tools are registered and both return an error today; the ONNX runtime and model delivery aren't built (#383, #384, #385). That also means transcript-based editing doesn't work end to end.
- The installers are unsigned (#386). macOS needs one xattr command after you drag the app to Applications, and Windows shows a SmartScreen warning. The release notes have the steps.
- It's an offline editor for files. It isn't a live DJ tool or a DAW: no MIDI, no plugin hosting. There's no master-bus effect tool yet, so the agent renders a baked "Master" track (#460).
- Local models: the first request is large (about 15,000 tokens in my one measurement, #395), and a model with an 8,192-token context refused it. I haven't run a local model through a full job; the recordings used a hosted model.
- On Linux, API keys are held in the kernel keyring and don't survive a reboot (#394).
- The limiter works on sample peaks with no look-ahead; it isn't a true-peak limiter.

I develop this with Claude Code; many commits carry a Co-Authored-By trailer, and the repo has a CLAUDE.md with the working rules.

Feedback I'd like:

1. If you edit audio: which edit would you ask for that isn't in the tool list (docs/tools-reference.md)?
2. If you run local models: which ones can drive a tool list this size without losing the thread?
3. If you do DSP: the vocoder and the limiter are where I expect the most justified criticism.

Download v0.4.0 (unsigned): https://github.com/laadtushar/edytlab/releases/tag/v0.4.0
Source: https://github.com/laadtushar/edytlab
```

Answers to have ready:

| Likely question | Answer |
|---|---|
| Why not script ffmpeg or sox? | For a fixed pipeline you should. This is for edits you would otherwise do by ear and by hand, with an audible A/B at each step and a history of what you tried. |
| Does the LLM hear the audio? | No. It gets the session state and tool results such as BPM, key, beat grid and loudness from `analyze_track`. |
| What does it cost? | The app is free. A hosted provider charges you per token under your own key; Ollama is free. Don't quote a per-edit price; none was measured. |
| Is the audio really never uploaded? | There is no server of mine in the loop. The traffic is the chat to your chosen provider. You can check it with your OS network tools. |
| Why Tauri and not Electron? | The back end is Rust, so the UI process and the DSP crates share one workspace. Don't claim size or speed numbers; none were measured. |
| Is there a web version? | No. A browser-based version is being evaluated in #389 and is undecided. |
| 93 tools is a lot for a model. | It is. That's #395. Agent profiles and a capabilities blacklist can narrow the list, and plan first keeps a human in the loop. |

---

## 7. Newsletter and Discord announcement

**Newsletter**

Subject options:

1. `edytlab v0.4.0: Play works, long requests finish, and three DJ demos`
2. `Three DJ jobs, done by chatting with an AI audio agent`

Body:

```
edytlab v0.4.0 is out for macOS, Windows and Linux. It's a free, open-source desktop audio editor where you describe an edit and an AI agent runs audio tools on your files.

What changed:
- Play works straight after opening a file or making an edit, and you can play, pause and place the playhead with the mouse.
- A request with many steps finishes. The tool budget is now 20, and reaching it ends with a summary of what was done and what is left.
- The limiter limits instead of clipping: 0.19% distortion on a test tone where the old clip gave 23%.
- With Plan first on, nothing changes until you approve it.
- Undo and redo follow the path you took.
- The assistant's replies are formatted, and the window no longer freezes after the assistant's edits on Linux.

I also recorded three DJ jobs on this build, with sound: a beatmatched blend, an extended club intro and a three-track mini-mix. The tracks are short synthetic loops, and it's for prepping files, not playing live: https://edytlab.com/#demos

Download (the installers are unsigned, so macOS and Windows warn on first launch): https://github.com/laadtushar/edytlab/releases/tag/v0.4.0
Full changelog: https://edytlab.com/changelog

If you try it, I'd like to hear which edit you ask for first.
```

**Discord (short)**

```
edytlab v0.4.0 is out (macOS, Windows, Linux). Play works straight after an edit, long requests finish, the limiter limits instead of clipping, and Plan first holds every edit until you approve it. Three DJ demos recorded on this build, with sound: https://edytlab.com/#demos
Download (unsigned, so expect a first-launch warning): https://github.com/laadtushar/edytlab/releases/tag/v0.4.0
Tell me which edit you'd ask for first.
```

---

## Notes (not part of the posts)

- Release facts (play, mouse transport, formatted replies, tool budget of 20,
  limiter 0.19% against 23%, Plan first, undo path, window freeze on Linux)
  are from the v0.4.0 release notes and `website/app/changelog/page.tsx`.
- Show HN says "The desktop app has no telemetry" and does **not** repeat the
  older draft's "the website has no analytics". `website/app/privacy/page.tsx`
  says the website uses anonymous Vercel analytics, so
  `marketing/launch/show-hn.md`, `marketing/README.md` and `marketing/press-kit.md`
  currently contradict the privacy page. Fix those before anyone quotes them.
- The newsletter and Discord text list release changes only. It does not
  mention the reasoning-effort setting (Anthropic models only) because that
  would need a sentence of qualification; add it if your audience uses
  Anthropic.
- Where the YouTube chapters come from: durations in `website/lib/demos.ts`.
  Recheck them against the real upload.
