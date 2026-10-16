# Script: the edytlab launch video (master and three platform cuts)

> **The voice lines in this file were written for the earlier takes.** The three demos were re-filmed in the app's violet look (#581, on a development
> build after v0.5.0) and the cuts re-made from them (2026-10-16). The re-filmed agent says different things, at different times, than the takes the lines were
> written against (the first downbeat, the 1 kHz low-pass, the 200 Hz high-pass, the 79-second mini-mix), and the cuts are now silent apart from the
> app's own sound. The *italic* lines and the narration clips under `out/narration/` are the source for re-narrating once ElevenLabs credits are available: check each line
> against the new footage and re-time it to the table below first. Nothing narrated is published.

One idea: **you describe the edit in plain English, an agent runs deterministic audio tools on your machine, and you hear the result.**
Structure (from `research.md`, section D): hook -> problem -> demo in numbered steps on real footage -> proof -> how it works ->
honest limits -> one call to action.

Every shot is the real recording in `website/public/demos/` (the app window, its own sound). Times in the "Footage" column are the
demo-video clock (the same clock the reel tools measured, `marketing/reels/tools/spec.py`): BT = `dj-beatmatched-transition` (2:45), EI = `dj-extended-club-intro` (1:55),
MM = `dj-mini-mix` (1:58). The recordings burn their own step caption into a card across the bottom; the cuts crop it out and set their own titles. The only thing
drawn by the editor is text, rings, chips, a labelled diagram and the end card. No simulated UI, no invented numbers.

Voice: none for now. The cuts carry the story in on-screen text; the only sound is the app's own (the Listen shot) and three synthesised UI sounds. The lines in *italics* are
the earlier narration, kept for re-narrating.

## The claim ledger (every claim in the video, and where it is true)

