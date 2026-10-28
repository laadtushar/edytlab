# Reddit posts

Seven posts, one per community. They are written to be different on purpose:
each one talks the way that community talks and leaves the project for second.
Do not cross-post one text. Read [`../brand-voice.md`](../brand-voice.md)
before you edit any of them. For r/WeAreTheMusicMakers, r/DJs, r/LocalLLaMA
and r/rust, `marketing/social-kit/reddit.md` has newer text; use that, not
this.

**Links to use in every post:**
Website <https://edytlab.com> ·
Download <https://github.com/laadtushar/edytlab/releases/latest> ·
Source <https://github.com/laadtushar/edytlab>

**Before each post:**

1. Re-read that sub's sidebar and rules on the day. Their treatment of
   self-promotion was not checked from this environment and changes without
   notice. If the rules say promotion goes in a weekly or flaired thread, post
   there.
2. Say you made it, in the first sentence.
3. Put the link where the rules allow (post body or first comment). If links
   are banned in posts, drop it in a comment only when asked.
4. Do not mention stem separation as a feature anywhere. It does not work.
   Transcription works from v0.8.0 after the user downloads the speech model;
   say that, and say what it was checked on (see `marketing/README.md`).
5. Stay for the replies. A drive-by post reads as spam.
6. If the sub asks whether AI helped build the project, say yes. It did: the
   repo is developed with Claude Code.

---

## 1. r/WeAreTheMusicMakers

**Audience:** home producers and songwriters. Wary of "AI music" and of
promotion. edytlab does not generate music, so lead with that.

**Flair:** whichever fits "Software/Tools" or "Feedback"; if the sub confines
self-promotion to a set thread, use that thread.

**Title:**
`I made a free audio editor where you describe the edit instead of hunting through menus. It doesn't make music, and I'd love feedback`

**Body:**

> Disclosure: I made this. It's free and MIT-licensed, no account, no
> subscription.
>
> You've got a loop that's almost right. It needs to be at 124 BPM, a couple of
> semitones lower, with a short reverb and a fade at the end. That's four trips
> through menus for something you can say in one breath.
>
> So I built an editor where you say it. Type "time-stretch the loop to
> 124 BPM, pitch it down two semitones, add a short reverb, fade the last two
> seconds" and it does each step on your file and shows you what it did. If you
> don't like the result you can go back, or keep both takes and compare them.
>
> To be clear, it doesn't generate music, and it isn't a DAW. No MIDI, no
> plugins. It's for the chores around a track. The processing happens on your
> computer, and your audio isn't uploaded. Only the chat goes out, to the AI you
> pick, or nowhere if you run a model locally with Ollama.
>
> One thing to know: the time-stretch is a phase vocoder I wrote, so big
> changes on dense material sound smeared.
>
> What I want to know: when you stop in the middle of a session to do a boring
> audio job, what is it? That list is my roadmap.
>
> Site and demo: https://edytlab.com
> Download: https://github.com/laadtushar/edytlab/releases/latest

---

## 2. r/DJs

**Audience:** bedroom and working DJs. Practical, allergic to hype. Make sure
nobody thinks this runs live.

**Title:**
`I built a free tool for prepping blends: you tell it the transition, it matches the tempos, crossfades and exports (demo inside)`

**Body:**

> Disclosure: I'm the developer. It's free and open source.
>
> Prepping a blend is a pile of small chores: find the tempos, stretch the
> incoming track, line it up, draw the fades, check the level, bounce a file.
> I got tired of them, so I made something that does them when I describe the
> transition.
>
> In the recording on the site, with sound, I ask for exactly that: get both
> tracks' tempos, time-stretch the incoming one to match, start it 8 bars
> before the outgoing one ends, crossfade, filter the outgoing track over the
> overlap, and export a WAV loud enough for streaming. Each step shows up in
> the chat. If I want to try it with the incoming track starting 4 bars
> earlier, I ask, then compare the two takes and keep the one I like.
>
> This is for prep, not for playing live. It edits files: no decks, and
> nothing for rekordbox or Serato. Beat detection is software, so on a track
> with a drifting tempo or a long ambient intro it can be off. Check a blend by
> ear.
>
> Demo: https://edytlab.com/#demos
> The installers are unsigned, so you'll get a warning the first time you open
> it on Mac or Windows.
> Download: https://github.com/laadtushar/edytlab/releases/latest
>
> If you prep blends, what's the part of the process you'd most like to skip?

---

## 3. r/audioengineering

