// Frames, safe areas and where things go in each. Pure data and arithmetic (no React), so scripts can import it too.
//
// 9:16 (Reels, Shorts, TikTok): Meta's published ad safe zone, "at least 14% of the top, 35% of the bottom, and 6% on
// each side" free of text and logos (the strictest published; marketing/launch-video/README.md has the citation). Every
// word the edit adds sits inside `safe`, and so does the footage card (the app's own words are inside it), so the
// whole of the app UI a viewer must read is clear of Meta's overlays too. `useSafe` (components/SafeCheck.tsx) measures
// the rendered words in the browser and fails the render if one lies outside; `assertLayouts` below checks the static boxes.

export type LayoutKind = 'portrait' | 'wide' | 'square';

export interface Box {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface Layout {
  kind: LayoutKind;
  width: number;
  height: number;
  /** Where words may be. */
  safe: Box;
  /** The title block. */
  title: Box;
  /** The footage card. */
  card: Box;
  /** The "v0.7.0 · real app · real Claude (Sonnet 5.5)" / "recorded live" note. */
  note: Box;
  /** How the note sits in its box. */
  noteAlign: 'left' | 'center' | 'right';
  /** Type sizes, px. */
  titleSize: number;
  subSize: number;
  labelSize: number;
  chipSize: number;
  noteSize: number;
  /** Corner radius of the card, px. */
  radius: number;
  /** Where the end card's block is centred (vertically), px from the top. */
  endCentreY: number;
  /** Scale of the end card's type relative to portrait. */
  endScale: number;
  /** Line breaks in titles are forced with \n; this is the characters per line titles may use (a guide for authors). */
  titleChars: number;
  /** The hook: big type. In 9:16 it sits in the title box; in the others it is centred over the card, which is dimmed. */
  hook: {box: Box; size: number; sub: number; scrim: boolean};
  /** Whether sub lines fit (a one-line title bar has no room for one). */
  subs: boolean;
  /** The thin progress line (no words, so it may sit in the 9:16 bottom zone): y and the x span. */
  progress: {y: number; x0: number; x1: number};
}

const safeBox = (w: number, h: number, top: number, bottom: number, side: number): Box => ({
  x: Math.ceil(w * side),
  y: Math.ceil(h * top),
  w: w - 2 * Math.ceil(w * side),
  h: Math.floor(h * (1 - bottom)) - Math.ceil(h * top),
});

export const PORTRAIT: Layout = {
  kind: 'portrait',
  width: 1080,
  height: 1920,
  safe: safeBox(1080, 1920, 0.14, 0.35, 0.06), // x 65..1015, y 269..1248
  title: {x: 80, y: 288, w: 920, h: 232},
  card: {x: 72, y: 540, w: 936, h: 604},
  note: {x: 72, y: 1166, w: 936, h: 44},
  noteAlign: 'left',
  titleSize: 74,
  subSize: 34,
  labelSize: 34,
  chipSize: 30,
  noteSize: 28,
  radius: 22,
  endCentreY: 748,
  endScale: 1,
  titleChars: 22,
  hook: {box: {x: 80, y: 288, w: 920, h: 240}, size: 86, sub: 34, scrim: false},
  subs: true,
  progress: {y: 1340, x0: 72, x1: 1008},
};

export const WIDE: Layout = {
  kind: 'wide',
  width: 1920,
  height: 1080,
  safe: {x: 80, y: 40, w: 1760, h: 1000}, // a 40 px margin all round (there is no published 16:9 rule to follow)
  title: {x: 96, y: 52, w: 1728, h: 84},
  card: {x: 96, y: 148, w: 1728, h: 840},
  note: {x: 96, y: 996, w: 1728, h: 40},
  noteAlign: 'left',
  titleSize: 56,
  subSize: 30,
  labelSize: 32,
  chipSize: 28,
  noteSize: 26,
  radius: 22,
  endCentreY: 540,
  endScale: 1.18,
  titleChars: 56,
  hook: {box: {x: 200, y: 340, w: 1520, h: 360}, size: 132, sub: 46, scrim: true},
  subs: false,
  progress: {y: 1058, x0: 96, x1: 1824},
};

export const SQUARE: Layout = {
  kind: 'square',
  width: 1080,
  height: 1080,
  safe: {x: 54, y: 54, w: 972, h: 972},
  title: {x: 70, y: 66, w: 940, h: 160},
  card: {x: 70, y: 244, w: 940, h: 690},
  note: {x: 70, y: 952, w: 940, h: 44},
  noteAlign: 'left',
  titleSize: 52,
  subSize: 28,
  labelSize: 30,
  chipSize: 26,
  noteSize: 26,
  radius: 20,
  endCentreY: 540,
  endScale: 0.86,
  titleChars: 34,
  hook: {box: {x: 90, y: 370, w: 900, h: 320}, size: 104, sub: 34, scrim: true},
  subs: true,
  progress: {y: 1054, x0: 70, x1: 1010},
};

export const LAYOUTS: Record<LayoutKind, Layout> = {portrait: PORTRAIT, wide: WIDE, square: SQUARE};

export const inside = (outer: Box, b: Box, tol = 0.5): boolean =>
  b.x >= outer.x - tol && b.y >= outer.y - tol && b.x + b.w <= outer.x + outer.w + tol && b.y + b.h <= outer.y + outer.h + tol;

const fmt = (b: Box) => `x ${Math.round(b.x)}..${Math.round(b.x + b.w)}, y ${Math.round(b.y)}..${Math.round(b.y + b.h)}`;

export function assertInside(outer: Box, b: Box, what: string): void {
  if (!inside(outer, b)) throw new Error(`${what} reaches ${fmt(b)}, outside the safe area ${fmt(outer)}`);
}

/** The static boxes of every layout lie in its safe area. Runs at import, so a layout edit that breaks it fails the build. */
export function assertLayouts(): void {
  for (const L of Object.values(LAYOUTS)) {
    assertInside(L.safe, L.title, `${L.kind} title`);
    assertInside(L.safe, L.card, `${L.kind} card`);
    assertInside(L.safe, L.note, `${L.kind} note`);
  }
  // Meta's numbers, spelled out, so a change to `safeBox` cannot loosen them.
  const s = PORTRAIT.safe;
  if (s.y < 0.14 * 1920 - 1 || s.y + s.h > 0.65 * 1920 + 1 || s.x < 0.06 * 1080 - 1 || s.x + s.w > 0.94 * 1080 + 1) {
    throw new Error(`the 9:16 safe area ${fmt(s)} is not Meta's (14% top, 35% bottom, 6% sides)`);
  }
}
assertLayouts();
