import type { ReactNode } from "react";

// Relative, not `@/`: `blog.test.ts` renders this file under vitest,
// which has no path aliases.
import { cn } from "../../lib/utils";

/**
 * One inline SVG cover per post, drawn from the site's own colour
 * tokens (`stroke-primary`, `fill-card`, … are Tailwind's names for the
 * `--primary`, `--card` CSS variables), so a cover follows the theme
 * instead of carrying a palette of its own.
 *
 * Inline rather than `<img src>` for that reason: an SVG loaded as an
 * image cannot see the page's CSS variables. They are also small, a
 * few KB each, because a bar is `M x y V y` in one shared `<path>`
 * and not an element apiece. `blog.test.ts` holds each under 8 KB.
 *
 * The motif is the post's subject, not a decoration: a beat grid for
 * the beatmatch post, a graph for the undo post. The social image
 * (`og-card.tsx`) is the PNG counterpart, because crawlers do not draw
 * SVG.
 */

/** Pseudo-random in [0, 1), the same for the same inputs, so a cover never changes between builds. */
function rand(seed: number, i: number): number {
  const x = Math.sin(seed * 127.1 + i * 311.7) * 43758.5453;
  return x - Math.floor(x);
}

const n1 = (v: number) => Math.round(v * 10) / 10;

/** Vertical bars as one path: a waveform. `shape` maps position 0..1 to a height multiplier. */
function bars(
  x0: number,
  x1: number,
  cy: number,
  amp: number,
  seed: number,
  step = 7,
  shape: (t: number) => number = () => 1,
): string {
  const count = Math.max(1, Math.floor((x1 - x0) / step));
  let d = "";
  for (let i = 0; i <= count; i++) {
    const h = Math.max(1.5, amp * shape(i / count) * (0.3 + 0.7 * rand(seed, i)));
    d += `M${n1(x0 + i * step)} ${n1(cy - h)}V${n1(cy + h)}`;
  }
  return d;
}

function Cover({ alt, className, children }: { alt: string; className?: string; children: ReactNode }) {
  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      viewBox="0 0 640 336"
      role="img"
      aria-label={alt}
      className={cn("block h-full w-full", className)}
      fill="none"
      strokeWidth={2.5}
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      <title>{alt}</title>
      <rect width="640" height="336" className="fill-card" />
      <circle cx="560" cy="30" r="170" className="fill-primary" opacity=".08" />
      <circle cx="40" cy="320" r="110" className="fill-primary" opacity=".05" />
      {children}
    </svg>
  );
}

/** A laptop and a cloud; only a short message crosses between them. */
function LocalFirst() {
  return (
    <>
      <rect x="64" y="84" width="270" height="168" rx="14" className="fill-secondary stroke-border" />
      <path d={bars(88, 310, 168, 44, 3, 8, (t) => Math.sin(t * Math.PI) ** 0.8)} className="stroke-primary" />
      <path d="M40 270h318" className="stroke-border" />
      <path d="M334 168h122" className="stroke-muted-foreground" strokeDasharray="3 9" />
      <rect x="360" y="126" width="72" height="34" rx="9" className="fill-card stroke-muted-foreground" />
      <path d="M374 138h34M374 148h22" className="stroke-muted-foreground" />
      <path d="M470 196a24 24 0 0 1 4-48 30 30 0 0 1 56-8 24 24 0 0 1 22 56z" className="stroke-muted-foreground" />
    </>
  );
}

/** One mix splitting into four lanes. */
function Stems() {
  const lanes = [64, 136, 208, 280];
  return (
    <>
      <path d={bars(40, 212, 168, 54, 11, 7, (t) => 0.5 + 0.5 * Math.sin(t * 9))} className="stroke-primary" />
      {lanes.map((y) => (
        <path
          key={y}
          d={`M222 168C282 168 292 ${y} 352 ${y}`}
          className="stroke-muted-foreground"
          opacity=".55"
        />
      ))}
      {lanes.map((y, i) => (
        <path key={`w${y}`} d={bars(360, 600, y, 18, 20 + i, 7, () => 0.4 + 0.6 * rand(i, 2))} className="stroke-primary" opacity={1 - i * 0.18} />
      ))}
    </>
  );
}

