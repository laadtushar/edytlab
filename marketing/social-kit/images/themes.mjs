/**
 * What each image says, and what each canvas is.
 *
 * Copy lives here, once, so the seven sizes of a theme cannot drift apart.
 * A number that exists elsewhere in the repo (the tool count) is filled in
 * by render.mjs at render time, not typed: `{{tools}}`.
 *
 * Every claim is one the released app makes true; where each comes from is
 * in README.md ("What the images claim").
 */

/** What the footage's own disclosure says for each source. */
const LIVE_CLAUDE = "Real app, real Claude";
const LIVE = "Real app, recorded live";

export const THEMES = [
  {
    id: "1-describe-the-edit",
    headline: `Describe <span class="gradient-text">the edit.</span>`,
    sub: "Type what you want. The assistant edits the timeline.",
    scale: 1.2,
    label: LIVE_CLAUDE,
  },
  {
    id: "2-dj-transition-in-one-sentence",
    headline: `A DJ transition <span class="gradient-text">in one sentence.</span>`,
    sub: "Beatmatched, then blended with a crossfade and a <span class='nw'>low-pass filter.</span>",
    label: LIVE_CLAUDE,
  },
  {
    id: "3-extend-a-club-intro",
    headline: `Extend a <span class="gradient-text">club intro.</span>`,
    sub: "Eight bars become sixteen, and the low end holds back until the drop.",
    scale: 1.15,
    label: LIVE_CLAUDE,
  },
  {
    id: "4-your-audio-stays-on-your-computer",
    headline: `Your audio <span class="gradient-text">stays on your computer.</span>`,
    sub: "Only the chat goes to the provider you choose. Ollama runs fully local, with no key.",
    label: LIVE_CLAUDE,
    // Two tags over the frame: what stays, and what is sent. Positions are
    // fractions of the frame, per crop, and sit on the empty part of each.
    tags: {
      wide: [
        { text: "Your audio stays here", x: 0.03, y: 0.3, tone: "lit" },
        { text: "The chat goes to your provider", x: 0.97, y: 0.035, align: "right" },
      ],
      mid: [
        { text: "Your audio stays here", x: 0.03, y: 0.3, tone: "lit" },
        { text: "The chat goes to your provider", x: 0.97, y: 0.035, align: "right" },
      ],
      tall: [
        { text: "Your audio stays here", x: 0.04, y: 0.2, tone: "lit" },
        { text: "The chat goes to your provider", x: 0.96, y: 0.035, align: "right" },
      ],
    },
  },
  {
    id: "5-every-edit-is-a-version",
    headline: `Every edit is <span class="gradient-text">a version.</span>`,
    sub: "Each change is a node in a history graph. Go back to any of them.",
    scale: 1.1,
    label: LIVE,
  },
  {
    id: "6-free-and-open-source",
    headline: `Free and <span class="gradient-text">open source.</span>`,
    sub: null, // the chips and the note are its supporting line
    scale: 1.1,
    label: LIVE_CLAUDE,
    chips: ["MIT", "{{tools}} tools", "macOS · Windows · Linux"],
    note: "The installers are unsigned.",
    url: "github.com/laadtushar/edytlab",
  },
];

/**
 * The canvases. `layout`: "stack" is brand, headline, line, frame, footer in
 * one column; "side" is the text in a column beside the frame. `crop` picks
 * which of frames/tN-<crop>.png to show. Sizes are px on the canvas.
 * `safeTop` / `safeBottom` are the platform UI zones kept free of text
 * (Stories, Reels and TikTok draw over the top 250 and the bottom 340).
 */
export const CANVASES = [
  { id: "square-1080x1080", w: 1080, h: 1080, layout: "stack", crop: "mid", head: 78, sub: 32, pad: 72, gap: 22, platforms: "Instagram, Facebook, Threads (square)" },
  { id: "portrait-1080x1350", w: 1080, h: 1350, layout: "stack", crop: "mid", head: 92, sub: 34, pad: 72, gap: 28, platforms: "Instagram, Threads (portrait)" },
  { id: "story-1080x1920", w: 1080, h: 1920, layout: "stack", crop: "tall", head: 108, sub: 38, pad: 72, gap: 34, safeTop: 260, safeBottom: 350, platforms: "Stories, Reels, TikTok cover" },
  { id: "x-1600x900", w: 1600, h: 900, layout: "side", crop: "tall", head: 72, sub: 34, pad: 80, col: 0.40, gap: 28, platforms: "X, Bluesky" },
  { id: "linkedin-1200x627", w: 1200, h: 627, layout: "side", crop: "tall", head: 50, sub: 26, pad: 64, col: 0.39, gap: 16, tight: true, platforms: "LinkedIn, Facebook link" },
  { id: "youtube-1280x720", w: 1280, h: 720, layout: "side", crop: "tall", head: 58, sub: 28, pad: 68, col: 0.41, gap: 20, tight: true, platforms: "YouTube thumbnail" },
  { id: "pinterest-1000x1500", w: 1000, h: 1500, layout: "stack", crop: "tall", head: 90, sub: 34, pad: 72, gap: 28, platforms: "Pinterest" },
];
