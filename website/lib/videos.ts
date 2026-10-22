/**
 * The videos on `/videos` besides the three full demos (those are
 * `lib/demos.ts`): the launch video and five short vertical reels.
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
 * - `<slug>.vtt`: for the launch video, its `.srt` converted to WebVTT
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
 * and nothing else.
 *
 * A caption says what the video shows and nothing more, like the demos':
 * nothing that has not shipped, no counts, no ratings, no superlatives.
 */

export interface SiteVideo {
  /** Kebab-case. Names the files in `public/videos/`. */
  slug: string;
  /** The launch video is 16:9; a reel is 9:16. */
  kind: "launch" | "reel";
  title: string;
  /** What it shows, for the page. */
  caption: string;
  /** One sentence for the video's structured data, which a search result may show. */
  description: string;
  /** Running time in whole seconds (`ffprobe`, rounded). The test reads the file's own header. */
  seconds: number;
  /**
   * The day the source file was first committed under `marketing/`
   * (`git log --diff-filter=A --format=%cs -- <source>`). Each has been
   * re-cut since, under the same name.
   */
  uploadDate: string;
  /** The picture's size in pixels, as the file states it; the player's box is reserved from it. */
  width: number;
  height: number;
  /** The file this is a copy of, from the repository root. */
  source: string;
  /** Where in the video, in seconds, the poster frame was taken. */
  posterAt: number;
  /** For the launch video: the `.srt` its WebVTT captions were converted from. */
  captionsSource?: string;
  /**
   * What happens, in order, for the watch page: one entry per cue of the
   * source (an `.srt` cue for the launch video, a cue of `reel-*-cues.json`
   * for a reel), at the second the cue starts. A reel's text is the cue's
   * own on-screen words; the launch video's is a plain description of
   * each caption. There is no voice-over in any of them. `videos.test.ts`
   * holds the times, and a reel's words, to the source, so a re-cut
   * cannot leave this stale.
   */
  moments: readonly Moment[];
  /** The cue file a reel's `moments` come from (the launch video's are its `captionsSource`). */
  cuesSource?: string;
}

export interface Moment {
  /** Seconds from the start of the video. */
  at: number;
  text: string;
}

export const videos: readonly SiteVideo[] = [
  {
    slug: "launch-video",
    kind: "launch",
    title: "Launch video: a DJ job from request to export",
    caption:
      "One DJ job from request to exported file, cut from the real recordings of the desktop app: the request is typed in plain English, the agent calls its tools and points out what it noticed, you hear the blend, and the mix is mastered and exported as a WAV. Two more jobs follow, then how it works (the audio stays on your machine, only the chat goes to the provider you choose) and its limits: demo tracks are short synthetic loops, it prepares files and is not a live DJ tool, there is no stem separation or transcription yet, and the installers are unsigned. Captions are available.",
    description:
      "A one-minute video of the edytlab desktop app: a DJ describes a blend in plain English, the agent calls its tools, and the mix is exported. Limits stated.",
    seconds: 64,
    uploadDate: "2026-10-10",
    width: 1920,
    height: 1080,
    source: "marketing/launch-video/out/launch-reddit.mp4",
    posterAt: 8,
    captionsSource: "marketing/launch-video/out/launch-reddit.srt",
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
      { at: 53.1, text: "The limits, stated on screen: the demo tracks are short synthetic loops, it prepares files and is not a live DJ tool, and there is no stem separation or transcription yet." },
      { at: 59.0, text: "End card: free and open source (MIT), github.com/laadtushar/edytlab, edytlab.com. The installers are unsigned." },
    ],
  },
  {
    slug: "reel-highlights",
    kind: "reel",
    title: "Highlights: three DJ jobs in a minute",
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
    cuesSource: "marketing/reels/reel-highlights-cues.json",
  },
  {
    slug: "reel-teaser",
    kind: "reel",
    title: "Teaser: ask for a blend, hear the result",
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
    cuesSource: "marketing/reels/reel-teaser-cues.json",
  },
  {
    slug: "reel-beatmatched-transition",
    kind: "reel",
    title: "Beatmatch and blend two tracks (short reel)",
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
    cuesSource: "marketing/reels/reel-beatmatched-transition-cues.json",
  },
  {
    slug: "reel-extended-club-intro",
    kind: "reel",
    title: "Extend an intro for mixing (short reel)",
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
    cuesSource: "marketing/reels/reel-extended-club-intro-cues.json",
  },
  {
    slug: "reel-mini-mix",
    kind: "reel",
    title: "A three-track mini-mix (short reel)",
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
    cuesSource: "marketing/reels/reel-mini-mix-cues.json",
  },
];

/** Where the files are served from: `public/videos/` in the source tree. */
export const VIDEOS_PATH = "/videos";

export const videoSrc = (slug: string) => `${VIDEOS_PATH}/${slug}.mp4`;
export const videoPosterSrc = (slug: string) => `${VIDEOS_PATH}/${slug}.jpg`;
export const videoCaptionsSrc = (slug: string) => `${VIDEOS_PATH}/${slug}.vtt`;
/** The video's own page: `/videos/<slug>`, where it is the main content. */
export const videoPath = (slug: string) => `${VIDEOS_PATH}/${slug}`;

export const getVideo = (slug: string): SiteVideo | undefined => videos.find((v) => v.slug === slug);

/** The launch video and the reels, split the way the page lays them out. */
export const launchVideo = videos.find((v) => v.kind === "launch") as SiteVideo;
export const reels: readonly SiteVideo[] = videos.filter((v) => v.kind === "reel");

/** A running time as the card shows it: 62 is "1:02". */
export const formatDuration = (seconds: number) =>
  `${Math.floor(seconds / 60)}:${String(Math.round(seconds % 60)).padStart(2, "0")}`;

/** A moment's time as the page shows it, to the whole second it falls in: 20.867 is "0:20". */
export const formatMoment = (at: number) =>
  `${Math.floor(at / 60)}:${String(Math.floor(at % 60)).padStart(2, "0")}`;