**Audience:** working engineers; hostile to shortcuts and to promotion. Many
posts here are questions. If self-promotion is not permitted, **do not post
this**; instead, answer relevant threads honestly and mention the project only
if asked. The post below is written as a discussion with disclosure, so it
holds up only if the rules allow it.

**Title:**
`I built an editor where an LLM only chooses and orders deterministic DSP tools and never touches samples. Where would you not trust it?`

**Body:**

> Disclosure: this is my own open-source project, MIT-licensed, linked at the bottom.
> I'm after criticism of the approach, not downloads.
>
> The idea is simple. The model gets a text description of the session: tracks,
> levels, effects, and measurements like tempo, key, beat grid and EBU R128
> loudness. It also gets a list of 99 tools. It picks and orders the tools, and
> the tools do the audio work, in Rust, on your machine. The same tool call on
> the same file gives the same bytes. The model's choice of calls is not
> deterministic, but the processing is, and the model never hears or sees a
> sample.
>
> I already know where it's weak. The limiter has no look-ahead and isn't a
> true-peak limiter. Time-stretch and pitch-shift are a phase vocoder, which
> is fine on sustained material and phasey on dense mixes. Noise reduction is
> plain spectral subtraction. And the model picks settings from a description,
> not from listening, which is why there's an optional plan-first mode and an
> audible A/B at each step.
>
> So the questions are for you. Which of those would make you refuse to use it
> on paid work? Which measurements should it hand the model that it doesn't?
> What would you want logged so an edit is reproducible?
>
> Source: https://github.com/laadtushar/edytlab. The tool list is in
> docs/tools-reference.md.

---

## 4. r/podcasting

**Audience:** hosts and editors; many have been burned by "AI podcast
editors". **Be explicit about what transcription was checked on**, because
that is what the category promises.

**Title:**
`I made a free, open-source editor for the boring cleanup passes on an episode: silences, noise, levels, loudness. Feedback welcome`

**Body:**

> Disclosure: I made this. Free, MIT, runs on your computer.
>
> After you stop recording, there's a pass of chores that isn't editing so much
> as housekeeping. Cut the long silences, take the hiss out, tame the harsh
> "s" sounds, even out the loud and quiet bits, high-pass the rumble, and bring
> the export to a podcast loudness target. I built an editor where you describe
> those passes in a chat box and it runs them one step at a time. After each one
> you can listen to it against the version before and back out.
>
> It can also transcribe, on your computer, once you press Download in Settings
> for the speech model, about 139 MB. Nothing downloads by itself. Then you can
> ask for the words with their times, cut a passage by its words, or have the
> "um"s found. It reports first and changes nothing until you say so. Your audio
> never leaves your machine, but the words go to your AI provider as text,
> unless you use a local model.
>
> I should be straight about what I've checked it on: one short synthetic
> English recording and about 16 minutes of English audiobook speech. Not real
> episodes, other accents or noisy rooms.
>
> Noise reduction here is basic spectral subtraction, so a very noisy recording
> will do better in a specialised denoiser. What I'd like to hear from you is
> what's in your standard cleanup chain, and in what order.
>
> https://edytlab.com
> The installers are unsigned, so expect a first-launch warning.
> Download: https://github.com/laadtushar/edytlab/releases/latest

---

## 5. r/LocalLLaMA

**Audience:** people running models locally; care about context size, tool
calling, and quantisation. They want real numbers, not slogans. Do not claim
any model works unless you have tried it.

**Title:**
`Open-source desktop app where a local model (Ollama) edits audio by calling 99 tools. Which tool-calling models can handle it?`

**Body:**

> Disclosure: my project, MIT-licensed.
>
> edytlab is a desktop audio editor you drive by chatting. The model chooses
> and orders calls to 99 audio tools, and the DSP runs in Rust on your machine.
> Ollama is one of the AI providers you can pick. It needs no key, so the chat
> stays on your computer too, and the Test button in Settings checks that the
> model can call tools.
>
> I'll tell you what I've actually measured, which isn't much. In an early
> version a one-line request came to a first request of about 15,000 tokens,
> the system prompt plus every tool schema. With llama.cpp's llama-server and
> Qwen2.5-3B at an 8,192 context, that request was refused outright. The numbers
> are in https://github.com/laadtushar/edytlab/issues/395.
>
> The fix was a smaller list for Ollama. It gets a core set of 15 tools plus the
> ones your message names, with shortened descriptions, and the names of the
> rest go in one line of the system prompt. The repo's request-size test puts
> the one-line request at about 8.7 KB of text, against about 67 KB with every
> tool. That's bytes, not tokens, and I haven't re-run Qwen2.5-3B on the new
> request.
>
> I haven't benchmarked models against it, and I'd rather ask than guess. If
> you try it, tell me which model and quantisation, what context setting, and
> whether it kept to the tool schemas. A tool is sent in full only when the
> message names it or it's in the core, so a request described by an effect, like
> "make it sound like a cathedral", may not carry the reverb schema. I don't know
> how often that matters. Failures are as useful to me as successes.
>
> Source: https://github.com/laadtushar/edytlab
> Download, unsigned: https://github.com/laadtushar/edytlab/releases/latest
> Site: https://edytlab.com