| On screen or said | Where it is true |
|---|---|
| Free, open source (MIT) | `README.md` "License: MIT"; `marketing/press-kit.md` one-sentence description |
| macOS, Windows, Linux | `README.md` "Builds for macOS (universal), Windows and Linux" |
| No version number anywhere | The footage is a **development build after v0.5.0** (the app's violet look, and a limiter that goes on the master bus, which v0.5.0 lacks: `marketing/social-kit/reddit.md`). The newest release, v0.5.0, looks different, so naming it would misstate what is shown. The note under every shot says "Development build" |
| 93 deterministic tools | `docs/tools-reference.md` ("Every one of the 93 tools the AI agent can call"); press kit boilerplate |
| Audio files stay here; only the chat goes to the provider you choose | press kit boilerplate ("audio files are never uploaded; only the chat goes to the language-model provider the user selects") and fact sheet ("Where audio goes: Nowhere. Processing is local. Only chat text goes to the chosen provider.") |
| Anthropic, OpenAI, OpenRouter, Groq, Gemini, Ollama (local) | press kit fact sheet "AI providers" |
| Rust DSP engine | press kit boilerplate ("run locally in a Rust engine"); README "Pure-Rust DSP" |
| No stem separation or transcription yet | press kit "What is not shipped": "registered as tools but not implemented in any build". The video says they are **not** there; it never shows them |
| It preps files, it is not a live DJ tool | press kit: "It is an offline editor for files. It is not a live DJ tool" |
| Installers are unsigned | press kit fact sheet "Signing: Not signed or notarized"; README Releases ("every release so far is unsigned", #386) |
| Demo tracks are short synthetic loops | **the owner's own posts** (`marketing/social-kit/linkedin-posts.md`, `marketing/social-kit/reddit.md`). Nothing in the app, the website or the docs says it. Owner to confirm (README, decision 5) |
| "You type the edit": the question typed into the composer ("I'm mixing Midnight Drive into Neon Rush. What tempo is each track? Don't change anything yet.") | the recording (BT 23.4-28.4); `demos.ts`: "A DJ asks what they are working with" |
| A real tool call; "Tempo matched, pitch kept" | the request "Time-stretch Neon Rush to Midnight Drive's tempo, keeping its pitch." and the green "Time stretch" chip (BT 40.4-43.4); `demos.ts`: "Neon Rush is time-stretched to Midnight Drive's 120 BPM, keeping its pitch" |
| It flags: bars a beat apart | the agent's reply: "Bars don't line up yet: Neon Rush's bars start one beat later than Midnight Drive's ... Kicks will hit together, but phrases will be one beat apart." (BT 48.4-52.3); `demos.ts`: "Claude points out that its bars then run a beat behind" |
| Fades + low-pass, in plain English; Three tool calls | the request "Blend them: fade Midnight Drive out from 16 seconds to its end, fade Neon Rush in over its first 16 seconds, and put a low-pass filter on Midnight Drive from 16 seconds to the end ..." and the Fade, Low-pass filter and Fade chips (BT 73.2-79) |
| Crossfade, 16 to 32 s | the same request, and the agent's "fading Midnight Drive out over 16-32 s" (BT 56-81); the fade shapes on the two waveforms (BT 77-81) |
| Press play: real audio; Heard in real time | the transport's play button and the playhead moving at the speed of the timeline (15.6 px/s measured on BT 86-103); `demos.ts`: "every playback is in real time, with sound" |
| Peak -1.0 dBFS, -13.9 LUFS; Exports a 50 s WAV | the export reply: "stereo WAV, 44.1 kHz, 50.13 s long ... the file peaks at -1.0 dBFS ... the same master I measured at -13.9 LUFS" (BT 142-148); `demos.ts`. The master was asked for at -14 LUFS, so the chip says what was measured |
| A 200 Hz high-pass until the drop | the agent's reply "Cutoff: 200 Hz takes out most of the kick's thump" to "Put a high-pass filter on the 16-bar intro, so the low end is held back and comes in at the drop." (EI 61.4-65.4); `demos.ts`: "A 200 Hz high-pass holds the low end back until the drop at 31.45 s" |
| Three-track mini-mix with 4-bar overlaps | the request "Sequence them: Midnight Drive first, then Solar Flare starting 4 bars before Midnight Drive ends, then Neon Rush starting 4 bars before Solar Flare ends." and the three staggered clips (MM 42.4-46.4); `demos.ts` (mini-mix caption) |
| Real audio, real time; waits are sped up | `demos.ts`: "Waits for Claude are sped up; every playback is in real time, with sound." |

Not said or shown anywhere: stem separation or transcription working, signed builds, user or download counts, testimonials, model names, a version number,
"live" or "real-time DJ" use. The recordings show the export keeping the one-beat offset the agent flagged earlier (it says so in the reply: "The one-beat offset between the two tracks'
bars is still in this export"); the cuts claim the numbers in the reply, not a perfect blend.

---

## Master script (the LinkedIn cut, 69.3 s, 1080x1080)

Motion kit on every scene: kinetic titles (word by word, emphasized decelerate), a camera that eases between a wide shot and the part of
the screen that matters, callouts that draw themselves on, the violet progress line (see `research.md`, section M). The look is the website's: see `README.md`.

| Time (s) | Footage | On-screen text | Camera and motion | *Voice (written for the earlier takes)* | Audio |
|---|---|---|---|---|---|
| 0.0-3.5 | none: brand bars | the site's lockup, then **I built an audio editor you talk to.** | lockup and words rise and de-blur; breathing bars underneath | *I built an audio editor you talk to.* | soft whoosh |
| 3.5-10.4 | none | **One blend. Seven chores.** then the list: Find both tempos / Stretch one to match / Line up the downbeats / Draw the fades / Filter the outgoing track / Check the loudness / Export a file; then **Or just describe it.** | list items slide in 0.3 s apart; the last card is big and centred | *Prepping one blend is a pile of small chores. Or just describe it.* | whoosh x2 |
| 9.9-14.9 | BT 23.4-28.4: the question is typed and sent | **1 · Describe it in plain English**; chip "You type the edit" | wide shot eases into the composer (0.9 s); ring draws around the input | *You type the edit in plain English.* | tick |
| 14.6-19.0 | BT 40.4-44.8: the request, the "Time stretch" chip | **2 · The agent calls a tool**; chips "A real tool call", "Tempo matched, pitch kept" | composer, then the chat's lower half; two rings | *The agent picks a tool, and runs it.* | ticks |
| 18.7-22.6 | BT 48.4-52.3: "Bars don't line up yet ..." | **3 · It flags what it noticed**; chip "It flags: bars a beat apart" | camera holds on the reply; one ring | *It flags a downbeat half a second into the file.* | tick |
| 22.3-30.3 | BT 73.2-81.2: the blend request, three tool chips, the waveforms | **4 · Describe the blend**; chips "Fades + low-pass, in plain English", "Three tool calls", "Crossfade, 16 to 32 s" | chat, then the two waveforms; the ring lands on the crossfade zone | *Then the blend: a crossfade, with a low-pass.* | ticks |
| 30.0-39.2 | BT 86.07-95.27: **the real playback** (the sound starts 0.26 s into the shot, at 86.33 s) | **5 · Hear it**; chips "Press play: real audio", "Heard in real time" | push-in to the crossfade; a thin ring follows the playhead | *Have a listen.* (before the music, never over it) | **real app audio** |
| 38.9-44.9 | BT 142.0-148.0: the export reply | **6 · Master and export**; chips "Peak −1.0 dBFS, −13.9 LUFS", "Exports a 50 s WAV" | camera on the reply; two rings | *Mastered to minus fourteen loofs, exported as a WAV.* | ticks |
| 44.6-48.6 | EI 61.4-65.4 | **Also: hold the low end until the drop**; chip "A 200 Hz high-pass until the drop" | the chat's lower half, then the reply; one ring | *The same recordings show an extended intro, and a three-track mini-mix.* | tick |
| 48.3-52.3 | MM 42.4-46.4 | **And a three-track mini-mix**; chip "4-bar overlaps" | wide timeline, ring on the overlaps | (same line) | tick |
| 52.2-58.9 | none: labelled diagram | **93 tools. Run on your machine.** Your computer (Audio files stay here / 93 deterministic tools / Rust DSP engine) -> "chat text only" -> The provider you choose (Anthropic, OpenAI, OpenRouter, Groq, Gemini, Ollama (local)) | boxes rise, the arrow draws itself | *Ninety-three tools run on your machine. Only the chat goes to your provider.* | whoosh |
| 58.8-64.2 | none | **Honest limits**: Demo tracks are short synthetic loops / It preps files. It is not a live DJ tool / No stem separation or transcription yet / Installers are unsigned: expect a warning | rows slide in 0.32 s apart | *Limits: synthetic tracks, no stems, unsigned installers.* | whoosh |
| 64.2-69.3 | none: brand end card | the site's lockup / **Free, open source** / MIT licence · macOS · Windows · Linux / edytlab.com (pill) / github.com/laadtushar/edytlab / Which prep job would you hand off first? | mark and wordmark rise, lines stagger, URL pill springs in; the longest hold | *Free and open source. What would you hand off first?* | chime |

Captions: every title and list above is burned in (the cut is meant to be watched muted) and is also in `launch-linkedin.srt`.
Note under the frame all the way through the demo: "Development build · real screen recording · waits sped up".

---

## Cut 1. LinkedIn: `launch-linkedin.mp4` (1080x1080, 69.3 s)

Square because LinkedIn lists 1:1 as supported, most people watch on a phone, a square fills more of a phone feed than 16:9, and the
chat panel is near-square so the zoomed shots fill the frame (`research.md`, section A). Founder framing in the first frame ("I built ...")
because that is how the owner's post opens; the proof (real numbers, an edge case the agent flags itself, real audio) follows in the demo. Timing: the master script above, exactly.

## Cut 2. Reddit: `launch-reddit.mp4` (1920x1080, 63.7 s)

Understated: no founder hook, no chores montage, no exclamation, no hype word. A plain first line, then the real recording from 4 s.

| Time (s) | Footage | On-screen text | *Voice (written for the earlier takes)* |
|---|---|---|---|
| 0.0-4.4 | none | the site's lockup, then **An open-source audio editor you drive in plain English** | *An open-source audio editor you drive in plain English.* |
| 4.0-9.0 | BT 23.4-28.4 | 1 · Describe it in plain English; chip "You type the edit" | *A DJ types the edit in plain English.* |
| 8.7-13.1 | BT 40.4-44.8 | 2 · The agent calls a tool; "A real tool call", "Tempo matched, pitch kept" | *The agent calls a tool: a time stretch.* |
| 12.8-16.7 | BT 48.4-52.3 | 3 · It flags what it noticed; "It flags: bars a beat apart" | *It points out a downbeat half a second into the file.* |
| 16.4-24.4 | BT 73.2-81.2 | 4 · Describe the blend; "Fades + low-pass, in plain English", "Three tool calls", "Crossfade, 16 to 32 s" | *Then the blend: a crossfade, with a low-pass on the outgoing track.* |
| 24.1-33.3 | BT 86.07-95.27 (real audio from 24.4 s) | 5 · Hear it; "Press play: real audio", "Heard in real time" | *Have a listen.* |
| 33.0-39.0 | BT 142.0-148.0 | 6 · Master and export; "Peak −1.0 dBFS, −13.9 LUFS", "Exports a 50 s WAV" | *Mastered to minus fourteen loofs, exported as a WAV.* |
| 38.7-42.7 | EI 61.4-65.4 | Also: hold the low end until the drop; "A 200 Hz high-pass until the drop" | *Two more jobs from the same recordings: an extended intro, and a three-track mini-mix.* |
| 42.4-46.4 | MM 42.4-46.4 | And a three-track mini-mix; "4-bar overlaps" | (same line) |
| 46.3-53.0 | none | 93 tools. Run on your machine. + the diagram | *The tools run locally, in Rust. Only the chat goes to your provider.* |
| 52.9-58.4 | none | Honest limits (4 rows) | *Limits: synthetic tracks, no stems, unsigned installers.* |
| 58.3-63.7 | none | the site's lockup / Free, open source / MIT licence · macOS · Windows · Linux · installers are unsigned / **github.com/laadtushar/edytlab** (pill) / edytlab.com | *Source is on GitHub, under the MIT license.* |

16:9 because the recording is landscape and Reddit shows it full width. Reddit video posts have no body: paste the post text from
`marketing/social-kit/reddit.md` as the first comment. Re-read that subreddit's rules on the day (README, decision 6).

## Cut 3. Instagram Reel: `launch-instagram.mp4` (1080x1920, 31.5 s)

The hook is on screen from frame 3, the point lands in the first 3 s, and every word the cut adds sits inside x 120-960, y 269-1248 (Meta's
published 14 % top and 35 % bottom, with the reels' 120 px at each side; Meta's own side figure is 6 %, 65 px. It is an ad rule, the strictest published). The recording itself may
use the rest, and `engine.py` refuses to build a title, chip, list or card line outside the zone.

| Time (s) | Footage | On-screen text | *Voice (written for the earlier takes)* |
|---|---|---|---|
| 0.0-2.5 | none: brand bars | the site's lockup, then **Describe the blend. Hear the mix.** | *Describe the blend. Hear the mix.* (played 1.1x so it ends before step 1) |
| 2.2-6.6 | BT 23.6-28.0 | 1 · Say it in plain English; "You type the edit" | *Type it, in plain English.* |
| 6.3-10.0 | BT 40.4-44.1 | 2 · The agent calls a tool; "A real tool call" | *The agent runs a tool.* |
| 9.7-15.7 | BT 74.4-80.4 | 3 · Describe the blend; "Three tool calls", "Crossfade, 16 to 32 s" | *Then ask for the blend: a crossfade, with a low-pass.* |
| 15.4-22.4 | BT 86.07-93.07 (real audio from 15.7 s) | 4 · Hear it; "Press play: real audio", "Heard in real time" | *Listen.* |
| 22.1-26.3 | BT 142.0-146.2 | 5 · Master and export; "Peak −1.0 dBFS, −13.9 LUFS" | *Mastered, and exported as a WAV.* |
| 26.1-31.5 | none | the site's lockup / Free, open source / Unsigned installers · synthetic demo tracks / edytlab.com (pill) / macOS · Windows · Linux | *Free and open source. Unsigned installers.* |

No trending audio: the only sound is the app's own, so the Reel is "original audio" (`research.md`, section C).

## Posting copy

Use the existing, already-checked copy: `marketing/social-kit/linkedin-posts.md` post 1 (attach `launch-linkedin.mp4`, upload `launch-linkedin.srt`),
`marketing/social-kit/reddit.md` (first comment under `launch-reddit.mp4`), `marketing/social-kit/instagram.md` (caption for `launch-instagram.mp4`). The video
repeats their honest limits; the posts add nothing the video contradicts.