/** Speech in bursts, the silences between them marked for trimming. */
function Podcast() {
  const bursts: [number, number][] = [
    [48, 150],
    [206, 300],
    [350, 440],
    [496, 596],
  ];
  return (
    <>
      <path d="M32 168h576" className="stroke-border" />
      {bursts.map(([a, b], i) => (
        <path key={a} d={bars(a, b, 168, 52, 40 + i, 7, (t) => Math.sin(t * Math.PI) ** 0.6)} className="stroke-primary" />
      ))}
      {[
        [158, 198],
        [308, 342],
        [448, 488],
      ].map(([a, b]) => (
        <g key={a} className="stroke-muted-foreground">
          <path d={`M${a} 100v136M${b} 100v136`} strokeDasharray="4 7" />
          <path d={`M${a + 8} 168h${b - a - 16}`} strokeDasharray="2 7" />
        </g>
      ))}
      <circle cx="320" cy="60" r="10" className="fill-primary stroke-primary" />
      <path d="M320 70v18" className="stroke-primary" />
    </>
  );
}

/** A chat bubble feeding three track lanes. */
function Prompt() {
  return (
    <>
      <rect x="44" y="64" width="210" height="130" rx="16" className="fill-secondary stroke-border" />
      <path d="M72 100h136M72 126h104M72 152h58" className="stroke-muted-foreground" />
      <path d="M142 146v14" className="stroke-primary" strokeWidth={3.5} />
      <path d="M96 194l-18 28 44-28" className="fill-secondary stroke-border" />
      <path d="M264 130C312 130 322 80 364 80M264 130H364M264 130C312 130 322 190 364 190" className="stroke-primary" opacity=".7" />
      {[80, 130, 190].map((y, i) => (
        <g key={y}>
          <rect x="372" y={y - 26} width="224" height="52" rx="10" className="fill-secondary stroke-border" />
          <path d={bars(386, 582, y, 15, 60 + i, 7, () => 0.5 + 0.5 * rand(i, 5))} className="stroke-primary" />
        </g>
      ))}
    </>
  );
}

/** A key going into a socket on the user's side of the line. */
function BringYourKey() {
  return (
    <>
      <circle cx="116" cy="168" r="38" className="stroke-primary" />
      <circle cx="116" cy="168" r="12" className="stroke-primary" />
      <path d="M154 168h132M236 168v26M262 168v18" className="stroke-primary" />
      <rect x="298" y="120" width="92" height="96" rx="14" className="fill-secondary stroke-border" />
      <path d="M298 168h-12" className="stroke-primary" />
      <circle cx="344" cy="148" r="6" className="stroke-muted-foreground" />
      <circle cx="344" cy="188" r="6" className="stroke-muted-foreground" />
      <path d="M390 168h60" className="stroke-muted-foreground" />
      <rect x="450" y="92" width="144" height="152" rx="14" className="fill-secondary stroke-border" />
      <path d={bars(468, 576, 168, 38, 71, 8, (t) => Math.sin(t * Math.PI) ** 0.7)} className="stroke-primary" />
    </>
  );
}

/** Two tracks on one beat grid, overlapping and crossfading. */
function Beatmatch() {
  const grid = Array.from({ length: 15 }, (_, i) => 36 + i * 40);
  return (
    <>
      <rect x="248" y="40" width="176" height="256" rx="10" className="fill-primary" opacity=".1" />
      <path d={grid.map((x) => `M${x} 36V300`).join("")} className="stroke-border" strokeDasharray="2 8" />
      <path d={bars(36, 424, 112, 40, 5, 8, (t) => 1 - 0.55 * Math.max(0, t - 0.55) / 0.45)} className="stroke-muted-foreground" />
      <path d={bars(248, 604, 224, 40, 9, 8, (t) => 0.45 + 0.55 * Math.min(1, t * 2.5))} className="stroke-primary" />
      <path d="M248 74L424 150" className="stroke-muted-foreground" />
      <path d="M248 262L424 186" className="stroke-primary" />
    </>
  );
}

/** A drums-only bar repeated, the first repeats ramping in. */
function ExtendedIntro() {
  const w = 134;
  const xs = [36, 36 + (w + 10), 36 + 2 * (w + 10), 36 + 3 * (w + 10)];
  return (
    <>
      {xs.map((x, i) => (
        <g key={x}>
          <rect x={x} y="104" width={w} height="118" rx="10" className="fill-secondary stroke-border" />
          {[0, 1, 2, 3].map((b) => (
            <g key={b}>
              <circle cx={x + 22 + b * 30} cy={190} r="7" className="fill-primary stroke-primary" opacity={i < 2 ? 0.4 + 0.2 * i : 1} />
              <path d={`M${x + 37 + b * 30} 150v14`} className="stroke-muted-foreground" />
            </g>
          ))}
        </g>
      ))}
      <path d="M36 92L330 40" className="stroke-primary" strokeDasharray="3 8" />
      <path d={`M${xs[3] + w - 10} 250C${xs[3] + w + 30} 290 ${xs[0] - 30} 290 ${xs[0] + 10} 250`} className="stroke-muted-foreground" />
      <path d={`M${xs[0] + 10} 250l12-9M${xs[0] + 10} 250l14 7`} className="stroke-muted-foreground" />
    </>
  );
}

