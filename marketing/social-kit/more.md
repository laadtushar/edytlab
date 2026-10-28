# Everything else: X, Bluesky, Mastodon, TikTok, YouTube, Show HN, newsletter

All in your voice, the warm one from [`brand-voice.md`](../brand-voice.md):
a scene first, one idea a post, plain words. Technical channels (Show HN)
go deeper on how it works, but still as a maker explaining what was hard and
what is missing. Same accuracy rules as the other files: no stem separation
claims, transcription only as v0.8.0 describes it (a one-time model download,
checked on English only), installers described as unsigned wherever a
download appears, the demo tracks are synthetic loops, no invented numbers.

**Links:**
site <https://edytlab.com> ·
source <https://github.com/laadtushar/edytlab> ·
v0.9.3 <https://github.com/laadtushar/edytlab/releases/tag/v0.9.3> ·
DJ recordings <https://edytlab.com/use-cases/dj>

**Videos:** `reel-highlights.mp4`, `reel-beatmatched-transition.mp4`,
`reel-extended-club-intro.mp4`, `reel-mini-mix.mp4`, `reel-teaser.mp4` (no
voiceover yet). The full recordings are
`website/public/demos/dj-beatmatched-transition.mp4` (2:23),
`dj-extended-club-intro.mp4` (1:39) and `dj-mini-mix.mp4` (1:42), also at
`https://edytlab.com/demos/<name>.mp4`.

---

## 1. X thread (7 posts) and single post

Attach `reel-highlights.mp4` to post 1 as native video. Every post is 280
characters or fewer (counted by script; X counts any link as 23).

**1/7** (attach `reel-highlights.mp4`)

```
The night before a gig is all chores: matching tempos, counting bars, nudging levels.

So I built an audio editor I could just tell what to do. The video is three DJ jobs on synthetic demo loops, each described in plain English.
```

**2/7**

```
Job one: blend a 128 BPM track into a 120 BPM one. It slows the 128 to 120 without changing the pitch, starts it under the last 8 bars of the other, crossfades the two, and exports a WAV.
```

**3/7**

```
Job two: the intro is too short to mix over. It copies the drums-only first 8 bars and splices them in, so the intro runs 16 bars, and holds the low end back until the drop.
```

**4/7**

```
Job three: a warm-up mix. Three tracks at 124 BPM, each one coming in about 4 bars before the last one ends, crossfaded, with a limiter on the master. One WAV at the end.
```

**5/7**

```
The part I like best: it shows its work. Every edit appears in the chat as a step you can read, and every one can be undone.
```

**6/7**

```
It never hears your music. The AI reads a summary, like tempo, key and loudness, and picks from 99 editing tools. The tools run on your computer. Only the chat goes out, to the AI you choose, or stays home with a local model.
```

**7/7**

```
Free and open source (MIT), for macOS, Windows and Linux. Built for prep, not for playing live. You stay the DJ.

https://edytlab.com
https://github.com/laadtushar/edytlab
```

**Single post** (attach `reel-teaser.mp4` or `reel-highlights.mp4`):

```
The night before a gig is all chores. So I built a free, open-source audio editor you talk to. In the video, a DJ describes a blend and it time-stretches, crossfades, masters for streaming and exports a WAV. The editing runs on your computer.

https://edytlab.com
```

---

## 2. Bluesky (300 characters or fewer)

Attach `reel-highlights.mp4`.

```
The night before a gig is all chores: matching tempos, counting bars, nudging levels. So I built an editor you talk to. The video: a beatmatched blend, a 16-bar intro, a three-track mini-mix, all described in plain English. Free, open source, and your audio stays on your machine. https://edytlab.com
```

---

## 3. Mastodon (500 characters or fewer)

Attach `reel-highlights.mp4`, and add alt text (use the one in `instagram.md`).

