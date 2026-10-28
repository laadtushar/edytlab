# edytlab brand voice

Read this before writing or editing any copy under `marketing/`. It is how
the posts should sound. The accuracy rules in [`README.md`](README.md) say
what they may claim; this file does not change them.

Built with Anthropic's marketing skills (`content-creation`, `draft-content`)
and the brand-voice plugin's "voice constant, tone flexes" model
(`anthropics/knowledge-work-plugins`).

## The voice in one paragraph

A person who built something useful, showing it to people who would use it.
Plain words, short sentences, one good example instead of a feature list. Sure
of what it does and relaxed about what it doesn't do yet. It sounds like
someone talking across a desk, not a spec sheet, a changelog or a legal notice.

## We are / we are not

| We are | We are not |
|---|---|
| **Concrete.** One real job, told well: "match a 128 BPM track to a 120 BPM one and bring it in under the last 8 bars." | **A spec sheet.** Lists of every provider, format, tool count, model size and version in one post. |
| **Plain-spoken.** Everyday words, short sentences, one idea per sentence. Read it aloud and it sounds like a person. | **Mechanical.** Parentheses inside sentences, semicolon chains, "which", "so that" clauses stacked three deep. |
| **Honest, lightly.** The one limit that matters to this reader, said once, in passing. | **Defensive.** "Honest limits:", "so you don't have to find out", a paragraph of caveats in every post. |
| **Confident.** Say what it does. Let the demo prove it. | **Hype.** "Revolutionary", "game-changer", "AI-powered", superlatives, exclamation marks. |
| **Personal.** "I built", "what I care about most". A solo maker, open source. | **Corporate.** "We are excited to announce", "leverage", "solution", "empower". |

## What every post does

1. **Open with the reader's moment, not the product.** The job they hate, the
   thing they typed, the result they got. "I built X" can come second.
2. **One message per post.** Pick one pillar below. Everything else goes to
   the link.
3. **Show one example.** Specific beats complete. Two numbers at most.
4. **One limit, if any.** The one this reader would trip on. Short posts can
   skip limits that the download page already states (unsigned installers),
   unless the post asks them to download.
5. **End with one thing to do.** A question, or the link. Not both plus a
   pinned comment plus a second link.

## Message pillars

Use one per post.

1. **Say it, it happens.** Describe the edit in your own words; the editor does
   it and shows each step. ("It shows its work.")
2. **The AI never touches your audio.** The model reads a summary and picks
   tools; the editing runs on your computer. Your files aren't uploaded.
3. **Nothing is lost.** Every edit is undoable; the session is a history you
   can branch, compare and go back through. "Plan first" lets you approve
   steps before they run.
4. **Free and open.** MIT licence, macOS, Windows and Linux, your choice of AI,
   including a local model.
5. **The job it saves.** For a DJ: beatmatching and transitions. For a
   podcaster: long silences, speakers on separate tracks. For a narrator:
   checking loudness against ACX. Talk to one of them at a time.

## Words

| Say | Instead of |
|---|---|
| audio editor you talk to / edit by typing what you want | "chat-driven desktop audio workstation" |
| the AI / the model | "the agent" (except in technical posts) |
| your choice of AI, including a local one | the list of seven providers (list them only in posts about providers) |
| on your computer | "on-device", "locally processed" |
| editing tools | "DSP tools", "tool calls" (fine in Show HN and dev posts) |
| shows each step | "every edit is a tool call you can read" |

Never: emoji, "AI-powered", "revolutionary", "seamless", "unlock",
"supercharge", "game-changer", "we're excited". No more than three hashtags.

## Tone by channel

| Channel | Formality | Energy | Technical depth | Principle |
|---|---|---|---|---|
| X | low | high | low | One idea a post, the hook in the first line |
| LinkedIn | medium | medium | low | A short story: the job, what happened, why it matters |
| Instagram / reels | low | high | low | Visual first; the caption names the moment |
| Reddit | low | low | medium | A peer in the subreddit's own terms; ask, don't pitch |
| Show HN / dev | medium | low | high | How it works, what was hard, what's missing |
| Press kit | medium | low | medium | Facts in full sentences, quotable lines |

Technical channels (Show HN, r/rust, dev posts) can name the tools, the Rust
DSP and the provider list: that audience wants them. Everyone else gets the
outcome.

## Before and after

**Before** (X, v0.9.3):

> Here a DJ asks for two tracks' tempos, the incoming one is time-stretched to
> match, crossfaded in 8 bars before the first ends, and the mix is exported as
> a WAV at -14 LUFS.

**After:**

> I asked an audio editor to match a 128 BPM track to a 120 BPM one, slide it
> in under the last 8 bars and master the mix for streaming.
>
> It did. And it showed every step.

What changed: the reader's moment first, one sentence per idea, "for
streaming" for "-14 LUFS", and the result as a short line of its own.

**Before** (LinkedIn):

> Honest limits: the demo tracks are short synthetic loops I generated, and the
> agent in the recordings is Claude. It preps files; it isn't a live DJ tool. No
> stem separation yet. The installers are unsigned, so macOS and Windows warn on
> first launch.

**After:**

> It's for prep, not for playing live.

What changed: one limit, the one a DJ would trip on. The rest is on the
download page.

## Check before you post

- Read it aloud. Anything you would not say to a person goes.
- Count the ideas. More than one per post: cut or split.
- Count the parentheses. More than one: rewrite.
- Every claim still passes the accuracy rules in `README.md`.
