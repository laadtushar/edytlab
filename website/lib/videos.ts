/**
 * The videos on `/videos`: the v0.7.0 video, the launch video and eight
 * short vertical reels, which are copies of files under `marketing/`
 * (`videos`, below), and the three full demos, which are `lib/demos.ts`
 * and play from `public/demos/` (`demoVideos`). Every one of them has a
 * page of its own, `/videos/<slug>`, where it is the main content
 * (`watchVideos`); the demos also play on the home page and the DJ page,
 * without any structured data there.
 *
 * Each is a copy of a file under `marketing/`, where it is cut and kept,
 * placed in `public/videos/` so the site can serve it:
 *
 * - `<slug>.mp4`: byte-identical to `source`;
 * - `<slug>.jpg`: the poster, a frame of the video at `posterAt` seconds
 *   (`ffmpeg -ss <posterAt> -i <source> -frames:v 1 -vf scale=<w>:-2 -q:v 4`),
 *   shown until the reader presses play. With `preload="none"` it is the
 *   only part of a video a visitor downloads unasked. It is a frame of
 *   the app and carries no logo and no other product's imagery;
 * - `<slug>.vtt`: for a 16:9 video, its `.srt` converted to WebVTT
 *   (`lib/srt-to-vtt.ts`).
 *
 * `videos.test.ts` fails when a copy differs from its source (so a re-cut
 * in `marketing/` cannot leave the site stale), when the list and the
 * folder drift apart, and when a running time or size here is not the
 * file's own.
 *
 * Only the plain files are listed. The narrated cuts, `marketing/reels/audio/`,
 * `sfx/` and `marketing/archive/` carry ElevenLabs free-plan audio, which
 * is not licensed for commercial use. A reel here has the app's own sound
 * and nothing else. The v0.7.0 videos come from `marketing/promo-v0.7.0/out/`
 * (no voice, no stock music); the 1:1 cut is the 16:9 edit again, for
 * LinkedIn, and is not listed.
 *
 * The v0.7.0 videos are the released app, filmed live, with real Claude
 * as the assistant where there is one, and say so on every shot. Their
 * captions and descriptions here say it too, and claim no more than
 * `marketing/promo-v0.7.0/claims.md` allows.
 *
 * A caption says what the video shows and nothing more, like the demos':
 * nothing that has not shipped, no counts, no ratings, no superlatives.
 */

import { DEMO_FRAME, demoPosterSrc, demoVideoSrc, demos } from "./demos";

/** What every video's own page needs, whichever folder its files are served from. */
export interface WatchVideo {
  /** Kebab-case. Names the files, and the page `/videos/<slug>`. */
  slug: string;
  /**
   * The launch video and the release video are 16:9; a reel is 9:16; a
   * demo is a 1280 by 776 screen recording that plays from `public/demos/`.
   */
  kind: "launch" | "release" | "reel" | "demo";
  title: string;
  /** What it shows, for the page. */
  caption: string;
  /**
   * One line for a card and for the top of a page: what happens, in the
   * words a visitor would use. No length ("a 21-second video of"), no
   * "real app" notes: those are `seconds` and `disclosure`.
   */
  blurb: string;
  /**
   * The one thing a viewer should know about how the video was made,
   * shown in small type wherever the video is (its card, its page): for
   * example "Real app, real Claude (Sonnet 5.5)." A re-recording changes this
   * line, not the layout that shows it.
   */
  disclosure: string;
  /** One sentence for the video's structured data, which a search result may show. */
  description: string;
  /** Running time in whole seconds (`ffprobe`, rounded). The test reads the file's own header. */
  seconds: number;
  /**
   * The day the video's file was first committed (`git log --diff-filter=A
   * --format=%cs -- <the .mp4>`). Each has been re-cut or re-filmed since,
   * under the same name.
   */
  uploadDate: string;
  /** The picture's size in pixels, as the file states it; the player's box is reserved from it. */
  width: number;
  height: number;
  /** For a 16:9 video: the `.srt` its WebVTT captions were converted from. */
  captionsSource?: string;
  /**
   * What happens, in order, for the watch page: moments of the video,
   * each at the second a cue of `momentsSource` starts. `momentsSource`
   * is the cue file the moments follow: a reel's `reel-*-cues.json` or a
   * demo's `demo-*-cues.json`, whose `onscreen` words the moments are, as
   * written, one for each cue; or an `.srt`, whose cues are the burned-in
   * words, which a moment describes in plain words (one moment may stand
   * for several cues, and the launch video's cues carry a count that the
   * page's copy leaves out). There is no voice-over in any of them.
   * `videos.test.ts` holds the times, and the words of a cue file's
   * moments, to the source, so a re-cut cannot leave this stale.
   */
  moments: readonly Moment[];
  momentsSource: string;
}