```
The night before a gig is all chores: matching tempos, counting bars, nudging levels. So I built an audio editor you talk to. Free and open source (MIT), for macOS, Windows and Linux.

The video shows three DJ jobs, described in plain English: a 128 BPM track beatmatched to 120, an 8-bar drum intro stretched to 16, and a three-track mini-mix. The editing tools run on your computer and your audio isn't uploaded. The demo tracks are synthetic loops.

https://edytlab.com

#OpenSource #Rust #DJ
```

---

## 4. TikTok and YouTube Shorts

TikTok and Shorts hashtags are kept to three. If the app offers a toggle to
label AI-generated or AI-edited content, read its wording and decide: the
video is a screen recording of an AI agent editing synthetic demo tracks, and I
could not check from here whether either platform would want it flagged.

### `reel-highlights.mp4`

TikTok:

```
Matching tempos, counting bars, nudging levels. Or just say what you want. Three DJ jobs, described in plain English, each ending in a finished file. Free and open source. Link in bio. #dj #beatmatching #opensource
```

YouTube Shorts title (under 100 characters):

```
Three DJ jobs, described in plain English, in a free audio editor
```

YouTube Shorts description:

```
The night before a gig is all chores: matching tempos, counting bars, nudging levels. Here, three of those jobs are described in plain English: a beatmatched blend, an extended club intro and a three-track mini-mix. Each ends in an exported WAV.

edytlab is a free, open-source audio editor for macOS, Windows and Linux. The editing runs on your computer, and the demo tracks are short synthetic loops.

Demos and downloads (the installers are unsigned): https://edytlab.com
Source: https://github.com/laadtushar/edytlab

#Shorts #DJ #OpenSource
```

### `reel-beatmatched-transition.mp4`

TikTok:

```
Blending a 128 BPM track into a 120 BPM one usually means doing the maths on both. Here you just say it: the faster track is slowed to match and starts under the last 8 bars of the other. Link in bio. #beatmatching #dj #opensource
```

Shorts title:

```
Beatmatch a 128 and a 120 BPM track by describing it
```

Shorts description:

```
A 128 BPM track has to blend into a 120 BPM one. The DJ starts by asking for each tempo, and nothing changes yet. Then the 128 is slowed to 120 without changing its pitch and started under the last 8 bars of the other. The first track fades out under a low-pass filter while the second fades in. The mix is mastered for streaming and exported as a WAV. The demo tracks are synthetic loops.

edytlab is free and open source (MIT): https://edytlab.com
The installers are unsigned, so macOS and Windows warn on first launch.

#Shorts #DJ #BeatMatching
```

### `reel-extended-club-intro.mp4`

TikTok:

```
The intro is too short to mix over. So copy the drums-only first 8 bars, splice them in, and it runs 16. A high-pass filter keeps the low end back until the drop. Link in bio. #djedits #dj #opensource
```

Shorts title:

```
Extend a drum intro from 8 to 16 bars by describing it
```

Shorts description:

```
A club intro that's too short to mix over. The DJ asks for the tempo and bar length first, and nothing changes. Then the drums-only first 8 bars are copied and spliced in, so the intro runs 16 bars, and a high-pass filter holds the low end back until the drop on bar 17. The demo track is a synthetic loop.

edytlab is free and open source (MIT): https://edytlab.com
The installers are unsigned, so macOS and Windows warn on first launch.

#Shorts #DJ #MusicProduction
```

### `reel-mini-mix.mp4`

TikTok:

```
A warm-up mix is mostly overlap maths. Here it's three tracks at 124 BPM, each one coming in about 4 bars before the last ends, then one exported WAV. Link in bio. #djmix #dj #opensource
```

Shorts title:

```
A three-track DJ warm-up mix, built by describing it
```

Shorts description:

```
Two tracks are time-stretched to 124 BPM, keeping their pitch, then all three are lined up so each comes in about 4 bars before the previous one ends, with a crossfade over the overlap. Each is brought to a similar loudness, the master gets a limiter, and the mix is exported as a WAV. The demo tracks are synthetic loops.

edytlab is free and open source (MIT): https://edytlab.com
The installers are unsigned, so macOS and Windows warn on first launch.

#Shorts #DJ #DJMix
```