/** One agent, six providers on a ring, the local one drawn as a laptop. */
function Providers() {
  const cx = 320;
  const cy = 168;
  const r = 112;
  const pts = Array.from({ length: 6 }, (_, i) => {
    const a = -Math.PI / 2 + (i * Math.PI) / 3;
    return [n1(cx + r * 1.45 * Math.cos(a)), n1(cy + r * Math.sin(a))] as const;
  });
  return (
    <>
      {pts.map(([x, y], i) => (
        <path key={`s${i}`} d={`M${cx} ${cy}L${x} ${y}`} className="stroke-muted-foreground" strokeDasharray={i === 5 ? "3 8" : undefined} opacity=".7" />
      ))}
      <circle cx={cx} cy={cy} r="40" className="fill-secondary stroke-primary" />
      <path d={bars(300, 340, cy, 14, 2, 6, () => 1)} className="stroke-primary" />
      {pts.map(([x, y], i) =>
        i === 5 ? (
          <g key="local" className="stroke-primary">
            <rect x={x - 26} y={y - 18} width="52" height="34" rx="5" className="fill-card" />
            <path d={`M${x - 36} ${y + 24}h72`} />
          </g>
        ) : (
          <circle key={`n${i}`} cx={x} cy={y} r="20" className="fill-card stroke-muted-foreground" />
        ),
      )}
      <circle cx={pts[0][0]} cy={pts[0][1]} r="20" className="fill-primary stroke-primary" opacity=".85" />
    </>
  );
}

/** A graph of edits: a trunk, two branches, the head marked, A and B on two nodes. */
function UndoGraph() {
  const trunk: [number, number][] = [
    [64, 168],
    [154, 168],
    [244, 168],
  ];
  const up: [number, number][] = [
    [244, 168],
    [352, 96],
    [460, 96],
    [568, 96],
  ];
  const down: [number, number][] = [
    [244, 168],
    [352, 240],
    [460, 240],
  ];
  const line = (pts: [number, number][]) =>
    pts
      .slice(1)
      .map(([x, y], i) => {
        const [px, py] = pts[i];
        return `M${px} ${py}C${n1(px + (x - px) / 2)} ${py} ${n1(px + (x - px) / 2)} ${y} ${x} ${y}`;
      })
      .join("");
  return (
    <>
      <path d={line(trunk) + line(up) + line(down)} className="stroke-muted-foreground" opacity=".8" />
      {[...trunk, ...up.slice(1), ...down.slice(1)].map(([x, y]) => (
        <circle key={`${x}-${y}`} cx={x} cy={y} r="14" className="fill-card stroke-muted-foreground" />
      ))}
      <circle cx="568" cy="96" r="18" className="fill-primary stroke-primary" />
      <circle cx="568" cy="96" r="28" className="stroke-primary" opacity=".35" />
      <g className="fill-foreground" stroke="none" fontSize="15" fontWeight="700" fontFamily="ui-sans-serif, system-ui, sans-serif" textAnchor="middle">
        <text x="460" y="101">A</text>
        <text x="460" y="245">B</text>
      </g>
      <path d="M460 126v84" className="stroke-primary" strokeDasharray="3 7" />
    </>
  );
}

const COVERS: Record<string, () => ReactNode> = {
  "ai-audio-editing-local-first": LocalFirst,
  "stem-separation-explained-demucs": Stems,
  "podcast-production-ai-workflow": Podcast,
  "conversational-daw-prompt-to-mix": Prompt,
  "open-source-audio-editor-byo-llm": BringYourKey,
  "beatmatch-and-blend-two-tracks": Beatmatch,
  "make-a-mixable-extended-intro": ExtendedIntro,
  "which-llm-should-drive-your-audio-editor": Providers,
  "undo-is-a-graph-branches-and-ab-compare": UndoGraph,
};

/** Slugs that have a cover of their own, for the test that holds every post to one. */
export const coverSlugs: readonly string[] = Object.keys(COVERS);

export function hasCover(slug: string): boolean {
  return slug in COVERS;
}

/** A plain waveform, for a post nobody has drawn a cover for yet. */
function Fallback() {
  return <path d={bars(40, 600, 168, 70, 1, 8, (t) => Math.sin(t * Math.PI) ** 0.7)} className="stroke-primary" />;
}

export function BlogCover({ slug, alt, className }: { slug: string; alt: string; className?: string }) {
  const Motif = COVERS[slug] ?? Fallback;
  return (
    <Cover alt={alt} className={className}>
      <Motif />
    </Cover>
  );
}
