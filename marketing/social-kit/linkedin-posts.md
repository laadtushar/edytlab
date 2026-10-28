# LinkedIn: five posts

Five posts with different angles, written in your voice (see
[`../brand-voice.md`](../brand-voice.md)). Post one a few days
apart, not back to back (see `calendar.md`). Each has a video to attach as
native video and a one-line comment to pin right after you publish. Each also
has a square image, and post 3 has a carousel PDF to post in place of its reel;
both are in the new design of the site, under `linkedin/` (see
[`linkedin/README.md`](linkedin/README.md)).

**Videos:** `reel-highlights.mp4`, `reel-beatmatched-transition.mp4`,
`reel-mini-mix.mp4`, `reel-extended-club-intro.mp4`, `reel-teaser.mp4`. The
reels carry their captions on screen, so there is no `.srt` to upload to
LinkedIn's captions option yet (narrated versions with `.srt` captions will
come once narration credits are available).

**Links:**
site <https://edytlab.com> ·
source <https://github.com/laadtushar/edytlab> ·
download <https://github.com/laadtushar/edytlab/releases/latest> (the newest
release, v0.9.3 on 2026-10-27; the posts name no version, so the link is
all to check)

**Conventions used in every post:** no emoji; at most 3 hashtags at the end;
one message per post, in the voice of [`../brand-voice.md`](../brand-voice.md);
only post 1 has a link in the body, the others carry the link in the pinned
comment, because outbound links in the body are commonly reported to reduce
reach (not verified here). Wherever a download is mentioned, the installers
are described as unsigned.

---

## 1. Launch: what it is (attach `reel-highlights.mp4`)

Angle: the chore every DJ knows, handed off in plain English. About 190 words.

```
The night before a gig goes on chores. Matching tempos, counting bars, nudging levels, track after track. None of it is the fun part.

I wanted an editor I could just tell what to do. So I built one.

In the video I ask it for three things, in plain English. Blend a 128 BPM track into a 120 BPM one over the last 8 bars. Build an extended club intro from a track's opening drums. Stitch three tracks into a short mix. Each one comes back as a finished WAV.

The part I care about most: it shows its work. Every edit appears in the chat as a step you can read, check and undo. You stay the DJ. It just does the counting.

And your music stays yours. The AI never hears the audio. It reads a summary of the session and picks from 99 editing tools that run on your own computer, so nothing gets uploaded.

It's called edytlab. Free and open source, for macOS, Windows and Linux, and built for prep, not for playing live.

What's the prep job you'd hand off first?

https://edytlab.com

#DJ #MusicProduction #OpenSource
```

**Pin this comment:**

```
Download for macOS, Windows and Linux: https://github.com/laadtushar/edytlab/releases/latest

The installers aren't signed yet, so expect a warning the first time you open it.
```

**Image:** `linkedin/out/post-1-launch.png` (1200x1200). Use it as the video's
cover if LinkedIn's uploader offers a custom thumbnail, or post it alone. The
chat in it is the app's real chat panel with a scripted example conversation (the
numbers are fixture values, not a model's output), and the image says so.

**Shorter version (about 50 words), same video:**

```
Most DJ prep is chores: matching tempos, lining up bars, checking levels.

I built an audio editor you can hand them to in plain English. It shows every step, and your audio never leaves your computer.

Free and open source. https://edytlab.com

#DJ #OpenSource
```

---

## 2. The DJ workflow, before and after (attach `reel-beatmatched-transition.mp4`)

Angle: the small chores behind one blend, then the same blend described in a
sentence, and the moment that builds trust. About 200 words.

```
Every blend between two tracks starts the same way. Find both tempos. Stretch one to match. Line up the downbeats. Draw the fades. Check the level. Export. Then you do it again for the next pair.

In the video, the DJ describes the blend. Bring the 128 BPM track in under the last 8 bars of the 120 BPM one, fade the old track out under a low-pass filter, and get the mix ready for streaming.

It starts by telling the DJ each track's tempo, without changing anything. Then it slows the faster track to 120 BPM without changing its pitch, starts it at 16 seconds, lays the two fades over each other and exports a WAV.

The moment I trust it most comes after each edit. It says in plain words what it just changed, so you never hunt for what moved. And you can listen to the transition in the app before anything is exported.

One thing to know: the tracks in the video are short loops I generated, so it shows the workflow, not how it copes with a real record.

Which step of your own prep would you hand off first?

#DJing #AudioEditing #OpenSource
```

**Pin this comment:**

```
The DJ walkthrough, with all three recordings: https://edytlab.com/use-cases/dj
```

**Image:** `linkedin/out/post-2-before-after.png` (1200x1200), same use, and the
same scripted example conversation, captioned as one.

---

## 3. The build story (attach `reel-mini-mix.mp4`)

Angle: the one design decision everything hangs off, that the model never
touches a sample, told as how I built it. About 210 words.

```
If you've ever wondered whether an AI audio editor is listening to your music: mine isn't. The model never touches a sample, and everything else in the design follows from that.

It reads a short summary of the session, like each track's tempo, key and loudness, and picks from 99 editing tools: fades, EQ, filters, time-stretch, loudness, export. Those tools are ordinary Rust code running on your computer. The video shows it sequencing a three-track mix with them.

That one choice gave me the things I care about. Every step is a tool you can read, so nothing happens behind your back. Every edit is a node in a history you can fork, compare and undo, so nothing is lost. And since the model never gets the audio, there is nothing to upload.

I even wrote the time-stretch myself instead of bundling a library, so the app builds the same way on macOS, Windows and Linux. The price is that dense mixes can smear when you stretch them a lot.

I build it with Claude Code, and the working rules live in a CLAUDE.md in the repo. The source is MIT, if you want to read how any of it works.

What would you want to review first?

#Rust #OpenSource #AudioEngineering
```