### `reel-teaser.mp4` (12 to 15 seconds)

TikTok:

```
Describe the blend. Get the file. A free, open-source audio editor you talk to. Link in bio. #dj #beatmatching #opensource
```

Shorts title:

```
Describe the blend, get the file: a free open-source audio editor
```

Shorts description:

```
edytlab is a free, open-source desktop audio editor you talk to. The editing tools run on your computer. Full demos: https://edytlab.com (the installers are unsigned).

#Shorts #DJ #OpenSource
```

---

## 5. YouTube long-form description (the three full recordings)

Title options:

1. `edytlab: three DJ jobs by describing them (beatmatch, extended intro, mini-mix)`
2. `I built an audio editor you talk to: three full DJ demos, with sound`

**Chapters.** The three recordings are 2:23, 1:39 and 1:42. If you upload them
back to back with no title cards, the chapters are 0:00, 2:23 and 4:02 and the
total is 5:44. That is arithmetic on the durations in `website/lib/demos.ts`,
not a measurement of the finished file: **check them against the actual
upload and change the numbers if you added intros or cards.** Per-step chapters
inside each demo are left out because the step times are not in the repo.

```
The night before a gig is all chores: matching tempos, counting bars, nudging levels. I built edytlab, a free, open-source desktop audio editor, so I could just tell it what to do. These are three full screen recordings of it at work, with sound, on a released build of the app.

A few things about how they were made. The three tracks are short synthetic loops I generated, not real records, so they show the workflow and not how it copes with a messy library. A test script types the requests into the real app when I record; the replies and edits are live, with Claude as the AI, and I only publish runs that passed my checks. Waits are sped up, and every playback is in real time.

Chapters
0:00 Beatmatch and blend two tracks
2:23 Extend an intro for mixing
4:02 A three-track mini-mix

What happens in each
1. Two tracks, one at 120 BPM and one at 128. The DJ asks for each tempo first, without changing anything. Then the 128 is slowed to 120 and started under the last 8 bars of the other, which fades out under a low-pass filter while the new one fades in. The mix is mastered for streaming and exported as a WAV.
2. The DJ asks for the tempo and bar length first, and nothing changes. Then the drums-only first 8 bars are copied and spliced in, so the intro runs 16 bars. A high-pass filter holds the low end back until the drop on bar 17, the first 4 bars fade in, and the track is exported as a WAV.
3. A warm-up mix at 124 BPM. Two tracks are time-stretched to 124, keeping their pitch, and all three are lined up so each comes in about 4 bars before the one before it ends, with a crossfade over each overlap. Each is brought to a similar loudness, the master gets a limiter, and the mix is exported as a WAV.

How it works
The AI never hears the audio. It chooses from 99 editing tools, and they run on your computer, in Rust. Your audio files aren't uploaded. The chat goes to the AI you pick, or stays home with a local model (Ollama).

Get it
It's free and open source (MIT), for macOS, Windows and Linux. The installers are unsigned, so macOS and Windows warn on first launch, and the release notes have the steps.
Website and docs: https://edytlab.com
Download v0.9.3: https://github.com/laadtushar/edytlab/releases/tag/v0.9.3
Source: https://github.com/laadtushar/edytlab
The DJ walkthrough: https://edytlab.com/use-cases/dj

#DJ #BeatMatching #OpenSource
```

Tags field (comma separated):

```
edytlab, AI audio editor, beatmatching, DJ prep, extended intro, time stretch, open source audio editor, Rust, Tauri, local-first, Ollama
```

---

## 6. Show HN (refreshed for v0.9.3)

Submit <https://edytlab.com>. Post the first comment as soon as it is up and
stay for several hours. Do not ask anyone to upvote.

**Title options** (counts include the `Show HN: ` prefix; all 80 or under):