/**
 * A video that is a copy of a file under `marketing/`, placed in
 * `public/videos/`: the launch video, the release video and the reels.
 */
export interface SiteVideo extends WatchVideo {
  kind: "launch" | "release" | "reel";
  /** The file this is a copy of, from the repository root. */
  source: string;
  /** Where in the video, in seconds, the poster frame was taken. */
  posterAt: number;
}

/** One of the three full demos, played from `public/demos/` (no copy). */
export interface DemoVideo extends WatchVideo {
  kind: "demo";
}

export interface Moment {
  /** Seconds from the start of the video. */
  at: number;
  text: string;
}

export const videos: readonly SiteVideo[] = [
  {
    slug: "whats-new-v070",
    kind: "release",
    title: "What's new in edytlab v0.7.0",
    blurb: "From a typed request to a 200-track session: what changed in v0.7.0, shown in the app.",
    disclosure: "Real app, real Claude (Sonnet 5.5). Typing and waits sped up where labelled.",
    caption:
      "What's new in v0.7.0, shown in the real app, filmed live. It starts with a request typed in plain English that Claude turns into a fade and a gain change, which you then hear. Then six changes: faders that stay where you put them (#596); the history graph keeping up as a version is named and the history compacted, each by asking (#602); a 200-track session where list_session reads what the summary leaves out and twelve string tracks come down 6 dB (#618); the ONNX Runtime now shipping in every installer, as groundwork only, with no model yet; bundled skills that update unless you edited them (#574); and playback that survives switching tabs (#607). Real app, real Claude (Sonnet 5.5): every shot is the released v0.7.0 app recorded live, the assistant's replies and tool calls are the model's, and the demo audio is made in code, as the note on every shot says. The installers are unsigned. Captions are available.",
    description:
      "A 76-second video of what's new in edytlab v0.7.0, filmed live in the real app with Claude: a typed edit, steady faders, the history graph, 200 tracks.",
    seconds: 76,
    uploadDate: "2026-10-22",
    width: 1920,
    height: 1080,
    source: "marketing/promo-v0.7.0/out/edytlab-v0.7.0-16x9.mp4",
    posterAt: 10,
    captionsSource: "marketing/promo-v0.7.0/out/edytlab-v0.7.0-16x9.srt",
    momentsSource: "marketing/promo-v0.7.0/out/edytlab-v0.7.0-16x9.srt",
    moments: [
      { at: 0, text: "Title: edytlab v0.7.0, “Describe the edit. It happens.” Every app shot carries a note: “v0.7.0 · real app · real Claude (Sonnet 5.5)” where the assistant is in the shot, “recorded live” where it is not." },
      { at: 3.6, text: "Describe the edit in plain English: the request is typed into the chat box (shown at 2.2× speed)." },
      { at: 6.8, text: "The assistant calls the tools, fade and then gain, and the timeline updates: the backing fades in over 4 seconds and the vocal lane reads −3.0 dB (the wait for Claude shown at 2.5× speed)." },
      { at: 13.282, text: "Press play and hear it; the demo audio is made in code." },
      { at: 21.667, text: "Faders that stay where you put them (#596): four key taps on a gain slider step it +0.5, +1.0, +1.5 and +2.0 dB, with no bounce back." },
      { at: 25.967, text: "Every edit is a node in the history graph." },
      { at: 29.667, text: "The history graph keeps up (#602): asking Claude to name a version shows at once, and asking it to compact the history removes nodes at once, 20 nodes becoming 8 (shown at 2× and 1.6× speed)." },
      { at: 36.633, text: "200 tracks? Just ask (#618): list_session reads what the summary left out (shown at up to 2.2× speed), then twelve string tracks sit at −6.0 dB." },
      { at: 48.267, text: "The ONNX Runtime now ships in every installer: groundwork only, no model yet." },
      { at: 54.167, text: "Bundled skills update when a release changes them, unless you edited them (#574)." },
      { at: 58.667, text: "Switch tabs, press play: it still plays, with no error (#607)." },
      { at: 71.467, text: "End card: edytlab v0.7.0, edytlab.com, free and open source (MIT), for macOS, Windows and Linux. The installers are unsigned." },
    ],
  },
  {
    slug: "launch-video",
    kind: "launch",
    title: "edytlab in a minute: from request to exported mix",
    blurb: "Describe a blend in plain English, hear the transition, then master and export the mix.",
    disclosure: "Real screen recordings of the app on short synthetic demo tracks. Waits sped up.",
    caption:
      "One DJ job from request to exported file, cut from the real recordings of the desktop app: the request is typed in plain English, the agent calls its tools and points out what it noticed, you hear the blend, and the mix is mastered and exported as a WAV. Two more jobs follow, then how it works (the audio stays on your machine, only the chat goes to the provider you choose) and an end card: free and open source, with the installers unsigned. Captions are available.",
    description:
      "A one-minute video of the edytlab desktop app: a DJ describes a blend in plain English, the agent calls its tools, and the mix is exported.",
    seconds: 58,
    uploadDate: "2026-10-10",
    width: 1920,
    height: 1080,
    source: "marketing/launch-video/out/launch-reddit.mp4",
    posterAt: 8,
    captionsSource: "marketing/launch-video/out/launch-reddit.srt",
    momentsSource: "marketing/launch-video/out/launch-reddit.srt",
    moments: [
      { at: 0.55, text: "Title: an open-source audio editor you drive in plain English." },
      { at: 4.35, text: "Step 1, describe it in plain English: a request is typed asking what tempo each track is, without changing anything yet." },
      { at: 9.05, text: "Step 2, the agent calls a tool: it time-stretches one track to the other's tempo, keeping its pitch." },
      { at: 13.15, text: "Step 3, it flags what it noticed: the bars of the two tracks do not line up yet, and it offers ways to line them up." },
      { at: 16.75, text: "Step 4, describe the blend: fade one track out and the other in, with a low-pass filter on the outgoing track, which becomes three tool calls." },
      { at: 24.45, text: "Step 5, hear it: the transition plays in real time." },
      { at: 33.35, text: "Step 6, master and export: the mix is mastered for streaming and exported as a WAV at about −14 LUFS, peaking at −1 dBFS." },
      { at: 39.05, text: "Also: hold the low end until the drop, with a 200 Hz high-pass." },
      { at: 42.75, text: "And a three-track mini-mix, with four-bar overlaps." },
      { at: 46.5, text: "How it works: your audio files stay on your computer and the app's own engine edits them; only the chat goes to the provider you choose." },
      { at: 53.7, text: "End card: free and open source (MIT), github.com/laadtushar/edytlab, edytlab.com. The installers are unsigned." },
    ],
  },
  {
    slug: "reel-v070-whats-new",
    kind: "reel",
    title: "What's new in v0.7.0 (short reel)",
    blurb: "Five changes in v0.7.0, in thirty seconds.",
    disclosure: "Real app, real Claude (Sonnet 5.5). Typing and waits sped up where labelled.",
    caption:
      "Five changes you can see in v0.7.0, in under 40 seconds: faders that stay where you put them (#596), the history graph keeping up (#602), a 200-track session where list_session reads what the summary leaves out (#618), playback that survives switching tabs (#607), and bundled skills that stay current (#574). Real app, real Claude (Sonnet 5.5): filmed live, the assistant's replies and tool calls are the model's, as the note on every shot says.",
    description:
      "A 39-second vertical video of five changes in edytlab v0.7.0, filmed live in the real app with Claude as the assistant.",
    seconds: 39,
    uploadDate: "2026-10-22",
    width: 1080,
    height: 1920,
    source: "marketing/promo-v0.7.0/out/edytlab-v0.7.0-whats-new-9x16.mp4",
    posterAt: 17,
    momentsSource: "marketing/promo-v0.7.0/out/edytlab-v0.7.0-whats-new-9x16.srt",
    moments: [
      { at: 0, text: "What's new in v0.7.0: five changes you can see." },
      { at: 2.6, text: "Faders stay where you put them (#596): four key taps, four steps, +0.5 to +2.0 dB with no bounce back." },
      { at: 6.7, text: "The history graph keeps up (#602): names and clean-ups show at once; 20 nodes became 8 (shown at 2× and 1.6× speed)." },
      { at: 13.667, text: "200 tracks? Just ask (#618): list_session reads what the summary left out (shown at up to 2.2× speed), then the strings sit at −6.0 dB." },
      { at: 23.167, text: "Switch tabs and it still plays, even when the mix is gone from the cache (#607): it plays with no error." },
      { at: 30.467, text: "Bundled skills stay current (#574): updated when a release changes them, unless you edited them." },
      { at: 35.067, text: "End card: edytlab v0.7.0, edytlab.com, free and open source (MIT), for macOS, Windows and Linux; the installers are unsigned." },
    ],
  },
  {
    slug: "reel-v070-describe-the-edit",
    kind: "reel",
    title: "Describe the edit, and it happens (short reel)",
    blurb: "Type an edit in plain English and watch the fade and the gain change appear.",
    disclosure: "Real app, real Claude (Sonnet 5.5). Typing and waits sped up where labelled.",
    caption:
      "Describe the edit: a request typed in plain English, Claude calls fade and then gain, the timeline changes (the backing fades in over 4 seconds and the vocal lane reads −3.0 dB), and you press play to hear it, on a synthetic demo track made in code. Real app, real Claude (Sonnet 5.5): filmed live, the assistant's replies and tool calls are the model's, as the note on every shot says.",
    description:
      "A 26-second vertical video: an edit typed in plain English, carried out by Claude, then played, filmed live in the real edytlab app.",
    seconds: 26,
    uploadDate: "2026-10-22",
    width: 1080,
    height: 1920,
    source: "marketing/promo-v0.7.0/out/edytlab-v0.7.0-describe-the-edit-9x16.mp4",
    posterAt: 12.5,
    momentsSource: "marketing/promo-v0.7.0/out/edytlab-v0.7.0-describe-the-edit-9x16.srt",
    moments: [
      { at: 0, text: "Describe the edit." },
      { at: 1.9, text: "Say it in plain English: the request is typed into the chat box." },
      { at: 5.467, text: "The assistant calls the tools, fade and then gain (the wait for Claude shown at 2.4× speed)." },
      { at: 11.767, text: "The timeline changes: the backing fades in over 4 seconds and the vocal lane reads −3.0 dB." },
      { at: 13.367, text: "Then hear it: press play on a synthetic demo track, with demo audio made in code." },
      { at: 22.667, text: "End card: edytlab v0.7.0, edytlab.com, free and open source (MIT), for macOS, Windows and Linux; the installers are unsigned." },
    ],
  },
  {
    slug: "reel-v070-200-tracks",
    kind: "reel",
    title: "200 tracks? Just ask (short reel)",
    blurb: "One sentence quiets the twelve string tracks in a 200-track session.",
    disclosure: "Real app, real Claude (Sonnet 5.5). Typing and waits sped up where labelled.",
    caption:
      "A session of 200 tracks and one sentence, “make all the strings quieter”: Claude reads what the session summary left out with list_session, then lowers twelve tracks (violins, viola, cello and contrabass) by 6 dB, each volume change a node on the graph (#618). Real app, real Claude (Sonnet 5.5): filmed live, the assistant's replies and tool calls are the model's, and the sound is interface sounds only.",
    description:
      "A 27-second vertical video: one sentence quiets twelve of 200 tracks, filmed live in the real edytlab app with Claude as the assistant.",
    seconds: 27,
    uploadDate: "2026-10-22",
    width: 1080,
    height: 1920,
    source: "marketing/promo-v0.7.0/out/edytlab-v0.7.0-200-tracks-9x16.mp4",
    posterAt: 11,
    momentsSource: "marketing/promo-v0.7.0/out/edytlab-v0.7.0-200-tracks-9x16.srt",
    moments: [
      { at: 0, text: "200 tracks? Ask." },
      { at: 3.3, text: "Just say it: “make all the strings quieter”." },
      { at: 6.767, text: "It reads what the summary left out: list_session, then gain." },
      { at: 10.167, text: "Twelve strings, one sentence: each change is a node on the graph." },
      { at: 15.967, text: "All twelve strings 6 dB quieter: violins, viola, cello and contrabass read −6.0 dB." },
      { at: 23.567, text: "End card: edytlab v0.7.0, edytlab.com, free and open source (MIT), for macOS, Windows and Linux; the installers are unsigned." },
    ],
  },
  {
    slug: "reel-highlights",
    kind: "reel",
    title: "Highlights: three DJ jobs in a minute",
    blurb: "A moment from each of the three DJ jobs, in about a minute.",
    disclosure: "Cut from the full recordings, with the app's own sound. Waits sped up.",
    caption:
      "A moment from each of the three demos with the sound of the app: beatmatching two tracks and blending them with a crossfade and a low-pass on the outgoing track, extending an intro with a filter that opens into the drop, and a three-track mini-mix at 124 BPM.",
    description:
      "A one-minute vertical video of the edytlab desktop app with a moment from three DJ jobs: a beatmatched blend, an extended intro and a three-track mini-mix.",
    seconds: 62,
    uploadDate: "2026-10-10",
    width: 1080,
    height: 1920,
    source: "marketing/reels/reel-highlights.mp4",
    posterAt: 9.7,
    moments: [
      { at: 0.0, text: "Describe the blend. Hear the mix." },
      { at: 2.5, text: "2 · Beatmatch" },
      { at: 7.3, text: "4 · Blend: crossfade, low-pass on the outgoing track" },
      { at: 12.1, text: "Listen: the transition, from 0:12" },
      { at: 20.867, text: "2 · Extend the intro" },
      { at: 25.667, text: "3 · A filter that opens into the drop" },
      { at: 30.467, text: "Listen: the low end arrives at the drop (0:31)" },
      { at: 39.667, text: "1 · Match every track to 124 BPM" },
      { at: 44.467, text: "2 · Sequence with 4-bar overlaps" },
      { at: 49.267, text: "Listen: Midnight Drive into Solar Flare (0:23)" },
      { at: 58.533, text: "edytlab · Free, open source · edytlab.com · macOS · Windows · Linux" },
    ],
    momentsSource: "marketing/reels/reel-highlights-cues.json",
  },
  {
    slug: "reel-teaser",
    kind: "reel",
    title: "Teaser: ask for a blend, hear the result",
    blurb: "Ask for a blend and hear the result, in fifteen seconds.",
    disclosure: "Cut from the full recordings, with the app's own sound. Waits sped up.",
    caption:
      "Fifteen seconds: ask for the blend, the beatmatch, the crossfade with a low-pass on the outgoing track, and a few seconds of the transition you can hear.",
    description:
      "A fifteen-second vertical video of the edytlab desktop app: a request for a blend, the beatmatch, the crossfade and a few seconds of the result.",
    seconds: 15,
    uploadDate: "2026-10-10",
    width: 1080,
    height: 1920,
    source: "marketing/reels/reel-teaser.mp4",
    posterAt: 4,
    moments: [
      { at: 0.0, text: "Ask for the blend." },
      { at: 2.5, text: "2 · Beatmatch" },
      { at: 5.2, text: "4 · Blend: crossfade, low-pass on the outgoing track" },
      { at: 7.9, text: "Listen: the transition, from 0:12" },
      { at: 12.4, text: "edytlab · Free, open source · edytlab.com · macOS · Windows · Linux" },
    ],
    momentsSource: "marketing/reels/reel-teaser-cues.json",
  },
  {
    slug: "reel-beatmatched-transition",
    kind: "reel",
    title: "Beatmatch and blend two tracks (short reel)",
    blurb: "Beatmatch two tracks, blend them and export a mix at streaming loudness.",
    disclosure: "Cut from the full recording, with the app's own sound. Waits sped up.",
    caption:
      "Midnight Drive into Neon Rush: what the two tracks are, the beatmatch, lining up the overlap, the crossfade with a low-pass on the outgoing track, both listening moments, mastering for streaming, and the exported WAV at about −14 LUFS.",
    description:
      "A vertical video of the edytlab desktop app: two tracks are beatmatched, blended with a crossfade and a low-pass, mastered for streaming and exported as a WAV.",
    seconds: 51,
    uploadDate: "2026-10-10",
    width: 1080,
    height: 1920,
    source: "marketing/reels/reel-beatmatched-transition.mp4",
    posterAt: 9,
    moments: [
      { at: 0.0, text: "Beatmatch two tracks. Just ask." },
      { at: 2.5, text: "1 · What am I working with?" },
      { at: 7.3, text: "2 · Beatmatch" },
      { at: 12.1, text: "3 · Line up the overlap" },
      { at: 16.9, text: "4 · Blend: crossfade, low-pass on the outgoing track" },
      { at: 21.7, text: "Listen: the transition, from 0:12" },
      { at: 31.967, text: "5 · Master for streaming" },
      { at: 36.767, text: "The mastered mix, from 0:20" },
      { at: 43.5, text: "Done: midnight-into-neon.wav, 0:50, -14 LUFS" },
      { at: 47.6, text: "edytlab · Free, open source · edytlab.com · macOS · Windows · Linux" },
    ],
    momentsSource: "marketing/reels/reel-beatmatched-transition-cues.json",
  },
  {
    slug: "reel-extended-club-intro",
    kind: "reel",
    title: "Extend an intro for mixing (short reel)",
    blurb: "Make a sixteen-bar intro from the first eight bars and hear the drop arrive.",
    disclosure: "Cut from the full recording, with the app's own sound. Waits sped up.",
    caption:
      "Solar Flare: its tempo and bar length, the drums-only first eight bars repeated to make a 16-bar intro, a filter that opens into the drop, a fade-in and the export, with a listening moment where the low end arrives at the drop.",
    description:
      "A vertical video of the edytlab desktop app: a track's first eight bars become a 16-bar intro, a high-pass opens into the drop, and it is exported.",
    seconds: 39,
    uploadDate: "2026-10-10",
    width: 1080,
    height: 1920,
    source: "marketing/reels/reel-extended-club-intro.mp4",
    posterAt: 14,
    moments: [
      { at: 0.0, text: "Extend an intro. Just ask." },
      { at: 2.5, text: "1 · Tempo and bar length" },
      { at: 7.3, text: "2 · Extend the intro" },
      { at: 12.1, text: "3 · A filter that opens into the drop" },
      { at: 16.9, text: "4 · Fade in and export" },
      { at: 21.7, text: "Listen: the low end arrives at the drop (0:31)" },
      { at: 32.3, text: "Done: solar-flare-extended-intro.wav, a 16-bar intro" },
      { at: 36.4, text: "edytlab · Free, open source · edytlab.com · macOS · Windows · Linux" },
    ],
    momentsSource: "marketing/reels/reel-extended-club-intro-cues.json",
  },
  {
    slug: "reel-mini-mix",
    kind: "reel",
    title: "A three-track mini-mix (short reel)",
    blurb: "Three tracks, one mix: match the tempo, overlap the transitions and export.",
    disclosure: "Cut from the full recording, with the app's own sound. Waits sped up.",
    caption:
      "Three tracks matched to 124 BPM, sequenced with four-bar overlaps, brought to about the same loudness with a limiter on the master, both transitions heard, and the mix exported as a WAV.",
    description:
      "A vertical video of the edytlab desktop app: three tracks are matched to 124 BPM, joined with overlapping crossfades, loudness-matched and exported as a mix.",
    seconds: 42,
    uploadDate: "2026-10-10",
    width: 1080,
    height: 1920,
    source: "marketing/reels/reel-mini-mix.mp4",
    posterAt: 9.5,
    moments: [
      { at: 0.0, text: "Build a mini-mix. Just ask." },
      { at: 2.5, text: "1 · Match every track to 124 BPM" },
      { at: 7.3, text: "2 · Sequence with 4-bar overlaps" },
      { at: 12.1, text: "3 · Match loudness, limit the master" },
      { at: 16.9, text: "Listen: Midnight Drive into Solar Flare (0:23)" },
      { at: 23.633, text: "Listen: Solar Flare into Neon Rush (0:46)" },
      { at: 30.333, text: "4 · Export the mix" },
      { at: 35.133, text: "Done: mini-mix-124bpm.wav, 1:19" },
      { at: 39.133, text: "edytlab · Free, open source · edytlab.com · macOS · Windows · Linux" },
    ],
    momentsSource: "marketing/reels/reel-mini-mix-cues.json",
  },
];

