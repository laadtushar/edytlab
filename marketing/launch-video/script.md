# Script: the edytlab launch video (master and three platform cuts)

One idea: **you describe the edit in plain English, an agent runs deterministic audio tools on your machine, and you hear the result.**
Structure (from `research.md`, section D): hook -> problem -> demo in numbered steps on real footage -> proof -> how it works ->
honest limits -> one call to action.

Every shot is the real recording in `website/public/demos/` (the app window, its own sound). Times in the "Footage" column are the
demo-video clock (the same clock the reel tools measured). The only thing drawn by the editor is text, rings, chips, a labelled diagram
and the end card. No simulated UI, no invented numbers.

Voice: optional. The un-narrated cuts carry the story in on-screen text. The narrated cuts use Roger (ElevenLabs, Eleven v4,
stability 0.5, similarity 0.75), one clip per line, laid on the same timing. Lines in *italics* are the narration.

## The claim ledger (every claim in the video, and where it is true)

| On screen or said | Where it is true |
|---|---|
| Free and open source (MIT) | `README.md` "License: MIT"; `marketing/press-kit.md` one-sentence description |
| macOS, Windows, Linux, v0.4.0 | `README.md` "Builds for macOS (universal), Windows and Linux"; GitHub release `v0.4.0` is Latest, not a prerelease, with `.dmg`, `.msi`, `-setup.exe`, `.deb`, `.AppImage` (checked with the GitHub API on 2026-10-10) |
| 93 deterministic tools | `docs/tools-reference.md` ("Every one of the 93 tools the AI agent can call"); press kit boilerplate |
| Audio files stay here; only the chat goes to the provider you choose | press kit boilerplate ("audio files are never uploaded; only the chat goes to the language-model provider the user selects") and fact sheet ("Where audio goes: Nowhere. Processing is local. Only chat text goes to the chosen provider.") |
| Anthropic, OpenAI, OpenRouter, Groq, Gemini, Ollama (local) | press kit fact sheet "AI providers" |
| Rust DSP engine | press kit boilerplate ("run locally in a Rust engine"); README "Pure-Rust DSP" |
| No stem separation or transcription yet | press kit "What is not shipped": "registered as tools but not implemented in any build". The video says they are **not** there; it never shows them |
| It preps files, it is not a live DJ tool | press kit: "It is an offline editor for files. It is not a live DJ tool" |
| Installers are unsigned | press kit fact sheet "Signing: Not signed or notarized"; README Releases ("every release so far is unsigned", #386) |
| Demo tracks are short synthetic loops | **the owner's own posts** (`marketing/social-kit/linkedin-posts.md`, `marketing/social-kit/reddit.md`). Nothing in the app, the website or the docs says it. Owner to confirm (README, decision 5) |
| "Neon Rush ... stretched by 0.9375 (128 -> 120 BPM), pitch unchanged" | the agent's reply in the recording (BT 46 s); `demos.ts`: "Neon Rush is time-stretched to Midnight Drive's 120 BPM" |
| First downbeat sits half a second into the file (the agent flags it) | `demos.ts`: "Claude points out that its first downbeat sits half a second into the file and offers to nudge it"; the reply in the recording (BT 62 s) |
| Crossfade 16 to 32 s with a 2 kHz low-pass | the reply in the recording (BT 84 s: "fade-out 16 -> 32 s, plus a 2 kHz low-pass"); `demos.ts` |
| Mastered to -14 LUFS, peak -1 dBFS, a 50 s WAV | the export reply in the recording (BT 166 s: "50.1 s, stereo, 44.1 kHz, peak -1.0 dBFS, -14.0 LUFS"); `demos.ts` |
| A 150 Hz high-pass holds the low end until the drop | the reply in the recording (EI 64 s); `demos.ts` |
| Three-track mini-mix with 4-bar overlaps | `demos.ts` (mini-mix caption); the recording (MM 54 s) |
| Real audio, real time; waits are sped up | `demos.ts`: "Waits for Claude are sped up; every playback is in real time, with sound." |

Not said or shown anywhere: stem separation or transcription working, signed builds, user or download counts, testimonials, model names,
"live" or "real-time DJ" use.

---

## Master script (the LinkedIn cut, 69.0 s, 1080x1080)

Motion kit on every scene: kinetic titles (word by word, emphasized decelerate), a camera that eases between a wide shot and the part of
the screen that matters, callouts that draw themselves on, the orange progress line (see `research.md`, section M).

| Time (s) | Footage | On-screen text | Camera and motion | *Voice (optional)* | Audio |
|---|---|---|---|---|---|
| 0.0-3.4 | none: brand bars | **I built an audio editor you talk to.** | words rise and de-blur; breathing bars underneath | *I built an audio editor you talk to.* | soft whoosh |
| 3.4-9.9 | none | **One blend. Seven chores.** then the list: Find both tempos / Stretch one to match / Line up the downbeats / Draw the fades / Filter the outgoing track / Check the loudness / Export a file; then **Or just describe it.** | list items slide in 0.3 s apart; the last card is big and centred | *Prepping one blend is a pile of small chores. Or just describe it.* | whoosh x2 |
| 9.9-14.9 | BT 37.9-42.9: the request is typed and sent | **1 · Describe it in plain English**; chip "You type the edit" | wide shot eases into the composer (0.8 s); ring draws around the input | *You type the edit in plain English.* | tick |
| 14.6-19.0 | BT 45.6-50.0: the tool chip and the answer | **2 · The agent calls a tool**; chips "A real tool call", "128 to 120 BPM, pitch kept" | camera glides up to the chat's top; two rings | *The agent picks a tool, and runs it.* | ticks |
| 18.7-23.3 | BT 62.0-66.6: "One thing to check ..." | **3 · It flags what it noticed**; chip "It flags its own edge case" | camera settles on the paragraph | *It flags a downbeat half a second into the file.* | tick |
| 23.0-30.0 | BT 82.4-89.4: the blend request, three tool chips, the waveforms | **4 · Describe the blend**; chips "Three tool calls", "A 2 kHz low-pass", "Crossfade, 16 to 32 s" | chat -> chat top -> the two waveforms; the ring lands on the crossfade zone | *Then the blend: a crossfade, with a two kilohertz low-pass.* | ticks |
| 29.7-38.9 | BT 90.3-99.5: **the real playback** (sound from 92.3 s) | **5 · Hear it**; chips "Press play: real audio", "Heard in real time" | push-in to the crossfade; a thin ring follows the playhead | *Have a listen.* (before the music, never over it) | **real app audio** |
| 38.6-44.6 | BT 163.2-169.2: the export reply | **6 · Master and export**; chips "-14 LUFS, peak -1 dBFS", "Exports a 50 s WAV" | camera on the reply; two rings | *Mastered to minus fourteen loofs, exported as a WAV.* | ticks |
| 44.3-48.3 | EI 62.4-66.4 | **Also: hold the low end until the drop**; chip "A 150 Hz high-pass, as asked" | wide -> chat | *The same recordings show an extended intro, and a three-track mini-mix.* | tick |
| 48.0-52.0 | MM 55.4-59.4 | **And a three-track mini-mix**; chip "4-bar overlaps" | wide timeline, ring on the overlaps | (same line) | tick |
| 51.9-58.5 | none: labelled diagram | **93 tools. Run on your machine.** Your computer (Audio files stay here / 93 deterministic tools / Rust DSP engine) -> "chat text only" -> The provider you choose (Anthropic, OpenAI, OpenRouter, Groq, Gemini, Ollama (local)) | boxes rise, the arrow draws itself | *Ninety-three tools run on your machine. Only the chat goes to your provider.* | whoosh |
| 58.5-63.8 | none | **Honest limits**: Demo tracks are short synthetic loops / It preps files. It is not a live DJ tool / No stem separation or transcription yet / Installers are unsigned: expect a warning | rows slide in 0.32 s apart | *Limits: synthetic tracks, no stems, unsigned installers.* | whoosh |
| 63.8-69.0 | none: brand end card | **edytlab** (serif italic "edyt" + "lab" as in the app) / Free and open source (MIT) / macOS · Windows · Linux · v0.4.0 / github.com/laadtushar/edytlab / edytlab.com / Which prep job would you hand off first? | wordmark rises, lines stagger, URL pill springs in; the longest hold | *Free and open source. What would you hand off first?* | chime |

Captions: every title and list above is burned in (the cut is meant to be watched muted) and is also in `launch-linkedin.srt`.
Note under the frame all the way through the demo: "Real screen recording | waits for the agent are sped up".

---

## Cut 1. LinkedIn: `launch-linkedin.mp4` (1080x1080, 69.0 s)

Square because LinkedIn lists 1:1 as supported, most people watch on a phone, a square fills more of a phone feed than 16:9, and the
chat panel is near-square so the zoomed shots fill the frame (`research.md`, section A). Founder framing in the first frame ("I built ...")
because that is how the owner's post opens; the proof (real numbers, an edge case the agent flags itself, real audio) follows in the demo. Timing: the master script above, exactly.

## Cut 2. Reddit: `launch-reddit.mp4` (1920x1080, 63.4 s)

Understated: no founder hook, no chores montage, no exclamation, no hype word. A plain first line, then the real recording from 4 s.

| Time (s) | Footage | On-screen text | *Voice (optional)* |
|---|---|---|---|
| 0.0-4.2 | none | **edytlab: an open-source audio editor you drive in plain English** | *An open-source audio editor you drive in plain English.* |
| 4.0-9.0 | BT 37.9-42.9 | 1 · Describe it in plain English; chip "You type the edit" | *A DJ types the edit in plain English.* |
| 8.7-13.1 | BT 45.6-50.0 | 2 · The agent calls a tool; "A real tool call", "128 to 120 BPM, pitch kept" | *The agent calls a tool: a time stretch.* |
| 12.8-17.4 | BT 62.0-66.6 | 3 · It flags what it noticed; "It flags its own edge case" | *It points out a downbeat half a second into the file.* |
| 17.1-24.1 | BT 82.4-89.4 | 4 · Describe the blend; "Three tool calls", "A 2 kHz low-pass", "Crossfade, 16 to 32 s" | *Then the blend: a crossfade, with a low-pass on the outgoing track.* |
| 23.8-33.0 | BT 90.3-99.5 (real audio from 25.8 s) | 5 · Hear it; "Press play: real audio", "Heard in real time" | *Have a listen.* |
| 32.7-38.7 | BT 163.2-169.2 | 6 · Master and export; "-14 LUFS, peak -1 dBFS", "Exports a 50 s WAV" | *Mastered to minus fourteen loofs, exported as a WAV.* |
| 38.4-42.4 | EI 62.4-66.4 | Also: hold the low end until the drop | *Two more jobs from the same recordings: an extended intro, and a three-track mini-mix.* |
| 42.1-46.1 | MM 55.4-59.4 | And a three-track mini-mix | (same line) |
| 46.0-52.6 | none | 93 tools. Run on your machine. + the diagram | *The tools run locally, in Rust. Only the chat goes to your provider.* |
| 52.6-57.9 | none | Honest limits (4 rows) | *Limits: synthetic tracks, no stems, unsigned installers.* |
| 57.9-63.4 | none | edytlab / Free and open source (MIT) / macOS · Windows · Linux · v0.4.0 · installers are unsigned / **github.com/laadtushar/edytlab** / edytlab.com | *Source is on GitHub, under the MIT license.* |

16:9 because the recording is landscape and Reddit shows it full width. Reddit video posts have no body: paste the post text from
`marketing/social-kit/reddit.md` as the first comment. Re-read that subreddit's rules on the day (README, decision 6).

## Cut 3. Instagram Reel: `launch-instagram.mp4` (1080x1920, 31.4 s)

The hook is on screen from frame 3, the point lands in the first 3 s, and everything that matters sits inside x 65-1015, y 269-1248 (Meta's
published 14 % / 35 % / 6 % safe zone; it is an ad rule, the strictest published).

| Time (s) | Footage | On-screen text | *Voice (optional)* |
|---|---|---|---|
| 0.0-2.3 | none: brand bars | **Describe the blend. Hear the mix.** | *Describe the blend. Hear the mix.* (played 1.1x so it ends before step 1) |
| 2.2-6.6 | BT 38.2-42.6 | 1 · Say it in plain English; "You type the edit" | *Type it, in plain English.* |
| 6.3-10.0 | BT 45.6-49.3 | 2 · The agent calls a tool; "A real tool call" | *The agent runs a tool.* |
| 9.7-15.6 | BT 82.4-88.3 | 3 · Describe the blend; "Three tool calls", "A 2 kHz low-pass", "Crossfade, 16 to 32 s" | *Then ask for the blend: a crossfade, with a low-pass.* |
| 15.3-22.3 | BT 91.0-98.0 (real audio from 16.6 s) | 4 · Hear it; "Press play: real audio", "Heard in real time" | *Listen.* |
| 22.0-26.2 | BT 163.2-167.4 | 5 · Master and export; "-14 LUFS, peak -1 dBFS" | *Mastered, and exported as a WAV.* |
| 25.9-31.4 | none | edytlab / Free and open source / Unsigned installers · synthetic demo tracks / edytlab.com / macOS · Windows · Linux · v0.4.0 | *Free and open source. Unsigned installers.* |

No trending audio: the only sound is the app's own, so the Reel is "original audio" (`research.md`, section C).

## Posting copy

Use the existing, already-checked copy: `marketing/social-kit/linkedin-posts.md` post 1 (attach `launch-linkedin.mp4`, upload `launch-linkedin.srt`),
`marketing/social-kit/reddit.md` (first comment under `launch-reddit.mp4`), `marketing/social-kit/instagram.md` (caption for `launch-instagram.mp4`). The video
repeats their honest limits; the posts add nothing the video contradicts.
