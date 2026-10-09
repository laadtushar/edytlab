/**
 * The demo videos: screen recordings of the desktop app, with Claude as
 * the agent, played in the home page's `#demos` section and linked from
 * the root README.
 *
 * Each entry names two files by its slug, both in `public/demos/`:
 *
 * - `<slug>.mp4` — H.264 video with AAC sound, so it plays wherever
 *   `<video>` does;
 * - `<slug>.jpg` — the poster, shown until the reader presses play.
 *   With `preload="none"` it is the only part of the demo a visitor
 *   downloads unasked.
 *
 * `demos.test.ts` fails when either file is missing for an entry, and
 * when an `.mp4` in that folder has no entry here, so the list and the
 * folder cannot drift apart.
 *
 * A caption says what the recording shows and nothing more. It sits next
 * to a video anyone can watch, so a claim the video does not back up is
 * caught by the first reader who presses play.
 */

export interface Demo {
  /** Kebab-case. Names both files in `public/demos/`. */
  slug: string;
  title: string;
  caption: string;
  /** Running time shown on the card, such as "1:42". Omit until known. */
  duration?: string;
}

export const demos: readonly Demo[] = [
  {
    slug: "dj-beatmatched-transition",
    title: "Beatmatch and blend two tracks",
    caption:
      "A DJ asks what they are working with: both tracks are in F major, at 120 and 128 BPM. Neon Rush is time-stretched to Midnight Drive's 120 BPM and started at 16 s, under Midnight Drive's last 8 bars, and Claude points out that its first downbeat sits half a second into the file and offers to nudge it. Midnight Drive fades out under a 2 kHz low-pass while Neon Rush fades in, then the mix is mastered for streaming and exported as a 50-second WAV at −14 LUFS, peaking at −1 dBFS. Waits for Claude are sped up; every playback is in real time, with sound.",
    duration: "3:08",
  },
  {
    slug: "dj-extended-club-intro",
    title: "Extend an intro for mixing",
    caption:
      "A DJ asks for Solar Flare's tempo and bar length without changing anything: 124 BPM, a 1.935-second bar, the first downbeat at 0.476 s. The drums-only first 8 bars are copied from that downbeat and spliced in after themselves, so the intro runs 16 bars. A 150 Hz high-pass holds the low end back until the drop on bar 17, the first 4 bars fade in, and the track is exported as a WAV; Claude flags that it peaks at 0 dBFS with no headroom. You hear the new intro, then the drop at 0:31. Waits for Claude are sped up; every playback is in real time, with sound.",
    duration: "1:56",
  },
  {
    slug: "dj-mini-mix",
    title: "A three-track mini-mix",
    caption:
      "A DJ builds a warm-up mix at 124 BPM. Midnight Drive and Neon Rush are time-stretched to 124, keeping their pitch. Claude analyses all three and sequences them so each comes in 4 bars before the one before it ends, with a crossfade over each overlap, and flags that Solar Flare's 16½ bars put the last entry half a bar off its beat grid. Each track is brought to about −14 LUFS, then Claude renders the three into a Master track with a limiter at −1 dBFS and mutes the originals. You hear both transitions, then the mix is exported as an 80-second WAV. Waits for Claude are sped up; every playback is in real time, with sound.",
    duration: "2:18",
  },
];

/** Where the files are served from — `public/demos/` in the source tree. */
export const DEMOS_PATH = "/demos";

export const demoVideoSrc = (slug: string) => `${DEMOS_PATH}/${slug}.mp4`;
export const demoPosterSrc = (slug: string) => `${DEMOS_PATH}/${slug}.jpg`;

/**
 * The frame every demo is laid out at, before the video has loaded a
 * byte. The recordings are captures of the app's content area at
 * 1280×776; reserving that box up front is what stops the page jumping
 * when a poster arrives. A recording at another ratio is letterboxed
 * inside the box rather than resizing it.
 */
export const DEMO_FRAME = { width: 1280, height: 776 } as const;
