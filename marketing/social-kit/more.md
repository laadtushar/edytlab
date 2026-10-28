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
The night before a gig is all chores: matching tempos, counting bars, nudging levels. So I built an audio editor you talk to. The video is a beatmatched blend, a 16-bar intro and a three-track mini-mix, each described in plain English. Free, open source, and your audio stays on your computer. https://edytlab.com
```

---

## 3. Mastodon (500 characters or fewer)

Attach `reel-highlights.mp4`, and add alt text (use the one in `instagram.md`).

```
The night before a gig is all chores: matching tempos, counting bars, nudging levels. So I built a desktop audio editor you talk to. Free, open source (MIT), for macOS, Windows and Linux.

The video shows three DJ jobs, each described in plain English: a 128 BPM track beatmatched to 120, a drum intro stretched from 8 bars to 16, and a three-track mini-mix. The AI picks the editing tools. They run on your computer, so your audio isn't uploaded. The demo tracks are synthetic loops.

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
Two tracks are slowed or sped up to 124 BPM without changing their pitch, then all three are lined up so each comes in about 4 bars before the previous one ends, with a crossfade over the overlap. Each is brought to a similar loudness, the master gets a limiter, and the mix is exported as a WAV. The demo tracks are synthetic loops.

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
It's free and open source (MIT), for macOS, Windows and Linux. It's for prepping files, not for playing live. The installers are unsigned, so macOS and Windows warn on first launch, and the release notes have the steps.
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
2. `Show HN: edytlab – edit audio by chatting; the DSP runs locally in Rust` (71)

Recommended: 1.

**First comment:**

```
Hi HN. edytlab is a desktop audio editor (macOS, Windows, Linux, MIT) where you edit by describing what you want. Load some audio and ask for something like "time-stretch the second track to match the first one's tempo, start it 8 bars before the first ends, crossfade, limit at -1 dB, bring it to -14 LUFS and export a WAV". An LLM agent turns that into calls to 99 audio tools. The tools do the signal processing; the model only chooses and sequences them, and it never receives samples. It gets text: the session state and measurements such as tempo, key, beat grid and loudness.

There are three screen recordings of DJ jobs on the site (2:23, 1:39 and 1:42, with sound): https://edytlab.com/#demos. Some things about them that the captions don't say:

- The three tracks are short synthetic loops (about 32 seconds each) generated by a script, not real records.
- A test script types the requests into the real app. The agent's replies and edits are live, with Claude as the model, and I only publish runs that passed my checks. The model isn't deterministic, so yours will differ.
- The replies on screen are the model's as they came, including the readings it reports and the filter settings it picked. None of that is a spec of the product, and a take will differ.

Where things run. Decode, DSP, mixing and export run in-process in Rust. Audio files are never uploaded. The chat (your prompts, the context and the tool results) goes to the provider you pick: Anthropic, OpenAI, OpenRouter, Groq, Gemini, or Ollama, which needs no key and keeps the chat on your machine. In a preview you can instead sign in with your ChatGPT plan, and the chat goes to api.openai.com on that sign-in. Settings also asks that provider for its model list, with your key or sign-in and no prompt or audio. The desktop app has no telemetry.

How it's built:

- Tauri 2 shell, React front end, a Rust workspace of 17 crates.
- Effects live in an in-house audio-dsp crate with no dependencies. Decode is symphonia, resampling rubato, FFT realfft.
- Time-stretch and pitch-shift are a phase vocoder written for this project. I wanted Rubber Band and dropped it: it needs a different native package on each of the three CI targets, and a native dependency like that breaks every build at once. The trade is documented at the top of crates/audio-time/src/vocoder.rs. It's cleaner on sustained material than on dense mixes, and the further the factor is from 1.0 the more it smears.
- Session state is a content-addressed graph (node id is a blake3 of the state): fork, name, compare (A/B), revert. That was a trap for undo: a revisited state keeps its first parent, so undo went to the wrong place after mute then unmute. v0.4.0 keeps the path the user took in the frontend (#398).
- An optional "plan first" mode: the first step that would change the session is held for your approval.

What doesn't work, so you don't have to find it:

- No stem separation. The tool is registered and returns an error today; the inference isn't built (#385).
- Transcription (v0.8.0) needs a one-time download of the Whisper base model (about 139 MB), which you start in Settings. It runs on your computer and the audio never leaves it; the words reach your AI provider as text, unless you use Ollama. I've checked it on one short synthetic English recording and about 16 minutes of English audiobook speech (4% to 7% of the words wrong), so read the words before you cut on them.
- The installers are unsigned (#386). macOS needs one xattr command after you drag the app to Applications, and Windows shows a SmartScreen warning. The release notes have the steps.
- It's an offline editor for files. It isn't a live DJ tool or a DAW: no MIDI, no plugin hosting. Only five effects (gain, the limiter and three filters) can sit live on a track, a bus or the master bus; reverb, EQ or a compressor is baked into the track's audio instead.
- Local models: in v0.4.0 the first request was about 15,000 tokens in my one measurement (#395), and a model with an 8,192-token context refused it. Since v0.5.0 Ollama gets a smaller tool set (about 8.7 KB of request text for a one-line message, against about 67 KB with every tool; bytes, not tokens). I haven't re-run that model on it, and I haven't run a local model through a full job; the recordings used a hosted model.
- On Linux, v0.4.0 and earlier kept API keys in the kernel keyring only until you reboot (#394). Since v0.5.0 they go in the Secret Service, with a warning when none is running. I haven't checked it across a real reboot or against a locked keyring yet (#497).
- If you tried an earlier build: v0.5.0 and older could delete your own recordings when the assistant compacted history (#589), and v0.6.0 could lose a Save As copy's audio once the original was deleted (#612), so please use v0.6.1 or later.
- The limiter works on sample peaks with no look-ahead; it isn't a true-peak limiter.

I develop this with Claude Code; many commits carry a Co-Authored-By trailer, and the repo has a CLAUDE.md with the working rules.

Feedback I'd like:

1. If you edit audio: which edit would you ask for that isn't in the tool list (docs/tools-reference.md)?
2. If you run local models: which ones can drive a tool list this size without losing the thread?
3. If you do DSP: the vocoder and the limiter are where I expect the most justified criticism.

Download v0.9.3 (unsigned): https://github.com/laadtushar/edytlab/releases/tag/v0.9.3
Source: https://github.com/laadtushar/edytlab
```

