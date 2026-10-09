/**
 * The demo videos: screen recordings of the desktop app, with Claude as
 * the agent, played in the home page's `#demos` section and linked from
 * the root README.
 *
 * Each entry names two files by its slug, both in `public/demos/`:
 *
 * - `<slug>.mp4` — H.264, so it plays wherever `<video>` does;
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
      "A DJ asks for both tracks' tempos, and the incoming track is time-stretched to match. It is overlapped with the outgoing track and the two are crossfaded, with a low-pass filter sweep on the outgoing track. The mix is normalized to −14 LUFS and exported as a WAV.",
  },
  {
    slug: "dj-extended-club-intro",
    title: "Build an extended intro for mixing",
    caption:
      "A DJ takes one track and extends its intro by repeating the opening bars. A filter is added that opens up into the drop, the track is faded in, and the result is exported.",
  },
  {
    slug: "dj-mini-mix",
    title: "A three-track mini-mix, tempo- and loudness-matched",
    caption:
      "Three tracks at different tempos are matched to one tempo and sequenced with overlaps. They are loudness-matched and limited, and the mix is exported.",
  },
];

/** Where the files are served from — `public/demos/` in the source tree. */
export const DEMOS_PATH = "/demos";

export const demoVideoSrc = (slug: string) => `${DEMOS_PATH}/${slug}.mp4`;
export const demoPosterSrc = (slug: string) => `${DEMOS_PATH}/${slug}.jpg`;

/**
 * The frame every demo is laid out at, before the video has loaded a
 * byte. The recordings are 16:10 captures of the app window; reserving
 * that box up front is what stops the page jumping when a poster
 * arrives. A recording at another ratio is letterboxed inside the box
 * rather than resizing it.
 */
export const DEMO_FRAME = { width: 1280, height: 800 } as const;
