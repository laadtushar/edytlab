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
 *   It is the only part of the demo a visitor downloads unasked, and it
 *   loads lazily (`FeaturedVideo`'s `lazy`).
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
  /**
   * One sentence for the video's structured data (`videoObjectJsonLd`),
   * which a search result may show. Shorter than the caption and held to
   * the same rule: it says what the recording shows and nothing more.
   */
  description: string;
  /**
   * The running time in whole seconds, as the file itself states it
   * (`ffprobe -show_entries format=duration`, rounded). `demos.test.ts`
   * reads the length out of the `.mp4` and fails when this disagrees, so
   * a re-recording cannot leave a stale figure in the structured data.
   */
  seconds: number;
  /**
   * The day the file was first committed to the repository, as
   * `YYYY-MM-DD` (`git log --diff-filter=A --format=%cs -- <the .mp4>`).
   * The three recordings were re-filmed under the same names on
   * 2026-10-13; the day each name first went up is the upload date.
   */
  uploadDate: string;
}

export const demos: readonly Demo[] = [
  {
    slug: "dj-beatmatched-transition",
    title: "Beatmatch and blend two tracks",
    caption:
      "A DJ checks the two tracks' tempos (Neon Rush about 6.7% faster). Claude time-stretches Neon Rush to Midnight Drive's 120 BPM, keeping its pitch, starts it at 16 s under Midnight Drive's last 8 bars, and blends the two with a crossfade and a 1 kHz low-pass on the outgoing track. The mix is mastered to about −14 LUFS, limited at −1 dB and exported as a 50-second WAV. Playbacks are in real time.",
    duration: "2:45",
    description:
      "A screen recording of the edytlab desktop app: a DJ has Claude match one track's tempo to another's, blend the two with a crossfade and a low-pass, master the mix and export a WAV.",
    seconds: 165,
    uploadDate: "2026-10-09",
  },
  {
    slug: "dj-extended-club-intro",
    title: "Extend an intro for mixing",
    caption:
      "A DJ asks for Solar Flare's tempo and bar length (124 BPM, a 1.935-second bar) without changing anything. Its drums-only first 8 bars are copied and pasted after themselves for a 16-bar intro, a 200 Hz high-pass holds the low end back until the drop at 31.45 s, and the first 4 bars fade in. Claude exports a WAV and flags that it peaks at full scale. Playbacks are in real time.",
    duration: "1:55",
    description:
      "A screen recording of the edytlab desktop app: a DJ asks Claude for a track's tempo and bar length, then has its first 8 bars copied to make a 16-bar intro, and exports a WAV.",
    seconds: 115,
    uploadDate: "2026-10-09",
  },
  {
    slug: "dj-mini-mix",
    title: "A three-track mini-mix",
    caption:
      "A DJ builds a warm-up mix at 124 BPM. Midnight Drive and Neon Rush are time-stretched to 124, keeping their pitch, and each track comes in about 4 bars before the one before it ends, with a straight-line crossfade over each overlap. Each is brought to about −14 LUFS, a limiter at −1 dB sits on the master, and the mix is exported as a 79-second WAV. Playbacks are in real time.",
    duration: "1:58",
    description:
      "A screen recording of the edytlab desktop app: a DJ has Claude bring three tracks to 124 BPM, join them with overlapping crossfades, match their loudness and export the mix as a WAV.",
    seconds: 118,
    uploadDate: "2026-10-09",
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
