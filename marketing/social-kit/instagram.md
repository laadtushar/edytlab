# Instagram: five reel captions, story frames, bio line

Captions are written in your voice, visual first: each one names the moment
the reel shows, tells a line or two of story, then points to the link in bio.
Instagram does not make URLs in captions clickable, so every call to action
says "link in bio" and there is no URL in a caption.

**Bio link:** set it to <https://edytlab.com> (or
<https://edytlab.com/use-cases/dj> if you want the DJ page with all three
recordings).

**Bio line (under 150 characters):**

```
An audio editor you talk to. Free and open source, for prepping DJ blends. Mac, Windows, Linux. Demos below.
```

**Videos:** `reel-highlights.mp4`, `reel-beatmatched-transition.mp4`,
`reel-extended-club-intro.mp4`, `reel-mini-mix.mp4`, `reel-teaser.mp4`. They
carry the app's real audio and their own on-screen captions, no voiceover
(narrated versions, with `.srt` captions, will come once narration credits are
available). Instagram generally does not take an `.srt` upload, so the
on-screen captions have to do the work there.

**Conventions:** no emoji, and three hashtags at the end of each caption.
Each caption carries at most one limit, said lightly: the demo tracks are
synthetic loops, or this is for prep and not for playing live. The download
page states the unsigned installers, and the story frame that sends people to
the site repeats it. Hashtag popularity was not checked (no live Instagram
access here): swap any tag that looks dead or off-topic when you paste.

---

## 1. `reel-highlights.mp4` (the lead reel)

**Hook line:**
The night before a gig goes on chores.

**Caption:**

```
The night before a gig goes on chores. Matching tempos, counting bars, nudging levels.

Here I hand three of them to an editor I just type to: a blend, a longer intro, a short mix. Each one comes back as a finished WAV.

It's called edytlab. I built it, it's free and open source, and your music stays on your computer.

It's for prep, not for playing live. You stay the DJ.

Link in bio for the demos. Which prep chore would you hand off first?

#dj #djtips #opensource
```

**On-screen hook text (pick one):**
1. Three DJ jobs, described in plain English
2. Beatmatch, intro, mini-mix: all from a chat
3. I built an editor you talk to

**Alt text:**

```
Screen recording of the edytlab desktop audio editor, showing a chat panel and waveform timeline as the AI handles three DJ jobs: a beatmatched blend, an extended intro and a three-track mix.
```

---

## 2. `reel-beatmatched-transition.mp4`

**Hook line:**
You have a 128 BPM track, and the one playing is 120.

**Caption:**

```
You have a 128 BPM track, and the one playing is 120. Getting from one to the other without a clunk is a chore.

Here I just say what I want. The editor slows the new track to 120 without changing its pitch, brings it in under the last 8 bars of the other, and fades one out while the other fades in.

The tracks are short demo loops. Link in bio to try it, free and open source.

#beatmatching #djtips #opensource
```

**On-screen hook text (pick one):**
1. Beatmatching 120 and 128 BPM by chatting
2. Ask for the blend. Get the WAV.
3. Watch two tracks get beatmatched from a chat

**Alt text:**

```
Screen recording of the edytlab app: the AI time-stretches a 128 BPM track to 120 BPM, places it under the end of another track, crossfades them with a low-pass filter, and exports a WAV.
```

---

## 3. `reel-extended-club-intro.mp4`

**Hook line:**
The track opens with 8 bars of drums, and you wish it had 16.

**Caption:**

```
The track opens with 8 bars of drums, and you wish it had 16.

Here the editor copies those first 8 bars and splices them in, so the intro runs twice as long. The low end stays out until the drop, and the first few bars fade in. Then it exports a WAV.

The demo track is a short loop. Link in bio to try it, free and open source.

#djedits #clubmusic #opensource
```

**On-screen hook text (pick one):**
1. Turn an 8-bar intro into 16 bars
2. Extended club intro, built from a chat
3. Need a longer intro to mix over?

**Alt text:**

```
Screen recording of the edytlab app: the AI copies a track's drums-only first 8 bars to make a 16-bar intro, adds a high-pass filter until the drop, and exports a WAV.
```

---

## 4. `reel-mini-mix.mp4`

**Hook line:**
A warm-up mix is mostly joins.

**Caption:**

```
A warm-up mix is mostly joins. Three tracks, and every join has to sound like it was meant.

Here I ask for the whole thing in one go. The three tracks come to one tempo, overlap by a few bars, crossfade, and settle at one level for streaming, with a limiter at the end so nothing clips. Out comes one WAV.

It preps files, it doesn't play live. Link in bio, free and open source for Mac, Windows and Linux.

#djmix #minimix #opensource
```

**On-screen hook text (pick one):**
1. Three tracks, one warm-up mix
2. A 124 BPM mini-mix, built from a chat
3. Overlaps and crossfades, from one request

**Alt text:**

```
Screen recording of the edytlab app: the AI lines up three tracks at 124 BPM with overlaps of about 4 bars and crossfades, limits the result, and exports a WAV.
```

---

## 5. `reel-teaser.mp4` (12 to 15 seconds)

**Hook line:**
Describe the blend. Get the file.

**Caption:**

```
Describe the blend. Get the file.

That's the whole idea behind edytlab, an audio editor you talk to. Beatmatching, intros, crossfades, loudness: say it in plain words and watch each step happen.

It's for prep, not for playing live. Free and open source. Link in bio for the full demos.

#dj #djtips #opensource
```

**On-screen hook text (pick one):**
1. Describe the blend. Get the file.
2. A DJ prep tool you talk to
3. Free, open-source, and it beatmatches

**Alt text:**

```
Short screen recording of the edytlab desktop audio editor, with a chat panel next to a waveform timeline, as the AI edits a DJ blend.
```

---

## Story frames

1. Question sticker on a still from `reel-highlights.mp4`:
   `What's the most tedious step when you prep a blend?`
2. Text over a short clip from `reel-beatmatched-transition.mp4`:
   `New reel on the feed: three prep chores, handed off by typing.`
3. Text on a plain frame, with the link sticker to <https://edytlab.com>:
   `Free and open source, for Mac, Windows and Linux. The installers are unsigned, so expect a warning the first time you open it.`

---

## Notes (not part of the captions)

Where the details come from: the captions in `website/lib/demos.ts` (the
fixture tempos, 8 and 16 bars, and the -14 LUFS and -1 dBFS the DJ asks for;
not the cutoffs the agent picks, the length of an export or what it says it
found, which differ from take to take); the demo tracks being synthetic comes from
`apps/desktop/e2e-native/fixtures.mjs`.

- The captions do not name the model. The recordings use Claude; say so in a
  comment if asked.
- "The audio is processed on your computer" is true. If a comment asks about
  privacy, the full answer is: audio files are not uploaded; the chat text goes
  to the AI provider you pick, or stays local with Ollama.
- The reels are vertical edits made separately from the website videos. If a
  detail in a caption (for example the high-pass filter) is cut from your
  final edit, trim that line so the caption describes only what the viewer
  sees.