**Pin this comment:**

```
Every tool, with its parameters, generated from the tool registry: https://github.com/laadtushar/edytlab/blob/main/docs/tools-reference.md
```

**Image and carousel:** `linkedin/out/post-3-model-never-touches-a-sample.png`
(1200x1200), and `linkedin/out/post-3-carousel.pdf`, seven 1080x1350 pages
that walk through the same four points with screenshots of the app (the real
interface; the conversation, plan and history in them are scripted examples, and
each page says so). LinkedIn
takes a PDF as a document post, so post it instead of the reel. If you do,
delete the sentence in the post's second paragraph that says what the video
shows: there is no video in that version.

---

## 4. Privacy and local-first, with the Ollama option (attach `reel-extended-club-intro.mp4`)

Angle: the first question people ask about an AI audio tool, answered plainly,
with the one part still being tested said once. About 160 words.

```
You have a recording you would rather not send anywhere. An unreleased track, or an interview someone trusted you with. So the first thing you ask about any AI audio tool is: where does my audio go?

For edytlab, nowhere. Decoding, editing, mixing and export all run on your computer. The extended intro in the video was built that way, and the app itself sends no telemetry.

What does leave is the chat. Your messages, plus a short text summary of the session (a track's tempo, key and loudness), go to the AI you picked. The model never receives the audio.

If you'd rather keep even the chat at home, run a local model with Ollama. It needs no key, and the chat stays on your machine too.

Local models are the part I'm still testing. I haven't confirmed which ones can finish a whole job yet, so if you run one, I'd like to hear how it goes.

#LocalFirst #Privacy #OpenSource
```

**Pin this comment:**

```
What stays on your machine, what leaves, and how to run it with Ollama: https://edytlab.com/use-cases/local-ai-audio-editor
```

**Image:** `linkedin/out/post-4-privacy.png` (1200x1200), same use.

---

## 5. Open source: help wanted (attach `reel-teaser.mp4`)

Angle: three specific invitations, including the big unshipped piece. About
170 words.

```
You find a project you like, you'd happily help, and you can't tell where to start. So here is where, in mine.

edytlab is a free audio editor you drive by chatting with an AI, MIT licensed, for macOS, Windows and Linux. I'd love some company on it.

Three places to start:

- A small first change. Five open issues are labelled "good first issue" (#846 to #850): a wah-wah effect, a curve option for fades, a mute-all-tracks tool, and tests for mute, solo and pan. Each one says where in the code to begin and how it is checked.
- A Mac. Microphone recording on macOS still needs a proper check by someone who has one (issue 464).
- The big one. Stem separation isn't built yet: nothing runs a stem model. If you've shipped model inference inside a Tauri app, I'd like to talk (issue 385).

New audio tools and new AI providers are welcome too. docs/contributing.md walks through both.

Which of those would you pick up?

#OpenSource #Rust #Contributing
```

**Pin this comment:**

```
Contributing guide: https://github.com/laadtushar/edytlab/blob/main/docs/contributing.md · open issues: https://github.com/laadtushar/edytlab/issues
```

**Image:** `linkedin/out/post-5-help-wanted.png` (1200x1200), same use.

---

## Notes (not part of the posts)

Sources for the claims:

- Demo wording: the captions in `website/lib/demos.ts`. The posts quote the
  prompts the DJ types (the fixture tempos, 8 and 16 bars, 16 seconds), never
  what a take's reply reports (cutoffs, readings, lengths): those differ from
  take to take, see `marketing/README.md`, "Evergreen media rules".
- "Short loops I generated": `apps/desktop/e2e-native/fixtures.mjs` writes
  the three tracks (32 seconds each) from code. The DJ page and README say
  "tracks", so this is a disclosure the site does not currently make.
- 99 tools: `docs/tools-reference.md`. Providers and Ollama:
  `README.md`. Local-first table: `website/app/use-cases/local-ai-audio-editor/page.tsx`.
  Post 4 deliberately says nothing about context sizes or request sizes; if
  someone asks, the numbers are in `marketing/README.md` ("Known rough edges")
  and `crates/ai/tests/request_size.rs`.
- No telemetry in the app: `website/app/privacy/page.tsx`. Post 4 says
  nothing about the website's analytics, which is a different claim.
- Own time-stretch: `crates/audio-time/src/vocoder.rs`; the Rubber Band
  decision is in the build-story history (a native dependency per CI target).
- Undo, fork and compare: README, `apps/desktop/src/lib/headTrail.ts`.
- Master bus: issue 460, closed by PR 516, which v0.6.0 is the first release to ship (v0.5.0 and earlier render a baked "Master" track instead); the recordings were made after it and put the limiter on the master bus, as the mini-mix caption in `demos.ts` says.

Decisions for you:

- Post 3 says "I build it with Claude Code". It is true (commit trailers,
  `CLAUDE.md`) and `marketing/README.md` recommends disclosing once. Delete
  the line if you prefer, but decide before someone asks.
- Post 5 names #846 to #850 as the open `good first issue` issues (true when
  checked on 2026-10-27). Re-check that they are still open before posting;
  edit that paragraph if one has been taken.
- Post 1 says the agent in the recordings is Claude. That is what the demo
  captions say. No model version is named anywhere.
- A test script types the requests into the real app when the demos are
  recorded (`apps/desktop/e2e-native/stories/demos.mjs`); the replies and
  edits are the live agent's, and only runs that passed the script's checks
  become videos. The posts do not say "I typed", for that reason. Say it
  openly if asked.