/**
 * The three full demos as videos with a page of their own. Everything
 * but the moments comes from `lib/demos.ts`, so the page, the card on
 * the home page and the DJ page cannot say different things; the files
 * are the demos' own in `public/demos/`, not copies.
 *
 * The moments are each demo's caption cues, `onscreen` as written, from
 * `marketing/reels/demo-<slug>-cues.json` (its `narration` lines belong
 * to an earlier take and are not read). The demos' videos have the app's
 * own sound and no voice-over.
 */
const DEMO_MOMENTS: Record<string, readonly Moment[]> = {
  "dj-beatmatched-transition": [
    { at: 0.0, text: "A DJ blends Midnight Drive (120 BPM) into Neon Rush (128 BPM)" },
    { at: 2.467, text: "Open the tracks" },
    { at: 10.067, text: "Before: both tracks from the top, 120 against 128 BPM" },
    { at: 21.933, text: "1 · What am I working with?" },
    { at: 34.867, text: "2 · Beatmatch" },
    { at: 47.333, text: "3 · Line up the overlap" },
    { at: 59.6, text: "4 · Blend: crossfade, low-pass on the outgoing track" },
    { at: 82.133, text: "Listen: the transition, from 0:12" },
    { at: 104.4, text: "5 · Master for streaming" },
    { at: 132.067, text: "6 · Export" },
    { at: 144.933, text: "The mastered mix, from 0:20" },
    { at: 161.2, text: "Done: midnight-into-neon.wav, 0:50, -14 LUFS" },
  ],
  "dj-extended-club-intro": [
    { at: 0.0, text: "A DJ makes a mixable intro for Solar Flare (124 BPM)" },
    { at: 2.4, text: "Open the tracks" },
    { at: 9.733, text: "Before: the original intro" },
    { at: 22.733, text: "1 · Tempo and bar length" },
    { at: 35.4, text: "2 · Extend the intro" },
    { at: 51.6, text: "3 · A filter that opens into the drop" },
    { at: 66.333, text: "4 · Fade in and export" },
    { at: 80.467, text: "Listen: the new intro fades in" },
    { at: 94.2, text: "Listen: the low end arrives at the drop (0:31)" },
    { at: 110.733, text: "Done: solar-flare-extended-intro.wav, a 16-bar intro" },
  ],
  "dj-mini-mix": [
    { at: 0.0, text: "A DJ builds a three-track mini-mix at 124 BPM" },
    { at: 2.467, text: "Open the tracks" },
    { at: 10.267, text: "1 · Match every track to 124 BPM" },
    { at: 24.6, text: "2 · Sequence with 4-bar overlaps" },
    { at: 46.667, text: "3 · Match loudness, limit the master" },
    { at: 65.467, text: "Listen: Midnight Drive into Solar Flare (0:23)" },
    { at: 83.4, text: "Listen: Solar Flare into Neon Rush (0:46)" },
    { at: 100.8, text: "4 · Export the mix" },
    { at: 114.0, text: "Done: mini-mix-124bpm.wav, 1:19" },
  ],
};

