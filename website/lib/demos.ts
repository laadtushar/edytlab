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
      "A DJ asks for both tracks' tempos, and the incoming track is time-stretched to match. It is started 8 bars before the outgoing track ends, the two are crossfaded, and a low-pass filter is put on the outgoing track over the overlap. The mix is compressed, limited at −1 dB, brought to −14 LUFS and exported as a WAV. Along the way Claude points out what it would change: at the 16 s start the bars land a beat apart. Shown at 1.6× speed.",
    duration: "2:58",
  },
  {
    slug: "dj-extended-club-intro",
    title: "Extend an intro for mixing",
    caption:
      "A DJ asks for Solar Flare's tempo and bar length, then has the drums-only first 8 bars repeated so the intro runs 16 bars, long enough to mix in over. A high-pass filter goes on the new intro, the first 4 bars fade in, and the track is exported as a WAV. You hear the original intro first, then the new one, then the drop at 0:31. Waits for Claude are sped up; every playback is in real time, with sound.",
    duration: "2:26",
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
