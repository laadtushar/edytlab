/**
 * The four posts that went up with v0.3.0, kept apart from the older
 * ones in `blog.ts` because they were written against the code, not
 * against what the product was meant to become: every tool, shortcut
 * and menu named here was checked against `docs/tools-reference.md`
 * and `apps/desktop/src` when it was written.
 *
 * Rules these follow, which a new post should too:
 *
 * - Example numbers (tempos, cutoffs, lengths) are marked as examples,
 *   and no number is offered as a measurement.
 * - Nothing here describes stem separation or transcription as working.
 *   Neither has shipped; a workflow that would need them says so.
 * - A prompt block is a sentence a reader can type as it stands.
 *
 * Type-only import: `blog.ts` imports this file for its value, so the
 * reverse import must not exist at run time.
 */

import type { BlogPost } from "./blog";
import { siteConfig } from "./site";

const RELEASES = siteConfig.releases;

export const octoberPosts: BlogPost[] = [
  {
    slug: "beatmatch-and-blend-two-tracks",
    title: "Beatmatch and Blend Two Tracks by Asking",
    date: "2026-10-09",
    description:
      "Match tempos, overlap two tracks, fade and filter the outgoing one, then master and export a WAV: the DJ transition from the demo, as prompts you can type.",
    coverAlt:
      "Two waveforms on a shared beat grid, the second starting near the end of the first, with crossing fade ramps over the overlap",
    excerpt:
      "A DJ transition is a short list of exact operations. Here is the one in the demo video, step by step, with the sentence to type for each and the tool that runs it.",
    readTime: 5,
    tags: ["DJ transition", "beatmatching", "time stretch", "tutorial"],
    body: [
      {
        type: "p",
        text: "A smooth transition between two tracks sounds like one gesture, but it is a short list of exact operations: find both tempos, make them agree, line the beats up, overlap the ends, take something away from the outgoing track, and master the result. Each of those is a tool edytlab already has, so you can ask for them in plain sentences. This post follows the [beatmatch demo](/#demos) on the home page (2:58, shown at 1.6× speed) and gives you the sentence to type at every step.",
      },
      {
        type: "callout",
        text:
          "You need edytlab ([latest release](" +
          RELEASES +
          ")), two audio files and a language model set up in Settings. If you have not chosen a model yet, [Which LLM should drive your audio editor?](/blog/which-llm-should-drive-your-audio-editor) covers the options, and [Getting Started](/docs/getting-started) walks through the first launch.",
      },
      { type: "h2", text: "Set up: two tracks with names" },
      {
        type: "p",
        text: "Drag both files onto the timeline, or type `load /path/to/file.wav` for each. Then name them. The assistant can see every track's name, where its audio sits, its level and its effects, and “Outgoing” and “Incoming” are harder to mix up than track numbers.",
      },
      {
        type: "prompt",
        text: "Rename the first track Outgoing and the second one Incoming.",
      },
      { type: "h2", text: "Step 1: Ask for both tempos" },
      {
        type: "prompt",
        text: "What are the tempo, key and loudness of Outgoing and Incoming?",
      },
      {
        type: "p",
        text: "That runs `analyze_track` on each track. It returns the BPM, the key, a beat grid, downbeats, sections, a loudness curve and the integrated loudness in LUFS. The analysis is pure Rust on your machine, with no model to download. Treat the tempo as a strong hint rather than a verdict: tempo estimators can land on half or double time, so check it against what you hear and correct the agent if it is off.",
      },
      { type: "h2", text: "Step 2: Stretch the incoming track to match" },
      {
        type: "prompt",
        text: "Time-stretch Incoming so its tempo matches Outgoing, keeping the pitch.",
      },
      {
        type: "p",
        text: "`time_stretch` takes a factor, where 2.0 is twice as fast. To bring a 124 BPM track up to 128 BPM the factor is 128 ÷ 124, about 1.032. Those two tempos are an example; use whatever step 1 reported. The stretch is done by a phase vocoder, so the pitch stays where it was. That is what separates it from `change_speed`, which resamples, so pitch moves with speed.",
      },
      {
        type: "p",
        text: "Quality is the trade-off to know about. The tool's own notes say sustained material stays clean and attacks are preserved, but dense material can sound slightly phasey, and the further the factor is from 1.0, the more you hear it. A few BPM is routine. A large gap is a reason to choose a different pair of tracks, or a different section of the same one.",
      },
      {
        type: "p",
        text: "If a track drifts, as a live drummer does, rather than simply running at a different speed, there is also `align_to_beat`. It warps each stretch between beats onto a grid you give it, using the beat positions from `analyze_track`, again without changing pitch.",
      },
      { type: "h2", text: "Step 3: Start the incoming track 8 bars early" },
      {
        type: "p",
        text: "A bar of 4/4 lasts 4 × 60 ÷ BPM seconds, so at 128 BPM it is 1.875 seconds and eight bars are 15 seconds. If Outgoing is 200 seconds long (an example length), Incoming should start at 185.",
      },
      {
        type: "prompt",
        text: "Start Incoming 8 bars before Outgoing ends, with its first downbeat on one of Outgoing's downbeats.",
      },
      {
        type: "p",
        text: "The agent moves the whole track with `time_shift`, or a single clip with `move_clip`. Then listen. In the demo, Claude noticed that at the 16-second starting point the bars landed a beat apart, and said what it would change. Expect that kind of check from the assistant and make it yourself: whether the overlap works is a decision for your ears, and each move is one undoable step.",
      },
      {
        type: "h2",
        text: "Step 4: Crossfade, and take the top off the outgoing track",
      },
      {
        type: "prompt",
        text: "Fade Outgoing out and Incoming in across the 8-bar overlap.",
      },
      {
        type: "p",
        text: "`fade` applies a linear fade-in or fade-out over a time range, so a crossfade is two fades over the same stretch.",
      },
      {
        type: "prompt",
        text: "Put a low-pass filter on Outgoing across the overlap, cutoff 3 kHz.",
      },
      {
        type: "p",
        text: "Dulling the outgoing track leaves room for the top end of the incoming one. `low_pass_filter` takes a range and a cutoff, and the cutoff is fixed across the range rather than swept. 3 kHz is a starting point, not a rule. To choose by ear without committing to anything, audition it first:",
      },
      {
        type: "prompt",
        text: "Audition a 2 kHz low-pass on Outgoing across the overlap.",
      },
      {
        type: "p",
        text: "`audition_effect` renders a few seconds of the session with the effect added and plays it. It adds no node to the session, so there is nothing to undo. When you like what you hear, ask for the same effect with `add_effect` and it stays.",
      },
      {
        type: "h2",
        text: "Step 5: Mix down, compress, limit and match loudness",
      },
      {
        type: "prompt",
        text: "Mix Outgoing and Incoming down to a new track, then mute the two originals.",
      },
      {
        type: "p",
        text: "That is `mix_to_new_track` followed by `mute_track`. Muted tracks produce silence in the mix, so only the mixed track is heard from here on.",
      },
      {
        type: "prompt",
        text: "Compress the mix gently: ratio 2:1, threshold -18 dB, attack 30 ms, release 200 ms.",
      },
      {
        type: "prompt",
        text: "Limit the peaks at -1 dB, then normalize the loudness to -14 LUFS.",
      },
      {
        type: "p",
        text: "The compressor numbers are starting points. `compressor` needs a threshold, ratio, attack and release. `limiter` keeps every sample under the ceiling you give it by turning the gain down and letting it recover over `release_ms`, rather than clipping the peaks off. `normalize_loudness` sets the gain so integrated loudness reaches a target, and it has presets for delivery: −14 LUFS for Spotify and YouTube, −16 for Apple Podcasts, −23 for broadcast. If the gain needed would push peaks past the true-peak ceiling (−1 dBFS by default), it stops there instead of clipping and reports the shortfall, which is why the limiter comes first. That is the order in the demo: compressed, limited at −1 dB, brought to −14 LUFS.",
      },
      { type: "h2", text: "Step 6: Export" },
      {
        type: "prompt",
        text: "Export the mix as a WAV to /Users/me/Desktop/transition.wav.",
      },
      {
        type: "p",
        text: "`render_final` writes WAV, FLAC or MP3. Ask for FLAC if you want a smaller lossless file to send on.",
      },
      { type: "h2", text: "Keep your options open" },
      {
        type: "p",
        text: "Every step above is a node in the session graph, so none of it is destructive. If an 8-bar overlap feels long, step back to Step 3 and try four; the first attempt stays in the graph, ready to compare. [Undo is a graph](/blog/undo-is-a-graph-branches-and-ab-compare) shows how.",
      },
      {
        type: "callout",
        text: "What this does not do: it cannot lift the vocals out of one track to lay over the other, because stem separation has not shipped. The workflow works on whole tracks. The [tools reference](/docs/tools) lists everything the assistant can call.",
      },
      {
        type: "p",
        text:
          "To see the whole thing end to end, play the [demo](/#demos). To try it on your own tracks, [download edytlab](" +
          RELEASES +
          ") for macOS, Windows or Linux.",
      },
    ],
  },
  {
    slug: "make-a-mixable-extended-intro",
    title: "Make a Mixable Extended Intro",
    date: "2026-10-09",
    description:
      "Turn a track's drums-only opening into a longer, DJ-friendly intro: copy the bars, paste them back, high-pass, fade in and export. A prompt for every step.",
    coverAlt:
      "Four identical drum-bar blocks in a row, a rising fade-in ramp over the first two, and a loop arrow beneath them",
    excerpt:
      "Many dance tracks open with a few bars of drums and nothing else. Paste those bars back in at the start, thin them out and fade them in, and the track gains an intro a DJ can mix over.",
    readTime: 5,
    tags: ["extended intro", "DJ edit", "loop", "tutorial"],
    body: [
      {
        type: "p",
        text: "DJs like long, sparse intros. They give the previous track time to leave, and they make the start of a track easy to beat-match. Plenty of tracks already open with a few bars of drums and nothing else, but not many open with enough of them. If yours does, you can build a longer intro out of its own first bars, and in edytlab that is a short conversation. The [extended-intro demo](/#demos) on the home page (2:26, with sound) shows the whole thing on one track; this post goes through it a step at a time and takes it a little further.",
      },
      {
        type: "callout",
        text: "This works when the opening bars really are drums alone. Pulling the drums out of a full mix needs stem separation, which has not shipped, so edytlab cannot isolate them for you. It can repeat what is already there.",
      },
      { type: "h2", text: "What you are building" },
      {
        type: "p",
        text: "The plan is short: copy the drums-only bars to the clipboard, paste them back in at the start as many times as you like, thin the new intro out with a high-pass filter, fade it in, and export. Pasting is a splice. The audio after the insertion point shifts later by exactly the length you pasted, so the track starts later by a whole number of bars and the join stays on the grid.",
      },
      { type: "h2", text: "Step 1: Find the bars" },
      {
        type: "prompt",
        text: "Analyze the track: tempo, downbeats and sections. How long is a bar, and where do the drums-only bars end?",
      },
      {
        type: "p",
        text: "`analyze_track` returns the BPM, the downbeats and the sections. In 4/4 a bar lasts 4 × 60 ÷ BPM seconds. At 120 BPM that is 2 seconds, so eight bars are 16 seconds. Those are example numbers; use your track's. Section detection is a guide, not an oracle: play the opening and confirm by ear that nothing but drums happens in the bars you plan to loop. A loop that includes the first bass note repeats the bass note.",
      },
      { type: "h2", text: "Step 2: Copy the bars" },
      {
        type: "prompt",
        text: "Copy the first 8 bars of the track, from 0 to 16 seconds.",
      },
      {
        type: "p",
        text: "`copy_region` puts a time range of a track on an in-memory clipboard and leaves the session alone, so this step adds nothing to the graph. Cut on the bar line. A loop that starts or ends slightly off the downbeat will stumble every time it comes around, so ask the agent to take the bar boundaries from the analysis rather than from a rounded guess.",
      },
      { type: "h2", text: "Step 3: Paste them in at the start" },
      {
        type: "prompt",
        text: "Paste the copied bars at the very start of the track, three times.",
      },
      {
        type: "p",
        text: "`paste_region` takes an insertion point in seconds and splices the clipboard in there, shifting everything after it to the right. Pasting at 0 puts the new bars in front of the original opening. At 120 BPM each paste adds 16 seconds, so three pastes add 48 seconds and the intro runs 32 bars: 24 pasted ones, then the original eight. The demo pastes once, for a 16-bar intro. Three is a choice, not a rule.",
      },
      {
        type: "p",
        text: "Each paste is its own node in the session graph, so undo takes them off one at a time, and you can stop at whatever length sounds right. There is also `repeat_selection`, but it appends its copies to the end of a track's audio. That suits looping a tail, not lengthening an intro.",
      },
      { type: "h2", text: "Step 4: Thin the new intro out" },
      {
        type: "prompt",
        text: "High-pass the new intro, the first 24 bars, at 150 Hz.",
      },
      {
        type: "p",
        text: "Taking the low end out keeps the intro from piling bass and kick weight on top of the track you are mixing out of. Leaving the last eight bars, the track's own opening, full range brings the low end back as the cue that the track is about to start. `high_pass_filter` takes a cutoff and an optional start and end time, and the cutoff is fixed across that range. 150 Hz is a starting point. To find yours without committing, audition a few:",
      },
      {
        type: "prompt",
        text: "Audition a 200 Hz high-pass on the track from 0 to 10 seconds.",
      },
      {
        type: "p",
        text: "`audition_effect` plays a few seconds with the effect added and creates no node. Once you have a cutoff you like, ask for the real thing.",
      },
      { type: "h2", text: "Step 5: Fade it in" },
      {
        type: "prompt",
        text: "Fade the track in over the first 4 bars.",
      },
      {
        type: "p",
        text: "`fade` is linear, in or out, over a range you give it. Four bars at 120 BPM is 8 seconds. The fade is what lets the intro slide in under the end of the previous track instead of arriving as a hard edge.",
      },
      { type: "h2", text: "Step 6: Listen, then export" },
      {
        type: "p",
        text: "Play the start, then the join at the end of the new bars, then the drop. In the demo you hear the original intro first, then the new one, then the drop. If something is off, say what you hear in plain words (“the join clicks”, “the filter comes off too early”) and the agent will adjust the range or cutoff. When it sounds right:",
      },
      {
        type: "prompt",
        text: "Export the track as a WAV to /Users/me/Desktop/extended-intro.wav.",
      },
      {
        type: "p",
        text: "`render_final` also writes FLAC and MP3. A DJ edit usually keeps the level of the original, so there is no loudness step here; the [beatmatch walkthrough](/blog/beatmatch-and-blend-two-tracks) covers mastering for a mix.",
      },
      { type: "h2", text: "Try two lengths, keep both" },
      {
        type: "p",
        text: "You rarely know whether 16 or 32 bars is right until you hear it. Name the version you have (`name_node` labels a node in the graph, and right-clicking a node in the Graph tab renames it too), step back to Step 3, and paste a different number of times. Both versions stay in the graph, and you can switch between them with A/B compare. [Undo is a graph](/blog/undo-is-a-graph-branches-and-ab-compare) explains how.",
      },
      {
        type: "p",
        text:
          "Every tool named here is listed in the [tools reference](/docs/tools). To try this on your own tracks, [download the latest release](" +
          RELEASES +
          ") for macOS, Windows or Linux.",
      },
    ],
  },
  {
    slug: "which-llm-should-drive-your-audio-editor",
    title: "Which LLM Should Drive Your Audio Editor?",
    date: "2026-10-09",
    description:
      "edytlab works with six LLM providers, one of them local. How to weigh Anthropic, OpenRouter, OpenAI, Groq, Gemini and Ollama, and how to switch in Settings.",
    coverAlt:
      "A central agent node with six spokes to six provider nodes, one drawn as a laptop to mark the local option",
    excerpt:
      "edytlab does not ship a model. You pick one of six providers, and the choice shapes cost, speed, privacy and how well a long edit holds together. Here is how to think about it.",
    readTime: 5,
    tags: ["LLM providers", "Ollama", "bring your own key", "privacy"],
    body: [
      {
        type: "p",
        text: "The editing in edytlab is done by its own audio engine. The language model only decides which tools to call, and with what settings. That makes the model a swappable part, and the choice is yours: six providers, each with its own key, one of them running on your own computer.",
      },
      { type: "h2", text: "What the model does, and what it never sees" },
      {
        type: "p",
        text: "You type a sentence. The model reads it together with a description of your session (track names, where each clip sits, levels, pan, effects, the current version) and answers with tool calls such as `analyze_track`, `time_stretch` or `fade`. edytlab runs those on your machine and sends the results back. Your audio files are not uploaded to the provider. What does travel is text: your messages, the session description, and tool results like a tempo reading.",
      },
      {
        type: "p",
        text: "So the model matters for how well your sentence turns into the right sequence of tools, not for how the result sounds. A given tool call produces the same audio whichever model made it.",
      },
      { type: "h2", text: "The six providers" },
      {
        type: "ul",
        items: [
          "**Anthropic.** Claude models, reached with your own Anthropic key. Built for tool use, and a sensible default when a request has many steps.",
          "**OpenRouter.** One key that reaches many models from many labs. Useful for trying several without opening several accounts, or for routing by price.",
          "**OpenAI.** GPT-class models. The obvious choice if you already have an account and credit.",
          "**Groq.** Hosted open models, chosen for response speed.",
          "**Google Gemini.** Gemini models with a long context window.",
          "**Ollama (local).** Models running on your own machine through Ollama. No account and no key, and nothing leaves the computer, not even the chat.",
        ],
      },
      { type: "h2", text: "Trade-offs, without the benchmarks" },
      {
        type: "p",
        text: "Prices, speeds and model line-ups change often, and any figure printed here would be stale by the time you read it. So these are questions to put to your own sessions rather than a leaderboard.",
      },
      { type: "h3", text: "Capability on long chains" },
      {
        type: "p",
        text: "A transition like the one in [Beatmatch and blend two tracks](/blog/beatmatch-and-blend-two-tracks) is a dozen tool calls in the right order with the right numbers. Larger hosted models tend to hold a plan like that together. Smaller ones may do the first steps well and then lose the thread. If a model drops steps, split the job into shorter requests or try a bigger model. The plan-first option in the chat panel helps either way: the assistant shows its steps, you can edit them, and nothing runs until you approve.",
      },
      { type: "h3", text: "Cost" },
      {
        type: "p",
        text: "You pay your provider directly, per use. What you spend follows the conversation (your messages, the session description on each turn, the tool results), not the length of your audio. A smaller, cheaper model is often enough for plain edits like trimming and normalizing, and a stronger one earns its price on arrangements with many moving parts. Check your provider's pricing page for current numbers. Ollama has no per-use cost; you pay in hardware and waiting.",
      },
      { type: "h3", text: "Latency" },
      {
        type: "p",
        text: "Response time depends on the model, on the provider's load and, for Ollama, on your hardware. The audio processing itself runs locally, so once the model has answered, the edit happens at the speed of your machine. For quick back-and-forth on small tweaks, try Groq's hosted open models or a small local one. For a big plan, waiting for a stronger model can save you the retries.",
      },
      { type: "h3", text: "Privacy" },
      {
        type: "p",
        text: "With a hosted provider, your messages and session description go to that company under its terms, so read its data-retention policy if you work with client material. With Ollama they go nowhere. In neither case do your audio files leave your computer. Keys are stored in your operating system's keychain (macOS Keychain, Windows Credential Manager, or the Secret Service on Linux), never in plaintext on disk.",
      },
      { type: "h3", text: "Tool support in local models" },
      {
        type: "p",
        text: "Not every local model is trained to call tools. edytlab gives Ollama the same tools as any other provider, so pick a model trained for tool use. The **Test** button in Settings checks that the model you chose can call tools before you rely on it.",
      },
      { type: "h2", text: "How to switch" },
      {
        type: "ol",
        items: [
          "Open Settings with the gear icon (⚙) in the top-right corner.",
          "Pick a provider. Each provider has its own key slot, so switching does not overwrite the others.",
          "Paste the key. Ollama needs none.",
          "Optionally change the model name (every provider has a default) and the base URL, which lets you point a provider at a gateway, a proxy or a local server on another port.",
          "Press **Test**, then **Save & Continue**.",
        ],
      },
      {
        type: "p",
        text: "Your edits live in the project's session graph, not with the provider, so switching in the middle of a project loses nothing. Agent profiles go one step further and pin a model to a kind of job, such as a cheap one for cleanup and a stronger one for mastering. [Getting Started](/docs/getting-started) has the full first-launch steps.",
      },
      { type: "h2", text: "A fair way to choose" },
      {
        type: "p",
        text: "Run the same request under two models and listen. Because [every edit is a node in the session graph](/blog/undo-is-a-graph-branches-and-ab-compare), you can run it, undo, switch provider, run it again and A/B the two results. Use a job you actually do, and the answer will be about your material rather than somebody's chart.",
      },
      {
        type: "prompt",
        text: "Normalize track 1 to -14 LUFS, add a gentle high-pass at 80 Hz, and fade out the last four seconds.",
      },
      {
        type: "p",
        text:
          "A short, concrete request like that one is a good test: it has three tools in a fixed order, and any model that handles it has cleared the basic bar. The [Models section of the home page](/#providers) lists the six providers with links to their key pages, and [the latest release](" +
          RELEASES +
          ") has installers for macOS, Windows and Linux.",
      },
    ],
  },
  {
    slug: "undo-is-a-graph-branches-and-ab-compare",
    title: "Undo Is a Graph: Branches, A/B Compare and Never Losing a Take",
    date: "2026-10-09",
    description:
      "Every edit in edytlab is a node in a graph. How undo, branches and A/B compare work, and how history stays under 2 GiB without losing the takes you want.",
    coverAlt:
      "A graph of nodes branching from one trunk, with the current version highlighted and two nodes joined by an A and B marker",
    excerpt:
      "Most editors keep one undo stack and drop the future when you edit. edytlab keeps a graph, so a take you stepped back from is still there to hear, compare and return to.",
    readTime: 5,
    tags: ["undo", "session graph", "A/B compare", "non-destructive editing"],
    body: [
      {
        type: "p",
        text: "In a conventional editor, undo walks a stack, and the moment you make a new edit the steps ahead of you are gone. That is fine until the take you stepped back from was the better one. edytlab keeps your history as a graph instead, so stepping back and trying something else never costs you the thing you stepped back from.",
      },
      { type: "h2", text: "One edit, one node" },
      {
        type: "p",
        text: "Every change the assistant makes, whether a fade, a filter or a time-stretch, adds a node to the session graph, parented on the version you were on. Your original files are never modified. A node records the state of the session, and an edit that changes audio writes new audio next to the originals. A node's id is a hash of its state, so it names that state and nothing else.",
      },
      { type: "h2", text: "Undo and redo move a pointer" },
      {
        type: "p",
        text: "The version you are on is called the head. Undo (Ctrl+Z, or Cmd+Z on macOS) moves the head to the parent of the current node. Redo (Ctrl or Cmd+Shift+Z, or Ctrl or Cmd+Y) moves it forward again. Nothing is deleted either way; only the pointer moves.",
      },
      {
        type: "p",
        text: "The interesting part is what happens next. Make an edit after an undo and the new node hangs off the older one, which is a branch. The take you stepped back from is not discarded. It is a sibling, still in the graph.",
      },
      { type: "h2", text: "See the graph and name the good takes" },
      {
        type: "p",
        text: "Open the **Graph** tab to see every node. Click a node to look at that version. Right-click for **Set as head**, **Compare with…** and **Rename**. Delete is listed but disabled for now, because nodes are content-addressed and there is no safe way to remove one yet. Names are worth the few seconds: “before reverb” and “tighter fade” are easier to find than a hash. You can also ask the assistant to label a version:",
      },
      {
        type: "prompt",
        text: "Label the current version “drums only, 8 bars”.",
      },
      {
        type: "p",
        text: "That is `name_node`. It changes the label and nothing about the audio.",
      },
      { type: "h2", text: "Branch on purpose" },
      {
        type: "p",
        text: "Branching happens by accident after an undo, but you can also do it deliberately before a risky edit. `fork_node` branches the graph at a node you choose and makes it the head, so what you do next forms a new branch off it.",
      },
      {
        type: "prompt",
        text: "Go back to the version before the reverb and try a shorter tail on a new branch.",
      },
      {
        type: "p",
        text: "Two related tools are worth knowing. `revert_to` appends a new node whose state matches an earlier one, an “undo to checkpoint” that keeps the history in between. `apply_diff` writes several alternative takes as sibling nodes from one parent in a single step, when you want to hear a few variations side by side.",
      },
      { type: "h2", text: "A/B compare" },
      {
        type: "p",
        text: "To choose between two versions, compare them. Right-click a node and pick **Compare with…**, or ask:",
      },
      {
        type: "prompt",
        text: "Compare the current version with the one before the reverb.",
      },
      {
        type: "p",
        text: "The compare bar appears with **A**, the version you are on, and **B**, the one you picked. edytlab renders both sides first, so switching is gapless. As of v0.3.0 switching crossfades from the same moment instead of cutting, and playback keeps its position, so you hear the same bar both ways. **Accept B** makes B the current version. Closing the bar keeps A.",
      },
      {
        type: "p",
        text: "If you want to know what changed between two versions rather than how they sound, ask for it. `compare_nodes` returns what was added, removed and modified between two nodes. For the narrower question of whether to keep one effect, `audition_effect` plays a few seconds with it added and creates no node at all.",
      },
      { type: "h2", text: "What history costs, and the 2 GiB limit" },
      {
        type: "p",
        text: "Edits that change audio write a new file, so a long session would grow without bound if nothing cleaned up. edytlab keeps a project's derived audio under 2 GiB. Past that, audio that only older versions use is removed, oldest first, and only when the app can rebuild it by replaying the edits that made it. A removed version is rebuilt on demand when you undo to it, preview it or export it, which can take a moment. The code's own estimate is that the cap holds about 36 edits of five-minute stereo audio.",
      },
      {
        type: "p",
        text: "Audio the current version needs is never touched, and neither is audio nothing can rebuild. `storage_report` shows where the space goes, split three ways: what the current version needs, what only undo history holds, and what nothing refers to. `compact_session` is the blunt tool: it prunes old history and deletes the audio only that history used, which drops the undo steps permanently. Ask for the report first.",
      },
      {
        type: "callout",
        text: "The 2 GiB limit is described in the v0.3.0 entry of the [changelog](/changelog). The [user guide](/docs/user-guide) covers the graph, A/B compare and export.",
      },
      { type: "h2", text: "Exporting from any version" },
      {
        type: "p",
        text: "Exports render a node, not just the latest state, so an earlier take is as exportable as the current one. Right-click it in the Graph tab and choose **Set as head**, then ask:",
      },
      {
        type: "prompt",
        text: "Export this version as FLAC to /Users/me/Desktop/take-b.flac.",
      },
      {
        type: "p",
        text: "The session graph is not changed by an export, so you can export several versions from different nodes without redoing any work.",
      },
      { type: "h2", text: "Four habits that pay off" },
      {
        type: "ol",
        items: [
          "Name a version before you try something drastic.",
          "Branch rather than overwrite: step back and try again, and let both stay.",
          "Compare before you accept. The ear catches what the graph cannot.",
          "On very long sessions, ask for a storage report before you go looking for disk space.",
        ],
      },
      {
        type: "p",
        text:
          "The workflows in [Beatmatch and blend two tracks](/blog/beatmatch-and-blend-two-tracks) and [Make a mixable extended intro](/blog/make-a-mixable-extended-intro) both lean on this: try the overlap at eight bars, then four, and keep whichever you prefer. To use it yourself, [download edytlab](" +
          RELEASES +
          ") for macOS, Windows or Linux.",
      },
    ],
  },
];