1. `Show HN: edytlab – desktop audio editor where an LLM calls 99 DSP tools` (71)
2. `Show HN: edytlab – an audio editor you talk to, with the DSP in Rust` (68)
3. `Show HN: edytlab – edit audio by describing it, with every step undoable` (72)

Recommended: 1.

**First comment:**

```
Hi HN. I wanted an audio editor I could just tell what to do, so I built one. edytlab is a desktop app for macOS, Windows and Linux, under the MIT licence. You load some audio and ask for something like "bring the second track in 8 bars before the first one ends, crossfade them, keep the levels sensible and export a WAV". An LLM works through it by calling editing tools, and every step shows up in the chat.

The part I'd most like you to look at is the boundary between the model and the audio. The model never receives samples. It gets text: the state of the session, and measurements like tempo, key, beat grid and loudness. It chooses from 99 tools and puts them in order, and the tools do the signal processing, in Rust, on your machine.

There are three screen recordings of DJ jobs on the site, with sound: https://edytlab.com/#demos. Two things about them that the captions don't say. The tracks are short synthetic loops, about 32 seconds each, that a script generated. And a test script types the requests into the real app: the replies and edits are live, with Claude as the model, and I only published runs that passed my checks. The model isn't deterministic, so your run will differ, and the filter settings you see in mine aren't a spec.

Your audio files are never uploaded. What leaves your machine is the chat: your prompts, the session summary and the tool results. It goes to the provider you pick: Anthropic, OpenAI, OpenRouter, Groq or Gemini. Or it stays on your machine if you use Ollama, which needs no key. There's also a preview that signs in with your ChatGPT plan instead of a key. The desktop app has no telemetry.

How it's built. It's a Tauri 2 shell with a React front end and a Rust workspace of 17 crates. The effects live in a small in-house DSP crate with no dependencies. Decoding is symphonia, resampling is rubato and the FFTs are realfft.

Time-stretch and pitch-shift are a phase vocoder I wrote for this project. I wanted Rubber Band and dropped it, because it needs a different native package on each of the three CI targets, and one native dependency like that can break every build at once. The trade is written down at the top of crates/audio-time/src/vocoder.rs. It's cleaner on sustained material than on dense mixes, and the further the stretch gets from 1.0 the more it smears.

A project is a history, not a file. The session is a content-addressed graph, where a node's id is a blake3 of the state, so you can fork it, name a version, compare two by ear and go back. That graph taught me something about undo: a revisited state keeps its first parent, so undo went to the wrong place after mute then unmute. Since v0.4.0 the front end remembers the path you actually took (#398). There's also an optional plan-first mode, which holds the first step that would change the session until you approve it.

What isn't there yet:

- Stem separation. The tool is registered and returns an error today, because the inference isn't built (#385).
- The installers are unsigned (#386). macOS needs one xattr command after you drag the app to Applications, and Windows shows a SmartScreen warning. The release notes have the steps.
- It edits files. It isn't a live DJ tool or a DAW, so no MIDI and no plugin hosting. Only gain, the limiter and three filters can sit live on a track, a bus or the master. Reverb, EQ or a compressor gets baked into the track's audio instead.
- Local models are what I've tested least. In one measurement the first request was about 15,000 tokens (#395), too big for a model with an 8,192-token context, so Ollama now gets a smaller tool set. I haven't run a local model through a full job, and the recordings used a hosted model.
- The limiter works on sample peaks with no look-ahead, so it isn't a true-peak limiter.
- On Linux, API keys go in the Secret Service, with a warning when none is running. I haven't checked that across a real reboot or against a locked keyring (#497).

I develop this with Claude Code. Many commits carry a Co-Authored-By trailer, and the repo has a CLAUDE.md with the working rules.

If you try it, I'd love to know which edit you'd ask for that isn't in the tool list in docs/tools-reference.md. If you run local models, which ones can drive a tool list this size without losing the thread? And if you do DSP, the vocoder and the limiter are where I expect the sharpest criticism.

Download v0.9.3 (unsigned): https://github.com/laadtushar/edytlab/releases/tag/v0.9.3
Source: https://github.com/laadtushar/edytlab
```