/** The demos' `blurb` and `disclosure`: `lib/demos.ts` is shared with the home page, which has its own words. */
const DEMO_COPY: Record<string, { blurb: string; disclosure: string }> = {
  "dj-beatmatched-transition": {
    blurb: "Bring two tracks to the same tempo, line up the bars and blend them with a crossfade.",
    disclosure: "Recorded in the real app with Claude, on short synthetic demo tracks. Waits sped up.",
  },
  "dj-extended-club-intro": {
    blurb: "Turn an eight-bar intro into a sixteen-bar one and open a filter into the drop.",
    disclosure: "Recorded in the real app with Claude, on short synthetic demo tracks. Waits sped up.",
  },
  "dj-mini-mix": {
    blurb: "Match three tracks to 124 BPM, join them with overlapping crossfades and export the mix.",
    disclosure: "Recorded in the real app with Claude, on short synthetic demo tracks. Waits sped up.",
  },
};

export const demoVideos: readonly DemoVideo[] = demos.map((d) => ({
  slug: d.slug,
  kind: "demo",
  title: d.title,
  caption: d.caption,
  blurb: DEMO_COPY[d.slug]?.blurb ?? d.description,
  disclosure: DEMO_COPY[d.slug]?.disclosure ?? "Recorded in the real app. Waits sped up.",
  description: d.description,
  seconds: d.seconds,
  uploadDate: d.uploadDate,
  width: DEMO_FRAME.width,
  height: DEMO_FRAME.height,
  momentsSource: `marketing/reels/demo-${d.slug}-cues.json`,
  moments: DEMO_MOMENTS[d.slug] ?? [],
}));