Answers to have ready:

| Likely question | Answer |
|---|---|
| Why not script ffmpeg or sox? | For a fixed pipeline you should. This is for edits you would otherwise do by ear and by hand, with an audible A/B at each step and a history of what you tried. |
| Does the LLM hear the audio? | No. It gets the session state and tool results such as BPM, key, beat grid and loudness from `analyze_track`. |
| What does it cost? | The app is free. A hosted provider charges you per token under your own key; Ollama is free; with a ChatGPT Plus or Pro plan (a preview, v0.9.0 and later) the usage comes out of the plan instead. Don't quote a per-edit price; none was measured. |
| Is the audio really never uploaded? | There is no server of mine in the loop. The traffic is the chat to your chosen provider, plus the request Settings makes to it for its model list (your key or sign-in, no prompt or audio). You can check it with your OS network tools. |
| Why Tauri and not Electron? | The back end is Rust, so the UI process and the DSP crates share one workspace. Don't claim size or speed numbers; none were measured. |
| Is there a web version? | No. A browser-based version is being evaluated in #389 and is undecided. |
| 99 tools is a lot for a model. | It is. For Ollama, the app sends a core set of about 15 plus the ones your message names, and lists the rest by name (#395, #479; since v0.5.0). Hosted providers still get all of them; Anthropic's prompt cache needs a fixed list. Agent profiles and a capabilities blacklist can narrow the list, and plan first keeps a human in the loop. |

---

## 7. Newsletter and Discord announcement

**Newsletter**

Subject options:

1. `edytlab v0.9.3: measure loudness, find who speaks when, and import AIFF and Opus`
2. `Three DJ jobs, done by chatting with an AI audio agent`

Body:

```
edytlab v0.9.3 is out for macOS, Windows and Linux. It's a free, open-source desktop audio editor where you describe an edit and an AI agent runs audio tools on your files.

Two tools lead this release. The new measure tool reports integrated, momentary and short-term loudness, loudness range, sample and true peak, RMS, noise floor, digital silence and clipping, for the mix or one track, and check "acx" gives a pass or fail for each ACX rule. The new diarize tool labels who speaks when on a track or file, on your computer, and the assistant can hand the turns to split_by_speaker. It needs a speaker model (about 33 MB, two files from Hugging Face) that you download once with the Download button under the speech model in Settings → Account; nothing downloads at launch, when Settings opens or when the assistant asks, and no audio is sent.

The preview: Sign in with ChatGPT (new in v0.9.0; in v0.9.0 its first message after signing in failed with "Unsupported content type"; v0.9.1 fixes that error, so use v0.9.1 or later). In Settings, choose ChatGPT plan and press Continue with ChatGPT, and the assistant uses your ChatGPT Plus or Pro plan's usage instead of an API key. Your browser opens OpenAI's sign-in page and edytlab never sees your password. If your plan's limit is reached, the chat says so and stops; it does not switch to another provider. It may not be available on workspace plans, which depends on OpenAI and your admin. Your audio stays on your computer; the chat goes to OpenAI, as it does with any provider.

What else changed since v0.7.0:
- Test signals (v0.9.3): generate_click_track, generate_chirp and generate_dtmf add a track, including in an empty project. AIFF and Opus (mono and stereo) files now import; surround Opus and M4A/AAC fail with a message that says to convert.
- Settings has an Audio tab (v0.9.3) that measures how late your microphone hears what the computer plays, for the output and input pair you use. Nothing uses the number yet.
- macOS asks for your login password at most once per launch, because your API keys and the ChatGPT sign-in share one keychain entry (v0.9.2; the first launch after updating moves your existing keys over, and may ask once for each old entry). Transcription no longer skips stretches of speech, and its word times are closer (v0.9.2).
- Retry sends your message once, a template's empty tracks show as lanes, and tone and noise generation work in an empty project (v0.9.2).
- Transcription (v0.8.0). Ask the assistant to transcribe a track, or cut words in the Transcript tab. It runs on your computer with the Whisper base speech model (about 139 MB), which you download once with the Download button in Settings → Account; nothing downloads by itself. I've checked it on one short synthetic English recording and about 16 minutes of English audiobook speech (4% to 7% of the words wrong), so read the words before you cut on them. The words it finds reach your AI provider as text, never the audio.
- Edits to a track recorded at a different sample rate from the project land in the right place (v0.8.1).
- Noise reduction no longer clicks at the start and end of a track, and tempo and key are read correctly in more cases (v0.9.0).
- A render can no longer be written over audio your project reads, and a batch run no longer renders two inputs onto one file (v0.8.0).

Stem separation is still not available.

If you are on v0.7.0 or older, update: a batch run could render onto its own inputs. If you are on v0.6.0 or older, update before you compact a project or delete the original of a Save As copy: those versions could lose audio.

The three DJ demos were recorded with sound in the app's violet look, with the limiter on the master bus: a beatmatched blend, an extended club intro and a three-track mini-mix. The tracks are short synthetic loops, and it's for prepping files, not playing live: https://edytlab.com/#demos

Download (the installers are unsigned, so macOS and Windows warn on first launch): https://github.com/laadtushar/edytlab/releases/latest
Full changelog: https://edytlab.com/changelog

If you try it, I'd like to hear which edit you ask for first.
```

**Discord (short)**

```
edytlab v0.9.3 is out (macOS, Windows, Linux). New: a measure tool for loudness, peaks, noise floor and clipping with an ACX check, a diarize tool that labels who speaks when on your computer (it needs a 33 MB speaker model you download once in Settings), click-track, chirp and DTMF generators, and AIFF and Opus import. Since v0.9.2, macOS asks for your password at most once per launch (keys and the ChatGPT sign-in share one keychain entry), and transcription no longer skips stretches of speech. Sign in with ChatGPT is a preview (v0.9.1 or later; the first message failed in v0.9.0): the assistant can use your ChatGPT Plus or Pro plan instead of an API key. Since v0.8.0 it can also transcribe, on your computer, once you download the speech model in Settings (checked on English only: a short synthetic recording and about 16 minutes of audiobook speech), and edits to a track at a different sample rate land in the right place. Stem separation is still not available. On v0.7.0 or older, update: a batch run could render onto its own inputs. On v0.6.0 or older, update before you compact a project or delete a Save As copy's original: those versions could lose audio. Three DJ demos, recorded with sound: https://edytlab.com/#demos
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