Answers to have ready:

| Likely question | Answer |
|---|---|
| Why not script ffmpeg or sox? | If the pipeline is fixed, you should. This is for the edits you would otherwise do by ear and by hand: you hear each step, and you keep a history of what you tried. |
| Does the LLM hear the audio? | No. It reads the session state and results from tools like `analyze_track`: BPM, key, beat grid and loudness. |
| What does it cost? | The app is free. A hosted provider charges per token on your own key, Ollama is free, and with a ChatGPT Plus or Pro plan (a preview, v0.9.0 and later) the usage comes out of the plan instead. Don't quote a per-edit price; I never measured one. |
| Is the audio really never uploaded? | There is no server of mine in the loop. What leaves your machine is the chat to the provider you chose, plus the request Settings makes to that provider for its model list (your key or sign-in, no prompt or audio). You can watch it with your OS network tools. |
| Why Tauri and not Electron? | The back end is Rust, so the UI process and the DSP crates share one workspace. I haven't measured size or speed against Electron, so I won't claim either. |
| Is there a web version? | No. A browser version is something I'm weighing in #389, and it's undecided. |
| 99 tools is a lot for a model. | It is. For Ollama the app sends a core set of about 15 plus the ones your message names, and lists the rest by name (#395, #479). Hosted providers get all of them, partly because Anthropic's prompt cache needs a fixed list. Agent profiles and a capabilities blacklist can narrow the list, and plan first keeps you in the loop. |

---

## 7. Newsletter and Discord announcement

**Newsletter**

Subject options:

1. `edytlab v0.9.3: check loudness, find who's speaking, open more file types`
2. `Three DJ jobs, described in plain English`

Body:

```
You finish a recording and the same questions come up. Is it loud enough? Is there noise under the quiet bits? Will it pass? edytlab v0.9.3 can answer them now. It's out for macOS, Windows and Linux, and it's the free, open-source audio editor you talk to.

The new measure tool reports loudness, peaks, noise floor and clipping, for the whole mix or one track. Ask it to check against ACX and it gives a pass or fail for each ACX rule.

The other new thing is for interviews. The assistant can now work out who is speaking when, on your computer, and hand that to the tool that splits a track by speaker. It needs a speaker model, about 33 MB from Hugging Face, which you download once with the Download button under the speech model in Settings → Account. Nothing downloads at launch, when Settings opens or when the assistant asks, and no audio is sent anywhere.

Also in this release: a click track, a chirp and DTMF tones you can generate, and AIFF and Opus (mono and stereo) files now open.

If you last tried it around v0.7.0, a few more things have landed since:

- It can transcribe. Ask the assistant to transcribe a track, or cut words in the Transcript tab. It runs on your computer once you've downloaded the speech model from Settings → Account (about 139 MB). Nothing downloads by itself. I've checked it on English only, on a short recording and about 16 minutes of audiobook speech, so read the words before you cut on them. The words reach your AI provider as text, never the audio.
- Sign in with ChatGPT is a preview. Choose ChatGPT plan in Settings, press Continue with ChatGPT, and the assistant uses your Plus or Pro plan instead of an API key. Your browser opens OpenAI's sign-in page, and edytlab never sees your password. If your plan's limit runs out, the chat says so and stops. It may not work on workspace plans, which depends on OpenAI and your admin. If your first message failed on v0.9.0, update: v0.9.1 fixed that.
- On macOS, the login password prompt now comes at most once per launch, because your keys and the ChatGPT sign-in share one keychain entry. The first launch after updating may ask once for each old entry while it moves your keys over.
- Edits to a track recorded at a different sample rate than the project now land in the right place, and noise reduction no longer clicks at the start and end of a track.

One thing worth doing: if you're on v0.7.0 or older, please update, because a batch run could render onto its own inputs. And if you're on v0.6.0 or older, update before you compact a project or delete the original of a Save As copy, because those versions could lose audio.

Stem separation still isn't there.

The three DJ demos have sound now: a beatmatched blend, an extended club intro and a three-track mini-mix. The tracks are short synthetic loops. https://edytlab.com/#demos

Download (the installers are unsigned, so macOS and Windows warn on first launch): https://github.com/laadtushar/edytlab/releases/latest
Full changelog: https://edytlab.com/changelog

If you try it, I'd like to hear which edit you ask for first.
```

**Discord (short)**

```
edytlab v0.9.3 is out (macOS, Windows, Linux). The new measure tool reports loudness, peaks, noise floor and clipping, with an ACX pass or fail. The new diarize tool labels who speaks when, on your computer, once you've downloaded its 33 MB speaker model in Settings. You can also generate click tracks, chirps and DTMF tones, and open AIFF and Opus files.

Since v0.8.0 it can transcribe too, on your computer, after you download the speech model in Settings (checked on English only). And Sign in with ChatGPT is a preview: the assistant can use your Plus or Pro plan instead of an API key. If your first message failed on v0.9.0, update, v0.9.1 fixed it.

Worth updating if you're on v0.7.0 or older (a batch run could render onto its own inputs) or v0.6.0 or older (compacting a project or deleting a Save As original could lose audio).

Stem separation still isn't there. Three DJ demos, with sound: https://edytlab.com/#demos
Download (unsigned, so expect a first-launch warning): https://github.com/laadtushar/edytlab/releases/latest
Tell me which edit you'd ask for first.
```

---

## Notes (not part of the posts)

- Release facts (Sign in with ChatGPT as a preview, transcription and its
  download, the sample-rate fix, the render-over-source fix, the noise
  reduction click, and "stem separation is still not available") are from the
  v0.9.3, v0.9.2, v0.9.1 and v0.9.0 entries in `website/app/changelog/page.tsx` and the
  v0.8.1 and v0.8.0 release notes on GitHub. The
  v0.7.0 version of this section (controls that stay put, the History graph,
  bundled skills, the ONNX Runtime in the installers with no model) and the
  v0.6.0 and v0.6.1 version, which led with the compaction and Save As
  data-loss fixes, are in git history (`git log -p -- marketing/social-kit/more.md`).
  The "update if you are on v0.6.0 or older" line is the data-loss advice
  that still applies (#589 compaction, fixed in v0.6.0; #612 Save As, fixed in
  v0.6.1). The v0.6.0 notes' "adding a label works on macOS" is still left out:
  it is old news now, though the in-page name box that fixes it did pass on one
  real Mac (v0.9.0 and v0.9.1, 2026-10-25; #567, closed).
- The Show HN comment and the newsletter were rewritten in the warmer voice
  of `brand-voice.md` (the earlier text is in
  `marketing/archive/copy/2026-10-28-before-brand-voice/social-kit/more.md`).
  What they leave out on purpose is not a retraction: the older-version
  data-loss advice stays in the newsletter and Discord post, where readers on
  old builds are, and the byte counts, the transcription accuracy figures and
  the v0.5.0 and older warnings stay in this file's archive copy and in the
  Q&A and `marketing/README.md`.
- Show HN says "The desktop app has no telemetry" and does **not** repeat the
  older draft's "the website has no analytics". `website/app/privacy/page.tsx`
  says the website uses anonymous Vercel analytics, and
  `marketing/launch/show-hn.md`, `marketing/README.md` and
  `marketing/press-kit.md` now say the same (#472). Since #684 the site can
  also run Google Analytics 4 behind a consent banner once a measurement ID is
  set; those three files name it, and none of them says "no cookies" without
  scoping it to Vercel's pair.
- The newsletter and Discord text list release changes only, and not all of
  them (the Discord post names the headline ones): the full list is on the
  changelog page if your audience wants it.
- Where the YouTube chapters come from: durations in `website/lib/demos.ts`.
  Recheck them against the real upload.