/** The release video, the launch video and the reels, split the way the hub lays them out. */
export const releaseVideo = videos.find((v) => v.kind === "release") as SiteVideo;
export const launchVideo = videos.find((v) => v.kind === "launch") as SiteVideo;
export const reels: readonly SiteVideo[] = videos.filter((v) => v.kind === "reel");

/** Every video that has a page of its own, in the hub's order. */
export const watchVideos: readonly WatchVideo[] = [releaseVideo, launchVideo, ...demoVideos, ...reels];

export const getVideo = (slug: string): WatchVideo | undefined => watchVideos.find((v) => v.slug === slug);

/** Where a video's files are served from: `public/videos/`, or `public/demos/` for a demo. */
export const VIDEOS_PATH = "/videos";
const folderOf = (slug: string) => (demoVideos.some((d) => d.slug === slug) ? "/demos" : VIDEOS_PATH);

export const videoSrc = (slug: string) => (folderOf(slug) === "/demos" ? demoVideoSrc(slug) : `${VIDEOS_PATH}/${slug}.mp4`);
export const videoPosterSrc = (slug: string) =>
  folderOf(slug) === "/demos" ? demoPosterSrc(slug) : `${VIDEOS_PATH}/${slug}.jpg`;
export const videoCaptionsSrc = (slug: string) => `${VIDEOS_PATH}/${slug}.vtt`;
/** The video's own page: `/videos/<slug>`, where it is the main content. */
export const videoPath = (slug: string) => `${VIDEOS_PATH}/${slug}`;