---

## 6. r/rust

**Audience:** Rust programmers. Value-first means the engineering. Check
whether the sub has a project-showcase thread or specific rules on AI-assisted
code, and follow them. Answer the "was this AI-generated" question with the
truth.

**Title:**
`edytlab: a Tauri 2 audio editor with a dependency-free DSP crate and a hand-written phase vocoder on realfft`

**Body:**

> Disclosure: my project, MIT. The repo is developed with Claude Code; the
> commit trailers say so.
>
> This is the Rust side of a desktop audio editor that an LLM agent drives
> through 99 tools. I'd like review more than praise, so here is how it's put
> together.
>
> The DSP lives in `audio-dsp`, which has no dependencies on purpose. It's
> functions over `&mut [f32]` plus a sample rate and channel count, and it sits
> under both the tools and the renderer, so a dependency there would be a
> dependency everywhere. Around it, decoding is `symphonia`, resampling is
> `rubato`, FFTs are `realfft` and microphone capture is `cpal`.
>
> I wanted Rubber Band for time-stretching and dropped it. It needs
> `librubberband-dev` on Linux, vcpkg on Windows and Homebrew on macOS, and a
> native dependency like that breaks all three CI targets at once. So the
> time-stretch and pitch-shift are a phase vocoder I wrote, with peak-based
> phase locking and onset-triggered phase resets. The module docs in
> `crates/audio-time/src/vocoder.rs` write down what that costs.
>
> Session state is a content-addressed DAG, where a node's id is the blake3 of
> its state. That had a sharp edge for undo, #398: a revisited state keeps its
> first parent, so undo went to the wrong node. The fix was to make undo and
> redo follow the path the user took, and I'd still like to hear how you'd
> model it.
>
> Two things I'd particularly like looked at: the vocoder's transient handling,
> and the lock discipline in the agent loop. Long tools used to hold the store
> lock and freeze the window, #421. Now a tool that opts in runs without the
> lock and commits only if the head hasn't moved, and runs again if it has.
>
> Stem separation is a stub, not built.
>
> Source: https://github.com/laadtushar/edytlab
> Site and demo: https://edytlab.com

---

## 7. r/tauri

**Audience:** Tauri developers. Show what you learned shipping a real app on
all three platforms. Only claim things you hit.

**Title:**
`Shipped a Tauri 2 audio editor on macOS, Windows and Linux. Notes on asset scope, main-thread commands and Linux keyrings`

**Body:**

> Disclosure: it's my open-source project, edytlab, MIT-licensed, a desktop audio
> editor driven by an LLM agent. The repo is developed with Claude Code.
>
> Three things I tripped over getting it onto all three platforms. They might
> save someone an afternoon.
>
> The first was the asset protocol scope. I narrowed it in v0.2.0, but the
> rendered mix and previews lived in a hidden folder, so playback returned 403
> on Linux and macOS. Windows was fine. I couldn't see it until a native run on
> Linux. Fixed in v0.3.0.
>
> The second was synchronous commands on the main thread. A few of them took
> the same store lock that a long-running edit held, so the whole window froze
> during the edit, #421. Moving them off the main thread fixed it, and now long
> tools run without holding the lock at all. The lesson I took: don't take a
> lock inside a sync command if a background worker might be holding it.
>
> The third was the keyring on Linux. The `linux-native` backend is the kernel
> keyring, which lives in memory and is gone after a reboot. That's what early
> versions used, #394. Now the app stores credentials in the Secret Service and
> warns when there isn't one. I haven't checked that across a real reboot yet,
> so if you ship credentials on Linux, read the backend's docs before you rely
> on it.
>
> Happy to go into any of these. The one thing I haven't solved is signing on a
> budget: the installers are unsigned, with a README section on the macOS
> quarantine flag. If you've done that for an open-source Tauri app, I'd like
> to hear how.
>
> https://github.com/laadtushar/edytlab · https://edytlab.com