/** A running time as the card shows it: 62 is "1:02". */
export const formatDuration = (seconds: number) =>
  `${Math.floor(seconds / 60)}:${String(Math.round(seconds % 60)).padStart(2, "0")}`;

/** A moment's time as the page shows it, to the whole second it falls in: 20.867 is "0:20". */
export const formatMoment = (at: number) =>
  `${Math.floor(at / 60)}:${String(Math.floor(at % 60)).padStart(2, "0")}`;

/** A card's title: the page's title without the "(short reel)" that tells it apart from the demo it was cut from. */
export const cardTitle = (video: WatchVideo) => video.title.replace(/\s*\(short reel\)$/i, "");

/**
 * A caption or a moment as a viewer reads it: without the tracker
 * numbers, "(#596)", that the repository's notes carry and a visitor has
 * no use for.
 */
export const forViewers = (text: string) => text.replace(/\s*\(#\d+(?:,\s*#\d+)*\)/g, "");

/**
 * The videos to offer after this one: three of the same shape (landscape
 * or vertical), four for a reel so a four-column row is full. Its own
 * kind first, starting after it in the list and wrapping round, so each
 * page leads to a different neighbour; then the other kinds.
 */
export function relatedVideos(video: WatchVideo): WatchVideo[] {
  const wanted = video.kind === "reel" ? 4 : 3;
  const vertical = video.height > video.width;
  const sameShape = watchVideos.filter((v) => v.slug !== video.slug && v.height > v.width === vertical);
  const at = watchVideos.indexOf(video);
  const byDistance = (a: WatchVideo, b: WatchVideo) => {
    const d = (v: WatchVideo) => (watchVideos.indexOf(v) - at + watchVideos.length) % watchVideos.length;
    return d(a) - d(b);
  };
  const sameKind = sameShape.filter((v) => v.kind === video.kind).sort(byDistance);
  const others = sameShape.filter((v) => v.kind !== video.kind);
  return [...sameKind, ...others].slice(0, wanted);
}
